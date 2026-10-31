//! `ctx.happenings`: happenings at stations (issue #47, 4.1). A pack reads them with
//! `happenings.read` and writes them with `happenings.write`:
//!
//! - `put` writes a new happening, or rewrites one that hasn't started, with the
//!   pack as its author (`by`). It waits upcoming: the runtime
//!   (`galaxy_progress::happenings`) starts it once its cause facts exist and the
//!   player is docked at its station with nothing else running there.
//! - `remove` takes away one that hasn't started.
//!
//! Every happening passes `galaxy_core::happenings::check` first, and a cause fact
//! named by ID must exist (`check_facts`). One that is running or over is the
//! runtime's: a pack can't rewrite or remove it. Each write carries an
//! `ExpectHappening` on the record the hook read, so it fails whole if another batch
//! changed that happening earlier in the tick.

use galaxy_contract::happening::{Happening, HappeningRun};
use galaxy_contract::prelude::*;
use mlua::{Lua, LuaString, Scope, Table};

use super::{CallEnv, m, method, to_lua};

pub(super) fn namespace<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Table> {
    let ns = lua.create_table()?;
    method(
        env,
        &ns,
        "happenings",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.happenings.get(id.as_str()))
        })?,
    )?;
    method(
        env,
        &ns,
        "happenings",
        "list",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            Ok(galaxy_core::happenings::of_station(env.world, &station)
                .map(|h| h.id.to_string())
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        env,
        &ns,
        "happenings",
        "active",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            Ok(galaxy_progress::happenings::active(env.world, &station)
                .first()
                .map(|h| h.id.to_string()))
        })?,
    )?;
    method(
        env,
        &ns,
        "happenings",
        "put",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            let mut h: Happening = env.decode(lua, spec, "happening")?;
            if !env.may("talk.write") {
                return Err(env.fail(
                    lua,
                    "undeclared_capability",
                    "a happening's speaker talks when it starts: declare `talk.write` too",
                ));
            }
            if h.by.is_some() || !h.run.is_upcoming() {
                return Err(env.fail(
                    lua,
                    "bad_argument",
                    "leave `run` and `by` out: the runtime and the hook write them",
                ));
            }
            let old = env.world.happenings.get(&h.id);
            if old.is_some_and(|o| !o.run.is_upcoming()) {
                return Err(env.fail(
                    lua,
                    "happening_started",
                    format!("`{}` has started; it can't be rewritten", h.id),
                ));
            }
            env.check_new_id(lua, h.id.as_str(), old.is_some())?;
            h.by = Some(env.pack.clone());
            h.run = HappeningRun::Upcoming;
            let mut problems = galaxy_core::happenings::check(env.world, &h)
                .err()
                .unwrap_or_default();
            problems.extend(
                galaxy_core::happenings::check_facts(env.history, &h)
                    .err()
                    .unwrap_or_default(),
            );
            if !problems.is_empty() {
                return Err(env.fail(lua, "invalid_happening", problems.join("; ")));
            }
            expect(lua, env, &why, &h.id)?;
            env.push(
                lua,
                &why,
                Command::PutHappening {
                    id: h.id.clone(),
                    value: Some(Box::new(h)),
                },
            )
        })?,
    )?;
    method(
        env,
        &ns,
        "happenings",
        "remove",
        m(scope, env, |lua, env, (id, why): (LuaString, Table)| {
            let id: HappeningId = env.id(lua, &id)?;
            let Some(h) = env.world.happenings.get(&id) else {
                return Err(env.fail(lua, "unknown_id", format!("no happening `{id}`")));
            };
            if !h.run.is_upcoming() {
                return Err(env.fail(
                    lua,
                    "happening_started",
                    format!("`{id}` has started; it stays"),
                ));
            }
            expect(lua, env, &why, &id)?;
            env.push(lua, &why, Command::PutHappening { id, value: None })
        })?,
    )?;
    Ok(ns)
}

/// Guards a write to `id` with the record the hook read, unless this call has already
/// written it: a batch committed earlier in the tick that changed it (the game
/// master's, or another call's) makes this one fail rather than be overwritten.
fn expect(lua: &Lua, env: &CallEnv<'_>, why: &Table, id: &HappeningId) -> mlua::Result<()> {
    let written = env.buffer.borrow().commands.iter().any(|c| {
        matches!(c, Command::PutHappening { id: i, .. } | Command::ExpectHappening { id: i, .. } if i == id)
    });
    if written {
        return Ok(());
    }
    env.push(
        lua,
        why,
        Command::ExpectHappening {
            id: id.clone(),
            value: env.world.happenings.get(id).cloned().map(Box::new),
        },
    )
}
