//! Mineable rocks of three kinds that visibly shrink, chip and crack as they are mined.

use super::rng::Rng;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use crate::palette;
use glam::{Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// What a rock is made of. Each kind has its own silhouette and colours.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RockKind {
    /// Pale, faceted and a little flattened, with snowy tops.
    Ice,
    /// Lumpy rust-brown with brass ore veins.
    Iron,
    /// Dark slate with glowing violet crystals growing out of it.
    Glassite,
}

/// Parameters for [`rock`].
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct RockParams {
    pub kind: RockKind,
    /// Radius at full size, in world units.
    pub radius: f32,
    /// How much is left, `0..=1`. Lower values shrink, chip and crack the rock.
    pub remaining: f32,
}

impl RockParams {
    pub fn new(kind: RockKind, radius: f32, remaining: f32) -> Self {
        Self {
            kind,
            radius,
            remaining,
        }
    }
}

/// How many cracks and chips a fully mined-out rock shows.
const MAX_CRACKS: usize = 4;
const MAX_CHIPS: usize = 5;

/// The visual size of a rock relative to full, for a `remaining` fraction.
pub fn rock_scale(remaining: f32) -> f32 {
    let r = if remaining.is_finite() {
        remaining.clamp(0.0, 1.0)
    } else {
        1.0
    };
    0.42 + 0.58 * r.sqrt()
}

