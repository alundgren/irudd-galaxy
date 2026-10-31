//! World to scene: the sim's plain data drawn with the render kit.
//!
//! World units map one to one onto render units on the `z = 0` plane. The kit's meshes
//! are modelled small (a Wren is 6 long), so ships and stations are scaled up to the
//! sim's distances: a station is fitted inside its docking radius, with its kit ring
//! on the sim's ring, and a ship is a few times its kit size.
//!
//! [`View`] keeps the uploaded meshes and re-meshes what changes: a rock when its
//! `remaining` crosses one of [`ROCK_STEPS`] steps, a station when its build stage
//! moves (Kettle Dock's greenhouse), a hull the first time it's seen in a paint and a
//! fit ([`fit_look`]: fitted parts change the ship's look). A rock's mesh morphs from
//! one step to the next as it is mined, and its instance shrinks with what is left, so
//! its shape and shading change continuously.

pub mod cradle;
pub mod fit_look;
pub mod life;
mod live;
pub mod map;
pub mod modules;
pub mod recipes;
pub mod themes;

use render::art_bible::ArtMaterial;
use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use render::glam::{Mat4, Quat, Vec2 as G2, Vec3};
use render::kit::{
    self, CradlePose, LookId, RockParams, ShipFitLook, SocketRing, StationLight, StationRecipe,
};
use render::{
    ArtBible, BackdropHandle, Camera, Color, Glow, InkRole, Line, MAX_POINT_LIGHTS, MeshHandle,
    MeshInstance, PointLight, Renderer, Scene, palette,
};

use crate::session::Pose;

/// Rocks are re-meshed when `remaining / initial` crosses one of this many steps, and
/// morph between them.
pub const ROCK_STEPS: f32 = 20.0;
/// Ships are drawn this many times their kit size (a Wren is about 30 units long).
pub const SHIP_SCALE: f32 = 5.0;
/// Ships fly this high above the plane, so they pass over rocks rather than through.
pub const SHIP_HOVER: f32 = 22.0;
/// The camera's gentle tilt in flight, degrees: top-down with a little depth.
pub const TILT: f32 = 14.0;
/// The docked three-quarter view (#47, 4.1): docking tilts the camera this far from
/// straight down, so module heights, awnings and cranes read. View only: everything
/// still moves on the plane.
pub const DOCK_TILT: f32 = 40.0;
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

/// The follow camera: where it looks and how much it sees. Its zoom is data (the
/// flight rules' `zoom` and the ship's zoom modifiers, [`CameraRig::fit`]); the mouse
/// wheel zooms on top of that.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraRig {
    pub center: G2,
    /// Visible world height.
    pub zoom: f32,
    /// The mouse wheel's share of the zoom, on top of the data's (1 = as the data says).
    pub wheel: f32,
    /// Degrees from straight down: [`TILT`] in flight, [`DOCK_TILT`] docked.
    pub tilt: f32,
    /// A glide in progress: docking, undocking, opening or closing a room.
    pub glide: Option<Glide>,
}

/// A short camera move: from where the camera was when it began to wherever its goal is
/// now (the goal may move, as a ship does), eased, over [`GLIDE_SECONDS`]. View only: a
/// click skips it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Glide {
    pub from: G2,
    pub from_zoom: f32,
    pub from_tilt: f32,
    /// Seconds into the glide.
    pub t: f32,
}

/// How long a glide takes: under half a second.
pub const GLIDE_SECONDS: f32 = 0.4;
/// A glide that tilts the camera (docking swoops in, undocking tilts back) takes a
/// little longer, so the turn reads. A click still lands it at once.
pub const SWOOP_SECONDS: f32 = 0.75;

impl CameraRig {
    pub const MIN_ZOOM: f32 = 220.0;
    pub const MAX_ZOOM: f32 = 1800.0;
    /// The visible height at zoom 1.0.
    pub const DEFAULT_ZOOM: f32 = 440.0;

    pub fn new(center: G2) -> Self {
        Self {
            center,
            zoom: Self::DEFAULT_ZOOM,
            wheel: 1.0,
            tilt: TILT,
            glide: None,
        }
    }

    /// Straight onto `goal`, with no glide: a shot, a test, the first frame.
    pub fn settle(&mut self, goal: &crate::frame::CameraGoal, world: &WorldState) {
        self.glide = None;
        self.center = goal.center;
        self.tilt = goal.tilt;
        match goal.zoom {
            Some(zoom) => self.zoom = zoom,
            None => self.fit(world),
        }
    }

    /// Starts a glide from where the camera is now.
    pub fn begin_glide(&mut self) {
        self.glide = Some(Glide {
            from: self.center,
            from_zoom: self.zoom,
            from_tilt: self.tilt,
            t: 0.0,
        });
    }

    /// Whether a glide is under way.
    pub fn gliding(&self) -> bool {
        self.glide.is_some()
    }

    /// Ends a glide at once (a click during it).
    pub fn skip_glide(&mut self) {
        if let Some(g) = &mut self.glide {
            g.t = f32::INFINITY;
        }
    }

