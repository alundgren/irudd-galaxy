//! The scenes behind the screens, drawn with the slice's kit: Lantern Wharf up close with
//! its modules, and the Wren in the cradle. Each frame they publish the screen anchors
//! the UI attaches to (`module.market`, `socket.nose`, ...).

use render::glam::{Mat4, Vec2, Vec3};
use render::kit::{
    self, Cradle, CradlePose, HullParams, LookId, Placement, ShipFitLook, Socket, SocketRing,
    StationRecipe,
};
use render::{
    BackdropHandle, Camera, Glow, MAX_POINT_LIGHTS, Material, MeshHandle, MeshInstance, PointLight,
    Renderer, Scene,
};
use serde_json::Value as Json;

use crate::ui::draw::{Anchor, Anchors};

/// How far a module's strut reaches back to the station's art, in recipe units.
const CORE: f32 = 12.0;
const MODULE_REACH: f32 = 5.0;
const LIFT: f32 = 2.2;
const LIFT_SCALE: f32 = 1.08;
const TILT: f32 = 24.0;
/// The wheel's range at the station: from 2.5 times closer to 2.5 times farther.
const WHEEL_IN: f32 = 0.4;
const WHEEL_OUT: f32 = 2.5;

/// The station's modules: id, kit module kind, direction from the core (degrees
/// counter-clockwise from east) and whether it is being built. Spread round the core
/// with room between them for signs and bubbles, not on the slice's crowded quays.
const MODULES: [(&str, &str, f32, bool); 5] = [
    ("office", "block", 180.0, false),
    ("market", "banner", 70.0, false),
    ("wharf", "docking_arm", 290.0, false),
    ("notices", "lamp_mast", 125.0, false),
    ("canteen", "dome", 235.0, true),
];
/// How far out the modules sit (east-west, north-south), and how big they are drawn.
const SPREAD: Vec2 = Vec2::new(62.0, 25.0);
const MODULE_SCALE: f32 = 1.4;

struct Module {
    id: &'static str,
    mesh: MeshHandle,
    /// Module space to world.
    at: Mat4,
    lift: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    Station,
    Cradle,
}

#[derive(Clone, Copy, Debug)]
struct Cam {
    center: Vec2,
    zoom: f32,
}

pub struct World {
    station: MeshHandle,
    recipe: StationRecipe,
    modules: Vec<Module>,
    backdrop: BackdropHandle,
    hull: HullParams,
    look: ShipFitLook,
    ship: MeshHandle,
    ghost: Option<(LookId, Socket, MeshHandle)>,
    /// The hull dips and settles after a fit.
    settle: f32,
    pub kind: SceneKind,
    cam: Cam,
    goal: Cam,
    whole_zoom: f32,
    whole_center: Vec2,
    /// A room's close-up of its module.
    focus_zoom: f32,
    /// The mouse wheel's share of the station zoom (1: as framed).
    wheel: f32,
    /// Where zooming toward the pointer moved the framing.
    pan: Vec2,
    aspect: f32,
    pub time: f32,
    /// The module under the pointer: it lifts and gets an outline.
    pub hover: Option<String>,
    pub focus: Option<String>,
}

fn look_of(s: &str) -> Option<LookId> {
    LookId::ALL.into_iter().find(|l| l.as_str() == s)
}

pub fn socket_of(s: &str) -> Option<Socket> {
    Some(match s {
        "nose" => Socket::Nose,
        "utility" => Socket::Utility,
        "belly" => Socket::Belly,
        "tail" => Socket::Tail,
        _ => return None,
    })
}

fn socket_name(s: Socket) -> &'static str {
    match s {
        Socket::Nose => "nose",
        Socket::Utility => "utility",
        Socket::Belly => "belly",
        Socket::Tail => "tail",
    }
}

