//! World to scene: the sim's plain data drawn with the render kit.
//!
//! World units map one to one onto render units on the `z = 0` plane. The kit's meshes
//! are modelled small (a Wren is 6 long), so ships and stations are scaled up to the
//! sim's distances: a station is fitted inside its docking radius, with its kit ring
//! on the sim's ring, and a ship is a few times its kit size.
//!
//! [`View`] keeps the uploaded meshes and re-meshes what changes: a rock when its
//! `remaining` crosses one of [`ROCK_STEPS`] visible steps, a station when its build
//! stage moves (Kettle Dock's greenhouse), a hull the first time it's seen.

mod live;
pub mod map;
pub mod recipes;

use render::art_bible::ArtMaterial;
use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use render::glam::{Mat4, Quat, Vec2 as G2, Vec3};
use render::kit::{self, RockParams, StationLight, StationRecipe};
use render::{
    ArtBible, BackdropHandle, Camera, Color, Glow, InkRole, Line, MAX_POINT_LIGHTS, MeshHandle,
    MeshInstance, PointLight, Renderer, Scene, palette,
};

use crate::session::Pose;

/// Rocks are re-meshed when `remaining / initial` crosses one of this many steps.
pub const ROCK_STEPS: f32 = 20.0;
/// Ships are drawn this many times their kit size (a Wren is about 30 units long).
pub const SHIP_SCALE: f32 = 5.0;
/// Ships fly this high above the plane, so they pass over rocks rather than through.
pub const SHIP_HOVER: f32 = 22.0;
/// The camera's gentle tilt, degrees: top-down with a little depth.
pub const TILT: f32 = 14.0;
/// A station's kit docking ring is drawn this share of the sim's docking radius...
const RING_FILL: f32 = 0.9;
/// ...unless that makes the whole station wider than this many docking radii.
const STATION_MAX: f32 = 2.2;

/// Contract to render vector.
pub fn g(v: Vec2) -> G2 {
    G2::new(v.x, v.y)
}

/// A stable small hash of an ID, for seeds and spins.
pub fn seed_of(id: &str) -> u64 {
    id.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// The follow camera: where it looks and how much it sees.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraRig {
    pub center: G2,
    /// Visible world height.
    pub zoom: f32,
}

impl CameraRig {
    pub const MIN_ZOOM: f32 = 220.0;
    pub const MAX_ZOOM: f32 = 1800.0;
    pub const DEFAULT_ZOOM: f32 = 440.0;

    pub fn new(center: G2) -> Self {
        Self {
            center,
            zoom: Self::DEFAULT_ZOOM,
        }
    }

    pub fn camera(&self) -> Camera {
        Camera::top_down(self.center, self.zoom).with_tilt(TILT)
    }

    /// Eases towards `target`: quick enough to keep the ship in view, soft enough not
    /// to jitter. `dt` is real seconds since the last frame.
    pub fn follow(&mut self, target: G2, dt: f32) {
        let k = 1.0 - (-dt.max(0.0) * 5.0).exp();
        self.center = self.center.lerp(target, k);
        // Never let the ship leave the middle of the screen, whatever the frame rate.
        let max = self.zoom * 0.3;
        let off = self.center - target;
        if off.length() > max {
            self.center = target + off.normalize() * max;
        }
    }

    /// Mouse wheel: positive steps zoom in.
    pub fn zoom_by(&mut self, steps: f32) {
        self.zoom = (self.zoom * 0.88f32.powf(steps)).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
    }
}

struct StationArt {
    mesh: MeshHandle,
    stage: u32,
    /// Station space to world.
    transform: Mat4,
    lights: Vec<StationLight>,
}

struct RockArt {
    mesh: MeshHandle,
    step: i32,
}

/// Uploaded meshes and backdrops for the world.
#[derive(Default)]
pub struct View {
    hulls: BTreeMap<(HullId, Option<FactionId>), MeshHandle>,
    stations: BTreeMap<StationId, StationArt>,
    rocks: BTreeMap<RockId, RockArt>,
    backdrops: BTreeMap<Option<RegionId>, BackdropHandle>,
}

