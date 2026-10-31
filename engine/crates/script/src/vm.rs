//! The sandboxed Luau state every script call runs in. Carried over from the slice's
//! `vm.rs` (its sandbox, portable maths, `require` and budgets), without its beats and its
//! typed world.
//!
//! - Only `string`, `table`, `math` (without `random`), `bit32`, `utf8`, `buffer` and
//!   `vector` are loaded; `loadstring`, `getfenv`, `setfenv`, `collectgarbage` and
//!   `gcinfo` are removed. There is no `os`, `io` or `debug`.
//! - Sandbox mode freezes the libraries and globals; each chunk runs in its own
//!   read-only environment, so scripts can't stash state in globals.
//! - `require` sees the module's own `.luau` files (`./name`, `../dir/name`), from the
//!   in-memory copy of the module: exactly the files that were loaded and recorded.
//! - **Each call runs in a fresh Luau state**, with the collector stopped, and may
//!   allocate [`Limits::memory_bytes`] (garbage included). Luau's allocator hands out whole
//!   pages, so in a reused state how close a call gets to the limit would depend on what
//!   earlier calls left; a fresh state makes the same allocations every time. The module's
//!   bytecode is compiled once and shared by its fresh states.
//! - **No memory between calls.** Every call runs the script and the modules it requires
//!   afresh, so locals and upvalues carry nothing from one call to the next: the world is
//!   a script's only memory, and a restored save or a replay runs exactly the same.
//! - A VM interrupt counts steps (loop back-edges and calls) per call, and native helpers
//!   charge steps for the work they do ([`crate::meter`]); over the budget, every further
//!   step fails, so even `pcall` can't keep a loop alive.
//! - **No addresses.** `tostring`, `string.format` and `print` show tables, functions and
//!   buffers as their type name; `pairs`, `next` and `for k, v in t do` refuse tables keyed
//!   by them (their order follows heap addresses; see [`crate::iteration`]).
//!   `table.foreach` is removed.
//! - **Same bits on every OS.** `math` and `vector` functions that call the platform's C
//!   library (`sin`, `pow`, `log` ...) or whose C++ arithmetic a compiler may fuse
//!   (`math.lerp`, `vector.dot` ...) are replaced with `libm` and plain-Rust versions, and
//!   the compiler is told they aren't builtins ([`PORTABLE_BUILTINS`]), so neither a fast
//!   call nor constant folding reaches the C versions. `math.noise` is removed.
//! - Text a script makes that leaves the VM (abort messages, `print` lines) is cut to
//!   [`TEXT_CAP`] bytes.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use galaxy_module::module::ModuleFiles;
use mlua::{
    Function, Lua, LuaOptions, LuaString, MultiValue, StdLib, Table, Value, Variadic, VmState,
};

use crate::iteration;
use crate::meter::{self, Budget};

/// What one call may use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    /// VM steps (loop back-edges and calls) plus what native helpers charge.
    pub steps_per_call: u64,
    /// Bytes the call may allocate, garbage included.
    pub memory_bytes: u64,
    /// Commands one call may write.
    pub commands_per_call: usize,
}

/// The limits every call runs under.
pub const LIMITS: Limits = Limits {
    steps_per_call: 200_000,
    memory_bytes: 8 * 1024 * 1024,
    commands_per_call: 1_000,
};

/// A place in a module file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: Option<u32>,
}

impl std::fmt::Display for Location {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.line {
            Some(line) => write!(f, "{}:{line}", self.file),
            None => write!(f, "{}", self.file),
        }
    }
}

/// Why a call was aborted. Its writes were discarded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Abort {
    /// `over_budget`, `out_of_memory`, `over_quota`, `bad_command`, `runtime_error`,
    /// `load_error` or `host_error`.
    pub code: String,
    pub message: String,
    pub location: Option<Location>,
}

impl std::fmt::Display for Abort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.location {
            Some(at) => write!(f, "{at}: {} ({})", self.message, self.code),
            None => write!(f, "{} ({})", self.message, self.code),
        }
    }
}

/// Lines `print` keeps per call.
const LOG_CAP: usize = 200;
/// Longest script-made text kept outside the VM, in bytes, marker included.
pub const TEXT_CAP: usize = 1024;
/// Ends text that was cut to [`TEXT_CAP`].
pub const CUT_MARKER: &str = " [... cut]";

/// `text`, cut to at most [`TEXT_CAP`] bytes (at a character boundary, with
/// [`CUT_MARKER`]) if it's longer.
pub fn clip_text(text: &str) -> String {
    clip_bytes(text.as_bytes())
}

