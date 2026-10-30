//! A small sample world for the examples, screenshots and GPU tests.
//!
//! This shows how an app maps its own state onto a [`Scene`]: upload kit meshes once,
//! re-mesh a rock when its `remaining` changes enough to see, and rebuild the scene
//! description every frame. It is sample code, not part of the game.

use crate::art_bible::ArtMaterial;
use crate::art_bible::{ArtBible, InkRole};
use crate::camera::Camera;
use crate::color::Color;
use crate::kit::{self, HullParams, RockKind, RockParams, StationRecipe};
use crate::palette;
use crate::renderer::Renderer;
use crate::scene::*;
use glam::{Mat4, Quat, Vec2, Vec3};

/// Rocks are re-meshed when `remaining` crosses one of this many steps.
const ROCK_STEPS: f32 = 20.0;
/// Height of ships above the play plane.
const SHIP_HOVER: f32 = 6.0;

pub struct DemoRock {
    pub kind: RockKind,
    pub pos: Vec2,
    pub radius: f32,
    pub remaining: f32,
    pub seed: u64,
    pub spin: f32,
    mesh: MeshHandle,
    meshed_step: i32,
}

pub struct DemoStation {
    pub recipe: StationRecipe,
    pub pos: Vec2,
    pub angle: f32,
    mesh: MeshHandle,
}

/// The sample world: two stations, a rock field and a Wren.
pub struct DemoWorld {
    pub ship_pos: Vec2,
    pub ship_heading: f32,
    pub ship_moving: bool,
    pub stations: Vec<DemoStation>,
    pub rocks: Vec<DemoRock>,
    /// Index of the rock being mined.
    pub mining: Option<usize>,
    pub paused: bool,
    pub hold: u32,
    pub time: f32,
    /// Visible world height.
    pub zoom: f32,
    /// Camera tilt in degrees (0 is straight down).
    pub tilt: f32,
    pub camera_center: Vec2,
    wren: MeshHandle,
    backdrop: BackdropHandle,
}

fn step_of(remaining: f32) -> i32 {
    (remaining.clamp(0.0, 1.0) * ROCK_STEPS).ceil() as i32
}

