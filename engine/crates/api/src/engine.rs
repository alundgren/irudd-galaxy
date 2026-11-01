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

/// The engine's event kinds (in the `engine` namespace).
/// - `start`: once, on the first tick of a new world.
/// - `hot_loaded`: a module was hot-loaded (`module`, `version`).
/// - `script_failed`: a handler call was aborted and its writes discarded (`module`,
///   `script`, `handler`, `code`, `error`).
pub const EVENTS: [&str; 3] = ["start", "hot_loaded", "script_failed"];

pub const START: &str = "engine.start";
pub const HOT_LOADED: &str = "engine.hot_loaded";
pub const SCRIPT_FAILED: &str = "engine.script_failed";

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
    use FieldType::{Int, Number, Text};
    let mut model = fields(&[("model", Text), ("look", Text), ("morph", Number)]);
    model.extend(numbers(&[("scale", 1.0)]));
    vec![
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
