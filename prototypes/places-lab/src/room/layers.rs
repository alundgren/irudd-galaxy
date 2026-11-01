//! A flat staged scene in layers: each room is a few painted flats stood one behind the
//! other, like a toy theatre's, seen straight on through a frame of curtains. The people
//! stand among the flats, and a line or two of scene text says what the room is like
//! right now.
//!
//! Authoring a room is a short list of shapes at a few depths (the room functions at the
//! bottom): rectangles, discs and outlines, in the set's own floor coordinates (x to the
//! right, y away from the front, z up), so the people's spots fall between the right
//! flats. The floor, the back wall, the apron and the curtains come with every room.

use std::collections::HashMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

use render::art_bible::ArtMaterial;
use render::glam::{Mat4, Vec2, Vec3};
use render::{
    ArtBible, Camera, Color, Glow, InkRole, MAX_POINT_LIGHTS, MeshBuilder, MeshHandle,
    MeshInstance, PointLight, Renderer, Scene, palette,
};

use super::Room;
use crate::places::{Kind, Place, Places};
use crate::stage::{self, Point, Shown, Stage};
use crate::widgets::W;
use crate::world::World;

/// How far the camera stands back from the stage, metres: a long lens flattens the room
/// into layers.
const BACK: f32 = 32.0;
/// A flat's thickness, and how far each shape at one depth stands in front of the last,
/// so a later shape paints over an earlier one.
const THICK: f32 = 0.05;
const STEP: f32 = 0.025;
/// The aspect the curtains are hung for.
const ASPECT: f32 = 16.0 / 9.0;

fn hex(h: u32) -> Color {
    Color::hex(h)
}

/// What a room's flats are built with.
struct Flats {
    b: MeshBuilder,
    y: f32,
    lamps: Vec<(Vec3, f32)>,
}

impl Flats {
    fn new() -> Flats {
        Flats {
            b: MeshBuilder::new(),
            y: 0.0,
            lamps: Vec::new(),
        }
    }

    /// The shapes that follow stand at depth `y`.
    fn at(&mut self, y: f32) -> &mut Self {
        self.y = y;
        self
    }

    /// An upright flat cut to `outline` ((x, z), counter-clockwise, star-shaped). Window
    /// and lantern paint glow.
    fn shape(&mut self, color: Color, outline: &[Vec2]) -> &mut Self {
        let y = self.y;
        self.y -= STEP;
        self.b.paint(
            color,
            if color == palette::WINDOW || color == palette::LANTERN {
                1.0
            } else {
                0.0
            },
        );
        let up = Mat4::from_translation(Vec3::Y * y) * Mat4::from_rotation_x(FRAC_PI_2);
        self.b.with(up, |b| b.extrude(outline, THICK, 1.0));
        self
    }

    fn rect(&mut self, color: Color, x0: f32, z0: f32, x1: f32, z1: f32) -> &mut Self {
        let (x0, x1) = (x0.min(x1), x0.max(x1));
        let (z0, z1) = (z0.min(z1), z0.max(z1));
        self.shape(
            color,
            &[
                Vec2::new(x0, z0),
                Vec2::new(x1, z0),
                Vec2::new(x1, z1),
                Vec2::new(x0, z1),
            ],
        )
    }

    fn disc(&mut self, color: Color, x: f32, z: f32, r: f32) -> &mut Self {
        let pts: Vec<Vec2> = (0..14)
            .map(|i| {
                let a = i as f32 / 14.0 * TAU;
                Vec2::new(x + r * a.cos(), z + r * a.sin())
            })
            .collect();
        self.shape(color, &pts)
    }

    /// Any polygon by its corners.
    fn poly(&mut self, color: Color, pts: &[(f32, f32)]) -> &mut Self {
        let pts: Vec<Vec2> = pts.iter().map(|&(x, z)| Vec2::new(x, z)).collect();
        self.shape(color, &pts)
    }

    /// A lit lamp: a glowing disc, and a light just in front of it.
    fn lamp(&mut self, x: f32, z: f32, r: f32, reach: f32) -> &mut Self {
        self.disc(palette::LANTERN, x, z, r);
        self.lamps.push((Vec3::new(x, self.y - 0.4, z), reach));
        self
    }

