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
//! - `engine.spin { rate }`: a slow steady turn, degrees per second (positive is
//!   counter-clockwise). The spin service turns `angle` every tick, so a body's outline
//!   turns with it. Things with `engine.flight` are turned by flight instead.
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
//! Effects, drawn only (`engine/docs/effects.md`). Nothing in the world reads them:
//!
//! - `engine.beam { target, look, x, y, height }`: a beam from this thing (offset by `x`,
//!   `y`, `height` in its own frame) to the drawn surface of `target`, while `target`
//!   names one; `look` is a `[beams.<name>]` from a style.
//! - `engine.emitter { recipe, target, x, y, height, angle, rate, by }`: sprays a
//!   `[particles.<name>]` recipe from this thing (or `target`), `rate` times the recipe's
//!   rate, scaled by `by` (`thrust` or `speed`; empty is always).
//! - `engine.burst { recipe, tick, target, x, y, height, angle }`: one burst of a recipe's
//!   `count`, born on `tick` (0 is none).
//! - `engine.camera_feel { speed_zoom, lead, shake, shake_tick, shake_seconds }`, beside
//!   `engine.camera`: `speed_zoom` widens the view by that share at the thing's top speed,
//!   `lead` looks that many seconds ahead of it, and `shake` (a share of the view) shakes
//!   it for `shake_seconds` from `shake_tick`. A separate component, so worlds without it
//!   keep their hash.
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
//!   face the target, and keep on its surface as it shrinks or moves), `dock` (stop on the target's point, facing its way) or `follow`
//!   (pass each `engine.waypoint` in turn from `target`, stopping at the last); empty is
//!   no order. The engine sets `done` and records `engine.arrived` when it gets there.
//! - `engine.waypoint { next }`: a point on a route; `next` is the following one.
//! - `engine.body { radius, mass, fixed, drag }`: makes a thing solid. Its outline is a
//!   circle of `radius`, or with radius 0 its model's footprint. Overlaps push apart by
//!   `mass`; a `fixed` body, or one with no `engine.motion`, never moves. Things that
//!   don't fly lose `drag` of their speed per second after a push.
//! - `engine.explorer { radius }` and `engine.explored { cell, cells, place }`: explored
//!   memory. Each tick the explore service marks the cells within `radius` of an explorer into its
//!   `engine.explored`, in the place it is in (`place`) ([`crate::explored`],
//!   `engine/docs/map-and-fog.md`).
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
pub const SPIN: &str = "engine.spin";
pub const SURFACE: &str = "engine.surface";
pub const MODEL: &str = "engine.model";
pub const CAMERA: &str = "engine.camera";
pub const FLIGHT: &str = "engine.flight";
pub const HELM: &str = "engine.helm";
pub const STEER: &str = "engine.steer";
pub const WAYPOINT: &str = "engine.waypoint";
pub const ANCHOR: &str = "engine.anchor";

pub const TALK: &str = "engine.talk";
pub const TALK_LINE: &str = "engine.talk_line";
pub const TALK_CHOICE: &str = "engine.talk_choice";
pub const FOCUS: &str = "engine.focus";
pub const TALK_START: &str = "engine.talk_start";
pub const TALK_NEXT: &str = "engine.talk_next";
pub const TALK_CHOSEN: &str = "engine.talk_chosen";
pub const BODY: &str = "engine.body";
pub const PLACE: &str = "engine.place";
pub const SPOT: &str = "engine.spot";
pub const DOOR: &str = "engine.door";
pub const LOCATION: &str = "engine.location";
pub const BEING: &str = "engine.being";
pub const WALK: &str = "engine.walk";
pub const ROOM: &str = "engine.room";
pub const TRAVEL: &str = "engine.travel";
pub const PLACE_ARRIVED: &str = "engine.place_arrived";
pub const CAMERA_SETTLED: &str = "engine.camera_settled";
pub const CLOCK: &str = "engine.clock";
pub const LIFE: &str = "engine.life";
pub const ACT: &str = "engine.act";
pub const CLOCK_ADVANCED: &str = "engine.clock_advanced";
pub const WALK_FINISHED: &str = "engine.walk_finished";
pub const BEAM: &str = "engine.beam";
pub const EMITTER: &str = "engine.emitter";
pub const BURST: &str = "engine.burst";
pub const CAMERA_FEEL: &str = "engine.camera_feel";
pub const EXPLORER: &str = "engine.explorer";
pub const EXPLORED: &str = "engine.explored";

