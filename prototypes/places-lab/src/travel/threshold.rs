//! A threshold: between any two places you pass down one short connecting corridor to a
//! door with the place's name over it. The door slides open on the place's light and you
//! step through. The same corridor and the same length for every room, in and out.

use render::glam::{Mat4, Vec2, Vec3};
use render::{
    Align, Camera, Color, Glow, MeshBuilder, MeshHandle, MeshInstance, PointLight, Renderer, Scene,
    Text, UiLayer, palette,
};

use super::Travel;
use super::shared::{self, DARK, smooth, span};
use crate::places::{Kind, Places};
use crate::stage::{self, Shown, Stage};
use crate::world::World;

/// Fading out of where you were, the corridor, and stepping out into the place
/// (seconds, from the start of the move).
const LEAVE: f32 = 0.15;
const ARRIVE: f32 = 1.05;
const TIME: f32 = 1.25;
/// The door opens over this stretch.
const DOOR: (f32, f32) = (0.5, 0.82);

/// The corridor, metres: from y = 0 to the door at `LEN`.
const LEN: f32 = 6.0;
const WIDTH: f32 = 2.4;
const HEIGHT: f32 = 2.6;
/// The doorway.
const GAP: Vec2 = Vec2::new(1.4, 2.2);

#[derive(Default)]
pub struct Threshold {
    from: String,
    to: String,
    t: f32,
    busy: bool,
    corridor: Option<MeshHandle>,
    leaf: Option<MeshHandle>,
    light: Option<MeshHandle>,
}

fn cube(b: &mut MeshBuilder, at: Vec3, size: Vec3) {
    b.with(Mat4::from_translation(at), |b| b.cuboid(size));
}

fn corridor() -> render::MeshData {
    let mut b = MeshBuilder::new();
    let (hw, wall) = (WIDTH * 0.5, 0.2);
    // Floor, walls and ceiling.
    b.paint(Color::hex(0x3b2d4a), 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, LEN * 0.5 - 1.0, -0.05),
        Vec3::new(WIDTH, LEN + 2.0, 0.1),
    );
    b.paint(Color::hex(0x6a5a80), 0.0);
    for side in [-1.0f32, 1.0] {
        cube(
            &mut b,
            Vec3::new(side * (hw + wall * 0.5), LEN * 0.5 - 1.0, HEIGHT * 0.5),
            Vec3::new(wall, LEN + 2.0, HEIGHT),
        );
    }
    b.paint(Color::hex(0x4a3c5c), 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, LEN * 0.5 - 1.0, HEIGHT + 0.05),
        Vec3::new(WIDTH + 0.4, LEN + 2.0, 0.1),
    );
    // A runner down the floor.
    b.paint(palette::MUSTARD.scale(0.55), 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, LEN * 0.5 - 1.0, 0.01),
        Vec3::new(0.7, LEN + 2.0, 0.02),
    );
    // Ribs: a frame round the corridor every metre and a bit, a lamp in each.
    let mut y = -0.6;
    while y < LEN - 0.5 {
        b.paint(Color::hex(0x2e7d74), 0.0);
        for side in [-1.0f32, 1.0] {
            cube(
                &mut b,
                Vec3::new(side * (hw - 0.06), y, HEIGHT * 0.5),
                Vec3::new(0.14, 0.22, HEIGHT),
            );
        }
        cube(
            &mut b,
            Vec3::new(0.0, y, HEIGHT - 0.06),
            Vec3::new(WIDTH, 0.22, 0.14),
        );
        b.paint(palette::CREAM, 1.0);
        cube(
            &mut b,
            Vec3::new(0.0, y + 0.6, HEIGHT - 0.02),
            Vec3::new(0.5, 0.12, 0.05),
        );
        y += 1.2;
    }
    // The end wall with the doorway, and its frame.
    b.paint(Color::hex(0x6a5a80), 0.0);
    let side_w = (WIDTH - GAP.x) * 0.5;
    for side in [-1.0f32, 1.0] {
        cube(
            &mut b,
            Vec3::new(side * (GAP.x * 0.5 + side_w * 0.5), LEN, HEIGHT * 0.5),
            Vec3::new(side_w, 0.2, HEIGHT),
        );
    }
    cube(
        &mut b,
        Vec3::new(0.0, LEN, (GAP.y + HEIGHT) * 0.5),
        Vec3::new(GAP.x, 0.2, HEIGHT - GAP.y),
    );
    b.paint(palette::MUSTARD, 0.0);
    for side in [-1.0f32, 1.0] {
        cube(
            &mut b,
            Vec3::new(side * (GAP.x * 0.5 + 0.06), LEN - 0.08, GAP.y * 0.5),
            Vec3::new(0.12, 0.12, GAP.y),
        );
    }
    cube(
        &mut b,
        Vec3::new(0.0, LEN - 0.08, GAP.y + 0.06),
        Vec3::new(GAP.x + 0.24, 0.12, 0.12),
    );
    // The sign board over the door (the name is drawn on it).
    b.paint(Color::hex(0x231a2e), 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, LEN - 0.12, GAP.y + 0.24),
        Vec3::new(1.3, 0.06, 0.2),
    );
    b.build()
}

fn leaf() -> render::MeshData {
    let mut b = MeshBuilder::new();
    b.paint(Color::hex(0xd7c49e), 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, 0.0, GAP.y * 0.5),
        Vec3::new(GAP.x * 0.5, 0.1, GAP.y),
    );
    b.paint(palette::MUSTARD, 0.0);
    cube(
        &mut b,
        Vec3::new(0.0, -0.06, GAP.y * 0.55),
        Vec3::new(GAP.x * 0.5, 0.02, 0.18),
    );
    b.build()
}