/// Like [`clip_text`] for bytes that may not be UTF-8.
pub(crate) fn clip_bytes(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(TEXT_CAP + 1)]);
    if bytes.len() <= TEXT_CAP && text.len() <= TEXT_CAP {
        return text.into_owned();
    }
    let mut keep = TEXT_CAP - CUT_MARKER.len();
    while !text.is_char_boundary(keep) {
        keep -= 1;
    }
    format!("{}{CUT_MARKER}", &text[..keep])
}

/// Values that are only their identity: printing or hashing one exposes an address.
pub(crate) fn by_reference(v: &Value) -> bool {
    !matches!(
        v,
        Value::Nil
            | Value::Boolean(_)
            | Value::Integer(_)
            | Value::Number(_)
            | Value::Vector(_)
            | Value::String(_)
    )
}

/// A Luau number that is a whole number Rust can hold exactly (|n| <= 2^53 for
/// floats). Anything else is `None`: callers reject it instead of truncating.
pub(crate) fn whole_number(v: &Value) -> Option<i64> {
    const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
    match v {
        Value::Integer(i) => Some(*i),
        Value::Number(n) if n.fract() == 0.0 && n.abs() <= EXACT => Some(*n as i64),
        _ => None,
    }
}

/// `tostring` without addresses.
fn stable_tostring(lua: &Lua, raw: &Function, v: Value) -> mlua::Result<LuaString> {
    let own = match &v {
        Value::Table(t) => t
            .metatable()
            .is_some_and(|mt| matches!(mt.raw_get::<Value>("__tostring"), Ok(Value::Function(_)))),
        other => !by_reference(other),
    };
    if own {
        raw.call(v)
    } else {
        lua.create_string(v.type_name())
    }
}

/// The `math` and `vector` functions [`install_portable_math`] replaces or removes. The
/// compiler must not treat them as builtins: a builtin runs Luau's own C++ version
/// through a fast call whatever the table holds, and `--!optimize 2` folds constant
/// calls with the host's C library at compile time.
pub const PORTABLE_BUILTINS: &[&str] = &[
    "math.sin",
    "math.cos",
    "math.tan",
    "math.asin",
    "math.acos",
    "math.atan",
    "math.atan2",
    "math.sinh",
    "math.cosh",
    "math.tanh",
    "math.exp",
    "math.log",
    "math.log10",
    "math.pow",
    "math.map",
    "math.lerp",
    "math.noise",
    "vector.magnitude",
    "vector.normalize",
    "vector.cross",
    "vector.dot",
    "vector.angle",
    "vector.lerp",
];

/// The Luau compiler every module chunk goes through (see [`PORTABLE_BUILTINS`]).
fn compiler() -> mlua::chunk::Compiler {
    mlua::chunk::Compiler::new().set_disabled_builtins(PORTABLE_BUILTINS.iter().copied())
}