/// The engine's event kinds (in the `engine` namespace).
/// - `start`: once, on the first tick of a new world.
/// - `hot_loaded`: a module was hot-loaded (`module`, `version`).
/// - `edited`: the world was edited from outside its rules (`galaxy_sim::edit`), between
///   two ticks: `source` (`scenario`, `inputs`, `live`), `entities` and `components`, the
///   full IDs each edit named, in order, each once.
/// - `resumed`: a save was resumed (`galaxy play --load`/`--continue`, `sim load`),
///   recorded as input with source `resume` on the first tick after the save; no payload.
/// - `script_failed`: a handler call was aborted and its writes discarded (`module`,
///   `script`, `handler`, `code`, `error`).
/// - `arrived`: a steering order got where it was going (`entity`, `mode`, `target`).
/// - `walk_finished`: a recorded walk completed (`entity`, `place`, `spot`).
/// - `place_arrived`: play landed a trip, recorded as input (`entity`, `place`, `start_tick`).
/// - `camera_settled`: play landed a glide to another camera in the same room, recorded
///   as input (`entity`, `place`, `start_tick`).
/// - `talk_start`, `talk_next`: a speaker or bubble click, recorded as input (`talk`, and
///   `line` for next).
/// - `talk_chosen`: an answer button in a bubble, recorded as input (`talk`, `line`,
///   `choice`). A stale answer (not on the line showing) records no event.
/// - `contact`: two bodies started touching at least as fast as a module's manifest
///   `contact_speed` (`entity`, the one that moves more; `other`; `speed`, the closing
///   speed in units a second). Recorded only when a loaded module sets `contact_speed`.
pub const EVENTS: [&str; 14] = [
    "clock_advanced",
    "start",
    "hot_loaded",
    "edited",
    "resumed",
    "script_failed",
    "arrived",
    "walk_finished",
    "place_arrived",
    "camera_settled",
    "talk_start",
    "talk_next",
    "talk_chosen",
    "contact",
];

pub const START: &str = "engine.start";
pub const HOT_LOADED: &str = "engine.hot_loaded";
pub const EDITED: &str = "engine.edited";
pub const RESUMED: &str = "engine.resumed";
/// The input source of [`RESUMED`].
pub const RESUME_SOURCE: &str = "resume";
/// The input source of a module's debug-only events (`debug` in its manifest), the only
/// source that may send them.
pub const DEBUG_SOURCE: &str = "debug";
pub const ARRIVED: &str = "engine.arrived";
pub const CONTACT: &str = "engine.contact";
pub const SCRIPT_FAILED: &str = "engine.script_failed";