impl World {
    pub fn new(r: &mut Renderer) -> World {
        let recipe = StationRecipe::lantern_wharf();
        let station = r.upload_mesh(&kit::station(&recipe));
        let modules: Vec<Module> = MODULES
            .iter()
            .filter_map(|&(id, kind, bearing, building)| {
                let part = kit::module_part(kind, 1)?;
                // On a wide ellipse round the core, as wide as the screen is.
                let b = bearing.to_radians();
                let p = Vec2::new(SPREAD.x * b.cos(), SPREAD.y * b.sin());
                let (angle, distance) = (p.y.atan2(p.x).to_degrees(), p.length());
                // The strut is in the module's own (scaled) units, back to the core.
                let strut = (distance - CORE) / MODULE_SCALE;
                let mesh = kit::module(
                    &part,
                    recipe.palette,
                    recipe.faction.clone(),
                    strut,
                    building,
                );
                let at = Placement {
                    scale: MODULE_SCALE,
                    ..Placement::polar(angle, distance)
                };
                Some(Module {
                    id,
                    mesh: r.upload_mesh(&mesh),
                    at: at.matrix(),
                    lift: 0.0,
                })
            })
            .collect();
        // The whole station framed in the band between the title and the key: the core
        // and every module with room around it for its sign.
        let core = recipe.footprint().max(10.0);
        let (mut lo, mut hi) = (Vec2::splat(-core), Vec2::splat(core));
        for m in &modules {
            let p = Self::module_pos(m).truncate();
            let reach = Vec2::splat(MODULE_REACH * MODULE_SCALE + 4.0);
            lo = lo.min(p - reach);
            hi = hi.max(p + reach);
        }
        let size = hi - lo;
        let whole_zoom = size.y.max(size.x / (16.0 / 9.0)) / 0.74;
        let whole_center = (lo + hi) * 0.5 - Vec2::new(0.0, whole_zoom * 0.03);
        let focus_zoom = core * 2.7 * 0.62;
        let backdrop = r.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), 7));
        let hull = HullParams::wren();
        let look = ShipFitLook::default();
        let ship = r.upload_mesh(&kit::fitted_hull(&hull, 1, &look));
        let cam = Cam {
            center: whole_center,
            zoom: whole_zoom,
        };
        World {
            station,
            recipe,
            modules,
            backdrop,
            hull,
            look,
            ship,
            ghost: None,
            settle: 0.0,
            kind: SceneKind::Station,
            cam,
            goal: cam,
            whole_zoom,
            whole_center,
            focus_zoom,
            wheel: 1.0,
            pan: Vec2::ZERO,
            aspect: 16.0 / 9.0,
            time: 0.0,
            hover: None,
            focus: None,
        }
    }

    fn camera(&self) -> Camera {
        Camera::top_down(self.cam.center, self.cam.zoom).with_tilt(TILT)
    }

    fn module_pos(m: &Module) -> Vec3 {
        m.at.transform_point3(Vec3::ZERO)
    }

    /// Applies what the screen's root asks for: the scene, the focus and the ship's fit.
    pub fn apply(
        &mut self,
        r: &mut Renderer,
        scene: Option<&str>,
        focus: Option<&str>,
        ship: &Json,
        viewport: Vec2,
    ) {
        let kind = match scene {
            Some("cradle") => SceneKind::Cradle,
            _ => SceneKind::Station,
        };
        let changed_scene = kind != self.kind;
        if changed_scene || self.focus.as_deref() != focus {
            // A new framing starts centred.
            self.pan = Vec2::ZERO;
        }
        self.kind = kind;
        self.focus = focus.map(str::to_string);
        self.aspect = viewport.x / viewport.y.max(1.0);
        self.retarget();
        if changed_scene {
            self.cam = self.goal;
        }
        // The ship's fit, from `ship = { nose = "lens", ... }`.
        let mut look = ShipFitLook::default();
        if let Json::Object(map) = ship {
            for (k, v) in map {
                if let (Some(s), Some(l)) = (socket_of(k), v.as_str().and_then(look_of)) {
                    look.set(s, Some(l));
                }
            }
        }
        if look != self.look {
            self.look = look;
            r.update_mesh(self.ship, &kit::fitted_hull(&self.hull, 1, &self.look));
            self.settle = 1.0;
        }
    }

    /// Where the camera heads for the scene and focus, times the wheel's zoom.
    fn retarget(&mut self) {
        let aspect = self.aspect;
        let focused = self
            .focus
            .as_deref()
            .and_then(|f| self.modules.iter().find(|m| m.id == f));
        let pan = self.pan;
        self.goal = match self.kind {
            SceneKind::Station => match focused {
                // The module sits in the left part of the screen, beside its room panel.
                Some(m) => {
                    let zoom = self.focus_zoom * self.wheel;
                    let p = Self::module_pos(m).truncate();
                    Cam {
                        center: p + Vec2::new((0.5 - 0.27) * zoom * aspect, -2.0) + pan,
                        zoom,
                    }
                }
                None => Cam {
                    center: self.whole_center + pan,
                    zoom: self.whole_zoom * self.wheel,
                },
            },
            SceneKind::Cradle => {
                let zoom = 19.0;
                Cam {
                    center: Vec2::new((0.5 - 0.27) * zoom * aspect, 1.2),
                    zoom,
                }
            }
        };
    }

    fn goal_camera(&self) -> Camera {
        Camera::top_down(self.goal.center, self.goal.zoom).with_tilt(TILT)
    }

    /// The mouse wheel at the station: positive steps zoom in, toward the pointer, within
    /// a range. Zoomed in you can look around a little; zooming back out recentres.
    pub fn zoom_at(&mut self, steps: f32, at: Option<Vec2>, viewport: Vec2) {
        if self.kind != SceneKind::Station {
            return;
        }
        let before = self.goal_camera();
        self.set_wheel(self.wheel * 0.88f32.powf(steps));
        let after = self.goal_camera();
        if let Some(p) = at
            && let (Some(a), Some(b)) = (
                before.screen_to_plane(p, viewport, 0.0),
                after.screen_to_plane(p, viewport, 0.0),
            )
        {
            self.pan += (a - b).truncate();
        }
        self.set_wheel(self.wheel);
    }

    /// Sets the wheel's zoom; the pan shrinks to what that zoom allows.
    pub fn set_wheel(&mut self, wheel: f32) {
        self.wheel = wheel.clamp(WHEEL_IN, WHEEL_OUT);
        let room = ((1.0 - self.wheel) / (1.0 - WHEEL_IN)).clamp(0.0, 1.0);
        self.pan = self.pan.clamp_length_max(self.whole_zoom * 0.5 * room);
        self.retarget();
    }

    /// The ghost of the dragged part on the socket it would go to.
    pub fn set_ghost(&mut self, r: &mut Renderer, ghost: Option<(&str, &str)>) {
        let want = ghost.and_then(|(look, socket)| Some((look_of(look)?, socket_of(socket)?)));
        let have = self.ghost.map(|(l, s, _)| (l, s));
        if want == have {
            return;
        }
        if let Some((.., mesh)) = self.ghost.take() {
            r.remove_mesh(mesh);
        }
        if let Some((look, socket)) = want {
            let mesh = r.upload_mesh(&kit::part_ghost(look, &self.hull));
            self.ghost = Some((look, socket, mesh));
        }
    }

    /// Moves the camera and the hover lift on by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.time += dt;
        // About 0.35 s to settle on a new framing.
        let k = 1.0 - (-dt * 9.0).exp();
        self.cam.center = self.cam.center.lerp(self.goal.center, k);
        self.cam.zoom += (self.goal.zoom - self.cam.zoom) * k;
        for m in &mut self.modules {
            let up = self.hover.as_deref() == Some(m.id) || self.focus.as_deref() == Some(m.id);
            let target = if up { 1.0 } else { 0.0 };
            m.lift += (target - m.lift) * (1.0 - (-dt * 14.0).exp());
        }
        self.settle = (self.settle - dt * 1.4).max(0.0);
    }

    /// Lands any glide at once (a second click during a glide).
    pub fn land(&mut self) {
        self.cam = self.goal;
    }

    pub fn gliding(&self) -> bool {
        self.cam.center.distance(self.goal.center) > 0.05
            || (self.cam.zoom - self.goal.zoom).abs() > 0.05
    }

    fn pose(&self) -> CradlePose {
        let mut pose = CradlePose::fit(Vec2::ZERO, 7.5, &self.hull);
        // A dip and settle after a fit.
        let s = self.settle;
        pose.lift -= (s * std::f32::consts::PI * 2.5).sin() * s * 0.35;
        pose
    }

    fn cradle(&self) -> Cradle {
        Cradle {
            ship: self.ship,
            hull: self.hull.clone(),
            look: self.look,
            pose: self.pose(),
            rings: [SocketRing::None; 4],
            ghost: self.ghost.map(|(.., m)| m),
            shadow: kit::CRADLE_SHADOW,
        }
    }

    /// The anchors the UI can attach to this frame.
    pub fn anchors(&self, viewport: Vec2) -> Anchors {
        let cam = self.camera();
        let px = cam.world_per_pixel(viewport).max(f32::EPSILON);
        let mut out = Anchors::new();
        match self.kind {
            SceneKind::Station => {
                for m in &self.modules {
                    let p = Self::module_pos(m) + Vec3::Z * (3.0 + m.lift * LIFT);
                    if let Some(s) = cam.world_to_screen(p, viewport) {
                        out.insert(
                            format!("module.{}", m.id),
                            Anchor {
                                at: s,
                                radius: MODULE_REACH * MODULE_SCALE * 1.3 / px,
                            },
                        );
                    }
                }
            }
            SceneKind::Cradle => {
                // Where Orrin stands: below and left of the ship.
                if let Some(at) = cam.world_to_screen(Vec3::new(-5.5, -7.0, 0.0), viewport) {
                    out.insert("keeper".into(), Anchor { at, radius: 10.0 });
                }
                for s in self.cradle().sockets(&cam, viewport) {
                    if let Some(at) = s.screen {
                        out.insert(
                            format!("socket.{}", socket_name(s.socket)),
                            Anchor {
                                at,
                                radius: s.radius_px.max(22.0),
                            },
                        );
                    }
                }
            }
        }
        out
    }

    pub fn scene(&self, viewport: Vec2) -> Scene {
        let mut scene = Scene::new(self.camera());
        scene.backdrop = Some(self.backdrop);
        scene.time = self.time;
        match self.kind {
            SceneKind::Station => {
                scene
                    .meshes
                    .push(MeshInstance::new(self.station, Mat4::IDENTITY));
                for l in kit::station_lights(&self.recipe) {
                    scene.glows.push(Glow {
                        position: l.position + Vec3::Z * 2.0,
                        radius: l.radius,
                        color: l.color.with_alpha(0.55),
                    });
                    if l.radius > 5.0 && scene.lights.points.len() < MAX_POINT_LIGHTS {
                        scene.lights.points.push(PointLight {
                            position: l.position + Vec3::Z * 3.0,
                            color: l.color.scale(0.8),
                            radius: l.radius * 2.5,
                        });
                    }
                }
                for m in &self.modules {
                    let lift = Mat4::from_translation(Vec3::Z * m.lift * LIFT);
                    let grow = Mat4::from_scale(Vec3::splat(1.0 + (LIFT_SCALE - 1.0) * m.lift));
                    let mut mat = Material::default();
                    if self.hover.as_deref() == Some(m.id) || self.focus.as_deref() == Some(m.id) {
                        mat = mat.with_outline(render::Color::hex(0xE69F00), 6.0);
                    }
                    scene
                        .meshes
                        .push(MeshInstance::new(m.mesh, lift * m.at * grow).with_material(mat));
                }
            }
            SceneKind::Cradle => {
                // A warm lamp over the cradle.
                scene.lights.points.push(PointLight {
                    position: Vec3::new(-2.0, 3.0, 9.0),
                    color: render::palette::LANTERN.scale(0.7),
                    radius: 30.0,
                });
                self.cradle().draw(&mut scene, viewport);
            }
        }
        scene
    }
}
