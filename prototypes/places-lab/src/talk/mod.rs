//! Choice 3: how people talk. Key `3` cycles the variants.
//!
//! A screen says who speaks and what (`ui.talk{ who = "ilse", text = ..., more = ...,
//! on = "talk_next" }`); the talk variant decides how that shows. When the screen runs,
//! [`Talk::widgets`] turns the line into ordinary widgets (a bubble on the person, a
//! portrait panel, a column); every frame [`Talk::frame`] may move the view (a camera
//! that frames the speaker).
//!
//! Whatever a variant does, the speaker's face must stay on screen and uncovered while
//! their line shows: the lab checks it every frame and `--shots` fails if it breaks.
//!
//! To add a variant: a new file here with a type that implements [`Talk`], and one line
//! in [`all`]. Nothing outside this folder needs to change.

mod bubble;
mod column;
mod framing;
mod portrait;

use render::glam::{Vec2, Vec3};
use render::kit::{self, HullParams};
use render::{Camera, Projection, Rect, Renderer};
use serde_json::Value as Json;

use crate::places::Places;
use crate::stage::Shown;
use crate::ui::rules;
use crate::widgets::W;
use crate::world::World;

/// A line being said, with the speaker as the place model knows them.
#[derive(Clone, Debug, Default)]
pub struct Line {
    /// The speaker's person id: their anchors are `person.<who>` (over the head) and
    /// `person.<who>.face`.
    pub who: String,
    pub name: String,
    pub role: String,
    pub text: String,
    /// More lines follow.
    pub more: bool,
    /// The event a click on the line sends, and its args.
    pub on: Option<String>,
    pub args: Json,
    /// Lines said before this one in the same talk, oldest first.
    #[allow(dead_code)] // for variants (a text column)
    pub before: Vec<String>,
}

pub trait Talk {
    /// The variant's name, by technique: "bubble", "portrait", ...
    fn name(&self) -> &'static str;
    /// Once at start and after F5, for every variant (not only the picked one): upload
    /// any meshes of its own.
    fn setup(&mut self, _r: &mut Renderer, _world: &World, _model: &Places) {}
    /// The widgets that show this line, when the screen runs (not per frame).
    fn widgets(&self, line: &Line) -> Vec<W>;
    /// Each frame, while in a place: may change the view, for instance its camera, with
    /// the line being said if there is one. `viewport` is the screen in logical pixels.
    fn frame(&mut self, _shown: &mut Shown, _line: Option<&Line>, _viewport: Vec2, _dt: f32) {}
}

/// Every variant, in the order the key cycles them. The first is the baseline.
pub fn all() -> Vec<Box<dyn Talk>> {
    vec![
        Box::new(bubble::Bubble),
        Box::new(portrait::Portrait::default()),
        Box::new(framing::Framing::default()),
        Box::new(column::Column::default()),
    ]
}

/// The line's name and role, as a heading row.
fn name_row(line: &Line) -> W {
    W::new("row")
        .set("gap", 10)
        .set("align", "center")
        .child(W::text(line.name.as_str()).set("style", "heading"))
        .child(W::text(line.role.as_str()).set("style", "caption"))
}

/// The button that moves the talk on: the shot run clicks it as `bubble`.
fn next_button(line: &Line) -> W {
    W::new("button")
        .set("id", "bubble")
        .set("text", if line.more { "Go on" } else { "Done" })
        .set("on", line.on.clone())
        .set("args", line.args.clone())
}

/// 0 to 1, eased at both ends.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A camera's axes and how screen pixels map into its world, for variants that place
/// things by where they land on screen.
struct Lens {
    eye: Vec3,
    /// Forward, right and screen-up, unit length.
    fwd: Vec3,
    right: Vec3,
    up: Vec3,
    viewport: Vec2,
    camera: Camera,
}

impl Lens {
    fn new(camera: &Camera, viewport: Vec2) -> Lens {
        let fwd = (camera.target - camera.eye).normalize_or(Vec3::NEG_Z);
        let right = fwd.cross(camera.up).normalize_or(Vec3::X);
        Lens {
            eye: camera.eye,
            fwd,
            right,
            up: right.cross(fwd),
            viewport: viewport.max(Vec2::ONE),
            camera: *camera,
        }
    }

    /// Half the view's height, in world units, at `depth` in front of the eye.
    fn half_height(&self, depth: f32) -> f32 {
        match self.camera.projection {
            Projection::Perspective { fov_y, .. } => depth * (fov_y * 0.5).tan(),
            Projection::Orthographic { view_height, .. } => view_height * 0.5,
        }
    }

