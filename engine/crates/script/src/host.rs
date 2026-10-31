//! A module's scripts, loaded and called. The sim decides when to call them (on events and
//! on the slow cadence, never every frame) and applies what they write.
//!
//! A script file returns a table:
//!
//! ```lua
//! return {
//!     on = {
//!         ["engine.start"] = function(event, ctx) ... end,
//!         pressed = function(event, ctx) ... end,   -- this module's own event kind
//!     },
//!     cadence = function(ctx) ... end,             -- once per second of game time
//! }
//! ```
//!
//! [`ScriptSet::load`] runs each entry script once to learn which handlers it has, and
//! checks their event kinds exist. [`ScriptSet::call`] then runs one handler in a fresh,
//! budgeted state ([`crate::vm`]) with a `ctx` ([`crate::api::CTX`]) that reads the world
//! through a [`WorldView`] and buffers writes as commands. Each write is checked against
//! the registry (schemas and capabilities) when it is made, so a mistake fails at the line
//! that made it. The call returns its commands, or an [`Abort`]; an aborted call writes
//! nothing.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::rc::Rc;

use galaxy_api::command::{Actor, Command, Fields};
use galaxy_api::rng::SimRng;
use galaxy_api::{Registry, WorldView};
use galaxy_module::ids;
use galaxy_module::module::{Module, ModuleFiles};
use mlua::{Lua, Table, Value};
use serde_json::Value as Json;

use crate::meter::{self, RECORDS_PER_STEP};
use crate::vm::{Abort, Code, LIMITS, Limits, Location, Vm, script_location, whole_number};

/// One entry script and the handlers it defines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedScript {
    pub file: String,
    /// Event kinds it handles, as full IDs.
    pub events: BTreeSet<String>,
    pub cadence: bool,
}

/// What a call is for.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    Event(&'a Event<'a>),
    Cadence,
}

/// An event as a handler sees it.
#[derive(Clone, Copy, Debug)]
pub struct Event<'a> {
    pub kind: &'a str,
    pub tick: u64,
    /// The fact's sequence number.
    pub fact: u64,
    /// The input's source, for an input.
    pub source: Option<&'a str>,
    pub payload: &'a Fields,
}

/// What a call did: its commands, or why it was aborted; and what it printed.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub result: Result<Vec<Command>, Abort>,
    pub log: Vec<String>,
}

/// A module's loaded scripts.
pub struct ScriptSet {
    pub module: String,
    files: Rc<ModuleFiles>,
    code: Code,
    limits: Limits,
    pub scripts: Vec<LoadedScript>,
}

impl ScriptSet {
    /// Loads the module's entry scripts. Every problem is reported, not just the first.
    pub fn load(module: &Module, registry: &Registry) -> Result<ScriptSet, Vec<Abort>> {
        let mut set = ScriptSet {
            module: module.id().to_owned(),
            files: Rc::new(module.files.clone()),
            code: Code::default(),
            limits: LIMITS,
            scripts: Vec::new(),
        };
        let mut problems = Vec::new();
        for file in &module.manifest.scripts {
            match set.load_script(file, registry) {
                Ok(script) => set.scripts.push(script),
                Err(mut list) => problems.append(&mut list),
            }
        }
        if problems.is_empty() {
            Ok(set)
        } else {
            Err(problems)
        }
    }

    fn load_script(&self, file: &str, registry: &Registry) -> Result<LoadedScript, Vec<Abort>> {
        let at = |message: String| Abort {
            code: "load_error".into(),
            message,
            location: Some(Location {
                file: file.to_owned(),
                line: None,
            }),
        };
        let vm = self.vm().map_err(|e| vec![e])?;
        let def = vm.budgeted(|| vm.run_file(file)).map_err(|mut a| {
            if a.code == "runtime_error" {
                a.code = "load_error".into();
            }
            vec![a]
        })?;
        let Value::Table(def) = def else {
            return Err(vec![at(
                "a script returns a table: `return { on = { ... }, cadence = function(ctx) ... end }`"
                    .into(),
            )]);
        };
        let mut problems = Vec::new();
        let mut events = BTreeSet::new();
        let mut cadence = false;
        for pair in def.pairs::<Value, Value>() {
            let Ok((key, value)) = pair else { continue };
            let key = key.to_string().unwrap_or_default();
            match (key.as_str(), value) {
                ("cadence", Value::Function(_)) => cadence = true,
                ("cadence", _) => problems.push(at("`cadence` must be a function(ctx)".into())),
                ("on", Value::Table(on)) => {
                    for pair in on.pairs::<Value, Value>() {
                        let Ok((name, handler)) = pair else { continue };
                        let name = name.to_string().unwrap_or_default();
                        let Some(kind) = ids::qualify_type(&self.module, &name) else {
                            problems.push(at(format!("`on[\"{name}\"]`: not an event kind")));
                            continue;
                        };
                        if !registry.events.contains(&kind) {
                            problems.push(at(format!(
                                "`on[\"{name}\"]`: there is no event kind `{kind}` (a module lists \
                                 its event kinds in its manifest's `events`)"
                            )));
                        } else if !matches!(handler, Value::Function(_)) {
                            problems.push(at(format!(
                                "`on[\"{name}\"]` must be a function(event, ctx)"
                            )));
                        } else {
                            events.insert(kind);
                        }
                    }
                }
                ("on", _) => problems.push(at("`on` must be a table of handlers".into())),
                (other, _) => problems.push(at(format!(
                    "unknown key `{other}`; a script has `on` and `cadence`"
                ))),
            }
        }
        if problems.is_empty() {
            Ok(LoadedScript {
                file: file.to_owned(),
                events,
                cadence,
            })
        } else {
            Err(problems)
        }
    }

