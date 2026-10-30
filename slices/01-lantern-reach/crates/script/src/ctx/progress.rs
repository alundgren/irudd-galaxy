//! The W4 namespaces on `ctx`: `ctx.orders`, `ctx.beats`, `ctx.mechanics`,
//! `ctx.standing`, `ctx.traits`, `ctx.pacing`, and the market levers on
//! `ctx.market`. Each wraps `galaxy-progress` or `galaxy-economy`, so scripts and
//! native code share one implementation.

use galaxy_contract::prelude::*;
use galaxy_contract::progress::TunableValue;
use galaxy_economy::{Lever, lever_batch, lever_commands};
use galaxy_progress::{beats, standing};
use mlua::{Lua, LuaSerdeExt, LuaString, Scope, Table, Value};

use super::{CallEnv, m, method, only_keys, to_lua};
use crate::meter::RECORDS_PER_STEP;

/// A goal line as Luau sees it.
fn goal_table(lua: &Lua, g: &beats::GoalLine) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("beat", g.beat.as_str())?;
    t.raw_set("act", g.act)?;
    t.raw_set("goal", g.goal.as_str())?;
    t.raw_set("giver", g.giver.as_ref().map(|n| n.to_string()))?;
    t.raw_set("step", g.step.clone())?;
    Ok(t)
}

impl CallEnv<'_> {
    /// A ship scripts may order: it exists, isn't the player's ship, and isn't a hull
    /// berthed at prestige.
    fn orderable(&self, lua: &Lua, s: &LuaString) -> mlua::Result<ShipId> {
        let id: ShipId = self.id(lua, s)?;
        if !self.world.ships.contains(&id) {
            return Err(self.fail(lua, "unknown_id", format!("no ship `{id}`")));
        }
        if self.world.player.ship.as_ref() == Some(&id) {
            return Err(self.fail(
                lua,
                "player_ship",
                "scripts never give the player's ship orders; order scripted craft and tenders",
            ));
        }
        if galaxy_progress::prestige::is_berthed(self.world, &id) {
            return Err(self.fail(
                lua,
                "berthed",
                format!("`{id}` is berthed for good and can't be flown"),
            ));
        }
        Ok(id)
    }

    fn order(&self, lua: &Lua, v: Value) -> mlua::Result<Order> {
        let Value::Table(t) = v else {
            return Err(self.fail(
                lua,
                "bad_order",
                "an order is a table, e.g. `{ verb = \"undock\" }`",
            ));
        };
        let order: Order = self.decode(lua, t, "an order")?;
        self.check_order(lua, &order)?;
        Ok(order)
    }

    fn check_order(&self, lua: &Lua, o: &Order) -> mlua::Result<()> {
        let w = self.world;
        let missing = |kind: &str, id: &dyn std::fmt::Display| {
            self.fail(lua, "unknown_id", format!("no {kind} `{id}`"))
        };
        match o {
            Order::Dock { station } if !w.stations.contains(station) => {
                Err(missing("station", station))
            }
            Order::Mine { rock } if !w.rocks.contains(rock) => Err(missing("rock", rock)),
            Order::Buy { ware, .. } | Order::Sell { ware, .. } if !w.wares.contains(ware) => {
                Err(missing("ware", ware))
            }
            Order::Loop { orders, .. } => orders.iter().try_for_each(|o| self.check_order(lua, o)),
            _ => Ok(()),
        }
    }

    fn mech_flag(&self, lua: &Lua, s: &LuaString) -> mlua::Result<FlagId> {
        let id: FlagId = self.id(lua, s)?;
        if !id.as_str().starts_with("mech.") {
            return Err(self.fail(
                lua,
                "not_a_mechanic",
                format!(
                    "`{id}`: scripts set `mech.*` flags; capabilities come from upgrades and beats"
                ),
            ));
        }
        Ok(id)
    }

    fn slot(&self, lua: &Lua, s: &LuaString) -> mlua::Result<SlotId> {
        let id: SlotId = self.id(lua, s)?;
        if !self.world.progress.rules.slots.contains_key(&id) {
            return Err(self.fail(
                lua,
                "unknown_id",
                format!("`{id}` isn't a reserved slot (controls.toml)"),
            ));
        }
        Ok(id)
    }

    fn levers(&self, lua: &Lua, why: &Table, levers: &[Lever]) -> mlua::Result<()> {
        let summary: String = why.raw_get("summary").unwrap_or_default();
        let batch = lever_batch(self.world, Cause::system("lever", summary), levers)
            .map_err(|e| self.fail(lua, "bad_lever", e))?;
        for c in batch.commands {
            self.push(lua, why, c)?;
        }
        Ok(())
    }

    fn lever(&self, lua: &Lua, why: &Table, lever: Lever) -> mlua::Result<()> {
        let cmds =
            lever_commands(self.world, &lever).map_err(|e| self.fail(lua, "bad_lever", e))?;
        for c in cmds {
            self.push(lua, why, c)?;
        }
        Ok(())
    }
}