fn rock_step(rock: &Rock) -> i32 {
    let frac = rock.remaining as f32 / rock.initial.max(1) as f32;
    (frac.clamp(0.0, 1.0) * ROCK_STEPS).ceil() as i32
}

/// A rock's radius when full; the kit shrinks it with `remaining`.
fn rock_full_radius(world: &WorldState, rock: &Rock) -> f32 {
    let m = &world.ship_rules.mining;
    m.radius_base + m.radius_per_sqrt_unit * (rock.initial as f32).sqrt()
}

fn rock_params(world: &WorldState, rock: &Rock) -> RockParams {
    let frac = rock.remaining as f32 / rock.initial.max(1) as f32;
    RockParams::new(
        recipes::rock_kind(world, &rock.rock_type),
        rock_full_radius(world, rock),
        frac,
    )
}

/// The faction a ship flies for, for its paint.
fn ship_faction(world: &WorldState, ship: &Ship) -> Option<FactionId> {
    match &ship.owner {
        Owner::Player => None,
        Owner::Npc { npc } => world.npcs.get(npc).and_then(|n| n.faction.clone()),
        Owner::Faction { faction } => Some(faction.clone()),
    }
}

fn paint_key(world: &WorldState, ship: &Ship) -> (HullId, Option<FactionId>) {
    let faction = match ship.owner {
        Owner::Player => None,
        _ => Some(
            ship_faction(world, ship).unwrap_or_else(|| FactionId::from_static("faction.none")),
        ),
    };
    (ship.hull.clone(), faction)
}

/// The radius of the recipe's first docking ring, in station space.
fn ring_radius(recipe: &StationRecipe) -> Option<f32> {
    recipe.parts.iter().find_map(|p| match p.kind {
        kit::PartKind::DockingRing { radius, .. } => Some(radius * p.at.scale),
        _ => None,
    })
}

/// Station space to world: the kit's ring sits on the sim's ring (where you dock), as
/// large as fits.
pub fn station_transform(station: &Station, recipe: &StationRecipe) -> Mat4 {
    let fit = station.dock_radius * STATION_MAX / recipe.footprint().max(1.0);
    let scale = ring_radius(recipe).map_or(fit, |r| {
        (station.dock_radius * RING_FILL / r.max(0.5)).min(fit)
    });
    // The kit's docking ring sits on the sim's ring.
    let ring = kit::docking_ring_center(recipe).unwrap_or(G2::ZERO) * scale;
    let pos = g(station.position) - ring;
    Mat4::from_scale_rotation_translation(Vec3::splat(scale), Quat::IDENTITY, pos.extend(0.0))
}

impl View {
    pub fn new() -> Self {
        Self::default()
    }

    /// Uploads what's new and re-meshes what changed visibly.
    pub fn sync(&mut self, r: &mut Renderer, world: &WorldState) {
        for s in world.stations.values() {
            let stale = self
                .stations
                .get(&s.id)
                .is_none_or(|a| a.stage != s.build.stage);
            if !stale {
                continue;
            }
            let recipe = world
                .station_recipes
                .get(&s.id)
                .cloned()
                .unwrap_or_else(|| recipes::station_recipe(s));
            let data = kit::station(&recipe);
            let mesh = match self.stations.get(&s.id) {
                Some(a) => {
                    r.update_mesh(a.mesh, &data);
                    a.mesh
                }
                None => r.upload_mesh(&data),
            };
            self.stations.insert(
                s.id.clone(),
                StationArt {
                    mesh,
                    stage: s.build.stage,
                    transform: station_transform(s, &recipe),
                    lights: kit::station_lights(&recipe),
                },
            );
        }
        for rock in world.rocks.values() {
            let step = rock_step(rock);
            match self.rocks.get_mut(&rock.id) {
                Some(a) if a.step == step => {}
                Some(a) => {
                    a.step = step;
                    r.update_mesh(
                        a.mesh,
                        &kit::rock(&rock_params(world, rock), seed_of(rock.id.as_str())),
                    );
                }
                None => {
                    let mesh = r.upload_mesh(&kit::rock(
                        &rock_params(world, rock),
                        seed_of(rock.id.as_str()),
                    ));
                    self.rocks.insert(rock.id.clone(), RockArt { mesh, step });
                }
            }
        }
        for ship in world.ships.values() {
            let key = paint_key(world, ship);
            self.hulls.entry(key).or_insert_with_key(|(hull, faction)| {
                let params = match faction {
                    None => recipes::hull_params(hull),
                    Some(f) => recipes::npc_hull_params(hull, Some(f)),
                };
                r.upload_mesh(&kit::hull(&params, seed_of(hull.as_str())))
            });
        }
        let mut regions: Vec<Option<RegionId>> = vec![None];
        regions.extend(world.regions.keys().cloned().map(Some));
        for region in regions {
            self.backdrops.entry(region).or_insert_with_key(|region| {
                let seed = region.as_ref().map_or(7, |r| seed_of(r.as_str()));
                r.upload_backdrop(&kit::backdrop(&recipes::backdrop(region.as_ref()), seed))
            });
        }
    }

