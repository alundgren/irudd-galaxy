//! The `ctx` table a handler gets: one namespace per hook, built from the manifest's
//! `uses`, so undeclared namespaces and functions are simply absent.
//!
//! Reads see the world as it was when the call started. Writes become commands in the
//! call's [`Buffer`] under its one cause (`ctx:cause { ... }`; a second one is an
//! error), so a call's writes are one batch that applies whole or not at all. Inputs
//! are checked here so a bad write fails at the script's own line instead of as a
//! rejected batch later.
//!
//! Nothing crosses into Rust unchecked: string arguments are length-checked and table
//! arguments measured before anything is copied out of them, numbers must be whole and
//! in range where a whole number is expected (no `as` truncation or wrapping), and a
//! call's writes are capped at [`MAX_CALL_WRITES`] commands holding [`MAX_CALL_BYTES`].
//! Going over the quota aborts the whole call, even if the script catches the error.
//!
//! **Adding a hook function**: add its `FnDef` in `contract::api`, then a
//! `method(...)` line in [`build`] under its namespace.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::str::FromStr;

use galaxy_contract::api;
use galaxy_contract::events::{CounterKind, CounterScope, EventEnvelope};
use galaxy_contract::history::{FactDraft, HistoryQuery};
use galaxy_contract::pack::{ScriptDecl, ScriptValue, ValueType};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::History;
use mlua::{FromLuaMulti, IntoLuaMulti, Lua, LuaSerdeExt, LuaString, Scope, Table, Value};
use serde::Serialize;

use crate::meter::{Budget, FACTS_PER_STEP, table_slots};
use crate::vm::{RngSource, TEXT_CAP, by_reference, caller_location, whole_number};

mod happenings;
mod live;
mod parts;
mod progress;
mod space;
mod talk;

/// Most commands one handler call can write.
pub const MAX_CALL_WRITES: usize = 256;
/// Most bytes one handler call's commands can hold (their JSON size).
pub const MAX_CALL_BYTES: usize = 256 * 1024;
/// Longest ID, name or kind argument, in bytes.
const MAX_ARG: usize = 256;
/// Table arguments: at most this many entries, levels and bytes of string keys and
/// values, checked before anything is copied out of them.
const MAX_TABLE_ENTRIES: usize = 256;
const MAX_TABLE_DEPTH: usize = 4;
const MAX_TABLE_BYTES: usize = 16 * 1024;
/// Largest whole number a store keeps (2^53: exact in a Luau number).
const MAX_STORE_INT: i64 = 1 << 53;

/// The commands one handler call wrote, under its one cause.
#[derive(Default)]
pub(crate) struct Buffer {
    cause: Option<Cause>,
    commands: Vec<Command>,
    /// JSON size of `commands`.
    bytes: usize,
    /// The code of the last ctx error, to classify the abort.
    pub failed: Option<String>,
    /// Set when the call went over its write quota: the call aborts even if the script
    /// caught the error.
    pub over_quota: Option<String>,
}

impl Buffer {
    /// A buffer whose cause is already `seed`'s, holding its commands: a beat
    /// transition, so a beat callback's writes (with `beat.why`) join its batch.
    pub fn seeded(seed: CommandBatch) -> Self {
        Self {
            cause: Some(seed.cause),
            commands: seed.commands,
            ..Self::default()
        }
    }

    /// Whether the call wrote anything.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// The call's batch, if it wrote anything.
    pub fn into_batch(self) -> Option<CommandBatch> {
        match self.cause {
            Some(cause) if !self.commands.is_empty() => Some(CommandBatch {
                cause,
                commands: self.commands,
            }),
            _ => None,
        }
    }
}

/// Counts bytes written, to size a command without keeping its JSON.
struct Count(usize);

impl std::io::Write for Count {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0 = self.0.saturating_add(buf.len());
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn json_size(cmd: &Command) -> usize {
    let mut n = Count(0);
    let _ = serde_json::to_writer(&mut n, cmd);
    n.0
}

/// What one call can see and write.
pub(crate) struct CallEnv<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    pub pack: &'a PackId,
    pub revision: &'a str,
    pub opens: &'a [String],
    pub uses: &'a [String],
    pub script: &'a ScriptDecl,
    pub buffer: &'a RefCell<Buffer>,
    pub rng: &'a RefCell<&'a mut dyn RngSource>,
    pub rng_key: &'a str,
    /// The call's step budget: reading a table argument or history charges it.
    pub budget: &'a Budget,
}

/// Longest cause summary, in bytes.
const MAX_SUMMARY: usize = 200;
/// Longest string a store keeps, in bytes.
const MAX_STORE_STRING: usize = 1000;
/// Most facts one cause or line can cite.
const MAX_CITES: usize = 64;