    /// Something lying on the floor (a rug, a stripe), `x0..x1` by `y0..y1`.
    fn floor(&mut self, color: Color, x0: f32, y0: f32, x1: f32, y1: f32) -> &mut Self {
        self.b.paint(color, 0.0);
        let at = Vec3::new((x0 + x1) * 0.5, (y0 + y1) * 0.5, 0.01);
        self.b.with(Mat4::from_translation(at), |b| {
            b.cuboid(Vec3::new((x1 - x0).abs(), (y1 - y0).abs(), 0.02))
        });
        self
    }
}

/// What a room is framed to: the stage from `left` across `width` metres fills the
/// screen's width, the people in the part left of the room panel. The camera looks down
/// `pitch` degrees (a little, so the floor shows; the cradle more, to see along the
/// Wren). `hd` is half the floor's depth; the back wall stands at `back`.
struct Frame {
    left: f32,
    width: f32,
    pitch: f32,
    hd: f32,
    back: f32,
}

impl Frame {
    fn new(left: f32, width: f32, hd: f32) -> Frame {
        Frame {
            left,
            width,
            pitch: 5.0,
            hd,
            back: hd,
        }
    }

    /// The height of the picture at the stage, for `aspect`.
    fn height(&self, aspect: f32) -> f32 {
        self.width / aspect
    }

    /// Half the picture's height across a flat at depth `y`: nearer flats see less.
    fn half(&self, aspect: f32, y: f32) -> f32 {
        let p = self.pitch.to_radians();
        self.height(aspect) * 0.5 * (BACK + y * p.cos()) / BACK
    }

    /// The height the camera aims at, over the floor's middle: the floor's front edge
    /// sits near the bottom of the screen.
    fn aim(&self, aspect: f32) -> f32 {
        let p = self.pitch.to_radians();
        (self.half(aspect, -self.hd) - 0.35 - self.hd * p.sin()) / p.cos()
    }

    /// Where the top of the screen cuts a flat standing at depth `y`.
    fn top(&self, y: f32) -> f32 {
        let p = self.pitch.to_radians();
        self.aim(ASPECT) + (self.half(ASPECT, y) - y * p.sin()) / p.cos()
    }
}

/// The colours of a room's stage.
struct Paint {
    floor: Color,
    wall: Color,
    trim: Color,
}

/// What every room stands on and in: the floor boards, the back wall with its skirting
/// and rail, the apron in front, and the curtains framing it all.
fn stage_box(f: &mut Flats, fr: &Frame, p: &Paint) {
    let hd = fr.hd;
    let (x0, x1) = (fr.left - 2.0, fr.left + fr.width + 2.0);
    let back = fr.top(fr.back);
    let top = back + 3.0;
    // The floor, with a few darker boards across it.
    f.b.paint(p.floor, 0.0);
    f.b.with(
        Mat4::from_translation(Vec3::new((x0 + x1) * 0.5, 0.0, -0.15)),
        |b| b.cuboid(Vec3::new(x1 - x0, hd * 2.0 + 0.6, 0.3)),
    );
    let mut y = -hd + 0.7;
    while y < hd {
        f.floor(p.floor.scale(0.8), x0, y, x1, y + 0.06);
        y += 1.3;
    }
    // The back wall: one flat, its skirting and rail painted on.
    f.at(fr.back + 0.3)
        .rect(p.wall, x0, 0.0, x1, top)
        .rect(p.wall.scale(0.82), x0, back - 1.0, x1, top)
        .rect(p.trim, x0, 0.0, x1, 0.28)
        .rect(p.trim, x0, back - 1.08, x1, back - 0.92);
    // The apron: the stage's front edge, below the floor.
    f.at(-hd)
        .rect(hex(0x3a2418), x0, -3.0, x1, 0.0)
        .rect(palette::BRASS, x0, -0.12, x1, 0.0);
    // The curtains: a scalloped valance along the top and a drape at each side.
    let front = -hd - 0.4;
    let (red, dark) = (hex(0x8c2f3a), hex(0x5e1d2a));
    let vz = fr.top(front) - 0.45;
    f.at(front).rect(red, x0, vz, x1, top + 2.0);
    let n = (fr.width / 1.1).round().max(4.0) as i32;
    let w = fr.width / n as f32;
    for i in 0..n {
        let cx = fr.left + (i as f32 + 0.5) * w;
        let pts: Vec<(f32, f32)> = (0..=8)
            .map(|k| {
                let a = PI + k as f32 / 8.0 * PI;
                (cx + w * 0.5 * a.cos(), vz + w * 0.32 * a.sin())
            })
            .collect();
        f.poly(red, &pts);
    }
    f.rect(palette::MUSTARD, x0, vz + 0.1, x1, vz + 0.2);
    for (a, b) in [(x0, fr.left + 0.35), (fr.left + fr.width - 0.35, x1)] {
        f.at(front - 0.1).rect(red, a, -3.0, b, top);
        let mut x = a + 0.4;
        while x < b - 0.1 {
            f.rect(dark, x, -3.0, x + 0.07, top);
            x += 0.45;
        }
    }
}

