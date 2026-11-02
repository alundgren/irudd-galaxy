//! The world-owned trip; presentations can be replaced without losing its endpoints.
use crate::{Input, World};
use galaxy_api::Registry;
use galaxy_api::command::{Batch, Cause, Command};
use galaxy_api::engine::{BEING, LOCATION, PLACE, PLACE_ARRIVED, POSITION, TRAVEL};
use serde_json::json;

pub const CUT_TICKS: u64 = 30;

pub fn player(world: &World) -> Option<(&str, &str)> {
    world.entities.iter().find_map(|(id, c)| {
        (c.get(BEING)?.fields["player"] == true).then_some(())?;
        Some((id.as_str(), c.get(LOCATION)?.fields["place"].as_str()?))
    })
}

/// The player's source pose, captured once before stepping.
pub struct Departure {
    id: String,
    place: String,
    fields: galaxy_api::Fields,
}

pub fn position(world: &World) -> Option<Departure> {
    let (id, place) = player(world)?;
    world.component(id, TRAVEL)?;
    let mut fields = galaxy_api::Fields::new();
    fields.insert(
        "from_spot".into(),
        world.component(id, LOCATION)?.fields["spot"].clone(),
    );
    if let Some(p) = world.component(id, POSITION) {
        for key in ["x", "y", "height", "angle", "layer"] {
            fields.insert(format!("from_{key}"), p.fields[key].clone());
        }
    }
    Some(Departure {
        id: id.into(),
        place: place.into(),
        fields,
    })
}

pub fn start(
    world: &mut World,
    before: Option<Departure>,
    registry: &Registry,
) -> Result<(), String> {
    let Some(Departure {
        id,
        place: from,
        mut fields,
    }) = before
    else {
        return Ok(());
    };
    let Some((now, to)) = player(world) else {
        return Ok(());
    };
    if world.component(now, TRAVEL).is_none() {
        return Ok(());
    }
    if id != now || from == to {
        return Ok(());
    }
    let mut ancestor = from.as_str();
    let mut reverse = false;
    for _ in 0..world.entities.len() {
        ancestor = world
            .component(ancestor, PLACE)
            .and_then(|c| c.fields["parent"].as_str())
            .unwrap_or("");
        if ancestor.is_empty() {
            break;
        }
        if ancestor == to {
            reverse = true;
            break;
        }
    }
    fields.extend(json!({"active": true, "from": from, "to": to, "start_tick": world.tick, "reverse": reverse}).as_object().unwrap().clone());
    world.apply(
        &Batch {
            cause: Cause::Engine {
                service: "travel".into(),
            },
            commands: vec![Command::Put {
                entity: id,
                component: TRAVEL.into(),
                fields,
            }],
        },
        registry,
    )
}

/// Play supplies this input; replay never invents it. A click can finish before due.
pub fn arrival(world: &World, clicked: bool) -> Option<Input> {
    let (id, place) = player(world)?;
    let t = &world.component(id, TRAVEL)?.fields;
    if t["active"] != true || t["to"] != place {
        return None;
    }
    let start = t["start_tick"].as_u64()?;
    if !clicked && world.tick + 1 < start + CUT_TICKS {
        return None;
    }
    Some(Input {
        tick: world.tick + 1,
        source: "travel".into(),
        name: PLACE_ARRIVED.into(),
        payload: json!({"entity": id, "place": place, "start_tick": start})
            .as_object()
            .unwrap()
            .clone(),
    })
}

/// Reject stale/duplicate arrivals before scripts see them; finish in the input batch.
pub fn finish(world: &World, input: &Input) -> Result<Option<Command>, String> {
    if input.name != PLACE_ARRIVED {
        return Ok(None);
    }
    let expected = arrival(world, true).ok_or("no active trip to arrive from")?;
    if input.payload != expected.payload {
        return Err("arrival does not match the active trip".into());
    }
    Ok(Some(Command::Patch {
        entity: expected.payload["entity"].as_str().unwrap().into(),
        component: TRAVEL.into(),
        fields: json!({"active": false}).as_object().unwrap().clone(),
    }))
}