/// What each field of the engine's components means, with its unit: component (local
/// name), field, meaning. `galaxy-gm fields components` prints it beside each field's type
/// and default from [`components`]; a test keeps the two in step.
pub const FIELDS: &[(&str, &str, &str)] = &[
    ("life", "clock", "clock driving this being's repeating day"),
    (
        "life",
        "first",
        "first act in a finite chain; the end loops to first",
    ),
    (
        "life",
        "name",
        "name on the being's sign; empty hides the sign",
    ),
    ("life", "speed", "walking speed in world units per second"),
    (
        "life",
        "presentation",
        "states animates visible changes; instant finishes them",
    ),
    ("life", "current", "selected act; engine-owned"),
    ("life", "phase", "rise, walk, settle or empty; engine-owned"),
    (
        "life",
        "start_tick",
        "start of the current pose phase; engine-owned",
    ),
    ("life", "from_morph", "saved source morph; engine-owned"),
    ("life", "from_angle", "saved source facing; engine-owned"),
    (
        "life",
        "walk_spot",
        "visible segment endpoint; engine-owned",
    ),
    (
        "life",
        "departed_tick",
        "trip tick whose source fade keeps this pose; engine-owned",
    ),
    (
        "life",
        "departed_place",
        "source place of a finished unseen walk; engine-owned",
    ),
    ("life", "departed_x", "source fade x; engine-owned"),
    ("life", "departed_y", "source fade y; engine-owned"),
    (
        "life",
        "departed_height",
        "source fade floor height; engine-owned",
    ),
    ("life", "departed_angle", "source fade facing; engine-owned"),
    ("life", "departed_morph", "source fade morph; engine-owned"),
    (
        "act",
        "next",
        "next act; empty loops to the day's first act",
    ),
    ("act", "duration", "positive whole clock units"),
    (
        "act",
        "place",
        "destination place (without following enter)",
    ),
    ("act", "spot", "destination spot in that place"),
    ("act", "state", "module-defined activity shown on the sign"),
    (
        "act",
        "morph",
        "resting pose in the module's model morph target, 0 to 1",
    ),
    ("act", "angle", "resting facing in radians"),
    (
        "talk",
        "speaker",
        "entity speaking; anything with an anchor can speak",
    ),
    (
        "talk",
        "anchor",
        "published anchor; empty uses being.<speaker>",
    ),
    ("talk", "first", "first engine.talk_line entity"),
    ("talk", "current", "current line; engine-owned progress"),
    ("talk", "active", "conversation currently showing"),
    ("talk", "enabled", "available for interaction"),
    (
        "talk",
        "story",
        "unseen story takes priority and stays pending until completed",
    ),
    ("talk", "seen", "last line acknowledged; engine-owned"),
    ("talk", "width", "bubble width in logical pixels"),
    (
        "talk",
        "title",
        "bubble heading; empty uses the speaker's life sign name",
    ),
    (
        "talk",
        "subtitle",
        "quieter line under the heading, such as a place; empty shows none",
    ),
    (
        "talk",
        "x",
        "horizontal offset from anchor in logical pixels",
    ),
    (
        "talk",
        "y",
        "vertical offset above anchor in logical pixels",
    ),
    ("talk_line", "text", "line displayed in the bubble"),
    (
        "talk_line",
        "next",
        "next line entity; empty ends the conversation; must be empty on a line with choices",
    ),
    (
        "talk_choice",
        "line",
        "engine.talk_line that offers this answer as a button",
    ),
    ("talk_choice", "text", "button label"),
    (
        "talk_choice",
        "next",
        "line shown after this answer; empty ends the conversation",
    ),
    (
        "talk_choice",
        "order",
        "button order on its line, lowest first; ties by entity ID",
    ),
    (
        "focus",
        "part",
        "model mesh part to keep visible while speaking; empty opts out; beings default to face",
    ),
    ("room", "title", "place title on the travel card"),
    (
        "room",
        "camera",
        "named glTF camera node; empty uses camera",
    ),
    (
        "room",
        "fov",
        "posed camera vertical field of view in degrees",
    ),
    (
        "room",
        "glide",
        "seconds (0 to 10) the camera glides when arriving here or when camera changes; 0 is the fade cut",
    ),
    (
        "travel",
        "active",
        "trip awaiting its recorded place_arrived input",
    ),
    ("travel", "from", "source place"),
    ("travel", "to", "destination place"),
    ("travel", "start_tick", "tick the trip started"),
    (
        "travel",
        "from_spot",
        "source spot occupied when the trip started",
    ),
    (
        "travel",
        "from_x",
        "source x before changing coordinate spaces",
    ),
    (
        "travel",
        "from_y",
        "source y before changing coordinate spaces",
    ),
    ("travel", "from_height", "source floor height"),
    ("travel", "from_angle", "source facing in radians"),
    ("travel", "from_layer", "source floor layer"),
    (
        "travel",
        "reverse",
        "leaving for an ancestor plays the cut backward",
    ),
    (
        "travel",
        "glide",
        "seconds this trip glides the camera; 0 plays the fade cut",
    ),
    (
        "travel",
        "from_camera",
        "source camera node of a glide within one room; empty for a trip between places",
    ),
    (
        "travel",
        "interrupted",
        "the glide this trip took over while it still moved, as JSON text: its end (`to`, `camera`), `start_tick`, `glide` and the camera it started from as weights over camera nodes; empty otherwise",
    ),
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
        "spin",
        "rate",
        "a steady turn, degrees a second, positive counter-clockwise; ignored with engine.flight",
    ),
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
    (
        "camera_feel",
        "speed_zoom",
        "0 to 1: the view widens by this share at the followed thing's top speed; drawn only",
    ),
    (
        "camera_feel",
        "lead",
        "0 to 2: seconds of its travel the camera looks ahead; eases back as it stops",
    ),
    (
        "camera_feel",
        "shake",
        "0 to 0.2: a kick and shake, as a share of the view, from shake_tick",
    ),
    (
        "camera_feel",
        "shake_tick",
        "the tick the shake starts (set it to ctx.tick on an event); 0 is none",
    ),
    (
        "camera_feel",
        "shake_seconds",
        "0 to 2: how long the shake takes to die away",
    ),
    (
        "beam",
        "target",
        "the thing it lands on; empty shows no beam",
    ),
    (
        "beam",
        "look",
        "a [beams.<name>] from a style.toml (`<module>.<name>`), or empty for the default",
    ),
    (
        "beam",
        "x",
        "where it starts, ahead in this thing's frame, world units",
    ),
    ("beam", "y", "where it starts, to the left, world units"),
    ("beam", "height", "where it starts, up, world units"),
    (
        "emitter",
        "recipe",
        "a [particles.<name>] from a style.toml (`<module>.<name>`); empty sprays nothing",
    ),
    (
        "emitter",
        "target",
        "the thing it sprays from; empty is this thing",
    ),
    (
        "emitter",
        "x",
        "offset ahead in that thing's frame, world units",
    ),
    ("emitter", "y", "offset to the left, world units"),
    ("emitter", "height", "offset up, world units"),
    (
        "emitter",
        "angle",
        "turns the spray from that thing's facing, degrees",
    ),
    ("emitter", "rate", "times the recipe's rate (0 stops it)"),
    (
        "emitter",
        "by",
        "empty sprays always; thrust while it pushes ahead; speed with its share of top speed",
    ),
    (
        "burst",
        "recipe",
        "a [particles.<name>] from a style.toml; the burst is its count",
    ),
    (
        "burst",
        "tick",
        "the tick it bursts (set it to ctx.tick); 0 is none",
    ),
    (
        "burst",
        "target",
        "the thing it bursts from; empty is this thing",
    ),
    (
        "burst",
        "x",
        "offset ahead in that thing's frame, world units",
    ),
    ("burst", "y", "offset to the left, world units"),
    ("burst", "height", "offset up, world units"),
    (
        "burst",
        "angle",
        "turns the spray from that thing's facing, degrees",
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
        "arrive, hold (keeps on the target's surface once there), dock or follow; empty is no order",
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
    (
        "explorer",
        "radius",
        "world units around it the explore service marks explored each tick",
    ),
    (
        "explored",
        "cell",
        "grid cell size of this memory, world units; changing it reinterprets cells",
    ),
    (
        "explored",
        "cells",
        "explored cells as rows of runs `y:x+count,...;...`; the explore service adds to it",
    ),
    (
        "explored",
        "place",
        "the place these cells are in, empty for the open plane; an empty memory moves to its explorer's place",
    ),
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
        ("act", {
            let mut f = fields(&[
                ("next", Id),
                ("duration", Int),
                ("place", Id),
                ("spot", Id),
                ("state", Text),
            ]);
            f.get_mut("duration").unwrap().default = 1.into();
            f.extend(numbers(&[("morph", 0.0), ("angle", 0.0)]));
            f
        }),
        ("life", {
            let mut f = fields(&[
                ("clock", Id),
                ("first", Id),
                ("name", Text),
                ("presentation", Text),
                ("current", Id),
                ("phase", Text),
                ("start_tick", Int),
                ("walk_spot", Id),
                ("departed_tick", Int),
                ("departed_place", Id),
            ]);
            f.get_mut("presentation").unwrap().default = "states".into();
            f.extend(numbers(&[
                ("speed", 1.5),
                ("from_morph", 0.0),
                ("from_angle", 0.0),
            ]));
            f.extend(numbers(&[
                ("departed_x", 0.0),
                ("departed_y", 0.0),
                ("departed_height", 0.0),
                ("departed_angle", 0.0),
                ("departed_morph", 0.0),
            ]));
            f
        }),
        ("focus", {
            let mut f = fields(&[("part", Text)]);
            f.get_mut("part").unwrap().default = "face".into();
            f
        }),
        ("talk_line", fields(&[("text", Text), ("next", Id)])),
        (
            "talk_choice",
            fields(&[("line", Id), ("text", Text), ("next", Id), ("order", Int)]),
        ),
        ("talk", {
            let mut f = fields(&[
                ("speaker", Id),
                ("anchor", Text),
                ("first", Id),
                ("current", Id),
                ("active", Bool),
                ("enabled", Bool),
                ("story", Bool),
                ("seen", Bool),
                ("title", Text),
                ("subtitle", Text),
            ]);
            f.get_mut("enabled").unwrap().default = true.into();
            f.extend(numbers(&[("width", 320.0), ("x", 0.0), ("y", -12.0)]));
            f
        }),
        ("room", {
            let mut room = fields(&[("title", Text), ("camera", Text)]);
            room.extend(numbers(&[("fov", 40.0), ("glide", 0.0)]));
            room
        }),
        (
            "travel",
            fields(&[
                ("active", Bool),
                ("from", Id),
                ("to", Id),
                ("start_tick", Int),
                ("reverse", Bool),
                ("from_spot", Id),
                ("from_x", Number),
                ("from_y", Number),
                ("from_height", Number),
                ("from_angle", Number),
                ("from_layer", Int),
                ("glide", Number),
                ("from_camera", Text),
                ("interrupted", Text),
            ]),
        ),
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
        ("spin", numbers(&[("rate", 0.0)])),
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
        ("explorer", numbers(&[("radius", 40.0)])),
        ("explored", {
            let mut f = fields(&[("cells", Text), ("place", Id)]);
            f.extend(numbers(&[("cell", crate::explored::DEFAULT_CELL)]));
            f
        }),
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
        ("camera_feel", {
            let mut f = numbers(&[
                ("speed_zoom", 0.0),
                ("lead", 0.0),
                ("shake", 0.0),
                ("shake_seconds", 0.4),
            ]);
            f.extend(fields(&[("shake_tick", Int)]));
            f
        }),
        ("beam", {
            let mut f = fields(&[("target", Id), ("look", Text)]);
            f.extend(numbers(&[("x", 0.0), ("y", 0.0), ("height", 0.0)]));
            f
        }),
        ("emitter", {
            let mut f = fields(&[("recipe", Text), ("target", Id), ("by", Text)]);
            f.extend(numbers(&[
                ("x", 0.0),
                ("y", 0.0),
                ("height", 0.0),
                ("angle", 0.0),
                ("rate", 1.0),
            ]));
            f
        }),
        ("burst", {
            let mut f = fields(&[("recipe", Text), ("tick", Int), ("target", Id)]);
            f.extend(numbers(&[
                ("x", 0.0),
                ("y", 0.0),
                ("height", 0.0),
                ("angle", 0.0),
            ]));
            f
        }),
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
