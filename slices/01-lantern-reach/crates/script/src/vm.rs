//! One sandboxed Luau state per pack.
//!
//! - Only `string`, `table`, `math` (without `random`), `bit32`, `utf8`, `buffer` and
//!   `vector` are loaded; `loadstring`, `getfenv`, `setfenv`, `collectgarbage` and
//!   `gcinfo` are removed. There is no `os`, `io` or `debug`.
//! - Sandbox mode freezes the libraries and globals; each chunk runs in its own
//!   read-only environment, so scripts can't stash state in globals.
//! - `require` sees `@galaxy` and the pack's own modules (`./name`, `../dir/name`)
//!   in its script roots ([`Pack::loadable`]), from the in-memory copy of the pack
//!   that was hashed and checked: exactly the files the checks covered.
//! - A VM interrupt counts steps (loop back-edges and calls) per handler call, and
//!   native helpers charge steps for the work they do ([`crate::meter`]); over the
//!   budget, every further step fails, so even `pcall` can't keep a loop alive.
//! - **Each handler call, beat callback and `G.test` predicate runs in a fresh Luau
//!   state**, with the collector stopped, and may allocate `limits.memory_bytes`
//!   (garbage included). Luau's allocator hands out whole pages, so in a reused state
//!   how close a call gets to the limit depends on which pages earlier calls left half
//!   full; a fresh state makes the same allocations every time. So whether a call runs out of memory, and what weak tables hold,
//!   depends only on the call itself. The pack's bytecode is compiled once and shared
//!   by its fresh states, so a fresh state costs little more than its libraries.
//! - Text a script makes that leaves the VM (abort messages, `print` lines) is cut to
//!   [`TEXT_CAP`] bytes with a marker, so it can't bloat the log, history or saves.
//! - **No memory between calls.** Every call runs the script and the modules it
//!   requires afresh in its fresh state, so locals and upvalues can't carry anything
//!   from one call to the next: the script's declared stores are its only memory, and
//!   a restored save replays exactly.
//! - **No addresses.** `tostring`, `string.format` (and so string interpolation) and
//!   `print` show tables, functions and buffers as their type name; `pairs` and `next`
//!   refuse tables keyed by them (their order follows heap addresses). `table.foreach`
//!   is removed. Bare `for k, v in t` loops are rejected by the static scan.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::Rc;

use galaxy_contract::events::{CounterKind, CounterScope, EventEnvelope, EventKind};
use galaxy_contract::ids::{BeatId, FactionId, FlagId, NpcId, RegionId, StationId, UpgradeId};
use galaxy_contract::pack::{Diagnostic, Location, ScriptDecl};
use galaxy_contract::prelude::CommandBatch;
use galaxy_contract::progress::{BeatDef, BeatKind, Requirement, Rewards, StepDef};
use galaxy_contract::world::WorldState;
use galaxy_core::History;
use galaxy_core::rng::{RngBank, SimRng};
use mlua::LuaSerdeExt;
use mlua::{
    Function, Lua, LuaOptions, LuaString, MultiValue, RegistryKey, StdLib, Table, Value, Variadic,
    VmState,
};

use crate::ctx::{self, Buffer, CallEnv};
use crate::meter::{self, Budget};
use crate::pack::{Pack, error};

/// Where a script's random numbers come from: the sim's saved streams in a tick, or a
/// throwaway bank in checks and tests.
pub trait RngSource {
    fn stream(&mut self, key: &str) -> &mut SimRng;
}

impl RngSource for galaxy_core::TickCtx<'_> {
    fn stream(&mut self, key: &str) -> &mut SimRng {
        self.rng_named(key)
    }
}

impl RngSource for RngBank {
    fn stream(&mut self, key: &str) -> &mut SimRng {
        RngBank::stream(self, key)
    }
}

/// Why a handler call was aborted. Its commands were discarded.
#[derive(Clone, Debug, PartialEq)]
pub struct Abort {
    /// `over_budget`, `out_of_memory`, `runtime_error`, or a ctx code such as
    /// `uncited_line` or `undeclared_store`.
    pub code: String,
    pub message: String,
    pub location: Option<Location>,
}

impl Abort {
    pub fn diagnostic(&self) -> Diagnostic {
        Diagnostic {
            severity: galaxy_contract::pack::Severity::Error,
            code: self.code.clone(),
            message: clip_text(&self.message),
            location: self.location.clone(),
        }
    }
}

/// `G.after { ... }`: an activity trigger.
#[derive(Clone, Debug, PartialEq)]
pub struct AfterSpec {
    pub counters: BTreeMap<CounterKind, u64>,
    pub scope: CounterScope,
    /// Fire again each time the counters grow by the same amount.
    pub every: bool,
}

impl AfterSpec {
    /// Due if every counter reached its threshold for the next firing.
    pub fn due(&self, world: &WorldState, fired: u64) -> bool {
        if fired > 0 && !self.every {
            return false;
        }
        let times = fired.saturating_add(1);
        self.counters
            .iter()
            .all(|(kind, at)| world.counters.get(*kind, &self.scope) >= at.saturating_mul(times))
    }
}

#[derive(Clone, Debug)]
pub struct TriggerDef {
    pub name: String,
    pub spec: AfterSpec,
}

/// A loaded script.
pub struct VmScript {
    pub decl: ScriptDecl,
    def: RegistryKey,
    pub events: BTreeSet<EventKind>,
    /// Sorted by name.
    pub triggers: Vec<TriggerDef>,
    /// Beats it registers (`G.beat`), in the order it lists them.
    pub beats: Vec<LoadedBeat>,
    /// Where each `G.test` predicate sits in the script's definition, by the index
    /// `Requirement::Test` holds. Called on a fresh instance, like every handler.
    tests: Vec<Vec<PathKey>>,
}