/// A room's flats, uploaded.
struct Picture {
    mesh: MeshHandle,
    lamps: Vec<(Vec3, f32)>,
    frame: Frame,
    /// Where the Wren rests, its length, and which way its nose points (radians).
    ship: Option<(Vec2, f32, f32)>,
}

#[derive(Default)]
pub struct Layers {
    pictures: HashMap<String, Picture>,
}

impl Room for Layers {
    fn name(&self) -> &'static str {
        "layers"
    }

    fn setup(&mut self, r: &mut Renderer, _world: &World, model: &Places) {
        for (_, p) in self.pictures.drain() {
            r.remove_mesh(p.mesh);
        }
        for p in model.places.iter().filter(|p| p.kind == Kind::Interior) {
            let mut f = Flats::new();
            let hd = p.floor.y * 0.5;
            let (frame, ship) = match p.set.as_deref().unwrap_or("") {
                "office" => (office(&mut f, hd), None),
                "market" => (market(&mut f, hd), None),
                "cradle" => cradle(&mut f, hd),
                "canteen" => (canteen(&mut f, hd), None),
                _ => (bare(&mut f, hd, p.floor.x), None),
            };
            let mesh = r.upload_mesh(&f.b.build());
            self.pictures.insert(
                p.id.clone(),
                Picture {
                    mesh,
                    lamps: f.lamps,
                    frame,
                    ship,
                },
            );
        }
    }

    fn view(&self, stage: &Stage, place: &Place) -> Shown {
        let Some(pic) = self.pictures.get(&place.id) else {
            return Shown::new(
                Scene::new(Camera::perspective(Vec3::NEG_Y, Vec3::ZERO, 30.0)),
                &place.id,
            );
        };
        let fr = &pic.frame;
        // The picture's width fills the screen; a slow sway of the eye shows the layers
        // sliding over each other.
        let aspect = stage.aspect();
        let fov = 2.0 * (fr.height(aspect) * 0.5 / BACK).atan().to_degrees();
        let target = Vec3::new(fr.left + fr.width * 0.5, 0.0, fr.aim(aspect));
        // Less in the cradle: its bubble, the scene text and the Wren's labels are tight.
        let amp = if place.set.as_deref() == Some("cradle") { 0.2 } else { 0.7 };
        let sway = (stage.world.time * 0.3).sin() * amp;
        let p = fr.pitch.to_radians();
        let eye = target + Vec3::new(sway, -BACK * p.cos(), BACK * p.sin());
        let mut shown = Shown::new(Scene::new(Camera::perspective(eye, target, fov)), &place.id);
        shown.scene.clear_color = hex(0x1b1424);
        shown.scene.time = stage.world.time;
        stage::room_lights(&mut shown.scene);
        let bible = ArtBible::bundled();
        shown.scene.meshes.push(
            MeshInstance::new(pic.mesh, Mat4::IDENTITY)
                .with_material(bible.material(InkRole::Station)),
        );
        for &(at, reach) in &pic.lamps {
            if shown.scene.lights.points.len() < MAX_POINT_LIGHTS {
                shown.scene.lights.points.push(PointLight {
                    position: at,
                    color: palette::LANTERN,
                    radius: reach,
                });
            }
            shown.scene.glows.push(Glow {
                position: at,
                radius: 0.6,
                color: palette::LANTERN.with_alpha(0.55),
            });
        }
        cast(stage, place, &mut shown);
        if let Some((center, span, heading)) = pic.ship {
            let mut pose = stage.world.cradle_pose(center, span);
            pose.heading = heading;
            stage
                .world
                .cradle(pose)
                .draw(&mut shown.scene, stage.viewport);
            shown.cradle = Some(pose);
        }
        shown
    }

    fn widgets(&self, place: &Place, model: &Places) -> Vec<W> {
        let lines = scene_text(place, model);
        if lines.is_empty() {
            return Vec::new();
        }
        let mut panel = W::new("panel")
            .set("id", "scene_text")
            .set("role", "status")
            .set("place", "top-left")
            .set("width", 400)
            .set("gap", 4);
        for l in lines {
            panel = panel.child(W::text(l).set("style", "small"));
        }
        vec![panel]
    }
}