impl DemoWorld {
    pub fn new(renderer: &mut Renderer) -> Self {
        let wren = renderer.upload_mesh(&kit::hull(&HullParams::wren(), 1));
        let backdrop = renderer.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), 7));
        let station =
            |r: &mut Renderer, recipe: StationRecipe, pos: Vec2, angle: f32| DemoStation {
                mesh: r.upload_mesh(&kit::station(&recipe)),
                recipe,
                pos,
                angle,
            };
        let stations = vec![
            station(renderer, StationRecipe::lantern_wharf(), Vec2::ZERO, 0.0),
            station(
                renderer,
                StationRecipe::kettle_dock(2),
                Vec2::new(116.0, 30.0),
                0.4,
            ),
        ];
        let field = [
            (RockKind::Ice, Vec2::new(56.0, -14.0), 4.5, 1.0),
            (RockKind::Ice, Vec2::new(70.0, -6.0), 3.2, 0.55),
            (RockKind::Iron, Vec2::new(66.0, -28.0), 5.0, 1.0),
            (RockKind::Iron, Vec2::new(86.0, -20.0), 3.6, 0.3),
            (RockKind::Glassite, Vec2::new(46.0, -32.0), 4.0, 0.85),
            (RockKind::Ice, Vec2::new(96.0, -34.0), 2.6, 1.0),
            (RockKind::Glassite, Vec2::new(102.0, -8.0), 3.4, 1.0),
            (RockKind::Iron, Vec2::new(36.0, -24.0), 2.4, 0.7),
        ];
        let rocks = field
            .iter()
            .enumerate()
            .map(|(i, &(kind, pos, radius, remaining))| {
                let seed = 100 + i as u64;
                DemoRock {
                    kind,
                    pos,
                    radius,
                    remaining,
                    seed,
                    spin: i as f32 * 1.7,
                    mesh: renderer
                        .upload_mesh(&kit::rock(&RockParams::new(kind, radius, remaining), seed)),
                    meshed_step: step_of(remaining),
                }
            })
            .collect();
        Self {
            ship_pos: Vec2::new(47.0, -2.0),
            ship_heading: -0.5,
            ship_moving: true,
            stations,
            rocks,
            mining: Some(0),
            paused: false,
            hold: 2,
            time: 0.0,
            zoom: 92.0,
            tilt: 20.0,
            camera_center: Vec2::new(54.0, 0.0),
            wren,
            backdrop,
        }
    }

    /// Re-meshes rocks whose `remaining` moved to a new visible step.
    pub fn sync_meshes(&mut self, renderer: &mut Renderer) {
        for r in &mut self.rocks {
            let step = step_of(r.remaining);
            if step != r.meshed_step {
                r.meshed_step = step;
                let p = RockParams::new(r.kind, r.radius, r.remaining);
                renderer.update_mesh(r.mesh, &kit::rock(&p, r.seed));
            }
        }
    }

    /// The nearest rock within `reach` of the ship that still has something left.
    pub fn rock_in_reach(&self, reach: f32) -> Option<usize> {
        self.rocks
            .iter()
            .enumerate()
            .filter(|(_, r)| r.remaining > 0.0)
            .map(|(i, r)| (i, r.pos.distance(self.ship_pos) - r.radius))
            .filter(|&(_, d)| d < reach)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    }

    pub fn camera(&self) -> Camera {
        Camera::top_down(self.camera_center, self.zoom).with_tilt(self.tilt)
    }

    /// Builds this frame's scene. `viewport` is the renderer's logical size.
    pub fn scene(&self, viewport: Vec2) -> Scene {
        let camera = self.camera();
        let mut scene = Scene::new(camera);
        scene.backdrop = Some(self.backdrop);
        scene.time = self.time;

        // Stations, their lantern glows and their lights.
        for s in &self.stations {
            let m =
                Mat4::from_rotation_translation(Quat::from_rotation_z(s.angle), s.pos.extend(0.0));
            scene.meshes.push(MeshInstance::new(s.mesh, m));
            for l in kit::station_lights(&s.recipe) {
                let p = m.transform_point3(l.position);
                scene.glows.push(Glow {
                    position: p + Vec3::Z * 2.0,
                    radius: l.radius,
                    color: l.color.with_alpha(0.55),
                });
                if l.radius > 5.0 && scene.lights.points.len() < MAX_POINT_LIGHTS {
                    scene.lights.points.push(PointLight {
                        position: p + Vec3::Z * 3.0,
                        color: l.color.scale(0.8),
                        radius: l.radius * 2.5,
                    });
                }
            }
        }

        // A trade lane between the stations and a soft region border.
        let (a, b) = (self.stations[0].pos, self.stations[1].pos);
        let dir = (b - a).normalize_or_zero();
        scene.lines.push(
            Line::path(
                vec![(a + dir * 36.0).extend(-6.0), (b - dir * 30.0).extend(-6.0)],
                1.1,
                palette::MUSTARD.with_alpha(0.75),
            )
            .dashed(3.5, 2.5)
            .wobbly(0.5),
        );
        scene.lines.push(
            Line::circle(
                Vec3::new(70.0, -20.0, -8.0),
                46.0,
                0.9,
                palette::TEAL.with_alpha(0.4),
            )
            .dashed(6.0, 4.0)
            .wobbly(1.2),
        );

        // Rocks, gently turning.
        for r in &self.rocks {
            let spin = r.spin + self.time * 0.05;
            scene.meshes.push(MeshInstance::new(
                r.mesh,
                Mat4::from_rotation_translation(Quat::from_rotation_z(spin), r.pos.extend(0.0)),
            ));
        }

        // The Wren, its engine glow and the mining beam.
        // Ships fly a little above the plane so they pass over rocks, not through them.
        let mut ship = MeshInstance::at(self.wren, self.ship_pos, self.ship_heading, 1.0);
        ship.transform = Mat4::from_translation(Vec3::Z * SHIP_HOVER) * ship.transform;
        scene.meshes.push(ship);
        let back = Vec2::from_angle(self.ship_heading) * -4.6;
        if self.ship_moving {
            scene.glows.push(Glow {
                position: (self.ship_pos + back).extend(SHIP_HOVER + 1.0),
                radius: 3.5,
                color: palette::LANTERN.with_alpha(0.9),
            });
        }
        if let Some(r) = self.mining.and_then(|i| self.rocks.get(i)) {
            let from = self.ship_pos + Vec2::from_angle(self.ship_heading) * 3.0;
            let to = r.pos + (from - r.pos).normalize_or_zero() * r.radius * 0.5;
            scene.lines.push(Line::beam(
                from.extend(SHIP_HOVER),
                to.extend(2.0),
                1.6,
                Color::hex(0x7ff0d8),
            ));
            scene.glows.push(Glow {
                position: to.extend(3.0),
                radius: 4.0,
                color: Color::hex(0xbaffef).with_alpha(0.9),
            });
        }

        // HUD: one goal line and the hold, as the slice asks.
        let o = &mut scene.overlay;
        o.panels
            .push(Panel::new(Rect::new(20.0, 20.0, 340.0, 62.0)));
        o.texts
            .push(Text::new("Bring ice to Kettle Dock", Vec2::new(40.0, 36.0), 24.0).weight(600));
        let hold = Rect::new(viewport.x - 170.0, 20.0, 150.0, 62.0);
        o.panels.push(Panel {
            fill: palette::INK.with_alpha(0.85),
            border: palette::MUSTARD,
            ..Panel::new(hold)
        });
        o.texts.push(
            Text::new(
                format!("Hold {}/6", self.hold),
                Vec2::new(hold.center().x, 36.0),
                24.0,
            )
            .weight(600)
            .color(palette::CREAM)
            .align(Align::Center),
        );
        // Station name tags under each station.
        for s in &self.stations {
            if let Some(p) = camera.world_to_screen(
                (s.pos - Vec2::Y * (s.recipe.footprint() * 0.75)).extend(0.0),
                viewport,
            ) {
                o.texts.push(
                    Text::new(s.recipe.name.clone(), p, 18.0)
                        .weight(600)
                        .color(palette::CREAM)
                        .align(Align::Center),
                );
            }
        }
        if self.paused {
            o.paused = Some(PausedCard::default());
        }
        scene
    }
}

