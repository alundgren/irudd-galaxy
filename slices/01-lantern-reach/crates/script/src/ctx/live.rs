//! Canon and render-free runtime assets.
use super::*;
use galaxy_contract::canon::CanonStatus;
use galaxy_contract::live::{MapOverlay, SurveyBuoy};

/// Whether a founded station's ring is open: a rollback shuts it to a scaffold with no
/// market and no services.
fn open_ring(world: &WorldState, id: &StationId) -> bool {
    world.markets.contains(id)
        || world
            .stations
            .get(id)
            .is_some_and(|s| !s.services.is_empty())
}

/// The host is the boundary for what a pack may pay. A buoy's reward is paid natively
/// when the pilot claims it, so placing or replacing one that pays credits needs
/// `beats.write` (the capability of `ctx.beats:pay`) and one that moves standing needs
/// `standing.write` (the payment is a standing deed). Whether the pilot has read it and
/// claimed its reward is the pilot's doing: a script never sets either.
fn check_buoy(lua: &Lua, env: &CallEnv<'_>, id: &str, buoy: &SurveyBuoy) -> mlua::Result<()> {
    let old = env.world.survey_buoys.get(id);
    let claimed = |b: &SurveyBuoy| b.reward.as_ref().is_some_and(|r| r.claimed);
    if buoy.read && !old.is_some_and(|b| b.read) {
        return Err(env.fail(
            lua,
            "native_state",
            "a buoy is read by the pilot; place it with `read = false`",
        ));
    }
    if claimed(buoy) && !old.is_some_and(claimed) {
        return Err(env.fail(
            lua,
            "native_state",
            "a reward is claimed by the pilot; place it with `claimed = false`",
        ));
    }
    if let Some(reward) = &buoy.reward {
        if reward.credits > 0 && !env.may("beats.write") {
            return Err(env.fail(
                lua,
                "undeclared_hook",
                "a buoy reward that pays credits needs `beats.write` in uses",
            ));
        }
        if !reward.standing.is_empty() && !env.may("standing.write") {
            return Err(env.fail(
                lua,
                "undeclared_hook",
                "a buoy reward that moves standing needs `standing.write` in uses",
            ));
        }
    }
    Ok(())
}

