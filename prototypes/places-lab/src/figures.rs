//! The station's people as figures in the cel-and-ink style, from the mesh builder's
//! primitives. Each is a body and a head (the head turns and nods on its own), with a
//! silhouette and colours of their own. Metres; feet at the origin, facing -Y (towards a
//! room's camera), Z up.

use render::glam::{Mat4, Quat, Vec3};
use render::{Color, MeshBuilder, MeshData, palette};

/// What a figure needs besides its meshes: where its head sits and its height.
#[derive(Clone, Copy, Debug)]
pub struct Build {
    /// The neck: the head mesh's origin, above the feet.
    pub neck: f32,
    /// The face's centre above the neck.
    pub face: f32,
    /// The top of the head or hat above the feet.
    pub top: f32,
    /// How wide the figure is, for hit areas.
    pub width: f32,
}

pub struct Figure {
    pub body: MeshData,
    pub head: MeshData,
    pub build: Build,
}

/// How a figure stands this frame: idle motion, turns and nods (`life` decides).
#[derive(Clone, Copy, Debug, Default)]
pub struct Pose {
    /// Breathing: the body's height scale, around 1.
    pub breathe: f32,
    /// The body's turn from facing the camera, radians.
    pub turn: f32,
    /// A lean to the side, radians.
    pub sway: f32,
    pub head_turn: f32,
    pub head_nod: f32,
    /// Lift off the floor (a hop, sitting is negative).
    pub lift: f32,
}

impl Pose {
    pub fn still() -> Pose {
        Pose {
            breathe: 1.0,
            ..Pose::default()
        }
    }

    /// Body and head transforms for a figure standing at `at` (feet), facing -Y turned
    /// by `facing` radians plus the pose's turn.
    pub fn transforms(&self, at: Vec3, facing: f32, build: &Build) -> (Mat4, Mat4) {
        let body = Mat4::from_translation(at + Vec3::Z * self.lift)
            * Mat4::from_rotation_z(facing + self.turn)
            * Mat4::from_rotation_y(self.sway)
            * Mat4::from_scale(Vec3::new(1.0, 1.0, self.breathe.max(0.5)));
        let neck = body.transform_point3(Vec3::Z * build.neck);
        let head = Mat4::from_translation(neck)
            * Mat4::from_rotation_z(facing + self.turn + self.head_turn)
            * Mat4::from_rotation_y(self.sway)
            * Mat4::from_rotation_x(self.head_nod);
        (body, head)
    }
}

fn srgb(hex: u32) -> Color {
    Color::hex(hex)
}

const SKIN_A: u32 = 0xe8b08a;
const SKIN_B: u32 = 0xb97a56;
const SKIN_C: u32 = 0x8a5a3c;

fn cuboid_at(b: &mut MeshBuilder, at: Vec3, size: Vec3) {
    b.with(Mat4::from_translation(at), |b| b.cuboid(size));
}

/// An arm hanging from the shoulder at `at`, angled out by `out` radians.
fn arm(b: &mut MeshBuilder, at: Vec3, len: f32, thick: f32, out: f32, color: Color, hand: Color) {
    b.with(
        Mat4::from_translation(at) * Mat4::from_rotation_y(out),
        |b| {
            b.paint(color, 0.0);
            b.with(Mat4::from_translation(Vec3::new(0.0, 0.0, -len * 0.5)), |b| {
                b.cuboid(Vec3::new(thick, thick, len))
            });
            b.paint(hand, 0.0);
            b.with(Mat4::from_translation(Vec3::new(0.0, 0.0, -len - thick * 0.3)), |b| {
                b.cuboid(Vec3::splat(thick * 0.9))
            });
        },
    );
}

