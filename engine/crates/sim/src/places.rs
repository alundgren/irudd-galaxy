//! Places, locations and beings are ordinary components. Walks are reconstructible
//! straight segments, advanced here in Rust. No presentation or game clock drives them.

use std::collections::BTreeSet;

use galaxy_api::command::{Command, Fields};
use galaxy_api::engine::{
    BEING, CLOCK, DOOR, FLIGHT, LOCATION, MOTION, PLACE, POSITION, SPOT, STEER, TICK_SECONDS, WALK,
    WALK_FINISHED,
};
use serde_json::{Value, json};

use crate::world::World;

fn text<'a>(f: &'a Fields, key: &str) -> &'a str {
    f.get(key).and_then(Value::as_str).unwrap_or("")
}

fn number(f: &Fields, key: &str) -> f64 {
    f.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

fn fields<'a>(world: &'a World, entity: &str, component: &str) -> Result<&'a Fields, String> {
    world
        .component(entity, component)
        .map(|c| &c.fields)
        .ok_or_else(|| format!("`{entity}` has no `{component}`"))
}

/// Empty is the original shared plane. Places are separate coordinate spaces, even
/// when one is the parent of another.
pub fn place_of<'a>(world: &'a World, entity: &str) -> &'a str {
    world
        .component(entity, LOCATION)
        .map_or("", |c| text(&c.fields, "place"))
}

pub fn walking(world: &World, entity: &str) -> bool {
    world
        .component(entity, WALK)
        .is_some_and(|c| c.fields["active"] == true)
}

fn spot_in<'a>(world: &'a World, spot: &str, place: &str) -> Result<&'a Fields, String> {
    let f = fields(world, spot, SPOT)?;
    if text(f, "place") != place {
        return Err(format!("spot `{spot}` is not in place `{place}`"));
    }
    Ok(f)
}

fn chain(world: &World, start: &str, field: &str) -> Result<String, String> {
    let mut at = start;
    let mut seen = BTreeSet::new();
    loop {
        if !seen.insert(at) {
            return Err(format!("place `{start}` has a cycle in `{field}`"));
        }
        let f = fields(world, at, PLACE)?;
        let next = text(f, field);
        if next.is_empty() {
            return Ok(at.to_owned());
        }
        at = next;
    }
}