/// One step from a script's definition table down to a function.
#[derive(Clone, Debug, PartialEq)]
enum PathKey {
    Key(String),
    Index(i64),
}

/// A beat a script registers. Its callbacks are found by name on a fresh instance.
pub struct LoadedBeat {
    pub def: BeatDef,
    /// Its position in the script's `beats` list (from 1).
    index: i64,
    hooks: BTreeSet<&'static str>,
}

impl LoadedBeat {
    pub fn has_hook(&self, name: &str) -> bool {
        self.hooks.contains(name)
    }
}

/// The callbacks a beat may define, by transition.
pub const BEAT_HOOKS: [&str; 3] = ["on_start", "on_step", "on_complete"];

/// One Luau unit test: its name, and how it went.
pub type TestOutcome = (String, Result<(), Abort>);

/// What a handler call is for.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    Event(&'a EventEnvelope),
    Trigger { name: &'a str, fired: u64 },
}

/// The world a call reads.
#[derive(Clone, Copy)]
pub struct View<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
}

pub struct PackVm {
    lua: Lua,
    pub pack: Rc<Pack>,
    pub scripts: Vec<VmScript>,
    budget: Rc<Budget>,
    /// `print` output, newest last (dev only; capped).
    pub log: Rc<RefCell<Vec<String>>>,
    code: Code,
}

/// Bytecode for the pack's sources, compiled once and shared by every fresh state of
/// the pack: compiling is most of what a fresh state costs. Keyed by the source text,
/// so a file always runs the code it was checked as.
#[derive(Clone, Default)]
struct Code(Rc<RefCell<BTreeMap<String, Rc<[u8]>>>>);

impl Code {
    /// The bytecode for `src`, or `None` if it doesn't compile (loading the text then
    /// reports the error, exactly as before).
    fn of(&self, src: &str) -> Option<Rc<[u8]>> {
        if let Some(b) = self.0.borrow().get(src) {
            return Some(b.clone());
        }
        let b: Rc<[u8]> = mlua::chunk::Compiler::new().compile(src).ok()?.into();
        self.0.borrow_mut().insert(src.to_owned(), b.clone());
        Some(b)
    }
}

thread_local! {
    /// The prelude's bytecode, compiled once per thread.
    static PRELUDE_CODE: Rc<[u8]> = mlua::chunk::Compiler::new()
        .compile(PRELUDE)
        .expect("the prelude compiles")
        .into();
}

const PRELUDE: &str = r#"
local G = {}
function G.script(def)
	if type(def) ~= "table" then error("G.script expects a table", 2) end
	if type(def.id) ~= "string" then error("G.script: `id` must be a string", 2) end
	return def
end
function G.after(spec, run)
	if type(spec) ~= "table" then error("G.after expects a table of activity counters", 2) end
	if type(run) ~= "function" then error("G.after: the second argument must be a function", 2) end
	return { after = spec, run = run }
end
function G.beat(def)
	if type(def) ~= "table" then error("G.beat expects a table", 2) end
	if type(def.id) ~= "string" then error("G.beat: `id` must be a beat ID", 2) end
	return def
end
G.always = "always"
function G.all(list) return { all = list } end
function G.any(list) return { any = list } end
function G.never(r) return { ["not"] = r } end
function G.done(beat) return { beat_done = beat } end
function G.started(beat) return { beat_started = beat } end
function G.active(beat) return { beat_active = beat } end
function G.at_step(beat, step) return { beat_step = { beat = beat, at_least = step } } end
function G.counter(kind, at_least, scope)
	local c = { kind = kind, at_least = at_least }
	if scope then c.station = scope.station; c.region = scope.region end
	return { counter = c }
end
function G.fact(q, opts)
	opts = opts or {}
	return { facts = { query = q, at_least = opts.at_least or 1, since = opts.since or "ever" } }
end
function G.sum(q, at_least, opts)
	opts = opts or {}
	return { sum = { query = q, at_least = at_least, since = opts.since or "ever" } }
end
function G.flag(id) return { flag = id } end
function G.trait(id) return { ["trait"] = id } end
function G.fitted(id) return { fitted = id } end
function G.hull(id) return { hull = id } end
function G.owns(id) return { owns = id } end
function G.credits(n)
	if type(n) ~= "number" then error("G.credits expects a number of credits", 2) end
	return { credits = { at_least = math.round(n * 100) } }
end
function G.cargo(ware, at_least) return { cargo = { ware = ware, at_least = at_least } } end
function G.hold_free(at_least) return { hold_free = { at_least = at_least } } end
function G.docked_at(station) return { docked_at = station } end
function G.docked() return "docked" end
function G.in_region(region) return { in_region = region } end
function G.near(spot, within)
	if type(spot) ~= "table" or spot.region == nil then
		error("G.near expects a spot { x, y, region } and a distance", 2)
	end
	return { near = { region = spot.region, x = spot.x, y = spot.y, within = within } }
end
function G.regard(faction, tier) return { regard = { faction = faction, at_least = tier } } end
function G.tunable_is(id, value) return { tunable_is = { tunable = id, value = value } } end
function G.test(f)
	if type(f) ~= "function" then error("G.test expects a function(ctx) returning a boolean", 2) end
	return { test = f }
end
function G.tunable(id) return { tunable = id } end
return table.freeze(G)
"#;

const LOG_CAP: usize = 200;
/// Longest script-made text kept outside the VM, in bytes, marker included: abort
/// messages (which reach the host log and history) and `print` lines.
pub const TEXT_CAP: usize = 1024;
/// Ends text that was cut to [`TEXT_CAP`].
pub const CUT_MARKER: &str = " [... cut]";

