//! The world as the player sees it.
//!
//! [`View`] holds what modules bring for the eye ([`assets::Assets`]: models and styles),
//! uploads the models to a renderer once, and each frame turns the world into a
//! [`Scene`]: every thing with an `engine.position` and an `engine.model` drawn where it
//! is, in its look, and the camera on the thing with an `engine.camera`. Positions are
//! interpolated between the last two ticks, so motion is smooth at any frame rate while
//! the sim steps at its fixed rate. The view only reads the world; nothing here changes
//! it, so what is drawn never affects a replay. How it works:
//! `engine/docs/renderer-and-style.md`.

pub mod assets;
pub mod course;
pub mod film;

use std::collections::{BTreeMap, BTreeSet};

use galaxy_api::engine::{CAMERA, MODEL, POSITION, STEER, WAYPOINT};
use galaxy_mesh::{Color, MeshBuilder, MeshData};
use galaxy_module::module::Module;
use galaxy_render::glam::{Mat4, Quat, Vec2, Vec3};
use galaxy_render::{
    BackdropHandle, Camera, Material, MeshHandle, MeshInstance, Panel, Projection, Rect, Renderer,
    Scene, Text, backdrop, defaults,
};
use galaxy_sim::World;
use serde_json::{Map, Value, json};

pub use assets::Assets;

/// Where a thing is drawn: the sim's position, as floats for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub x: f32,
    pub y: f32,
    pub height: f32,
    pub angle: f32,
}

impl Pose {
    fn of(fields: &Map<String, Value>) -> Pose {
        let n = |k: &str| fields.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
        Pose {
            x: n("x"),
            y: n("y"),
            height: n("height"),
            angle: n("angle"),
        }
    }

    /// `t` of the way from `self` to `next`, turning the short way round.
    pub fn lerp(self, next: Pose, t: f32) -> Pose {
        let tau = std::f32::consts::TAU;
        let mut turn = (next.angle - self.angle) % tau;
        if turn > tau / 2.0 {
            turn -= tau;
        } else if turn < -tau / 2.0 {
            turn += tau;
        }
        Pose {
            x: self.x + (next.x - self.x) * t,
            y: self.y + (next.y - self.y) * t,
            height: self.height + (next.height - self.height) * t,
            angle: self.angle + turn * t,
        }
    }

    pub fn point(&self) -> Vec3 {
        Vec3::new(self.x, self.y, self.height)
    }
}

/// An `engine.camera`, read from the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraSpec {
    pub tilt: f32,
    pub view: f32,
    pub min_view: f32,
    pub max_view: f32,
    pub fov: f32,
}

impl Default for CameraSpec {
    fn default() -> Self {
        Self {
            tilt: 30.0,
            view: 60.0,
            min_view: 20.0,
            max_view: 240.0,
            fov: 0.0,
        }
    }
}

impl CameraSpec {
    fn of(fields: &Map<String, Value>) -> CameraSpec {
        let d = CameraSpec::default();
        let n = |k: &str, or: f32| {
            fields
                .get(k)
                .and_then(Value::as_f64)
                .map(|v| v as f32)
                .filter(|v| v.is_finite())
                .unwrap_or(or)
        };
        let min_view = n("min_view", d.min_view).max(1.0);
        CameraSpec {
            tilt: n("tilt", d.tilt).clamp(0.0, 80.0),
            view: n("view", d.view).max(1.0),
            min_view,
            max_view: n("max_view", d.max_view).max(min_view),
            fov: n("fov", d.fov).clamp(0.0, 120.0),
        }
    }

    /// A camera looking at `target`, showing `view` world units top to bottom.
    pub fn camera(&self, target: Vec3, view: f32) -> Camera {
        let t = self.tilt.to_radians();
        let back = Vec3::new(0.0, -t.sin(), t.cos());
        let up = Vec3::new(0.0, t.cos(), t.sin());
        if self.fov > 0.0 {
            let fov = self.fov.to_radians();
            let distance = view * 0.5 / (fov * 0.5).tan();
            Camera {
                eye: target + back * distance,
                target,
                up,
                projection: Projection::Perspective {
                    fov_y: fov,
                    near: (distance * 0.05).max(0.1),
                    far: distance * 20.0 + 1000.0,
                },
            }
        } else {
            let distance = 400.0 + view;
            Camera {
                eye: target + back * distance,
                target,
                up,
                projection: Projection::Orthographic {
                    view_height: view,
                    near: 1.0,
                    far: distance * 2.0,
                },
            }
        }
    }
}

