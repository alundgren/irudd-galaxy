//! What the engine itself owns in the world: the tick rate, the slow cadence, and the few
//! component types engine services act on.
//!
//! The world is 2.5D: a flat 2D simulation, plus a floor layer and the height under each
//! thing. There is no vertical physics.
//!
//! - `engine.position { x, y, layer, height, angle }`: where a thing is on the sim plane,
//!   which floor layer it stands on (0 the ground, 1 a deck above it, and so on), the
//!   height of the floor under it and which way it faces (radians, counter-clockwise, 0
//!   facing +x). `height` is the engine's to write: the ground service sets it.
//! - `engine.motion { vx, vy }`: velocity in world units per second. The motion service
//!   moves everything with a position and a motion every tick.
//! - `engine.surface { layer, min_x, min_y, max_x, max_y, height, rise_x, rise_y }`: a
//!   walkable rectangle on a layer. Its height at a point is `height + rise_x * (x -
//!   min_x) + rise_y * (y - min_y)`, so a flat deck has no rise and a ramp or hill side
//!   has some. Where surfaces on a layer overlap, the highest one is the floor.
//!
//! - `engine.model { model, look, scale, morph }`: what a thing looks like. `model` is a
//!   model's ID (`<module>.<name>` is `models/<name>.gltf` in that module), `look` a look
//!   from a module's `style.toml` (`<module>.<name>`, or empty for the default), `scale`
//!   its size, and `morph` how far it is drawn towards its morph target, 0 to 1.
//! - `engine.camera { tilt, view, min_view, max_view, fov }`: the camera follows the thing
//!   that has this. `tilt` leans it back from straight down (degrees), `view` is how many
//!   world units it shows from the bottom of the screen to the top, the wheel zooms
//!   between `min_view` and `max_view`, and `fov` above 0 makes it a perspective camera
//!   with that vertical field of view (degrees).
//!
//! Flight and collision (`engine/docs/flight-and-collision.md`):
//!
//! - `engine.flight { speed, accel, brake, turn, inertia, boost, stop_time, slowdown }`:
//!   how a thing flies. Top speed (units per second), thrust and braking (units per
//!   second squared), turn rate (degrees per second), inertia (0 stops at once, 1 coasts
//!   for ever), the boost factor, how long letting go of thrust takes to stop (seconds, 0
//!   for no auto-brake), and the share of speed an order loses flying away from the nose.
//! - `engine.helm { turn, thrust, brake, boost, stopping }`: the hands on the controls.
//!   `turn` -1 to 1 (positive turns left, counter-clockwise), `thrust` -1 to 1 along the
//!   nose. `stopping` is the engine's: auto-brake ticks left.
//! - `engine.steer { mode, target, x, y, range, done }`: an order the flight service flies
//!   instead of the helm. `mode` is `arrive` (stop within `range` of the target's
//!   outline, or of the point `x`, `y` when `target` is empty), `hold` (the same, then
//!   face the target), `dock` (stop on the target's point, facing its way) or `follow`
//!   (pass each `engine.waypoint` in turn from `target`, stopping at the last); empty is
//!   no order. The engine sets `done` and records `engine.arrived` when it gets there.
//! - `engine.waypoint { next }`: a point on a route; `next` is the following one.
//! - `engine.body { radius, mass, fixed, drag }`: makes a thing solid. Its outline is a
//!   circle of `radius`, or with radius 0 its model's footprint. Overlaps push apart by
//!   `mass`; a `fixed` body, or one with no `engine.motion`, never moves. Things that
//!   don't fly lose `drag` of their speed per second after a push.
//!
//! Places, locations, beings, spots, doors, walks and clocks are described in
//! `engine/docs/places-and-beings.md`. They use ordinary components and entity IDs.
//!
//! Scripts move things between layers (a ladder, a ramp's top) by patching `layer`, with
//! the `engine.position` capability.

use std::collections::BTreeMap;

use galaxy_module::manifest::{FieldDecl, FieldType};

/// Sim ticks per second of game time.
pub const TICKS_PER_SECOND: u32 = 30;

/// Seconds per tick.
pub const TICK_SECONDS: f64 = 1.0 / TICKS_PER_SECOND as f64;

/// Ticks between two runs of the slow cadence: scripts' `cadence` handlers run once per
/// second of game time.
pub const CADENCE_TICKS: u64 = TICKS_PER_SECOND as u64;