/// The market levers, on the existing `ctx.market` table.
pub(super) fn add_levers<'scope, 'env: 'scope>(
    _lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
    market: &Table,
) -> mlua::Result<()> {
    type Rate = (LuaString, LuaString, Value, Table);
    let ids = |lua: &Lua,
               env: &CallEnv<'_>,
               s: &LuaString,
               w: &LuaString|
     -> mlua::Result<(StationId, WareId)> {
        Ok((env.station(lua, s)?, env.id(lua, w)?))
    };
    method(
        env,
        market,
        "market",
        "scale_producers",
        m(scope, env, move |lua, env, (s, w, factor, why): Rate| {
            let (station, ware) = ids(lua, env, &s, &w)?;
            env.lever(
                lua,
                &why,
                Lever::ScaleProducers {
                    station,
                    ware,
                    factor: number(env, lua, &factor, "factor")?,
                },
            )
        })?,
    )?;
    method(
        env,
        market,
        "market",
        "scale_consumers",
        m(scope, env, move |lua, env, (s, w, factor, why): Rate| {
            let (station, ware) = ids(lua, env, &s, &w)?;
            env.lever(
                lua,
                &why,
                Lever::ScaleConsumers {
                    station,
                    ware,
                    factor: number(env, lua, &factor, "factor")?,
                },
            )
        })?,
    )?;
    method(
        env,
        market,
        "market",
        "add_producers",
        m(scope, env, move |lua, env, (s, w, n, why): Rate| {
            let (station, ware) = ids(lua, env, &s, &w)?;
            env.lever(
                lua,
                &why,
                Lever::AddProducers {
                    station,
                    ware,
                    per_pulse: env.int(lua, &n, "per_pulse")?,
                },
            )
        })?,
    )?;
    method(
        env,
        market,
        "market",
        "add_consumers",
        m(scope, env, move |lua, env, (s, w, n, why): Rate| {
            let (station, ware) = ids(lua, env, &s, &w)?;
            env.lever(
                lua,
                &why,
                Lever::AddConsumers {
                    station,
                    ware,
                    per_pulse: env.int(lua, &n, "per_pulse")?,
                },
            )
        })?,
    )?;
    method(
        env,
        market,
        "market",
        "set_stock",
        m(scope, env, move |lua, env, (s, w, n, why): Rate| {
            let (station, ware) = ids(lua, env, &s, &w)?;
            let stock: u32 = env.int(lua, &n, "stock")?;
            env.lever(
                lua,
                &why,
                Lever::SetStock {
                    station,
                    ware,
                    stock,
                },
            )
        })?,
    )?;
    method(
        env,
        market,
        "market",
        "levers",
        m(scope, env, |lua, env, (list, why): (Table, Table)| {
            let levers: Vec<Lever> = env.decode(lua, list, "levers")?;
            env.levers(lua, &why, &levers)
        })?,
    )?;
    Ok(())
}

