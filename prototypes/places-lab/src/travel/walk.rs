//! Walk: the player is a figure standing at the station's hub. Going into a room, the
//! camera comes down over the station, the figure walks along the deck and out along the
//! module's strut to its door and goes in; a short dip, and you are in the room. Coming
//! out, the figure steps out of the module's door and walks back to the hub. The figure
//! is drawn larger than life (the station is drawn small) and walks at a brisk, sped-up
//! pace; the move's time grows with the distance, which is the thing to judge.

use std::collections::HashMap;

use render::glam::{Mat4, Vec2, Vec3};
use render::kit;
use render::{
    Color, Glow, MeshBuilder, MeshData, MeshHandle, MeshInstance, Renderer, UiLayer, palette,
};

use super::Travel;
use super::shared::{self, DARK, End, blend, framing, smooth, span};
use crate::places::{Kind, Places};
use crate::stage::{Shown, Stage};
use crate::world::{self, World};

/// Station units per second the figure walks.
const SPEED: f32 = 24.0;
/// Going in at the door, and the dip between the station and the room (seconds).
const ENTER: f32 = 0.3;
const DIP: f32 = 0.15;
/// The figure's size against the station.
const SIZE: f32 = 4.0;
/// How far the camera leans back from straight down while you walk (degrees).
const WALK_TILT: f32 = 50.0;
/// Where the figure stands at rest: the hub.
const HOME: Vec2 = Vec2::new(0.0, -3.0);
/// Where a module's strut meets the hub: the world's core radius.
const CORE: f32 = 12.0;

#[derive(Default)]
pub struct Walk {
    from: String,
    to: String,
    t: f32,
    busy: bool,
    figure: Option<MeshHandle>,
    /// Where the figure stands at rest, on the hub's deck.
    home: Vec3,
    /// Each room's way from the hub to its module's door.
    ways: HashMap<String, Way>,
}

/// The way to a room: along the deck from the hub to the module's strut and out along
/// it to the door, in points close together on the station's surface.
struct Way {
    module: Vec3,
    points: Vec<Vec3>,
    length: f32,
}

impl Way {
    /// The point `d` along the way, and the way's heading there.
    fn at(&self, d: f32) -> (Vec3, Vec2) {
        let mut left = d.clamp(0.0, self.length);
        let mut heading = Vec2::NEG_Y;
        for w in self.points.windows(2) {
            let step = w[0].truncate().distance(w[1].truncate());
            heading = (w[1] - w[0]).truncate().normalize_or(heading);
            if left <= step {
                let k = if step > 0.0 { left / step } else { 0.0 };
                return (w[0].lerp(w[1], k), heading);
            }
            left -= step;
        }
        (self.door(), heading)
    }

    fn door(&self) -> Vec3 {
        self.points.last().copied().unwrap_or_default()
    }
}

/// The station's surface height under `p`: the highest of `meshes` (each with its
/// placement), or `None` off the station.
fn surface(meshes: &[(MeshData, Mat4)], p: Vec2) -> Option<f32> {
    let top = 80.0;
    meshes
        .iter()
        .filter_map(|(mesh, at)| {
            let inv = at.inverse();
            let o = inv.transform_point3(p.extend(top));
            let d = inv.transform_vector3(Vec3::NEG_Z);
            mesh.raycast(o, d).map(|t| top - t)
        })
        .reduce(f32::max)
}

fn figure() -> MeshData {
    let mut b = MeshBuilder::new();
    // Boots, a suit, a pack and a round helmet with a visor: a spacer, not anyone here.
    b.paint(Color::hex(0x2a2236), 0.0);
    for x in [-0.12f32, 0.12] {
        b.with(Mat4::from_translation(Vec3::new(x, 0.0, 0.0)), |b| {
            b.lathe(&[(0.1, 0.0), (0.09, 0.45)], 6, 0.0)
        });
    }
    b.paint(Color::hex(0xe07a3f), 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.4), |b| {
        b.lathe(&[(0.24, 0.0), (0.3, 0.5), (0.22, 0.95)], 8, 0.0)
    });
    b.paint(Color::hex(0x5c4a6e), 0.0);
    b.with(Mat4::from_translation(Vec3::new(0.0, 0.26, 0.95)), |b| {
        b.cuboid(Vec3::new(0.36, 0.18, 0.5))
    });
    b.paint(palette::CREAM, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 1.32), |b| {
        b.lathe(
            &[
                (0.0, 0.0),
                (0.2, 0.04),
                (0.25, 0.22),
                (0.2, 0.42),
                (0.0, 0.48),
            ],
            10,
            0.0,
        )
    });
    b.paint(Color::hex(0x2e7d74), 0.3);
    b.with(Mat4::from_translation(Vec3::new(0.0, -0.2, 1.57)), |b| {
        b.cuboid(Vec3::new(0.3, 0.1, 0.14))
    });
    b.build()
}