    /// Moves towards `center`, `zoom` (the visible height) and `tilt`: along the glide
    /// while one is under way, `dt` real seconds further; else straight there. A docked
    /// camera holds still on its goal.
    pub fn glide_to(&mut self, center: G2, zoom: f32, tilt: f32, dt: f32) {
        let Some(g) = &mut self.glide else {
            self.center = center;
            self.zoom = zoom;
            self.tilt = tilt;
            return;
        };
        let seconds = if (tilt - g.from_tilt).abs() > 1.0 {
            SWOOP_SECONDS
        } else {
            GLIDE_SECONDS
        };
        g.t += dt.max(0.0);
        let k = (g.t / seconds).clamp(0.0, 1.0);
        let k = k * k * (3.0 - 2.0 * k);
        self.center = g.from.lerp(center, k);
        self.zoom = g.from_zoom + (zoom - g.from_zoom) * k;
        self.tilt = g.from_tilt + (tilt - g.from_tilt) * k;
        if g.t >= seconds {
            self.glide = None;
        }
    }

    /// The flight zoom the data and the wheel ask for now.
    pub fn flight_zoom(&self, world: &WorldState) -> f32 {
        let ship = world.player.ship.as_ref().and_then(|s| world.ships.get(s));
        let zoom = galaxy_ship::flight::camera_zoom(world, ship);
        (Self::DEFAULT_ZOOM / zoom * self.wheel).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM)
    }

    /// Sets the visible height from the data: [`Self::DEFAULT_ZOOM`] at zoom 1.0, more
    /// below it (0.8 shows 550 u), times the wheel's share.
    pub fn fit(&mut self, world: &WorldState) {
        self.zoom = self.flight_zoom(world);
    }

    pub fn camera(&self) -> Camera {
        Camera::top_down(self.center, self.zoom).with_tilt(self.tilt)
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
        let base = self.zoom / self.wheel.max(f32::EPSILON);
        self.zoom = (self.zoom * 0.88f32.powf(steps)).clamp(Self::MIN_ZOOM, Self::MAX_ZOOM);
        self.wheel = self.zoom / base.max(f32::EPSILON);
    }
}

struct StationArt {
    mesh: MeshHandle,
    stage: u32,
    /// Station space to world.
    transform: Mat4,
    lights: Vec<StationLight>,
    /// How far the art reaches from the station-space origin, in world units.
    reach: f32,
}

struct RockArt {
    mesh: MeshHandle,
    step: i32,
    /// The mesh as uploaded (full size, morphing from this step to the next), for
    /// placing the mining beam on its surface.
    data: render::MeshData,
}

/// Which hull mesh draws a ship: its hull, its paint (`None` for the player's own) and
/// what it wears.
type HullKey = (HullId, Option<FactionId>, ShipFitLook);

/// Uploaded meshes and backdrops for the world.
#[derive(Default)]
pub struct View {
    /// One mesh per hull, paint and fit: built when a combination is first seen, never
    /// per frame.
    hulls: BTreeMap<HullKey, MeshHandle>,
    /// Single parts for the cradle's ghost previews.
    parts: BTreeMap<(HullId, LookId), MeshHandle>,
    /// Single parts in their paint, for the crane to carry in the cradle.
    solids: BTreeMap<(HullId, LookId), MeshHandle>,
    stations: BTreeMap<StationId, StationArt>,
    rocks: BTreeMap<RockId, RockArt>,
    backdrops: BTreeMap<Option<RegionId>, BackdropHandle>,
    /// Station parts' modules, by what changes their look.
    modules: BTreeMap<StationPartId, (modules::ModuleKey, MeshHandle)>,
    /// How stations build, light and people themselves ([`themes`]).
    themes: Option<themes::Themes>,
    /// How much ambient life a docked station shows (the player's setting).
    pub life: life::LifeLevel,
    /// Life's meshes, per theme and paint ([`life`]).
    life_art: BTreeMap<ArtKey, LifeArt>,
}

/// A theme's ID and the faction it paints with.
type ArtKey = (String, Option<String>);

/// One theme's life meshes in one paint.
struct LifeArt {
    residents: BTreeMap<themes::Resident, MeshHandle>,
    stock: BTreeMap<WareId, MeshHandle>,
    /// Dark, lit.
    lamps: [MeshHandle; 2],
    mast: MeshHandle,
    jib: MeshHandle,
    pole: MeshHandle,
}

/// How long a craft takes to come in to its berth, or pull away from it, in ticks.
const BERTH_TICKS: u64 = 2 * galaxy_contract::TICKS_PER_SECOND as u64;

fn rock_step(rock: &Rock) -> i32 {
    let frac = rock.remaining as f32 / rock.initial.max(1) as f32;
    (frac.clamp(0.0, 1.0) * ROCK_STEPS).ceil() as i32
}

/// A rock's radius when full; the kit shrinks it with `remaining`.
fn rock_full_radius(world: &WorldState, rock: &Rock) -> f32 {
    let m = &world.ship_rules.mining;
    m.radius_base + m.radius_per_sqrt_unit * (rock.initial as f32).sqrt()
}

fn rock_frac(rock: &Rock) -> f32 {
    (rock.remaining as f32 / rock.initial.max(1) as f32).clamp(0.0, 1.0)
}