    fn vm(&self) -> Result<Vm, Abort> {
        Vm::new(self.files.clone(), &self.code, self.limits).map_err(|e| Abort {
            code: "host_error".into(),
            message: format!("can't make a Luau state: {e}"),
            location: None,
        })
    }

    /// Calls one handler of script `script` in a fresh state.
    pub fn call(
        &self,
        script: usize,
        target: Target<'_>,
        view: &dyn WorldView,
        registry: &Registry,
        rng: &mut SimRng,
    ) -> Outcome {
        let file = &self.scripts[script].file;
        let vm = match self.vm() {
            Ok(vm) => vm,
            Err(abort) => {
                return Outcome {
                    result: Err(abort),
                    log: Vec::new(),
                };
            }
        };
        let env = Env {
            module: &self.module,
            view,
            registry,
            rng: RefCell::new(rng),
            buffer: RefCell::new(Vec::new()),
            failed: RefCell::new(None),
            limits: self.limits,
            budget: &vm.budget,
        };
        let lua = &vm.lua;
        let result = vm.budgeted(|| {
            let Value::Table(def) = vm.run_file(file)? else {
                return Err(mlua::Error::runtime("the script no longer returns a table"));
            };
            lua.scope(|scope| {
                let ctx = build_ctx(lua, scope, &env)?;
                match target {
                    Target::Event(event) => {
                        let on: Table = def.get("on")?;
                        let local = event
                            .kind
                            .strip_prefix(&format!("{}.", self.module))
                            .filter(|l| ids::valid_field(l));
                        let mut handler: Value = on.get(event.kind)?;
                        if handler.is_nil()
                            && let Some(local) = local
                        {
                            handler = on.get(local)?;
                        }
                        let Value::Function(f) = handler else {
                            return Err(mlua::Error::runtime(format!(
                                "the script no longer handles `{}`",
                                event.kind
                            )));
                        };
                        f.call::<()>((event_table(lua, event)?, ctx))
                    }
                    Target::Cadence => {
                        let f: mlua::Function = def.get("cadence")?;
                        f.call::<()>(ctx)
                    }
                }
            })
        });
        let log = vm.log.borrow().clone();
        let failed = env.failed.borrow_mut().take();
        let result = match (result, failed) {
            // A ctx failure is final even if the script caught it with pcall.
            (_, Some(abort)) => Err(abort),
            (Ok(()), None) => Ok(std::mem::take(&mut *env.buffer.borrow_mut())),
            (Err(abort), None) => Err(abort),
        };
        Outcome { result, log }
    }
}

/// What a call's `ctx` functions share.
struct Env<'a> {
    module: &'a str,
    view: &'a dyn WorldView,
    registry: &'a Registry,
    rng: RefCell<&'a mut SimRng>,
    buffer: RefCell<Vec<Command>>,
    failed: RefCell<Option<Abort>>,
    limits: Limits,
    budget: &'a meter::Budget,
}

