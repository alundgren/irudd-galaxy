//! Canon and render-free runtime assets.
use super::*;
use galaxy_contract::canon::CanonStatus;
use galaxy_contract::live::MapOverlay;

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
                let station: Station = env.decode(lua, station, "ctx.kit:station")?;
                env.check_new_id(lua, station.id.as_str(), false)?;
                if env.world.stations.contains(&station.id) {
                    return Err(env.fail(lua, "exists", "station already exists"));
                }
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
                    || !env.world.station_recipes.is_empty()
                    || env
                        .world
                        .stations
                        .values()
                        .any(|s| s.position.distance(station.position) < 600.0)
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
                        id: id.clone(),
                        value: Some(Box::new(Market { station: id, lines })),
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
                let value = env.decode(lua, value, "ctx.kit:buoy")?;
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