/// The people in the room among the flats, with the points the screen attaches to: over
/// each head, each face, and the floor spots.
fn cast(stage: &Stage, place: &Place, shown: &mut Shown) {
    let mat = ArtBible::bundled().material(InkRole::Prop);
    for who in stage.model.people_in(&place.id) {
        let Some(fig) = stage.world.figures.get(&who.id) else {
            continue;
        };
        let talking = stage.speaking == Some(who.id.as_str());
        let pose = stage.life.pose(who, stage.world.time + fig.phase, talking);
        let feet = stage.model.floor_pos(who).extend(0.0);
        let (body, head) = pose.transforms(feet, 0.0, &fig.build);
        shown
            .scene
            .meshes
            .push(MeshInstance::new(fig.body, body).with_material(mat));
        shown
            .scene
            .meshes
            .push(MeshInstance::new(fig.head, head).with_material(mat));
        shown.points.push(Point {
            name: format!("person.{}", who.id),
            at: feet + Vec3::Z * (fig.build.top + 0.12 + pose.lift),
            radius: fig.build.width * 0.45,
        });
        shown.points.push(Point {
            name: format!("person.{}.face", who.id),
            at: head.transform_point3(Vec3::Z * fig.build.face),
            radius: 0.16,
        });
    }
    for (name, p) in &place.spots {
        shown.points.push(Point {
            name: format!("spot.{name}"),
            at: p.extend(1.0),
            radius: 0.5,
        });
    }
}

/// What the room is like right now: a line or two, for whoever is in it.
fn scene_text(place: &Place, model: &Places) -> Vec<String> {
    let here = |id: &str| model.people_in(&place.id).any(|p| p.id == id);
    let state = |id: &str| model.person(id).map(|p| p.state.as_str()).unwrap_or("");
    let mut out = Vec::new();
    match place.set.as_deref().unwrap_or("") {
        "office" => {
            out.push(
                "Lamplight on the berth book. Three pegs in the board, one berth open.".into(),
            );
            out.push(if here("ilse") {
                "Ilse keeps one eye on the board and one on the door.".into()
            } else {
                "The desk is empty; the book lies open where she left it.".into()
            });
        }
        "market" => {
            out.push("Ice sweats on the counter. The ore heap still smells of the drift.".into());
            if here("pim") {
                out.push("Pim's parcels lean against the crates, string still knotted.".into());
            } else {
                out.push("A gap by the crates, where the courier's parcels go.".into());
            }
        }
        "cradle" => {
            out.push("The bay doors stand open on the dark. Cold air, warm metal.".into());
            out.push("The Wren rests in the cradle arms, ticking as it cools.".into());
        }
        "canteen" => {
            out.push("The broth is on. Steam fogs the porthole glass.".into());
            out.push(if here("pim") && state("pim") == "eat" {
                "Pim is bent over a bowl at the far table.".into()
            } else {
                "Pim's stool is empty, the bowl wiped and stacked.".into()
            });
        }
        _ => {}
    }
    out
}