/// How far a rock's mesh is drawn towards its next step: 0 at the step it was meshed
/// for, 1 at the next one down.
fn rock_morph(rock: &Rock, step: i32) -> f32 {
    (step as f32 - rock_frac(rock) * ROCK_STEPS).clamp(0.0, 1.0)
}

/// Where a rock is drawn: turning slowly about its seed's spin, and scaled from its
/// full-size mesh to what is left. Rubble is meshed at its own size.
fn rock_transform(rock: &Rock, art: &RockArt, time: f32) -> Mat4 {
    let seed = seed_of(rock.id.as_str());
    let spin = (seed % 628) as f32 / 100.0 + time * (0.02 + (seed % 7) as f32 * 0.006);
    let scale = if art.step > 0 {
        kit::rock_scale(rock_frac(rock))
    } else {
        1.0
    };
    Mat4::from_scale_rotation_translation(
        Vec3::splat(scale),
        Quat::from_rotation_z(spin),
        g(rock.position).extend(0.0),
    )
}

/// Where a beam from `from` towards the rock's centre meets its drawn surface.
fn rock_hit(rock: &Rock, art: &RockArt, time: f32, from: Vec3) -> Option<Vec3> {
    let to_local = rock_transform(rock, art, time).inverse();
    let dir = g(rock.position).extend(0.0) - from;
    let t = art.data.morphed(rock_morph(rock, art.step)).raycast(
        to_local.transform_point3(from),
        to_local.transform_vector3(dir),
    )?;
    Some(from + dir * t)
}

