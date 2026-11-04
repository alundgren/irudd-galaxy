//! Effects a module asks for, drawn only: beams (`engine.beam`), particles
//! (`engine.emitter`, `engine.burst` and a beam's landing), halos round self-lit paint (a
//! look's `halo`), and camera feel (`engine.camera_feel`: `speed_zoom`, `lead`, `shake`).
//!
//! Nothing here writes the world. Every particle is worked out afresh each frame from
//! where its emitter was on the tick it was born, the tick, the entity and the world's
//! seed, so a replay, a film or an F8 note draws the very same sparks. The view keeps a
//! short history of where spraying things and the followed thing were on each tick
//! ([`Effects::record`], from [`crate::View::before_step`]), which every path that draws a
//! session (play, shot, film, audit, look) fills the same way by stepping the world.
//! How it works: `engine/docs/effects.md`.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use galaxy_api::engine::{BEAM, BURST, CAMERA_FEEL, EMITTER, FLIGHT, HELM, MODEL, MOTION};
use galaxy_mesh::gltf::PartRange;
use galaxy_mesh::{Color, MeshData};
use galaxy_render::effects::{Blend, ParticleRecipe};
use galaxy_render::glam::{Mat4, Quat, Vec2, Vec3};
use galaxy_render::{Camera, Glow, Line, MAX_PARTICLES, Particle, Scene};
use galaxy_sim::World;
use serde_json::{Map, Value};

use crate::{Pose, View, rooms};

/// Ticks a second; effect times are in ticks.
const TICKS: f32 = 30.0;
/// Ticks of history kept: the longest particle life, and a little over.
const KEEP: usize = (galaxy_render::effects::MAX_LIFE * TICKS) as usize + 3;
/// The camera's speed is measured over this many ticks, so a bump doesn't jolt it.
const SMOOTH_TICKS: f32 = 8.0;
/// The most halos one scene draws.
pub const MAX_HALOS: usize = 512;
/// The most lamps one model gives halos to.
const MAX_LAMPS: usize = 64;

/// Where a thing was on one tick, and how hard it was flying.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Sample {
    tick: u64,
    pose: Pose,
    velocity: Vec2,
    /// 0 to 1: pushing ahead, by the helm or by speeding up along its nose.
    thrust: f32,
    /// 0 to 1: its share of its top speed.
    speed: f32,
}

/// One thing's recent ticks, oldest first, and the place they were in.
#[derive(Clone, Debug, Default)]
struct Trail {
    place: String,
    samples: VecDeque<Sample>,
}

/// What the view keeps for effects between frames: only the history of poses, which the
/// world's ticks fill.
#[derive(Clone, Debug, Default)]
pub struct Effects {
    trails: BTreeMap<String, Trail>,
    /// What the last scene's effects found wrong (too many particles), as text.
    pub problems: Vec<String>,
}

/// A patch of self-lit paint: where a halo goes, in the model's own frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Lamp {
    /// The model part it is on, when that part moves on its own.
    pub part: Option<usize>,
    pub centre: Vec3,
    pub radius: f32,
    pub color: Color,
}