/// A whole, positive number of units.
fn units(env: &CallEnv<'_>, lua: &Lua, n: &Value) -> mlua::Result<u32> {
    let q: u32 = env.int(lua, n, "quantity")?;
    if q == 0 {
        return Err(env.fail(lua, "bad_argument", "quantity must be 1 or more"));
    }
    Ok(q)
}

/// A finite number.
fn number(env: &CallEnv<'_>, lua: &Lua, v: &Value, what: &str) -> mlua::Result<f64> {
    match v {
        Value::Integer(i) => Ok(*i as f64),
        Value::Number(n) if n.is_finite() => Ok(*n),
        _ => Err(env.fail(
            lua,
            "bad_number",
            format!("`{what}` must be a finite number"),
        )),
    }
}

/// A position as Luau sees it (`Vec2`).
fn point(lua: &Lua, p: Vec2) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.raw_set("x", f64::from(p.x))?;
    t.raw_set("y", f64::from(p.y))?;
    Ok(t)
}

/// `part / whole`, 1 when there's no whole.
fn share(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        1.0
    } else {
        part as f64 / whole as f64
    }
}

impl CallEnv<'_> {
    /// A `ctx.beats:distance` target: a station ID, or a `Spot` table. Returns the
    /// region it's in (`None`: the ship's) and the point.
    fn spot(&self, lua: &Lua, to: Value) -> mlua::Result<(Option<RegionId>, Vec2)> {
        match to {
            Value::String(s) => {
                let id = self.station(lua, &s)?;
                let st =
                    self.world.stations.get(&id).ok_or_else(|| {
                        self.fail(lua, "unknown_id", format!("no station `{id}`"))
                    })?;
                Ok((Some(st.region.clone()), st.position))
            }
            Value::Table(t) => {
                only_keys(self, lua, &t, "ctx.beats:distance", &["x", "y", "region"])?;
                let coord = |k: &str| -> mlua::Result<f32> {
                    let n = number(self, lua, &t.raw_get::<Value>(k)?, k)?;
                    Ok(n as f32)
                };
                let at = Vec2::new(coord("x")?, coord("y")?);
                if !(at.x.is_finite() && at.y.is_finite()) {
                    return Err(self.fail(lua, "bad_number", "a spot must be finite"));
                }
                let region = t
                    .raw_get::<Option<LuaString>>("region")?
                    .map(|r| {
                        let id: RegionId = self.id(lua, &r)?;
                        if self.world.regions.contains(&id) {
                            Ok(id)
                        } else {
                            Err(self.fail(lua, "unknown_id", format!("no region `{id}`")))
                        }
                    })
                    .transpose()?;
                Ok((region, at))
            }
            _ => Err(self.fail(
                lua,
                "bad_argument",
                "ctx.beats:distance takes a station ID or `{ x = ..., y = ..., region = ... }`",
            )),
        }
    }
}

/// The player's ship, docked at `station`.
fn docked_player<'w>(env: &CallEnv<'w>, lua: &Lua, station: &StationId) -> mlua::Result<&'w Ship> {
    let ship = galaxy_progress::eval::player_ship(env.world)
        .ok_or_else(|| env.fail(lua, "no_ship", "the player has no ship"))?;
    if ship.docked_at.as_ref() != Some(station) {
        return Err(env.fail(
            lua,
            "not_docked",
            format!("the player isn't docked at `{station}`"),
        ));
    }
    Ok(ship)
}