/// A round head on the neck: a lathe, eyes as two ink dots on the front.
fn head(b: &mut MeshBuilder, r: f32, skin: Color) -> f32 {
    b.paint(skin, 0.0);
    b.frustum(r * 0.35, r * 0.35, r * 0.35, 6);
    b.with(Mat4::from_translation(Vec3::Z * r * 0.25), |b| {
        b.lathe(
            &[
                (0.0, 0.0),
                (r * 0.75, r * 0.2),
                (r, r * 0.75),
                (r * 0.9, r * 1.35),
                (r * 0.5, r * 1.75),
                (0.0, r * 1.85),
            ],
            10,
            0.0,
        )
    });
    b.paint(palette::INK, 0.0);
    for x in [-0.36, 0.36] {
        cuboid_at(
            b,
            Vec3::new(x * r, -r * 0.93, r * 1.15),
            Vec3::new(r * 0.16, r * 0.12, r * 0.22),
        );
    }
    // The face's centre above the neck.
    r * 1.05
}

/// Ilse: tall and narrow in a long teal coat that flares at the hem, a peaked square cap
/// and a cream scarf, the berth book under her arm.
fn ilse() -> Figure {
    let (coat, trim, skin) = (srgb(0x2f7f86), srgb(0x1f4f5e), srgb(SKIN_A));
    let mut b = MeshBuilder::new();
    b.paint(coat, 0.0);
    b.lathe(
        &[(0.42, 0.0), (0.34, 0.5), (0.24, 1.0), (0.26, 1.42), (0.12, 1.55)],
        8,
        0.0,
    );
    b.paint(srgb(0xf3e3c0), 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 1.42), |b| {
        b.torus(0.17, 0.12, 0.1, 10, 4)
    });
    b.paint(trim, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.9), |b| {
        b.frustum(0.275, 0.27, 0.06, 8)
    });
    arm(&mut b, Vec3::new(-0.3, 0.0, 1.38), 0.62, 0.13, -0.12, coat, skin);
    arm(&mut b, Vec3::new(0.3, 0.0, 1.38), 0.55, 0.13, 0.35, coat, skin);
    // The berth book against her hip.
    b.paint(srgb(0x9a4a2a), 0.0);
    cuboid_at(&mut b, Vec3::new(0.42, -0.12, 0.92), Vec3::new(0.06, 0.32, 0.42));
    let neck = 1.52;
    let mut h = MeshBuilder::new();
    let face = head(&mut h, 0.13, skin);
    h.paint(trim, 0.0);
    // A square cap with a peak.
    cuboid_at(&mut h, Vec3::new(0.0, 0.0, 0.33), Vec3::new(0.27, 0.27, 0.12));
    cuboid_at(&mut h, Vec3::new(0.0, -0.17, 0.29), Vec3::new(0.25, 0.14, 0.03));
    h.paint(palette::MUSTARD, 0.3);
    cuboid_at(&mut h, Vec3::new(0.0, -0.14, 0.34), Vec3::new(0.06, 0.02, 0.06));
    Figure {
        body: b.build(),
        head: h.build(),
        build: Build {
            neck,
            face,
            top: neck + 0.4,
            width: 0.85,
        },
    }
}

/// Orrin: short, broad and blocky in a rust apron, big forearms, a bald round head with
/// goggles pushed up on it.
fn orrin() -> Figure {
    let (shirt, apron, skin) = (srgb(0x5a6b3a), srgb(0xc0562e), srgb(SKIN_B));
    let mut b = MeshBuilder::new();
    b.paint(srgb(0x3b2f2a), 0.0);
    for x in [-0.17, 0.17] {
        cuboid_at(&mut b, Vec3::new(x, 0.0, 0.3), Vec3::new(0.22, 0.26, 0.6));
    }
    b.paint(shirt, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.58), |b| {
        b.lathe(&[(0.36, 0.0), (0.44, 0.4), (0.46, 0.7), (0.2, 0.8)], 6, 0.5)
    });
    b.paint(apron, 0.0);
    cuboid_at(&mut b, Vec3::new(0.0, -0.36, 0.78), Vec3::new(0.5, 0.06, 0.78));
    b.paint(palette::BRASS, 0.0);
    cuboid_at(&mut b, Vec3::new(0.0, -0.4, 0.8), Vec3::new(0.24, 0.04, 0.16));
    arm(&mut b, Vec3::new(-0.5, 0.0, 1.22), 0.58, 0.2, -0.25, shirt, skin);
    arm(&mut b, Vec3::new(0.5, 0.0, 1.22), 0.58, 0.2, 0.25, shirt, skin);
    let neck = 1.36;
    let mut h = MeshBuilder::new();
    let face = head(&mut h, 0.17, skin);
    // Goggles on the forehead.
    h.paint(palette::MUSTARD, 0.0);
    h.with(Mat4::from_translation(Vec3::Z * 0.27), |b| {
        b.torus(0.165, 0.05, 0.06, 12, 4)
    });
    h.paint(palette::GLASS, 0.4);
    for x in [-0.07, 0.07] {
        h.with(
            Mat4::from_translation(Vec3::new(x, -0.16, 0.3)) * Mat4::from_rotation_x(1.4),
            |b| b.frustum(0.055, 0.05, 0.05, 8),
        );
    }
    Figure {
        body: b.build(),
        head: h.build(),
        build: Build {
            neck,
            face,
            top: neck + 0.42,
            width: 1.1,
        },
    }
}