/// The patches of self-lit paint in `mesh`: each connected run of glowing triangles in a
/// part is one lamp, with its centre, size and average paint. At most [`MAX_LAMPS`].
pub fn lamps(mesh: &MeshData, parts: &[PartRange]) -> Vec<Lamp> {
    let whole = [PartRange {
        name: String::new(),
        vertices: 0..mesh.vertices.len(),
        motion: None,
    }];
    let parts = if parts.is_empty() { &whole[..] } else { parts };
    let mut out = Vec::new();
    for (index, part) in parts.iter().enumerate() {
        let tris: Vec<[usize; 3]> = mesh
            .indices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|t| t.map(|i| i as usize))
            .filter(|t| t.iter().all(|i| part.vertices.contains(i)))
            .filter(|t| t.iter().all(|&i| mesh.vertices[i].color[3] >= 0.5))
            .collect();
        // Join triangles that share a corner: union-find over quantised positions.
        let mut parent: Vec<usize> = (0..tris.len()).collect();
        fn root(parent: &mut [usize], mut i: usize) -> usize {
            while parent[i] != i {
                parent[i] = parent[parent[i]];
                i = parent[i];
            }
            i
        }
        let mut seen: BTreeMap<[i64; 3], usize> = BTreeMap::new();
        for (t, tri) in tris.iter().enumerate() {
            for &v in tri {
                let key = mesh.vertices[v]
                    .position
                    .map(|c| (c * 1000.0).round() as i64);
                match seen.get(&key) {
                    Some(&other) => {
                        let (a, b) = (root(&mut parent, t), root(&mut parent, other));
                        parent[a] = b;
                    }
                    None => {
                        seen.insert(key, t);
                    }
                }
            }
        }
        let mut islands: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for t in 0..tris.len() {
            let r = root(&mut parent, t);
            islands.entry(r).or_default().push(t);
        }
        for members in islands.values() {
            let verts: BTreeSet<usize> = members.iter().flat_map(|&t| tris[t]).collect();
            let n = verts.len() as f32;
            let centre = verts
                .iter()
                .map(|&v| Vec3::from(mesh.vertices[v].position))
                .sum::<Vec3>()
                / n;
            let radius = verts
                .iter()
                .map(|&v| (Vec3::from(mesh.vertices[v].position) - centre).length())
                .fold(0.0, f32::max)
                .max(0.05);
            let paint = verts
                .iter()
                .map(|&v| Vec3::from_slice(&mesh.vertices[v].color[..3]))
                .sum::<Vec3>()
                / n;
            out.push(Lamp {
                part: part.motion.is_some().then_some(index),
                centre,
                radius,
                color: Color::linear(paint.x, paint.y, paint.z, 1.0),
            });
            if out.len() >= MAX_LAMPS {
                return out;
            }
        }
    }
    out
}

/// A seeded stream of numbers from 0 to 1 (splitmix64): the same keys give the same
/// numbers on every run.
struct Dice(u64);

impl Dice {
    fn new(keys: &[u64]) -> Dice {
        Dice(keys.iter().fold(0x2545_f491_4f6c_dd1d, |h, k| mix(h ^ k)))
    }

