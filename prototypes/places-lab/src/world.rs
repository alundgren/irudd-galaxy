//! What the views are drawn from, uploaded once: the station's outside with its modules
//! (the slice's kit), each interior's set, each person's figure, and the Wren. The
//! [`crate::stage::Stage`] turns these and the place model into a view each frame.

use std::collections::HashMap;

use render::glam::{Mat4, Vec2, Vec3};
use render::kit::{
    self, Cradle, CradlePose, HullParams, LookId, Placement, ShipFitLook, Socket, SocketRing,
    StationRecipe,
};
use render::{BackdropHandle, Camera, Color, Glow, MeshHandle, PointLight, Renderer};
use serde_json::Value as Json;

use crate::figures::{self, Build};
use crate::places::{Kind, Places};
use crate::sets;

/// How far a module's strut reaches back to the station's art, in recipe units.
const CORE: f32 = 12.0;
pub const MODULE_REACH: f32 = 5.0;
pub const LIFT: f32 = 2.2;
pub const LIFT_SCALE: f32 = 1.08;
pub const TILT: f32 = 24.0;
/// The wheel's range at the station: from 2.5 times closer to 2.5 times farther.
const WHEEL_IN: f32 = 0.4;
const WHEEL_OUT: f32 = 2.5;
/// How far out the modules sit (east-west, north-south), and how big they are drawn.
const SPREAD: Vec2 = Vec2::new(62.0, 25.0);
pub const MODULE_SCALE: f32 = 1.4;

pub struct Module {
    /// The module's place id.
    pub id: String,
    pub mesh: MeshHandle,
    /// Module space to world.
    pub at: Mat4,
    pub lift: f32,
}

impl Module {
    pub fn pos(&self) -> Vec3 {
        self.at.transform_point3(Vec3::ZERO)
    }
}

/// An interior's set, uploaded.
pub struct Set {
    pub mesh: MeshHandle,
    pub lights: Vec<PointLight>,
    pub glows: Vec<Glow>,
    pub eye: Vec3,
    pub target: Vec3,
    pub fov: f32,
    pub ship: Option<(Vec2, f32)>,
    pub clear: Color,
}

impl Set {
    /// The set's own fixed camera, in set space.
    pub fn camera(&self) -> Camera {
        Camera::perspective(self.eye, self.target, self.fov)
    }
}

pub struct Fig {
    pub body: MeshHandle,
    pub head: MeshHandle,
    pub build: Build,
    /// A per-person offset so idle motion doesn't run in step.
    pub phase: f32,
}

pub struct World {
    pub station: MeshHandle,
    pub recipe: StationRecipe,
    pub modules: Vec<Module>,
    pub backdrop: BackdropHandle,
    pub sets: HashMap<String, Set>,
    pub figures: HashMap<String, Fig>,
    pub hull: HullParams,
    pub look: ShipFitLook,
    pub ship: MeshHandle,
    ghost: Option<(LookId, Socket, MeshHandle)>,
    /// The hull dips and settles after a fit.
    settle: f32,
    whole_zoom: f32,
    whole_center: Vec2,
    focus_zoom: f32,
    /// The mouse wheel's share of the station zoom (1: as framed).
    wheel: f32,
    /// Where zooming toward the pointer moved the framing.
    pan: Vec2,
    pub time: f32,
    /// The module under the pointer: it lifts and gets an outline.
    pub hover: Option<String>,
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

pub fn socket_name(s: Socket) -> &'static str {
    match s {
        Socket::Nose => "nose",
        Socket::Utility => "utility",
        Socket::Belly => "belly",
        Socket::Tail => "tail",
    }
}

fn phase_of(id: &str) -> f32 {
    id.bytes().fold(0u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) as f32 % 97.0 * 0.37
}