// The rooms. Each is a few depths of flats; the spots in `lab/places.luau` fall between
// them.

/// Ilse's dock office: the berth board and a porthole on the back wall, the desk in
/// front of her, ledgers on a case at the side, a chair for visitors.
fn office(f: &mut Flats, hd: f32) -> Frame {
    let fr = Frame::new(-4.2, 9.5, hd);
    let wood = hex(0x7a4a2a);
    stage_box(
        f,
        &fr,
        &Paint {
            floor: hex(0xb8643c),
            wall: palette::HULL_CREAM,
            trim: hex(0x2f7f86),
        },
    );
    // On the back wall: the porthole and the berth board, three berths taken.
    f.at(hd + 0.2)
        .disc(palette::BRASS, -2.6, 2.0, 0.66)
        .disc(palette::SPACE_DEEP, -2.6, 2.0, 0.5)
        .rect(palette::WINDOW, -2.8, 2.2, -2.72, 2.28)
        .rect(palette::WINDOW, -2.4, 1.8, -2.34, 1.86)
        .rect(hex(0x3b4a5a), 0.2, 1.2, 2.8, 2.5);
    let pegs = [
        Some(palette::CORAL),
        Some(palette::TEAL),
        None,
        Some(palette::BLOSSOM),
    ];
    for (i, peg) in pegs.iter().enumerate() {
        let x = 0.35 + i as f32 * 0.6;
        f.rect(palette::CREAM, x, 1.35, x + 0.45, 2.35);
        if let Some(c) = peg {
            f.rect(*c, x + 0.12, 1.95, x + 0.33, 2.16);
        }
    }
    // The ledger case at the side.
    f.at(1.0).rect(wood, -4.3, 0.0, -3.2, 2.7);
    for (i, z) in [0.15, 0.95, 1.75].iter().enumerate() {
        f.rect(hex(0x4a2a18), -4.2, *z, -3.3, z + 0.7);
        for k in 0..5 {
            let c = [palette::TERRACOTTA, palette::TEAL, palette::MUSTARD][(k + i) % 3];
            let x = -4.15 + k as f32 * 0.17;
            f.rect(c, x, *z, x + 0.14, z + 0.5 + (k % 2) as f32 * 0.1);
        }
    }
    // The desk in front of Ilse, the berth book open on it and the lamp.
    f.at(1.3)
        .rect(wood, -2.0, 0.0, 0.4, 0.82)
        .rect(hex(0x5a3420), -2.15, 0.78, 0.55, 0.92)
        .rect(hex(0x5a3420), -1.0, 0.3, -0.6, 0.38)
        .poly(
            palette::PARCHMENT,
            &[(-0.9, 0.92), (-0.15, 0.92), (-0.25, 1.06), (-0.8, 1.06)],
        )
        .rect(palette::BRASS, -1.72, 0.92, -1.58, 1.28)
        .poly(
            palette::BRASS,
            &[(-1.98, 1.25), (-1.32, 1.25), (-1.48, 1.55), (-1.82, 1.55)],
        )
        .lamp(-1.65, 1.3, 0.12, 6.0);
    // The visitor's chair.
    f.at(-0.1)
        .rect(palette::TEAL, 0.65, 0.45, 0.72, 1.05)
        .rect(palette::TEAL, 1.08, 0.45, 1.15, 1.05)
        .rect(palette::TEAL, 0.65, 0.85, 1.15, 1.05)
        .rect(palette::TEAL, 0.6, 0.4, 1.2, 0.52)
        .rect(palette::TEAL, 0.64, 0.0, 0.7, 0.4)
        .rect(palette::TEAL, 1.1, 0.0, 1.16, 0.4);
    // The door out to the dock.
    f.at(0.8)
        .rect(hex(0x2f7f86), 3.0, 0.0, 4.2, 2.35)
        .rect(hex(0x2a1f36), 3.15, 0.0, 4.05, 2.2);
    fr
}

