//! Small meshes for a station's life (issue #47, 4.1): residents, stock piles, lamp
//! posts, the cargo crane and the banner pole. Modelled in station units (a resident is
//! about two tall, a station hub about thirteen across), drawn at the station's scale.
//!
//! Each takes the theme's motif, so a Compact's bins are square and a Cooperative's
//! barrels round, and its paint. They are all quiet shapes: nothing here glows except a
//! lit lamp.

use super::station::StationPalette;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use glam::{Mat4, Quat, Vec3};
use std::f32::consts::TAU;

/// A theme's shape language, from the art bible's faction motifs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum Motif {
    #[default]
    Round,
    Square,
    Pointed,
}

impl Motif {
    /// The art bible's name for it: `round`, `square` or `pointed`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "round" => Self::Round,
            "square" => Self::Square,
            "pointed" => Self::Pointed,
            _ => return None,
        })
    }
}

/// What a stock pile is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum StockStyle {
    /// Timber crates and barrels.
    #[default]
    Crates,
    /// Square brass bins with lids.
    Bins,
    /// Tall glass jars in a cage.
    Jars,
}

/// How tall a resident is, in station units.
pub const RESIDENT_HEIGHT: f32 = 2.5;
/// One stock unit's size (a crate's side), in station units.
pub const CRATE_SIZE: f32 = 1.3;

fn bar(b: &mut MeshBuilder, from: Vec3, to: Vec3, thick: f32) {
    let d = to - from;
    let len = d.length();
    if len < 1e-4 {
        return;
    }
    let rot = Quat::from_rotation_arc(Vec3::X, d / len);
    b.with(
        Mat4::from_scale_rotation_translation(Vec3::new(len, thick, thick), rot, (from + to) * 0.5),
        |b| b.cuboid(Vec3::ONE),
    );
}

/// A resident: a coat, a head and a hat in the theme's shape. Feet at the origin, facing
/// +X. `coat` is their trade's colour, `trim` the hat and belt.
pub fn resident(coat: Color, trim: Color, skin: Color, motif: Motif) -> MeshData {
    let mut b = MeshBuilder::new();
    let h = RESIDENT_HEIGHT;
    b.paint(coat, 0.0);
    // A coat that flares to the hem: a simple lathe reads as a person from above.
    b.lathe(
        &[
            (h * 0.24, 0.0),
            (h * 0.2, h * 0.45),
            (h * 0.15, h * 0.66),
            (h * 0.06, h * 0.7),
        ],
        8,
        0.0,
    );
    b.paint(skin, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * h * 0.68), |b| {
        b.lathe(
            &[
                (0.0, 0.0),
                (h * 0.11, h * 0.04),
                (h * 0.13, h * 0.12),
                (h * 0.1, h * 0.2),
                (0.0, h * 0.24),
            ],
            8,
            0.0,
        )
    });
    b.paint(trim, 0.0);
    b.with(
        Mat4::from_translation(Vec3::Z * h * 0.86),
        |b| match motif {
            Motif::Round => b.lathe(
                &[(h * 0.17, 0.0), (h * 0.1, h * 0.06), (0.0, h * 0.1)],
                8,
                0.0,
            ),
            Motif::Square => {
                b.with(Mat4::from_translation(Vec3::Z * h * 0.05), |b| {
                    b.cuboid(Vec3::new(h * 0.22, h * 0.22, h * 0.1))
                });
            }
            Motif::Pointed => b.lathe(&[(h * 0.13, 0.0), (0.0, h * 0.3)], 6, 0.0),
        },
    );
    // A belt, so the figure has a waist from the side.
    b.with(Mat4::from_translation(Vec3::Z * h * 0.42), |b| {
        b.frustum(h * 0.205, h * 0.2, h * 0.05, 8)
    });
    b.build()
}

/// One unit of stock: a crate, a bin or a jar, sitting on the deck at the origin.
pub fn stock_unit(style: StockStyle, paint: Color, trim: Color) -> MeshData {
    let mut b = MeshBuilder::new();
    let s = CRATE_SIZE;
    match style {
        StockStyle::Crates => {
            b.paint(paint, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * s * 0.5), |b| {
                b.cuboid(Vec3::splat(s))
            });
            b.paint(trim, 0.0);
            for y in [-0.3, 0.3] {
                bar(
                    &mut b,
                    Vec3::new(-s * 0.51, y * s, s * 1.01),
                    Vec3::new(s * 0.51, y * s, s * 1.01),
                    s * 0.1,
                );
            }
        }
        StockStyle::Bins => {
            b.paint(paint, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * s * 0.45), |b| {
                b.cuboid(Vec3::new(s, s, s * 0.9))
            });
            b.paint(trim, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * s * 0.95), |b| {
                b.cuboid(Vec3::new(s * 1.08, s * 1.08, s * 0.14))
            });
        }
        StockStyle::Jars => {
            b.paint(paint, 0.0);
            b.lathe(
                &[
                    (s * 0.38, 0.0),
                    (s * 0.45, s * 0.5),
                    (s * 0.3, s * 1.1),
                    (s * 0.22, s * 1.2),
                ],
                8,
                0.0,
            );
            b.paint(trim, 0.0);
            b.with(Mat4::from_translation(Vec3::Z * s * 1.2), |b| {
                b.frustum(s * 0.26, s * 0.22, s * 0.16, 8)
            });
        }
    }
    b.build()
}