    /// The backdrop for the region around `at`.
    pub fn backdrop_at(&self, world: &WorldState, at: Vec2) -> Option<BackdropHandle> {
        let region = galaxy_ship::space::region_at(world, at).cloned();
        self.backdrops
            .get(&region)
            .or_else(|| self.backdrops.get(&None))
            .copied()
    }

    /// The world as the player sees it: stations, lanes, rocks and ships, interpolated
    /// to `poses`. `time` is presentation seconds for gentle animation only.
    pub fn world_scene(
        &self,
        world: &WorldState,
        poses: &BTreeMap<ShipId, Pose>,
        rig: &CameraRig,
        time: f32,
        viewport: G2,
    ) -> Scene {
        let camera = rig.camera();
        let mut scene = Scene::new(camera);
        crate::view::live::draw(&mut scene, world);
        scene.time = time;
        scene.backdrop = self.backdrop_at(world, Vec2::new(rig.center.x, rig.center.y));
        let aspect = viewport.x / viewport.y.max(1.0);
        let reach = 0.5 * rig.zoom * (1.0 + aspect * aspect).sqrt() + 160.0;
        let near = |p: G2, r: f32| p.distance(rig.center) <= reach + r;
        let flagship = world.player.ship.as_ref().and_then(|s| world.ships.get(s));

        // Region edges: faint, so you notice crossing into somewhere new.
        for region in world.regions.values() {
            let c = g(region.center);
            let d = c.distance(rig.center);
            if (d - region.radius).abs() > reach {
                continue;
            }
            let color = region
                .owner
                .as_ref()
                .map_or(palette::BLOSSOM, recipes::faction_color);
            scene.lines.push(
                Line::circle(c.extend(-30.0), region.radius, 5.0, color.with_alpha(0.22))
                    .dashed(40.0, 28.0)
                    .wobbly(6.0),
            );
        }

        // Lanes: a line of old lamps between the two mouths.
        let mouth = world.ship_rules.lanes.mouth_radius;
        for lane in world.lanes.values() {
            let (a, b) = (g(lane.a_gate), g(lane.b_gate));
            let dir = (b - a).normalize_or_zero();
            scene.lines.push(
                Line::path(
                    vec![
                        (a + dir * mouth).extend(-8.0),
                        (b - dir * mouth).extend(-8.0),
                    ],
                    4.0,
                    palette::MUSTARD.with_alpha(0.55),
                )
                .dashed(26.0, 18.0)
                .wobbly(2.0),
            );
            let len = a.distance(b);
            let lamps = (len / 140.0) as i32;
            for k in 1..lamps {
                let p = a.lerp(b, k as f32 / lamps as f32);
                if near(p, 0.0) {
                    scene.glows.push(Glow {
                        position: p.extend(0.0),
                        radius: 14.0,
                        color: palette::LANTERN.with_alpha(0.7),
                    });
                }
            }
            for gate in [a, b] {
                if !near(gate, mouth) {
                    continue;
                }
                let pulse = 0.5 + 0.15 * (time * 1.3).sin();
                scene.lines.push(
                    Line::circle(
                        gate.extend(-6.0),
                        mouth,
                        4.5,
                        palette::LANTERN.with_alpha(pulse),
                    )
                    .wobbly(2.0),
                );
                scene.lines.push(
                    Line::circle(
                        gate.extend(-6.0),
                        mouth * 0.62,
                        2.5,
                        palette::MUSTARD.with_alpha(0.45),
                    )
                    .dashed(12.0, 10.0),
                );
                scene.glows.push(Glow {
                    position: gate.extend(0.0),
                    radius: mouth * 0.7,
                    color: palette::LANTERN.with_alpha(0.25),
                });
            }
        }

        // Stations, their docking rings and lanterns.
        let mut lights: Vec<(f32, PointLight)> = Vec::new();
        for s in world.stations.values() {
            let Some(art) = self.stations.get(&s.id) else {
                continue;
            };
            let pos = g(s.position);
            if !near(pos, s.dock_radius * 2.0) {
                continue;
            }
            scene.meshes.push(
                MeshInstance::new(art.mesh, art.transform)
                    .with_material(ArtBible::bundled().material(InkRole::Station)),
            );
            for l in &art.lights {
                let p = art.transform.transform_point3(l.position);
                let scale = art.transform.x_axis.length();
                scene.glows.push(Glow {
                    position: p + Vec3::Z * 3.0,
                    radius: l.radius * scale,
                    color: l.color.with_alpha(0.5),
                });
                if l.radius > 5.0 {
                    lights.push((
                        p.truncate().distance(rig.center),
                        PointLight {
                            position: p + Vec3::Z * 6.0,
                            color: l.color.scale(0.8),
                            radius: l.radius * scale * 2.5,
                        },
                    ));
                }
            }
            let inside = flagship.is_some_and(|f| {
                f.docked_at.is_none()
                    && f.region == s.region
                    && f.position.distance(s.position) <= s.dock_radius
            });
            let docked_here = flagship.is_some_and(|f| f.docked_at.as_ref() == Some(&s.id));
            let (alpha, width) = if docked_here {
                (0.85, 3.5)
            } else if inside {
                (0.8 + 0.15 * (time * 3.0).sin(), 3.5)
            } else {
                (0.45, 2.5)
            };
            scene.lines.push(
                Line::circle(
                    pos.extend(-4.0),
                    s.dock_radius,
                    width,
                    palette::LANTERN.with_alpha(alpha),
                )
                .dashed(14.0, 9.0)
                .wobbly(1.2),
            );
        }
        lights.sort_by(|a, b| a.0.total_cmp(&b.0));
        scene
            .lights
            .points
            .extend(lights.into_iter().take(MAX_POINT_LIGHTS).map(|(_, l)| l));

        // Rocks, turning slowly.
        for rock in world.rocks.values() {
            let Some(art) = self.rocks.get(&rock.id) else {
                continue;
            };
            let pos = g(rock.position);
            if !near(pos, 60.0) {
                continue;
            }
            let seed = seed_of(rock.id.as_str());
            let spin = (seed % 628) as f32 / 100.0 + time * (0.02 + (seed % 7) as f32 * 0.006);
            scene.meshes.push(
                MeshInstance::new(
                    art.mesh,
                    Mat4::from_rotation_translation(Quat::from_rotation_z(spin), pos.extend(0.0)),
                )
                .with_material(ArtBible::bundled().material(InkRole::Rock)),
            );
        }

        // Ships, engine glow and the mining beam.
        for ship in world.ships.values() {
            let pose = poses.get(&ship.id).copied().unwrap_or(Pose {
                position: ship.position,
                heading: ship.heading,
            });
            let pos = g(pose.position);
            if !near(pos, 40.0) {
                continue;
            }
            let Some(&mesh) = self.hulls.get(&paint_key(world, ship)) else {
                continue;
            };
            let docked = ship.docked_at.is_some();
            let (pos, heading) = match ship.docked_at.as_ref().and_then(|s| world.stations.get(s)) {
                Some(st) => berth(st, &ship.id),
                None => (pos, pose.heading),
            };
            let pose = Pose {
                position: Vec2::new(pos.x, pos.y),
                heading,
            };
            let scale = if docked {
                SHIP_SCALE * 0.85
            } else {
                SHIP_SCALE
            };
            let hover = SHIP_HOVER;
            let mut inst = MeshInstance::at(mesh, pos, pose.heading, scale)
                .with_material(ArtBible::bundled().material(InkRole::Ship));
            inst.transform = Mat4::from_translation(Vec3::Z * hover) * inst.transform;
            scene.meshes.push(inst);
            if world.traders.contains_key(&ship.id)
                && let Some(at) = camera.world_to_screen(pos.extend(hover), viewport)
            {
                scene.overlay.texts.push(
                    render::Text::new(ship.name.clone(), at + G2::new(0.0, 18.0), 13.0)
                        .align(render::Align::Center)
                        .color(palette::CREAM),
                );
            }
            let fwd = G2::from_angle(pose.heading);
            let moving = ship.velocity.length() > 2.0 || ship.activity.transit.is_some();
            if moving && !docked {
                scene.glows.push(Glow {
                    position: (pos - fwd * 3.2 * scale).extend(hover + 1.0),
                    radius: 3.0 * scale,
                    color: palette::LANTERN.with_alpha(0.9),
                });
            }
            if let Some(run) = &ship.activity.mining
                && !docked
                && let Some(rock) = world.rocks.get(&run.rock)
            {
                let r = g(rock.position);
                let from = pos + fwd * 2.2 * scale;
                let radius = galaxy_ship::space::rock_radius(world, rock);
                let to = r + (from - r).normalize_or_zero() * radius * 0.45;
                let shimmer = 0.85 + 0.15 * (time * 9.0).sin();
                scene.lines.push(Line::beam(
                    from.extend(hover),
                    to.extend(radius * 0.4),
                    9.0,
                    Color::hex(0x7ff0d8).with_alpha(shimmer),
                ));
                scene.glows.push(Glow {
                    position: to.extend(radius * 0.5),
                    radius: 22.0,
                    color: Color::hex(0xbaffef).with_alpha(0.9),
                });
                scene.glows.push(Glow {
                    position: from.extend(hover + 1.0),
                    radius: 9.0,
                    color: Color::hex(0xbaffef).with_alpha(0.8),
                });
            }
        }
        scene
    }
}

