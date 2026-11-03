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
    // A glide that has not landed when the camera changes again becomes the new one's source.
    let interrupted = interrupted(&trip.fields, camera.as_deref());
    if from == to {
        // A new camera node in the player's room glides when the room asks for it. A fade
        // cut still running keeps going: it shows the room's current camera as it fades in.
        let now_camera = room_camera(world, to);
        let landing = trip.fields["active"] == true;
        if glide <= 0.0 || now_camera == camera || (landing && glide_seconds(&trip.fields) <= 0.0) {
            return Ok(());
        }
        if landing && trip.fields["from"] != trip.fields["to"] {
            // A trip between places still gliding in keeps its endpoints, so it still lands
            // with `engine.place_arrived`; it glides on from where the camera is now.
            fields = trip.fields.clone();
            fields.extend(
                json!({"start_tick": world.tick, "glide": glide, "interrupted": interrupted})
                    .as_object()
                    .unwrap()
                    .clone(),
            );
        } else {
            fields.extend(
                json!({"active": true, "from": from, "to": to, "start_tick": world.tick,
                    "reverse": false, "glide": glide, "from_camera": camera,
                    "interrupted": interrupted})
                .as_object()
                .unwrap()
                .clone(),
            );
        }
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
    fields.extend(
        json!({"active": true, "from": from, "to": to, "start_tick": world.tick,
        "reverse": reverse, "glide": glide, "interrupted": interrupted})
        .as_object()
        .unwrap()
        .clone(),
    );
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

/// The `interrupted` text for a new trip: the player's trip as it was, if it is a glide
/// that has not landed, ending on node `camera`. Empty otherwise. A glide that already
/// reached its end still counts: the camera shows its end node.
fn interrupted(trip: &galaxy_api::Fields, camera: Option<&str>) -> String {
    let gliding = trip["active"] == true && glide_seconds(trip) > 0.0;
    match camera.filter(|_| gliding) {
        Some(camera) => Glide::of(trip, camera).to_text(),
        None => String::new(),
    }
}

/// Weights at or below this are dropped from a blend; what they moved is far below a
/// pixel.
const NEGLIGIBLE: f64 = 1e-9;

/// One camera in a [`Blend`]: node `node` of room `place`, or with no node the camera
/// the player left `place` with on a trip between places.
#[derive(Clone, Debug, PartialEq)]
pub struct Part {
    pub place: String,
    pub node: Option<String>,
    pub weight: f64,
}

/// A camera pose as weights over the cameras a chain of glides passed through. However
/// many glides take each other over, it holds at most one part per camera node, plus
/// the camera the chain first left, so it stays small. The view blends the cameras'
/// framings with these weights; the sim only does the arithmetic, which is exact on
/// every OS.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Blend {
    /// Weights for the direction looked in, up, shown height and field of view.
    pub pose: Vec<Part>,
    /// Weights for the point looked at. Only nodes of one room count: across places a
    /// glide looks at its destination's point throughout, so the rest of the weight goes
    /// there.
    pub focus: Vec<Part>,
    /// The source pose (`from_*` fields) of the trip between places that a part with no
    /// node left from.
    pub left: galaxy_api::Fields,
}

impl Blend {
    /// The camera a trip's glide starts from: where the glide it took over was at its
    /// `start_tick`, else the room's previous node, else the camera of the place left.
    pub fn source(trip: &galaxy_api::Fields) -> Blend {
        if let Some(taken) = trip
            .get("interrupted")
            .and_then(|t| t.as_str())
            .and_then(Glide::parse)
        {
            return taken.at(trip["start_tick"].as_f64().unwrap_or(0.0));
        }
        let text = |k: &str| trip[k].as_str().unwrap_or("").to_owned();
        let (from, to) = (text("from"), text("to"));
        if from == to {
            let node = Part {
                place: to,
                node: Some(text("from_camera")),
                weight: 1.0,
            };
            return Blend {
                pose: vec![node.clone()],
                focus: vec![node],
                left: Default::default(),
            };
        }
        Blend {
            pose: vec![Part {
                place: from,
                node: None,
                weight: 1.0,
            }],
            focus: Vec::new(),
            left: trip
                .iter()
                .filter(|(k, _)| k.starts_with("from_"))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        }
    }

