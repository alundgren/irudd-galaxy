//! The honest menu: you never leave the station's view. Going into a room opens it as a
//! window over the station, which stays where you left it, dimmed, behind the room's
//! screen. The room is shown as the room variant shows it, pulled back into a framed
//! window on the left where its people stand. Coming out closes the window the same way.

use render::glam::{Mat3, Mat4, Vec2, Vec3};
use render::{Camera, Color, Panel, Projection, Rect, UiLayer, palette};

use super::Travel;
use super::shared::{self, End, flat, smooth};
use crate::sets;
use crate::stage::{Shown, Stage};

/// Opening or closing the window (seconds).
const TIME: f32 = 0.35;
/// How far the room's camera stands back once open, for a room up to `WIDE` metres
/// across (a wider room stands back less, never closer than its own view, so its people
/// and labels keep their room), and from how far it comes.
const BACK: f32 = 1.3;
const WIDE: f32 = 9.0;
const FAR: f32 = 7.0;
/// Where the room's centre sits across the screen.
const ACROSS: f32 = 0.3;
/// How far the station behind is dimmed.
const DIM: f32 = 0.8;
/// How far behind the room the station is placed (it is drawn to look as it does at
/// rest).
const DEPTH: f32 = 500.0;

#[derive(Default)]
pub struct Menu {
    from: String,
    to: String,
    t: f32,
    busy: bool,
}

/// A camera's right, up and forward.
fn basis(cam: &Camera) -> Mat3 {
    let f = (cam.target - cam.eye).normalize_or(Vec3::NEG_Z);
    let r = f.cross(cam.up).normalize_or(Vec3::X);
    let u = r.cross(f);
    Mat3::from_cols(r, u, f)
}

/// The room's camera stood back by `back` and moved so the room sits on the left, as
/// far as its people allow.
fn room_camera(stage: &Stage, room: &str, rest: &Camera, back: f32) -> Camera {
    let mut cam = shared::pulled_back(rest, back);
    if !stands_back(stage) {
        return cam;
    }
    let width = shared::view_height(&cam) * stage.aspect();
    let right = basis(&cam).x_axis;
    let most = (0.5 - ACROSS) * width;
    // Nobody in the room comes nearer the edge than where they stood at rest, or 120 px.
    let people = stage
        .model
        .people_in(room)
        .filter_map(|who| {
            let feet = stage.model.floor_pos(who).extend(0.0);
            let rest_x = rest.world_to_screen(feet, stage.viewport)?.x;
            Some(cam.world_to_screen(feet, stage.viewport)?.x - rest_x.min(120.0))
        })
        .fold(f32::MAX, f32::min);
    let shift = if people == f32::MAX {
        most
    } else {
        (people * cam.world_per_pixel(stage.viewport)).clamp(0.0, most)
    };
    cam.eye += right * shift;
    cam.target += right * shift;
    cam
}

/// Only the staged set is stood back and moved: it places people where the place model
/// says. Other room variants place them their own way, so they keep their own framing
/// and the dimmed station shows wherever they leave room.
fn stands_back(stage: &Stage) -> bool {
    stage.room.name() == "staged"
}

fn open_back(stage: &Stage, room: &str) -> f32 {
    if !stands_back(stage) {
        return 1.0;
    }
    let wide = stage.place(room).map(|p| p.floor.x).unwrap_or(WIDE);
    (BACK * WIDE / wide.max(WIDE)).max(1.0)
}

/// The room `room` open `k` of the way (0 shut, 1 open) over the station.
fn view(stage: &Stage, room: &str, k: f32) -> Shown {
    let mut shown = stage.view(room);
    let rest = shown.scene.camera;
    let back = FAR + (open_back(stage, room) - FAR) * smooth(k);
    let cam = room_camera(stage, room, &rest, back);
    shown.scene.camera = cam;
    // The station as you left it, drawn far behind the room so it looks as it does at
    // rest through the room's camera.
    let module = match shared::end(stage, room) {
        End::Room {
            module: Some(m), ..
        } => Some(m.id.as_str()),
        _ => None,
    };
    let station = stage.exterior(module, Some(shared::station_camera(stage)));
    let s_cam = station.scene.camera;
    let fov = match cam.projection {
        Projection::Perspective { fov_y, .. } => fov_y,
        Projection::Orthographic { .. } => 30f32.to_radians(),
    };
    let scale = 2.0 * DEPTH * (fov * 0.5).tan() / shared::view_height(&s_cam).max(1.0);
    let turn = basis(&cam) * basis(&s_cam).transpose();
    let ahead = cam.eye + basis(&cam).z_axis * DEPTH;
    let to_room = Mat4::from_translation(ahead)
        * Mat4::from_mat3(turn * Mat3::from_diagonal(Vec3::splat(scale)))
        * Mat4::from_translation(-s_cam.target);
    for mut inst in station.scene.meshes {
        inst.transform = to_room * inst.transform;
        shown.scene.meshes.push(inst);
    }
    for mut g in station.scene.glows {
        g.position = to_room.transform_point3(g.position);
        g.radius *= scale;
        shown.scene.glows.push(g);
    }
    shown.scene.backdrop = station.scene.backdrop;
    shown
}