/// Backs `math` and `vector` with maths that gives the same bits on every OS.
///
/// The replacements use `libm` and plain Rust (which never fuses), with Luau's operation
/// order. Exact operations stay native: `sqrt`, `floor`, `ceil`, `round`, `abs`, `fmod`,
/// `modf`, `frexp`, `ldexp`, `min`, `max`, `clamp`, `sign`, `deg`, `rad`.
fn install_portable_math(lua: &Lua, globals: &Table) -> mlua::Result<()> {
    type Unary = fn(f64) -> f64;
    let math: Table = globals.get("math")?;
    let unary: [(&str, Unary); 11] = [
        ("sin", libm::sin),
        ("cos", libm::cos),
        ("tan", libm::tan),
        ("asin", libm::asin),
        ("acos", libm::acos),
        ("atan", libm::atan),
        ("sinh", libm::sinh),
        ("cosh", libm::cosh),
        ("tanh", libm::tanh),
        ("exp", libm::exp),
        ("log10", libm::log10),
    ];
    for (name, f) in unary {
        math.raw_set(name, lua.create_function(move |_, x: f64| Ok(f(x)))?)?;
    }
    math.raw_set(
        "atan2",
        lua.create_function(|_, (y, x): (f64, f64)| Ok(libm::atan2(y, x)))?,
    )?;
    math.raw_set(
        "pow",
        lua.create_function(|_, (x, y): (f64, f64)| Ok(libm::pow(x, y)))?,
    )?;
    math.raw_set(
        "log",
        lua.create_function(|_, (x, base): (f64, Option<f64>)| {
            Ok(match base {
                None => libm::log(x),
                Some(2.0) => libm::log2(x),
                Some(10.0) => libm::log10(x),
                Some(b) => libm::log(x) / libm::log(b),
            })
        })?,
    )?;
    math.raw_set(
        "map",
        lua.create_function(
            |_, (x, in_min, in_max, out_min, out_max): (f64, f64, f64, f64, f64)| {
                Ok(out_min + (x - in_min) * (out_max - out_min) / (in_max - in_min))
            },
        )?,
    )?;
    math.raw_set(
        "lerp",
        lua.create_function(|_, (a, b, t): (f64, f64, f64)| Ok(lerp(a, b, t)))?,
    )?;
    math.raw_set("noise", Value::Nil)?;

    use mlua::Vector;
    let v3 = |v: Vector| [v.x(), v.y(), v.z()];
    let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let cross = |a: [f32; 3], b: [f32; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    let vector: Table = globals.get("vector")?;
    vector.raw_set(
        "magnitude",
        lua.create_function(move |_, v: Vector| {
            let v = v3(v);
            Ok(f64::from(dot(v, v).sqrt()))
        })?,
    )?;
    vector.raw_set(
        "normalize",
        lua.create_function(move |_, v: Vector| {
            let v = v3(v);
            let inv = 1.0 / dot(v, v).sqrt();
            Ok(Vector::new(v[0] * inv, v[1] * inv, v[2] * inv))
        })?,
    )?;
    vector.raw_set(
        "cross",
        lua.create_function(move |_, (a, b): (Vector, Vector)| {
            let c = cross(v3(a), v3(b));
            Ok(Vector::new(c[0], c[1], c[2]))
        })?,
    )?;
    vector.raw_set(
        "dot",
        lua.create_function(move |_, (a, b): (Vector, Vector)| Ok(f64::from(dot(v3(a), v3(b)))))?,
    )?;
    vector.raw_set(
        "angle",
        lua.create_function(move |_, (a, b, axis): (Vector, Vector, Option<Vector>)| {
            let (a, b) = (v3(a), v3(b));
            let c = cross(a, b);
            let sin_a = f64::from(dot(c, c)).sqrt();
            let cos_a = f64::from(dot(a, b));
            let angle = libm::atan2(sin_a, cos_a);
            Ok(match axis {
                Some(axis) if dot(c, v3(axis)) < 0.0 => -angle,
                _ => angle,
            })
        })?,
    )?;
    vector.raw_set(
        "lerp",
        lua.create_function(move |_, (a, b, t): (Vector, Vector, f64)| {
            let (a, b, t) = (v3(a), v3(b), t as f32);
            let l = |a: f32, b: f32| if t == 1.0 { b } else { a + (b - a) * t };
            Ok(Vector::new(l(a[0], b[0]), l(a[1], b[1]), l(a[2], b[2])))
        })?,
    )?;
    Ok(())
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    if t == 1.0 { b } else { a + (b - a) * t }
}

/// Replaces the standard functions that can expose heap addresses or address order.
/// Runs before the sandbox freezes the libraries.
fn install_stable_library(lua: &Lua, globals: &Table) -> mlua::Result<()> {
    let raw_tostring: Function = globals.get("tostring")?;
    let raw = raw_tostring.clone();
    let tostring = lua.create_function(move |lua, v: Value| stable_tostring(lua, &raw, v))?;
    globals.raw_set("tostring", tostring)?;

    let string: Table = globals.get("string")?;
    let raw_format: Function = string.get("format")?;
    let raw = raw_tostring.clone();
    let format = lua.create_function(move |lua, args: Variadic<Value>| {
        let args = args
            .into_iter()
            .map(|v| {
                if by_reference(&v) {
                    stable_tostring(lua, &raw, v).map(Value::String)
                } else {
                    Ok(v)
                }
            })
            .collect::<mlua::Result<Variadic<Value>>>()?;
        raw_format.call::<MultiValue>(args)
    })?;
    string.raw_set("format", format)?;

    let table: Table = globals.get("table")?;
    table.raw_set("foreach", Value::Nil)?;

    lua.set_named_registry_value("galaxy.tostring", raw_tostring)?;
    Ok(())
}

/// A `(file, line)` from Luau's `file:line:` error prefix.
pub(crate) fn split_location(msg: &str) -> (Option<Location>, String) {
    let mut parts = msg.splitn(3, ':');
    if let (Some(file), Some(line), Some(rest)) = (parts.next(), parts.next(), parts.next())
        && file.ends_with(".luau")
        && let Ok(line) = line.trim().parse::<u32>()
    {
        let loc = Location {
            file: file.trim().to_owned(),
            line: Some(line),
        };
        return (Some(loc), rest.trim().to_owned());
    }
    (None, msg.to_owned())
}

/// The innermost message of an mlua error, and whether it was out of memory.
fn root_message(e: &mlua::Error) -> (bool, String) {
    match e {
        mlua::Error::CallbackError { cause, .. } => root_message(cause),
        mlua::Error::MemoryError(m) => (true, m.clone()),
        mlua::Error::RuntimeError(m) => (false, m.clone()),
        mlua::Error::SyntaxError { message, .. } => (false, message.clone()),
        mlua::Error::WithContext { cause, .. } => root_message(cause),
        other => (false, other.to_string()),
    }
}

/// Resolves `./x` or `../dir/x` against the requiring file's folder, inside the module.
fn resolve(from: &str, name: &str) -> Option<String> {
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop(); // the file itself
    for seg in name.split('/') {
        match seg {
            "." | "" => {}
            ".." => {
                parts.pop()?;
            }
            s => parts.push(s),
        }
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("{}.luau", parts.join("/")))
}

/// The `@file` chunk name and line of the Lua function at stack `level`.
pub(crate) fn caller_location(lua: &Lua, level: usize) -> Option<Location> {
    lua.inspect_stack(level, |d| {
        let src = d.source().source.map(|s| s.into_owned())?;
        let file = src.strip_prefix('@')?.to_owned();
        Some(Location {
            file,
            line: d.current_line().and_then(|l| u32::try_from(l).ok()),
        })
    })
    .flatten()
}

/// The first Luau caller on the stack: where a ctx function was called from.
pub(crate) fn script_location(lua: &Lua) -> Option<Location> {
    (0..4).find_map(|level| caller_location(lua, level))
}

/// Bytecode for a module's sources, compiled once and shared by every fresh state of the
/// module. Keyed by the source text, so a file always runs the code it was loaded as.
#[derive(Clone, Default)]
pub struct Code(Rc<RefCell<BTreeMap<String, Rc<[u8]>>>>);

impl Code {
    /// The bytecode of `src` with every generic `for` loop's iterator wrapped
    /// ([`crate::iteration`]). `Ok(None)` if `src` isn't valid Luau: loading the source then
    /// reports Luau's own error. Fails, at the loop's line in `path`, if a loop can't be
    /// wrapped.
    fn of(&self, path: &str, src: &str) -> mlua::Result<Option<Rc<[u8]>>> {
        if let Some(b) = self.0.borrow().get(src) {
            return Ok(Some(b.clone()));
        }
        let compiler = compiler();
        let compiles = |s: &str| compiler.compile(s).is_ok();
        let wrapped = iteration::loops(src).and_then(|loops| {
            compiler
                .compile(iteration::wrap(src, &loops))
                .map_err(|_| iteration::culprit(src, &loops, compiles))
        });
        match wrapped {
            Ok(b) => {
                let b: Rc<[u8]> = b.into();
                self.0.borrow_mut().insert(src.to_owned(), b.clone());
                Ok(Some(b))
            }
            Err(_) if !compiles(src) => Ok(None),
            Err(p) => Err(mlua::Error::runtime(format!(
                "{path}:{}: {}",
                p.line, p.message
            ))),
        }
    }
}

/// A fresh sandboxed state for one call of a module's scripts.
pub(crate) struct Vm {
    pub(crate) lua: Lua,
    pub(crate) budget: Rc<Budget>,
    pub(crate) log: Rc<RefCell<Vec<String>>>,
    files: Rc<ModuleFiles>,
    limits: Limits,
}

impl Vm {
    /// A state with the sandbox and `require` over `files`, no script loaded.
    pub(crate) fn new(files: Rc<ModuleFiles>, code: &Code, limits: Limits) -> mlua::Result<Vm> {
        let libs = StdLib::TABLE
            | StdLib::STRING
            | StdLib::UTF8
            | StdLib::BIT
            | StdLib::MATH
            | StdLib::BUFFER
            | StdLib::VECTOR;
        let lua = Lua::new_with(libs, LuaOptions::default())?;
        lua.set_compiler(compiler());
        lua.set_memory_limit(usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX))?;

        let budget = Rc::new(Budget::default());
        budget.reset(limits.steps_per_call);
        let b = budget.clone();
        lua.set_interrupt(move |lua| {
            b.tick(lua)?;
            Ok(VmState::Continue)
        });

        let globals = lua.globals();
        for name in [
            "loadstring",
            "getfenv",
            "setfenv",
            "collectgarbage",
            "gcinfo",
        ] {
            globals.raw_set(name, Value::Nil)?;
        }
        let math: Table = globals.get("math")?;
        math.raw_set("random", Value::Nil)?;
        math.raw_set("randomseed", Value::Nil)?;
        install_portable_math(&lua, &globals)?;

        let log = Rc::new(RefCell::new(Vec::new()));
        let sink = log.clone();
        let print = lua.create_function(move |lua, args: Variadic<Value>| {
            let raw: Function = lua.named_registry_value("galaxy.tostring")?;
            let mut line: Vec<u8> = Vec::new();
            for (i, a) in args.into_iter().enumerate() {
                if i > 0 {
                    line.push(b'\t');
                }
                let s = stable_tostring(lua, &raw, a)?;
                let bytes = s.as_bytes();
                let room = (TEXT_CAP + 1).saturating_sub(line.len());
                line.extend_from_slice(&bytes[..bytes.len().min(room)]);
                if line.len() > TEXT_CAP {
                    break;
                }
            }
            let mut l = sink.borrow_mut();
            if l.len() < LOG_CAP {
                l.push(clip_bytes(&line));
            }
            Ok(())
        })?;
        globals.raw_set("print", print)?;

        lua.set_app_data(code.clone());
        lua.set_named_registry_value("galaxy.modules", lua.create_table()?)?;
        lua.set_named_registry_value("galaxy.loading", lua.create_table()?)?;

        let sources = files.clone();
        let require = lua.create_function(move |lua, name: String| -> mlua::Result<Value> {
            let from = caller_location(lua, 1).map(|l| l.file).unwrap_or_default();
            let path = if name.starts_with("./") || name.starts_with("../") {
                resolve(&from, &name)
            } else {
                None
            }
            .ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "require(\"{name}\"): only the module's own files (`./name`, \
                     `../dir/name`) can be required"
                ))
            })?;
            let modules: Table = lua.named_registry_value("galaxy.modules")?;
            if let Some(v) = modules.raw_get::<Option<Value>>(path.as_str())? {
                return Ok(v);
            }
            let loading: Table = lua.named_registry_value("galaxy.loading")?;
            if loading.raw_get::<bool>(path.as_str()).unwrap_or(false) {
                return Err(mlua::Error::runtime(format!(
                    "require(\"{name}\"): `{path}` requires itself"
                )));
            }
            let src = sources.get(&path).ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "require(\"{name}\"): no file `{path}` in this module"
                ))
            })?;
            loading.raw_set(path.as_str(), true)?;
            let result = load_chunk(lua, &path, src).and_then(|f| f.call::<Value>(()));
            loading.raw_set(path.as_str(), Value::Nil)?;
            let v = result?;
            modules.raw_set(path.as_str(), v.clone())?;
            Ok(v)
        })?;
        globals.raw_set("require", require)?;
        install_stable_library(&lua, &globals)?;
        meter::install_iteration(&lua, &globals, &budget)?;
        meter::install(&lua, &globals, &budget)?;
        // Chunks read these (frozen by sandbox mode), never the writable per-thread proxy
        // sandbox mode installs, so no script can stash state in a global.
        lua.set_named_registry_value("galaxy.globals", globals)?;

        lua.sandbox(true)?;
        Ok(Vm {
            lua,
            budget,
            log,
            files,
            limits,
        })
    }

    /// Runs a script file and returns what it returns.
    pub(crate) fn run_file(&self, file: &str) -> mlua::Result<Value> {
        let src = self
            .files
            .get(file)
            .ok_or_else(|| mlua::Error::runtime(format!("`{file}` is not in the module")))?;
        load_chunk(&self.lua, file, src)?.call::<Value>(())
    }

    /// Runs `f` with the step budget and memory allowance. Returns an abort if it ran over
    /// either, or if the host panicked.
    ///
    /// The heap is fully collected first and the collector stays off while `f` runs, so
    /// the call may allocate exactly the limit, garbage included.
    pub(crate) fn budgeted<R>(&self, f: impl FnOnce() -> mlua::Result<R>) -> Result<R, Abort> {
        self.budget.reset(self.limits.steps_per_call);
        let lua = &self.lua;
        let memory = usize::try_from(self.limits.memory_bytes).unwrap_or(usize::MAX);
        let prepared = lua
            .set_memory_limit(0)
            .and_then(|_| lua.gc_collect())
            .and_then(|()| lua.set_memory_limit(lua.used_memory().saturating_add(memory)));
        if let Err(e) = prepared {
            return Err(abort_from(&e, Some("host_error")));
        }
        lua.gc_stop();
        let result = catch_unwind(AssertUnwindSafe(f));
        lua.gc_restart();
        let result = match result {
            Ok(r) => r,
            Err(panic) => {
                let what = panic
                    .downcast_ref::<&str>()
                    .map(|s| (*s).to_owned())
                    .or_else(|| panic.downcast_ref::<String>().cloned())
                    .unwrap_or_default();
                return Err(Abort {
                    code: "host_error".into(),
                    message: format!("the host failed on this call ({what})"),
                    location: self.budget.at.borrow().clone(),
                });
            }
        };
        if self.budget.exhausted.get() {
            return Err(Abort {
                code: "over_budget".into(),
                message: format!(
                    "ran over its budget of {} steps (an endless loop?)",
                    self.limits.steps_per_call
                ),
                location: self.budget.at.borrow().clone(),
            });
        }
        result.map_err(|e| abort_from(&e, None))
    }
}