impl CallEnv<'_> {
    /// A script-facing error at the caller's line, with a stable code.
    fn fail(&self, lua: &Lua, code: &str, msg: impl std::fmt::Display) -> mlua::Error {
        self.buffer.borrow_mut().failed = Some(code.to_owned());
        let at = caller_location(lua, 1)
            .map(|l| format!("{}:{}: ", l.file, l.line.unwrap_or(0)))
            .unwrap_or_default();
        mlua::Error::runtime(format!("{at}{msg}"))
    }

    /// A short string argument (an ID, a name, a kind), refused before it's copied if
    /// it's longer than [`MAX_ARG`].
    fn arg(&self, lua: &Lua, s: &LuaString) -> mlua::Result<String> {
        if s.as_bytes().len() > MAX_ARG {
            return Err(self.fail(
                lua,
                "too_long",
                format!("an ID or name argument is at most {MAX_ARG} bytes"),
            ));
        }
        Ok(s.to_string_lossy())
    }

    fn opt_arg(&self, lua: &Lua, s: Option<LuaString>) -> mlua::Result<Option<String>> {
        s.map(|s| self.arg(lua, &s)).transpose()
    }

    /// A whole number in `T`'s range. Fractions, out-of-range values and non-numbers
    /// are errors, never truncated or wrapped.
    fn int<T: TryFrom<i64>>(&self, lua: &Lua, v: &Value, what: &str) -> mlua::Result<T> {
        whole_number(v)
            .and_then(|n| T::try_from(n).ok())
            .ok_or_else(|| {
                self.fail(
                    lua,
                    "bad_number",
                    format!(
                        "`{what}` must be a whole number in range, got {}",
                        describe(v)
                    ),
                )
            })
    }

    /// Checks a table argument's shape and size before anything is copied out of it:
    /// string or number keys, at most [`MAX_TABLE_ENTRIES`] entries in all,
    /// [`MAX_TABLE_DEPTH`] levels and [`MAX_TABLE_BYTES`] of strings. Every violation
    /// gives the same message, so it doesn't depend on the order the table is walked.
    ///
    /// Each table walked charges the step budget for its slots first.
    fn measured(&self, lua: &Lua, t: &Table, what: &str) -> mlua::Result<()> {
        fn walk(
            env: &CallEnv<'_>,
            lua: &Lua,
            t: &Table,
            depth: usize,
            entries: &mut usize,
            bytes: &mut usize,
        ) -> mlua::Result<bool> {
            if depth > MAX_TABLE_DEPTH {
                return Ok(false);
            }
            env.budget.charge(lua, 1 + table_slots(t))?;
            for pair in t.pairs::<Value, Value>() {
                let Ok((k, v)) = pair else { return Ok(false) };
                *entries += 1;
                match &k {
                    Value::String(s) => *bytes += s.as_bytes().len(),
                    Value::Integer(_) | Value::Number(_) => {}
                    _ => return Ok(false),
                }
                let ok = match &v {
                    Value::String(s) => {
                        *bytes += s.as_bytes().len();
                        true
                    }
                    Value::Table(inner) => walk(env, lua, inner, depth + 1, entries, bytes)?,
                    other => !by_reference(other),
                };
                if !ok || *entries > MAX_TABLE_ENTRIES || *bytes > MAX_TABLE_BYTES {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        let (mut entries, mut bytes) = (0, 0);
        if walk(self, lua, t, 1, &mut entries, &mut bytes)? {
            Ok(())
        } else {
            Err(self.fail(
                lua,
                "too_large",
                format!(
                    "{what}: a table argument holds plain values keyed by strings or numbers, \
                     at most {MAX_TABLE_ENTRIES} entries, {MAX_TABLE_DEPTH} levels deep and \
                     {} KiB of text",
                    MAX_TABLE_BYTES / 1024
                ),
            ))
        }
    }

    /// Deserializes a measured table argument.
    fn decode<T: serde::de::DeserializeOwned>(
        &self,
        lua: &Lua,
        t: Table,
        what: &str,
    ) -> mlua::Result<T> {
        self.measured(lua, &t, what)?;
        lua.from_value(Value::Table(t))
            .map_err(|e| self.fail(lua, "bad_argument", format!("{what}: {e}")))
    }

    fn id<T: FromStr<Err = IdError>>(&self, lua: &Lua, s: &LuaString) -> mlua::Result<T> {
        let s = self.arg(lua, s)?;
        s.parse().map_err(|e: IdError| self.fail(lua, "bad_id", e))
    }

    /// Adds a command under the call's cause `why`. Over the call's write quota, the
    /// whole call is aborted.
    fn push(&self, lua: &Lua, why: &Table, cmd: Command) -> mlua::Result<()> {
        let handle = why
            .raw_get::<Value>("handle")
            .ok()
            .as_ref()
            .and_then(whole_number);
        let mut b = self.buffer.borrow_mut();
        if b.cause.is_none() || handle != Some(1) {
            drop(b);
            return Err(self.fail(
                lua,
                "bad_cause",
                "every write needs the cause from `ctx:cause { summary = ..., cites = { ... } }`",
            ));
        }
        if let Some(msg) = b.over_quota.clone() {
            drop(b);
            return Err(self.fail(lua, "over_quota", msg));
        }
        let bytes = b.bytes.saturating_add(json_size(&cmd));
        if b.commands.len() >= MAX_CALL_WRITES || bytes > MAX_CALL_BYTES {
            let msg = format!(
                "a handler call writes at most {MAX_CALL_WRITES} commands holding {} KiB; \
                 its writes were discarded",
                MAX_CALL_BYTES / 1024
            );
            b.over_quota = Some(msg.clone());
            drop(b);
            return Err(self.fail(lua, "over_quota", msg));
        }
        b.bytes = bytes;
        b.commands.push(cmd);
        Ok(())
    }

    fn cites(&self, lua: &Lua, ids: Vec<String>) -> mlua::Result<Vec<FactId>> {
        if ids.len() > MAX_CITES {
            return Err(self.fail(
                lua,
                "too_many_cites",
                format!("cite at most {MAX_CITES} facts"),
            ));
        }
        ids.iter()
            .map(|s| {
                let id: FactId = s
                    .parse()
                    .map_err(|e: IdError| self.fail(lua, "bad_id", e))?;
                if self.history.fact(id).is_none() {
                    return Err(self.fail(
                        lua,
                        "unknown_fact",
                        format!("`{id}` isn't a history fact"),
                    ));
                }
                Ok(id)
            })
            .collect()
    }

    fn station(&self, lua: &Lua, s: &LuaString) -> mlua::Result<StationId> {
        let id: StationId = self.id(lua, s)?;
        if !self.world.stations.contains(&id) {
            return Err(self.fail(lua, "unknown_id", format!("no station `{id}`")));
        }
        Ok(id)
    }

    fn faction(&self, lua: &Lua, s: &LuaString) -> mlua::Result<FactionId> {
        let id: FactionId = self.id(lua, s)?;
        if !self.world.factions.contains(&id) {
            return Err(self.fail(lua, "unknown_id", format!("no faction `{id}`")));
        }
        Ok(id)
    }

    fn may(&self, need: &str) -> bool {
        need.is_empty() || self.uses.iter().any(|u| u == need)
    }

    /// New records a pack creates live are namespaced by the pack's first segment
    /// (`core` itself, and only `core`, names its records freely).
    fn check_new_id(&self, lua: &Lua, id: &str, exists: bool) -> mlua::Result<()> {
        let ns = self.pack.as_str().split('.').next().unwrap_or("");
        if exists || self.pack.as_str() == "core" || id.starts_with(&format!("{ns}.")) {
            Ok(())
        } else {
            Err(self.fail(
                lua,
                "unnamespaced_id",
                format!("new records from this pack need IDs starting with `{ns}.`, got `{id}`"),
            ))
        }
    }
}

/// A value for an error message, without addresses.
fn describe(v: &Value) -> String {
    match v {
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        other => format!("a {}", other.type_name()),
    }
}

fn ser_options() -> mlua::serde::SerializeOptions {
    let mut o = mlua::serde::SerializeOptions::new();
    o.set_array_metatable = false;
    o.serialize_none_to_null = false;
    o.serialize_unit_to_null = false;
    o
}

pub(crate) fn to_lua<T: Serialize + ?Sized>(lua: &Lua, v: &T) -> mlua::Result<Value> {
    lua.to_value_with(v, ser_options())
}

/// Checks a table only has the given keys (Luau tables don't reject typos). The error
/// lists every unexpected key, sorted, so it reads the same on every run.
fn only_keys(
    env: &CallEnv<'_>,
    lua: &Lua,
    t: &Table,
    what: &str,
    allowed: &[&str],
) -> mlua::Result<()> {
    env.measured(lua, t, what)?;
    let mut unexpected = BTreeSet::new();
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        let name = k.as_string().map(|s| s.to_string_lossy());
        if !name.as_deref().is_some_and(|n| allowed.contains(&n)) {
            unexpected.insert(name.unwrap_or_else(|| describe(&k)));
        }
    }
    if unexpected.is_empty() {
        return Ok(());
    }
    Err(env.fail(
        lua,
        "bad_argument",
        format!(
            "{what}: unexpected key {} (expected {})",
            unexpected
                .iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", "),
            allowed.join(", ")
        ),
    ))
}