/// `text`, cut to at most [`TEXT_CAP`] bytes (at a character boundary, with
/// [`CUT_MARKER`]) if it's longer.
pub fn clip_text(text: &str) -> String {
    clip_bytes(text.as_bytes())
}

/// Like [`clip_text`] for bytes that may not be UTF-8. Reads at most `TEXT_CAP + 1`
/// of them.
pub(crate) fn clip_bytes(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(&bytes[..bytes.len().min(TEXT_CAP + 1)]);
    // (Decoding can grow an invalid byte into a three-byte replacement character.)
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
        // Whole and within 2^53, so the cast is exact.
        Value::Number(n) if n.fract() == 0.0 && n.abs() <= EXACT => Some(*n as i64),
        _ => None,
    }
}

/// `tostring` without addresses: a table with a `__tostring` metamethod uses it; other
/// tables, functions, buffers and userdata print as their type name.
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
            column: None,
        };
        return (Some(loc), rest.trim().to_owned());
    }
    (None, msg.to_owned())
}

/// The innermost message of an mlua error.
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

/// Resolves `./x` or `../dir/x` against the requiring file's folder, inside the pack.
/// `require` then loads it only if it's [`Pack::loadable`].
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

/// The `@file` chunk name of the Lua function at stack `level`.
pub(crate) fn caller_location(lua: &Lua, level: usize) -> Option<Location> {
    lua.inspect_stack(level, |d| {
        let src = d.source().source.map(|s| s.into_owned())?;
        let file = src.strip_prefix('@')?.to_owned();
        Some(Location {
            file,
            line: d.current_line().and_then(|l| u32::try_from(l).ok()),
            column: None,
        })
    })
    .flatten()
}

impl PackVm {
    /// Builds the pack's state and loads every declared script.
    pub fn new(pack: Rc<Pack>) -> Result<PackVm, Vec<Diagnostic>> {
        let vm = Self::bare(pack).map_err(|e| vec![error("vm", e.to_string(), None, None)])?;
        vm.load_scripts()
    }

    /// A state with the sandbox, `G` and `require`, but no scripts loaded.
    pub fn bare(pack: Rc<Pack>) -> mlua::Result<PackVm> {
        Self::bare_with_log(pack, Rc::new(RefCell::new(Vec::new())), Code::default())
    }