pub(super) fn namespaces<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Vec<(&'static str, Table)>> {
    let canon = lua.create_table()?;
    method(
        env,
        &canon,
        "canon",
        "status",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.canon.nodes.get(&id).map(|n| n.status))
        })?,
    )?;
    method(
        env,
        &canon,
        "canon",
        "open",
        m(scope, env, |lua, env, (id, why): (LuaString, Table)| {
            let id = env.arg(lua, &id)?;
            if !env.opens.contains(&id) {
                return Err(env.fail(
                    lua,
                    "canon_undeclared",
                    format!("{id} is not in manifest opens"),
                ));
            }
            env.world
                .canon
                .may_open(&id, env.pack, env.world)
                .map_err(|e| env.fail(lua, "canon_locked", e))?;
            let node = &env.world.canon.nodes[&id];
            if node.status == CanonStatus::Open {
                return Ok(());
            }
            env.push(
                lua,
                &why,
                Command::SetCanonStatus {
                    id: id.clone(),
                    status: CanonStatus::Open,
                },
            )?;
            env.push(
                lua,
                &why,
                Command::RecordFact {
                    fact: Box::new(FactDraft {
                        kind: format!(
                            "{}.canon_opened",
                            env.pack.as_str().split('.').next().unwrap_or("gm")
                        )
                        .parse()
                        .unwrap(),
                        subjects: Subjects {
                            other: Some(id.parse().map_err(|e| env.fail(lua, "bad_id", e))?),
                            ..Subjects::default()
                        },
                        amount: None,
                        note: node.statement.clone(),
                    }),
                },
            )
        })?,
    )?;
    let kit = lua.create_table()?;
    method(
        env,
        &kit,
        "kit",
        "station",
        m(
            scope,
            env,
            |lua,
             env,
             (station, recipe, dockmaster, market, why): (
                Table,
                LuaString,
                Table,
                Table,
                Table,
            )| {
                // A station brings a resident and a market: the pack must hold the
                // capabilities that write them.
                for need in ["npcs.write", "market.lines"] {
                    if !env.may(need) {
                        return Err(env.fail(
                            lua,
                            "undeclared_hook",
                            format!("founding a station needs `{need}` in uses"),
                        ));
                    }
                }
                let station: Station = env.decode(lua, station, "ctx.kit:station")?;
                env.check_new_id(lua, station.id.as_str(), false)?;
                // A rollback leaves a founded station as a closed scaffold. A later
                // revision of the pack that founded it (its ID carries the pack's own
                // prefix) may reopen it in place; anything else already there is taken.
                let reopen = match env.world.stations.get(&station.id) {
                    None => false,
                    Some(old)
                        if station
                            .id
                            .as_str()
                            .starts_with(&format!("{}.", env.pack.as_str()))
                            && env.world.station_recipes.contains_key(&station.id)
                            && !open_ring(env.world, &station.id)
                            && old.region == station.region
                            && old.position == station.position =>
                    {
                        true
                    }
                    Some(_) => return Err(env.fail(lua, "exists", "station already exists")),
                };
                if recipe.as_bytes().len() > 16 * 1024 {
                    return Err(env.fail(lua, "too_large", "recipe exceeds 16 KiB"));
                }
                env.budget
                    .charge(lua, (recipe.as_bytes().len() / 64 + 1) as u64)?;
                let recipe: galaxy_kit::StationRecipe = serde_json::from_slice(&recipe.as_bytes())
                    .map_err(|e| env.fail(lua, "recipe", e))?;
                recipe
                    .validate_art(galaxy_kit::art_bible::ArtBible::bundled())
                    .map_err(|e| env.fail(lua, "art_bible", format!("{e:?}")))?;
                if recipe.region.as_deref() != Some(station.region.as_str())
                    || recipe.faction.as_deref() != station.owner.as_ref().map(|f| f.as_str())
                {
                    return Err(env.fail(
                        lua,
                        "recipe",
                        "recipe region and faction must match the station",
                    ));
                }
                let region = env
                    .world
                    .regions
                    .get(&station.region)
                    .ok_or_else(|| env.fail(lua, "placement", "unknown region"))?;
                let nearby_fact = env.buffer.borrow().cause.as_ref().is_some_and(|cause| {
                    cause.cites.iter().any(|id| {
                        let Some(f) = env.history.fact(*id) else {
                            return false;
                        };
                        let owned = f
                            .subjects
                            .ship
                            .as_ref()
                            .and_then(|id| env.world.ships.get(id))
                            .is_some_and(|s| s.owner == Owner::Player);
                        let pos = f
                            .subjects
                            .rock
                            .as_ref()
                            .and_then(|id| env.world.rocks.get(id))
                            .map(|r| r.position)
                            .or_else(|| {
                                f.subjects
                                    .station
                                    .as_ref()
                                    .and_then(|id| env.world.stations.get(id))
                                    .map(|s| s.position)
                            });
                        owned
                            && matches!(
                                f.kind.as_str(),
                                "mine" | "rock_depleted" | "sell" | "deliver"
                            )
                            && pos.is_some_and(|p| p.distance(station.position) <= 900.0)
                    })
                });
                if station.position.distance(region.center) > region.radius - 150.0
                    || !nearby_fact
                    || env
                        .world
                        .station_recipes
                        .keys()
                        .any(|id| id != &station.id && open_ring(env.world, id))
                    || env.world.stations.values().any(|s| {
                        s.id != station.id && s.position.distance(station.position) < 600.0
                    })
                    || env
                        .world
                        .fields
                        .values()
                        .any(|f| f.center.distance(station.position) < f.radius + 200.0)
                    || env.world.lanes.values().any(|l| {
                        l.a_gate.distance(station.position) < 250.0
                            || l.b_gate.distance(station.position) < 250.0
                    })
                {
                    return Err(env.fail(lua,"placement","station must be inside the region, spaced from rings, fields and mouths, and near a cited player deed"));
                }
                let npc: Npc = env.decode(lua, dockmaster, "station dockmaster")?;
                env.check_new_id(lua, npc.id.as_str(), false)?;
                if env.world.npcs.contains(&npc.id)
                    || npc.location.as_ref() != Some(&station.id)
                    || npc.role != "dockmaster"
                {
                    return Err(env.fail(
                        lua,
                        "station",
                        "a new station needs its own resident dockmaster",
                    ));
                }
                let lines: std::collections::BTreeMap<WareId, MarketLine> =
                    env.decode(lua, market, "station market")?;
                if lines.is_empty() {
                    return Err(env.fail(lua, "station", "a new station needs a market"));
                }
                let id = station.id.clone();
                let market = Market {
                    station: id.clone(),
                    lines,
                };
                if reopen {
                    // One reversible command: rolling this revision back shuts the
                    // ring again and leaves the scaffold as it was.
                    env.push(
                        lua,
                        &why,
                        Command::SetFoundedStation {
                            station: Box::new(station),
                            recipe: Box::new(recipe),
                            market: Some(Box::new(market)),
                        },
                    )?;
                    return env.push(
                        lua,
                        &why,
                        Command::PutNpc {
                            id: npc.id.clone(),
                            value: Some(Box::new(npc)),
                        },
                    );
                }
                env.push(
                    lua,
                    &why,
                    Command::FoundStation {
                        station: Box::new(station),
                        recipe: Box::new(recipe),
                        market: None,
                    },
                )?;
                env.push(
                    lua,
                    &why,
                    Command::PutNpc {
                        id: npc.id.clone(),
                        value: Some(Box::new(npc)),
                    },
                )?;
                env.push(
                    lua,
                    &why,
                    Command::PutMarket {
                        id,
                        value: Some(Box::new(market)),
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        &kit,
        "kit",
        "overlay",
        m(
            scope,
            env,
            |lua, env, (id, value, why): (LuaString, Option<Table>, Table)| {
                let id = env.arg(lua, &id)?;
                env.check_new_id(lua, &id, false)?;
                let value: Option<MapOverlay> = value
                    .map(|v| env.decode(lua, v, "ctx.kit:overlay"))
                    .transpose()?;
                env.push(
                    lua,
                    &why,
                    Command::PutMapOverlay {
                        id,
                        value: value.map(Box::new),
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        &kit,
        "kit",
        "buoy",
        m(
            scope,
            env,
            |lua, env, (id, value, why): (LuaString, Table, Table)| {
                let id = env.arg(lua, &id)?;
                env.check_new_id(lua, &id, false)?;
                let value: SurveyBuoy = env.decode(lua, value, "ctx.kit:buoy")?;
                check_buoy(lua, env, &id, &value)?;
                env.push(
                    lua,
                    &why,
                    Command::PutSurveyBuoy {
                        id,
                        value: Some(Box::new(value)),
                    },
                )
            },
        )?,
    )?;
    Ok(vec![("canon", canon), ("kit", kit)])
}