/// The room's window on screen: its floor and walls through the camera.
fn window(stage: &Stage, room: &str, cam: &Camera) -> Option<Rect> {
    let floor = stage.place(room)?.floor;
    let (hw, hd) = (floor.x * 0.5 + 0.3, floor.y * 0.5 + 0.3);
    let (mut lo, mut hi) = (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN));
    for x in [-hw, hw] {
        for y in [-hd, hd] {
            for z in [0.0, sets::WALL_H + 0.2] {
                let p = cam.world_to_screen(Vec3::new(x, y, z), stage.viewport)?;
                lo = lo.min(p);
                hi = hi.max(p);
            }
        }
    }
    // Inside the screen, a margin from its edges.
    let m = 14.0;
    let lo = (lo - Vec2::splat(10.0)).max(Vec2::splat(m));
    let hi = (hi + Vec2::splat(10.0)).min(stage.viewport - Vec2::splat(m));
    Some(Rect::new(
        lo.x,
        lo.y,
        (hi.x - lo.x).max(0.0),
        (hi.y - lo.y).max(0.0),
    ))
}

/// The station dimmed round the room's window, and a frame round it.
fn dim(stage: &Stage, room: &str, k: f32, layer: &mut UiLayer) {
    let v = stage.viewport;
    let rest = stage.view(room).scene.camera;
    let back = FAR + (open_back(stage, room) - FAR) * smooth(k);
    let cam = room_camera(stage, room, &rest, back);
    let shade = Color::hex(0x120c1a).with_alpha(DIM * smooth(k));
    let Some(w) = window(stage, room, &cam) else {
        shared::veil(layer, v, shade, shade.a);
        return;
    };
    let (x0, y0, x1, y1) = (w.x, w.y, w.x + w.w, w.y + w.h);
    for r in [
        Rect::new(-4.0, -4.0, v.x + 8.0, y0 + 4.0),
        Rect::new(-4.0, y1, v.x + 8.0, v.y - y1 + 4.0),
        Rect::new(-4.0, y0, x0 + 4.0, y1 - y0),
        Rect::new(x1, y0, v.x - x1 + 4.0, y1 - y0),
    ] {
        layer.panels.push(flat(r, shade, 0.0));
    }
    layer.panels.push(Panel {
        fill: Color::TRANSPARENT,
        border: palette::CREAM.with_alpha(0.85 * smooth(k)),
        border_px: 3.0,
        radius: 10.0,
        shadow: false,
        ..Panel::new(w)
    });
}

impl Menu {
    /// The room the window is of and how open it is, this frame; None at the station.
    fn open(&self, stage: &Stage) -> Option<(String, f32)> {
        let p = if self.busy { self.t / TIME } else { 1.0 };
        let room = |id: &str| match shared::end(stage, id) {
            End::Room { id, .. } => Some(id.to_string()),
            End::Station => None,
        };
        if !self.busy {
            return room(&stage.model.here).map(|r| (r, 1.0));
        }
        match (room(&self.from), room(&self.to)) {
            (None, Some(b)) => Some((b, p)),
            (Some(a), None) => Some((a, 1.0 - p)),
            (Some(a), Some(_)) if p < 0.5 => Some((a, 1.0 - p * 2.0)),
            (Some(_), Some(b)) => Some((b, p * 2.0 - 1.0)),
            (None, None) => None,
        }
    }
}

impl Travel for Menu {
    fn name(&self) -> &'static str {
        "menu"
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
        match self.open(stage) {
            Some((room, k)) if k > 0.0 => view(stage, &room, k),
            _ => stage.view(&stage.model.station().id),
        }
    }

    fn overlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if self.busy
            && let Some((room, k)) = self.open(stage)
        {
            dim(stage, &room, k, layer);
        }
    }

    fn underlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if let Some((room, k)) = self.open(stage) {
            dim(stage, &room, k, layer);
        }
    }
}