    /// [`PackVm::bare`], printing to `log`.
    fn bare_with_log(
        pack: Rc<Pack>,
        log: Rc<RefCell<Vec<String>>>,
        code: Code,
    ) -> mlua::Result<PackVm> {
        let libs = StdLib::TABLE
            | StdLib::STRING
            | StdLib::UTF8
            | StdLib::BIT
            | StdLib::MATH
            | StdLib::BUFFER
            | StdLib::VECTOR;
        let lua = Lua::new_with(libs, LuaOptions::default())?;
        let limits = pack.limits();
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

        let sink = log.clone();
        let print = lua.create_function(move |lua, args: Variadic<Value>| {
            let raw: Function = lua.named_registry_value("galaxy.tostring")?;
            // Copies at most one byte past the cap, so the cut is noticed.
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
            if l.len() >= LOG_CAP {
                l.remove(0);
            }
            l.push(clip_bytes(&line));
            Ok(())
        })?;
        globals.raw_set("print", print)?;

        lua.set_app_data(code.clone());
        let prelude = PRELUDE_CODE.with(Rc::clone);
        let g: Table = lua.load(&prelude[..]).set_name("=galaxy").call(())?;
        lua.set_named_registry_value("galaxy.G", g)?;
        lua.set_named_registry_value("galaxy.modules", lua.create_table()?)?;
        lua.set_named_registry_value("galaxy.loading", lua.create_table()?)?;

        let files = pack.clone();
        let require = lua.create_function(move |lua, name: String| -> mlua::Result<Value> {
            if name == "@galaxy" {
                return lua.named_registry_value("galaxy.G");
            }
            let from = caller_location(lua, 1).map(|l| l.file).unwrap_or_default();
            let path = if name.starts_with("./") || name.starts_with("../") {
                resolve(&from, &name)
            } else {
                None
            }
            .ok_or_else(|| {
                mlua::Error::runtime(format!(
                    "require(\"{name}\"): only `@galaxy` and the pack's own modules (`./name`) can be required"
                ))
            })?;
            if !files.loadable(&path) {
                let roots: Vec<String> = files.script_roots().iter().map(|r| format!("`{r}/`")).collect();
                return Err(mlua::Error::runtime(format!(
                    "require(\"{name}\"): `{path}` isn't a module in this pack's script roots ({})",
                    roots.join(", ")
                )));
            }
            let modules: Table = lua.named_registry_value("galaxy.modules")?;
            if let Some(v) = modules.raw_get::<Option<Value>>(path.as_str())? {
                return Ok(v);
            }
            let loading: Table = lua.named_registry_value("galaxy.loading")?;
            if loading.raw_get::<bool>(path.as_str()).unwrap_or(false) {
                return Err(mlua::Error::runtime(format!("require(\"{name}\"): `{path}` requires itself")));
            }
            let src = files
                .files
                .get(&path)
                .ok_or_else(|| mlua::Error::runtime(format!("require(\"{name}\"): no module `{path}` in this pack")))?;
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
        // Chunks read these (frozen by sandbox mode), never the writable per-thread
        // proxy sandbox mode installs, so no script can stash state in a global.
        lua.set_named_registry_value("galaxy.globals", globals)?;

        lua.sandbox(true)?;
        Ok(PackVm {
            lua,
            pack,
            scripts: Vec::new(),
            budget,
            log,
            code,
        })
    }

    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// Runs `f` with a fresh step budget and memory allowance. Returns an abort if it
    /// ran over either, or if the host panicked.
    ///
    /// The heap is fully collected first and the collector stays off while `f` runs,
    /// so the call may allocate exactly `limits.memory_bytes`, garbage included:
    /// running out doesn't depend on what earlier calls left behind, and weak tables
    /// don't change under the call.
    pub(crate) fn budgeted<R>(&self, f: impl FnOnce() -> mlua::Result<R>) -> Result<R, Abort> {
        let limits = self.pack.limits();
        self.budget.reset(limits.steps_per_call);
        let lua = &self.lua;
        let memory = usize::try_from(limits.memory_bytes).unwrap_or(usize::MAX);
        let prepared = lua
            .set_memory_limit(0)
            .and_then(|_| lua.gc_collect())
            .and_then(|()| lua.set_memory_limit(lua.used_memory().saturating_add(memory)));
        if let Err(e) = prepared {
            return Err(abort_from(&e, Some("vm")));
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
                    message: format!(
                        "the host failed on this call ({what}); its writes were discarded"
                    ),
                    location: self.budget.at.borrow().clone(),
                });
            }
        };
        if self.budget.exhausted.get() {
            return Err(Abort {
                code: "over_budget".into(),
                message: format!(
                    "ran over its budget of {} steps (an endless loop?); its writes were discarded",
                    self.pack.limits().steps_per_call
                ),
                location: self.budget.at.borrow().clone(),
            });
        }
        result.map_err(|e| abort_from(&e, None))
    }

    fn load_scripts(mut self) -> Result<PackVm, Vec<Diagnostic>> {
        let mut diags = Vec::new();
        let pack = self.pack.clone();
        for decl in &pack.manifest.scripts {
            match self.load_script(decl) {
                Ok(s) => self.scripts.push(s),
                Err(mut d) => diags.append(&mut d),
            }
        }
        if diags.is_empty() {
            Ok(self)
        } else {
            Err(diags)
        }
    }

    fn load_script(&self, decl: &ScriptDecl) -> Result<VmScript, Vec<Diagnostic>> {
        let file = decl.file.as_str();
        let at = |code: &str, msg: String| vec![error(code, msg, Some(file), None)];
        let src = self
            .pack
            .files
            .get(file)
            .ok_or_else(|| at("missing_file", format!("`{file}` doesn't exist")))?;
        let def: Value = self
            .budgeted(|| load_chunk(&self.lua, file, src)?.call::<Value>(()))
            .map_err(|a| {
                let mut d = a.diagnostic();
                if d.code == "runtime_error" {
                    d.code = "load_error".into();
                }
                vec![d]
            })?;
        let Value::Table(def) = def else {
            return Err(at(
                "bad_script",
                format!("`{file}` must `return G.script {{ ... }}`"),
            ));
        };
        let id: String = def.get("id").unwrap_or_default();
        if id != decl.id {
            return Err(at(
                "bad_script",
                format!(
                    "`{file}` declares script id `{id}`, but the manifest says `{}`",
                    decl.id
                ),
            ));
        }
        let mut diags = Vec::new();
        let mut events = BTreeSet::new();
        if let Ok(Some(on)) = def.get::<Option<Table>>("on") {
            for pair in on.pairs::<String, Value>() {
                let Ok((name, f)) = pair else { continue };
                let line = fn_line(&f);
                match EventKind::ALL.iter().find(|k| k.as_str() == name) {
                    Some(k) if matches!(f, Value::Function(_)) => {
                        events.insert(*k);
                    }
                    Some(_) => diags.push(error(
                        "bad_handler",
                        format!("`on.{name}` must be a function"),
                        Some(file),
                        line,
                    )),
                    None => diags.push(error(
                        "unknown_event",
                        format!("`on.{name}`: there is no `{name}` event"),
                        Some(file),
                        line,
                    )),
                }
            }
        }
        let mut tests = Vec::new();
        let mut beats = Vec::new();
        match def.get::<Value>("beats") {
            Ok(Value::Nil) => {}
            Ok(Value::Table(list)) => {
                for (n, item) in list.sequence_values::<Value>().enumerate() {
                    let index = n as i64 + 1;
                    let parsed = item.map_err(|e| e.to_string()).and_then(|v| match v {
                        Value::Table(t) => self.parse_beat(&t, index, &mut tests),
                        _ => Err("each entry must be `G.beat { ... }`".to_owned()),
                    });
                    match parsed {
                        Ok(b) => beats.push(b),
                        Err(msg) => diags.push(error(
                            "bad_beat",
                            format!("`beats[{index}]`: {msg}"),
                            Some(file),
                            None,
                        )),
                    }
                }
            }
            Ok(_) => diags.push(error(
                "bad_beat",
                "`beats` must be a list of `G.beat { ... }`",
                Some(file),
                None,
            )),
            Err(e) => diags.push(error("bad_beat", e.to_string(), Some(file), None)),
        }
        let mut triggers = Vec::new();
        // Handlers that exist but didn't parse: reported once, not also as missing.
        let mut broken = BTreeSet::new();
        if let Ok(Some(after)) = def.get::<Option<Table>>("after") {
            for pair in after.pairs::<String, Table>() {
                let (name, t) = match pair {
                    Ok(p) => p,
                    Err(e) => {
                        diags.push(error(
                            "bad_trigger",
                            format!("`after` entries must be `G.after {{ ... }}`: {e}"),
                            Some(file),
                            None,
                        ));
                        continue;
                    }
                };
                let line = t.get::<Value>("run").ok().as_ref().and_then(fn_line);
                match parse_after(&t) {
                    Ok(spec) => triggers.push(TriggerDef { name, spec }),
                    Err(msg) => {
                        diags.push(error(
                            "bad_trigger",
                            format!("`after.{name}`: {msg}"),
                            Some(file),
                            line,
                        ));
                        broken.insert(format!("after.{name}"));
                    }
                }
            }
        }
        triggers.sort_by(|a, b| a.name.cmp(&b.name));
        // The manifest's handler list must match what the script defines.
        let defined: BTreeSet<String> = events
            .iter()
            .map(|k| format!("on.{}", k.as_str()))
            .chain(triggers.iter().map(|t| format!("after.{}", t.name)))
            .collect();
        let declared: BTreeSet<String> = decl.handlers.iter().cloned().collect();
        for h in defined.difference(&declared) {
            diags.push(error(
                "undeclared_handler",
                format!(
                    "script `{}` defines `{h}`, which isn't in its manifest `handlers`",
                    decl.id
                ),
                Some(file),
                None,
            ));
        }
        for h in declared
            .difference(&defined)
            .filter(|h| !broken.contains(*h))
        {
            diags.push(error(
                "missing_handler",
                format!(
                    "the manifest declares `{h}` for script `{}`, but the script doesn't define it",
                    decl.id
                ),
                Some("manifest.toml"),
                None,
            ));
        }
        if !diags.is_empty() {
            return Err(diags);
        }
        let key = self
            .lua
            .create_registry_value(def)
            .map_err(|e| at("vm", e.to_string()))?;
        Ok(VmScript {
            decl: decl.clone(),
            def: key,
            events,
            triggers,
            beats,
            tests,
        })
    }

    /// Reads one `G.beat { ... }` table: which callbacks it has, where its `G.test`
    /// predicates are, and the rest as a [`BeatDef`].
    fn parse_beat(
        &self,
        t: &Table,
        index: i64,
        tests: &mut Vec<Vec<PathKey>>,
    ) -> Result<LoadedBeat, String> {
        let lua = &self.lua;
        let mut hooks = BTreeSet::new();
        for name in BEAT_HOOKS {
            match t.get::<Value>(name).map_err(|e| e.to_string())? {
                Value::Nil => {}
                Value::Function(_) => {
                    hooks.insert(name);
                }
                _ => return Err(format!("`{name}` must be a function(ctx, beat)")),
            }
        }
        let base = vec![PathKey::Key("beats".into()), PathKey::Index(index)];
        let plain = lua.create_table().map_err(|e| e.to_string())?;
        let mut keys: Vec<(Value, Value)> = t
            .pairs::<Value, Value>()
            .collect::<mlua::Result<_>>()
            .map_err(|e| e.to_string())?;
        // Walk in a fixed order so predicates are numbered the same on every load.
        keys.sort_by_key(|(k, _)| k.as_string().map(|s| s.to_string_lossy()));
        for (k, v) in keys {
            let key = k
                .as_string()
                .map(|s| s.to_string_lossy())
                .unwrap_or_default();
            if BEAT_HOOKS.contains(&key.as_str()) {
                continue;
            }
            let mut path = base.clone();
            path.push(PathKey::Key(key.clone()));
            let v = strip_tests(lua, v, &path, tests)?;
            let v = match key.as_str() {
                "requires" | "done" => requirement_list(lua, v)?,
                "steps" => steps(lua, v)?,
                _ => v,
            };
            plain.raw_set(k, v).map_err(|e| e.to_string())?;
        }
        let spec: BeatSpec = lua
            .from_value(Value::Table(plain))
            .map_err(|e| format!("{e}"))?;
        let def = spec.into_def()?;
        if let Some(p) = galaxy_progress::beats::validate(&def).first() {
            return Err(format!("{}: {p}", def.id));
        }
        Ok(LoadedBeat { def, index, hooks })
    }

    /// The function at `path` in a fresh instance of a script.
    fn function_at(&self, instance: &Table, path: &[PathKey]) -> mlua::Result<Function> {
        let mut v = Value::Table(instance.clone());
        for k in path {
            let Value::Table(t) = v else {
                return Err(mlua::Error::runtime(
                    "the script's definition changed shape between instances",
                ));
            };
            v = match k {
                PathKey::Key(s) => t.raw_get(s.as_str())?,
                PathKey::Index(i) => t.raw_get(*i)?,
            };
        }
        match v {
            Value::Function(f) => Ok(f),
            _ => Err(mlua::Error::runtime(
                "the script's definition changed shape between instances",
            )),
        }
    }

    /// Runs a `G.test` predicate on a fresh instance of its script, in a fresh Luau
    /// state, like a handler call. It may read but not write; it must return a boolean.
    pub fn predicate(
        &self,
        script: usize,
        index: u32,
        view: View<'_>,
        rng: &mut dyn RngSource,
    ) -> Result<bool, Abort> {
        let s = &self.scripts[script];
        let path = s.tests.get(index as usize).cloned().ok_or_else(|| Abort {
            code: "bad_beat".into(),
            message: format!("no G.test predicate {index}"),
            location: None,
        })?;
        let fresh = self.fresh()?;
        let (result, buffer) = fresh.with_ctx(&s.decl, view, rng, None, |_, ctx| {
            let f = fresh.function_at(&fresh.instantiate(&s.decl.file)?, &path)?;
            f.call::<Value>(ctx)
        });
        let v = result?;
        if !buffer.is_empty() {
            return Err(Abort {
                code: "predicate_wrote".into(),
                message: "a G.test predicate only reads; its writes were discarded".into(),
                location: None,
            });
        }
        match v {
            Value::Boolean(b) => Ok(b),
            other => Err(Abort {
                code: "bad_predicate".into(),
                message: format!(
                    "a G.test predicate must return true or false, not {}",
                    other.type_name()
                ),
                location: None,
            }),
        }
    }

    /// Runs a beat callback on a fresh instance of its script, in a fresh Luau state,
    /// its buffer seeded with the transition's batch: `beat.why` is that batch's cause,
    /// so the callback's writes and the transition apply together or not at all.
    #[allow(clippy::too_many_arguments)]
    pub fn beat_hook(
        &self,
        script: usize,
        beat: usize,
        hook: &str,
        info: &BeatCall<'_>,
        view: View<'_>,
        rng: &mut dyn RngSource,
        seed: CommandBatch,
    ) -> Result<Option<CommandBatch>, Abort> {
        let s = &self.scripts[script];
        let Some(b) = s.beats.get(beat).filter(|b| b.has_hook(hook)) else {
            return Ok(Some(seed));
        };
        let path = vec![
            PathKey::Key("beats".into()),
            PathKey::Index(b.index),
            PathKey::Key(hook.to_owned()),
        ];
        let fresh = self.fresh()?;
        let (result, buffer) = fresh.with_ctx(&s.decl, view, rng, Some(seed), |lua, ctx| {
            let f = fresh.function_at(&fresh.instantiate(&s.decl.file)?, &path)?;
            let t = lua.create_table()?;
            t.raw_set("id", info.beat.as_str())?;
            t.raw_set("transition", info.transition)?;
            t.raw_set("step", info.step)?;
            t.raw_set("step_id", info.step_id)?;
            let why = lua.create_table()?;
            why.raw_set("summary", info.summary)?;
            why.raw_set("cites", lua.create_table()?)?;
            why.raw_set("handle", 1)?;
            why.set_readonly(true);
            t.raw_set("why", why)?;
            t.set_readonly(true);
            f.call::<()>((ctx, t))
        });
        result?;
        Ok(buffer.into_batch())
    }

    /// A fresh state for this pack, with no scripts loaded, printing to the same log.
    fn fresh(&self) -> Result<PackVm, Abort> {
        Self::bare_with_log(self.pack.clone(), self.log.clone(), self.code.clone())
            .map_err(|e| abort_from(&e, Some("vm")))
    }

    /// Calls `f` with a fresh `ctx` for the script `decl` inside the step budget, and
    /// returns its result (with quota and ctx error codes applied) and its write buffer.
    fn with_ctx<R>(
        &self,
        decl: &ScriptDecl,
        view: View<'_>,
        rng: &mut dyn RngSource,
        seed: Option<CommandBatch>,
        f: impl FnOnce(&Lua, Table) -> mlua::Result<R>,
    ) -> (Result<R, Abort>, Buffer) {
        let buffer = RefCell::new(seed.map(Buffer::seeded).unwrap_or_default());
        let rng = RefCell::new(&mut *rng as &mut dyn RngSource);
        let rng_key = format!("script/{}/{}", self.pack.manifest.id, decl.id);
        let env = CallEnv {
            world: view.world,
            history: view.history,
            pack: &self.pack.manifest.id,
            revision: &self.pack.revision,
            opens: &self.pack.manifest.opens,
            uses: &self.pack.manifest.uses,
            script: decl,
            buffer: &buffer,
            rng: &rng,
            rng_key: &rng_key,
            budget: &self.budget,
        };
        let lua = &self.lua;
        let result = self.budgeted(|| {
            lua.scope(|scope| {
                let ctx = ctx::build(lua, scope, &env)?;
                f(lua, ctx)
            })
        });
        let buffer = buffer.into_inner();
        let result = match result {
            Ok(r) => match buffer.over_quota.clone() {
                Some(message) => Err(Abort {
                    code: "over_quota".into(),
                    message,
                    location: None,
                }),
                None => Ok(r),
            },
            Err(mut a) => {
                if a.code == "runtime_error"
                    && let Some(code) = buffer.failed.clone()
                {
                    a.code = code;
                }
                Err(a)
            }
        };
        (result, buffer)
    }

    /// The line a Luau function is defined on.
    pub fn handler_line(&self, script: usize, target: Target<'_>) -> Option<u32> {
        self.handler(script, target)
            .ok()
            .and_then(|f| fn_line(&Value::Function(f)))
    }

    fn handler(&self, script: usize, target: Target<'_>) -> mlua::Result<Function> {
        let def: Table = self.lua.registry_value(&self.scripts[script].def)?;
        handler_of(&def, target)
    }

    /// Runs a script's file afresh, with an empty module cache, and returns its
    /// definition. Nothing a previous call left in a local or upvalue is reachable.
    fn instantiate(&self, file: &str) -> mlua::Result<Table> {
        let lua = &self.lua;
        lua.set_named_registry_value("galaxy.modules", lua.create_table()?)?;
        lua.set_named_registry_value("galaxy.loading", lua.create_table()?)?;
        // Never run a file the checks didn't cover.
        if !self.pack.loadable(file) {
            return Err(mlua::Error::runtime(format!(
                "`{file}` isn't a loadable script"
            )));
        }
        let src = self
            .pack
            .files
            .get(file)
            .ok_or_else(|| mlua::Error::runtime(format!("`{file}` doesn't exist")))?;
        load_chunk(lua, file, src)?.call::<Table>(())
    }

    /// Calls one handler on a fresh instance of its script, in a fresh Luau state. On
    /// success returns its batch (its one cause and every write, if it wrote
    /// anything); on error, over budget or over quota, nothing.
    pub fn call(
        &self,
        script: usize,
        target: Target<'_>,
        view: View<'_>,
        rng: &mut dyn RngSource,
    ) -> Result<Option<CommandBatch>, Abort> {
        self.fresh()?
            .call_here(&self.scripts[script].decl, target, view, rng)
    }

    /// Runs one handler of the script `decl` in this state.
    fn call_here(
        &self,
        decl: &ScriptDecl,
        target: Target<'_>,
        view: View<'_>,
        rng: &mut dyn RngSource,
    ) -> Result<Option<CommandBatch>, Abort> {
        let buffer = RefCell::new(Buffer::default());
        let rng = RefCell::new(&mut *rng as &mut dyn RngSource);
        let rng_key = format!("script/{}/{}", self.pack.manifest.id, decl.id);
        let env = CallEnv {
            world: view.world,
            history: view.history,
            pack: &self.pack.manifest.id,
            revision: &self.pack.revision,
            opens: &self.pack.manifest.opens,
            uses: &self.pack.manifest.uses,
            script: decl,
            buffer: &buffer,
            rng: &rng,
            rng_key: &rng_key,
            budget: &self.budget,
        };
        let lua = &self.lua;
        let result = self.budgeted(|| {
            let func = handler_of(&self.instantiate(&decl.file)?, target)?;
            lua.scope(|scope| {
                let ctx = ctx::build(lua, scope, &env)?;
                match target {
                    Target::Event(e) => func.call::<()>((ctx::event_table(lua, e)?, ctx)),
                    Target::Trigger { name, fired } => {
                        let fire = lua.create_table()?;
                        fire.raw_set("name", name)?;
                        fire.raw_set("fired", fired)?;
                        func.call::<()>((ctx, fire))
                    }
                }
            })
        });
        let buffer = buffer.into_inner();
        match result {
            // A quota error is final even if the script caught it.
            Ok(()) => match buffer.over_quota {
                Some(message) => Err(Abort {
                    code: "over_quota".into(),
                    message,
                    location: None,
                }),
                None => Ok(buffer.into_batch()),
            },
            Err(mut a) => {
                if a.code == "runtime_error"
                    && let Some(code) = buffer.failed.clone()
                {
                    a.code = code;
                }
                Err(a)
            }
        }
    }

    /// Runs a Luau test file: it returns `{ [name] = function() ... end }`.
    pub fn run_tests(&self, file: &str) -> Result<Vec<TestOutcome>, Abort> {
        let src = self.pack.files.get(file).cloned().unwrap_or_default();
        let tests: Table =
            self.budgeted(|| load_chunk(&self.lua, file, &src)?.call::<Table>(()))?;
        let mut names: Vec<(String, Function)> = tests
            .pairs::<String, Function>()
            .filter_map(Result::ok)
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(names
            .into_iter()
            .map(|(name, f)| {
                let r = self.budgeted(|| f.call::<()>(()));
                (name, r)
            })
            .collect())
    }
}