/// The event table a handler gets: the event's fields plus `fact`, `tick`, `cause`.
pub(crate) fn event_table(lua: &Lua, e: &EventEnvelope) -> mlua::Result<Table> {
    let Value::Table(t) = to_lua(lua, &e.event)? else {
        return Err(mlua::Error::runtime("event did not serialize to a table"));
    };
    t.raw_set("fact", e.fact.to_string())?;
    t.raw_set("tick", e.tick.0)?;
    t.raw_set("cause", e.cause.to_string())?;
    Ok(t)
}

const QUERY_KEYS: &[&str] = &[
    "kind", "cause", "ship", "station", "ware", "rock", "npc", "faction", "region", "other",
    "since", "until", "after", "limit",
];

/// A scoped method: the first argument is `self`.
fn m<'scope, 'env: 'scope, A, R>(
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
    f: impl Fn(&Lua, &'env CallEnv<'env>, A) -> mlua::Result<R> + 'scope,
) -> mlua::Result<mlua::Function>
where
    A: FromLuaMulti,
    R: IntoLuaMulti,
{
    scope.create_function(move |lua, (_self, args): (Value, A)| f(lua, env, args))
}

/// Registers `ns:name(...)` if the script may use it.
fn method(
    env: &CallEnv<'_>,
    ns: &Table,
    ns_name: &str,
    name: &str,
    f: mlua::Function,
) -> mlua::Result<()> {
    let def =
        api::function(ns_name, name).expect("every ctx method is in contract::api::FUNCTIONS");
    if env.may(def.needs) {
        ns.raw_set(name, f)?;
    }
    Ok(())
}