/// Juno's market: jars on shelves and bunting on the back wall, the counter with the
/// day's wares, crates stacked at the side, a lamp hung low.
fn market(f: &mut Flats, hd: f32) -> Frame {
    let fr = Frame::new(-4.6, 10.0, hd);
    let wood = hex(0x7a4a2a);
    stage_box(
        f,
        &fr,
        &Paint {
            floor: hex(0x9a6a3a),
            wall: palette::SAGE,
            trim: wood,
        },
    );
    // Shelves of jars, and bunting along the top.
    f.at(hd + 0.2);
    let jars = [
        palette::CORAL,
        palette::MUSTARD,
        palette::LEAF,
        palette::BLOSSOM,
    ];
    for row in 0..3 {
        let z = 1.2 + row as f32 * 0.6;
        f.rect(wood, -3.5, z - 0.06, 0.7, z);
        for i in 0..8 {
            let x = -3.3 + i as f32 * 0.5;
            f.rect(jars[(i + row) % 4], x, z, x + 0.26, z + 0.34).rect(
                palette::CREAM,
                x + 0.04,
                z + 0.34,
                x + 0.22,
                z + 0.42,
            );
        }
    }
    f.rect(wood, fr.left - 1.0, 3.48, fr.left + fr.width + 1.0, 3.52);
    let mut x = fr.left + 0.2;
    let mut i = 0;
    while x < fr.left + fr.width {
        let c = [palette::CORAL, palette::MUSTARD, palette::TEAL][i % 3];
        f.poly(c, &[(x, 3.5), (x + 0.2, 3.1), (x + 0.4, 3.5)]);
        x += 0.62;
        i += 1;
    }
    // Crates stacked at the side, by where Pim leaves the parcels.
    f.at(0.4)
        .rect(palette::TERRACOTTA, -4.5, 0.0, -3.7, 0.8)
        .rect(palette::TEAL, -4.4, 0.8, -3.6, 1.6)
        .rect(palette::MUSTARD, -3.6, 0.0, -3.05, 0.55)
        .rect(palette::TERRACOTTA.scale(0.7), -4.5, 0.35, -3.7, 0.45)
        .rect(palette::TEAL.scale(0.7), -4.4, 1.15, -3.6, 1.25);
    // The counter in front of Juno, with ice, ore and plate on it.
    f.at(1.3)
        .rect(wood, -3.4, 0.0, 0.2, 0.85)
        .rect(wood.scale(0.75), -3.3, 0.38, 0.1, 0.44)
        .rect(palette::MUSTARD, -3.55, 0.82, 0.35, 0.95)
        .rect(palette::GLASS, -3.2, 0.95, -2.85, 1.3)
        .rect(palette::GLASS, -2.78, 0.95, -2.5, 1.22)
        .poly(hex(0x8a5a4a), &[(-0.75, 0.95), (0.0, 0.95), (-0.3, 1.32)])
        .rect(hex(0xb0b8c0), -0.1, 0.95, 0.3, 1.15);
    // A lamp hung over the stall.
    f.at(1.6)
        .rect(palette::INK, -1.02, 2.9, -0.98, 5.0)
        .poly(
            palette::BRASS,
            &[(-1.35, 2.7), (-0.65, 2.7), (-0.85, 2.95), (-1.15, 2.95)],
        )
        .lamp(-1.0, 2.66, 0.1, 7.0);
    // The door.
    f.at(-0.2)
        .rect(wood, 3.6, 0.0, 4.7, 2.35)
        .rect(hex(0x2a1f36), 3.75, 0.0, 4.55, 2.2);
    fr
}