/// The facing that turns the figure (made facing -Y) toward `dir`.
fn facing(dir: Vec2) -> f32 {
    dir.y.atan2(dir.x) + std::f32::consts::FRAC_PI_2
}

impl Walk {
    /// Seconds walking between the hub and a room's door.
    fn walking(&self, room: &str) -> f32 {
        self.ways.get(room).map(|w| w.length / SPEED).unwrap_or(0.0)
    }

    /// The station's part of a move to or from `room`: walking, then the door.
    fn outside(&self, room: &str) -> f32 {
        self.walking(room) + ENTER
    }

    /// The whole move's length: out of a room is half a dip and the walk home; into one
    /// is the walk out and a whole dip.
    fn time(&self) -> f32 {
        let room = |id: &str| self.ways.contains_key(id);
        let mut t = 0.0;
        if room(&self.from) {
            t += DIP * 0.5;
            if !room(&self.to) {
                t += self.outside(&self.from);
            }
        }
        if room(&self.to) {
            t += self.outside(&self.to) + DIP;
        }
        t.max(DIP)
    }

    /// The station with the figure `s` seconds along the way from the hub into `room`'s
    /// door (0 at rest at the hub).
    fn station(&self, stage: &Stage, room: Option<&str>, s: f32) -> Shown {
        let rest = shared::station_camera(stage);
        let Some(way) = room.and_then(|r| self.ways.get(r)) else {
            let mut shown = stage.exterior(None, Some(rest));
            self.draw(&mut shown, self.home, 0.0, 1.0, 0.0);
            return shown;
        };
        let walking = way.length / SPEED;
        let (home, door) = (self.home, way.door());
        // The camera comes down over the way, then closes on the door; lower than the
        // station's own view, so the figure reads as a person.
        let lean = WALK_TILT.to_radians();
        let tilt = Vec3::new(0.0, -lean.sin(), lean.cos());
        let over = framing(
            (home + door) * 0.5,
            tilt,
            (home.distance(door) * 0.55).max(22.0),
            20.0,
        );
        let close = framing(door, tilt, 16.0, 20.0);
        let cam = blend(&rest, &over, smooth(s / 0.45));
        let cam = blend(
            &cam,
            &close,
            smooth(span(s, walking - 0.15, walking + ENTER)),
        );
        let mut shown = stage.exterior(None, Some(cam));
        let u = span(s, 0.0, walking);
        // Mostly an even pace, easing off at each end.
        let along = 0.5 * u + 0.5 * smooth(u);
        let (on, heading) = way.at(way.length * along);
        let e = span(s, walking, walking + ENTER);
        let at = on + (way.module.with_z(door.z) - door) * 0.35 * e;
        let turn = facing(heading) * span(s, 0.0, 0.15);
        let stride = if u > 0.0 && u < 1.0 { s * 9.0 } else { 0.0 };
        self.draw(&mut shown, at, turn, 1.0 - e, stride);
        shown.scene.glows.push(Glow {
            position: door + Vec3::Z * 1.5,
            radius: 1.5 + 4.0 * e,
            color: Color::hex(0xffe2b0).with_alpha(0.25 + 0.6 * e),
        });
        shown
    }

    /// The figure at `at` (feet), turned by `turn`, `size` of its size, mid-stride.
    fn draw(&self, shown: &mut Shown, at: Vec3, turn: f32, size: f32, stride: f32) {
        let Some(mesh) = self.figure else { return };
        if size <= 0.01 {
            return;
        }
        let bob = stride.sin().abs() * 0.12 * SIZE;
        let sway = stride.sin() * 0.08;
        let t = Mat4::from_translation(at + Vec3::Z * bob)
            * Mat4::from_rotation_z(turn)
            * Mat4::from_rotation_y(sway)
            * Mat4::from_scale(Vec3::splat(SIZE * size));
        shown.scene.meshes.push(MeshInstance::new(mesh, t));
    }

