//! What the travel variants share: timing curves, camera blends, the ends of a move,
//! a room's set standing inside its module, and screen veils.

use render::glam::{Mat4, Vec2, Vec3};
use render::{Camera, Color, MeshInstance, Panel, Projection, Rect, UiLayer};

use crate::places::Kind;
use crate::stage::{Shown, Stage};
use crate::world::{self, Module};

/// The dark the lab fades through.
pub const DARK: Color = Color::linear(0.011, 0.007, 0.018, 1.0);

/// Where `t` is between `a` and `b`, from 0 to 1.
pub fn span(t: f32, a: f32, b: f32) -> f32 {
    ((t - a) / (b - a).max(1e-6)).clamp(0.0, 1.0)
}

pub fn smooth(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

pub fn ease_out(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// One end of a move: the station from outside, or a room in a module.
pub enum End<'a> {
    Station,
    Room {
        id: &'a str,
        module: Option<&'a Module>,
    },
}

pub fn end<'a>(stage: &'a Stage, id: &'a str) -> End<'a> {
    match stage.place(id) {
        Some(p) if p.kind == Kind::Interior => End::Room {
            id,
            module: stage
                .model
                .module_of(id)
                .and_then(|m| stage.world.module(&m.id)),
        },
        Some(p) if p.kind == Kind::Module => End::Room {
            id,
            module: stage.world.module(id),
        },
        _ => End::Station,
    }
}

/// The station's camera as the player left it (their zoom and pan), whole.
pub fn station_camera(stage: &Stage) -> Camera {
    stage.world.exterior_camera(None, stage.aspect())
}

/// The vertical field of view in radians; an orthographic camera counts as nearly 0.
fn fov(cam: &Camera) -> f32 {
    match cam.projection {
        Projection::Perspective { fov_y, .. } => fov_y,
        Projection::Orthographic { .. } => 0.0,
    }
}

/// How much of the world the camera shows from top to bottom at its target.
pub fn view_height(cam: &Camera) -> f32 {
    match cam.projection {
        Projection::Orthographic { view_height, .. } => view_height,
        Projection::Perspective { fov_y, .. } => {
            2.0 * (cam.eye - cam.target).length() * (fov_y * 0.5).tan()
        }
    }
}

/// A camera from `eye` direction looking at `target`, `height` of the world framed at
/// the target, through `fov_deg`.
pub fn framing(target: Vec3, from: Vec3, height: f32, fov_deg: f32) -> Camera {
    let f = fov_deg.to_radians();
    let dist = height / (2.0 * (f * 0.5).tan());
    Camera::perspective(target + from.normalize_or(Vec3::Z) * dist, target, fov_deg)
}

/// The camera `k` times as far back from its target (`k` above 1 shows more).
pub fn pulled_back(cam: &Camera, k: f32) -> Camera {
    let mut c = *cam;
    match &mut c.projection {
        Projection::Orthographic { view_height, .. } => *view_height *= k,
        Projection::Perspective { .. } => c.eye = c.target + (c.eye - c.target) * k,
    }
    c
}

/// A flight between two cameras: the framed height changes evenly (in ratio), and the
/// target drifts so the place you fly toward keeps steady on screen. An orthographic end
/// is flown as a very narrow perspective, so the ends match what is drawn at rest.
pub fn blend(a: &Camera, b: &Camera, t: f32) -> Camera {
    let t = t.clamp(0.0, 1.0);
    if t <= 0.0 {
        return *a;
    }
    if t >= 1.0 {
        return *b;
    }
    let both_ortho = matches!(a.projection, Projection::Orthographic { .. })
        && matches!(b.projection, Projection::Orthographic { .. });
    let (ha, hb) = (view_height(a), view_height(b));
    let h = ha.max(1e-3).powf(1.0 - t) * hb.max(1e-3).powf(t);
    // How far the target has come: by the zoom when there is one, else by time.
    let u = if (ha - hb).abs() > ha.max(hb) * 0.05 {
        1.0 - ((h - hb) / (ha - hb)).clamp(0.0, 1.0)
    } else {
        t
    };
    let target = a.target.lerp(b.target, u);
    let dir = (a.eye - a.target)
        .normalize_or(Vec3::Z)
        .lerp((b.eye - b.target).normalize_or(Vec3::Z), t)
        .normalize_or(Vec3::Z);
    let up = a.up.lerp(b.up, t).normalize_or(Vec3::Z);
    if both_ortho {
        let mut c = Camera::top_down(Vec2::ZERO, h);
        c.target = target;
        c.eye = target + dir * (a.eye - a.target).length();
        c.up = up;
        return c;
    }
    let narrow = 6.0f32.to_radians();
    let fa = if fov(a) > 0.0 { fov(a) } else { narrow };
    let fb = if fov(b) > 0.0 { fov(b) } else { narrow };
    let f = fa + (fb - fa) * t;
    let dist = h / (2.0 * (f * 0.5).tan());
    let mut c = Camera::perspective(target + dir * dist, target, f.to_degrees());
    c.up = up;
    c
}

/// Where a room's set stands inside its module: set space to world, scaled to fit the
/// module and facing the station's camera like the module does.
pub fn inside(stage: &Stage, room: &str, module: &Module) -> Mat4 {
    let floor = stage
        .place(room)
        .map(|p| p.floor)
        .unwrap_or(Vec2::splat(8.0));
    let fit = world::MODULE_REACH * world::MODULE_SCALE * 0.72;
    let s = fit / floor.x.max(floor.y).max(1.0);
    Mat4::from_translation(module.pos() + Vec3::Z * 0.3) * Mat4::from_scale(Vec3::splat(s))
}

/// The room's own camera, for its set placed by `at`.
pub fn set_camera_at(stage: &Stage, room: &str, at: Mat4) -> Option<Camera> {
    let c = stage.set_camera(room)?;
    let mut out = Camera::perspective(
        at.transform_point3(c.eye),
        at.transform_point3(c.target),
        fov(&c).to_degrees(),
    );
    out.up = c.up;
    Some(out)
}

/// Mesh `i` of the station's outside: the core is 0, then the modules in the world's
/// order. Which one is this module.
pub fn module_index(stage: &Stage, id: &str) -> Option<usize> {
    stage
        .world
        .modules
        .iter()
        .position(|m| m.id == id)
        .map(|i| i + 1)
}

/// A mesh going (`k` = 1) into the dark: sinks by `drop` and dims to [`DARK`].
pub fn dim(m: &mut MeshInstance, k: f32, drop: Vec3) {
    let k = k.clamp(0.0, 1.0);
    m.transform = Mat4::from_translation(drop * k) * m.transform;
    m.material.tint = m.material.tint.lerp(DARK, k);
    m.material.emissive = m.material.emissive.scale(1.0 - k);
}

/// Lights and glows of a view at `k` of their strength.
pub fn lights_at(shown: &mut Shown, k: f32) {
    let k = k.clamp(0.0, 1.0);
    for g in &mut shown.scene.glows {
        g.color = g.color.with_alpha(g.color.a * k);
    }
    for p in &mut shown.scene.lights.points {
        p.color = p.color.scale(k);
    }
}

/// A plain panel without border or shadow.
pub fn flat(rect: Rect, fill: Color, radius: f32) -> Panel {
    Panel {
        fill,
        border: Color::TRANSPARENT,
        border_px: 0.0,
        radius,
        shadow: false,
        ..Panel::new(rect)
    }
}

/// The whole screen in one colour.
pub fn veil(layer: &mut UiLayer, viewport: Vec2, color: Color, alpha: f32) {
    if alpha <= 0.001 {
        return;
    }
    layer.panels.push(flat(
        Rect::new(-4.0, -4.0, viewport.x + 8.0, viewport.y + 8.0),
        color.with_alpha(alpha.clamp(0.0, 1.0)),
        0.0,
    ));
}