/// Loads a pack file (its cached bytecode) as a chunk with its own read-only environment.
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
    let code = lua.app_data_ref::<Code>().and_then(|c| c.of(src));
    let chunk = match &code {
        Some(b) => lua.load(&b[..]),
        None => lua.load(src),
    };
    chunk
        .set_name(format!("@{path}"))
        .set_environment(env)
        .into_function()
}

/// What a beat callback is told about its beat.
pub struct BeatCall<'a> {
    pub beat: &'a BeatId,
    /// `start`, `step` or `complete`.
    pub transition: &'static str,
    pub step: u32,
    pub step_id: Option<&'a str>,
    pub summary: &'a str,
}

/// A `G.beat` table as Luau writes it (credits as credits).
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BeatSpec {
    id: BeatId,
    act: u32,
    #[serde(default)]
    kind: BeatKind,
    #[serde(default)]
    giver: Option<NpcId>,
    goal: String,
    #[serde(default)]
    requires: Requirement,
    #[serde(default)]
    steps: Vec<StepDef>,
    #[serde(default)]
    done: Requirement,
    #[serde(default)]
    rewards: RewardSpec,
}

#[derive(Default, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
struct RewardSpec {
    credits: f64,
    standing: BTreeMap<FactionId, i32>,
    flags: Vec<FlagId>,
    fit: Vec<UpgradeId>,
}