/// `ctx.orders`, `ctx.beats`, `ctx.mechanics`, `ctx.standing`, `ctx.traits`,
/// `ctx.pacing`.
pub(super) fn namespaces<'scope, 'env: 'scope>(
    lua: &Lua,
    scope: &'scope Scope<'scope, 'env>,
    env: &'env CallEnv<'env>,
) -> mlua::Result<Vec<(&'static str, Table)>> {
    // --- ctx.orders ---
    let orders = lua.create_table()?;
    method(
        env,
        &orders,
        "orders",
        "get",
        m(scope, env, |lua, env, ship: LuaString| {
            let ship = env.arg(lua, &ship)?;
            to_lua(lua, &env.world.ships.get(ship.as_str()).map(|s| &s.orders))
        })?,
    )?;
    method(
        env,
        &orders,
        "orders",
        "can_order",
        m(scope, env, |lua, env, ship: LuaString| {
            let ship = env.arg(lua, &ship)?;
            let w = env.world;
            Ok(w.ships.contains(ship.as_str())
                && w.player.ship.as_ref().is_none_or(|p| p.as_str() != ship)
                && ShipId::new(&ship)
                    .is_ok_and(|id| !galaxy_progress::prestige::is_berthed(w, &id)))
        })?,
    )?;
    method(
        env,
        &orders,
        "orders",
        "give",
        m(
            scope,
            env,
            |lua, env, (ship, list, why): (LuaString, Table, Table)| {
                let ship = env.orderable(lua, &ship)?;
                env.measured(lua, &list, "orders")?;
                let orders = list
                    .sequence_values::<Value>()
                    .map(|v| v.and_then(|v| env.order(lua, v)))
                    .collect::<mlua::Result<Vec<_>>>()?;
                env.push(
                    lua,
                    &why,
                    Command::SetOrders {
                        ship,
                        orders: OrderQueue {
                            orders,
                            ..OrderQueue::default()
                        },
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        &orders,
        "orders",
        "append",
        m(
            scope,
            env,
            |lua, env, (ship, order, why): (LuaString, Value, Table)| {
                let ship = env.orderable(lua, &ship)?;
                let order = env.order(lua, order)?;
                // Builds on orders given earlier in this call, not just the world's.
                let earlier = env
                    .buffer
                    .borrow()
                    .commands
                    .iter()
                    .rev()
                    .find_map(|c| match c {
                        Command::SetOrders { ship: s, orders } if *s == ship => {
                            Some(orders.clone())
                        }
                        _ => None,
                    });
                let mut queue = earlier
                    .or_else(|| env.world.ships.get(&ship).map(|s| s.orders.clone()))
                    .unwrap_or_default();
                queue.orders.push(order);
                env.push(
                    lua,
                    &why,
                    Command::SetOrders {
                        ship,
                        orders: queue,
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        &orders,
        "orders",
        "clear",
        m(scope, env, |lua, env, (ship, why): (LuaString, Table)| {
            let ship = env.orderable(lua, &ship)?;
            env.push(
                lua,
                &why,
                Command::SetOrders {
                    ship,
                    orders: OrderQueue::default(),
                },
            )
        })?,
    )?;

    // --- ctx.beats ---
    let beat_ns = lua.create_table()?;
    method(
        env,
        &beat_ns,
        "beats",
        "state",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.progress.beats.get(id.as_str()))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "is_done",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            Ok(env
                .world
                .progress
                .beats
                .get(id.as_str())
                .is_some_and(|b| b.status == BeatStatus::Completed))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "is_active",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            Ok(env
                .world
                .progress
                .beats
                .get(id.as_str())
                .is_some_and(|b| b.status == BeatStatus::Active))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "active",
        m(scope, env, |_, env, (): ()| {
            Ok(beats::goal_lines(env.world)
                .into_iter()
                .map(|g| g.beat.to_string())
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "goal",
        m(scope, env, |lua, env, (): ()| {
            beats::hud_goal(env.world)
                .map(|g| goal_table(lua, &g))
                .transpose()
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "goals",
        m(scope, env, |lua, env, (): ()| {
            let scanned = u64::try_from(env.world.progress.beats.len()).unwrap_or(u64::MAX);
            env.budget.charge(lua, 1 + scanned / RECORDS_PER_STEP)?;
            beats::goal_lines(env.world)
                .iter()
                .map(|g| goal_table(lua, g))
                .collect::<mlua::Result<Vec<_>>>()
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "act",
        m(scope, env, |_, env, (): ()| {
            Ok(beats::current_act(env.world))
        })?,
    )?;

    method(
        env,
        &beat_ns,
        "beats",
        "pilot",
        m(scope, env, |lua, env, (): ()| {
            let w = env.world;
            let ship = galaxy_progress::eval::player_ship(w);
            let t = lua.create_table()?;
            if let Some(s) = ship {
                t.raw_set("ship", s.id.as_str())?;
                t.raw_set("hull", s.hull.as_str())?;
                t.raw_set("docked_at", s.docked_at.as_ref().map(|d| d.to_string()))?;
                t.raw_set("region", s.region.as_str())?;
                t.raw_set("position", point(lua, s.position)?)?;
            }
            let cargo = lua.create_table()?;
            for (ware, n) in ship.map(|s| &s.hold.cargo).into_iter().flatten() {
                cargo.raw_set(ware.as_str(), *n)?;
            }
            t.raw_set("cargo", cargo)?;
            t.raw_set("hold_free", galaxy_progress::eval::hold_free(w, ship))?;
            t.raw_set("credits", w.player.credits as f64 / 100.0)?;
            t.raw_set("name", w.player.name.as_str())?;
            Ok(t)
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "distance",
        m(scope, env, |lua, env, to: Value| {
            let (region, at) = env.spot(lua, to)?;
            let Some(ship) = galaxy_progress::eval::player_ship(env.world) else {
                return Ok(None);
            };
            if region.as_ref().is_some_and(|r| *r != ship.region) {
                return Ok(None);
            }
            Ok(Some(f64::from(ship.position.distance(at))))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "field",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            let w = env.world;
            let Some(field) = w.fields.get(id.as_str()) else {
                return Ok(None);
            };
            // The scan visits every rock in the world, so it pays for all of them first.
            let scanned = u64::try_from(w.rocks.len()).unwrap_or(u64::MAX);
            env.budget.charge(lua, 1 + scanned / RECORDS_PER_STEP)?;
            let (mut rocks, mut remaining, mut full) = (0u32, 0u64, 0u64);
            for r in w.rocks.values().filter(|r| r.field == field.id) {
                rocks = rocks.saturating_add(1);
                remaining += u64::from(r.remaining);
                full += u64::from(r.initial);
            }
            let t = lua.create_table()?;
            t.raw_set("id", field.id.as_str())?;
            t.raw_set("region", field.region.as_str())?;
            t.raw_set("rocks", rocks)?;
            t.raw_set("remaining", remaining)?;
            t.raw_set("full", full)?;
            t.raw_set("left", share(remaining, full))?;
            t.raw_set("regrow", f64::from(field.regrow))?;
            Ok(Some(t))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "rock",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            let Some(r) = env.world.rocks.get(id.as_str()) else {
                return Ok(None);
            };
            let t = lua.create_table()?;
            t.raw_set("id", r.id.as_str())?;
            t.raw_set("field", r.field.as_str())?;
            t.raw_set("position", point(lua, r.position)?)?;
            t.raw_set("remaining", r.remaining)?;
            t.raw_set("initial", r.initial)?;
            t.raw_set("left", share(u64::from(r.remaining), u64::from(r.initial)))?;
            Ok(Some(t))
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "deliver",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.beats:deliver",
                &["station", "ware", "quantity", "ledger"],
            )?;
            let station = env.station(lua, &spec.get::<LuaString>("station")?)?;
            let ware: WareId = env.id(lua, &spec.get::<LuaString>("ware")?)?;
            let quantity = units(env, lua, &spec.get::<Value>("quantity")?)?;
            let ledger = spec
                .get::<Option<LuaString>>("ledger")?
                .map(|l| env.id::<LedgerId>(lua, &l))
                .transpose()?;
            let ship = docked_player(env, lua, &station)?;
            if ship.hold.count(&ware) < quantity {
                return Err(env.fail(
                    lua,
                    "not_enough_cargo",
                    format!(
                        "the player carries {} {ware}, not {quantity}",
                        ship.hold.count(&ware)
                    ),
                ));
            }
            // Checked again when it applies: an undock earlier in the tick rejects it.
            env.push(
                lua,
                &why,
                Command::ExpectDocked {
                    ship: ship.id.clone(),
                    station: station.clone(),
                },
            )?;
            env.push(
                lua,
                &why,
                Command::AdjustCargo {
                    ship: ship.id.clone(),
                    ware: ware.clone(),
                    delta: -i64::from(quantity),
                },
            )?;
            env.push(
                lua,
                &why,
                Command::Emit {
                    event: Event::Deliver {
                        ship: ship.id.clone(),
                        station,
                        ware,
                        quantity,
                        ledger,
                    },
                },
            )
        })?,
    )?;
    method(
        env,
        &beat_ns,
        "beats",
        "load",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.beats:load",
                &["ware", "quantity", "found"],
            )?;
            let ware: WareId = env.id(lua, &spec.get::<LuaString>("ware")?)?;
            if !env.world.wares.contains(&ware) {
                return Err(env.fail(lua, "unknown_id", format!("no ware `{ware}`")));
            }
            let quantity = units(env, lua, &spec.get::<Value>("quantity")?)?;
            let found = spec.get::<Option<bool>>("found")?.unwrap_or(false);
            let ship = galaxy_progress::eval::player_ship(env.world)
                .ok_or_else(|| env.fail(lua, "no_ship", "the player has no ship"))?;
            let station = ship.docked_at.clone();
            if station.is_none() && !found {
                return Err(env.fail(lua, "not_docked", "cargo is loaded at a dock"));
            }
            let free = galaxy_progress::eval::hold_free(env.world, Some(ship));
            if free < u64::from(quantity) {
                return Err(env.fail(
                    lua,
                    "no_room",
                    format!("the hold has room for {free}, not {quantity}"),
                ));
            }
            // Checked when it applies: a consignment needs the ship still docked; a
            // find needs only the room, which the hold invariant checks.
            if let Some(station) = station.filter(|_| !found) {
                env.push(
                    lua,
                    &why,
                    Command::ExpectDocked {
                        ship: ship.id.clone(),
                        station,
                    },
                )?;
            }
            env.push(
                lua,
                &why,
                Command::AdjustCargo {
                    ship: ship.id.clone(),
                    ware,
                    delta: i64::from(quantity),
                },
            )
        })?,
    )?;

    method(
        env,
        &beat_ns,
        "beats",
        "pay",
        m(scope, env, |lua, env, (credits, why): (Value, Table)| {
            let credits = number(env, lua, &credits, "credits")?;
            let cents = (credits * CENTS_PER_CREDIT as f64 * 100.0).round() / 100.0;
            if !((1.0..=1e12).contains(&cents) && cents.fract() == 0.0) {
                return Err(env.fail(
                    lua,
                    "bad_number",
                    "`credits` is more than 0, in whole centicredits (40.25), and below 10^10",
                ));
            }
            env.push(
                lua,
                &why,
                Command::AdjustCredits {
                    delta: cents as i64,
                },
            )
        })?,
    )?;

    // --- ctx.mechanics ---
    let mech = lua.create_table()?;
    method(
        env,
        &mech,
        "mechanics",
        "is_on",
        m(scope, env, |lua, env, flag: LuaString| {
            let flag = env.arg(lua, &flag)?;
            Ok(env.world.player.flags.contains(flag.as_str()))
        })?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "slots",
        m(scope, env, |lua, env, (): ()| {
            to_lua(lua, &env.world.progress.slots)
        })?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "free_slots",
        m(scope, env, |_, env, (): ()| {
            let p = &env.world.progress;
            Ok(p.rules
                .slots
                .keys()
                .filter(|s| !p.slots.contains_key(*s))
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "set",
        m(
            scope,
            env,
            |lua, env, (flag, on, why): (LuaString, bool, Table)| {
                let flag = env.mech_flag(lua, &flag)?;
                env.push(lua, &why, Command::SetFlag { flag, on })
            },
        )?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "bind",
        m(
            scope,
            env,
            |lua, env, (slot, mechanic, why): (LuaString, LuaString, Table)| {
                let slot = env.slot(lua, &slot)?;
                let mechanic = env.mech_flag(lua, &mechanic)?;
                if let Some(other) = env
                    .world
                    .progress
                    .slots
                    .get(&slot)
                    .filter(|m| **m != mechanic)
                {
                    return Err(env.fail(
                        lua,
                        "slot_taken",
                        format!("`{slot}` is already bound to `{other}`"),
                    ));
                }
                env.push(
                    lua,
                    &why,
                    Command::BindSlot {
                        slot,
                        mechanic: Some(mechanic),
                    },
                )
            },
        )?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "unbind",
        m(scope, env, |lua, env, (slot, why): (LuaString, Table)| {
            let slot = env.slot(lua, &slot)?;
            env.push(
                lua,
                &why,
                Command::BindSlot {
                    slot,
                    mechanic: None,
                },
            )
        })?,
    )?;
    method(
        env,
        &mech,
        "mechanics",
        "teach",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.mechanics:teach",
                &["mechanic", "teacher", "slots"],
            )?;
            let mechanic = env.mech_flag(lua, &spec.get::<LuaString>("mechanic")?)?;
            let teacher: NpcId = env.id(lua, &spec.get::<LuaString>("teacher")?)?;
            if !env.world.npcs.contains(&teacher) {
                return Err(env.fail(lua, "unknown_id", format!("no npc `{teacher}`")));
            }
            env.push(
                lua,
                &why,
                Command::SetFlag {
                    flag: mechanic.clone(),
                    on: true,
                },
            )?;
            for s in spec
                .get::<Option<Vec<LuaString>>>("slots")?
                .unwrap_or_default()
            {
                let slot = env.slot(lua, &s)?;
                if env
                    .world
                    .progress
                    .slots
                    .get(&slot)
                    .is_some_and(|m| *m != mechanic)
                {
                    return Err(env.fail(lua, "slot_taken", format!("`{slot}` is already bound")));
                }
                env.push(
                    lua,
                    &why,
                    Command::BindSlot {
                        slot,
                        mechanic: Some(mechanic.clone()),
                    },
                )?;
            }
            let ns = env.pack.as_str().split('.').next().unwrap_or("core");
            let kind = FactKind::new(format!("{ns}.mechanic_taught"))
                .map_err(|e| env.fail(lua, "bad_id", e))?;
            env.push(
                lua,
                &why,
                Command::RecordFact {
                    fact: Box::new(galaxy_contract::history::FactDraft {
                        kind,
                        subjects: Subjects {
                            npc: Some(teacher.clone()),
                            other: AnyId::new(mechanic.as_str()).ok(),
                            ..Subjects::default()
                        },
                        amount: None,
                        note: format!("{teacher} taught {mechanic}"),
                    }),
                },
            )
        })?,
    )?;

    // --- ctx.standing ---
    let stand = lua.create_table()?;
    method(
        env,
        &stand,
        "standing",
        "tier",
        m(scope, env, |lua, env, faction: LuaString| {
            let faction = env.faction(lua, &faction)?;
            to_lua(lua, &standing::tier(env.world, &faction))
        })?,
    )?;
    method(
        env,
        &stand,
        "standing",
        "station_tier",
        m(scope, env, |lua, env, station: LuaString| {
            let station = env.station(lua, &station)?;
            to_lua(lua, &standing::station_tier(env.world, &station))
        })?,
    )?;
    method(
        env,
        &stand,
        "standing",
        "tiers",
        m(scope, env, |lua, env, (): ()| {
            to_lua(lua, &env.world.progress.rules.regard)
        })?,
    )?;
    method(
        env,
        &stand,
        "standing",
        "deed",
        m(scope, env, |lua, env, (spec, why): (Table, Table)| {
            only_keys(
                env,
                lua,
                &spec,
                "ctx.standing:deed",
                &["faction", "deed", "delta", "tags"],
            )?;
            let faction = env.faction(lua, &spec.get::<LuaString>("faction")?)?;
            let deed: AnyId = env.id(lua, &spec.get::<LuaString>("deed")?)?;
            let delta: i32 = env.int(lua, &spec.get::<Value>("delta")?, "delta")?;
            let tags: Vec<String> = spec
                .get::<Option<Vec<LuaString>>>("tags")?
                .unwrap_or_default()
                .iter()
                .map(|t| env.arg(lua, t))
                .collect::<mlua::Result<_>>()?;
            // Counts the standing this call has already written (earlier deeds, a beat
            // transition's rewards), so the clamp and the tier hold across the call.
            let mut deeds = standing::Deeds::after(env.world, &env.buffer.borrow().commands);
            deeds.deed(&faction, &deed, delta, &tags);
            for c in deeds.commands {
                env.push(lua, &why, c)?;
            }
            Ok(())
        })?,
    )?;

    // --- ctx.traits ---
    let traits = lua.create_table()?;
    method(
        env,
        &traits,
        "traits",
        "has",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            Ok(env.world.player.traits.contains(id.as_str()))
        })?,
    )?;
    method(
        env,
        &traits,
        "traits",
        "earned",
        m(scope, env, |_, env, (): ()| {
            Ok(env
                .world
                .player
                .traits
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>())
        })?,
    )?;
    method(
        env,
        &traits,
        "traits",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(lua, &env.world.progress.rules.traits.get(id.as_str()))
        })?,
    )?;

    // --- ctx.pacing ---
    let pacing = lua.create_table()?;
    method(
        env,
        &pacing,
        "pacing",
        "get",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            to_lua(
                lua,
                &env.world
                    .progress
                    .tunables
                    .get(id.as_str())
                    .map(|t| &t.value),
            )
        })?,
    )?;
    method(
        env,
        &pacing,
        "pacing",
        "tuned",
        m(scope, env, |lua, env, id: LuaString| {
            let id = env.arg(lua, &id)?;
            Ok(env
                .world
                .progress
                .tunables
                .get(id.as_str())
                .is_some_and(|t| t.tuned()))
        })?,
    )?;
    method(
        env,
        &pacing,
        "pacing",
        "set",
        m(
            scope,
            env,
            |lua, env, (id, value, why): (LuaString, Value, Table)| {
                let id: TunableId = env.id(lua, &id)?;
                if let Value::Table(t) = &value {
                    env.measured(lua, t, "a tunable")?;
                }
                let value: TunableValue = lua
                    .from_value(value)
                    .map_err(|e| env.fail(lua, "bad_argument", e))?;
                if let TunableValue::Number(n) = value
                    && !n.is_finite()
                {
                    return Err(env.fail(lua, "bad_argument", "a tunable must be a finite number"));
                }
                let existing = env.world.progress.tunables.get(&id);
                env.check_new_id(lua, id.as_str(), existing.is_some())?;
                let tunable = Tunable {
                    authored: existing.map_or_else(|| value.clone(), |t| t.authored.clone()),
                    value,
                    set_by: Some(env.pack.clone()),
                };
                env.push(
                    lua,
                    &why,
                    Command::SetTunable {
                        id,
                        value: Some(Box::new(tunable)),
                    },
                )
            },
        )?,
    )?;

    Ok(vec![
        ("orders", orders),
        ("beats", beat_ns),
        ("mechanics", mech),
        ("standing", stand),
        ("traits", traits),
        ("pacing", pacing),
    ])
}
