//! The rock shape: a lumpy faceted ball, and as its morph target the same rock chipped,
//! cut and shrunk, so a thing wearing it changes shape continuously as its
//! `engine.model` morph runs from 0 to 1 (as when it is mined).
//!
//! The rock and its target share their triangles (only the corners move), which is what
//! a glTF morph target needs. Carried over from the slice's rocks, without their kinds:
//! colours come from the palette and the shape from the keys.

use std::collections::BTreeMap;

use galaxy_mesh::{MeshData, Vertex};
use glam::Vec3;

use crate::params::Values;
use crate::rng::Rng;
use crate::shapes::Ctx;

/// A unit icosahedron subdivided `detail` times: corners on the unit sphere, and
/// counter-clockwise triangles.
fn icosphere(detail: u32) -> (Vec<Vec3>, Vec<[usize; 3]>) {
    let t = (1.0 + libm::sqrtf(5.0)) / 2.0;
    let mut pos: Vec<Vec3> = [
        (-1.0, t, 0.0),
        (1.0, t, 0.0),
        (-1.0, -t, 0.0),
        (1.0, -t, 0.0),
        (0.0, -1.0, t),
        (0.0, 1.0, t),
        (0.0, -1.0, -t),
        (0.0, 1.0, -t),
        (t, 0.0, -1.0),
        (t, 0.0, 1.0),
        (-t, 0.0, -1.0),
        (-t, 0.0, 1.0),
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
    for _ in 0..detail {
        let mut mid: BTreeMap<(usize, usize), usize> = BTreeMap::new();
        let mut split = |a: usize, b: usize, pos: &mut Vec<Vec3>| {
            *mid.entry((a.min(b), a.max(b))).or_insert_with(|| {
                pos.push(((pos[a] + pos[b]) * 0.5).normalize());
                pos.len() - 1
            })
        };
        let mut next = Vec::with_capacity(tris.len() * 4);
        for [a, b, c] in tris {
            let (ab, bc, ca) = (
                split(a, b, &mut pos),
                split(b, c, &mut pos),
                split(c, a, &mut pos),
            );
            next.extend([[a, ab, ca], [b, bc, ab], [c, ca, bc], [ab, bc, ca]]);
        }
        tris = next;
    }
    (pos, tris)
}

/// A direction spread evenly over the sphere.
fn random_dir(r: &mut Rng) -> Vec3 {
    let z = r.signed();
    let a = r.f32() * std::f32::consts::TAU;
    let s = libm::sqrtf((1.0 - z * z).max(0.0));
    Vec3::new(s * libm::cosf(a), s * libm::sinf(a), z)
}

pub fn build(c: &mut Ctx, v: &Values) {
    let radius = v.num("radius");
    let lumps = v.num("lumps");
    let squash = v.num("squash");
    let mined = v.num("mined");
    let (unit, tris) = icosphere(v.int("detail"));

    // Lumps: a few broad bumps and a little jitter on each corner, then the squash.
    let mut shape = c.rng.fork(1);
    let bumps: Vec<(Vec3, f32)> = (0..5)
        .map(|_| (random_dir(&mut shape), shape.range(-0.18, 0.28) * lumps))
        .collect();
    let pos: Vec<Vec3> = unit
        .iter()
        .map(|&dir| {
            let mut r = 1.0 + shape.signed() * 0.09 * lumps;
            for &(d, a) in &bumps {
                let k = dir.dot(d).max(0.0);
                r += a * k * k * k;
            }
            let p = dir * r * radius;
            Vec3::new(p.x, p.y, p.z * squash)
        })
        .collect();

    // The morph target: flat cuts eat into it, then it shrinks a little.
    let mut target = pos.clone();
    let mut cuts: Vec<(Vec3, f32)> = Vec::new();
    if mined > 0.0 {
        let mut chip = c.rng.fork(2);
        for _ in 0..v.int("chips") {
            let d = random_dir(&mut chip);
            let depth = chip.range(0.45, 0.75);
            let top = target.iter().map(|p| p.dot(d)).fold(f32::MIN, f32::max);
            let plane = top - (top - depth * radius) * mined;
            for p in &mut target {
                let k = p.dot(d);
                if k > plane {
                    *p -= d * (k - plane);
                }
            }
            cuts.push((d, plane));
        }
        let shrink = 1.0 - 0.35 * mined;
        for p in &mut target {
            *p *= shrink;
        }
        for cut in &mut cuts {
            cut.1 *= shrink;
        }
    }
    let on_cut = |t: &[usize; 3]| {
        cuts.iter().any(|&(d, plane)| {
            t.iter()
                .all(|&i| (target[i].dot(d) - plane).abs() < 1e-3 * radius.max(1.0))
        })
    };

    let skin = c.paints["paint"];
    let top = c.paints["accent"];
    let face = |p: [Vec3; 3]| (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Z);
    let mut mesh = MeshData::default();
    for t in &tris {
        let now = t.map(|i| pos[i]);
        let then = t.map(|i| target[i]);
        let (n, tn) = (face(now), face(then));
        // Faces turned to the sky take the accent, on the uncut rock so a cut never
        // flips one.
        let paint = if n.z > 0.8 { top } else { skin };
        let color = [paint.r, paint.g, paint.b, 0.0];
        let bite = if on_cut(t) { 0.85 } else { 0.0 };
        let base = mesh.vertices.len() as u32;
        for k in 0..3 {
            mesh.vertices.push(Vertex {
                position: now[k].into(),
                normal: n.into(),
                outline_normal: n.into(),
                color,
                span: [radius, radius * 2.0],
                facet: radius * 0.1,
                morph_position: then[k].into(),
                morph_normal: tn.into(),
                morph_color: color,
                bite: [0.0, bite],
            });
        }
        mesh.indices.extend([base, base + 1, base + 2]);
    }
    c.b.append(&mesh);
}