    /// World units per logical pixel at `depth`.
    fn per_px(&self, depth: f32) -> f32 {
        2.0 * self.half_height(depth) / self.viewport.y
    }

    /// The point `depth` in front of the eye that lands on pixel `px`.
    fn at(&self, px: Vec2, depth: f32) -> Vec3 {
        let h = self.half_height(depth);
        let aspect = self.viewport.x / self.viewport.y;
        let nx = 2.0 * px.x / self.viewport.x - 1.0;
        let ny = 1.0 - 2.0 * px.y / self.viewport.y;
        self.eye + self.fwd * depth + self.right * (nx * h * aspect) + self.up * (ny * h)
    }
}

/// The widest the room's panel gets, from the right edge (`ui::rules::ROOM_SHARE`, plus
/// its margin): a camera variant keeps what the room hangs on the scene left of it.
fn panel_left(viewport: Vec2) -> f32 {
    viewport.x * (1.0 - rules::ROOM_SHARE) - 24.0
}

/// What the room's screen hangs on the scene, seen through `cam`: a box round each thing
/// with the room its widget takes (a face, the sign over a head, a socket with its label
/// to the right). Off screen or behind the eye counts as far off screen.
fn hung(shown: &Shown, hull: &HullParams, cam: &Camera, viewport: Vec2) -> Vec<Rect> {
    let fwd = (cam.target - cam.eye).normalize_or(Vec3::NEG_Z);
    let far = Rect::new(-1.0e4, -1.0e4, 1.0, 1.0);
    let mut out = Vec::new();
    for p in &shown.points {
        let Some(rest) = p.name.strip_prefix("person.") else {
            continue;
        };
        let at = if (p.at - cam.eye).dot(fwd) > 0.1 {
            cam.world_to_screen(p.at, viewport)
        } else {
            None
        };
        out.push(match at {
            None => far,
            Some(a) if rest.ends_with(".face") => Rect::new(a.x - 24.0, a.y - 24.0, 48.0, 48.0),
            Some(a) => Rect::new(a.x - 52.0, a.y - 55.0, 104.0, 65.0),
        });
    }
    if let Some(pose) = &shown.cradle {
        for s in kit::cradle_sockets(hull, pose, cam, viewport) {
            let r = s.radius_px.max(22.0);
            out.push(match s.screen {
                None => far,
                Some(a) => Rect::new(a.x - r - 6.0, a.y - r, r * 2.0 + 14.0 + 182.0, r * 2.0),
            });
        }
    }
    out
}

/// The screen left of the widest room panel, inside the margins.
fn usable(viewport: Vec2) -> Rect {
    Rect::new(16.0, 16.0, panel_left(viewport) - 16.0, viewport.y - 32.0)
}

/// Every box inside `region`, of those [`hung`] found inside `screen` in the room's own
/// view (`first`): what was already off it isn't the talk's to bring back.
fn fits(boxes: &[Rect], first: &[Rect], screen: Rect, region: Rect) -> bool {
    boxes
        .iter()
        .zip(first)
        .all(|(b, f)| !rules::contains(screen, *f) || rules::contains(region, *b))
}

/// `cam` turned on the spot so the picture moves `shift` pixels (right, down): close to
/// a lens shift for small turns.
fn turned(cam: &Camera, viewport: Vec2, shift: Vec2) -> Camera {
    let lens = Lens::new(cam, viewport);
    let k = 2.0 * lens.half_height(1.0) / lens.viewport.y;
    let dist = (cam.target - cam.eye).length();
    let dir = (lens.fwd - lens.right * (shift.x * k) + lens.up * (shift.y * k)).normalize();
    Camera {
        target: cam.eye + dir * dist,
        ..*cam
    }
}

/// `a` towards `b` by `t`: eye, target and field of view.
fn blend(a: &Camera, b: &Camera, t: f32) -> Camera {
    let projection = match (a.projection, b.projection) {
        (
            Projection::Perspective { fov_y, near, far },
            Projection::Perspective { fov_y: fb, .. },
        ) => Projection::Perspective {
            fov_y: fov_y + (fb - fov_y) * t,
            near,
            far,
        },
        (p, _) => p,
    };
    Camera {
        eye: a.eye.lerp(b.eye, t),
        target: a.target.lerp(b.target, t),
        up: a.up,
        projection,
    }
}
