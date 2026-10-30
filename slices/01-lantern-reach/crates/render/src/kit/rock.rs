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
/// The share of a rock's life over which one chip grows from a graze to full depth.
const CHIP_WINDOW: f32 = 0.5;
/// How far (unit rock radius) a cut's plane passes a face before the face shows fully
/// as cut.
const CHIP_FADE: f32 = 0.35;
/// The share of a rock's life over which one crack opens.
const CRACK_WINDOW: f32 = 0.45;

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

    // Chips: flat planar cuts that eat into the rock as it's mined. Each one grows in
    // over its own stretch of `remaining`, from a graze to its full depth, so a re-mesh
    // at the next visible step only moves the surface a little.
    let mut chip_rng = rng.fork(2);
    let chips: Vec<(Vec3, f32)> = (0..MAX_CHIPS)
        .map(|_| (random_dir(&mut chip_rng), chip_rng.range(0.55, 0.78)))
        .collect();
    // Chip i grows over its own window of the rock's life, half of it long; the
    // windows overlap so each step bites a little off several chips.
    let mined = 1.0 - remaining;
    let chip_start = |i: usize| i as f32 * (1.0 - CHIP_WINDOW) / (MAX_CHIPS - 1) as f32;
    // Crystals seat on the vertex nearest their direction on the uncut rock, so a
    // chip never makes one jump to another vertex.
    let lumpy = pos.clone();
    let mut cuts: Vec<(Vec3, f32)> = Vec::new();
    // How far each face sits into each chip's cut: 0 as the plane first flattens it,
    // 1 once the plane is CHIP_FADE below where the face's lowest corner was. A face
    // takes on the cut's flat shading and bitten paint by this much, not all at once.
    let mut into = vec![[0.0f32; MAX_CHIPS]; tris.len()];
    for (i, &(d, depth)) in chips.iter().enumerate() {
        let grow = ((mined - chip_start(i)) / CHIP_WINDOW).clamp(0.0, 1.0);
        if grow <= 0.0 {
            break;
        }
        let top = pos.iter().map(|p| p.dot(d)).fold(f32::MIN, f32::max);
        let plane = top + (depth.min(top) - top) * grow;
        for (t, f) in tris.iter().zip(into.iter_mut()) {
            let low = t.iter().map(|&k| pos[k].dot(d)).fold(f32::MAX, f32::min);
            f[i] = ((low - plane) / CHIP_FADE).clamp(0.0, 1.0);
        }
        for p in pos.iter_mut() {
            let k = p.dot(d);
            if k > plane {
                *p -= d * (k - plane);
            }
        }
        cuts.push((d, plane));
    }

    // Each face's smoothing group: 0 for the rock's skin, 1 + i for the flat face of
    // chip i. Normals are smoothed within a group, so the skin shades as one lumpy
    // surface and a chip stays a crisp cut.
    let on_plane = |p: Vec3, (d, plane): (Vec3, f32)| (p.dot(d) - plane).abs() < 1e-4;
    // Each face's group and how far it has turned into it (`into`).
    let group: Vec<(usize, f32)> = tris
        .iter()
        .zip(&into)
        .map(|(t, f)| {
            cuts.iter()
                .rposition(|&cut| t.iter().all(|&i| on_plane(pos[i], cut)))
                .map_or((0, 0.0), |c| (c + 1, f[c]))
        })
        .collect();

    // Cracks: bands of faces near a plane through the rock, inked dark and sunk in as
    // they open.
    let mut crack_rng = rng.fork(3);
    let cracks: Vec<(Vec3, f32)> = (0..MAX_CRACKS)
        .map(|_| (random_dir(&mut crack_rng), crack_rng.range(-0.3, 0.3)))
        .collect();
    // Crack i opens over its own window of the rock's life; the windows overlap.
    let crack_start = |i: usize| 0.08 + i as f32 * (0.92 - CRACK_WINDOW) / (MAX_CRACKS - 1) as f32;
    let open = |c: Vec3| {
        cracks
            .iter()
            .enumerate()
            .filter(|&(_, &(n, off))| (c.dot(n) - off).abs() < 0.09)
            .map(|(i, _)| ((mined - crack_start(i)) / CRACK_WINDOW).clamp(0.0, 1.0))
            .fold(0.0, f32::max)
    };
    // Which faces a crack runs through is fixed on the uncut rock, so a chip never
    // moves a face into or out of one.
    let crack: Vec<f32> = tris
        .iter()
        .map(|t| open((lumpy[t[0]] + lumpy[t[1]] + lumpy[t[2]]) / 3.0))
        .collect();
    let mut sink = vec![0.0f32; pos.len()];
    for (t, &c) in tris.iter().zip(&crack) {
        for &i in t {
            sink[i] = sink[i].max(c);
        }
    }
    for (p, s) in pos.iter_mut().zip(&sink) {
        *p *= 1.0 - 0.1 * s;
    }

    // Angle-weighted vertex normals per smoothing group. A face part-way into a cut
    // counts towards both the skin and the cut.
    let mut smooth = vec![[Vec3::ZERO; MAX_CHIPS + 1]; pos.len()];
    for (t, &(g, f)) in tris.iter().zip(&group) {
        let n = (pos[t[1]] - pos[t[0]])
            .cross(pos[t[2]] - pos[t[0]])
            .normalize_or_zero();
        for k in 0..3 {
            let (a, b, c) = (pos[t[k]], pos[t[(k + 1) % 3]], pos[t[(k + 2) % 3]]);
            let angle = (b - a).angle_between(c - a);
            if angle.is_finite() {
                smooth[t[k]][0] += n * angle * (1.0 - f);
                smooth[t[k]][g] += n * angle * f;
            }
        }
    }

    let scale = radius * rock_scale(remaining);
    let mut paint = rng.fork(4);
    for ((t, &(g, f)), &c) in tris.iter().zip(&group).zip(&crack) {
        let [a, bb, cc] = [pos[t[0]], pos[t[1]], pos[t[2]]];
        let normal = (bb - a).cross(cc - a).normalize_or_zero();
        // Caps are chosen on the uncut rock, so a chip never flips one.
        let [la, lb, lc] = [lumpy[t[0]], lumpy[t[1]], lumpy[t[2]]];
        let upright = (lb - la).cross(lc - la).normalize_or_zero().z > 0.82;
        let speckle = paint.f32() < style.speckle_rate;
        // A mined face shows a warm interior, like a cut fruit, coming through as the
        // cut reaches it. A crack darkens to its ink colour as it opens.
        let skin = match style.cap {
            Some(cap) if upright => cap,
            _ if speckle => style.speckle,
            _ => style.base,
        };
        let color = if g > 0 {
            skin.lerp(style.bite, f)
        } else {
            skin
        };
        b.paint(color.lerp(style.crack, c), 0.0);
        let normals = t.map(|i| {
            let skin = smooth[i][0].try_normalize().unwrap_or(normal);
            let cut = smooth[i][g].try_normalize().unwrap_or(normal);
            skin.lerp(cut, f).try_normalize().unwrap_or(normal)
        });
        b.tri_shaded(a * scale, bb * scale, cc * scale, normals);
    }

    // Glassite grows glowing crystals; they break off as the rock is mined.
    if params.kind == RockKind::Glassite {
        let mut cr = rng.fork(5);
        let total = 5;
        let keep = ((remaining * total as f32).ceil() as usize).clamp(1, total);
        for i in 0..total {
            let dir = (random_dir(&mut cr) + Vec3::Z * 0.8).normalize();
            // A crystal wears down to a stub before it breaks off.
            let wear = 0.25 + 0.75 * (remaining * total as f32 - i as f32).clamp(0.0, 1.0);
            let len = cr.range(0.45, 0.85) * wear;
            let thick = cr.range(0.14, 0.22) * wear;
            if i >= keep {
                continue;
            }
            let root = pos[surface_vertex(&lumpy, dir)] * scale * 0.85;
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
    b.build()
}

struct Style {
    base: Color,
    speckle: Color,
    speckle_rate: f32,
    cap: Option<Color>,
    /// Warm colour of a fresh chip. Never the crack colour.
    bite: Color,
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
                bite: Color::hex(0xf4e2c4),
                crack: Color::hex(0x2e6f86),
                lumpiness: 0.6,
                squash: Vec3::new(1.15, 0.95, 0.75),
            },
            RockKind::Iron => Self {
                base: Color::hex(0xa65a3f),
                speckle: Color::hex(0xd99a4e),
                speckle_rate: 0.18,
                cap: None,
                bite: Color::hex(0xe2a15a),
                crack: Color::hex(0x3b1f22),
                lumpiness: 1.2,
                squash: Vec3::new(1.0, 1.05, 0.85),
            },
            RockKind::Glassite => Self {
                base: Color::hex(0x5a4a6e),
                speckle: Color::hex(0x7d5f9a),
                speckle_rate: 0.3,
                cap: None,
                bite: Color::hex(0xc9b6f0),
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
    for i in 0..4 {
        let at = Vec3::new(rng.signed(), rng.signed(), 0.0) * radius * 0.45;
        let s = radius * rng.range(0.12, 0.2);
        let tall = if i == 0 { 1.2 } else { 0.8 };
        b.paint(style.base, 0.0);
        b.with(
            Mat4::from_translation(at)
                * Mat4::from_rotation_z(rng.range(0.0, std::f32::consts::TAU)),
            |b| {
                b.lathe(
                    &[
                        (0.0, -s * 0.55),
                        (s, 0.0),
                        (s * 0.55, s * 0.35 * tall),
                        (0.0, s * 0.7 * tall),
                    ],
                    5,
                    0.0,
                );
                // A warm cap, so a mined-out pile still has a face.
                b.paint(style.bite, 0.0);
                b.with(
                    Mat4::from_translation(Vec3::new(s * 0.12, -s * 0.05, s * 0.15 * tall)),
                    |b| b.lathe(&[(s * 0.32, 0.0), (0.0, s * 0.36)], 4, 0.5),
                );
            },
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

/// The vertex closest to direction `dir` (good enough to seat a crystal on).
fn surface_vertex(pos: &[Vec3], dir: Vec3) -> usize {
    (0..pos.len())
        .max_by(|&a, &b| {
            pos[a]
                .normalize()
                .dot(dir)
                .total_cmp(&pos[b].normalize().dot(dir))
        })
        .unwrap_or(0)
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
                .filter(|v| {
                    let d = Vec3::from_slice(&v.color[..3]) - Vec3::new(crack.r, crack.g, crack.b);
                    d.length() < 1e-4
                })
                .count()
        };
        assert_eq!(count(1.0), 0);
        assert!(count(0.3) > 0);
    }

    /// Body vertices of a rock with its overall size taken out (the app scales the
    /// instance between re-meshes), and its shading normals.
    fn body(kind: RockKind, remaining: f32, seed: u64) -> Vec<(Vec3, Vec3)> {
        let m = rock(&RockParams::new(kind, 4.0, remaining), seed);
        let s = 4.0 * rock_scale(remaining);
        m.vertices
            .iter()
            .take(80 * 3)
            .map(|v| (Vec3::from(v.position) / s, Vec3::from(v.normal)))
            .collect()
    }

    #[test]
    fn remeshing_one_step_moves_the_surface_only_a_little() {
        // The app re-meshes at every 1/20 of `remaining`. No step may jump the surface
        // or flip the shading of a large part of it (the "blocks" a player saw).
        let mut worst = (0.0f32, 0.0f32);
        for kind in KINDS {
            for seed in 0..24 {
                for i in 0..20 {
                    let (hi, lo) = ((20 - i) as f32 / 20.0, (19 - i) as f32 / 20.0);
                    if lo <= 0.0 {
                        continue;
                    }
                    let (a, b) = (body(kind, hi, seed), body(kind, lo, seed));
                    assert_eq!(a.len(), b.len(), "{kind:?} {seed} {i}");
                    let moved = a
                        .iter()
                        .zip(&b)
                        .map(|(p, q)| p.0.distance(q.0))
                        .fold(0.0, f32::max);
                    let turned = a
                        .iter()
                        .zip(&b)
                        .map(|(p, q)| 1.0 - p.1.dot(q.1))
                        .sum::<f32>()
                        / a.len() as f32;
                    worst = (worst.0.max(moved), worst.1.max(turned));
                }
            }
        }
        // Unit-radius rock: no vertex moves more than a tenth of the radius in a step,
        // and on average shading normals turn by only a few degrees.
        assert!(worst.0 < 0.1, "a vertex moved {}", worst.0);
        assert!(worst.1 < 0.01, "normals turned {}", worst.1);
    }

    #[test]
    fn remeshing_is_deterministic() {
        for kind in KINDS {
            for i in 0..=20 {
                let p = RockParams::new(kind, 3.0, i as f32 / 20.0);
                assert_eq!(rock(&p, 7), rock(&p, 7));
            }
        }
    }

    #[test]
    fn the_skin_is_smooth_shaded() {
        // Neighbouring skin faces share their corner normals, so the cel bands follow
        // the lumps instead of flipping a facet at a time.
        let m = rock(&RockParams::new(RockKind::Iron, 4.0, 1.0), 3);
        let mut by_pos: HashMap<[i32; 3], Vec<Vec3>> = HashMap::new();
        for v in &m.vertices {
            let k = v.position.map(|c| (c * 1e3).round() as i32);
            by_pos.entry(k).or_default().push(Vec3::from(v.normal));
        }
        assert!(
            by_pos
                .values()
                .all(|ns| ns.iter().all(|n| n.distance(ns[0]) < 1e-4))
        );
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