/// Loads a module file (its cached bytecode) as a chunk with its own read-only
/// environment.
fn load_chunk(lua: &Lua, path: &str, src: &str) -> mlua::Result<Function> {
    let env = lua.create_table()?;
    let mt = lua.create_table()?;
    mt.raw_set(
        "__index",
        lua.named_registry_value::<Table>("galaxy.globals")?,
    )?;
    mt.raw_set("__metatable", false)?;
    env.set_metatable(Some(mt))?;
    env.set_readonly(true);
    let code = match lua.app_data_ref::<Code>() {
        Some(c) => c.of(path, src)?,
        None => None,
    };
    let chunk = match &code {
        Some(b) => lua.load(&b[..]),
        None => lua.load(src),
    };
    chunk
        .set_name(format!("@{path}"))
        .set_environment(env)
        .into_function()
}

/// Replaces `0x` followed by eight or more hex digits (a heap address in a Luau
/// message), so abort messages, which reach history, are the same on every run.
fn without_addresses(msg: &str) -> String {
    let b = msg.as_bytes();
    let mut out = String::with_capacity(msg.len());
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"0x") {
            let digits = b[i + 2..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if digits >= 8 {
                out.push_str("0x?");
                i += 2 + digits;
                continue;
            }
        }
        let ch = msg[i..].chars().next().unwrap_or('?');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// An abort from an mlua error: `code`, or `runtime_error`.
pub(crate) fn abort_from(e: &mlua::Error, code: Option<&str>) -> Abort {
    let (memory, msg) = root_message(e);
    let msg = without_addresses(&msg);
    // Drop Luau's stack traceback: the location prefix says where.
    let msg = msg
        .split("\nstack traceback:")
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if memory {
        return Abort {
            code: "out_of_memory".into(),
            message: clip_text(&format!("ran out of memory ({msg})")),
            location: None,
        };
    }
    let (location, message) = split_location(&msg);
    Abort {
        code: code.unwrap_or("runtime_error").to_owned(),
        message: clip_text(&message),
        location,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inside_the_module() {
        assert_eq!(
            resolve("scripts/a.luau", "./ledger").as_deref(),
            Some("scripts/ledger.luau")
        );
        assert_eq!(
            resolve("tests/t.luau", "../scripts/ledger").as_deref(),
            Some("scripts/ledger.luau")
        );
        assert_eq!(resolve("scripts/a.luau", "../../x"), None);
    }

    #[test]
    fn strips_addresses() {
        assert_eq!(
            without_addresses("bad table: 0x000001d2c3b4a590 here, 0x1f stays"),
            "bad table: 0x? here, 0x1f stays"
        );
    }

    #[test]
    fn whole_numbers_are_exact_or_rejected() {
        assert_eq!(whole_number(&Value::Number(3.0)), Some(3));
        assert_eq!(whole_number(&Value::Number(2.5)), None);
        assert_eq!(whole_number(&Value::Number(1e300)), None);
        assert_eq!(whole_number(&Value::Number(f64::NAN)), None);
    }

    #[test]
    fn splits_luau_locations() {
        let (loc, msg) = split_location("scripts/a.luau:12: boom");
        assert_eq!(loc.unwrap().line, Some(12));
        assert_eq!(msg, "boom");
    }
}
