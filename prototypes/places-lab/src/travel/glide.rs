//! Glide in and dissolve: the camera travels from the station toward the module while
//! the station falls away into the dark, then the room's set comes up into the light
//! layer by layer (the set, then its people) as the camera settles. Coming out plays the
//! same thing backward: the room's layers sink away, the station rises back round the
//! module as the camera pulls out. Scene changes as stagecraft.

use render::glam::Vec3;
use render::{Camera, UiLayer};

use super::Travel;
use super::shared::{
    self, DARK, End, blend, dim, ease_out, framing, lights_at, pulled_back, smooth, span,
};
use crate::stage::{Shown, Stage};
use crate::world::Module;

/// Going into the dark from where you were, and coming up in the new place (seconds).
const OUT: f32 = 0.55;
const IN: f32 = 0.7;

#[derive(Default)]
pub struct Glide {
    from: String,
    to: String,
    t: f32,
    busy: bool,
}

/// The station, `k` of the way gone: 0 at rest, 1 dark with the camera close on `module`.
/// The parts farthest from the module go first; the module itself last.
fn station_side(stage: &Stage, module: Option<&Module>, k: f32) -> Shown {
    let rest = shared::station_camera(stage);
    let Some(m) = module else {
        let mut shown = stage.exterior(None, Some(rest));
        lights_at(&mut shown, 1.0 - k);
        return shown;
    };
    let close = framing(
        m.pos() + Vec3::Z * 1.0,
        Vec3::new(0.0, -0.75, 1.0),
        11.0,
        40.0,
    );
    let mut shown = stage.exterior(None, Some(blend(&rest, &close, k)));
    let here = m.pos();
    let far = shown
        .scene
        .meshes
        .iter()
        .map(|i| i.transform.transform_point3(Vec3::ZERO).distance(here))
        .fold(1.0f32, f32::max);
    for inst in &mut shown.scene.meshes {
        let d = inst.transform.transform_point3(Vec3::ZERO).distance(here) / far;
        // Far parts start early; the module (d = 0) starts last.
        let start = 0.45 * (1.0 - d);
        let f = smooth(span(k, start, start + 0.55));
        // The far parts fall away; the module only dims, so the eye stays on it.
        dim(inst, f, Vec3::NEG_Z * 40.0 * d);
    }
    lights_at(&mut shown, 1.0 - smooth(span(k, 0.0, 0.8)));
    shown
}

/// A room, `k` of the way gone: 0 at rest; at 1 its layers have sunk into the dark and
/// the camera stands back. Its people go first, the set last.
fn room_side(stage: &Stage, id: &str, k: f32) -> Shown {
    let mut shown = stage.view(id);
    let rest: Camera = shown.scene.camera;
    let mut cam = pulled_back(&rest, 1.0 + 0.6 * k);
    cam.eye += Vec3::Z * 1.2 * k;
    shown.scene.camera = cam;
    let n = shown.scene.meshes.len().max(1) as f32;
    for (i, inst) in shown.scene.meshes.iter_mut().enumerate() {
        // The first drawn (the set) has the latest start: it goes last and comes first.
        let start = 0.45 * (1.0 - i as f32 / n);
        let f = smooth(span(k, start, start + 0.55));
        dim(inst, f, Vec3::NEG_Z * 3.0);
    }
    shown.scene.clear_color = shown.scene.clear_color.lerp(DARK, smooth(k));
    lights_at(&mut shown, 1.0 - smooth(span(k, 0.0, 0.8)));
    shown
}

/// One side of the move, `k` of the way into the dark. `toward` is the other end, so the
/// station knows which module to fly at.
fn side(stage: &Stage, id: &str, toward: &str, k: f32) -> Shown {
    match shared::end(stage, id) {
        End::Station => {
            let module = match shared::end(stage, toward) {
                End::Room { module, .. } => module,
                End::Station => None,
            };
            station_side(stage, module, k)
        }
        End::Room { id, .. } => room_side(stage, id, k),
    }
}

impl Travel for Glide {
    fn name(&self) -> &'static str {
        "glide"
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
            if self.t >= OUT + IN {
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
        if self.t < OUT {
            side(stage, &self.from, &self.to, smooth(self.t / OUT))
        } else {
            let b = (self.t - OUT) / IN;
            side(stage, &self.to, &self.from, 1.0 - ease_out(b))
        }
    }

    fn overlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if !self.busy {
            return;
        }
        let a = if self.t < OUT {
            smooth(span(self.t / OUT, 0.6, 1.0))
        } else {
            1.0 - smooth(span((self.t - OUT) / IN, 0.0, 0.3))
        };
        shared::veil(layer, stage.viewport, DARK, a);
    }
}