/// Checks authored references after a whole batch, so forward references and a complete
/// rearrangement in one handler work. Start states are checked after all modules load.
pub fn validate(world: &World) -> Result<(), String> {
    let mut names = BTreeSet::new();
    let mut player = None;
    for (id, components) in &world.entities {
        if let Some(c) = components.get(PLACE) {
            chain(world, id, "parent")?;
            chain(world, id, "enter")?;
            for size in ["width", "depth"] {
                if number(&c.fields, size) < 0.0 {
                    return Err(format!("place `{id}` has negative {size}"));
                }
            }
            let arrival = text(&c.fields, "arrival");
            if !arrival.is_empty() {
                spot_in(world, arrival, id)?;
            }
        }
        if let Some(c) = components.get(SPOT) {
            let place = text(&c.fields, "place");
            fields(world, place, PLACE)?;
            let name = text(&c.fields, "name");
            if name.is_empty() || !names.insert((place, name)) {
                return Err(format!(
                    "spot `{id}` needs a nonempty name unique within `{place}`"
                ));
            }
        }
        if let Some(c) = components.get(DOOR) {
            for (side, spot) in [("from", "spot"), ("to", "arrival")] {
                let place = text(&c.fields, side);
                fields(world, place, PLACE)?;
                let spot = text(&c.fields, spot);
                if !spot.is_empty() {
                    spot_in(world, spot, place)?;
                }
            }
        }
        if let Some(c) = components.get(LOCATION) {
            let place = text(&c.fields, "place");
            if !place.is_empty() {
                fields(world, place, PLACE)?;
            }
            let spot = text(&c.fields, "spot");
            if !spot.is_empty() {
                spot_in(world, spot, place)?;
            }
        }
        if let Some(c) = components.get(BEING) {
            let loc = fields(world, id, LOCATION)?;
            fields(world, text(loc, "place"), PLACE)?;
            let home = text(&c.fields, "home");
            if !home.is_empty() {
                fields(world, home, PLACE)?;
            }
            if c.fields["player"] == true {
                if let Some(other) = player {
                    return Err(format!("both `{other}` and `{id}` are the player"));
                }
                player = Some(id);
            }
        }
        if let Some(c) = components.get(CLOCK)
            && c.fields["value"].as_i64().unwrap_or(0) < 0
        {
            return Err(format!("clock `{id}` must be nonnegative"));
        }
        if let Some(c) = components.get(WALK)
            && c.fields["active"] == true
        {
            let w = &c.fields;
            let place = text(w, "place");
            fields(world, place, PLACE)?;
            spot_in(world, text(w, "to"), place)?;
            let from = text(w, "from");
            if !from.is_empty() {
                spot_in(world, from, place)?;
            }
            let p = fields(world, id, POSITION)?;
            if place_of(world, id) != place || p["layer"] != w["layer"] {
                return Err(format!(
                    "walk `{id}` must stay in its place and floor layer"
                ));
            }
            if components.contains_key(FLIGHT) {
                return Err(format!("`{id}` cannot walk with engine.flight"));
            }
            let start = w["start_tick"].as_i64().unwrap_or(-1);
            let distance = libm::hypot(
                number(w, "to_x") - number(w, "from_x"),
                number(w, "to_y") - number(w, "from_y"),
            );
            if start < 0
                || start as u64 > world.tick
                || number(w, "speed") <= 0.0
                || !distance.is_finite()
            {
                return Err(format!(
                    "walk `{id}` needs a valid start tick, positive speed and finite distance"
                ));
            }
        }
    }
    Ok(())
}

fn patch(entity: &str, component: &str, value: Value) -> Command {
    Command::Patch {
        entity: entity.into(),
        component: component.into(),
        fields: value.as_object().unwrap().clone(),
    }
}

fn stop(world: &World, entity: &str, out: &mut Vec<Command>) {
    if world.component(entity, MOTION).is_some() {
        out.push(patch(entity, MOTION, json!({"vx": 0.0, "vy": 0.0})));
    }
    if world.component(entity, STEER).is_some() {
        out.push(patch(entity, STEER, json!({"mode": "", "done": false})));
    }
}