    fn next(&mut self) -> f32 {
        self.0 = mix(self.0);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

fn mix(mut z: u64) -> u64 {
    z = z.wrapping_add(0x9e37_79b9_7f4a_7c15);
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// FNV-1a of a text, a stable key for IDs.
fn key(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

fn number(f: &Map<String, Value>, k: &str, or: f32) -> f32 {
    f.get(k)
        .and_then(Value::as_f64)
        .map(|v| v as f32)
        .filter(|v| v.is_finite())
        .unwrap_or(or)
}

fn text<'a>(f: &'a Map<String, Value>, k: &str) -> &'a str {
    f.get(k).and_then(Value::as_str).unwrap_or("")
}

/// The thing a spray or beam comes from: `target` if it names one, else the holder.
fn source<'a>(f: &'a Map<String, Value>, holder: &'a str) -> &'a str {
    Some(text(f, "target"))
        .filter(|t| !t.is_empty())
        .unwrap_or(holder)
}

/// `offset` (ahead, left, up) in the frame of a thing at `pose`.
fn at(pose: &Pose, offset: Vec3) -> Vec3 {
    pose.point() + Quat::from_rotation_z(pose.angle) * offset
}

fn smoothstep(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// A steady flicker from 0 to 1 at `rate` a second: two sines, out of step per thing.
fn flicker(seconds: f32, rate: f32, phase: f32) -> f32 {
    let t = seconds * rate * std::f32::consts::TAU;
    0.5 + 0.32 * (t + phase).sin() + 0.18 * (t * 2.37 + phase * 1.7).sin()
}

impl Effects {
    /// Remembers where each thing that sprays, and the followed thing, is on the world's
    /// tick. Called once before each step, like [`crate::View::before_step`].
    pub(crate) fn record(&mut self, view: &View, world: &World) {
        let mut wanted = BTreeSet::new();
        for (id, c) in &world.entities {
            for kind in [EMITTER, BURST] {
                if let Some(f) = c.get(kind).map(|c| &c.fields) {
                    wanted.insert(source(f, id).to_owned());
                }
            }
        }
        if let Some((id, _)) = crate::camera_holder(world) {
            wanted.insert(id.to_owned());
        }
        self.trails.retain(|id, _| wanted.contains(id));
        for id in wanted {
            let place = galaxy_sim::places::place_of(world, &id).to_owned();
            let trail = self.trails.entry(id.clone()).or_default();
            if trail.place != place {
                trail.samples.clear();
                trail.place = place;
            }
            match trail.samples.back() {
                Some(last) if last.tick + 1 == world.tick => {}
                Some(last) if last.tick == world.tick => {
                    trail.samples.pop_back();
                }
                _ => trail.samples.clear(),
            }
            let Some(sample) = sampled(view, world, &id, trail.samples.back()) else {
                trail.samples.clear();
                continue;
            };
            trail.samples.push_back(sample);
            while trail.samples.len() > KEEP {
                trail.samples.pop_front();
            }
        }
    }

    /// `id`'s recorded ticks and its pose on the world's own tick, oldest first.
    fn samples(&self, view: &View, world: &World, id: &str) -> Vec<Sample> {
        let mut out: Vec<Sample> = self
            .trails
            .get(id)
            .filter(|t| t.place == galaxy_sim::places::place_of(world, id))
            .map(|t| {
                t.samples
                    .iter()
                    .copied()
                    .filter(|s| s.tick < world.tick)
                    .collect()
            })
            .unwrap_or_default();
        if let Some(now) = sampled(view, world, id, out.last()) {
            out.push(now);
        }
        out
    }
}

/// Where `id` is on the world's tick, and how hard it flies, after `before`.
fn sampled(view: &View, world: &World, id: &str, before: Option<&Sample>) -> Option<Sample> {
    let pose = view.pose(world, id, 1.0)?;
    let velocity = world.component(id, MOTION).map_or(Vec2::ZERO, |m| {
        Vec2::new(number(&m.fields, "vx", 0.0), number(&m.fields, "vy", 0.0))
    });
    let flight = world.component(id, FLIGHT).map(|c| &c.fields);
    let top = flight.map_or(14.0, |f| number(f, "speed", 14.0)).max(1e-3);
    let accel = flight.map_or(21.0, |f| number(f, "accel", 21.0)).max(1e-3);
    let helm = world
        .component(id, HELM)
        .map_or(0.0, |h| number(&h.fields, "thrust", 0.0));
    let nose = Vec2::from_angle(pose.angle);
    let pushed = before
        .filter(|b| b.tick + 1 == world.tick)
        .map_or(0.0, |b| (velocity - b.velocity).dot(nose) * TICKS / accel);
    Some(Sample {
        tick: world.tick,
        pose,
        velocity,
        thrust: helm.max(pushed).clamp(0.0, 1.0),
        speed: (velocity.length() / top).clamp(0.0, 1.0),
    })
}

/// The pose and velocity at `tick` (fractional), between the samples round it; held at
/// either end.
fn sample_at(samples: &[Sample], tick: f32) -> Option<(Pose, Vec2)> {
    let first = samples.first()?;
    let last = samples.last()?;
    if tick <= first.tick as f32 {
        return Some((first.pose, first.velocity));
    }
    if tick >= last.tick as f32 {
        return Some((last.pose, last.velocity));
    }
    let i = samples.partition_point(|s| (s.tick as f32) <= tick);
    let (a, b) = (samples[i - 1], samples[i]);
    let t = (tick - a.tick as f32) / (b.tick - a.tick).max(1) as f32;
    Some((a.pose.lerp(b.pose, t), a.velocity.lerp(b.velocity, t)))
}

/// One spray's particles on screen this frame.
struct Spray<'a> {
    recipe: &'a ParticleRecipe,
    /// Keys the dice: world seed, thing, recipe, kind.
    keys: [u64; 4],
}

impl Spray<'_> {
    /// The particle born at tick `born` from `from`, flying `facing` radians (before the
    /// recipe's direction and spread) with its emitter moving at `carried`, drawn `now`
    /// (ticks). None once it has died.
    fn particle(
        &self,
        dice: &mut Dice,
        born: f32,
        now: f32,
        from: Vec3,
        facing: f32,
        carried: Vec3,
    ) -> Option<Particle> {
        let r = self.recipe;
        let life = r.life * (1.0 - r.life_jitter * dice.next());
        let turn = (r.direction + r.spread * (dice.next() - 0.5)).to_radians();
        let speed = r.speed * (1.0 - r.speed_jitter * dice.next());
        let spin = dice.next() * std::f32::consts::TAU;
        let twirl = (dice.next() - 0.5) * 12.0;
        let age = (now - born) / TICKS;
        if !(0.0..life).contains(&age) {
            return None;
        }
        let (s, c) = (facing + turn).sin_cos();
        let v0 = Vec3::new(c, s, 0.0) * speed + carried * r.inherit;
        // Drag slows it smoothly: v = v0 e^(-k t), so it travels v0 (1 - e^(-k t)) / k.
        let k = r.drag;
        let (gone, slowed) = if k > 1e-4 {
            ((1.0 - (-k * age).exp()) / k, (-k * age).exp())
        } else {
            (age, 1.0)
        };
        let f = age / life;
        let color = r.color.lerp(r.end_color, f);
        Some(Particle {
            position: from + v0 * gone,
            velocity: v0 * slowed,
            size: r.size + (r.end_size - r.size) * f,
            color: color.with_alpha(color.a * (1.0 - f * f)),
            shape: r.shape,
            additive: r.blend == Blend::Add,
            spin: spin + twirl * age,
        })
    }

    /// What a steady spray from `samples` shows at `now`: on each tick it makes `rate`
    /// a second times `scale` of that tick's sample, each born at its own moment in the
    /// tick, where its emitter was then. `place(pose)` gives where on the thing it
    /// leaves from and which way.
    fn steady(
        &self,
        samples: &[Sample],
        now: f32,
        rate: f32,
        scale: impl Fn(&Sample) -> f32,
        place: impl Fn(&Pose) -> (Vec3, f32),
        out: &mut Vec<Particle>,
    ) {
        let r = self.recipe;
        let rate = (rate * r.rate).clamp(0.0, galaxy_render::effects::MAX_RATE);
        if rate <= 0.0 || r.life <= 0.0 {
            return;
        }
        let oldest = (now - r.life * TICKS).floor() as i64 - 1;
        for tick in oldest.max(0)..=now.floor() as i64 {
            let Some(here) = samples
                .iter()
                .rev()
                .find(|s| s.tick as i64 <= tick)
                .or(samples.first())
            else {
                return;
            };
            let due = rate * scale(here).clamp(0.0, 1.0) / TICKS;
            let mut dice = Dice::new(&[
                self.keys[0],
                self.keys[1],
                self.keys[2],
                self.keys[3],
                tick as u64,
            ]);
            let count = due.floor() as u32 + u32::from(dice.next() < due.fract());
            for _ in 0..count {
                let born = tick as f32 + dice.next();
                let mut each = Dice(dice.0 ^ 0x5851_f42d_4c95_7f2d);
                dice.next();
                if born > now {
                    continue;
                }
                let Some((pose, velocity)) = sample_at(samples, born) else {
                    continue;
                };
                let (from, facing) = place(&pose);
                if let Some(p) =
                    self.particle(&mut each, born, now, from, facing, velocity.extend(0.0))
                {
                    out.push(p);
                }
            }
        }
    }
}

impl View {
    /// Beams, sprays and halos for this frame, into `scene`; `drawn` maps each drawn
    /// thing to its model and scale.
    pub(crate) fn draw_effects(
        &mut self,
        world: &World,
        alpha: f32,
        place: Option<&str>,
        drawn: &BTreeMap<&str, (std::ops::Range<usize>, &str, f32)>,
        scene: &mut Scene,
    ) {
        let now = world.tick as f32 - 1.0 + alpha;
        let seconds = now.max(0.0) / TICKS;
        let mut particles = Vec::new();
        let mut missing = Vec::new();
        for (id, c) in &world.entities {
            if let Some(f) = c.get(BEAM).map(|c| &c.fields) {
                self.beam(
                    world,
                    alpha,
                    place,
                    id,
                    f,
                    now,
                    scene,
                    &mut particles,
                    &mut missing,
                );
            }
            if let Some(f) = c.get(EMITTER).map(|c| &c.fields) {
                self.emitter(world, place, id, f, now, &mut particles, &mut missing);
            }
            if let Some(f) = c.get(BURST).map(|c| &c.fields) {
                self.burst(world, place, id, f, now, &mut particles, &mut missing);
            }
        }
        self.halos(world, alpha, drawn, seconds, scene);
        self.missing.extend(missing);
        self.effects.problems.clear();
        if particles.len() > MAX_PARTICLES {
            self.effects.problems.push(format!(
                "more particles at once than the {MAX_PARTICLES} a scene draws, so the rest \
                 are not drawn (lower a recipe's rate, count or life)"
            ));
            particles.truncate(MAX_PARTICLES);
        }
        scene.particles = particles;
    }