impl BeatSpec {
    fn into_def(self) -> Result<BeatDef, String> {
        let credits =
            galaxy_contract::money::cents_from_credits(self.rewards.credits).ok_or_else(|| {
                format!(
                    "{}: rewards.credits {} isn't a whole number of centicredits",
                    self.id, self.rewards.credits
                )
            })?;
        Ok(BeatDef {
            id: self.id,
            act: self.act,
            kind: self.kind,
            giver: self.giver,
            goal: self.goal,
            requires: self.requires,
            steps: self.steps,
            done: self.done,
            rewards: Rewards {
                credits,
                standing: self.rewards.standing,
                flags: self.rewards.flags,
                fit: self.rewards.fit,
            },
        })
    }
}

/// A deep copy of `v` with each `{ test = function }` replaced by `{ test = index }`,
/// recording the function's path in `tests`. Any other function is an error. Keys are
/// walked in sorted order, so the numbering is the same on every load.
fn strip_tests(
    lua: &Lua,
    v: Value,
    path: &[PathKey],
    tests: &mut Vec<Vec<PathKey>>,
) -> Result<Value, String> {
    match v {
        Value::Table(t) => {
            if let Ok(Value::Function(_)) = t.raw_get::<Value>("test") {
                let mut p = path.to_vec();
                p.push(PathKey::Key("test".into()));
                tests.push(p);
                let out = lua.create_table().map_err(|e| e.to_string())?;
                out.raw_set("test", tests.len() - 1)
                    .map_err(|e| e.to_string())?;
                return Ok(Value::Table(out));
            }
            let out = lua.create_table().map_err(|e| e.to_string())?;
            let mut pairs: Vec<(Value, Value)> = t
                .pairs::<Value, Value>()
                .collect::<mlua::Result<_>>()
                .map_err(|e| e.to_string())?;
            pairs.sort_by_key(|(k, _)| match k {
                Value::Integer(i) => (0, *i, String::new()),
                Value::Number(n) => (0, *n as i64, String::new()),
                other => (
                    1,
                    0,
                    other
                        .as_string()
                        .map(|s| s.to_string_lossy())
                        .unwrap_or_default(),
                ),
            });
            for (k, v) in pairs {
                let key = match &k {
                    Value::Integer(i) => PathKey::Index(*i),
                    Value::Number(n) => PathKey::Index(*n as i64),
                    other => PathKey::Key(
                        other
                            .as_string()
                            .map(|s| s.to_string_lossy())
                            .unwrap_or_default(),
                    ),
                };
                let mut p = path.to_vec();
                p.push(key);
                out.raw_set(k, strip_tests(lua, v, &p, tests)?)
                    .map_err(|e| e.to_string())?;
            }
            Ok(Value::Table(out))
        }
        Value::Function(_) => Err(
            "functions go only in `on_start`, `on_step`, `on_complete` and `G.test(...)`".into(),
        ),
        other => Ok(other),
    }
}