/// Bel: round as a kettle in a rose dress and a cream apron, a tall cook's hat.
fn bel() -> Figure {
    let (dress, apron, skin) = (srgb(0xc2577a), srgb(0xf6ead0), srgb(SKIN_C));
    let mut b = MeshBuilder::new();
    b.paint(dress, 0.0);
    b.lathe(
        &[
            (0.3, 0.0),
            (0.46, 0.35),
            (0.5, 0.75),
            (0.42, 1.1),
            (0.22, 1.32),
            (0.1, 1.36),
        ],
        10,
        0.0,
    );
    b.paint(apron, 0.0);
    b.with(
        Mat4::from_translation(Vec3::new(0.0, -0.44, 0.7)) * Mat4::from_rotation_x(-0.12),
        |b| b.cuboid(Vec3::new(0.46, 0.05, 0.62)),
    );
    arm(&mut b, Vec3::new(-0.42, 0.0, 1.12), 0.5, 0.15, -0.5, dress, skin);
    arm(&mut b, Vec3::new(0.42, 0.0, 1.12), 0.5, 0.15, 0.5, dress, skin);
    let neck = 1.32;
    let mut h = MeshBuilder::new();
    let face = head(&mut h, 0.15, skin);
    h.paint(srgb(0xfffaf0), 0.0);
    h.with(Mat4::from_translation(Vec3::Z * 0.27), |b| {
        b.lathe(
            &[(0.13, 0.0), (0.15, 0.18), (0.22, 0.3), (0.2, 0.4), (0.0, 0.44)],
            10,
            0.0,
        )
    });
    Figure {
        body: b.build(),
        head: h.build(),
        build: Build {
            neck,
            face,
            top: neck + 0.72,
            width: 1.0,
        },
    }
}

/// Juno: slim in a long mustard tunic, a green scarf trailing, a wide flat hat.
fn juno() -> Figure {
    let (tunic, scarf, skin) = (srgb(0xd9a03a), srgb(0x4f9a5a), srgb(SKIN_B));
    let mut b = MeshBuilder::new();
    b.paint(srgb(0x4a3550), 0.0);
    for x in [-0.1, 0.1] {
        cuboid_at(&mut b, Vec3::new(x, 0.0, 0.3), Vec3::new(0.13, 0.15, 0.6));
    }
    b.paint(tunic, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.5), |b| {
        b.lathe(&[(0.3, 0.0), (0.22, 0.45), (0.22, 0.9), (0.1, 1.02)], 8, 0.0)
    });
    b.paint(scarf, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 1.36), |b| {
        b.torus(0.15, 0.1, 0.1, 10, 4)
    });
    cuboid_at(&mut b, Vec3::new(0.12, 0.12, 1.08), Vec3::new(0.1, 0.05, 0.5));
    arm(&mut b, Vec3::new(-0.27, 0.0, 1.36), 0.62, 0.11, -0.08, tunic, skin);
    arm(&mut b, Vec3::new(0.27, 0.0, 1.36), 0.62, 0.11, 0.08, tunic, skin);
    let neck = 1.5;
    let mut h = MeshBuilder::new();
    let face = head(&mut h, 0.13, skin);
    h.paint(srgb(0x7a4a2a), 0.0);
    h.with(Mat4::from_translation(Vec3::Z * 0.3), |b| {
        b.frustum(0.46, 0.4, 0.04, 14)
    });
    h.with(Mat4::from_translation(Vec3::Z * 0.33), |b| {
        b.frustum(0.15, 0.13, 0.13, 10)
    });
    Figure {
        body: b.build(),
        head: h.build(),
        build: Build {
            neck,
            face,
            top: neck + 0.48,
            width: 0.95,
        },
    }
}