/// Expands a high-level command against the world at its point in the batch. Every
/// resulting write still passes the caller's component capabilities and shares its cause.
pub(crate) fn expand(world: &World, command: &Command) -> Result<Vec<Command>, String> {
    let mut out = Vec::new();
    match command {
        Command::Move {
            entity,
            place,
            spot,
            state,
        } => {
            fields(world, entity, LOCATION)?;
            let place = chain(world, place, "enter")?;
            let p = fields(world, &place, PLACE)?;
            let spot = spot.as_deref().unwrap_or_else(|| text(p, "arrival"));
            let (x, y, layer) = if spot.is_empty() {
                (0.0, 0.0, 0)
            } else {
                let s = spot_in(world, spot, &place)?;
                (
                    number(s, "x"),
                    number(s, "y"),
                    s["layer"].as_i64().unwrap_or(0),
                )
            };
            out.push(patch(
                entity,
                LOCATION,
                json!({"place": place, "spot": spot}),
            ));
            // Preserve facing when it exists; a newly located entity starts facing +x.
            let mut position = world
                .component(entity, POSITION)
                .map(|c| c.fields.clone())
                .unwrap_or_default();
            position.extend(
                json!({"x": x, "y": y, "layer": layer, "height": 0.0})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
            out.push(Command::Put {
                entity: entity.clone(),
                component: POSITION.into(),
                fields: position,
            });
            if world.component(entity, WALK).is_some() {
                out.push(patch(entity, WALK, json!({"active": false})));
            }
            if let Some(state) = state {
                out.push(patch(entity, BEING, json!({"state": state})));
            }
            stop(world, entity, &mut out);
        }
        Command::Walk {
            entity,
            spot,
            speed,
            state,
        } => {
            let loc = fields(world, entity, LOCATION)?;
            let place = text(loc, "place");
            let target = spot_in(world, spot, place)?;
            let p = fields(world, entity, POSITION)?;
            if p["layer"] != target["layer"] {
                return Err("walk stays on one floor layer; move to change layers".into());
            }
            if world.component(entity, FLIGHT).is_some() {
                return Err(format!("`{entity}` cannot walk with engine.flight"));
            }
            let current_state = world
                .component(entity, BEING)
                .map_or("", |c| text(&c.fields, "state"));
            if state.is_some() {
                fields(world, entity, BEING)?;
            }
            let start = i64::try_from(world.tick).map_err(|_| "walk start tick is too large")?;
            let w = json!({"active": true, "place": place, "from": loc["spot"], "to": spot,
                "from_x": p["x"], "from_y": p["y"], "to_x": target["x"], "to_y": target["y"],
                "layer": p["layer"], "start_tick": start, "speed": speed,
                "state": state.as_deref().unwrap_or(current_state)});
            out.push(Command::Put {
                entity: entity.clone(),
                component: WALK.into(),
                fields: w.as_object().unwrap().clone(),
            });
            out.push(patch(entity, LOCATION, json!({"spot": null})));
            stop(world, entity, &mut out);
        }
        Command::AdvanceClock { entity, amount } => {
            let clock = fields(world, entity, CLOCK)?;
            let value = clock["value"]
                .as_i64()
                .unwrap_or(0)
                .checked_add(*amount)
                .ok_or("clock overflow")?;
            out.push(patch(entity, CLOCK, json!({"value": value})));
        }
        _ => unreachable!("only high-level place commands expand"),
    }
    Ok(out)
}

/// Advance absolute segments, never accumulated velocity. The saved segment is enough
/// to rebuild presentation at any tick, including after a save or while out of view.
pub fn advance_walks(world: &mut World) {
    let walks: Vec<_> = world
        .entities
        .iter()
        .filter_map(|(id, c)| {
            let w = &c.get(WALK)?.fields;
            (w["active"] == true).then(|| (id.clone(), w.clone()))
        })
        .collect();
    if walks.is_empty() {
        return;
    }
    let cause = world.service_cause("walk");
    for (id, w) in walks {
        let dx = number(&w, "to_x") - number(&w, "from_x");
        let dy = number(&w, "to_y") - number(&w, "from_y");
        let distance = libm::hypot(dx, dy);
        let elapsed = world
            .tick
            .saturating_sub(w["start_tick"].as_u64().unwrap_or(world.tick));
        let progress = if distance == 0.0 {
            1.0
        } else {
            ((elapsed as f64 * TICK_SECONDS * number(&w, "speed")) / distance).min(1.0)
        };
        for (key, delta) in [("x", dx), ("y", dy)] {
            let value = if progress >= 1.0 {
                number(&w, &format!("to_{key}"))
            } else {
                number(&w, &format!("from_{key}")) + delta * progress
            };
            crate::services::set(world, &id, POSITION, key, Value::from(value), cause);
        }
        if progress >= 1.0 {
            crate::services::set(world, &id, WALK, "active", Value::Bool(false), cause);
            crate::services::set(world, &id, LOCATION, "spot", w["to"].clone(), cause);
            crate::services::set(world, &id, BEING, "state", w["state"].clone(), cause);
            world.record(
                cause,
                WALK_FINISHED,
                json!({"entity": id, "place": w["place"], "spot": w["to"]})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        }
    }
}