/// Where a docked ship is drawn: parked on the station's ring, nose along it, at a
/// spot that depends on the ship so several don't stack.
fn berth(station: &Station, ship: &ShipId) -> (G2, f32) {
    let a = -0.7 + (seed_of(ship.as_str()) % 6) as f32 * std::f32::consts::TAU / 6.0;
    let r = station.dock_radius * RING_FILL;
    (
        g(station.position) + G2::from_angle(a) * r,
        a + std::f32::consts::FRAC_PI_2,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_keeps_up_and_zoom_is_bounded() {
        let mut rig = CameraRig::new(G2::ZERO);
        rig.follow(G2::new(1000.0, 0.0), 0.016);
        assert!(G2::new(1000.0, 0.0).distance(rig.center) <= rig.zoom * 0.3 + 0.01);
        for _ in 0..100 {
            rig.zoom_by(1.0);
        }
        assert_eq!(rig.zoom, CameraRig::MIN_ZOOM);
        for _ in 0..100 {
            rig.zoom_by(-1.0);
        }
        assert_eq!(rig.zoom, CameraRig::MAX_ZOOM);
    }

    #[test]
    fn rocks_remesh_on_visible_steps_only() {
        let mut rock = Rock {
            id: RockId::from_static("rock.a.01"),
            field: FieldId::from_static("field.a"),
            rock_type: RockTypeId::from_static("rock_type.ice"),
            position: Vec2::ZERO,
            remaining: 100,
            initial: 100,
            growth: RockGrowth::default(),
        };
        assert_eq!(rock_step(&rock), 20);
        rock.remaining = 96;
        assert_eq!(rock_step(&rock), 20, "a small cut keeps the mesh");
        rock.remaining = 94;
        assert_eq!(rock_step(&rock), 19);
        rock.remaining = 0;
        assert_eq!(rock_step(&rock), 0);
    }
}
