//! Script-owned dock menus. Only authored, filled candidates cross to the mind.

use galaxy_contract::talk::{Channel, DockTalkCandidate, DockTalkRequest, MAX_LINE_LEN};
use mlua::{Lua, LuaString, Scope, Table, Value};

use super::*;

fn cited(env: &CallEnv<'_>, fact: FactId) -> bool {
    env.world.talk.on(Channel::DockTalk).any(|l| l.line.cites.contains(&fact))
        || env.buffer.borrow().commands.iter().any(|c| {
            matches!(c, Command::Say { line } if line.channel == Channel::DockTalk && line.cites.contains(&fact))
        })
}

fn current(env: &CallEnv<'_>, request: &DockTalkRequest) -> bool {
    request.pack == *env.pack
        && request.revision == env.revision
        && env.world.player.ship.as_ref() == Some(&request.ship)
        && env
            .world
            .ships
            .get(&request.ship)
            .is_some_and(|s| s.docked_at.as_ref() == Some(&request.station))
        && env
            .history
            .facts()
            .iter()
            .rev()
            .find(|f| {
                f.subjects.ship.as_ref() == Some(&request.ship)
                    && matches!(
                        f.payload,
                        FactPayload::Event(Event::Dock { .. } | Event::Undock { .. })
                    )
            })
            .is_some_and(|f| f.id == request.docking)
}

pub(super) fn add<'scope, 'env: 'scope>(
    _lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
    talk: &Table,
) -> mlua::Result<()> {
    // Do not allocate unused closures inside other packs' per-call memory budgets.
    if !env.may("talk.pick") {
        return Ok(());
    }
    method(
        env,
        talk,
        "talk",
        "request",
        m(
            scope,
            env,
            |lua, env, (dock, candidates, why): (LuaString, Table, Table)| {
                let docking: FactId = env.id(lua, &dock)?;
                let Some(Fact {
                    payload: FactPayload::Event(Event::Dock { ship, station }),
                    ..
                }) = env.history.fact(docking)
                else {
                    return Err(env.fail(
                        lua,
                        "bad_docking",
                        "a talk request needs a docking fact",
                    ));
                };
                let candidates: Vec<DockTalkCandidate> =
                    env.decode(lua, candidates, "dock candidates")?;
                let request = DockTalkRequest {
                    pack: env.pack.clone(),
                    revision: env.revision.to_owned(),
                    docking,
                    ship: ship.clone(),
                    station: station.clone(),
                    candidates,
                };
                env.budget
                    .charge(lua, 1 + env.history.facts().len() as u64 / FACTS_PER_STEP)?;
                if !current(env, &request) {
                    return Ok(());
                }
                if request.candidates.is_empty() || request.candidates.len() > 32 {
                    return Err(env.fail(
                        lua,
                        "bad_talk_request",
                        "offer 1 to 32 candidates at the player's current docking",
                    ));
                }
                let mut ids = BTreeSet::new();
                for c in &request.candidates {
                    if !ids.insert(&c.line)
                        || c.text.trim().is_empty()
                        || c.text.len() > MAX_LINE_LEN
                        || env.history.fact(c.cite).is_none()
                        || !env
                            .world
                            .npcs
                            .get(&c.npc)
                            .is_some_and(|n| n.location.as_ref() == Some(station))
                    {
                        return Err(env.fail(lua, "bad_talk_candidate", "candidates need unique IDs, authored text, an existing fact and a speaker here"));
                    }
                }
                env.push(
                    lua,
                    &why,
                    Command::ExpectDocked {
                        ship: ship.clone(),
                        station: station.clone(),
                    },
                )?;
                env.push(
                    lua,
                    &why,
                    Command::Emit {
                        event: Event::TalkRequested {
                            request: Box::new(request),
                        },
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        talk,
        "talk",
        "selected",
        m(
            scope,
            env,
            |lua, env, (id, line): (LuaString, LuaString)| {
                let id: FactId = env.id(lua, &id)?;
                let line: AnyId = env.id(lua, &line)?;
                env.budget.charge(
                    lua,
                    1 + (env.history.facts().len() + env.world.talk.lines.len()) as u64
                        / FACTS_PER_STEP,
                )?;
                let Some(Fact {
                    payload: FactPayload::Event(Event::TalkRequested { request }),
                    ..
                }) = env.history.fact(id)
                else {
                    return Ok(Value::Nil);
                };
                if !current(env, request) {
                    return Ok(Value::Nil);
                }
                let candidate = request.candidates.iter().find(|c| {
                    c.line == line
                        && !cited(env, c.cite)
                        && env
                            .world
                            .npcs
                            .get(&c.npc)
                            .is_some_and(|n| n.location.as_ref() == Some(&request.station))
                });
                to_lua(lua, &candidate)
            },
        )?,
    )?;
    method(
        env,
        talk,
        "talk",
        "cited",
        m(scope, env, |lua, env, id: LuaString| {
            let id: FactId = env.id(lua, &id)?;
            env.budget
                .charge(lua, 1 + env.world.talk.lines.len() as u64 / FACTS_PER_STEP)?;
            Ok(cited(env, id))
        })?,
    )?;
    Ok(())
}