impl Env<'_> {
    /// Marks the call as failed (for good) and returns the Luau error.
    fn fail(&self, lua: &Lua, code: &str, message: impl Into<String>) -> mlua::Error {
        let message = message.into();
        let mut failed = self.failed.borrow_mut();
        if failed.is_none() {
            *failed = Some(Abort {
                code: code.into(),
                message: message.clone(),
                location: script_location(lua),
            });
        }
        mlua::Error::runtime(message)
    }

    fn entity(&self, lua: &Lua, name: &str) -> mlua::Result<String> {
        if let Some((ns, _)) = ids::split(name)
            && (ns == ids::ENGINE || self.registry.modules.contains_key(ns))
        {
            return Ok(name.to_owned());
        }
        ids::qualify_entity(self.module, name)
            .ok_or_else(|| self.fail(lua, "bad_command", ids::local_rule(name)))
    }

    fn kind(&self, lua: &Lua, name: &str) -> mlua::Result<String> {
        ids::qualify_type(self.module, name).ok_or_else(|| {
            self.fail(
                lua,
                "bad_command",
                format!("`{name}` is not a component type or event kind"),
            )
        })
    }

    /// Checks and buffers a command.
    fn write(&self, lua: &Lua, command: Command) -> mlua::Result<()> {
        if self.buffer.borrow().len() >= self.limits.commands_per_call {
            return Err(self.fail(
                lua,
                "over_quota",
                format!(
                    "a call writes at most {} commands",
                    self.limits.commands_per_call
                ),
            ));
        }
        let actor = Actor::Module(self.module);
        let command = self
            .registry
            .check(actor, &command)
            .map_err(|e| self.fail(lua, "bad_command", format!("{}: {e}", command.op())))?;
        if let Command::Put { entity, .. } = &command
            && !self.view.exists(entity)
            && !self.registry.may_create(actor, entity)
        {
            return Err(self.fail(
                lua,
                "bad_command",
                format!(
                    "put: module `{}` may only create entities in its own namespace, not \
                     `{entity}`",
                    self.module
                ),
            ));
        }
        self.buffer.borrow_mut().push(command);
        Ok(())
    }
}

/// Builds `ctx` for one call. Every name in [`crate::api::CTX`] is set here.
fn build_ctx<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope mlua::Scope<'scope, 'env>,
    env: &'env Env<'env>,
) -> mlua::Result<Table> {
    let ctx = lua.create_table()?;
    ctx.raw_set("tick", env.view.tick())?;
    ctx.raw_set("module", env.module)?;

    ctx.raw_set(
        "get",
        scope.create_function(|lua, (entity, component): (String, String)| {
            let entity = env.entity(lua, &entity)?;
            let component = env.kind(lua, &component)?;
            match env.view.get(&entity, &component) {
                None => Ok(Value::Nil),
                Some(fields) => {
                    env.budget
                        .charge(lua, 1 + fields.len() as u64 / RECORDS_PER_STEP)?;
                    Ok(Value::Table(object_to_lua(lua, fields)?))
                }
            }
        })?,
    )?;
    ctx.raw_set(
        "has",
        scope.create_function(|lua, (entity, component): (String, String)| {
            let entity = env.entity(lua, &entity)?;
            let component = env.kind(lua, &component)?;
            Ok(env.view.get(&entity, &component).is_some())
        })?,
    )?;
    ctx.raw_set(
        "query",
        scope.create_function(|lua, component: String| {
            let component = env.kind(lua, &component)?;
            let found = env.view.with_component(&component);
            env.budget
                .charge(lua, 1 + found.len() as u64 / RECORDS_PER_STEP)?;
            lua.create_sequence_from(found)
        })?,
    )?;
    ctx.raw_set(
        "put",
        scope.create_function(
            |lua, (entity, component, fields): (String, String, Option<Table>)| {
                let command = Command::Put {
                    entity: env.entity(lua, &entity)?,
                    component: env.kind(lua, &component)?,
                    fields: fields_arg(lua, env, fields)?,
                };
                env.write(lua, command)
            },
        )?,
    )?;
    ctx.raw_set(
        "patch",
        scope.create_function(
            |lua, (entity, component, fields): (String, String, Table)| {
                let command = Command::Patch {
                    entity: env.entity(lua, &entity)?,
                    component: env.kind(lua, &component)?,
                    fields: fields_arg(lua, env, Some(fields))?,
                };
                env.write(lua, command)
            },
        )?,
    )?;
    ctx.raw_set(
        "remove",
        scope.create_function(|lua, (entity, component): (String, Option<String>)| {
            let command = Command::Remove {
                entity: env.entity(lua, &entity)?,
                component: component.map(|c| env.kind(lua, &c)).transpose()?,
            };
            env.write(lua, command)
        })?,
    )?;
    ctx.raw_set(
        "record",
        scope.create_function(|lua, (kind, payload): (String, Option<Table>)| {
            let command = Command::Record {
                kind: env.kind(lua, &kind)?,
                payload: fields_arg(lua, env, payload)?,
            };
            env.write(lua, command)
        })?,
    )?;
    ctx.raw_set(
        "random",
        scope.create_function(|_, ()| Ok(env.rng.borrow_mut().unit_f64()))?,
    )?;
    ctx.raw_set(
        "random_int",
        scope.create_function(|lua, (lo, hi): (Value, Value)| {
            let (Some(lo), Some(hi)) = (whole_number(&lo), whole_number(&hi)) else {
                return Err(env.fail(lua, "bad_command", "random_int takes two whole numbers"));
            };
            if hi < lo {
                return Err(env.fail(lua, "bad_command", "random_int: hi is below lo"));
            }
            let span = (i128::from(hi) - i128::from(lo) + 1) as u128;
            let n = u128::from(env.rng.borrow_mut().next_u64()) % span;
            Ok((i128::from(lo) + n as i128) as f64)
        })?,
    )?;
    ctx.set_readonly(true);
    Ok(ctx)
}

