//! `ctx.parts`: a station's parts (issue #47, 3.1). A pack opens, closes, upgrades or
//! takes away a room by writing its part, and the keeper says why:
//!
//! - `put` writes the part with the call's cause (the pack and the facts it cites) and
//!   the news ribbon, and the keeper says the part's `line` at the station, citing the
//!   same facts;
//! - `remove` takes it away and the keeper says its `rollback_line`.
//!
//! Both happen only at a calm moment: while the player is docked (handlers run inside
//! ticks, never paused). Otherwise they write nothing and return `false`, so a handler
//! can try again at the next docking. The batch expects the flagship still docked
//! where it was, so an undock earlier in the same tick is contention, never a fault.
//! Every part passes `galaxy_core::parts::check` first.

use galaxy_contract::parts::{PartCause, PartNews, Ribbon, StationPart};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use mlua::{Lua, LuaString, Scope, Table};

use super::{CallEnv, m, method, to_lua};

/// The flagship and where it's docked, if the player is at a calm moment.
fn calm(env: &CallEnv<'_>) -> Option<(ShipId, StationId)> {
    let ship = env.world.player.ship.clone()?;
    let station = galaxy_core::parts::docked_at(env.world)?;
    Some((ship, station))
}

/// The facts the call's cause cites, for the keeper's line.
fn cites(env: &CallEnv<'_>, lua: &Lua) -> mlua::Result<Vec<FactId>> {
    match env.buffer.borrow().cause.as_ref() {
        Some(c) => Ok(c.cites.clone()),
        None => Err(env.fail(
            lua,
            "bad_cause",
            "every write needs the cause from `ctx:cause { summary = ..., cites = { ... } }`",
        )),
    }
}

/// The keeper's line about `part` at its station, citing `cites`.
fn keeper_line(
    env: &CallEnv<'_>,
    lua: &Lua,
    part: &StationPart,
    text: &str,
    cites: Vec<FactId>,
) -> mlua::Result<TalkLine> {
    if !env.may("talk.write") {
        return Err(env.fail(
            lua,
            "undeclared_capability",
            "`ctx.parts` writes the keeper's line too: declare `talk.write`",
        ));
    }
    let line = TalkLine {
        channel: Channel::DockTalk,
        speaker: Some(part.keeper.clone()),
        place: Some(part.station.clone()),
        text: text.to_owned(),
        cites,
        about: Vec::new(),
        choices: Vec::new(),
    };
    line.validate()
        .map_err(|e| env.fail(lua, "illegible_line", e))?;
    Ok(line)
}

pub(super) fn namespace<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Table> {
    let parts = lua.create_table()?;
    method(
        env,
        &parts,
        "parts",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.station_parts.get(id.as_str()))
        })?,
    )?;
    method(
        env,
        &parts,
        "parts",
        "list",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            Ok(galaxy_core::parts::of_station(env.world, &station)
                .map(|p| p.id.to_string())
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        env,
        &parts,
        "parts",
        "put",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            let mut part: StationPart = env.decode(lua, spec, "part")?;
            let cites = cites(env, lua)?;
            let Some((ship, docked)) = calm(env) else {
                return Ok(false);
            };
            let old = env.world.station_parts.get(&part.id);
            env.check_new_id(lua, part.id.as_str(), old.is_some())?;
            part.cause = Some(PartCause {
                script: Some(env.pack.clone()),
                facts: cites.clone(),
            });
            part.news = Some(PartNews {
                ribbon: Ribbon::of(old, &part),
                docking: env
                    .world
                    .counters
                    .get(CounterKind::Dockings, &CounterScope::Player),
            });
            if let Err(e) = galaxy_core::parts::check(env.world, &part) {
                return Err(env.fail(lua, "invalid_part", e.join("; ")));
            }
            let line = keeper_line(env, lua, &part, &part.line, cites)?;
            env.push(
                lua,
                &why,
                Command::ExpectDocked {
                    ship,
                    station: docked,
                },
            )?;
            env.push(
                lua,
                &why,
                Command::PutStationPart {
                    id: part.id.clone(),
                    value: Some(Box::new(part)),
                },
            )?;
            env.push(
                lua,
                &why,
                Command::Say {
                    line: Box::new(line),
                },
            )?;
            Ok(true)
        })?,
    )?;
    method(
        env,
        &parts,
        "parts",
        "remove",
        m(scope, env, |lua, env, (id, why): (LuaString, Table)| {
            let id: StationPartId = env.id(lua, &id)?;
            let cites = cites(env, lua)?;
            let Some(part) = env.world.station_parts.get(&id) else {
                return Err(env.fail(lua, "unknown_id", format!("no station part `{id}`")));
            };
            let Some((ship, docked)) = calm(env) else {
                return Ok(false);
            };
            let line = keeper_line(env, lua, part, &part.rollback_line, cites)?;
            env.push(
                lua,
                &why,
                Command::ExpectDocked {
                    ship,
                    station: docked,
                },
            )?;
            env.push(lua, &why, Command::PutStationPart { id, value: None })?;
            env.push(
                lua,
                &why,
                Command::Say {
                    line: Box::new(line),
                },
            )?;
            Ok(true)
        })?,
    )?;
    Ok(parts)
}