    /// This pose `t` (eased, 0 to 1) of the way to node `node` of room `place`.
    pub fn toward(&self, place: &str, node: &str, t: f64) -> Blend {
        let end = |weight| Part {
            place: place.into(),
            node: Some(node.into()),
            weight,
        };
        let mut pose = scaled(self.pose.iter(), 1.0 - t);
        add(&mut pose, end(t));
        // Points looked at in another place's coordinates can't blend with this one's.
        let mut focus = scaled(self.focus.iter().filter(|p| p.place == place), 1.0 - t);
        let kept: f64 = focus.iter().map(|p| p.weight).sum();
        add(&mut focus, end(1.0 - kept));
        let pose = pruned(pose);
        let left = if pose.iter().any(|p| p.node.is_none()) {
            self.left.clone()
        } else {
            Default::default()
        };
        Blend {
            pose,
            focus: pruned(focus),
            left,
        }
    }
}

fn scaled<'a>(parts: impl Iterator<Item = &'a Part>, by: f64) -> Vec<Part> {
    parts
        .map(|p| Part {
            weight: p.weight * by,
            ..p.clone()
        })
        .collect()
}

fn add(parts: &mut Vec<Part>, part: Part) {
    match parts
        .iter_mut()
        .find(|p| p.place == part.place && p.node == part.node)
    {
        Some(p) => p.weight += part.weight,
        None => parts.push(part),
    }
}

/// `parts` without negligible weights, scaled back to a total of one.
fn pruned(parts: Vec<Part>) -> Vec<Part> {
    let kept: Vec<_> = parts
        .into_iter()
        .filter(|p| p.weight > NEGLIGIBLE)
        .collect();
    let total: f64 = kept.iter().map(|p| p.weight).sum();
    kept.into_iter()
        .map(|p| Part {
            weight: p.weight / total,
            ..p
        })
        .collect()
}

/// A camera glide: from `from` to node `camera` of room `to`, over `glide` seconds from
/// `start_tick`. `engine.travel.interrupted` stores the glide a trip took over this way,
/// as JSON text.
#[derive(Clone, Debug, PartialEq)]
pub struct Glide {
    pub to: String,
    pub camera: String,
    pub start_tick: f64,
    pub glide: f64,
    pub from: Blend,
}

impl Glide {
    /// The glide of trip fields `trip`, ending on node `camera` of its `to`.
    pub fn of(trip: &galaxy_api::Fields, camera: &str) -> Glide {
        Glide {
            to: trip["to"].as_str().unwrap_or("").into(),
            camera: camera.into(),
            start_tick: trip["start_tick"].as_f64().unwrap_or(0.0),
            glide: glide_seconds(trip),
            from: Blend::source(trip),
        }
    }

    /// How far the glide is at `now` (tick plus alpha), eased (smoothstep) from 0 to 1.
    pub fn progress(&self, now: f64) -> f64 {
        if self.glide <= 0.0 {
            return 1.0;
        }
        let ticks = self.glide * TICKS_PER_SECOND as f64;
        let t = ((now - self.start_tick) / ticks).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    }

    /// Where the camera is at `now` (tick plus alpha).
    pub fn at(&self, now: f64) -> Blend {
        self.from.toward(&self.to, &self.camera, self.progress(now))
    }

    fn to_text(&self) -> String {
        let parts = |parts: &[Part]| {
            parts
                .iter()
                .map(|p| json!([p.place, p.node, p.weight]))
                .collect::<Vec<_>>()
        };
        json!({"to": self.to, "camera": self.camera, "start_tick": self.start_tick,
            "glide": self.glide, "pose": parts(&self.from.pose),
            "focus": parts(&self.from.focus), "left": self.from.left})
        .to_string()
    }

    /// The glide stored in `engine.travel.interrupted` text; None for empty or bad text.
    pub fn parse(text: &str) -> Option<Glide> {
        let v: serde_json::Value = serde_json::from_str(text).ok()?;
        let parts = |key: &str| -> Option<Vec<Part>> {
            v[key]
                .as_array()?
                .iter()
                .map(|p| {
                    Some(Part {
                        place: p[0].as_str()?.into(),
                        node: p[1].as_str().map(Into::into),
                        weight: p[2].as_f64().filter(|w| w.is_finite())?,
                    })
                })
                .collect()
        };
        Some(Glide {
            to: v["to"].as_str()?.into(),
            camera: v["camera"].as_str()?.into(),
            start_tick: v["start_tick"].as_f64()?,
            glide: v["glide"].as_f64()?.clamp(0.0, MAX_GLIDE),
            from: Blend {
                pose: parts("pose")?,
                focus: parts("focus")?,
                left: v["left"].as_object().cloned().unwrap_or_default(),
            },
        })
    }
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