/// Builds a rock. The same `seed` gives the same rock at every `remaining`, so a rock
/// keeps its identity while it is mined down; cracks and chips only ever accumulate.
pub fn rock(params: &RockParams, seed: u64) -> MeshData {
    let radius = if params.radius.is_finite() {
        params.radius.clamp(0.2, 200.0)
    } else {
        1.0
    };
    let remaining = if params.remaining.is_finite() {
        params.remaining.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let rng = Rng::new(seed);
    let mut b = MeshBuilder::new();
    if remaining <= 0.0 {
        rubble(&mut b, params.kind, radius, rng.fork(9));
        return b.build();
    }
    let style = Style::of(params.kind);
    let (mut pos, tris) = icosphere(1);

    // Lumps: a few smooth bumps plus per-vertex jitter, then the kind's squash.
    let mut shape = rng.fork(1);
    let bumps: Vec<(Vec3, f32)> = (0..5)
        .map(|_| {
            (
                random_dir(&mut shape),
                shape.range(-0.18, 0.28) * style.lumpiness,
            )
        })
        .collect();
    for p in pos.iter_mut() {
        let dir = *p;
        let mut r = 1.0 + shape.signed() * 0.09 * style.lumpiness;
        for &(d, a) in &bumps {
            r += a * dir.dot(d).max(0.0).powi(3);
        }
        *p = dir * r * style.squash;
    }

    // Chips: flat planar cuts that eat into the rock as it's mined.
    let mut chip_rng = rng.fork(2);
    let chips: Vec<(Vec3, f32)> = (0..MAX_CHIPS)
        .map(|_| (random_dir(&mut chip_rng), chip_rng.range(0.55, 0.78)))
        .collect();
    let chip_count = ((1.0 - remaining) * (MAX_CHIPS as f32 + 0.99)) as usize;
    for &(d, depth) in chips.iter().take(chip_count.min(MAX_CHIPS)) {
        for p in pos.iter_mut() {
            let k = p.dot(d);
            if k > depth {
                *p -= d * (k - depth);
            }
        }
    }

    // Cracks: bands of faces near a plane through the rock, sunk in and inked dark.
    let mut crack_rng = rng.fork(3);
    let cracks: Vec<(Vec3, f32)> = (0..MAX_CRACKS)
        .map(|_| (random_dir(&mut crack_rng), crack_rng.range(-0.3, 0.3)))
        .collect();
    let crack_count = ((1.0 - remaining) * (MAX_CRACKS as f32 + 0.5)).round() as usize;
    let active = &cracks[..crack_count.min(MAX_CRACKS)];

    let scale = radius * rock_scale(remaining);
    let mut paint = rng.fork(4);
    let in_crack = |c: Vec3| active.iter().any(|&(n, off)| (c.dot(n) - off).abs() < 0.09);
    let mut sunk = vec![false; pos.len()];
    for t in &tris {
        let c = (pos[t[0]] + pos[t[1]] + pos[t[2]]) / 3.0;
        if in_crack(c) {
            for &i in t {
                sunk[i] = true;
            }
        }
    }
    for (p, s) in pos.iter_mut().zip(&sunk) {
        if *s {
            *p *= 0.9;
        }
    }

    for t in &tris {
        let [a, bb, c] = [pos[t[0]], pos[t[1]], pos[t[2]]];
        let centroid = (a + bb + c) / 3.0;
        let normal = (bb - a).cross(c - a).normalize_or_zero();
        let speckle = paint.f32() < style.speckle_rate;
        let color = match style.cap {
            _ if in_crack(centroid) => style.crack,
            Some(cap) if normal.z > 0.82 => cap,
            _ if speckle => style.speckle,
            _ => style.base,
        };
        b.paint(color, 0.0);
        b.tri(a * scale, bb * scale, c * scale);
    }

    // Glassite grows glowing crystals; they break off as the rock is mined.
    if params.kind == RockKind::Glassite {
        let mut cr = rng.fork(5);
        let total = 5;
        let keep = ((remaining * total as f32).ceil() as usize).clamp(1, total);
        for i in 0..total {
            let dir = (random_dir(&mut cr) + Vec3::Z * 0.8).normalize();
            let len = cr.range(0.45, 0.85);
            let thick = cr.range(0.14, 0.22);
            if i >= keep {
                continue;
            }
            let root = surface_point(&pos, dir) * scale * 0.85;
            let color = if i % 2 == 0 {
                palette::NEBULA_VIOLET.lerp(palette::BLOSSOM, 0.35)
            } else {
                Color::hex(0xc77dff)
            };
            b.paint(color, 0.35);
            b.with(
                Mat4::from_rotation_translation(Quat::from_rotation_arc(Vec3::Z, dir), root),
                |b| {
                    let (t, l) = (thick * scale, len * scale);
                    b.lathe(&[(t, -0.2 * l), (t, 0.7 * l), (0.0, l)], 6, 0.0);
                },
            );
        }
    }
    // Ice wears a couple of little crystal spikes.
    if params.kind == RockKind::Ice && remaining > 0.3 {
        let mut cr = rng.fork(6);
        for _ in 0..2 {
            let dir = (random_dir(&mut cr) + Vec3::Z).normalize();
            let root = surface_point(&pos, dir) * scale * 0.9;
            b.paint(Color::hex(0xeaf8f6), 0.1);
            b.with(
                Mat4::from_rotation_translation(Quat::from_rotation_arc(Vec3::Z, dir), root),
                |b| {
                    let s = scale * cr.range(0.25, 0.4);
                    b.lathe(&[(s * 0.35, -0.2 * s), (0.0, s)], 4, 0.0);
                },
            );
        }
    }
    b.build()
}

struct Style {
    base: Color,
    speckle: Color,
    speckle_rate: f32,
    cap: Option<Color>,
    crack: Color,
    lumpiness: f32,
    squash: Vec3,
}

impl Style {
    fn of(kind: RockKind) -> Self {
        match kind {
            RockKind::Ice => Self {
                base: Color::hex(0x8fd3e0),
                speckle: Color::hex(0x6fbfd6),
                speckle_rate: 0.25,
                cap: Some(Color::hex(0xd8f3f0)),
                crack: Color::hex(0x2e6f86),
                lumpiness: 0.6,
                squash: Vec3::new(1.15, 0.95, 0.75),
            },
            RockKind::Iron => Self {
                base: Color::hex(0xa65a3f),
                speckle: Color::hex(0xd99a4e),
                speckle_rate: 0.18,
                cap: None,
                crack: Color::hex(0x3b1f22),
                lumpiness: 1.2,
                squash: Vec3::new(1.0, 1.05, 0.85),
            },
            RockKind::Glassite => Self {
                base: Color::hex(0x5a4a6e),
                speckle: Color::hex(0x7d5f9a),
                speckle_rate: 0.3,
                cap: None,
                crack: Color::hex(0xe7a6ff),
                lumpiness: 0.9,
                squash: Vec3::new(1.0, 1.0, 0.7),
            },
        }
    }
}

/// A few small pebbles: what's left once a rock is mined out.
fn rubble(b: &mut MeshBuilder, kind: RockKind, radius: f32, mut rng: Rng) {
    let style = Style::of(kind);
    for _ in 0..4 {
        let at = Vec3::new(rng.signed(), rng.signed(), 0.0) * radius * 0.45;
        let s = radius * rng.range(0.1, 0.18);
        b.paint(style.base, 0.0);
        b.with(
            Mat4::from_translation(at)
                * Mat4::from_rotation_z(rng.range(0.0, std::f32::consts::TAU)),
            |b| b.lathe(&[(0.0, -s * 0.6), (s, 0.0), (0.0, s * 0.7)], 5, 0.0),
        );
    }
}

fn random_dir(rng: &mut Rng) -> Vec3 {
    loop {
        let v = Vec3::new(rng.signed(), rng.signed(), rng.signed());
        let l = v.length_squared();
        if l > 0.01 && l <= 1.0 {
            return v / l.sqrt();
        }
    }
}

/// The deformed surface point closest to direction `dir` (a vertex, good enough to seat a
/// crystal on).
fn surface_point(pos: &[Vec3], dir: Vec3) -> Vec3 {
    pos.iter()
        .copied()
        .max_by(|a, b| {
            a.normalize()
                .dot(dir)
                .partial_cmp(&b.normalize().dot(dir))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(Vec3::Z)
}

/// A unit icosphere: welded positions and counter-clockwise triangles.
pub(crate) fn icosphere(subdivisions: u32) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let t = (1.0 + 5f32.sqrt()) / 2.0;
    let mut pos: Vec<Vec3> = [
        (-1., t, 0.),
        (1., t, 0.),
        (-1., -t, 0.),
        (1., -t, 0.),
        (0., -1., t),
        (0., 1., t),
        (0., -1., -t),
        (0., 1., -t),
        (t, 0., -1.),
        (t, 0., 1.),
        (-t, 0., -1.),
        (-t, 0., 1.),
    ]
    .iter()
    .map(|&(x, y, z)| Vec3::new(x, y, z).normalize())
    .collect();
    let mut tris: Vec<[usize; 3]> = vec![
        [0, 11, 5],
        [0, 5, 1],
        [0, 1, 7],
        [0, 7, 10],
        [0, 10, 11],
        [1, 5, 9],
        [5, 11, 4],
        [11, 10, 2],
        [10, 7, 6],
        [7, 1, 8],
        [3, 9, 4],
        [3, 4, 2],
        [3, 2, 6],
        [3, 6, 8],
        [3, 8, 9],
        [4, 9, 5],
        [2, 4, 11],
        [6, 2, 10],
        [8, 6, 7],
        [9, 8, 1],
    ];
    for _ in 0..subdivisions {
        let mut mid: HashMap<(usize, usize), usize> = HashMap::new();
        let mut midpoint = |a: usize, b: usize, pos: &mut Vec<Vec3>| {
            *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                pos.push(((pos[a] + pos[b]) * 0.5).normalize());
                pos.len() - 1
            })
        };
        let mut next = Vec::with_capacity(tris.len() * 4);
        for [a, b, c] in tris {
            let ab = midpoint(a, b, &mut pos);
            let bc = midpoint(b, c, &mut pos);
            let ca = midpoint(c, a, &mut pos);
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
    }
    (pos, tris)
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [RockKind; 3] = [RockKind::Ice, RockKind::Iron, RockKind::Glassite];

    #[test]
    fn icosphere_faces_point_out() {
        let (pos, tris) = icosphere(1);
        assert_eq!(tris.len(), 80);
        for [a, b, c] in tris {
            let n = (pos[b] - pos[a]).cross(pos[c] - pos[a]);
            assert!(n.dot(pos[a] + pos[b] + pos[c]) > 0.0);
        }
    }

    #[test]
    fn rocks_are_well_formed_at_every_stage() {
        for kind in KINDS {
            for i in 0..=10 {
                let m = rock(&RockParams::new(kind, 4.0, i as f32 / 10.0), 11);
                assert!(m.is_well_formed(), "{kind:?} {i}");
                assert!(!m.is_empty());
                assert!(m.radius() < 4.0 * 2.0, "{kind:?} {i}: {}", m.radius());
            }
        }
    }

    #[test]
    fn mining_shrinks_rocks() {
        for kind in KINDS {
            let size = |r: f32| {
                let (lo, hi) = rock(&RockParams::new(kind, 4.0, r), 5).bounds().unwrap();
                (hi - lo).length()
            };
            let (full, half, low) = (size(1.0), size(0.5), size(0.1));
            assert!(full > half && half > low, "{kind:?}: {full} {half} {low}");
        }
    }

    #[test]
    fn mining_adds_cracks() {
        let crack = Style::of(RockKind::Iron).crack;
        let count = |r: f32| {
            rock(&RockParams::new(RockKind::Iron, 4.0, r), 5)
                .vertices
                .iter()
                .filter(|v| v.color[..3] == [crack.r, crack.g, crack.b])
                .count()
        };
        assert_eq!(count(1.0), 0);
        assert!(count(0.3) > 0);
    }

    #[test]
    fn deterministic_and_seed_sensitive() {
        let p = RockParams::new(RockKind::Glassite, 3.0, 0.6);
        assert_eq!(rock(&p, 9), rock(&p, 9));
        assert_ne!(rock(&p, 9), rock(&p, 10));
    }

    #[test]
    fn bad_input_is_clamped() {
        for r in [f32::NAN, -1.0, 7.0] {
            assert!(rock(&RockParams::new(RockKind::Ice, r, r), 1).is_well_formed());
        }
    }
}