/// The thing the camera follows, and its camera: the first entity, in ID order, with an
/// `engine.camera`.
pub fn camera_holder(world: &World) -> Option<(&str, CameraSpec)> {
    world.entities.iter().find_map(|(id, components)| {
        let spec = CameraSpec::of(&components.get(CAMERA)?.fields);
        Some((id.as_str(), spec))
    })
}

/// What a note keeps of how a moment was drawn, so the same frame can be drawn again from
/// a replay: how far between ticks it was, the presentation time, the frame's size and
/// scale, the player's zoom, and the camera it ended up with.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Moment {
    pub alpha: f32,
    pub time: f32,
    /// The frame in physical pixels, and the window's scale factor.
    pub width: u32,
    pub height: u32,
    pub scale: f32,
    /// The view height the wheel zoomed to, if it did.
    pub zoom: Option<f32>,
    /// How long the course had been showing (it settles faint after a while).
    #[serde(default)]
    pub course_age: Option<f32>,
    pub eye: [f32; 3],
    pub target: [f32; 3],
}

impl Moment {
    /// Whether another moment's camera is the same, within float noise.
    pub fn same_camera(&self, other: &Moment) -> bool {
        let near = |a: [f32; 3], b: [f32; 3]| (Vec3::from(a) - Vec3::from(b)).length() < 1e-3;
        near(self.eye, other.eye) && near(self.target, other.target)
    }
}

/// The player's own camera settings: not part of the world.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Eye {
    /// The view height the wheel zoomed to, if it did.
    pub view: Option<f32>,
}

/// Models on a renderer, the style, and what the last frame drew.
pub struct View {
    pub assets: Assets,
    handles: BTreeMap<String, MeshHandle>,
    /// How far each model reaches from its centre on the plane, for rings round it.
    reach: BTreeMap<String, f32>,
    placeholder: MeshHandle,
    sky: BackdropHandle,
    seed: u64,
    /// Poses at the tick before the world's current one.
    previous: BTreeMap<String, Pose>,
    pub eye: Eye,
    /// Model and look IDs things asked for that no module has, once each.
    missing: BTreeSet<String>,
    /// The camera of the last scene.
    pub camera: Option<Camera>,
    pub courses: course::Courses,
}

/// A small inked box, drawn where a model is missing.
fn placeholder() -> MeshData {
    let mut b = MeshBuilder::new();
    b.paint(Color::hex(0xf2b43c), 0.0);
    b.cuboid(Vec3::new(2.0, 2.0, 2.0));
    b.build()
}

