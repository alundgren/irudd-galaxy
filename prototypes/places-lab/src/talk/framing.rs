//! The camera frames the speaker: when someone starts talking the room's camera cuts in
//! close on their face, on a longer lens, and eases back out when the talk ends. No
//! black bars: the line is a caption low on the screen, under the speaker, inside the
//! scene.
//!
//! It frames as close as the room allows: whatever the room's screen hangs on the scene
//! (other people's signs, the Wren's sockets and their labels) stays on screen and clear
//! of the room's panel and the caption, so in the cradle it can only push in a little.

use render::glam::{Vec2, Vec3};
use render::kit::HullParams;
use render::{Camera, Projection, Rect, Renderer};

use super::{Line, Talk, blend, fits, hung, name_row, next_button, panel_left, smooth, usable};
use crate::places::Places;
use crate::stage::Shown;
use crate::widgets::W;
use crate::world::World;

/// Seconds to ease back out: short enough that what the pointer rests on settles fast.
const EASE_OUT: f32 = 0.2;
/// The framed lens is this much narrower than the room's.
const LENS: f32 = 0.55;
/// How much of the person shows top to bottom, metres: head and shoulders, some room.
const FRAME_H: f32 = 1.7;
/// Where the face lands, as a share of the screen: left of the room's panel, a little
/// above the middle; further left if that lets the camera come closer.
const FACE_AT: [Vec2; 3] = [
    Vec2::new(0.29, 0.40),
    Vec2::new(0.2, 0.40),
    Vec2::new(0.12, 0.40),
];
/// The caption's room at the bottom, logical pixels.
const CAPTION_H: f32 = 130.0;

#[derive(Default)]
pub struct Framing {
    hull: Option<HullParams>,
    /// How far in the camera is, 0 to 1.
    k: f32,
    /// The face it frames, kept while easing back out.
    face: Option<Vec3>,
    place: String,
    /// The shot, worked out once per speaker and room camera: the room's camera it was
    /// for, the close one, and how far towards it the room allows.
    shot: Option<(Camera, Camera, f32)>,
}

impl Talk for Framing {
    fn name(&self) -> &'static str {
        "framing"
    }

    fn setup(&mut self, _r: &mut Renderer, world: &World, _model: &Places) {
        self.hull = Some(world.hull.clone());
    }

    fn widgets(&self, line: &Line) -> Vec<W> {
        vec![
            W::new("panel")
                .set("id", "caption")
                .set("role", "speech")
                .set("place", "bottom-left")
                .set("width", 600)
                .set("pad", 14)
                .set("gap", 6)
                .child(name_row(line))
                .child(
                    W::new("row")
                        .set("gap", 16)
                        .set("align", "end")
                        .child(W::text(line.text.as_str()).set("grow", 1))
                        .child(next_button(line)),
                ),
        ]
    }

    fn frame(&mut self, shown: &mut Shown, line: Option<&Line>, viewport: Vec2, dt: f32) {
        // A new view (another room, or a move under way) starts wide.
        if shown.place != self.place {
            self.place = shown.place.clone();
            self.k = 0.0;
            self.face = None;
        }
        let face = line.and_then(|l| shown.point(&format!("person.{}.face", l.who)));
        match face {
            Some(p) => {
                // A new speaker gets a new shot; the same one keeps theirs, so the camera
                // holds still while they breathe and nod.
                if self.k < 1.0 || self.face.is_none_or(|f| f.distance(p.at) > 0.3) {
                    self.shot = None;
                    self.face = Some(p.at);
                }
                self.k = 1.0;
            }
            None if line.is_some() => self.k = 0.0,
            None => self.k = (self.k - dt / EASE_OUT).max(0.0),
        }
        let (Some(face), Some(hull), true) = (self.face, &self.hull, self.k > 0.0) else {
            return;
        };
        let base = shown.scene.camera;
        if self.shot.is_none_or(|(b, _, _)| b != base) {
            self.shot = shot(shown, hull, face, viewport).map(|(c, e)| (base, c, e));
        }
        if let Some((_, close, most)) = self.shot {
            shown.scene.camera = blend(&base, &close, smooth(self.k) * most);
        }
    }
}

/// The closest shot of `face` that keeps the room's things where they can be used: on
/// screen, clear of the room's panel and the caption. The close camera and how far
/// towards it to go.
fn shot(shown: &Shown, hull: &HullParams, face: Vec3, viewport: Vec2) -> Option<(Camera, f32)> {
    let base = shown.scene.camera;
    let region = Rect::new(
        16.0,
        16.0,
        panel_left(viewport) - 16.0,
        viewport.y - 32.0 - CAPTION_H,
    );
    let wide = hung(shown, hull, &base, viewport);
    let screen = usable(viewport);
    let mut best: Option<(Camera, f32)> = None;
    for at in FACE_AT {
        let close = framed(&base, face, at, viewport)?;
        let most = (0..=20)
            .map(|i| 1.0 - i as f32 * 0.05)
            .find(|&e| {
                let boxes = hung(shown, hull, &blend(&base, &close, e), viewport);
                fits(&boxes, &wide, screen, region)
            })
            .unwrap_or(0.0);
        if best.is_none_or(|(_, e)| most > e + 0.01) {
            best = Some((close, most));
        }
    }
    best
}

/// The room's camera close on `face`: the same direction, nearer level, a longer lens,
/// the face at `at` (a share of the screen).
fn framed(base: &Camera, face: Vec3, at: Vec2, viewport: Vec2) -> Option<Camera> {
    let Projection::Perspective { fov_y, near, far } = base.projection else {
        return None;
    };
    let wide = (base.target - base.eye).normalize_or(Vec3::Y);
    let fwd = Vec3::new(wide.x, wide.y, wide.z * 0.4).normalize();
    let fov = fov_y * LENS;
    let half = (fov * 0.5).tan();
    let dist = FRAME_H * 0.5 / half;
    let right = fwd.cross(base.up).normalize_or(Vec3::X);
    let up = right.cross(fwd);
    let aspect = viewport.x / viewport.y.max(1.0);
    let nx = 2.0 * at.x - 1.0;
    let ny = 1.0 - 2.0 * at.y;
    let target = face - right * (nx * dist * half * aspect) - up * (ny * dist * half);
    Some(Camera {
        eye: target - fwd * dist,
        target,
        up: base.up,
        projection: Projection::Perspective {
            fov_y: fov,
            near,
            far,
        },
    })
}
