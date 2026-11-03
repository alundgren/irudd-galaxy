//! The world-owned trip; presentations can be replaced without losing its endpoints.
use crate::{Input, World};
use galaxy_api::Registry;
use galaxy_api::command::{Batch, Cause, Command};
use galaxy_api::engine::{
    BEING, CAMERA_SETTLED, LOCATION, PLACE, PLACE_ARRIVED, POSITION, ROOM, TICKS_PER_SECOND, TRAVEL,
};
use serde_json::json;

pub const CUT_TICKS: u64 = 30;

/// The longest glide a room may ask for, in seconds.
pub const MAX_GLIDE: f64 = 10.0;

/// A room's or trip's `glide`, in seconds within `0..=MAX_GLIDE`; 0 is the fade cut.
pub fn glide_seconds(fields: &galaxy_api::Fields) -> f64 {
    fields["glide"]
        .as_f64()
        .unwrap_or(0.0)
        .clamp(0.0, MAX_GLIDE)
}

/// The camera node a room shows: its `camera`, or `camera` when empty. None for a place
/// without `engine.room`.
pub fn room_camera(world: &World, place: &str) -> Option<String> {
    let room = &world.component(place, ROOM)?.fields;
    Some(
        room["camera"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("camera")
            .to_owned(),
    )
}

/// Whether `name` is an input that lands the player's trip or camera glide.
pub fn is_landing(name: &str) -> bool {
    name == PLACE_ARRIVED || name == CAMERA_SETTLED
}

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
    camera: Option<String>,
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
        camera: room_camera(world, place),
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
        camera,
        mut fields,
    }) = before
    else {
        return Ok(());
    };
    let Some((now, to)) = player(world) else {
        return Ok(());
    };
    let Some(trip) = world.component(now, TRAVEL) else {
        return Ok(());
    };
    if id != now {
        return Ok(());
    }
    let glide = world
        .component(to, ROOM)
        .map_or(0.0, |r| glide_seconds(&r.fields));
    if from == to {
        // A new camera node in the player's room glides when the room asks for it. A trip
        // still landing keeps going: its glide already aims at the room's current camera.
        let now_camera = room_camera(world, to);
        if glide <= 0.0 || now_camera == camera || trip.fields["active"] == true {
            return Ok(());
        }
        fields.extend(
            json!({"active": true, "from": from, "to": to, "start_tick": world.tick,
                "reverse": false, "glide": glide, "from_camera": camera})
            .as_object()
            .unwrap()
            .clone(),
        );
        return world.apply(
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
        );
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
    fields.extend(json!({"active": true, "from": from, "to": to, "start_tick": world.tick, "reverse": reverse, "glide": glide}).as_object().unwrap().clone());
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

/// Play supplies this input; replay never invents it. A click can finish before due. A
/// trip between places lands with `engine.place_arrived`, a camera glide within one room
/// with `engine.camera_settled`; a glide is due after its own length, a cut after one
/// second.
pub fn arrival(world: &World, clicked: bool) -> Option<Input> {
    let (id, place) = player(world)?;
    let t = &world.component(id, TRAVEL)?.fields;
    if t["active"] != true || t["to"] != place {
        return None;
    }
    let start = t["start_tick"].as_u64()?;
    let glide = glide_seconds(t);
    let due = if glide > 0.0 {
        (glide * TICKS_PER_SECOND as f64).ceil() as u64
    } else {
        CUT_TICKS
    };
    if !clicked && world.tick + 1 < start + due {
        return None;
    }
    Some(Input {
        tick: world.tick + 1,
        source: "travel".into(),
        name: if t["from"] == t["to"] {
            CAMERA_SETTLED
        } else {
            PLACE_ARRIVED
        }
        .into(),
        payload: json!({"entity": id, "place": place, "start_tick": start})
            .as_object()
            .unwrap()
            .clone(),
    })
}

/// Reject stale/duplicate arrivals before scripts see them; finish in the input batch.
pub fn finish(world: &World, input: &Input) -> Result<Option<Command>, String> {
    if !is_landing(&input.name) {
        return Ok(None);
    }
    let expected = arrival(world, true).ok_or("no active trip to arrive from")?;
    if input.name != expected.name || input.payload != expected.payload {
        return Err("arrival does not match the active trip".into());
    }
    Ok(Some(Command::Patch {
        entity: expected.payload["entity"].as_str().unwrap().into(),
        component: TRAVEL.into(),
        fields: json!({"active": false}).as_object().unwrap().clone(),
    }))
}