    /// Where the move is: the view, and how dark the dip is.
    fn frame(&self, stage: &Stage) -> (Shown, f32) {
        let room = |id: &str| match shared::end(stage, id) {
            End::Room { id, .. } => Some(id.to_string()),
            End::Station => None,
        };
        let (a, b) = (room(&self.from), room(&self.to));
        let t = self.t;
        // Out of the room you were in: the dip, then out of its door and walking home.
        let mut t0 = 0.0;
        if let Some(a) = &a {
            if t < DIP * 0.5 {
                return (stage.view(a), smooth(t / (DIP * 0.5)));
            }
            let back = self.outside(a);
            let s = back - (t - DIP * 0.5);
            if s > 0.0 && b.is_none() {
                let dark = 1.0 - smooth(span(t, DIP * 0.5, DIP));
                return (self.station(stage, Some(a), s), dark);
            }
            t0 = if b.is_some() {
                DIP * 0.5
            } else {
                DIP * 0.5 + back
            };
        }
        // Into the room you are going to: walking out, in at its door, the dip.
        if let Some(b) = &b {
            let s = t - t0;
            let out = self.outside(b);
            if a.is_some() && s < DIP * 0.5 {
                // Room to room: from the dark straight onto the hub.
                return (
                    self.station(stage, Some(b), 0.0),
                    1.0 - smooth(s / (DIP * 0.5)),
                );
            }
            if s < out {
                return (
                    self.station(stage, Some(b), s),
                    smooth(span(s, out - 0.08, out)),
                );
            }
            let dark = 1.0 - smooth(span(s, out, out + DIP * 0.5));
            return (stage.view(b), dark);
        }
        (self.station(stage, None, 0.0), 0.0)
    }
}

impl Travel for Walk {
    fn name(&self) -> &'static str {
        "walk"
    }

    fn setup(&mut self, r: &mut Renderer, world: &World, model: &Places) {
        if let Some(m) = self.figure.take() {
            r.remove_mesh(m);
        }
        self.figure = Some(r.upload_mesh(&figure()));
        // The station's surface, to keep the figure's feet on it: the same meshes the
        // world draws, built again here to cast rays at.
        let recipe = &world.recipe;
        let mut meshes = vec![(kit::station(recipe), Mat4::IDENTITY)];
        for m in &world.modules {
            let Some(p) = model.place(&m.id) else {
                continue;
            };
            let Some(part) = p.art.as_deref().and_then(|a| kit::module_part(a, 1)) else {
                continue;
            };
            let strut = (m.pos().truncate().length() - CORE) / world::MODULE_SCALE;
            let mesh = kit::module(
                &part,
                recipe.palette,
                recipe.faction.clone(),
                strut,
                p.building,
            );
            meshes.push((mesh, m.at));
        }
        let deck = |p: Vec2, last: f32| surface(&meshes, p).unwrap_or(last);
        self.home = HOME.extend(deck(HOME, 0.0));
        self.ways.clear();
        for p in model.places.iter().filter(|p| p.kind == Kind::Interior) {
            let Some(m) = model.module_of(&p.id).and_then(|m| world.module(&m.id)) else {
                continue;
            };
            let pos = m.pos().truncate();
            let out = pos.normalize_or(Vec2::X);
            let door = pos - out * world::MODULE_REACH * world::MODULE_SCALE * 0.55;
            let corners = [HOME, out * (CORE + 1.0), door];
            let mut points = Vec::new();
            let mut last = self.home.z;
            for w in corners.windows(2) {
                let n = (w[0].distance(w[1]) / 0.5).ceil().max(1.0) as usize;
                for i in 0..n {
                    let q = w[0].lerp(w[1], i as f32 / n as f32);
                    last = deck(q, last);
                    points.push(q.extend(last));
                }
            }
            last = deck(door, last);
            points.push(door.extend(last));
            // Steps smoothed a little, so the figure climbs instead of jumping.
            let raw = points.clone();
            for (i, q) in points.iter_mut().enumerate() {
                let near = &raw[i.saturating_sub(2)..(i + 3).min(raw.len())];
                q.z = near.iter().map(|r| r.z).sum::<f32>() / near.len() as f32;
            }
            let length = corners.windows(2).map(|w| w[0].distance(w[1])).sum();
            self.ways.insert(
                p.id.clone(),
                Way {
                    module: m.pos(),
                    points,
                    length,
                },
            );
        }
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
            if self.t >= self.time() {
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
        if self.busy {
            return self.frame(stage).0;
        }
        match shared::end(stage, &stage.model.here) {
            End::Station => self.station(stage, None, 0.0),
            End::Room { .. } => stage.view(&stage.model.here),
        }
    }

    fn overlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if self.busy {
            let (_, dark) = self.frame(stage);
            shared::veil(layer, stage.viewport, DARK, dark);
        }
    }
}