/// The engine's component types.
pub const POSITION: &str = "engine.position";
pub const MOTION: &str = "engine.motion";
pub const SURFACE: &str = "engine.surface";
pub const MODEL: &str = "engine.model";
pub const CAMERA: &str = "engine.camera";
pub const FLIGHT: &str = "engine.flight";
pub const HELM: &str = "engine.helm";
pub const STEER: &str = "engine.steer";
pub const WAYPOINT: &str = "engine.waypoint";
pub const ANCHOR: &str = "engine.anchor";

pub const BODY: &str = "engine.body";
pub const PLACE: &str = "engine.place";
pub const SPOT: &str = "engine.spot";
pub const DOOR: &str = "engine.door";
pub const LOCATION: &str = "engine.location";
pub const BEING: &str = "engine.being";
pub const WALK: &str = "engine.walk";
pub const CLOCK: &str = "engine.clock";
pub const WALK_FINISHED: &str = "engine.walk_finished";

/// The engine's event kinds (in the `engine` namespace).
/// - `start`: once, on the first tick of a new world.
/// - `hot_loaded`: a module was hot-loaded (`module`, `version`).
/// - `script_failed`: a handler call was aborted and its writes discarded (`module`,
///   `script`, `handler`, `code`, `error`).
/// - `arrived`: a steering order got where it was going (`entity`, `mode`, `target`).
/// - `walk_finished`: a recorded walk completed (`entity`, `place`, `spot`).
pub const EVENTS: [&str; 5] = [
    "start",
    "hot_loaded",
    "script_failed",
    "arrived",
    "walk_finished",
];

pub const START: &str = "engine.start";
pub const HOT_LOADED: &str = "engine.hot_loaded";
pub const ARRIVED: &str = "engine.arrived";
pub const SCRIPT_FAILED: &str = "engine.script_failed";