    /// A recipe by ID, or a missing-recipe note.
    fn recipe(&self, id: &str, user: &str, missing: &mut Vec<String>) -> Option<ParticleRecipe> {
        if id.is_empty() {
            return None;
        }
        let found = self.assets.particles.get(id).copied();
        if found.is_none() {
            missing.push(format!("particle recipe `{id}` ({user} sprays it)"));
        }
        found
    }

    #[allow(clippy::too_many_arguments)]
    fn beam(
        &self,
        world: &World,
        alpha: f32,
        place: Option<&str>,
        id: &str,
        f: &Map<String, Value>,
        now: f32,
        scene: &mut Scene,
        particles: &mut Vec<Particle>,
        missing: &mut Vec<String>,
    ) {
        let target = text(f, "target");
        if target.is_empty()
            || !world.entities.contains_key(target)
            || !rooms::visible(world, id, place)
            || !rooms::visible(world, target, place)
        {
            return;
        }
        let look = text(f, "look");
        let style = if look.is_empty() {
            galaxy_render::effects::BeamStyle::default()
        } else if let Some(s) = self.assets.beams.get(look) {
            s.clone()
        } else {
            missing.push(format!("beam look `{look}` ({id} wears it)"));
            return;
        };
        let (Some(pose), Some(aim)) =
            (self.pose(world, id, alpha), self.pose(world, target, alpha))
        else {
            return;
        };
        let from = at(
            &pose,
            Vec3::new(
                number(f, "x", 0.0),
                number(f, "y", 0.0),
                number(f, "height", 0.0),
            ),
        );
        let to = self.surface_point(world, target, &aim, alpha, from);
        if (to - from).length() < 1e-3 {
            return;
        }
        let seconds = now.max(0.0) / TICKS;
        let phase = (key(id) % 6283) as f32 / 1000.0;
        let bright = 1.0 - style.flicker * flicker(seconds, style.flicker_rate, phase);
        scene.lines.push(
            Line::beam(from, to, style.width, style.color.with_alpha(bright)).with_glow(style.glow),
        );
        if style.glow > 0.0 && style.glow_size > 0.0 {
            let hot = style.color.lerp(Color::WHITE, 0.55);
            scene.glows.push(Glow {
                position: to,
                radius: style.glow_size,
                color: hot.with_alpha((0.9 * style.glow * bright).min(2.0)),
            });
            scene.glows.push(Glow {
                position: from,
                radius: style.glow_size * 0.5,
                color: hot.with_alpha((0.7 * style.glow * bright).min(2.0)),
            });
        }
        // Sparks and chips where it lands, thrown back up the beam.
        for name in &style.impact {
            let Some(recipe) = self.recipe(name, id, missing) else {
                continue;
            };
            let spray = Spray {
                recipe: &recipe,
                keys: [self.seed, key(id), key(name), 1],
            };
            let back = (from - to).truncate();
            let facing = back.y.atan2(back.x);
            let still = [Sample {
                tick: world.tick,
                pose: aim,
                velocity: Vec2::ZERO,
                thrust: 1.0,
                speed: 1.0,
            }];
            spray.steady(&still, now, 1.0, |_| 1.0, |_| (to, facing), particles);
        }
    }