/// Deepest nesting of tables a script may hand the host.
const MAX_DEPTH: usize = 8;

fn fields_arg(lua: &Lua, env: &Env<'_>, table: Option<Table>) -> mlua::Result<Fields> {
    let Some(table) = table else {
        return Ok(Fields::new());
    };
    match lua_to_json(lua, env, Value::Table(table), 0)? {
        Json::Object(map) => Ok(map),
        // An empty table reads as an empty list.
        Json::Array(items) if items.is_empty() => Ok(Fields::new()),
        _ => Err(env.fail(lua, "bad_command", "expected a table keyed by names")),
    }
}

/// A Luau value as JSON: whole numbers become integers, tables keyed `1..n` lists, other
/// tables must be keyed by strings.
fn lua_to_json(lua: &Lua, env: &Env<'_>, value: Value, depth: usize) -> mlua::Result<Json> {
    let bad = |message: String| env.fail(lua, "bad_command", message);
    Ok(match value {
        Value::Nil => Json::Null,
        Value::Boolean(b) => Json::Bool(b),
        Value::Integer(i) => Json::from(i),
        Value::Number(n) => match whole_number(&Value::Number(n)) {
            Some(i) => Json::from(i),
            None if n.is_finite() => Json::from(n),
            None => return Err(bad("numbers must be finite".into())),
        },
        Value::String(s) => Json::String(
            s.to_str()
                .map_err(|_| bad("text must be UTF-8".into()))?
                .to_owned(),
        ),
        Value::Table(t) => {
            if depth >= MAX_DEPTH {
                return Err(bad(format!("tables nest at most {MAX_DEPTH} deep")));
            }
            env.budget.charge(lua, 1 + meter::table_slots(&t))?;
            let len = t.raw_len();
            let mut pairs = Vec::new();
            for pair in t.pairs::<Value, Value>() {
                pairs.push(pair?);
            }
            if len > 0 && pairs.len() == len {
                let mut items = vec![Json::Null; len];
                for (k, v) in pairs {
                    let i = whole_number(&k)
                        .and_then(|i| usize::try_from(i).ok())
                        .filter(|i| (1..=len).contains(i))
                        .ok_or_else(|| bad("a list is keyed 1, 2, 3 ...".into()))?;
                    items[i - 1] = lua_to_json(lua, env, v, depth + 1)?;
                }
                Json::Array(items)
            } else {
                let mut map = Fields::new();
                for (k, v) in pairs {
                    let Value::String(k) = k else {
                        return Err(bad(format!(
                            "table keys must be names, not a {}",
                            k.type_name()
                        )));
                    };
                    let k = k
                        .to_str()
                        .map_err(|_| bad("text must be UTF-8".into()))?
                        .to_owned();
                    map.insert(k, lua_to_json(lua, env, v, depth + 1)?);
                }
                Json::Object(map)
            }
        }
        other => {
            return Err(bad(format!(
                "a {} can't be stored in the world",
                other.type_name()
            )));
        }
    })
}

fn json_to_lua(lua: &Lua, value: &Json) -> mlua::Result<Value> {
    Ok(match value {
        Json::Null => Value::Nil,
        Json::Bool(b) => Value::Boolean(*b),
        Json::Number(n) => Value::Number(n.as_f64().unwrap_or(0.0)),
        Json::String(s) => Value::String(lua.create_string(s)?),
        Json::Array(items) => {
            let t = lua.create_table_with_capacity(items.len(), 0)?;
            for item in items {
                t.raw_push(json_to_lua(lua, item)?)?;
            }
            Value::Table(t)
        }
        Json::Object(map) => Value::Table(object_to_lua(lua, map)?),
    })
}

fn object_to_lua(lua: &Lua, map: &Fields) -> mlua::Result<Table> {
    let t = lua.create_table_with_capacity(0, map.len())?;
    for (k, v) in map {
        // An `id` field with no entity reads as "", so the field is still there.
        let v = match v {
            Json::Null => Value::String(lua.create_string("")?),
            other => json_to_lua(lua, other)?,
        };
        t.raw_set(k.as_str(), v)?;
    }
    Ok(t)
}

fn event_table(lua: &Lua, event: &Event<'_>) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("kind", event.kind)?;
    t.raw_set("tick", event.tick)?;
    t.raw_set("fact", event.fact)?;
    if let Some(source) = event.source {
        t.raw_set("source", source)?;
    }
    t.raw_set("payload", object_to_lua(lua, event.payload)?)?;
    Ok(t)
}