/// Builds `ctx` for one call.
pub(crate) fn build<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Table> {
    let ctx = lua.create_table()?;
    ctx.raw_set("pack", env.pack.as_str())?;
    ctx.raw_set("script", env.script.id.as_str())?;

    // Registers `ns:name(...)` if the script may use it. `ns` is the namespace table.
    let method = |ns: &Table, ns_name: &str, name: &str, f: mlua::Function| -> mlua::Result<()> {
        let def =
            api::function(ns_name, name).expect("every ctx method is in contract::api::FUNCTIONS");
        if env.may(def.needs) {
            ns.raw_set(name, f)?;
        }
        Ok(())
    };
    // ctx:cause (once per call)
    ctx.raw_set(
        "cause",
        m(scope, env, |lua, env, spec: Table| {
            only_keys(env, lua, &spec, "ctx:cause", &["summary", "cites"])?;
            if env.buffer.borrow().cause.is_some() {
                return Err(env.fail(
                    lua,
                    "second_cause",
                    "a handler call makes one cause: everything it writes applies together \
                     or not at all, so pass the same `why` to every write",
                ));
            }
            let summary: String = spec
                .get("summary")
                .map_err(|_| env.fail(lua, "bad_cause", "ctx:cause needs a `summary` string"))?;
            if summary.trim().is_empty() || summary.len() > MAX_SUMMARY {
                return Err(env.fail(
                    lua,
                    "bad_cause",
                    format!("a cause summary is one line of 1 to {MAX_SUMMARY} bytes"),
                ));
            }
            let cites = env.cites(
                lua,
                spec.get::<Option<Vec<String>>>("cites")?
                    .unwrap_or_default(),
            )?;
            let cause = Cause::script(env.pack.clone(), env.revision, summary.clone())
                .citing(cites.clone());
            env.buffer.borrow_mut().cause = Some(cause);
            let t = lua.create_table()?;
            t.raw_set("summary", summary)?;
            t.raw_set(
                "cites",
                cites.iter().map(ToString::to_string).collect::<Vec<_>>(),
            )?;
            t.raw_set("handle", 1)?;
            t.set_readonly(true);
            Ok(t)
        })?,
    )?;

    // ctx.rng
    let rng = lua.create_table()?;
    method(
        &rng,
        "rng",
        "int",
        m(scope, env, |lua, env, (lo, hi): (Value, Value)| {
            let lo: i64 = env.int(lua, &lo, "lo")?;
            let hi: i64 = env.int(lua, &hi, "hi")?;
            if lo > hi {
                return Err(env.fail(lua, "bad_argument", "ctx.rng:int(lo, hi) needs lo <= hi"));
            }
            // In i128 so no pair of i64 bounds overflows: span is 1..=2^64.
            let span = u128::try_from(i128::from(hi) - i128::from(lo) + 1).unwrap_or(u128::MAX);
            let draw = u128::from(env.rng.borrow_mut().stream(env.rng_key).next_u64()) % span;
            let n = i128::from(lo) + i128::try_from(draw).unwrap_or(0);
            Ok(i64::try_from(n).unwrap_or(hi))
        })?,
    )?;
    method(
        &rng,
        "rng",
        "float",
        m(scope, env, |_, env, (): ()| {
            Ok(f64::from(env.rng.borrow_mut().stream(env.rng_key).next_u32()) / 4_294_967_296.0)
        })?,
    )?;
    rng.set_readonly(true);
    ctx.raw_set("rng", rng)?;

    // ctx.store (only for scripts that declare stores)
    if !env.script.stores.is_empty() {
        let store = lua.create_table()?;
        fn declared(
            env: &CallEnv<'_>,
            lua: &Lua,
            store: &str,
            key: &str,
        ) -> mlua::Result<ValueType> {
            env.script
                .stores
                .get(store)
                .and_then(|k| k.get(key))
                .copied()
                .ok_or_else(|| {
                    env.fail(
                        lua,
                        "undeclared_store",
                        format!(
                            "`{store}.{key}` isn't declared in this script's manifest `stores`"
                        ),
                    )
                })
        }
        method(
            &store,
            "store",
            "get",
            m(
                scope,
                env,
                |lua, env, (store, key): (LuaString, LuaString)| {
                    let (store, key) = (env.arg(lua, &store)?, env.arg(lua, &key)?);
                    declared(env, lua, &store, &key)?;
                    let v = env
                        .world
                        .packs
                        .get(env.pack)
                        .and_then(|p| p.stores.get(&env.script.id))
                        .and_then(|s| s.get(&store))
                        .and_then(|s| s.get(&key));
                    match v {
                        None => Ok(Value::Nil),
                        Some(ScriptValue::Boolean(b)) => Ok(Value::Boolean(*b)),
                        // Stores keep integers within 2^53, so this is exact.
                        Some(ScriptValue::Integer(i)) => Ok(Value::Number(*i as f64)),
                        Some(ScriptValue::Number(n)) => Ok(Value::Number(*n)),
                        Some(ScriptValue::String(s)) => to_lua(lua, s),
                    }
                },
            )?,
        )?;
        method(
            &store,
            "store",
            "set",
            m(
                scope,
                env,
                |lua, env, (store, key, value, why): (LuaString, LuaString, Value, Table)| {
                    let (store, key) = (env.arg(lua, &store)?, env.arg(lua, &key)?);
                    let ty = declared(env, lua, &store, &key)?;
                    let wrong = |what: &str| {
                        env.fail(
                            lua,
                            "bad_store_value",
                            format!("`{store}.{key}` is declared `{ty:?}`, got {what}"),
                        )
                    };
                    let v = match (ty, &value) {
                        (_, Value::Nil) => None,
                        (ValueType::Boolean, Value::Boolean(b)) => Some(ScriptValue::Boolean(*b)),
                        (ValueType::Integer, Value::Integer(_) | Value::Number(_)) => {
                            match whole_number(&value) {
                                Some(i) if i.abs() <= MAX_STORE_INT => {
                                    Some(ScriptValue::Integer(i))
                                }
                                _ => {
                                    return Err(wrong(&format!(
                                        "{} (whole numbers up to 2^53 only)",
                                        describe(&value)
                                    )));
                                }
                            }
                        }
                        (ValueType::Number, Value::Number(n)) if n.is_finite() => {
                            Some(ScriptValue::Number(*n))
                        }
                        (ValueType::Number, Value::Integer(i)) if i.abs() <= MAX_STORE_INT => {
                            // Within 2^53, so exact.
                            Some(ScriptValue::Number(*i as f64))
                        }
                        (ValueType::String, Value::String(s)) => {
                            // Measured before it's copied.
                            if s.as_bytes().len() > MAX_STORE_STRING {
                                return Err(wrong("a string over 1000 bytes"));
                            }
                            Some(ScriptValue::String(s.to_string_lossy()))
                        }
                        (_, other) => return Err(wrong(&describe(other))),
                    };
                    env.push(
                        lua,
                        &why,
                        Command::SetScriptValue {
                            pack: env.pack.clone(),
                            script: env.script.id.clone(),
                            store,
                            key,
                            value: v,
                        },
                    )
                },
            )?,
        )?;
        store.set_readonly(true);
        ctx.raw_set("store", store)?;
    }

    // ctx.counters
    let counters = lua.create_table()?;
    method(
        &counters,
        "counters",
        "get",
        m(
            scope,
            env,
            |lua, env, (kind, scope_arg): (LuaString, Option<Table>)| {
                let kind = env.arg(lua, &kind)?;
                let kind: CounterKind = serde_json::from_value(serde_json::Value::String(
                    kind.clone(),
                ))
                .map_err(|_| {
                    env.fail(
                        lua,
                        "bad_argument",
                        format!("`{kind}` isn't an activity counter"),
                    )
                })?;
                let scope = match scope_arg {
                    None => CounterScope::Player,
                    Some(t) => {
                        only_keys(
                            env,
                            lua,
                            &t,
                            "ctx.counters:get scope",
                            &["station", "region"],
                        )?;
                        match (
                            t.get::<Option<LuaString>>("station")?,
                            t.get::<Option<LuaString>>("region")?,
                        ) {
                            (Some(s), None) => CounterScope::Station {
                                station: env.id(lua, &s)?,
                            },
                            (None, Some(r)) => CounterScope::Region {
                                region: env.id(lua, &r)?,
                            },
                            (None, None) => CounterScope::Player,
                            _ => {
                                return Err(env.fail(
                                    lua,
                                    "bad_argument",
                                    "give `station` or `region`, not both",
                                ));
                            }
                        }
                    }
                };
                Ok(env.world.counters.get(kind, &scope))
            },
        )?,
    )?;

    // ctx.history
    let history = lua.create_table()?;
    // A query reads every fact, so it charges the budget for them.
    let query = |env: &CallEnv<'_>, lua: &Lua, q: Table| -> mlua::Result<HistoryQuery> {
        only_keys(env, lua, &q, "history query", QUERY_KEYS)?;
        let facts = u64::try_from(env.history.len()).unwrap_or(u64::MAX);
        env.budget.charge(lua, 1 + facts / FACTS_PER_STEP)?;
        lua.from_value(Value::Table(q))
            .map_err(|e| env.fail(lua, "bad_query", e))
    };
    method(
        &history,
        "history",
        "count",
        m(scope, env, move |lua, env, q: Table| {
            Ok(env.history.count(&query(env, lua, q)?))
        })?,
    )?;
    method(
        &history,
        "history",
        "sum",
        m(scope, env, move |lua, env, q: Table| {
            env.history.sum(&query(env, lua, q)?).ok_or_else(|| {
                env.fail(
                    lua,
                    "bad_number",
                    "the matching facts' amounts add up to more than a whole number can hold",
                )
            })
        })?,
    )?;
    method(
        &history,
        "history",
        "find",
        m(scope, env, move |lua, env, q: Table| {
            let mut q = query(env, lua, q)?;
            q.limit = Some(
                q.limit
                    .unwrap_or(api::HISTORY_FIND_CAP)
                    .min(api::HISTORY_FIND_CAP),
            );
            to_lua(lua, &env.history.find(&q))
        })?,
    )?;
    method(
        &history,
        "history",
        "last",
        m(scope, env, move |lua, env, q: Table| {
            to_lua(lua, &env.history.last(&query(env, lua, q)?))
        })?,
    )?;
    method(
        &history,
        "history",
        "fact",
        m(scope, env, |lua, env, id: LuaString| {
            let id: FactId = env
                .arg(lua, &id)?
                .parse()
                .map_err(|e: IdError| env.fail(lua, "bad_id", e))?;
            to_lua(lua, &env.history.fact(id))
        })?,
    )?;
    method(
        &history,
        "history",
        "record",
        m(scope, env, |lua, env, (fact, why): (Table, Table)| {
            let draft: FactDraft = env.decode(lua, fact, "ctx.history:record")?;
            // History is kept for good: a note is at most 1 KiB, like any script text.
            if draft.note.len() > TEXT_CAP {
                return Err(env.fail(
                    lua,
                    "too_long",
                    format!("a fact's note is at most {TEXT_CAP} bytes"),
                ));
            }
            // Recorded kinds live in the pack's own namespace, its whole ID
            // (`gm.kettle_growth.stage_built`); bare names are the engine's event kinds,
            // and another pack's kinds are its own.
            let ns = env.pack.as_str();
            let Some(name) = draft
                .kind
                .as_str()
                .strip_prefix(ns)
                .and_then(|k| k.strip_prefix('.'))
            else {
                return Err(env.fail(
                    lua,
                    "unprefixed_fact_kind",
                    format!(
                        "record kinds in this pack's namespace, e.g. `{ns}.stage_built`; got `{}`",
                        draft.kind
                    ),
                ));
            };
            // These report a checked change, so only the hook that makes it records
            // them (`galaxy_contract::needs`).
            // (Kinds match by their last segment when lines cite them.)
            let last = name.rsplit('.').next().unwrap_or(name);
            if galaxy_contract::needs::PROTECTED_FACTS.contains(&last) {
                let hook = if last == "canon_opened" {
                    "ctx.canon:open"
                } else {
                    "ctx.mechanics:teach"
                };
                return Err(env.fail(
                    lua,
                    "protected_fact_kind",
                    format!("`{}` is recorded only by `{hook}`", draft.kind),
                ));
            }
            env.push(
                lua,
                &why,
                Command::RecordFact {
                    fact: Box::new(draft),
                },
            )
        })?,
    )?;

    // ctx.stations
    let stations = lua.create_table()?;
    method(
        &stations,
        "stations",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.stations.get(id.as_str()))
        })?,
    )?;
    method(
        &stations,
        "stations",
        "list",
        m(scope, env, |_, env, (): ()| {
            Ok(env
                .world
                .stations
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        &stations,
        "stations",
        "stage",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.station(lua, &id)?;
            Ok(env.world.stations.get(&id).map_or(0, |s| s.build.stage))
        })?,
    )?;
    method(
        &stations,
        "stations",
        "parts",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.station(lua, &id)?;
            Ok(env
                .world
                .stations
                .get(&id)
                .map(|s| {
                    s.build
                        .parts
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default())
        })?,
    )?;
    method(
        &stations,
        "stations",
        "set_stage",
        m(
            scope,
            env,
            |lua, env, (id, stage, why): (LuaString, Value, Table)| {
                let station = env.station(lua, &id)?;
                let stage: u32 = env.int(lua, &stage, "stage")?;
                env.push(lua, &why, Command::SetStationStage { station, stage })
            },
        )?,
    )?;
    for (name, attached) in [("attach", true), ("detach", false)] {
        method(
            &stations,
            "stations",
            name,
            m(
                scope,
                env,
                move |lua, env, (id, part, why): (LuaString, LuaString, Table)| {
                    let station = env.station(lua, &id)?;
                    let part: PartId = env.id(lua, &part)?;
                    env.push(
                        lua,
                        &why,
                        Command::SetStationPart {
                            station,
                            part,
                            attached,
                        },
                    )
                },
            )?,
        )?;
    }
    method(
        &stations,
        "stations",
        "set_owner",
        m(
            scope,
            env,
            |lua, env, (id, owner, why): (LuaString, Option<LuaString>, Table)| {
                let station = env.station(lua, &id)?;
                let owner = owner.map(|o| env.faction(lua, &o)).transpose()?;
                env.push(lua, &why, Command::SetStationOwner { station, owner })
            },
        )?,
    )?;

    // ctx.factions
    let factions = lua.create_table()?;
    method(
        &factions,
        "factions",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.factions.get(id.as_str()))
        })?,
    )?;
    method(
        &factions,
        "factions",
        "list",
        m(scope, env, |_, env, (): ()| {
            Ok(env
                .world
                .factions
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        &factions,
        "factions",
        "standing",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.faction(lua, &id)?;
            Ok(env.world.factions.get(&id).map_or(0, |f| f.standing))
        })?,
    )?;
    method(
        &factions,
        "factions",
        "adjust_standing",
        m(
            scope,
            env,
            |lua, env, (id, delta, why): (LuaString, Value, Table)| {
                let faction = env.faction(lua, &id)?;
                let delta: i32 = env.int(lua, &delta, "delta")?;
                env.push(lua, &why, Command::AdjustStanding { faction, delta })
            },
        )?,
    )?;

    // ctx.npcs
    let npcs = lua.create_table()?;
    method(
        &npcs,
        "npcs",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.npcs.get(id.as_str()))
        })?,
    )?;
    method(
        &npcs,
        "npcs",
        "list",
        m(scope, env, |_, env, (): ()| {
            Ok(env
                .world
                .npcs
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        &npcs,
        "npcs",
        "at",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            let here: Vec<&Npc> = env
                .world
                .npcs
                .values()
                .filter(|n| n.location.as_ref() == Some(&station))
                .collect();
            to_lua(lua, &here)
        })?,
    )?;
    method(
        &npcs,
        "npcs",
        "put",
        m(scope, env, |lua, env, (npc, why): (Table, Table)| {
            let npc: Npc = env.decode(lua, npc, "ctx.npcs:put")?;
            env.check_new_id(lua, npc.id.as_str(), env.world.npcs.contains(&npc.id))?;
            env.push(
                lua,
                &why,
                Command::PutNpc {
                    id: npc.id.clone(),
                    value: Some(Box::new(npc)),
                },
            )
        })?,
    )?;
    method(
        &npcs,
        "npcs",
        "move",
        m(
            scope,
            env,
            |lua, env, (id, station, why): (LuaString, Option<LuaString>, Table)| {
                let id: NpcId = env.id(lua, &id)?;
                let mut npc = env
                    .world
                    .npcs
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| env.fail(lua, "unknown_id", format!("no npc `{id}`")))?;
                npc.location = station.map(|s| env.station(lua, &s)).transpose()?;
                env.push(
                    lua,
                    &why,
                    Command::PutNpc {
                        id,
                        value: Some(Box::new(npc)),
                    },
                )
            },
        )?,
    )?;

    // ctx.rumors
    let rumors = lua.create_table()?;
    method(
        &rumors,
        "rumors",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.rumors.get(id.as_str()))
        })?,
    )?;
    method(
        &rumors,
        "rumors",
        "list",
        m(scope, env, |_, env, (): ()| {
            Ok(env
                .world
                .rumors
                .keys()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        &rumors,
        "rumors",
        "put",
        m(scope, env, |lua, env, (rumor, why): (Table, Table)| {
            let rumor: Rumor = env.decode(lua, rumor, "ctx.rumors:put")?;
            env.check_new_id(lua, rumor.id.as_str(), env.world.rumors.contains(&rumor.id))?;
            env.push(
                lua,
                &why,
                Command::PutRumor {
                    id: rumor.id.clone(),
                    value: Some(Box::new(rumor)),
                },
            )
        })?,
    )?;
    let rumor_edit = |env: &CallEnv<'_>, lua: &Lua, id: &LuaString| -> mlua::Result<Rumor> {
        let id: RumorId = env.id(lua, id)?;
        env.world
            .rumors
            .get(&id)
            .cloned()
            .ok_or_else(|| env.fail(lua, "unknown_id", format!("no rumor `{id}`")))
    };
    method(
        &rumors,
        "rumors",
        "set_truth",
        m(
            scope,
            env,
            move |lua, env, (id, is_true, why): (LuaString, bool, Table)| {
                let mut r = rumor_edit(env, lua, &id)?;
                r.is_true = is_true;
                env.push(
                    lua,
                    &why,
                    Command::PutRumor {
                        id: r.id.clone(),
                        value: Some(Box::new(r)),
                    },
                )
            },
        )?,
    )?;
    method(
        &rumors,
        "rumors",
        "spread",
        m(
            scope,
            env,
            move |lua, env, (id, station, why): (LuaString, LuaString, Table)| {
                let mut r = rumor_edit(env, lua, &id)?;
                r.heard_at.insert(env.station(lua, &station)?);
                env.push(
                    lua,
                    &why,
                    Command::PutRumor {
                        id: r.id.clone(),
                        value: Some(Box::new(r)),
                    },
                )
            },
        )?,
    )?;

    // ctx.territory
    let territory = lua.create_table()?;
    method(
        &territory,
        "territory",
        "owner",
        m(scope, env, |lua, env, region: LuaString| {
            let id: RegionId = env.id(lua, &region)?;
            let r = env
                .world
                .regions
                .get(&id)
                .ok_or_else(|| env.fail(lua, "unknown_id", format!("no region `{id}`")))?;
            Ok(r.owner.as_ref().map(ToString::to_string))
        })?,
    )?;
    method(
        &territory,
        "territory",
        "station_owner",
        m(scope, env, |lua, env, station: LuaString| {
            let id = env.station(lua, &station)?;
            Ok(env
                .world
                .stations
                .get(&id)
                .and_then(|s| s.owner.as_ref())
                .map(ToString::to_string))
        })?,
    )?;
    method(
        &territory,
        "territory",
        "set_owner",
        m(
            scope,
            env,
            |lua, env, (region, owner, why): (LuaString, Option<LuaString>, Table)| {
                let id: RegionId = env.id(lua, &region)?;
                let mut r = env
                    .world
                    .regions
                    .get(&id)
                    .cloned()
                    .ok_or_else(|| env.fail(lua, "unknown_id", format!("no region `{id}`")))?;
                r.owner = owner.map(|o| env.faction(lua, &o)).transpose()?;
                env.push(
                    lua,
                    &why,
                    Command::PutRegion {
                        id,
                        value: Some(Box::new(r)),
                    },
                )
            },
        )?,
    )?;
    method(
        &territory,
        "territory",
        "set_station_owner",
        m(
            scope,
            env,
            |lua, env, (station, owner, why): (LuaString, Option<LuaString>, Table)| {
                let station = env.station(lua, &station)?;
                let owner = owner.map(|o| env.faction(lua, &o)).transpose()?;
                env.push(lua, &why, Command::SetStationOwner { station, owner })
            },
        )?,
    )?;

    // ctx.market (stock lines; levers are W3's extension point)
    let market = lua.create_table()?;
    method(
        &market,
        "market",
        "line",
        m(
            scope,
            env,
            |lua, env, (station, ware): (LuaString, LuaString)| {
                let (station, ware) = (env.arg(lua, &station)?, env.arg(lua, &ware)?);
                let line = env
                    .world
                    .markets
                    .get(station.as_str())
                    .and_then(|m| m.lines.get(ware.as_str()));
                to_lua(lua, &line)
            },
        )?,
    )?;
    method(
        &market,
        "market",
        "lines",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            match env.world.markets.get(&station) {
                Some(m) => to_lua(lua, &m.lines),
                None => Ok(Value::Table(lua.create_table()?)),
            }
        })?,
    )?;
    method(
        &market,
        "market",
        "pulses",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            let milli = env
                .world
                .economy
                .stations
                .get(&station)
                .map_or(0, |t| t.pulses_milli);
            Ok(milli as f64 / 1000.0)
        })?,
    )?;
    method(
        &market,
        "market",
        "add_line",
        m(
            scope,
            env,
            |lua, env, (station, ware, line, why): (LuaString, LuaString, Table, Table)| {
                let station = env.station(lua, &station)?;
                let ware: WareId = env.id(lua, &ware)?;
                if !env.world.wares.contains(&ware) {
                    return Err(env.fail(lua, "unknown_id", format!("no ware `{ware}`")));
                }
                let m = env.world.markets.get(&station).ok_or_else(|| {
                    env.fail(lua, "unknown_id", format!("`{station}` has no market"))
                })?;
                if m.lines.contains_key(&ware) {
                    return Err(env.fail(lua, "line_exists", format!("`{station}` already trades `{ware}`; changing it is a market lever (W3)")));
                }
                let line: MarketLine = env.decode(lua, line, "ctx.market:add_line")?;
                if !(line.min_price <= line.price && line.price <= line.max_price) {
                    return Err(env.fail(
                        lua,
                        "price_out_of_band",
                        "a line's price must sit inside min_price..=max_price",
                    ));
                }
                env.push(
                    lua,
                    &why,
                    Command::SetMarketLine {
                        station,
                        ware,
                        line: Some(line),
                    },
                )
            },
        )?,
    )?;
    method(
        &market,
        "market",
        "remove_line",
        m(
            scope,
            env,
            |lua, env, (station, ware, why): (LuaString, LuaString, Table)| {
                let station = env.station(lua, &station)?;
                let ware: WareId = env.id(lua, &ware)?;
                if !env
                    .world
                    .markets
                    .get(&station)
                    .is_some_and(|m| m.lines.contains_key(&ware))
                {
                    return Err(env.fail(
                        lua,
                        "unknown_id",
                        format!("`{station}` doesn't trade `{ware}`"),
                    ));
                }
                env.push(
                    lua,
                    &why,
                    Command::SetMarketLine {
                        station,
                        ware,
                        line: None,
                    },
                )
            },
        )?,
    )?;

    // ctx.talk, ctx.map, ctx.chronicle
    let say = |env: &CallEnv<'_>,
               lua: &Lua,
               channel: Channel,
               spec: Table,
               why: Table|
     -> mlua::Result<()> {
        only_keys(
            env,
            lua,
            &spec,
            channel.as_str(),
            &["speaker", "place", "text", "cites", "choices"],
        )?;
        let cites: Option<Vec<String>> = spec.get("cites")?;
        let Some(cites) = cites.filter(|c| !c.is_empty()) else {
            return Err(env.fail(
                lua,
                "uncited_line",
                "every line must cite at least one history fact ID (`cites = { ev.fact }`)",
            ));
        };
        let speaker = env
            .opt_arg(lua, spec.get("speaker")?)?
            .map(|s| -> mlua::Result<NpcId> {
                let id: NpcId = s.parse().map_err(|e: IdError| env.fail(lua, "bad_id", e))?;
                if !env.world.npcs.contains(&id) {
                    return Err(env.fail(lua, "unknown_id", format!("no npc `{id}`")));
                }
                Ok(id)
            })
            .transpose()?;
        let place: Option<LuaString> = spec.get("place")?;
        let place = place.map(|p| env.station(lua, &p)).transpose()?;
        let line = TalkLine {
            channel,
            speaker,
            place,
            text: spec.get::<Option<String>>("text")?.unwrap_or_default(),
            cites: env.cites(lua, cites)?,
            about: Vec::new(),
            choices: match spec.get::<Option<Table>>("choices")? {
                Some(t) => env.decode(lua, t, "choices")?,
                None => Vec::new(),
            },
        };
        line.validate()
            .map_err(|e| env.fail(lua, "illegible_line", e))?;
        env.push(
            lua,
            &why,
            Command::Say {
                line: Box::new(line),
            },
        )
    };
    let talk = lua.create_table()?;
    talk::add(lua, scope, env, &talk)?;
    method(
        &talk,
        "talk",
        "offer",
        m(scope, env, move |lua, env, (spec, why): (Table, Table)| {
            say(env, lua, Channel::DockTalk, spec, why)
        })?,
    )?;
    let map = lua.create_table()?;
    method(
        &map,
        "map",
        "note",
        m(scope, env, move |lua, env, (spec, why): (Table, Table)| {
            say(env, lua, Channel::MapNote, spec, why)
        })?,
    )?;
    let chronicle = lua.create_table()?;
    method(
        &chronicle,
        "chronicle",
        "write",
        m(scope, env, move |lua, env, (spec, why): (Table, Table)| {
            say(env, lua, Channel::Chronicle, spec, why)
        })?,
    )?;

    // Attach the namespaces that have anything in them.
    let present: BTreeSet<&str> = api::FUNCTIONS
        .iter()
        .filter(|d| env.may(d.needs))
        .map(|d| d.ns)
        .collect();
    progress::add_levers(lua, scope, env, &market)?;
    let mut tables = vec![
        ("counters", counters),
        ("history", history),
        ("stations", stations),
        ("factions", factions),
        ("npcs", npcs),
        ("rumors", rumors),
        ("territory", territory),
        ("market", market),
        ("talk", talk),
        ("map", map),
        ("chronicle", chronicle),
    ];
    tables.extend(progress::namespaces(lua, scope, env)?);
    tables.push(("space", space::namespace(lua, scope, env)?));
    tables.push(("parts", parts::namespace(lua, scope, env)?));
    tables.push(("happenings", happenings::namespace(lua, scope, env)?));
    tables.extend(live::namespaces(lua, scope, env)?);
    for (name, t) in tables {
        if present.contains(name) {
            t.set_readonly(true);
            ctx.raw_set(name, t)?;
        }
    }
    ctx.set_readonly(true);
    Ok(ctx)
}