    /// Where a line from `from` towards the middle of `target` (drawn at `pose`) meets
    /// its drawn surface; its outline's reach when it has no model or the line misses.
    fn surface_point(
        &self,
        world: &World,
        target: &str,
        pose: &Pose,
        alpha: f32,
        from: Vec3,
    ) -> Vec3 {
        let centre = pose.point();
        let Some(model) = world.component(target, MODEL).map(|c| &c.fields) else {
            return centre;
        };
        let scale = number(model, "scale", 1.0);
        let scale = if scale > 0.0 { scale } else { 1.0 };
        let transform = Mat4::from_scale_rotation_translation(
            Vec3::splat(scale),
            Quat::from_rotation_z(pose.angle),
            centre,
        );
        let mesh = self.assets.models.get(text(model, "model"));
        let middle = mesh.and_then(MeshData::bounds).map_or(centre, |(lo, hi)| {
            transform.transform_point3((lo + hi) * 0.5)
        });
        let dir = middle - from;
        if let Some(mesh) = mesh {
            let morph = self.morph(world, target, alpha);
            let inverse = transform.inverse();
            let (o, d) = (
                inverse.transform_point3(from),
                inverse.transform_vector3(dir),
            );
            let hit = if morph > 0.0 {
                mesh.morphed(morph).raycast(o, d)
            } else {
                mesh.raycast(o, d)
            };
            if let Some(t) = hit.filter(|t| (0.0..=1.0).contains(t)) {
                return from + dir * t;
            }
        }
        let reach = self.reach(world, target).unwrap_or(0.0);
        let flat = dir.truncate().normalize_or_zero();
        middle - (flat * reach).extend(0.0)
    }