/// Orrin's cradle: a high bay, the bay doors open on space behind the Wren, the cradle
/// arms under it, Orrin's bench and pegboard at the side.
fn cradle(f: &mut Flats, hd: f32) -> (Frame, Option<(Vec2, f32, f32)>) {
    // Seen from higher up than the other rooms, so the Wren's sockets stack up the
    // screen with room for their labels. The back wall stands just behind the Wren, and
    // the stage's front edge just in front of the door: the rest of the floor is bare.
    let fr = Frame {
        pitch: 32.0,
        back: 2.8,
        ..Frame::new(-6.8, 15.0, hd.min(2.6))
    };
    let (x, y) = (-1.9, 0.4);
    stage_box(
        f,
        &fr,
        &Paint {
            floor: hex(0x4a4a5a),
            wall: hex(0x2f6f78),
            trim: palette::MUSTARD,
        },
    );
    // The bay doors behind the Wren, open on the dark, hazard stripes along the top.
    f.at(fr.back + 0.2)
        .rect(palette::MUSTARD, x - 3.4, 0.3, x + 3.4, 4.3)
        .rect(palette::SPACE_DEEP, x - 3.1, 0.5, x + 3.1, 3.8);
    for (sx, sz) in [
        (-2.6, 3.3),
        (-1.2, 2.7),
        (0.6, 3.5),
        (2.2, 2.2),
        (-2.0, 1.2),
        (1.6, 0.9),
    ] {
        f.rect(palette::WINDOW, x + sx, sz, x + sx + 0.09, sz + 0.09);
    }
    for i in 0..7 {
        let c = if i % 2 == 0 {
            palette::MUSTARD
        } else {
            palette::INK
        };
        let x0 = x - 3.4 + i as f32 * (6.8 / 7.0);
        f.rect(c, x0, 3.85, x0 + 6.8 / 7.0, 4.25);
    }
    // The pegboard over the bench, its tools hung.
    f.at(1.6).rect(hex(0x8a6a4a), -6.6, 1.3, -4.3, 2.6);
    for i in 0..5 {
        let x0 = -6.3 + i as f32 * 0.42;
        f.rect(
            hex(0xb0b8c0),
            x0,
            1.55 + (i % 2) as f32 * 0.2,
            x0 + 0.09,
            2.35,
        );
    }
    // The pad: hazard stripes on the floor, and the cradle arms across it.
    for (x0, y0, x1, y1) in [
        (x - 1.7, y - 2.5, x + 1.7, y - 2.35),
        (x - 1.7, y + 2.35, x + 1.7, y + 2.5),
        (x - 1.7, y - 2.5, x - 1.55, y + 2.5),
        (x + 1.55, y - 2.5, x + 1.7, y + 2.5),
    ] {
        f.floor(palette::MUSTARD, x0, y0, x1, y1);
    }
    for dy in [-1.0, 1.2] {
        f.at(y + dy)
            .rect(hex(0x6a5a4a), x - 1.3, 0.0, x + 1.3, 0.35)
            .rect(hex(0x6a5a4a), x - 1.0, 0.35, x - 0.7, 0.7)
            .rect(hex(0x6a5a4a), x + 0.7, 0.35, x + 1.0, 0.7);
    }
    // Orrin's bench, the vise and a crate of parts on it.
    f.at(0.0)
        .rect(hex(0x7a4a2a), -6.6, 0.0, -4.6, 0.9)
        .rect(hex(0x5a3420), -6.7, 0.85, -4.5, 0.98)
        .rect(palette::BRASS, -6.3, 0.98, -5.8, 1.2)
        .rect(palette::CORAL, -5.2, 0.98, -4.8, 1.16);
    // A cage of spare parts on the right.
    f.at(2.0)
        .rect(palette::TEAL, 1.6, 0.0, 3.0, 1.4)
        .rect(palette::CORAL, 2.0, 1.4, 2.6, 1.8);
    // A lamp over the bench.
    f.at(1.4).lamp(-5.4, 2.8, 0.1, 6.0);
    (fr, Some((Vec2::new(x, y), 3.6, FRAC_PI_2)))
}