/// Pim: a small courier in a violet jacket with a satchel bigger than seems fair and a
/// knitted cap that ends in a bobble.
fn pim() -> Figure {
    let (jacket, bag, skin) = (srgb(0x6b4aa0), srgb(0x9a6a3a), srgb(SKIN_A));
    let mut b = MeshBuilder::new();
    b.paint(srgb(0x2a2a3a), 0.0);
    for x in [-0.09, 0.09] {
        cuboid_at(&mut b, Vec3::new(x, 0.0, 0.22), Vec3::new(0.12, 0.14, 0.44));
    }
    b.paint(jacket, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.42), |b| {
        b.lathe(&[(0.24, 0.0), (0.24, 0.4), (0.18, 0.55), (0.08, 0.6)], 8, 0.0)
    });
    arm(&mut b, Vec3::new(-0.24, 0.0, 0.9), 0.42, 0.1, -0.15, jacket, skin);
    arm(&mut b, Vec3::new(0.24, 0.0, 0.9), 0.42, 0.1, 0.15, jacket, skin);
    b.paint(bag, 0.0);
    cuboid_at(&mut b, Vec3::new(0.3, -0.05, 0.55), Vec3::new(0.16, 0.36, 0.3));
    b.with(
        Mat4::from_translation(Vec3::new(0.02, -0.02, 0.76)) * Mat4::from_rotation_y(-0.8),
        |b| b.cuboid(Vec3::new(0.05, 0.5, 0.06)),
    );
    let neck = 1.0;
    let mut h = MeshBuilder::new();
    let face = head(&mut h, 0.14, skin);
    h.paint(palette::TERRACOTTA, 0.0);
    h.with(Mat4::from_translation(Vec3::Z * 0.25), |b| {
        b.lathe(&[(0.15, 0.0), (0.13, 0.1), (0.0, 0.32)], 8, 0.0)
    });
    h.paint(palette::CREAM, 0.0);
    h.with(
        Mat4::from_rotation_translation(Quat::IDENTITY, Vec3::Z * 0.56),
        |b| b.lathe(&[(0.0, 0.0), (0.06, 0.05), (0.0, 0.11)], 6, 0.0),
    );
    Figure {
        body: b.build(),
        head: h.build(),
        build: Build {
            neck,
            face,
            top: neck + 0.67,
            width: 0.75,
        },
    }
}

/// A plain figure for a look the lab doesn't know.
fn stranger() -> Figure {
    let mut f = ilse();
    let mut b = MeshBuilder::new();
    b.paint(palette::SAGE, 0.0);
    b.lathe(&[(0.36, 0.0), (0.26, 1.0), (0.12, 1.5)], 8, 0.0);
    f.body = b.build();
    f
}

pub fn figure(look: &str) -> Figure {
    match look {
        "ilse" => ilse(),
        "orrin" => orrin(),
        "bel" => bel(),
        "juno" => juno(),
        "pim" => pim(),
        _ => stranger(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_figure_is_well_formed_and_distinct_in_height() {
        let mut tops = Vec::new();
        for look in ["ilse", "orrin", "bel", "juno", "pim"] {
            let f = figure(look);
            assert!(f.body.is_well_formed() && f.head.is_well_formed(), "{look}");
            assert!(f.build.top > f.build.neck, "{look}");
            tops.push((f.build.top * 100.0) as i32);
        }
        tops.sort();
        tops.dedup();
        assert_eq!(tops.len(), 5, "each silhouette its own height");
    }
}