/// What each field of the engine's components means, with its unit: component (local
/// name), field, meaning. `galaxy-gm fields components` prints it beside each field's type
/// and default from [`components`]; a test keeps the two in step.
pub const FIELDS: &[(&str, &str, &str)] = &[
    (
        "anchor",
        "name",
        "published name; widgets address publisher ID and this name",
    ),
    (
        "anchor",
        "target",
        "entity followed; empty means the publisher itself",
    ),
    ("anchor", "x", "local x offset in world units"),
    ("anchor", "y", "local y offset in world units"),
    ("anchor", "height", "height offset in world units"),
    ("place", "parent", "parent place; empty for a root"),
    (
        "place",
        "enter",
        "place entered by default; empty to enter this place",
    ),
    ("place", "arrival", "default arrival spot in this place"),
    (
        "place",
        "width",
        "floor width in world units; 0 for unspecified",
    ),
    (
        "place",
        "depth",
        "floor depth in world units; 0 for unspecified",
    ),
    ("spot", "place", "the place containing this named spot"),
    ("spot", "name", "spot name, unique within its place"),
    ("spot", "x", "local floor x in world units"),
    ("spot", "y", "local floor y in world units"),
    ("spot", "layer", "floor layer at this spot"),
    ("door", "from", "source place of this directed connection"),
    ("door", "to", "destination place"),
    ("door", "spot", "optional door spot in the source place"),
    (
        "door",
        "arrival",
        "optional arrival spot in the destination place",
    ),
    (
        "location",
        "place",
        "containing place; empty for the original shared plane",
    ),
    (
        "location",
        "spot",
        "named spot occupied; empty between spots",
    ),
    ("being", "state", "module-defined state"),
    ("being", "home", "home place; empty if none"),
    (
        "being",
        "player",
        "marks the player being; at most one in the world",
    ),
    ("walk", "active", "whether the walk is in progress"),
    ("walk", "place", "place in which the walk runs"),
    (
        "walk",
        "from",
        "source spot, or empty when starting between spots",
    ),
    ("walk", "to", "destination spot"),
    ("walk", "from_x", "saved starting x"),
    ("walk", "from_y", "saved starting y"),
    ("walk", "to_x", "saved destination x"),
    ("walk", "to_y", "saved destination y"),
    ("walk", "layer", "floor layer of the walk"),
    ("walk", "start_tick", "sim tick when the walk started"),
    ("walk", "speed", "world units per second"),
    (
        "walk",
        "state",
        "being state on completion; set by walk from the current state if omitted",
    ),
    (
        "clock",
        "value",
        "recorded clock in module-defined units; advanced only by scripts",
    ),
    ("position", "x", "world units"),
    ("position", "y", "world units"),
    (
        "position",
        "layer",
        "floor layer it stands on: 0 the ground, 1 a deck above, ...",
    ),
    (
        "position",
        "height",
        "height of the floor under it, world units; the engine sets it",
    ),
    (
        "position",
        "angle",
        "which way it faces, radians, counter-clockwise, 0 facing +x",
    ),
    ("motion", "vx", "velocity, world units a second"),
    ("motion", "vy", "velocity, world units a second"),
    (
        "surface",
        "layer",
        "the floor layer this walkable rectangle is on",
    ),
    ("surface", "min_x", "world units"),
    ("surface", "min_y", "world units"),
    ("surface", "max_x", "world units"),
    ("surface", "max_y", "world units"),
    ("surface", "height", "height at (min_x, min_y), world units"),
    ("surface", "rise_x", "height gained a world unit along x"),
    ("surface", "rise_y", "height gained a world unit along y"),
    (
        "model",
        "model",
        "a model ID: `<module>.<name>` is models/<name>.gltf in that module",
    ),
    (
        "model",
        "look",
        "a look from a style.toml (`<module>.<name>`), or empty for the default",
    ),
    ("model", "scale", "its size, times the model's"),
    (
        "model",
        "morph",
        "how far it is drawn towards its morph target, 0 to 1",
    ),
    (
        "camera",
        "tilt",
        "leans the camera back from straight down, degrees",
    ),
    (
        "camera",
        "view",
        "world units shown from the bottom of the screen to the top",
    ),
    (
        "camera",
        "min_view",
        "the wheel zooms between min_view and max_view",
    ),
    (
        "camera",
        "max_view",
        "the wheel zooms between min_view and max_view",
    ),
    (
        "camera",
        "fov",
        "above 0, a perspective camera with this vertical field of view, degrees",
    ),
    ("flight", "speed", "top speed, world units a second"),
    ("flight", "accel", "thrust, world units a second squared"),
    ("flight", "brake", "braking, world units a second squared"),
    ("flight", "turn", "turn rate, degrees a second"),
    ("flight", "inertia", "0 stops at once, 1 coasts for ever"),
    (
        "flight",
        "boost",
        "top speed and thrust times this while boosting",
    ),
    (
        "flight",
        "stop_time",
        "seconds letting go of thrust takes to stop; 0 for no auto-brake",
    ),
    (
        "flight",
        "slowdown",
        "the share of speed an order loses flying away from the nose",
    ),
    (
        "helm",
        "turn",
        "-1 to 1; positive turns left (counter-clockwise)",
    ),
    ("helm", "thrust", "-1 to 1 along the nose"),
    ("helm", "brake", "braking"),
    ("helm", "boost", "boosting"),
    ("helm", "stopping", "the engine's: auto-brake ticks left"),
    (
        "steer",
        "mode",
        "arrive, hold, dock or follow; empty is no order",
    ),
    (
        "steer",
        "target",
        "the entity to fly to (or the first waypoint, for follow)",
    ),
    (
        "steer",
        "x",
        "a point to fly to when target is empty, world units",
    ),
    (
        "steer",
        "y",
        "a point to fly to when target is empty, world units",
    ),
    (
        "steer",
        "range",
        "arrive and hold stop this far from the target's outline, world units",
    ),
    (
        "steer",
        "done",
        "the engine sets it, and records engine.arrived, when it gets there",
    ),
    ("waypoint", "next", "the following waypoint on the route"),
    (
        "body",
        "radius",
        "its outline is a circle this big, world units; 0 uses the model's footprint",
    ),
    ("body", "mass", "overlaps push apart by mass"),
    ("body", "fixed", "never moves"),
    (
        "body",
        "drag",
        "share of speed lost a second after a push, for things that don't fly",
    ),
];

fn field(ty: FieldType) -> FieldDecl {
    FieldDecl {
        ty,
        default: ty.zero(),
    }
}

fn fields(list: &[(&str, FieldType)]) -> BTreeMap<String, FieldDecl> {
    list.iter()
        .map(|(n, t)| ((*n).to_owned(), field(*t)))
        .collect()
}