/// A labelled line-up of every kit piece: the three hulls, the greenhouse stages, each
/// rock kind as it is mined down, and the three preset stations. For reviewing the art.
pub fn gallery(renderer: &mut Renderer) -> Scene {
    let viewport = renderer.logical_size();
    let camera = Camera::top_down(Vec2::new(0.0, 8.0), 250.0).with_tilt(18.0);
    let mut scene = Scene::new(camera);
    scene.backdrop =
        Some(renderer.upload_backdrop(&kit::backdrop(&kit::BackdropParams::default(), 3)));
    let label = |scene: &mut Scene, text: &str, at: Vec2| {
        if let Some(p) = camera.world_to_screen(at.extend(0.0), viewport) {
            scene.overlay.texts.push(
                Text::new(text, p, 15.0)
                    .weight(600)
                    .color(palette::CREAM)
                    .align(Align::Center),
            );
        }
    };
    let bible = ArtBible::bundled();
    let station_mat = bible.material(InkRole::Station);
    let ship_mat = bible.material(InkRole::Ship);
    let rock_mat = bible.material(InkRole::Rock);
    // Row 1: the five stations, spaced so their silhouettes don't share a wheel.
    let stations = [
        (StationRecipe::lantern_wharf(), Vec2::new(-170.0, 62.0)),
        (StationRecipe::kettle_dock(3), Vec2::new(-85.0, 62.0)),
        (StationRecipe::cinder_hook(), Vec2::new(0.0, 62.0)),
        (
            StationRecipe::glass_drift_observatory(),
            Vec2::new(85.0, 62.0),
        ),
        (StationRecipe::brassmouth(), Vec2::new(170.0, 62.0)),
    ];
    for (recipe, pos) in stations {
        let mesh = renderer.upload_mesh(&kit::station(&recipe));
        scene
            .meshes
            .push(MeshInstance::at(mesh, pos, 0.0, 0.72).with_material(station_mat));
        label(
            &mut scene,
            &recipe.name,
            pos - Vec2::Y * (recipe.footprint() * 0.8),
        );
    }
    // Kettle Dock's greenhouse at each stage, close up.
    for stage in 0..4u8 {
        let recipe = StationRecipe {
            name: format!("stage {stage}"),
            seed: 2,
            palette: StationRecipe::kettle_dock(0).palette,
            faction: None,
            region: None,
            decals: vec![],
            parts: vec![kit::Part::new(
                kit::PartKind::Greenhouse { radius: 7.5, stage },
                kit::Placement::default(),
            )],
        };
        let mesh = renderer.upload_mesh(&kit::station(&recipe));
        let pos = Vec2::new(28.0 + stage as f32 * 36.0, -8.0);
        scene
            .meshes
            .push(MeshInstance::at(mesh, pos, 0.0, 1.0).with_material(station_mat));
        label(&mut scene, &stage.to_string(), pos - Vec2::Y * 12.0);
    }
    // Row 2: hulls.
    for (i, p) in [
        HullParams::wren(),
        HullParams::tandem(),
        HullParams::heron(),
    ]
    .iter()
    .enumerate()
    {
        let mesh = renderer.upload_mesh(&kit::hull(p, 1));
        let pos = Vec2::new(-108.0 + i as f32 * 34.0, -16.0);
        scene.meshes.push(
            MeshInstance::at(mesh, pos, std::f32::consts::FRAC_PI_2, 2.2).with_material(ship_mat),
        );
        label(&mut scene, &format!("{:?}", p.class), pos - Vec2::Y * 20.0);
    }
    // Row 3: rocks at full, half and nearly gone.
    let kinds = [RockKind::Ice, RockKind::Iron, RockKind::Glassite];
    for (k, kind) in kinds.iter().enumerate() {
        for (j, remaining) in [1.0, 0.6, 0.25, 0.0].iter().enumerate() {
            let mesh = renderer.upload_mesh(&kit::rock(
                &RockParams::new(*kind, 4.5, *remaining),
                40 + k as u64,
            ));
            let pos = Vec2::new(-120.0 + j as f32 * 16.0 + k as f32 * 80.0, -62.0);
            scene
                .meshes
                .push(MeshInstance::at(mesh, pos, 0.3, 1.0).with_material(rock_mat));
            if j == 0 {
                let text = format!("{kind:?} 100 / 60 / 25 / 0%");
                label(&mut scene, &text, pos + Vec2::new(22.0, -10.0));
            }
        }
    }
    scene
}