/// A rock's mesh for `step`: the rock at that step, morphing into the next one down.
fn rock_mesh(world: &WorldState, rock: &Rock, step: i32) -> render::MeshData {
    let params = RockParams::new(
        recipes::rock_kind(world, &rock.rock_type),
        rock_full_radius(world, rock),
        step as f32 / ROCK_STEPS,
    );
    kit::rock_morph(
        &params,
        (step - 1) as f32 / ROCK_STEPS,
        seed_of(rock.id.as_str()),
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

fn paint_key(world: &WorldState, ship: &Ship) -> HullKey {
    let faction = match ship.owner {
        Owner::Player => None,
        _ => Some(
            ship_faction(world, ship).unwrap_or_else(|| FactionId::from_static("faction.none")),
        ),
    };
    (ship.hull.clone(), faction, fit_look::ship_look(world, ship))
}

/// The mesh parameters for a hull in its paint.
fn painted_params(hull: &HullId, faction: Option<&FactionId>) -> render::kit::HullParams {
    match faction {
        None => recipes::hull_params(hull),
        Some(f) => recipes::npc_hull_params(hull, Some(f)),
    }
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

    /// The station themes this view draws with: the core pack's built in, until
    /// [`Self::set_themes`] gives it others.
    pub fn themes(&self) -> &themes::Themes {
        self.themes
            .as_ref()
            .unwrap_or_else(|| themes::Themes::bundled())
    }

    /// Draws with these themes from now on (the packs folder's, checked).
    pub fn set_themes(&mut self, themes: themes::Themes) {
        self.themes = Some(themes);
        self.life_art.clear();
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
                    reach: recipe.footprint() * station_transform(s, &recipe).x_axis.length(),
                },
            );
        }
        for part in world.station_parts.values() {
            let key = modules::ModuleKey::of(world, part);
            if self.modules.get(&part.id).is_some_and(|(k, _)| *k == key) {
                continue;
            }
            let Some(data) = modules::mesh(world, part) else {
                continue;
            };
            let mesh = match self.modules.get(&part.id) {
                Some((_, m)) => {
                    r.update_mesh(*m, &data);
                    *m
                }
                None => r.upload_mesh(&data),
            };
            self.modules.insert(part.id.clone(), (key, mesh));
        }
        for rock in world.rocks.values() {
            let step = rock_step(rock);
            match self.rocks.get_mut(&rock.id) {
                Some(a) if a.step == step => {}
                Some(a) => {
                    a.step = step;
                    a.data = rock_mesh(world, rock, step);
                    r.update_mesh(a.mesh, &a.data);
                }
                None => {
                    let data = rock_mesh(world, rock, step);
                    let mesh = r.upload_mesh(&data);
                    self.rocks
                        .insert(rock.id.clone(), RockArt { mesh, step, data });
                }
            }
        }
        for ship in world.ships.values() {
            let (hull, faction, look) = paint_key(world, ship);
            self.hull_mesh(r, &hull, faction.as_ref(), look);
        }
        self.sync_life(r, world);
        let mut regions: Vec<Option<RegionId>> = vec![None];
        regions.extend(world.regions.keys().cloned().map(Some));
        for region in regions {
            self.backdrops.entry(region).or_insert_with_key(|region| {
                let seed = region.as_ref().map_or(7, |r| seed_of(r.as_str()));
                r.upload_backdrop(&kit::backdrop(&recipes::backdrop(region.as_ref()), seed))
            });
        }
    }

    /// The mesh for `hull` in its paint (`None`: the player's) wearing `look`, uploaded
    /// the first time it's asked for. The cradle uses it for a fit that isn't made yet.
    pub fn hull_mesh(
        &mut self,
        r: &mut Renderer,
        hull: &HullId,
        faction: Option<&FactionId>,
        look: ShipFitLook,
    ) -> MeshHandle {
        let key = (hull.clone(), faction.cloned(), look);
        *self.hulls.entry(key).or_insert_with(|| {
            let params = painted_params(hull, faction);
            r.upload_mesh(&kit::fitted_hull(&params, seed_of(hull.as_str()), &look))
        })
    }

    /// One part alone on `hull`, painted as a ghost for a preview in the cradle.
    pub fn part_mesh(&mut self, r: &mut Renderer, hull: &HullId, look: LookId) -> MeshHandle {
        *self
            .parts
            .entry((hull.clone(), look))
            .or_insert_with(|| r.upload_mesh(&kit::part_ghost(look, &recipes::hull_params(hull))))
    }

    /// The player's `hull` in the hullwright's cradle, wearing `look`, as large as `span`
    /// world units around `center`. With a `ghost` part, that part is drawn where it would
    /// go and its socket rings sky blue; fitted sockets ring gold. Draw it with
    /// [`kit::Cradle::draw`] after the room, and hit-test with [`kit::Cradle::sockets`];
    /// change its `rings` for other drop targets.
    pub fn cradle(
        &mut self,
        r: &mut Renderer,
        hull: &HullId,
        look: ShipFitLook,
        ghost: Option<LookId>,
        center: G2,
        span: f32,
    ) -> kit::Cradle {
        let params = recipes::hull_params(hull);
        let rings = kit::Socket::ALL.map(|s| {
            if ghost.is_some_and(|g| g.socket() == s) {
                SocketRing::DropTarget
            } else if look.get(s).is_some() {
                SocketRing::Fitted
            } else {
                SocketRing::Empty
            }
        });
        kit::Cradle {
            ship: self.hull_mesh(r, hull, None, look),
            ghost: ghost.map(|g| self.part_mesh(r, hull, g)),
            pose: CradlePose::fit(center, span, &params),
            hull: params,
            look,
            rings,
            shadow: kit::CRADLE_SHADOW,
        }
    }

    /// Every station part's module near the camera, the `hovered` one lifted and
    /// outlined in orange. A part removed since the last sync isn't drawn.
    pub fn draw_modules(
        &self,
        scene: &mut Scene,
        world: &WorldState,
        rig: &CameraRig,
        hovered: Option<&StationPartId>,
    ) {
        let material = ArtBible::bundled().material(InkRole::Station);
        for part in world.station_parts.values() {
            let Some((_, mesh)) = self.modules.get(&part.id) else {
                continue;
            };
            let Some((p, _)) = modules::module_point(world, part) else {
                continue;
            };
            if p.distance(rig.center) > rig.zoom * 2.5 + 400.0 {
                continue;
            }
            let lift = hovered == Some(&part.id);
            if let Some(m) = modules::draw_transform(world, part, lift) {
                scene
                    .meshes
                    .push(MeshInstance::new(*mesh, m).with_material(material));
            }
            if lift {
                modules::outline(scene, world, part);
            }
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
        history: Option<&galaxy_core::History>,
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

        // The edge current: near its region's edge, lantern arrows on the edge point the
        // flagship back in; past it, more flow inward beside the ship.
        if let Some(ship) = flagship.filter(|s| s.docked_at.is_none())
            && let Some(region) = world.regions.get(&ship.region)
        {
            edge_current(&mut scene, region, poses.get(&ship.id), ship, time);
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
        // Where station art covers the screen (centre, radius in pixels): ship names
        // keep off it.
        let mut station_art: Vec<(G2, f32)> = Vec::new();
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
            let centre = art.transform.w_axis.truncate();
            if let Some(c) = camera.world_to_screen(centre, viewport) {
                let r = [Vec3::X, Vec3::Y]
                    .into_iter()
                    .filter_map(|d| camera.world_to_screen(centre + d * art.reach, viewport))
                    .map(|p| p.distance(c))
                    .fold(0.0, f32::max);
                station_art.push((c, r));
            }
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
            if !near(g(rock.position), 60.0) {
                continue;
            }
            scene.meshes.push(
                MeshInstance::new(art.mesh, rock_transform(rock, art, time))
                    .with_material(ArtBible::bundled().material(InkRole::Rock))
                    .with_morph(rock_morph(rock, art.step))
                    .with_hatch_scale(1.0),
            );
        }

        // Craft coming in to a berth or pulling away from one, from their latest dock or
        // undock fact: (ship, the fact's station, docking, share of the way done).
        let berthing = berthing(world, history);

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
            let key = paint_key(world, ship);
            let Some(&mesh) = self.hulls.get(&key) else {
                continue;
            };
            let look = key.2;
            let docked = ship.docked_at.is_some();
            let (pos, heading) = match ship.docked_at.as_ref().and_then(|s| world.stations.get(s)) {
                Some(st) => berth(world, st, &ship.id),
                None => (pos, pose.heading),
            };
            // A real craft glides in to its berth from where it docked, and out again
            // to where it is, rather than jumping.
            let moving = berthing.get(&ship.id).and_then(|(st, docking, k)| {
                (*docking == docked).then_some((world.stations.get(st)?, *docking, *k))
            });
            let (pos, heading) = match moving {
                Some((st, docking, k)) => {
                    let (b, bh) = berth(world, st, &ship.id);
                    let (from, to, fh, th) = if docking {
                        (g(ship.position), b, ship.heading, bh)
                    } else {
                        (b, pos, bh, heading)
                    };
                    let k = k * k * (3.0 - 2.0 * k);
                    let turn = (th - fh + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                        - std::f32::consts::PI;
                    (from.lerp(to, k), fh + turn * k)
                }
                _ => (pos, heading),
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
            if look.utility == Some(LookId::SeamLamp) {
                let params = recipes::hull_params(&ship.hull);
                scene.glows.push(Glow {
                    position: inst
                        .transform
                        .transform_point3(kit::seam_lamp_light(&params)),
                    radius: params.width * scale * 0.4,
                    color: kit::SEAM_LIGHT.with_alpha(0.75),
                });
            }
            scene.meshes.push(inst);
            // A trader's name, off the station art and on screen. A docked ship sits
            // inside its station's art, so it goes unnamed until it flies off.
            if world.traders.contains_key(&ship.id)
                && !docked
                && let Some(at) = camera.world_to_screen(pos.extend(hover), viewport)
                && (0.0..=viewport.x).contains(&at.x)
                && (0.0..=viewport.y).contains(&at.y)
            {
                let size = G2::new(
                    crate::ui::text_width(&ship.name, LABEL_SIZE),
                    LABEL_SIZE * 1.3,
                );
                let nose = recipes::hull_params(&ship.hull).length * 0.5 * scale;
                let own = camera
                    .world_to_screen(pos.extend(hover) + Vec3::X * nose, viewport)
                    .map_or(0.0, |p| p.distance(at));
                scene.overlay.texts.push(
                    render::Text::new(
                        ship.name.clone(),
                        place_label(at, own, size, &station_art, viewport),
                        LABEL_SIZE,
                    )
                    .align(render::Align::Center)
                    .color(Color::WHITE),
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
                // From the nose tip (a long drill's tip) to where the beam meets the
                // rock's drawn surface (as morphed and scaled this frame). The renderer
                // draws beams over what they cross, so the end must sit on the surface,
                // not inside it.
                let nose = recipes::hull_params(&ship.hull).nose_reach(&look) * scale;
                let from = (pos + fwd * nose).extend(hover);
                let to = self
                    .rocks
                    .get(&rock.id)
                    .and_then(|art| rock_hit(rock, art, time, from))
                    .unwrap_or_else(|| {
                        // No surface in the way: the rock's edge facing the ship.
                        let r = g(rock.position);
                        let radius = galaxy_ship::space::rock_radius(world, rock);
                        (r + (from.truncate() - r).normalize_or_zero() * radius).extend(0.0)
                    });
                // A fitted boost (the mining lens) shows: a wider, quicker beam.
                let boost = (galaxy_contract::modifiers::for_ship(world, &ship.id).mining_rate
                    as f32)
                    .clamp(1.0, 3.0);
                let shimmer = 0.85 + 0.15 * (time * 9.0 * boost).sin();
                scene.lines.push(Line::beam(
                    from,
                    to,
                    9.0 * boost.sqrt(),
                    Color::hex(0x7ff0d8).with_alpha(shimmer),
                ));
                scene.glows.push(Glow {
                    position: to,
                    radius: 22.0 * boost.sqrt(),
                    color: Color::hex(0xbaffef).with_alpha(0.9),
                });
                scene.glows.push(Glow {
                    position: from,
                    radius: 9.0,
                    color: Color::hex(0xbaffef).with_alpha(0.8),
                });
            }
        }
        scene
    }
}

/// Lantern chevrons pointing in from a region's edge, fading in as the ship comes
/// within [`EDGE_NEAR`] of it; past the edge, rows of them drift inward beside the
/// ship, the current that carries it back.
fn edge_current(scene: &mut Scene, region: &Region, pose: Option<&Pose>, ship: &Ship, time: f32) {
    const EDGE_NEAR: f32 = 260.0;
    let c = g(region.center);
    let p = pose.map_or(g(ship.position), |p| g(p.position));
    let off = p - c;
    let d = off.length();
    if d <= region.radius - EDGE_NEAR || d <= 0.0 {
        return;
    }
    let out = off / d;
    let alpha = ((d - (region.radius - EDGE_NEAR)) / EDGE_NEAR).clamp(0.0, 1.0);
    let color = palette::LANTERN.with_alpha(0.75 * alpha);
    let chevron = |at: G2, dir: G2, size: f32| {
        let side = G2::new(-dir.y, dir.x);
        let tip = at - dir * size;
        let a = at + side * size;
        let b = at - side * size;
        Line::path(
            vec![a.extend(-6.0), tip.extend(-6.0), b.extend(-6.0)],
            4.0,
            color,
        )
    };
    for k in -4..=4 {
        let dir = G2::from_angle(k as f32 * 110.0 / region.radius).rotate(out);
        scene
            .lines
            .push(chevron(c + dir * region.radius, dir, 16.0));
    }
    let depth = d - region.radius;
    if depth > 0.0 {
        let side = G2::new(-out.y, out.x);
        let flow = (time * 60.0) % 90.0;
        let mut r = depth + 180.0 - flow;
        while r > 0.0 {
            for lane in [-1.0, 1.0] {
                let at = c + out * (region.radius + r) + side * (70.0 * lane);
                scene.lines.push(chevron(at, out, 12.0));
            }
            r -= 90.0;
        }
    }
}

/// Font size of a ship's name.
const LABEL_SIZE: f32 = 13.0;

/// Where a ship's name goes (the top centre of the text) for a ship drawn at `at` on
/// screen, `own` pixels in radius: under it, or over it, whichever keeps clear of the
/// station art in `avoid` (screen circles) without leaving the screen; failing both,
/// just outside the nearest station's art and the ship, on the ship's side. Always
/// inside the viewport.
fn place_label(at: G2, own: f32, size: G2, avoid: &[(G2, f32)], viewport: G2) -> G2 {
    const GAP: f32 = 18.0;
    const MARGIN: f32 = 6.0;
    let half = size.x * 0.5;
    let clamp = |p: G2| {
        G2::new(
            p.x.clamp(
                MARGIN + half,
                (viewport.x - MARGIN - half).max(MARGIN + half),
            ),
            p.y.clamp(MARGIN, (viewport.y - MARGIN - size.y).max(MARGIN)),
        )
    };
    let clear = |p: G2| {
        avoid.iter().all(|&(c, r)| {
            let min = G2::new(p.x - half, p.y);
            c.clamp(min, min + size).distance(c) >= r
        })
    };
    let below = G2::new(at.x, at.y + GAP);
    let above = G2::new(at.x, at.y - GAP - size.y);
    // A side that only fits by sliding over the ship (at the top or bottom edge)
    // doesn't count: the other side flips in.
    for p in [below, above] {
        let q = clamp(p);
        if (q.y - p.y).abs() < 0.5 && clear(q) {
            return q;
        }
    }
    if let Some(&(c, r)) = avoid
        .iter()
        .min_by(|a, b| (a.0.distance(at) - a.1).total_cmp(&(b.0.distance(at) - b.1)))
    {
        let reach = r.max(at.distance(c) + own);
        let out = c + (at - c).normalize_or(G2::Y) * (reach + size.length() * 0.5 + 4.0);
        let q = clamp(G2::new(out.x, out.y - size.y * 0.5));
        if clear(q) {
            return q;
        }
    }
    let q = clamp(below);
    if (q.y - below.y).abs() < 0.5 {
        q
    } else {
        clamp(above)
    }
}

/// Where a docked ship is drawn: parked on the station's drawn docking ring (the kit's,
/// which can be smaller than the sim's where the station's art had to shrink to fit),
/// nose along it, at a spot that depends on the ship so several don't stack.
pub(super) fn berth(world: &WorldState, station: &Station, ship: &ShipId) -> (G2, f32) {
    let a = -0.7 + (seed_of(ship.as_str()) % 6) as f32 * std::f32::consts::TAU / 6.0;
    let recipe = galaxy_core::parts::recipe_of(world, station);
    let scale = station_transform(station, &recipe)
        .x_axis
        .truncate()
        .length();
    let r = ring_radius(&recipe).map_or(station.dock_radius * RING_FILL, |r| r * scale);
    (
        g(station.position) + G2::from_angle(a) * r,
        a + std::f32::consts::FRAC_PI_2,
    )
}

/// Craft that docked or undocked within [`BERTH_TICKS`]: their station, whether they
/// docked, and how far through the move they are (0 to 1).
pub fn berthing(
    world: &WorldState,
    history: Option<&galaxy_core::History>,
) -> BTreeMap<ShipId, (StationId, bool, f32)> {
    let mut out = BTreeMap::new();
    let Some(history) = history else {
        return out;
    };
    let now = world.meta.tick.0;
    let since = Tick(now.saturating_sub(BERTH_TICKS));
    let (dock, undock) = (
        galaxy_contract::ids::FactKind::from_static("dock"),
        galaxy_contract::ids::FactKind::from_static("undock"),
    );
    let q = galaxy_contract::history::HistoryQuery {
        since: Some(since),
        ..Default::default()
    };
    for f in history.find(&q) {
        let docking = f.kind == dock;
        if !docking && f.kind != undock {
            continue;
        }
        let (Some(ship), Some(station)) = (f.subjects.ship.clone(), f.subjects.station.clone())
        else {
            continue;
        };
        let k = now.saturating_sub(f.tick.0) as f32 / BERTH_TICKS as f32;
        // Later facts replace earlier ones: the newest move wins.
        out.insert(ship, (station, docking, k.clamp(0.0, 1.0)));
    }
    out
}

impl View {
    /// Uploads the life meshes for every station's theme and paint, once each.
    fn sync_life(&mut self, r: &mut Renderer, world: &WorldState) {
        let mut wanted: Vec<(ArtKey, themes::Theme, Station)> = Vec::new();
        for st in world.stations.values() {
            let theme = self.themes().for_station(world, &st.id);
            let key = (
                theme.id.clone(),
                theme.paint_faction(Some(st)).map(str::to_owned),
            );
            if !self.life_art.contains_key(&key) && !wanted.iter().any(|(k, ..)| *k == key) {
                wanted.push((key, theme.clone(), st.clone()));
            }
        }
        for (key, theme, st) in wanted {
            let pal = life::palette_for(&theme, Some(&st));
            let motif = theme.motif;
            let residents = themes::Resident::ALL
                .into_iter()
                .map(|res| {
                    let (coat, hat, skin) = life::resident_colors(res, &pal);
                    (
                        res,
                        r.upload_mesh(&kit::life::resident(coat, hat, skin, motif)),
                    )
                })
                .collect();
            let stock = world
                .wares
                .keys()
                .map(|w| {
                    let paint = life::ware_paint(w, &pal);
                    let mesh = kit::life::stock_unit(theme.stock(), paint, pal.trim);
                    (w.clone(), r.upload_mesh(&mesh))
                })
                .collect();
            let art = LifeArt {
                residents,
                stock,
                lamps: [false, true]
                    .map(|lit| r.upload_mesh(&kit::life::lamp_post(&pal, motif, lit))),
                mast: r.upload_mesh(&kit::life::crane_mast(&pal, life::CRANE_HEIGHT)),
                jib: r.upload_mesh(&kit::life::jib(&pal, 0.5)),
                pole: r.upload_mesh(&kit::life::banner_pole(&pal, motif, life::POLE_HEIGHT)),
            };
            if let Some(paint) = key.1.as_deref().and_then(|f| f.parse::<FactionId>().ok()) {
                for hull in &theme.craft {
                    self.hull_mesh(r, hull, Some(&paint), ShipFitLook::default());
                }
            }
            self.life_art.insert(key, art);
        }
    }

    /// The docked station's life ([`life`]): residents, stock piles, lamps, the crane,
    /// filler craft and the trading craft's approach, drawn while the camera is in the
    /// docked tilt (not at the cradle, which keeps its own framing).
    pub fn draw_life(
        &self,
        scene: &mut Scene,
        world: &WorldState,
        history: &galaxy_core::History,
        rig: &CameraRig,
        time: f32,
    ) {
        let Some(station) = world
            .player
            .ship
            .as_ref()
            .and_then(|s| world.ships.get(s))
            .and_then(|s| s.docked_at.as_ref())
        else {
            return;
        };
        if rig.tilt < (TILT + DOCK_TILT) * 0.5 {
            return;
        }
        let theme = self.themes().for_station(world, station);
        let paint = theme
            .paint_faction(world.stations.get(station))
            .map(str::to_owned);
        let Some(art) = self.life_art.get(&(theme.id.clone(), paint)) else {
            return;
        };
        let f = life::frame(&life::LifeInput {
            world,
            history,
            station,
            theme,
            level: self.life,
            time,
        });
        let s = f.scale;
        let bible = ArtBible::bundled();
        let props = bible.material(InkRole::Prop);
        // People are lighter-inked than anything clickable.
        let people = render::Material {
            outline_px: 1.6,
            hatch: 0.0,
            ..props
        };
        let place = |at: Vec3, yaw: f32| {
            Mat4::from_scale_rotation_translation(Vec3::splat(s), Quat::from_rotation_z(yaw), at)
        };
        for fig in &f.figures {
            if let Some(m) = art.residents.get(&fig.resident) {
                scene
                    .meshes
                    .push(MeshInstance::new(*m, place(fig.at, fig.facing)).with_material(people));
            }
        }
        for c in &f.stock {
            if let Some(m) = art.stock.get(&c.ware) {
                scene
                    .meshes
                    .push(MeshInstance::new(*m, place(c.at, c.turn)).with_material(props));
            }
        }
        for l in &f.lamps {
            scene.meshes.push(
                MeshInstance::new(art.lamps[usize::from(l.lit)], place(l.at, 0.0))
                    .with_material(props),
            );
        }
        for p in &f.poles {
            scene
                .meshes
                .push(MeshInstance::new(art.pole, place(*p, 0.0)).with_material(props));
        }
        if let Some(c) = &f.crane {
            scene
                .meshes
                .push(MeshInstance::new(art.mast, place(c.at, 0.0)).with_material(props));
            let top = c.top();
            scene.meshes.push(
                MeshInstance::new(
                    art.jib,
                    Mat4::from_translation(top)
                        * Mat4::from_rotation_z(c.yaw)
                        * Mat4::from_scale(Vec3::new(c.reach, s, s)),
                )
                .with_material(props),
            );
            let [_, (tip, hook)] = c.lines();
            scene.lines.push(Line::path(
                vec![tip, hook],
                0.25 * s,
                palette::INK.with_alpha(if c.load.is_some() { 0.7 } else { 0.6 }),
            ));
            if let Some((ware, at)) = &c.load
                && let Some(m) = art.stock.get(ware)
            {
                scene
                    .meshes
                    .push(MeshInstance::new(*m, place(*at, c.yaw)).with_material(props));
            }
        }
        for c in &f.craft {
            if let Some(m) = self.hulls.get(&(
                c.hull.clone(),
                Some(c.faction.clone()),
                ShipFitLook::default(),
            )) {
                let mut inst = MeshInstance::at(*m, c.at, c.heading, SHIP_SCALE * 0.8)
                    .with_material(ArtBible::bundled().material(InkRole::Ship));
                inst.transform = Mat4::from_translation(Vec3::Z * SHIP_HOVER) * inst.transform;
                scene.meshes.push(inst);
            }
        }
        // The economy's own traders heading in to dock: a faint dashed way to the berth.
        for (from, to) in life::approaches(world, station) {
            scene.lines.push(
                Line::path(
                    vec![from.extend(SHIP_HOVER - 4.0), to.extend(SHIP_HOVER - 4.0)],
                    2.5,
                    palette::LANTERN.with_alpha(0.35),
                )
                .dashed(12.0, 10.0),
            );
        }
        scene.glows.extend(f.glows);
        scene.lines.extend(f.lines);
        let (glows, lines) = life::far_layer(world, station, rig.center, rig.zoom);
        scene.glows.extend(glows);
        scene.lines.extend(lines);
        // Lamps light the deck with whatever point lights the station left over.
        let free = MAX_POINT_LIGHTS.saturating_sub(scene.lights.points.len());
        let mut lights = f.lights;
        lights.sort_by(|a, b| {
            let d = |l: &PointLight| l.position.truncate().distance(rig.center);
            d(a).total_cmp(&d(b))
        });
        scene.lights.points.extend(lights.into_iter().take(free));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_zoom_is_data_and_the_wheel_zooms_on_top() {
        let mut world = WorldState::default();
        let mut rig = CameraRig::new(G2::ZERO);
        rig.fit(&world);
        assert!((rig.zoom - 550.0).abs() < 1e-3, "zoom 0.8 shows 550 u");
        world.ship_rules.flight.zoom = 1.0;
        rig.fit(&world);
        assert_eq!(rig.zoom, CameraRig::DEFAULT_ZOOM);
        rig.zoom_by(1.0);
        let wheeled = rig.zoom;
        assert!(wheeled < CameraRig::DEFAULT_ZOOM);
        rig.fit(&world);
        assert!((rig.zoom - wheeled).abs() < 1e-3, "the wheel's zoom stays");
        world.ship_rules.flight.zoom = 0.5;
        rig.fit(&world);
        assert!(
            (rig.zoom - wheeled * 2.0).abs() < 1e-2,
            "and rides on the data's"
        );
    }

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

    #[test]
    fn ship_names_keep_off_station_art_and_on_screen() {
        let view = G2::new(1280.0, 720.0);
        let size = G2::new(80.0, 17.0);
        let overlaps = |p: G2, (c, r): (G2, f32)| {
            let min = G2::new(p.x - size.x * 0.5, p.y);
            c.clamp(min, min + size).distance(c) < r
        };
        let on_screen = |p: G2| {
            p.x - size.x * 0.5 >= 0.0
                && p.x + size.x * 0.5 <= view.x
                && p.y >= 0.0
                && p.y + size.y <= view.y
        };
        // In open space: under the ship, as before.
        let p = place_label(G2::new(600.0, 300.0), 20.0, size, &[], view);
        assert_eq!(p, G2::new(600.0, 318.0));
        // A station just under the ship: the name goes over it.
        let wharf = (G2::new(600.0, 420.0), 90.0);
        let p = place_label(G2::new(600.0, 300.0), 20.0, size, &[wharf], view);
        assert!(p.y < 300.0 && !overlaps(p, wharf), "{p}");
        // Flying across the art: outside it and the ship, on the ship's side.
        let ship = (G2::new(670.0, 420.0), 20.0);
        let p = place_label(ship.0, ship.1, size, &[wharf], view);
        assert!(
            !overlaps(p, wharf) && !overlaps(p, ship) && p.x > 600.0,
            "{p}"
        );
        // At the edges: clamped in, and flipped over the ship at the bottom.
        for at in [
            G2::new(4.0, 300.0),
            G2::new(1276.0, 300.0),
            G2::new(600.0, 716.0),
            G2::new(600.0, 2.0),
        ] {
            let p = place_label(at, 20.0, size, &[], view);
            assert!(on_screen(p), "{at} -> {p}");
        }
        let p = place_label(G2::new(600.0, 716.0), 20.0, size, &[], view);
        assert!(p.y + size.y < 716.0, "{p}");
    }

    #[test]
    fn rocks_morph_continuously_between_steps() {
        let mut rock = Rock {
            id: RockId::from_static("rock.a.01"),
            field: FieldId::from_static("field.a"),
            rock_type: RockTypeId::from_static("rock_type.ice"),
            position: Vec2::ZERO,
            remaining: 1000,
            initial: 1000,
            growth: RockGrowth::default(),
        };
        // Mined a unit at a time, the drawn stage (step minus morph) only ever falls,
        // and by a sliver each unit, across re-meshes too.
        let mut last = ROCK_STEPS;
        for remaining in (1..1000).rev() {
            rock.remaining = remaining;
            let step = rock_step(&rock);
            let shown = step as f32 - rock_morph(&rock, step);
            assert!(
                shown <= last + 1e-4 && last - shown < 0.03,
                "{remaining}: {last} -> {shown}"
            );
            last = shown;
        }
    }
}
