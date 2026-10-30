//! `ctx.space`: scripted craft and live rock fields. A teacher's tender, a
//! prospector's skiff, a rumored vein laid out once someone finds it, a field left to
//! rest. Every write is a `Put*` command, so a rollback removes what a script made.

use galaxy_contract::prelude::*;
use galaxy_contract::ship::ShipActivity;
use galaxy_contract::verbs::OrderQueue;
use galaxy_contract::world::{Hold, Owner, Rock, RockField, Ship};
use mlua::{Lua, LuaString, Scope, Table, Value};
use serde::Deserialize;

use super::{CallEnv, m, method, only_keys, to_lua};

/// Most rocks one `put_field` lays out.
const MAX_ROCKS: usize = 32;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CraftSpec {
    id: ShipId,
    name: String,
    hull: HullId,
    npc: NpcId,
    station: StationId,
    #[serde(default)]
    cargo: std::collections::BTreeMap<WareId, u32>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FieldSpec {
    id: FieldId,
    name: String,
    region: RegionId,
    rock_type: RockTypeId,
    center: Vec2,
    radius: f32,
    amount: u32,
    #[serde(default)]
    regrow: f32,
    rocks: Vec<Vec2>,
}

pub(super) fn namespace<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Table> {
    let space = lua.create_table()?;
    method(
        env,
        &space,
        "space",
        "craft",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            let Some(s) = env.world.ships.get(id.as_str()) else {
                return Ok(Value::Nil);
            };
            let t = lua.create_table()?;
            t.raw_set("id", s.id.as_str())?;
            t.raw_set("name", s.name.as_str())?;
            t.raw_set("hull", s.hull.as_str())?;
            t.raw_set(
                "owner",
                match &s.owner {
                    Owner::Player => "player".to_owned(),
                    Owner::Npc { npc } => npc.to_string(),
                    Owner::Faction { faction } => faction.to_string(),
                },
            )?;
            t.raw_set("docked_at", s.docked_at.as_ref().map(ToString::to_string))?;
            t.raw_set("region", s.region.as_str())?;
            t.raw_set("position", to_lua(lua, &s.position)?)?;
            let cargo = lua.create_table()?;
            for (ware, n) in &s.hold.cargo {
                cargo.raw_set(ware.as_str(), *n)?;
            }
            t.raw_set("cargo", cargo)?;
            Ok(Value::Table(t))
        })?,
    )?;
    method(
        env,
        &space,
        "space",
        "put_craft",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.space:put_craft",
                &["id", "name", "hull", "npc", "station", "cargo"],
            )?;
            let spec: CraftSpec = env.decode(lua, spec, "ctx.space:put_craft")?;
            let w = env.world;
            env.check_new_id(lua, spec.id.as_str(), false)?;
            if w.ships.contains(&spec.id) {
                return Err(env.fail(
                    lua,
                    "exists",
                    format!("`{}` is already in the world", spec.id),
                ));
            }
            let fail = |what: &str, id: &dyn std::fmt::Display| {
                env.fail(lua, "unknown_id", format!("no {what} `{id}`"))
            };
            let hull = w.hulls.get(&spec.hull).ok_or_else(|| fail("hull", &spec.hull))?;
            if !w.npcs.contains(&spec.npc) {
                return Err(fail("npc", &spec.npc));
            }
            let station = w
                .stations
                .get(&spec.station)
                .ok_or_else(|| fail("station", &spec.station))?;
            if let Some(ware) = spec.cargo.keys().find(|w2| !w.wares.contains(*w2)) {
                return Err(fail("ware", ware));
            }
            let held: u64 = spec.cargo.values().map(|&n| u64::from(n)).sum();
            if held > u64::from(hull.hold_capacity) {
                return Err(env.fail(
                    lua,
                    "no_room",
                    format!("{} holds {}, not {held}", hull.id, hull.hold_capacity),
                ));
            }
            let ship = Ship {
                id: spec.id.clone(),
                name: spec.name,
                hull: spec.hull,
                owner: Owner::Npc { npc: spec.npc },
                region: station.region.clone(),
                position: station.position,
                velocity: Vec2::ZERO,
                heading: 0.0,
                docked_at: Some(station.id.clone()),
                hold: Hold {
                    cargo: spec.cargo.into_iter().filter(|(_, n)| *n > 0).collect(),
                },
                orders: OrderQueue::default(),
                activity: ShipActivity::default(),
            };
            env.push(
                lua,
                &why,
                Command::PutShip {
                    id: spec.id,
                    value: Some(Box::new(ship)),
                },
            )
        })?,
    )?;
    method(
        env,
        &space,
        "space",
        "put_field",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.space:put_field",
                &[
                    "id",
                    "name",
                    "region",
                    "rock_type",
                    "center",
                    "radius",
                    "amount",
                    "regrow",
                    "rocks",
                ],
            )?;
            let spec: FieldSpec = env.decode(lua, spec, "ctx.space:put_field")?;
            let w = env.world;
            env.check_new_id(lua, spec.id.as_str(), false)?;
            if w.fields.contains(&spec.id) {
                return Err(env.fail(
                    lua,
                    "exists",
                    format!("`{}` is already in the world", spec.id),
                ));
            }
            if !w.regions.contains(&spec.region) {
                return Err(env.fail(
                    lua,
                    "unknown_id",
                    format!("no region `{}`", spec.region),
                ));
            }
            if !w.rock_types.contains(&spec.rock_type) {
                return Err(env.fail(
                    lua,
                    "unknown_id",
                    format!("no rock type `{}`", spec.rock_type),
                ));
            }
            let finite = [spec.center.x, spec.center.y, spec.radius, spec.regrow]
                .into_iter()
                .chain(spec.rocks.iter().flat_map(|p| [p.x, p.y]))
                .all(f32::is_finite);
            if !finite || spec.radius <= 0.0 || spec.regrow < 0.0 {
                return Err(env.fail(
                    lua,
                    "bad_argument",
                    "a field needs a finite centre and rocks, a positive radius and a regrowth of 0 or more",
                ));
            }
            if spec.rocks.is_empty() || spec.rocks.len() > MAX_ROCKS || spec.amount == 0 {
                return Err(env.fail(
                    lua,
                    "bad_argument",
                    format!("a field lays out 1 to {MAX_ROCKS} rocks of 1 unit or more"),
                ));
            }
            // Rock IDs follow the field's: `field.ember_shoals` -> `rock.ember_shoals.01`.
            let name = spec.id.as_str().strip_prefix("field.").unwrap_or(spec.id.as_str());
            let prefix = format!("rock.{name}");
            let mut rocks = Vec::with_capacity(spec.rocks.len());
            for (i, at) in spec.rocks.iter().enumerate() {
                let id: RockId = format!("{prefix}.{:02}", i + 1)
                    .parse()
                    .map_err(|e: IdError| env.fail(lua, "bad_id", e))?;
                if w.rocks.contains(&id) {
                    return Err(env.fail(lua, "exists", format!("`{id}` is already in the world")));
                }
                rocks.push(Rock {
                    id,
                    field: spec.id.clone(),
                    rock_type: spec.rock_type.clone(),
                    position: *at,
                    remaining: spec.amount,
                    initial: spec.amount,
                    growth: Default::default(),
                });
            }
            let field = RockField {
                id: spec.id.clone(),
                name: spec.name,
                region: spec.region,
                rock_type: spec.rock_type,
                rock_prefix: prefix,
                center: spec.center,
                radius: spec.radius,
                count: rocks.len() as u32,
                amount_min: spec.amount,
                amount_max: spec.amount,
                regrow: spec.regrow,
            };
            env.push(
                lua,
                &why,
                Command::PutField {
                    id: spec.id,
                    value: Some(Box::new(field)),
                },
            )?;
            for rock in rocks {
                env.push(
                    lua,
                    &why,
                    Command::PutRock {
                        id: rock.id.clone(),
                        value: Some(Box::new(rock)),
                    },
                )?;
            }
            Ok(())
        })?,
    )?;
    method(
        env,
        &space,
        "space",
        "set_regrow",
        m(
            scope,
            env,
            |lua, env, (id, per_pulse, why): (LuaString, Value, Table)| {
                let id: FieldId = env.id(lua, &id)?;
                let mut field =
                    env.world.fields.get(&id).cloned().ok_or_else(|| {
                        env.fail(lua, "unknown_id", format!("no field `{id}`"))
                    })?;
                let regrow = match per_pulse {
                    Value::Integer(i) => i as f64,
                    Value::Number(n) => n,
                    _ => f64::NAN,
                };
                if !regrow.is_finite() || !(0.0..=1000.0).contains(&regrow) {
                    return Err(env.fail(
                        lua,
                        "bad_number",
                        "`per_pulse` must be a number from 0 to 1000",
                    ));
                }
                field.regrow = regrow as f32;
                env.push(
                    lua,
                    &why,
                    Command::PutField {
                        id,
                        value: Some(Box::new(field)),
                    },
                )
            },
        )?,
    )?;
    Ok(space)
}