impl World {
    pub fn new(r: &mut Renderer, model: &Places) -> World {
        let recipe = StationRecipe::lantern_wharf();
        let station = r.upload_mesh(&kit::station(&recipe));
        let modules: Vec<Module> = model
            .places
            .iter()
            .filter(|p| p.kind == Kind::Module)
            .filter_map(|p| {
                let part = kit::module_part(p.art.as_deref()?, 1)?;
                // On a wide ellipse round the core, as wide as the screen is.
                let b = p.bearing.to_radians();
                let at = Vec2::new(SPREAD.x * b.cos(), SPREAD.y * b.sin());
                let (angle, distance) = (at.y.atan2(at.x).to_degrees(), at.length());
                let strut = (distance - CORE) / MODULE_SCALE;
                let mesh = kit::module(
                    &part,
                    recipe.palette,
                    recipe.faction.clone(),
                    strut,
                    p.building,
                );
                let placement = Placement {
                    scale: MODULE_SCALE,
                    ..Placement::polar(angle, distance)
                };
                Some(Module {
                    id: p.id.clone(),
                    mesh: r.upload_mesh(&mesh),
                    at: placement.matrix(),
                    lift: 0.0,
                })
            })
            .collect();
        // The whole station framed in the band between the title and the key.
        let core = recipe.footprint().max(10.0);
        let (mut lo, mut hi) = (Vec2::splat(-core), Vec2::splat(core));
        for m in &modules {
            let p = m.pos().truncate();
            let reach = Vec2::splat(MODULE_REACH * MODULE_SCALE + 4.0);
            lo = lo.min(p - reach);
            hi = hi.max(p + reach);
        }
        let size = hi - lo;
        let whole_zoom = size.y.max(size.x / (16.0 / 9.0)) / 0.74;
        let whole_center = (lo + hi) * 0.5 - Vec2::new(0.0, whole_zoom * 0.03);
        let backdrop = r.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), 7));
        let mut sets = HashMap::new();
        for p in model.places.iter().filter(|p| p.kind == Kind::Interior) {
            let art = sets::build(p.set.as_deref().unwrap_or(""), p.floor);
            sets.insert(
                p.id.clone(),
                Set {
                    mesh: r.upload_mesh(&art.mesh),
                    lights: art.lights,
                    glows: art.glows,
                    eye: art.eye,
                    target: art.target,
                    fov: art.fov,
                    ship: art.ship,
                    clear: art.clear,
                },
            );
        }
        let mut figures = HashMap::new();
        for who in &model.people {
            let f = figures::figure(&who.look);
            figures.insert(
                who.id.clone(),
                Fig {
                    body: r.upload_mesh(&f.body),
                    head: r.upload_mesh(&f.head),
                    build: f.build,
                    phase: phase_of(&who.id),
                },
            );
        }
        let hull = HullParams::wren();
        let look = ShipFitLook::default();
        let ship = r.upload_mesh(&kit::fitted_hull(&hull, 1, &look));
        World {
            station,
            recipe,
            modules,
            backdrop,
            sets,
            figures,
            hull,
            look,
            ship,
            ghost: None,
            settle: 0.0,
            whole_zoom,
            whole_center,
            focus_zoom: core * 2.7 * 0.62,
            wheel: 1.0,
            pan: Vec2::ZERO,
            time: 0.0,
            hover: None,
        }
    }

    pub fn module(&self, id: &str) -> Option<&Module> {
        self.modules.iter().find(|m| m.id == id)
    }

    /// The station seen from outside: whole, or close on one module. `aspect` is the
    /// viewport's width over height.
    pub fn exterior_camera(&self, focus: Option<&str>, aspect: f32) -> Camera {
        let (center, zoom) = match focus.and_then(|f| self.module(f)) {
            Some(m) => {
                let zoom = self.focus_zoom * self.wheel;
                let p = m.pos().truncate();
                (p + Vec2::new((0.5 - 0.27) * zoom * aspect, -2.0) + self.pan, zoom)
            }
            None => (self.whole_center + self.pan, self.whole_zoom * self.wheel),
        };
        Camera::top_down(center, zoom).with_tilt(TILT)
    }

    /// The mouse wheel at the station: positive steps zoom in, toward the pointer, within
    /// a range. Zoomed in you can look around a little; zooming back out recentres.
    pub fn zoom_at(&mut self, steps: f32, at: Option<Vec2>, viewport: Vec2) {
        let aspect = viewport.x / viewport.y.max(1.0);
        let before = self.exterior_camera(None, aspect);
        self.set_wheel(self.wheel * 0.88f32.powf(steps));
        let after = self.exterior_camera(None, aspect);
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
    }

    /// The ship's fit, from the screen's `ship = { nose = "lens", ... }`.
    pub fn apply_ship(&mut self, r: &mut Renderer, ship: &Json) {
        let mut look = ShipFitLook::default();
        if let Json::Object(map) = ship {
            for (k, v) in map {
                if let (Some(s), Some(l)) = (socket_of(k), v.as_str().and_then(look_of)) {
                    look.set(s, Some(l));
                }
            }
        }
        if ship.is_object() && look != self.look {
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

    /// Moves time, the hover lift and the ship's settle on by `dt` seconds.
    pub fn step(&mut self, dt: f32, focus: Option<&str>) {
        self.time += dt;
        for m in &mut self.modules {
            let up = self.hover.as_deref() == Some(m.id.as_str()) || focus == Some(m.id.as_str());
            let target = if up { 1.0 } else { 0.0 };
            m.lift += (target - m.lift) * (1.0 - (-dt * 14.0).exp());
        }
        self.settle = (self.settle - dt * 1.4).max(0.0);
    }

    /// The Wren in a cradle at `center` (set space), `span` long, nose away from the
    /// set's open front.
    pub fn cradle_pose(&self, center: Vec2, span: f32) -> CradlePose {
        let mut pose = CradlePose::fit(center, span, &self.hull);
        let s = self.settle;
        pose.lift -= (s * std::f32::consts::PI * 2.5).sin() * s * 0.35;
        pose
    }

    pub fn cradle(&self, pose: CradlePose) -> Cradle {
        Cradle {
            ship: self.ship,
            hull: self.hull.clone(),
            look: self.look,
            pose,
            rings: [SocketRing::None; 4],
            ghost: self.ghost.map(|(.., m)| m),
            shadow: kit::CRADLE_SHADOW,
        }
    }
}
