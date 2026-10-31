//! The scenes behind the screens, drawn with the slice's kit: Lantern Wharf up close with
//! its modules, and the Wren in the cradle. Each frame they publish the screen anchors
//! the UI attaches to (`module.market`, `socket.nose`, ...).

use render::glam::{Mat4, Vec2, Vec3};
use render::kit::{
    self, Cradle, CradlePose, HullParams, LookId, ShipFitLook, Socket, SocketRing, StationRecipe,
};
use render::{
    BackdropHandle, Camera, Glow, MAX_POINT_LIGHTS, Material, MeshHandle, MeshInstance, PointLight,
    Renderer, Scene,
};
use serde_json::Value as Json;

use crate::ui::draw::{Anchor, Anchors};

const STATION_ID: &str = "station.lantern_wharf";
/// How far a module's strut reaches back to the station's art, in recipe units.
const CORE: f32 = 12.0;
const MODULE_REACH: f32 = 5.0;
const LIFT: f32 = 2.2;
const LIFT_SCALE: f32 = 1.08;
const TILT: f32 = 24.0;

/// The station's modules: id, kit module kind, anchor and whether it is being built.
const MODULES: [(&str, &str, &str, bool); 5] = [
    ("office", "block", "quay.west", false),
    ("market", "banner", "quay.north", false),
    ("wharf", "docking_arm", "pier.south", false),
    ("notices", "lamp_mast", "quay.northwest", false),
    ("canteen", "dome", "pier.southwest", true),
];

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
        let anchors = kit::anchors(STATION_ID, &recipe);
        let modules = MODULES
            .iter()
            .filter_map(|&(id, kind, anchor, building)| {
                let a = anchors.iter().find(|a| a.name == anchor)?;
                let part = kit::module_part(kind, 1)?;
                let strut = (a.at.distance - CORE).clamp(3.0, 16.0);
                let mesh = kit::module(
                    &part,
                    recipe.palette,
                    recipe.faction.clone(),
                    strut,
                    building,
                );
                Some(Module {
                    id,
                    mesh: r.upload_mesh(&mesh),
                    at: a.at.matrix(),
                    lift: 0.0,
                })
            })
            .collect();
        let backdrop = r.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), 7));
        let hull = HullParams::wren();
        let look = ShipFitLook::default();
        let ship = r.upload_mesh(&kit::fitted_hull(&hull, 1, &look));
        let whole_zoom = recipe.footprint().max(10.0) * 2.7;
        let cam = Cam {
            center: Vec2::new(0.0, -2.0),
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
        self.kind = kind;
        self.focus = focus.map(str::to_string);
        let aspect = viewport.x / viewport.y.max(1.0);
        self.goal = match kind {
            SceneKind::Station => match focus.and_then(|f| self.modules.iter().find(|m| m.id == f))
            {
                // The module sits in the left part of the screen, beside its room panel.
                Some(m) => {
                    let zoom = self.whole_zoom * 0.62;
                    let p = Self::module_pos(m).truncate();
                    Cam {
                        center: p + Vec2::new((0.5 - 0.27) * zoom * aspect, -2.0),
                        zoom,
                    }
                }
                None => Cam {
                    center: Vec2::new(0.0, -2.0),
                    zoom: self.whole_zoom,
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
                                radius: MODULE_REACH * 1.3 / px,
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
