//! The `ctx` table a handler gets: one namespace per hook, built from the manifest's
//! `uses`, so undeclared namespaces and functions are simply absent.
//!
//! Reads see the world as it was when the call started. Writes become commands in the
//! call's [`Buffer`], grouped by the cause they were made with (`ctx:cause { ... }`).
//! Nothing applies unless the whole call succeeds. Inputs are checked here so a bad
//! write fails at the script's own line instead of as a rejected batch later.
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
use mlua::{FromLuaMulti, IntoLuaMulti, Lua, LuaSerdeExt, Scope, Table, Value};
use serde::Serialize;

use crate::vm::{RngSource, caller_location};

mod progress;

/// Commands written by one handler call, by cause.
#[derive(Default)]
pub(crate) struct Buffer {
    causes: Vec<(Cause, Vec<Command>)>,
    /// The code of the last ctx error, to classify the abort.
    pub failed: Option<String>,
}

impl Buffer {
    /// A buffer whose cause handle 1 is `seed` (a beat transition's batch).
    pub fn seeded(seed: CommandBatch) -> Self {
        Self {
            causes: vec![(seed.cause, seed.commands)],
            failed: None,
        }
    }

    /// Whether any write was made.
    pub fn is_empty(&self) -> bool {
        self.causes.iter().all(|(_, c)| c.is_empty())
    }

    pub fn into_batches(self) -> Vec<CommandBatch> {
        self.causes
            .into_iter()
            .filter(|(_, c)| !c.is_empty())
            .map(|(cause, commands)| CommandBatch { cause, commands })
            .collect()
    }
}

