//! Cutaway: the room is inside the module, part of the same object. The camera comes
//! close over the module, its shell folds down into its floor like a stage flat, and
//! there is the room standing inside it, its people in it; the camera then settles down
//! into the room's own view. Coming out plays it backward: the camera rises out of the
//! room, the shell closes over it and the camera pulls back to the station.

use render::glam::{Mat4, Vec3};
use render::{Camera, Color};

use super::Travel;
use super::shared::{self, End, blend, framing, smooth, span};
use crate::stage::{self, Shown, Stage};

/// The whole move, either way (seconds).
const TIME: f32 = 1.5;
/// Parts of the move, as shares of it: closing in over the module, the shell opening,
/// settling into the room.
const CLOSE: f32 = 0.32;
const OPEN: (f32, f32) = (0.18, 0.55);

#[derive(Default)]
pub struct Cutaway {
    from: String,
    to: String,
    t: f32,
    busy: bool,
}

/// The station with `room` inside its module, `p` of the way in: 0 is the station at
/// rest, 1 the room's own view.
fn view(stage: &Stage, room: &str, p: f32) -> Shown {
    let End::Room {
        module: Some(module),
        ..
    } = shared::end(stage, room)
    else {
        return stage.view(if p < 0.5 {
            &stage.model.station().id
        } else {
            room
        });
    };
    let at = shared::inside(stage, room, module);
    let rest = shared::station_camera(stage);
    let Some(room_cam) = shared::set_camera_at(stage, room, at) else {
        return stage.view(room);
    };
    let reach = at.x_axis.truncate().length() * {
        let f = stage.place(room).map(|p| p.floor).unwrap_or_default();
        f.x.max(f.y)
    };
    let over: Camera = framing(
        at.transform_point3(Vec3::new(0.0, 0.0, 1.0)),
        Vec3::new(0.0, -0.8, 1.0),
        reach * 1.25,
        30.0,
    );
    let cam = if p < CLOSE {
        blend(&rest, &over, smooth(p / CLOSE))
    } else {
        blend(&over, &room_cam, smooth((p - CLOSE) / (1.0 - CLOSE)))
    };
    let open = smooth(span(p, OPEN.0, OPEN.1));
    let settle = smooth(span(p, CLOSE, 1.0));
    let mut shown = stage.exterior(Some(&module.id), Some(cam));
    if let Some(i) = shared::module_index(stage, &module.id) {
        let shell = &mut shown.scene.meshes[i];
        // The shell folds down into its floor, from the module's own base, and sinks
        // under the room so its strut doesn't cross the room's front.
        let base = module.pos();
        shell.transform = Mat4::from_translation(base - Vec3::Z * 3.0 * open)
            * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0 - 0.94 * open))
            * Mat4::from_translation(-base)
            * shell.transform;
    }
    if open > 0.0 {
        // The room's own lamps light it from inside; the station's go out.
        shown.scene.lights.points.clear();
        stage.set(room, at, &mut shown);
        let mut lit = shown.scene.clone();
        stage::room_lights(&mut lit);
        let l = &mut shown.scene.lights;
        l.sun_direction = l
            .sun_direction
            .lerp(lit.lights.sun_direction, settle)
            .normalize_or(Vec3::Z);
        l.ambient = l.ambient.lerp(lit.lights.ambient, settle);
    }
    // The rest of the station dims as you settle inside, so the room's own view is
    // what you land in.
    let dim = smooth(span(p, 0.6, 1.0));
    let shell = shared::module_index(stage, &module.id);
    for (j, inst) in shown.scene.meshes.iter_mut().enumerate() {
        // The core is mesh 0, then the modules; the room's set comes after.
        if j <= stage.world.modules.len() && Some(j) != shell {
            inst.material.tint = inst.material.tint.lerp(shared::DARK, dim);
        }
    }
    if p > 0.85 {
        let clear = stage
            .world
            .sets
            .get(room)
            .map(|s| s.clear)
            .unwrap_or(Color::BLACK);
        shown.scene.backdrop = None;
        shown.scene.clear_color = clear;
    }
    shown
}

impl Travel for Cutaway {
    fn name(&self) -> &'static str {
        "cutaway"
    }

    fn start(&mut self, from: &str, to: &str) {
        self.from = from.to_string();
        self.to = to.to_string();
        self.t = 0.0;
        self.busy = true;
    }

    fn step(&mut self, dt: f32) {
        if self.busy {
            self.t += dt;
            if self.t >= TIME {
                self.busy = false;
            }
        }
    }

    fn busy(&self) -> bool {
        self.busy
    }

    fn finish(&mut self) {
        self.busy = false;
    }

    fn compose(&self, stage: &Stage) -> Shown {
        if !self.busy {
            return stage.view(&stage.model.here);
        }
        // The same move both ways: coming out is going in, backward.
        let p = self.t / TIME;
        match (shared::end(stage, &self.from), shared::end(stage, &self.to)) {
            (End::Station, End::Room { id, .. }) => view(stage, id, p),
            (End::Room { id, .. }, End::Station) => view(stage, id, 1.0 - p),
            (End::Room { id: a, .. }, End::Room { id: b, .. }) => {
                if p < 0.5 {
                    view(stage, a, 1.0 - p * 2.0)
                } else {
                    view(stage, b, p * 2.0 - 1.0)
                }
            }
            _ => stage.view(&stage.model.here),
        }
    }
}