/// Number fields with these defaults.
fn numbers(list: &[(&str, f64)]) -> BTreeMap<String, FieldDecl> {
    list.iter()
        .map(|(n, d)| {
            let decl = FieldDecl {
                ty: FieldType::Number,
                default: serde_json::Value::from(*d),
            };
            ((*n).to_owned(), decl)
        })
        .collect()
}

/// The engine's component types, by local name.
pub fn components() -> Vec<(&'static str, BTreeMap<String, FieldDecl>)> {
    use FieldType::{Bool, Id, Int, Number, Text};
    let mut model = fields(&[("model", Text), ("look", Text), ("morph", Number)]);
    model.extend(numbers(&[("scale", 1.0)]));
    vec![
        (
            "anchor",
            fields(&[
                ("name", Text),
                ("target", Id),
                ("x", Number),
                ("y", Number),
                ("height", Number),
            ]),
        ),
        (
            "place",
            fields(&[
                ("parent", Id),
                ("enter", Id),
                ("arrival", Id),
                ("width", Number),
                ("depth", Number),
            ]),
        ),
        (
            "spot",
            fields(&[
                ("place", Id),
                ("name", Text),
                ("x", Number),
                ("y", Number),
                ("layer", Int),
            ]),
        ),
        (
            "door",
            fields(&[("from", Id), ("to", Id), ("spot", Id), ("arrival", Id)]),
        ),
        ("location", fields(&[("place", Id), ("spot", Id)])),
        (
            "being",
            fields(&[("state", Text), ("home", Id), ("player", Bool)]),
        ),
        (
            "walk",
            fields(&[
                ("active", Bool),
                ("place", Id),
                ("from", Id),
                ("to", Id),
                ("from_x", Number),
                ("from_y", Number),
                ("to_x", Number),
                ("to_y", Number),
                ("layer", Int),
                ("start_tick", Int),
                ("speed", Number),
                ("state", Text),
            ]),
        ),
        ("clock", fields(&[("value", Int)])),
        (
            "position",
            fields(&[
                ("x", Number),
                ("y", Number),
                ("layer", Int),
                ("height", Number),
                ("angle", Number),
            ]),
        ),
        ("motion", fields(&[("vx", Number), ("vy", Number)])),
        (
            "surface",
            fields(&[
                ("layer", Int),
                ("min_x", Number),
                ("min_y", Number),
                ("max_x", Number),
                ("max_y", Number),
                ("height", Number),
                ("rise_x", Number),
                ("rise_y", Number),
            ]),
        ),
        ("model", model),
        (
            "flight",
            numbers(&[
                ("speed", 14.0),
                ("accel", 21.0),
                ("brake", 16.0),
                ("turn", 180.0),
                ("inertia", 0.4),
                ("boost", 2.0),
                ("stop_time", 1.0),
                ("slowdown", 0.5),
            ]),
        ),
        (
            "helm",
            fields(&[
                ("turn", Number),
                ("thrust", Number),
                ("brake", Bool),
                ("boost", Bool),
                ("stopping", Int),
            ]),
        ),
        (
            "steer",
            fields(&[
                ("mode", Text),
                ("target", Id),
                ("x", Number),
                ("y", Number),
                ("range", Number),
                ("done", Bool),
            ]),
        ),
        ("waypoint", fields(&[("next", Id)])),
        ("body", {
            let mut body = fields(&[("radius", Number), ("fixed", Bool)]);
            body.extend(numbers(&[("mass", 1.0), ("drag", 1.5)]));
            body
        }),
        (
            "camera",
            numbers(&[
                ("tilt", 30.0),
                ("view", 60.0),
                ("min_view", 20.0),
                ("max_view", 240.0),
                ("fov", 0.0),
            ]),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_engine_field_is_described() {
        let all = components();
        for (component, fields) in &all {
            for field in fields.keys() {
                assert!(
                    FIELDS.iter().any(|f| f.0 == *component && f.1 == field),
                    "engine.{component}.{field} is not in FIELDS"
                );
            }
        }
        for (component, field, _) in FIELDS {
            assert!(
                all.iter()
                    .any(|(c, f)| c == component && f.contains_key(*field)),
                "FIELDS names engine.{component}.{field}, which doesn't exist"
            );
        }
    }
}