/// A lamp post on the quay, its lamp lit (self-lit glow paint) or dark.
pub fn lamp_post(pal: &StationPalette, motif: Motif, lit: bool) -> MeshData {
    let mut b = MeshBuilder::new();
    let h = 3.6;
    b.paint(pal.trim, 0.0);
    b.frustum(0.22, 0.14, h, 6);
    let lamp = if lit {
        pal.glow
    } else {
        pal.trim.lerp(pal.hull, 0.35)
    };
    b.paint(lamp, if lit { 1.0 } else { 0.0 });
    b.with(Mat4::from_translation(Vec3::Z * h), |b| {
        b.lathe(&[(0.3, 0.0), (0.4, 0.35), (0.26, 0.7)], 6, 0.0)
    });
    b.paint(pal.accent, 0.0);
    b.with(
        Mat4::from_translation(Vec3::Z * (h + 0.7)),
        |b| match motif {
            Motif::Round => b.lathe(&[(0.38, 0.0), (0.0, 0.3)], 6, 0.0),
            Motif::Square => {
                b.with(Mat4::from_translation(Vec3::Z * 0.08), |b| {
                    b.cuboid(Vec3::new(0.7, 0.7, 0.16))
                });
            }
            Motif::Pointed => b.lathe(&[(0.32, 0.0), (0.0, 0.8)], 6, 0.0),
        },
    );
    b.build()
}

/// Where a lamp post's light sits, above its foot.
pub const LAMP_LIGHT: Vec3 = Vec3::new(0.0, 0.0, 3.95);

/// The cargo crane's mast, `height` tall, with a cab on top. Its jib ([`jib`]) is drawn
/// on its own so it can swing and reach, and the hook's load hangs from the jib's end.
pub fn crane_mast(pal: &StationPalette, height: f32) -> MeshData {
    let mut b = MeshBuilder::new();
    let mast = (height * 0.08).max(0.5);
    b.paint(pal.trim, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * 0.3), |b| {
        b.cuboid(Vec3::new(mast * 3.0, mast * 3.0, 0.6))
    });
    b.frustum(mast * 0.7, mast * 0.45, height, 6);
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * (height + 0.3)), |b| {
        b.cuboid(Vec3::new(mast * 2.2, mast * 2.2, 0.9))
    });
    b.build()
}

/// A jib one unit long along +X from the origin, `thick` units square: scaled along X to
/// the crane's reach.
pub fn jib(pal: &StationPalette, thick: f32) -> MeshData {
    let mut b = MeshBuilder::new();
    b.paint(pal.trim, 0.0);
    bar(&mut b, Vec3::ZERO, Vec3::X, thick);
    b.paint(pal.accent, 0.0);
    bar(&mut b, Vec3::new(0.9, 0.0, 0.0), Vec3::X, thick * 1.3);
    b.build()
}

/// A banner pole: the owner's flag flies from its top (a ribbon drawn as a line).
pub fn banner_pole(pal: &StationPalette, motif: Motif, height: f32) -> MeshData {
    let mut b = MeshBuilder::new();
    b.paint(pal.trim, 0.0);
    b.frustum(0.2, 0.14, height, 6);
    b.paint(pal.accent, 0.0);
    b.with(Mat4::from_translation(Vec3::Z * height), |b| match motif {
        Motif::Round => b.lathe(
            &[(0.0, 0.0), (0.28, 0.12), (0.28, 0.4), (0.0, 0.52)],
            6,
            0.0,
        ),
        Motif::Square => b.cuboid(Vec3::splat(0.45)),
        Motif::Pointed => b.lathe(&[(0.25, 0.0), (0.0, 0.7)], 6, 0.0),
    });
    b.build()
}

/// A ring of `n` points around a centre, for crowds: a loose circle that never stacks.
pub fn huddle(n: usize, radius: f32) -> Vec<Vec3> {
    (0..n)
        .map(|i| {
            let ring = (i / 6) as f32;
            let a = i as f32 / 6.0_f32.min(n as f32).max(1.0) * TAU + ring * 0.5;
            let r = radius * (1.0 + ring * 0.8);
            Vec3::new(a.cos() * r, a.sin() * r, 0.0)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn life_meshes_are_well_formed_and_small() {
        let pal = StationPalette::default();
        let c = Color::hex(0x2f9d86);
        let mut meshes = vec![
            crane_mast(&pal, 8.0),
            jib(&pal, 0.4),
            stock_unit(StockStyle::Crates, c, c),
            stock_unit(StockStyle::Bins, c, c),
            stock_unit(StockStyle::Jars, c, c),
        ];
        for motif in [Motif::Round, Motif::Square, Motif::Pointed] {
            meshes.push(resident(c, c, c, motif));
            meshes.push(lamp_post(&pal, motif, true));
            meshes.push(lamp_post(&pal, motif, false));
            meshes.push(banner_pole(&pal, motif, 5.0));
        }
        for m in &meshes {
            assert!(m.is_well_formed());
            assert!(m.triangle_count() < 600, "{}", m.triangle_count());
        }
        let (lo, hi) = resident(c, c, c, Motif::Pointed).bounds().unwrap();
        assert!(lo.z >= -1e-4 && hi.z <= RESIDENT_HEIGHT * 1.3, "{lo} {hi}");
    }

    #[test]
    fn motifs_parse_and_huddles_spread() {
        assert_eq!(Motif::parse("square"), Some(Motif::Square));
        assert_eq!(Motif::parse("blobby"), None);
        let pts = huddle(9, 1.5);
        for (i, a) in pts.iter().enumerate() {
            for b in &pts[i + 1..] {
                assert!(a.distance(*b) > 0.5, "{a} {b}");
            }
        }
    }
}