    #[allow(clippy::too_many_arguments)]
    fn emitter(
        &self,
        world: &World,
        place: Option<&str>,
        id: &str,
        f: &Map<String, Value>,
        now: f32,
        particles: &mut Vec<Particle>,
        missing: &mut Vec<String>,
    ) {
        let name = text(f, "recipe");
        let Some(recipe) = self.recipe(name, id, missing) else {
            return;
        };
        let from = source(f, id);
        if !rooms::visible(world, from, place) {
            return;
        }
        let samples = self.effects.samples(self, world, from);
        let offset = Vec3::new(
            number(f, "x", 0.0),
            number(f, "y", 0.0),
            number(f, "height", 0.0),
        );
        let turn = number(f, "angle", 0.0).to_radians();
        let by = text(f, "by");
        let spray = Spray {
            recipe: &recipe,
            keys: [self.seed, key(id), key(name), 2],
        };
        spray.steady(
            &samples,
            now,
            number(f, "rate", 1.0).max(0.0),
            |s| match by {
                "thrust" => s.thrust,
                "speed" => s.speed,
                _ => 1.0,
            },
            |pose| (at(pose, offset), pose.angle + turn),
            particles,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn burst(
        &self,
        world: &World,
        place: Option<&str>,
        id: &str,
        f: &Map<String, Value>,
        now: f32,
        particles: &mut Vec<Particle>,
        missing: &mut Vec<String>,
    ) {
        let tick = f.get("tick").and_then(Value::as_i64).unwrap_or(0);
        let name = text(f, "recipe");
        if tick <= 0 {
            return;
        }
        let Some(recipe) = self.recipe(name, id, missing) else {
            return;
        };
        // It shows from the frame its tick is drawn into.
        let born = tick as f32 - 1.0;
        if now < born || now - born > recipe.life * TICKS {
            return;
        }
        let from = source(f, id);
        if !rooms::visible(world, from, place) {
            return;
        }
        let samples = self.effects.samples(self, world, from);
        let Some((pose, velocity)) = sample_at(&samples, born) else {
            return;
        };
        let offset = Vec3::new(
            number(f, "x", 0.0),
            number(f, "y", 0.0),
            number(f, "height", 0.0),
        );
        let facing = pose.angle + number(f, "angle", 0.0).to_radians();
        let spray = Spray {
            recipe: &recipe,
            keys: [self.seed, key(id), key(name), 3],
        };
        let mut dice = Dice::new(&[self.seed, key(id), key(name), tick as u64]);
        for _ in 0..recipe.count.min(galaxy_render::effects::MAX_COUNT) {
            let mut each = Dice(dice.0 ^ 0x5851_f42d_4c95_7f2d);
            dice.next();
            if let Some(p) = spray.particle(
                &mut each,
                born,
                now,
                at(&pose, offset),
                facing,
                velocity.extend(0.0),
            ) {
                particles.push(p);
            }
        }
    }

    /// A soft halo round each lamp on a drawn thing whose look asks for one.
    fn halos(
        &self,
        world: &World,
        alpha: f32,
        drawn: &BTreeMap<&str, (std::ops::Range<usize>, &str, f32)>,
        seconds: f32,
        scene: &mut Scene,
    ) {
        let mut halos = 0;
        for (id, (_, model, scale)) in drawn {
            let Some(look) = world
                .component(id, MODEL)
                .map(|c| text(&c.fields, "look"))
                .and_then(|l| self.assets.looks.get(l))
                .filter(|l| l.halo > 0.0)
            else {
                continue;
            };
            let Some(lamps) = self.assets.lamps.get(*model) else {
                continue;
            };
            let Some(pose) = self.pose(world, id, alpha) else {
                continue;
            };
            let transform = Mat4::from_scale_rotation_translation(
                Vec3::splat(*scale),
                Quat::from_rotation_z(pose.angle),
                pose.point(),
            );
            let parts = self.assets.parts.get(*model);
            for lamp in lamps {
                if halos >= MAX_HALOS {
                    return;
                }
                let motion = lamp
                    .part
                    .and_then(|i| parts?.get(i)?.motion)
                    .map(|m| m.at_time(f64::from(seconds)));
                let (turn, share) = motion.unwrap_or((Mat4::IDENTITY, 1.0));
                scene.glows.push(Glow {
                    position: (transform * turn).transform_point3(lamp.centre),
                    radius: lamp.radius * scale * look.halo_size,
                    color: lamp.color.with_alpha(0.5 * look.halo * share),
                });
                halos += 1;
            }
        }
    }

    /// Camera feel for the thing at `holder` drawn at `alpha`: how much wider the view
    /// gets, and how far the camera's aim moves (lead and shake). Nothing with reduced
    /// motion.
    pub(crate) fn camera_feel(
        &self,
        world: &World,
        holder: &str,
        alpha: f32,
        view: f32,
    ) -> (f32, Vec3) {
        if self.eye.still {
            return (1.0, Vec3::ZERO);
        }
        let Some(cam) = world.component(holder, CAMERA_FEEL).map(|c| &c.fields) else {
            return (1.0, Vec3::ZERO);
        };
        let speed_zoom = number(cam, "speed_zoom", 0.0).clamp(0.0, 1.0);
        let lead = number(cam, "lead", 0.0).clamp(0.0, 2.0);
        let shake = number(cam, "shake", 0.0).clamp(0.0, 0.2);
        let now = world.tick as f32 - 1.0 + alpha;
        let mut zoom = 1.0;
        let mut aim = Vec3::ZERO;
        if speed_zoom > 0.0 || lead > 0.0 {
            let samples = self.effects.samples(self, world, holder);
            let velocity = smoothed_velocity(&samples, now);
            let top = world
                .component(holder, FLIGHT)
                .map_or(14.0, |f| number(&f.fields, "speed", 14.0))
                .max(1e-3);
            zoom += speed_zoom * smoothstep(velocity.length() / top);
            let ahead = velocity * lead;
            aim += ahead.clamp_length_max(view * 0.3).extend(0.0);
        }
        let start = cam.get("shake_tick").and_then(Value::as_i64).unwrap_or(0);
        let length = number(cam, "shake_seconds", 0.4).clamp(0.0, 2.0);
        if shake > 0.0 && start > 0 && length > 0.0 {
            let age = (now - (start as f32 - 1.0)) / TICKS;
            if (0.0..length).contains(&age) {
                let fade = (1.0 - age / length).powi(2);
                let mut dice = Dice::new(&[self.seed, key(holder), start as u64]);
                let (p1, p2, p3) = (dice.next(), dice.next(), dice.next());
                let tau = std::f32::consts::TAU;
                let wave = |p: f32, q: f32| {
                    0.7 * (tau * (6.0 * age + p)).cos() + 0.3 * (tau * (11.0 * age + q)).sin()
                };
                // The first instant is a kick one way; then it shakes and dies away.
                let offset = Vec2::new(wave(p1, p2), wave(p3, p1)) * shake * view * 0.5 * fade;
                aim += offset.extend(0.0);
            }
        }
        (zoom, aim)
    }
}

/// The holder's velocity over the last [`SMOOTH_TICKS`] at `now` (ticks): continuous
/// between ticks, and calm through a single tick's bump.
fn smoothed_velocity(samples: &[Sample], now: f32) -> Vec2 {
    let Some(first) = samples.first() else {
        return Vec2::ZERO;
    };
    let then = (now - SMOOTH_TICKS).max(first.tick as f32);
    let span = now - then;
    if span < 1.0 {
        return Vec2::ZERO;
    }
    let (Some((a, _)), Some((b, _))) = (sample_at(samples, then), sample_at(samples, now)) else {
        return Vec2::ZERO;
    };
    Vec2::new(b.x - a.x, b.y - a.y) * TICKS / span
}

/// Moves a camera's eye and aim together by `by`, keeping its framing.
pub(crate) fn shifted(mut camera: Camera, by: Vec3) -> Camera {
    camera.eye += by;
    camera.target += by;
    camera
}

#[cfg(test)]
mod tests {
    use super::*;
    use galaxy_mesh::MeshBuilder;

    #[test]
    fn dice_repeat_for_the_same_keys() {
        let mut a = Dice::new(&[1, 2, 3]);
        let mut b = Dice::new(&[1, 2, 3]);
        let mut c = Dice::new(&[1, 2, 4]);
        let (x, y, z) = (a.next(), b.next(), c.next());
        assert_eq!(x, y);
        assert_ne!(x, z);
        assert!((0.0..1.0).contains(&x));
    }

    #[test]
    fn each_lit_patch_is_one_lamp() {
        let mut b = MeshBuilder::new();
        b.paint(Color::hex(0x806040), 0.0);
        b.cuboid(Vec3::ONE);
        let hull = b.build();
        let mut b = MeshBuilder::new();
        b.paint(Color::hex(0xffd27a), 1.0);
        b.cuboid(Vec3::splat(0.2));
        let lit = b.build();
        let mut mesh = hull.clone();
        for x in [-2.0f32, 2.0] {
            let base = mesh.vertices.len() as u32;
            mesh.vertices.extend(lit.vertices.iter().map(|v| {
                let mut v = *v;
                v.position[0] += x;
                v
            }));
            mesh.indices.extend(lit.indices.iter().map(|i| i + base));
        }
        let found = lamps(&mesh, &[]);
        assert_eq!(found.len(), 2, "{found:?}");
        assert!((found[0].centre.x.abs() - 2.0).abs() < 1e-4);
        assert!(found[0].radius > 0.1 && found[0].radius < 0.3);
        assert!(found.iter().all(|l| l.part.is_none()));
    }

    #[test]
    fn velocity_is_smoothed_and_held_still_without_history() {
        let s = |tick: u64, x: f32| Sample {
            tick,
            pose: Pose {
                x,
                y: 0.0,
                height: 0.0,
                angle: 0.0,
            },
            velocity: Vec2::ZERO,
            thrust: 0.0,
            speed: 0.0,
        };
        assert_eq!(smoothed_velocity(&[s(5, 0.0)], 5.0), Vec2::ZERO);
        // One unit a tick is 30 a second, whichever fraction of a tick it is drawn at.
        let line: Vec<Sample> = (0..20).map(|t| s(t, t as f32)).collect();
        for now in [12.0, 12.5, 19.0] {
            let v = smoothed_velocity(&line, now);
            assert!((v.x - 30.0).abs() < 1e-3, "{now}: {v}");
        }
    }
}