/// Bel's canteen: the serving counter with the broth pot steaming, a porthole, a round
/// table with its stools, lamps hung low.
fn canteen(f: &mut Flats, hd: f32) -> Frame {
    let fr = Frame::new(-3.9, 11.0, hd);
    let wood = hex(0x7a4a2a);
    stage_box(
        f,
        &fr,
        &Paint {
            floor: hex(0xa8443a),
            wall: palette::MUSTARD.scale(0.85),
            trim: wood,
        },
    );
    f.at(hd + 0.2)
        .disc(wood, 0.6, 2.0, 0.7)
        .disc(palette::SPACE_DEEP, 0.6, 2.0, 0.55)
        .rect(palette::WINDOW, 0.4, 2.2, 0.48, 2.28)
        .rect(palette::WINDOW, 0.8, 1.8, 0.86, 1.86);
    // The serving counter in front of Bel: the pot steaming, bowls stacked.
    f.at(2.0)
        .rect(wood, -3.6, 0.0, -0.4, 1.0)
        .rect(palette::CREAM, -3.75, 0.98, -0.25, 1.1)
        .poly(
            hex(0x5a5a6a),
            &[(-3.45, 1.1), (-2.75, 1.1), (-2.7, 1.5), (-3.5, 1.5)],
        )
        .rect(hex(0x5a5a6a), -3.55, 1.5, -2.65, 1.58)
        .disc(palette::CREAM, -3.05, 1.82, 0.12)
        .disc(palette::CREAM, -2.95, 2.08, 0.09)
        .disc(palette::CREAM, -3.05, 2.3, 0.06)
        .poly(
            palette::TEAL,
            &[(-1.3, 1.1), (-0.9, 1.1), (-0.85, 1.2), (-1.35, 1.2)],
        )
        .poly(
            palette::TEAL,
            &[(-1.3, 1.22), (-0.9, 1.22), (-0.85, 1.32), (-1.35, 1.32)],
        );
    // Checked tiles in front of the counter.
    for i in 0..7 {
        let x = -3.6 + i as f32 * 0.46;
        if i % 2 == 0 {
            f.floor(palette::CREAM, x, 1.0, x + 0.46, 1.46);
        }
    }
    // The table Pim eats at, just behind him, and its stools.
    f.at(0.95)
        .rect(wood, 1.65, 0.0, 1.85, 0.72)
        .rect(wood, 1.0, 0.68, 2.5, 0.8)
        .poly(
            palette::CREAM,
            &[(1.5, 0.8), (1.9, 0.8), (1.85, 0.9), (1.55, 0.9)],
        )
        .rect(palette::TERRACOTTA, 0.55, 0.0, 0.9, 0.45)
        .rect(palette::TERRACOTTA, 2.6, 0.0, 2.95, 0.45);
    // Lamps hung over the counter and the table.
    for x in [-2.0, 1.75] {
        f.at(1.0)
            .rect(palette::INK, x - 0.02, 2.75, x + 0.02, 6.0)
            .poly(
                palette::BRASS,
                &[
                    (x - 0.35, 2.55),
                    (x + 0.35, 2.55),
                    (x + 0.12, 2.8),
                    (x - 0.12, 2.8),
                ],
            )
            .lamp(x, 2.52, 0.1, 5.0);
    }
    // The door.
    f.at(1.4)
        .rect(wood, 3.6, 0.0, 4.7, 2.35)
        .rect(hex(0x2a1f36), 3.75, 0.0, 4.55, 2.2);
    fr
}

/// A bare stage for a room the lab doesn't know.
fn bare(f: &mut Flats, hd: f32, width: f32) -> Frame {
    let fr = Frame::new(-width * 0.5, width, hd);
    stage_box(
        f,
        &fr,
        &Paint {
            floor: hex(0x6a5a4a),
            wall: palette::HULL_CREAM,
            trim: palette::TEAL,
        },
    );
    fr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_room_is_well_formed() {
        type Build = fn(&mut Flats, f32) -> Frame;
        let rooms: [Build; 3] = [office, market, canteen];
        for room in rooms {
            let mut f = Flats::new();
            room(&mut f, 3.0);
            let m = f.b.build();
            assert!(m.is_well_formed() && !m.is_empty());
        }
        let mut f = Flats::new();
        cradle(&mut f, 4.5);
        assert!(f.b.build().is_well_formed());
    }
}