/// A requirement written as a list means all of them.
fn requirement_list(lua: &Lua, v: Value) -> Result<Value, String> {
    match &v {
        Value::Table(t) if t.raw_len() > 0 || t.pairs::<Value, Value>().next().is_none() => {
            let all = lua.create_table().map_err(|e| e.to_string())?;
            all.raw_set("all", t.clone()).map_err(|e| e.to_string())?;
            Ok(Value::Table(all))
        }
        _ => Ok(v),
    }
}

/// Each step's `done` may be a list too.
fn steps(lua: &Lua, v: Value) -> Result<Value, String> {
    if let Value::Table(list) = &v {
        for step in list.sequence_values::<Table>() {
            let step = step.map_err(|e| e.to_string())?;
            let done: Value = step.raw_get("done").map_err(|e| e.to_string())?;
            step.raw_set("done", requirement_list(lua, done)?)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(v)
}

fn handler_of(def: &Table, target: Target<'_>) -> mlua::Result<Function> {
    match target {
        Target::Event(e) => def.get::<Table>("on")?.get(e.event.kind().as_str()),
        Target::Trigger { name, .. } => def.get::<Table>("after")?.get::<Table>(name)?.get("run"),
    }
}

/// Replaces `0x` followed by eight or more hex digits (a heap address in a Luau
/// message) so abort messages, which reach history, are the same on every run.
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

fn fn_line(v: &Value) -> Option<u32> {
    match v {
        Value::Function(f) => f.info().line_defined.and_then(|l| u32::try_from(l).ok()),
        _ => None,
    }
}

pub(crate) fn abort_from(e: &mlua::Error, code: Option<&str>) -> Abort {
    let (memory, msg) = root_message(e);
    let msg = without_addresses(&msg);
    // Drop Luau's stack traceback: the location prefix says where.
    let msg = msg
        .split(
            "
stack traceback:",
        )
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if memory {
        return Abort {
            code: "out_of_memory".into(),
            message: clip_text(&format!(
                "ran out of memory ({msg}); its writes were discarded"
            )),
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

fn parse_after(t: &Table) -> Result<AfterSpec, String> {
    let spec: Table = t
        .get("after")
        .map_err(|_| "not made with `G.after`".to_owned())?;
    let mut counters = BTreeMap::new();
    let mut station = None;
    let mut region = None;
    let mut every = false;
    for pair in spec.pairs::<String, Value>() {
        let (k, v) = pair.map_err(|e| e.to_string())?;
        match k.as_str() {
            "station" => {
                let s = v
                    .as_string()
                    .map(|s| s.to_string_lossy())
                    .ok_or("`station` must be a station ID")?;
                station = Some(s.parse::<StationId>().map_err(|e| e.to_string())?);
            }
            "region" => {
                let s = v
                    .as_string()
                    .map(|s| s.to_string_lossy())
                    .ok_or("`region` must be a region ID")?;
                region = Some(s.parse::<RegionId>().map_err(|e| e.to_string())?);
            }
            "every" => every = v.as_boolean().ok_or("`every` must be true or false")?,
            name => {
                let kind = galaxy_contract::api::COUNTERS
                    .iter()
                    .find(|c| galaxy_contract::api::counter_name(**c) == name)
                    .ok_or_else(|| {
                        format!(
                            "`{name}` isn't an activity counter. Triggers pace by activity, never time; use one of: {}",
                            galaxy_contract::api::COUNTERS
                                .iter()
                                .map(|c| galaxy_contract::api::counter_name(*c))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?;
                let n = whole_number(&v)
                    .and_then(|n| u64::try_from(n).ok())
                    .filter(|n| *n >= 1)
                    .ok_or_else(|| format!("`{name}` must be a whole number of at least 1"))?;
                counters.insert(*kind, n);
            }
        }
    }
    if counters.is_empty() {
        return Err("name at least one activity counter, e.g. `G.after { dockings = 3 }`".into());
    }
    let scope = match (station, region) {
        (Some(_), Some(_)) => return Err("give `station` or `region`, not both".into()),
        (Some(station), None) => CounterScope::Station { station },
        (None, Some(region)) => CounterScope::Region { region },
        (None, None) => CounterScope::Player,
    };
    Ok(AfterSpec {
        counters,
        scope,
        every,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_inside_the_pack() {
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
        assert_eq!(whole_number(&Value::Integer(i64::MIN)), Some(i64::MIN));
    }

    #[test]
    fn splits_luau_locations() {
        let (loc, msg) = split_location("scripts/a.luau:12: boom");
        assert_eq!(loc.unwrap().line, Some(12));
        assert_eq!(msg, "boom");
    }
}