impl View {
    pub fn new<'a>(
        renderer: &mut Renderer,
        modules: impl IntoIterator<Item = &'a Module>,
        seed: u64,
    ) -> View {
        View::build(Some(renderer), modules, seed)
    }

    /// A view with no renderer: it builds the same scenes, but they draw nothing. For a
    /// film's probes, which need where things are on screen and not the pixels.
    pub fn detached<'a>(modules: impl IntoIterator<Item = &'a Module>, seed: u64) -> View {
        View::build(None, modules, seed)
    }

    fn build<'a>(
        mut renderer: Option<&mut Renderer>,
        modules: impl IntoIterator<Item = &'a Module>,
        seed: u64,
    ) -> View {
        let assets = Assets::load(modules);
        let (placeholder, sky) = match renderer.as_deref_mut() {
            Some(r) => (
                r.upload_mesh(&placeholder()),
                r.upload_backdrop(&backdrop(&assets.sky.params(), seed)),
            ),
            None => (MeshHandle::DETACHED, BackdropHandle::DETACHED),
        };
        let mut view = View {
            assets: Assets::default(),
            handles: BTreeMap::new(),
            reach: BTreeMap::new(),
            placeholder,
            sky,
            seed,
            previous: BTreeMap::new(),
            eye: Eye::default(),
            missing: BTreeSet::new(),
            camera: None,
            courses: course::Courses::default(),
        };
        view.replace(renderer, assets);
        view
    }

    /// Loads the modules' models and styles again (after a hot-load). A detached view
    /// passes no renderer.
    pub fn reload<'a>(
        &mut self,
        renderer: Option<&mut Renderer>,
        modules: impl IntoIterator<Item = &'a Module>,
    ) {
        let assets = Assets::load(modules);
        self.replace(renderer, assets);
    }

    fn replace(&mut self, mut renderer: Option<&mut Renderer>, assets: Assets) {
        for (_, handle) in std::mem::take(&mut self.handles) {
            if let Some(r) = renderer.as_deref_mut() {
                r.remove_mesh(handle);
            }
        }
        if let Some(r) = renderer.as_deref_mut() {
            r.remove_backdrop(self.sky);
            self.sky = r.upload_backdrop(&backdrop(&assets.sky.params(), self.seed));
        }
        self.reach.clear();
        for (id, mesh) in &assets.models {
            let handle = match renderer.as_deref_mut() {
                Some(r) => r.upload_mesh(mesh),
                None => MeshHandle::DETACHED,
            };
            self.handles.insert(id.clone(), handle);
            let reach = mesh
                .vertices
                .iter()
                .map(|v| v.position[0].hypot(v.position[1]))
                .fold(0.0, f32::max);
            self.reach.insert(id.clone(), reach);
        }
        self.missing.clear();
        self.assets = assets;
    }

    /// Remembers where everything is, before the sim steps. Call it once before each tick
    /// so [`View::scene`] can draw between the last two.
    pub fn before_step(&mut self, world: &World) {
        self.previous = world
            .entities
            .iter()
            .filter_map(|(id, c)| Some((id.clone(), Pose::of(&c.get(POSITION)?.fields))))
            .collect();
    }

    /// Where `entity` is drawn, `alpha` of the way from the last tick to this one.
    pub fn pose(&self, world: &World, entity: &str, alpha: f32) -> Option<Pose> {
        let now = Pose::of(&world.component(entity, POSITION)?.fields);
        Some(match self.previous.get(entity) {
            Some(before) => before.lerp(now, alpha.clamp(0.0, 1.0)),
            None => now,
        })
    }

    /// Zooms by wheel `steps` (positive is closer), within the camera's range.
    pub fn zoom(&mut self, world: &World, steps: f32) {
        let spec = camera_holder(world).map(|(_, s)| s).unwrap_or_default();
        let now = self.eye.view.unwrap_or(spec.view);
        let next = (now * 0.88_f32.powf(steps)).clamp(spec.min_view, spec.max_view);
        self.eye.view = Some(next);
    }

    /// The camera for this frame.
    pub fn frame_camera(&self, world: &World, alpha: f32) -> Camera {
        let (target, spec) = match camera_holder(world) {
            Some((id, spec)) => (
                self.pose(world, id, alpha)
                    .map_or(Vec3::ZERO, |p| p.point()),
                spec,
            ),
            None => (Vec3::ZERO, CameraSpec::default()),
        };
        let view = self
            .eye
            .view
            .unwrap_or(spec.view)
            .clamp(spec.min_view, spec.max_view);
        spec.camera(target, view)
    }

    /// The scene for this frame: `alpha` of the way between the last two ticks, at
    /// `logical` pixels, `time` seconds into the session (for shader shimmer only).
    pub fn scene(&mut self, world: &World, alpha: f32, logical: Vec2, time: f32) -> Scene {
        let camera = self.frame_camera(world, alpha);
        let mut scene = Scene::new(camera);
        scene.time = time;
        scene.ink = self.assets.ink;
        scene.lights = self.assets.light.lights();
        scene.backdrop = Some(self.sky);
        scene.clear_color = self.assets.sky.bottom;
        for (id, components) in &world.entities {
            let Some(model) = components.get(MODEL) else {
                continue;
            };
            let Some(pose) = self.pose(world, id, alpha) else {
                continue;
            };
            let f = &model.fields;
            let text = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or("");
            let number = |k: &str, or: f64| f.get(k).and_then(Value::as_f64).unwrap_or(or);
            let model_id = text("model");
            let handle = match self.handles.get(model_id) {
                Some(h) => *h,
                None => {
                    self.missing
                        .insert(format!("model `{model_id}` ({id} wears it)"));
                    self.placeholder
                }
            };
            let look_id = text("look");
            let material = if look_id.is_empty() {
                Material {
                    outline: scene.ink.color,
                    ..Material::default()
                }
            } else if let Some(look) = self.assets.looks.get(look_id) {
                look.material(&scene.ink)
            } else {
                self.missing
                    .insert(format!("look `{look_id}` ({id} wears it)"));
                Material::default()
            };
            let scale = number("scale", 1.0) as f32;
            let scale = if scale.is_finite() && scale > 0.0 {
                scale
            } else {
                1.0
            };
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(scale),
                Quat::from_rotation_z(pose.angle),
                pose.point(),
            );
            scene.meshes.push(
                MeshInstance::new(handle, transform)
                    .with_material(material)
                    .with_morph(number("morph", 0.0) as f32),
            );
        }
        let (plan, arrived) = self.plan(world, alpha);
        let per_pixel = camera.world_per_pixel(logical);
        self.courses.draw(
            plan,
            arrived,
            &self.assets.course,
            time,
            per_pixel,
            &mut scene,
        );
        self.problems_panel(&mut scene, logical);
        self.camera = Some(camera);
        scene
    }

    /// The followed thing's order, while it runs, for [`course::Courses`]: where it is
    /// going through the waypoints left, with a ring radius at each; and whether an order
    /// that has stopped running got there.
    fn plan(&self, world: &World, alpha: f32) -> (Option<course::Plan>, bool) {
        let Some((id, _)) = camera_holder(world) else {
            return (None, false);
        };
        let Some(order) = world.component(id, STEER).map(|c| &c.fields) else {
            return (None, false);
        };
        let text = |f: &Map<String, Value>, k: &str| {
            f.get(k).and_then(Value::as_str).unwrap_or("").to_owned()
        };
        let mode = text(order, "mode");
        let done = order.get("done").and_then(Value::as_bool).unwrap_or(false);
        if mode.is_empty() || done {
            return (None, done);
        }
        let Some(from) = self.pose(world, id, alpha) else {
            return (None, false);
        };
        let lift = Vec3::Z * 0.1;
        let mut points = vec![from.point() + lift];
        let mut rings = Vec::new();
        let mut target = text(order, "target");
        let end = if target.is_empty() {
            let n = |k: &str| order.get(k).and_then(Value::as_f64).unwrap_or(0.0) as f32;
            points.push(Vec3::new(n("x"), n("y"), from.height) + lift);
            rings.push(1.0);
            format!("{}, {}", n("x"), n("y"))
        } else {
            // The target, then on a route each waypoint after it, at most a few dozen.
            for _ in 0..64 {
                let Some(pose) = self.pose(world, &target, alpha) else {
                    break;
                };
                points.push(pose.point() + lift);
                rings.push(self.ring_radius(world, &target));
                let next = (mode == "follow")
                    .then(|| world.component(&target, WAYPOINT))
                    .flatten()
                    .map(|w| text(&w.fields, "next"))
                    .filter(|n| !n.is_empty() && world.entities.contains_key(n));
                match next {
                    Some(n) => target = n,
                    None => break,
                }
            }
            target
        };
        if points.len() < 2 {
            return (None, false);
        }
        let key = format!("{id} {mode} {end}");
        (Some(course::Plan { key, points, rings }), false)
    }

    /// How far what `entity` wears reaches from its centre on the plane, world units
    /// (scale included); none without a model.
    pub fn reach(&self, world: &World, entity: &str) -> Option<f32> {
        let model = &world.component(entity, MODEL)?.fields;
        let id = model.get("model").and_then(Value::as_str).unwrap_or("");
        let scale = model.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        self.reach.get(id).map(|r| r * scale)
    }

    /// A ring just outside what `entity` wears, or a small one for a bare point.
    fn ring_radius(&self, world: &World, entity: &str) -> f32 {
        let Some(model) = world.component(entity, MODEL).map(|c| &c.fields) else {
            return 1.0;
        };
        let id = model.get("model").and_then(Value::as_str).unwrap_or("");
        let scale = model.get("scale").and_then(Value::as_f64).unwrap_or(1.0) as f32;
        let scale = if scale.is_finite() && scale > 0.0 {
            scale
        } else {
            1.0
        };
        self.reach.get(id).map_or(1.0, |r| r * scale + 0.8)
    }

    /// What is wrong with the modules' models and styles, as text.
    pub fn problems(&self) -> Vec<String> {
        let mut all = self.assets.problems.clone();
        all.extend(self.assets.conflicts.iter().map(assets::Conflict::message));
        all.extend(
            self.missing
                .iter()
                .map(|m| format!("no module has the {m}")),
        );
        all
    }

    /// Problems in a panel at the top left, so a module author sees them while playing.
    fn problems_panel(&self, scene: &mut Scene, logical: Vec2) {
        let problems = self.problems();
        if problems.is_empty() {
            return;
        }
        let width = (logical.x * 0.5).clamp(240.0, 640.0);
        let lines: Vec<String> = problems.iter().take(6).cloned().collect();
        let text = lines.join("\n");
        let height = 24.0 + 20.0 * lines.len() as f32 * 1.6;
        scene.overlay.upper[2].panels.push(Panel {
            fill: defaults::PAPER,
            ..Panel::new(Rect::new(12.0, 12.0, width, height))
        });
        scene.overlay.upper[2].texts.push(
            Text::new(text, Vec2::new(24.0, 22.0), 15.0)
                .color(Color::BLACK)
                .wrap(width - 24.0),
        );
    }

    /// The moment the last scene showed, drawn at `alpha` and `time` into a frame of
    /// `width` x `height` physical pixels at `scale`.
    pub fn moment(&self, alpha: f32, time: f32, size: (u32, u32), scale: f32) -> Option<Moment> {
        let camera = self.camera?;
        Some(Moment {
            alpha,
            time,
            width: size.0,
            height: size.1,
            scale,
            zoom: self.eye.view,
            course_age: self.courses.age(time),
            eye: camera.eye.to_array(),
            target: camera.target.to_array(),
        })
    }

    /// The camera of the last frame, as data for a note.
    pub fn camera_json(&self) -> Value {
        match &self.camera {
            None => Value::Null,
            Some(c) => {
                let mut out = json!({
                    "eye": c.eye.to_array(),
                    "target": c.target.to_array(),
                    "up": c.up.to_array(),
                });
                match c.projection {
                    Projection::Orthographic { view_height, .. } => {
                        out["projection"] = json!("orthographic");
                        out["view"] = json!(view_height);
                    }
                    Projection::Perspective { fov_y, .. } => {
                        out["projection"] = json!("perspective");
                        out["fov"] = json!(fov_y.to_degrees());
                    }
                }
                out
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_interpolate_and_turn_the_short_way() {
        let a = Pose {
            x: 0.0,
            y: 0.0,
            height: 0.0,
            angle: 3.0,
        };
        let b = Pose {
            x: 10.0,
            y: -4.0,
            height: 2.0,
            angle: -3.0,
        };
        let mid = a.lerp(b, 0.5);
        assert_eq!((mid.x, mid.y, mid.height), (5.0, -2.0, 1.0));
        // From 3 to -3 radians is 0.28 rad through pi, not 6 rad back through 0.
        assert!((mid.angle - (3.0 + (std::f32::consts::TAU - 6.0) / 2.0)).abs() < 1e-5);
    }

    #[test]
    fn the_camera_shows_its_view_height_at_its_target() {
        let target = Vec3::new(5.0, 7.0, 0.0);
        for fov in [0.0, 50.0] {
            let spec = CameraSpec {
                fov,
                ..CameraSpec::default()
            };
            let cam = spec.camera(target, 60.0);
            let vp = Vec2::new(1280.0, 720.0);
            let centre = cam.world_to_screen(target, vp).unwrap();
            assert!((centre - vp * 0.5).length() < 1e-2, "{centre}");
            assert!((cam.world_per_pixel(vp) * vp.y - 60.0).abs() < 1e-2);
            // Tilted back: something further up the world is higher on the screen.
            let up = cam.world_to_screen(target + Vec3::Y * 10.0, vp).unwrap();
            assert!(up.y < centre.y);
        }
    }

    #[test]
    fn camera_fields_are_kept_in_range() {
        let fields = json!({ "tilt": 300.0, "view": -5.0, "min_view": 50.0, "max_view": 10.0 });
        let spec = CameraSpec::of(fields.as_object().unwrap());
        assert_eq!(spec.tilt, 80.0);
        assert_eq!(spec.view, 1.0);
        assert_eq!((spec.min_view, spec.max_view), (50.0, 50.0));
    }
}