/// What one call can see and write.
pub(crate) struct CallEnv<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    pub pack: &'a PackId,
    pub revision: &'a str,
    pub uses: &'a [String],
    pub script: &'a ScriptDecl,
    pub buffer: &'a RefCell<Buffer>,
    pub rng: &'a RefCell<&'a mut dyn RngSource>,
    pub rng_key: &'a str,
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

    fn id<T: FromStr<Err = IdError>>(&self, lua: &Lua, s: &str) -> mlua::Result<T> {
        s.parse().map_err(|e: IdError| self.fail(lua, "bad_id", e))
    }

    /// Adds a command under the cause handle `why`.
    fn push(&self, lua: &Lua, why: &Table, cmd: Command) -> mlua::Result<()> {
        let handle: i64 = why.raw_get("handle").unwrap_or(0);
        let mut b = self.buffer.borrow_mut();
        let slot = usize::try_from(handle - 1)
            .ok()
            .filter(|&i| i < b.causes.len());
        match slot {
            Some(i) => {
                b.causes[i].1.push(cmd);
                Ok(())
            }
            None => {
                drop(b);
                Err(self.fail(
                    lua,
                    "bad_cause",
                    "every write needs a cause from `ctx:cause { summary = ..., cites = { ... } }`",
                ))
            }
        }
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

    fn station(&self, lua: &Lua, s: &str) -> mlua::Result<StationId> {
        let id: StationId = self.id(lua, s)?;
        if !self.world.stations.contains(&id) {
            return Err(self.fail(lua, "unknown_id", format!("no station `{id}`")));
        }
        Ok(id)
    }

    fn faction(&self, lua: &Lua, s: &str) -> mlua::Result<FactionId> {
        let id: FactionId = self.id(lua, s)?;
        if !self.world.factions.contains(&id) {
            return Err(self.fail(lua, "unknown_id", format!("no faction `{id}`")));
        }
        Ok(id)
    }

    fn may(&self, need: &str) -> bool {
        need.is_empty() || self.uses.iter().any(|u| u == need)
    }

    /// New records a pack creates live are namespaced by the pack's first segment.
    fn check_new_id(&self, lua: &Lua, id: &str, exists: bool) -> mlua::Result<()> {
        let ns = self.pack.as_str().split('.').next().unwrap_or("");
        if exists || ns == "core" || id.starts_with(&format!("{ns}.")) {
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

/// Checks a table only has the given keys (Luau tables don't reject typos).
fn only_keys(
    env: &CallEnv<'_>,
    lua: &Lua,
    t: &Table,
    what: &str,
    allowed: &[&str],
) -> mlua::Result<()> {
    for pair in t.pairs::<Value, Value>() {
        let (k, _) = pair?;
        let name = k.as_string().map(|s| s.to_string_lossy());
        if !name.as_deref().is_some_and(|n| allowed.contains(&n)) {
            return Err(env.fail(
                lua,
                "bad_argument",
                format!(
                    "{what}: unexpected key `{}` (expected {})",
                    name.unwrap_or_else(|| format!("{k:?}")),
                    allowed.join(", ")
                ),
            ));
        }
    }
    Ok(())
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
    // ctx:cause
    ctx.raw_set(
        "cause",
        m(scope, env, |lua, env, spec: Table| {
            only_keys(env, lua, &spec, "ctx:cause", &["summary", "cites"])?;
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
            let mut b = env.buffer.borrow_mut();
            b.causes.push((cause, Vec::new()));
            let handle = b.causes.len();
            drop(b);
            let t = lua.create_table()?;
            t.raw_set("summary", summary)?;
            t.raw_set(
                "cites",
                cites.iter().map(ToString::to_string).collect::<Vec<_>>(),
            )?;
            t.raw_set("handle", handle)?;
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
        m(scope, env, |lua, env, (lo, hi): (i64, i64)| {
            if lo > hi {
                return Err(env.fail(lua, "bad_argument", "ctx.rng:int(lo, hi) needs lo <= hi"));
            }
            let span = (hi - lo) as u64 + 1;
            let n = env.rng.borrow_mut().stream(env.rng_key).next_u64() % span;
            Ok(lo + n as i64)
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
            m(scope, env, |lua, env, (store, key): (String, String)| {
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
                    Some(ScriptValue::Integer(i)) => Ok(Value::Number(*i as f64)),
                    Some(ScriptValue::Number(n)) => Ok(Value::Number(*n)),
                    Some(ScriptValue::String(s)) => to_lua(lua, s),
                }
            })?,
        )?;
        method(
            &store,
            "store",
            "set",
            m(
                scope,
                env,
                |lua, env, (store, key, value, why): (String, String, Value, Table)| {
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
                        (ValueType::Integer, Value::Integer(i)) => Some(ScriptValue::Integer(*i)),
                        (ValueType::Integer, Value::Number(n))
                            if n.fract() == 0.0 && n.abs() < 9.0e15 =>
                        {
                            Some(ScriptValue::Integer(*n as i64))
                        }
                        (ValueType::Number, Value::Number(n)) if n.is_finite() => {
                            Some(ScriptValue::Number(*n))
                        }
                        (ValueType::Number, Value::Integer(i)) => {
                            Some(ScriptValue::Number(*i as f64))
                        }
                        (ValueType::String, Value::String(s)) => {
                            let s = s.to_string_lossy();
                            if s.len() > MAX_STORE_STRING {
                                return Err(wrong("a string over 1000 bytes"));
                            }
                            Some(ScriptValue::String(s))
                        }
                        (_, other) => return Err(wrong(other.type_name())),
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
            |lua, env, (kind, scope_arg): (String, Option<Table>)| {
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
                            t.get::<Option<String>>("station")?,
                            t.get::<Option<String>>("region")?,
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
    let query = |env: &CallEnv<'_>, lua: &Lua, q: Table| -> mlua::Result<HistoryQuery> {
        only_keys(env, lua, &q, "history query", QUERY_KEYS)?;
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
            Ok(env.history.sum(&query(env, lua, q)?))
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
        m(scope, env, |lua, env, id: String| {
            let id: FactId = id
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
            let draft: FactDraft = lua
                .from_value(Value::Table(fact))
                .map_err(|e| env.fail(lua, "bad_argument", e))?;
            // Recorded kinds carry the pack's prefix (`gm.stage_built`); bare names
            // are the engine's event kinds.
            let ns = env.pack.as_str().split('.').next().unwrap_or("");
            if !draft.kind.as_str().starts_with(&format!("{ns}.")) {
                return Err(env.fail(
                    lua,
                    "unprefixed_fact_kind",
                    format!(
                        "record kinds with this pack's prefix, e.g. `{ns}.stage_built`; got `{}`",
                        draft.kind
                    ),
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
        m(scope, env, |lua, env, id: String| {
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
        m(scope, env, |lua, env, id: String| {
            let id = env.station(lua, &id)?;
            Ok(env.world.stations.get(&id).map_or(0, |s| s.build.stage))
        })?,
    )?;
    method(
        &stations,
        "stations",
        "parts",
        m(scope, env, |lua, env, id: String| {
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
            |lua, env, (id, stage, why): (String, u32, Table)| {
                let station = env.station(lua, &id)?;
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
                move |lua, env, (id, part, why): (String, String, Table)| {
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
            |lua, env, (id, owner, why): (String, Option<String>, Table)| {
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
        m(scope, env, |lua, env, id: String| {
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
        m(scope, env, |lua, env, id: String| {
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
            |lua, env, (id, delta, why): (String, i32, Table)| {
                let faction = env.faction(lua, &id)?;
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
        m(scope, env, |lua, env, id: String| {
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
        m(scope, env, |lua, env, station: String| {
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
            let npc: Npc = lua
                .from_value(Value::Table(npc))
                .map_err(|e| env.fail(lua, "bad_argument", e))?;
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
            |lua, env, (id, station, why): (String, Option<String>, Table)| {
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
        m(scope, env, |lua, env, id: String| {
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
            let rumor: Rumor = lua
                .from_value(Value::Table(rumor))
                .map_err(|e| env.fail(lua, "bad_argument", e))?;
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
    let rumor_edit = |env: &CallEnv<'_>, lua: &Lua, id: &str| -> mlua::Result<Rumor> {
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
            move |lua, env, (id, is_true, why): (String, bool, Table)| {
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
            move |lua, env, (id, station, why): (String, String, Table)| {
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
        m(scope, env, |lua, env, region: String| {
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
        m(scope, env, |lua, env, station: String| {
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
            |lua, env, (region, owner, why): (String, Option<String>, Table)| {
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
            |lua, env, (station, owner, why): (String, Option<String>, Table)| {
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
        m(scope, env, |lua, env, (station, ware): (String, String)| {
            let line = env
                .world
                .markets
                .get(station.as_str())
                .and_then(|m| m.lines.get(ware.as_str()));
            to_lua(lua, &line)
        })?,
    )?;
    method(
        &market,
        "market",
        "lines",
        m(scope, env, |lua, env, station: String| {
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
        "add_line",
        m(
            scope,
            env,
            |lua, env, (station, ware, line, why): (String, String, Table, Table)| {
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
                let line: MarketLine = lua
                    .from_value(Value::Table(line))
                    .map_err(|e| env.fail(lua, "bad_argument", e))?;
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
            |lua, env, (station, ware, why): (String, String, Table)| {
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
            &["speaker", "place", "text", "cites"],
        )?;
        let cites: Option<Vec<String>> = spec.get("cites")?;
        let Some(cites) = cites.filter(|c| !c.is_empty()) else {
            return Err(env.fail(
                lua,
                "uncited_line",
                "every line must cite at least one history fact ID (`cites = { ev.fact }`)",
            ));
        };
        let speaker: Option<String> = spec.get("speaker")?;
        let speaker = speaker
            .map(|s| -> mlua::Result<NpcId> {
                let id: NpcId = env.id(lua, &s)?;
                if !env.world.npcs.contains(&id) {
                    return Err(env.fail(lua, "unknown_id", format!("no npc `{id}`")));
                }
                Ok(id)
            })
            .transpose()?;
        let place: Option<String> = spec.get("place")?;
        let place = place.map(|p| env.station(lua, &p)).transpose()?;
        let line = TalkLine {
            channel,
            speaker,
            place,
            text: spec.get::<Option<String>>("text")?.unwrap_or_default(),
            cites: env.cites(lua, cites)?,
            about: Vec::new(),
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
    for (name, t) in tables {
        if present.contains(name) {
            t.set_readonly(true);
            ctx.raw_set(name, t)?;
        }
    }
    ctx.set_readonly(true);
    Ok(ctx)
}