fn light() -> render::MeshData {
    let mut b = MeshBuilder::new();
    b.paint(Color::WHITE, 1.0);
    cube(&mut b, Vec3::ZERO, Vec3::new(3.0, 0.1, 3.0));
    b.build()
}

/// The light through the door: a room's warm lamplight, or the station's starlight.
fn light_of(stage: &Stage, id: &str) -> Color {
    match stage.place(id).map(|p| p.kind) {
        Some(Kind::Interior) => Color::hex(0xffe2b0),
        _ => Color::hex(0xc9b8f0),
    }
}

impl Threshold {
    /// Where you are in the corridor: 0 at its start, 1 through the door.
    fn walked(&self) -> f32 {
        span(self.t, LEAVE, ARRIVE)
    }

    fn camera(&self) -> Camera {
        let k = self.walked();
        let y = -0.8 + (LEN - 1.1 + 0.8) * smooth(k);
        let bob = (k * std::f32::consts::TAU * 3.0).sin() * 0.025;
        Camera::perspective(
            Vec3::new(0.0, y, 1.55 + bob),
            Vec3::new(0.0, y + 4.0, 1.5),
            58.0,
        )
    }

    fn corridor_view(&self, stage: &Stage) -> Shown {
        let mut scene = Scene::new(self.camera());
        scene.clear_color = DARK;
        scene.time = stage.world.time;
        stage::room_lights(&mut scene);
        let open = smooth(span(self.t, DOOR.0, DOOR.1));
        let glow = light_of(stage, &self.to);
        if let Some(m) = self.corridor {
            scene.meshes.push(MeshInstance::new(m, Mat4::IDENTITY));
        }
        if let Some(m) = self.leaf {
            for side in [-1.0f32, 1.0] {
                let x = side * (GAP.x * 0.25 + open * GAP.x * 0.5);
                scene.meshes.push(MeshInstance::new(
                    m,
                    Mat4::from_translation(Vec3::new(x, LEN + 0.06, 0.0)),
                ));
            }
        }
        if let Some(m) = self.light {
            let mut inst =
                MeshInstance::new(m, Mat4::from_translation(Vec3::new(0.0, LEN + 0.7, 1.4)));
            inst.material.tint = glow;
            inst.material.outline_px = 0.0;
            scene.meshes.push(inst);
        }
        scene.glows.push(Glow {
            position: Vec3::new(0.0, LEN + 0.2, 1.2),
            radius: 0.6 + 2.2 * open,
            color: glow.with_alpha(0.25 + 0.5 * open),
        });
        let mut y = 0.0;
        while y < LEN && scene.lights.points.len() < 6 {
            scene.lights.points.push(PointLight {
                position: Vec3::new(0.0, y, HEIGHT - 0.3),
                color: Color::hex(0xffd9a0).scale(0.7),
                radius: 2.6,
            });
            y += 1.2;
        }
        scene.lights.points.push(PointLight {
            position: Vec3::new(0.0, LEN - 0.4, 1.4),
            color: glow.scale(1.6 * open),
            radius: 4.0,
        });
        Shown::new(scene, &self.to)
    }
}

impl Travel for Threshold {
    fn name(&self) -> &'static str {
        "threshold"
    }

    fn setup(&mut self, r: &mut Renderer, _world: &World, _model: &Places) {
        for m in [self.corridor.take(), self.leaf.take(), self.light.take()]
            .into_iter()
            .flatten()
        {
            r.remove_mesh(m);
        }
        self.corridor = Some(r.upload_mesh(&corridor()));
        self.leaf = Some(r.upload_mesh(&leaf()));
        self.light = Some(r.upload_mesh(&light()));
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
            if self.t >= TIME {
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
        if !self.busy || self.t >= ARRIVE {
            stage.view(&stage.model.here)
        } else if self.t < LEAVE {
            stage.view(&self.from)
        } else {
            self.corridor_view(stage)
        }
    }

    fn overlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if !self.busy {
            return;
        }
        let v = stage.viewport;
        let light = light_of(stage, &self.to);
        if self.t < LEAVE {
            shared::veil(layer, v, DARK, smooth(self.t / LEAVE));
            return;
        }
        if self.t >= ARRIVE {
            shared::veil(layer, v, light, 1.0 - smooth(span(self.t, ARRIVE, TIME)));
            return;
        }
        // The place's name on the board over the door.
        let cam = self.camera();
        // Texts draw over panels, so the name fades with the veils itself.
        let dark = 1.0 - smooth(span(self.t, LEAVE, LEAVE + 0.12));
        let bright = smooth(span(self.t, ARRIVE - 0.15, ARRIVE));
        let fade = (1.0 - dark) * (1.0 - bright);
        if let Some(at) = cam.world_to_screen(Vec3::new(0.0, LEN - 0.2, GAP.y + 0.24), v) {
            let (name, _) = stage.title(&self.to);
            let size = (cam.world_per_pixel(v).recip() * 0.15).clamp(14.0, 64.0);
            layer.texts.push(
                Text::new(name, Vec2::new(at.x, at.y - size * 0.6), size)
                    .weight(700)
                    .align(Align::Center)
                    .color(palette::CREAM.with_alpha(fade)),
            );
        }
        shared::veil(layer, v, DARK, dark);
        shared::veil(layer, v, light, bright);
    }
}
