//! CPU-side meshes and a small builder for flat-shaded low-poly geometry.

use crate::color::Color;
use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec2, Vec3};
use std::collections::HashMap;

/// One mesh vertex as uploaded to the GPU.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [f32; 3],
    /// Shading normal. The face normal for flat-shaded geometry; a smooth surface
    /// ([`MeshBuilder::tri_shaded`]) stores the normal of the surface it approximates,
    /// so cel bands move across it instead of whole facets flipping band.
    pub normal: [f32; 3],
    /// Normal averaged over every face sharing this position. The ink outline pushes the
    /// hull out along it, so hard edges don't crack open.
    pub outline_normal: [f32; 3],
    /// Paint colour (linear RGB). Alpha is the glow amount: 0 is lit by the scene's lights,
    /// 1 is fully self-lit (windows, engines, greenhouse lamps).
    pub color: [f32; 4],
    /// World-space size of the facet that owns this vertex, in mesh units.
    /// `x` is the minor span (a sliver's thickness, or the shorter side of a plate).
    /// `y` is the major span. The renderer fades inner ink and thins the silhouette
    /// when these cover only a few pixels, and skips the facet below about one pixel.
    pub span: [f32; 2],
}

/// Triangle mesh data. Front faces wind counter-clockwise.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Axis-aligned bounds `(min, max)`, or `None` for an empty mesh.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let mut it = self.vertices.iter().map(|v| Vec3::from(v.position));
        let first = it.next()?;
        Some(it.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p))))
    }

    /// Radius of the smallest origin-centred sphere containing the mesh.
    pub fn radius(&self) -> f32 {
        self.vertices
            .iter()
            .map(|v| Vec3::from(v.position).length())
            .fold(0.0, f32::max)
    }

    /// Where the ray `origin + dir * t` first enters the mesh: the smallest `t >= 0`
    /// at a front face, or `None` if it misses. `dir` needn't be unit length, so `t` is
    /// the same in any affinely transformed space (mesh space or world space).
    pub fn raycast(&self, origin: Vec3, dir: Vec3) -> Option<f32> {
        let mut best: Option<f32> = None;
        for t in self.indices.as_chunks::<3>().0 {
            let [a, b, c] = t.map(|i| Vec3::from(self.vertices[i as usize].position));
            let (e1, e2) = (b - a, c - a);
            let p = dir.cross(e2);
            let det = e1.dot(p);
            // Front faces only: the ray must come from outside.
            if det <= 1e-12 {
                continue;
            }
            let s = origin - a;
            let u = s.dot(p) / det;
            if !(0.0..=1.0).contains(&u) {
                continue;
            }
            let q = s.cross(e1);
            let v = dir.dot(q) / det;
            if v < 0.0 || u + v > 1.0 {
                continue;
            }
            let hit = e2.dot(q) / det;
            if hit >= 0.0 && best.is_none_or(|b| hit < b) {
                best = Some(hit);
            }
        }
        best
    }

    /// True when every position and normal is finite and every normal is unit length.
    pub fn is_well_formed(&self) -> bool {
        let unit = |n: [f32; 3]| (Vec3::from(n).length() - 1.0).abs() < 1e-3;
        self.indices.len().is_multiple_of(3)
            && self
                .indices
                .iter()
                .all(|&i| (i as usize) < self.vertices.len())
            && self.vertices.iter().all(|v| {
                Vec3::from(v.position).is_finite()
                    && unit(v.normal)
                    && unit(v.outline_normal)
                    && v.color.iter().all(|c| c.is_finite())
                    && v.span[0].is_finite()
                    && v.span[1].is_finite()
                    && v.span[0] >= 0.0
                    && v.span[1] >= v.span[0]
            })
    }
}

/// Builds flat-shaded meshes from primitives. Geometry is added through the current
/// transform and painted with the current colour and glow.
#[derive(Clone, Debug)]
pub struct MeshBuilder {
    mesh: MeshData,
    pub transform: Mat4,
    pub color: Color,
    pub glow: f32,
}

impl Default for MeshBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self {
            mesh: MeshData::default(),
            transform: Mat4::IDENTITY,
            color: Color::WHITE,
            glow: 0.0,
        }
    }

    /// Sets the paint colour and glow for the following geometry.
    pub fn paint(&mut self, color: Color, glow: f32) -> &mut Self {
        self.color = color;
        self.glow = glow;
        self
    }

    /// Runs `f` with `local` applied on top of the current transform.
    pub fn with(&mut self, local: Mat4, f: impl FnOnce(&mut Self)) -> &mut Self {
        let saved = self.transform;
        self.transform = saved * local;
        f(self);
        self.transform = saved;
        self
    }

    /// Adds one flat triangle (counter-clockwise when seen from the front).
    pub fn tri(&mut self, a: Vec3, b: Vec3, c: Vec3) {
        let (mut a, mut b, mut c) = (
            self.transform.transform_point3(a),
            self.transform.transform_point3(b),
            self.transform.transform_point3(c),
        );
        if self.transform.determinant() < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        let n = (b - a).cross(c - a);
        if n.length_squared() < 1e-12 {
            return; // degenerate
        }
        let n = n.normalize();
        // Snap tiny float noise so shared positions weld for outline normals.
        let snap = |p: Vec3| (p * 1e4).round() / 1e4;
        (a, b, c) = (snap(a), snap(b), snap(c));
        let color = [self.color.r, self.color.g, self.color.b, self.glow];
        let span = facet_span(a, b, c);
        let base = self.mesh.vertices.len() as u32;
        for p in [a, b, c] {
            self.mesh.vertices.push(Vertex {
                position: p.into(),
                normal: n.into(),
                outline_normal: n.into(),
                color,
                span,
            });
        }
        self.mesh.indices.extend([base, base + 1, base + 2]);
    }

    /// Adds a triangle shaded with its own per-corner normals (`na nb nc`, before the
    /// current transform) instead of the face normal: one facet of a smooth surface.
    /// Returns false, and adds nothing, for a degenerate triangle.
    pub fn tri_shaded(&mut self, a: Vec3, b: Vec3, c: Vec3, normals: [Vec3; 3]) -> bool {
        let start = self.mesh.vertices.len();
        self.tri(a, b, c);
        if self.mesh.vertices.len() == start {
            return false;
        }
        let m = Mat3::from_mat4(self.transform);
        let to_world = if m.determinant().abs() > 1e-12 {
            m.inverse().transpose()
        } else {
            Mat3::IDENTITY
        };
        let mut ns = normals.map(|n| (to_world * n).normalize_or_zero());
        if self.transform.determinant() < 0.0 {
            ns.swap(1, 2);
        }
        for (v, n) in self.mesh.vertices[start..].iter_mut().zip(ns) {
            if n != Vec3::ZERO {
                v.normal = n.into();
            }
        }
        true
    }

    /// Adds a flat quad `a b c d` (counter-clockwise).
    pub fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3) {
        self.tri(a, b, c);
        self.tri(a, c, d);
    }

    /// A fan-triangulated polygon (convex or star-shaped around its centroid).
    pub fn polygon(&mut self, pts: &[Vec3]) {
        if pts.len() < 3 {
            return;
        }
        let c = pts.iter().copied().sum::<Vec3>() / pts.len() as f32;
        for i in 0..pts.len() {
            self.tri(c, pts[i], pts[(i + 1) % pts.len()]);
        }
    }

    /// A box centred on the origin.
    pub fn cuboid(&mut self, size: Vec3) {
        let h = size * 0.5;
        let p = |x: f32, y: f32, z: f32| Vec3::new(x * h.x, y * h.y, z * h.z);
        let faces = [
            [
                p(1., -1., -1.),
                p(1., 1., -1.),
                p(1., 1., 1.),
                p(1., -1., 1.),
            ],
            [
                p(-1., 1., -1.),
                p(-1., -1., -1.),
                p(-1., -1., 1.),
                p(-1., 1., 1.),
            ],
            [
                p(1., 1., -1.),
                p(-1., 1., -1.),
                p(-1., 1., 1.),
                p(1., 1., 1.),
            ],
            [
                p(-1., -1., -1.),
                p(1., -1., -1.),
                p(1., -1., 1.),
                p(-1., -1., 1.),
            ],
            [
                p(-1., -1., 1.),
                p(1., -1., 1.),
                p(1., 1., 1.),
                p(-1., 1., 1.),
            ],
            [
                p(-1., 1., -1.),
                p(1., 1., -1.),
                p(1., -1., -1.),
                p(-1., -1., -1.),
            ],
        ];
        for [a, b, c, d] in faces {
            self.quad(a, b, c, d);
        }
    }

    /// A surface of revolution around +Z. `profile` lists `(radius, z)` from bottom to top;
    /// a radius of 0 closes that end to a point. Ends with a radius are capped flat.
    pub fn lathe(&mut self, profile: &[(f32, f32)], segments: u32, phase: f32) {
        let segments = segments.max(3);
        let ring = |r: f32, z: f32, i: u32| {
            let a = phase + i as f32 / segments as f32 * std::f32::consts::TAU;
            Vec3::new(r * a.cos(), r * a.sin(), z)
        };
        for w in profile.windows(2) {
            let ((r0, z0), (r1, z1)) = (w[0], w[1]);
            for i in 0..segments {
                let (a0, a1) = (ring(r0, z0, i), ring(r0, z0, i + 1));
                let (b0, b1) = (ring(r1, z1, i), ring(r1, z1, i + 1));
                if r0 <= 1e-6 {
                    self.tri(a0, b1, b0);
                } else if r1 <= 1e-6 {
                    self.tri(a0, a1, b0);
                } else {
                    self.quad(a0, a1, b1, b0);
                }
            }
        }
        let cap = |b: &mut Self, (r, z): (f32, f32), up: bool| {
            if r <= 1e-6 {
                return;
            }
            let c = Vec3::new(0.0, 0.0, z);
            for i in 0..segments {
                let (p0, p1) = (ring(r, z, i), ring(r, z, i + 1));
                if up {
                    b.tri(c, p0, p1);
                } else {
                    b.tri(c, p1, p0);
                }
            }
        };
        if let (Some(&first), Some(&last)) = (profile.first(), profile.last()) {
            let rising = last.1 >= first.1;
            cap(self, first, !rising);
            cap(self, last, rising);
        }
    }

    /// A cylinder or frustum along +Z from `z = 0` to `z = height`.
    pub fn frustum(&mut self, r_bottom: f32, r_top: f32, height: f32, segments: u32) {
        self.lathe(&[(r_bottom, 0.0), (r_top, height)], segments, 0.0);
    }

    /// A low-poly torus around +Z with a flattened tube (`tube_height` vs `tube_width`).
    pub fn torus(
        &mut self,
        radius: f32,
        tube_width: f32,
        tube_height: f32,
        segments: u32,
        sides: u32,
    ) {
        let segments = segments.max(3);
        let sides = sides.max(3);
        let tau = std::f32::consts::TAU;
        let p = |i: u32, j: u32| {
            let a = i as f32 / segments as f32 * tau;
            let b = j as f32 / sides as f32 * tau + tau / (2.0 * sides as f32);
            let r = radius + 0.5 * tube_width * b.cos();
            Vec3::new(r * a.cos(), r * a.sin(), 0.5 * tube_height * b.sin())
        };
        for i in 0..segments {
            for j in 0..sides {
                self.quad(p(i, j), p(i + 1, j), p(i + 1, j + 1), p(i, j + 1));
            }
        }
    }

    /// Extrudes a 2D outline (counter-clockwise, star-shaped) from `z = 0` to `z = height`,
    /// with the top face scaled by `top_scale` around its centroid for a bevelled look.
    pub fn extrude(&mut self, outline: &[Vec2], height: f32, top_scale: f32) {
        if outline.len() < 3 {
            return;
        }
        let c = outline.iter().copied().sum::<Vec2>() / outline.len() as f32;
        let bottom: Vec<Vec3> = outline.iter().map(|p| p.extend(0.0)).collect();
        let top: Vec<Vec3> = outline
            .iter()
            .map(|p| (c + (*p - c) * top_scale).extend(height))
            .collect();
        let n = outline.len();
        for i in 0..n {
            let j = (i + 1) % n;
            self.quad(bottom[i], bottom[j], top[j], top[i]);
        }
        self.polygon(&top);
        let rev: Vec<Vec3> = bottom.iter().rev().copied().collect();
        self.polygon(&rev);
    }

    /// Appends another mesh through the current transform (keeps its own colours).
    pub fn append(&mut self, other: &MeshData) {
        let base = self.mesh.vertices.len() as u32;
        let m = self.transform;
        let nm = Mat3::from_mat4(m).inverse().transpose();
        let flip = m.determinant() < 0.0;
        let axis = |v: glam::Vec4| Vec3::new(v.x, v.y, v.z).length();
        let (sx, sy, sz) = (axis(m.x_axis), axis(m.y_axis), axis(m.z_axis));
        let smin = sx.min(sy).min(sz);
        let smax = sx.max(sy).max(sz);
        for v in &other.vertices {
            let tf = |n: [f32; 3]| (nm * Vec3::from(n)).normalize_or(Vec3::Z).into();
            self.mesh.vertices.push(Vertex {
                position: m.transform_point3(v.position.into()).into(),
                normal: tf(v.normal),
                outline_normal: tf(v.outline_normal),
                color: v.color,
                span: [v.span[0] * smin, v.span[1] * smax],
            });
        }
        for &[a, b, c] in other.indices.as_chunks::<3>().0 {
            let (a, b, c) = (a + base, b + base, c + base);
            if flip {
                self.mesh.indices.extend([a, c, b]);
            } else {
                self.mesh.indices.extend([a, b, c]);
            }
        }
    }

    /// Finishes the mesh: computes outline normals by welding equal positions.
    pub fn build(self) -> MeshData {
        let mut mesh = self.mesh;
        compute_outline_normals(&mut mesh);
        mesh
    }
}

/// Minor and major world-space spans of a triangle.
///
/// A near-zero axis is the face plane, not the part's thickness: a plate keeps the
/// shorter of its two large sides, and a rib keeps its thickness.
fn facet_span(a: Vec3, b: Vec3, c: Vec3) -> [f32; 2] {
    let lo = a.min(b.min(c));
    let hi = a.max(b.max(c));
    let ext = hi - lo;
    let mut axes = [ext.x.abs(), ext.y.abs(), ext.z.abs()];
    axes.sort_by(|l, r| l.total_cmp(r));
    let longest = axes[2].max(1e-4);
    let minor = if axes[0] < longest * 0.2 {
        axes[1]
    } else {
        axes[0]
    };
    let minor = minor.max(1e-4);
    [minor, longest.max(minor)]
}

/// Recomputes [`Vertex::outline_normal`] as the average of the distinct face normals that
/// meet at each position.
pub fn compute_outline_normals(mesh: &mut MeshData) {
    let key = |p: [f32; 3]| {
        let q = |v: f32| (v * 1e3).round() as i64;
        (q(p[0]), q(p[1]), q(p[2]))
    };
    let mut sums: HashMap<(i64, i64, i64), Vec<Vec3>> = HashMap::new();
    for v in &mesh.vertices {
        let n = Vec3::from(v.normal);
        let list = sums.entry(key(v.position)).or_default();
        if !list.iter().any(|m| m.dot(n) > 0.999) {
            list.push(n);
        }
    }
    for v in &mut mesh.vertices {
        let sum: Vec3 = sums[&key(v.position)].iter().copied().sum();
        v.outline_normal = sum.try_normalize().unwrap_or(Vec3::from(v.normal)).into();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn primitives_are_well_formed_and_outward() {
        let mut b = MeshBuilder::new();
        b.cuboid(Vec3::new(2.0, 4.0, 6.0));
        b.with(Mat4::from_translation(Vec3::X * 10.0), |b| {
            b.lathe(&[(0.0, -1.0), (1.0, 0.0), (0.8, 1.0), (0.0, 1.5)], 8, 0.0);
            b.torus(3.0, 0.5, 0.3, 12, 4);
            b.frustum(1.0, 0.5, 2.0, 6);
        });
        // A mirrored transform must keep faces pointing outwards.
        b.with(Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)), |b| {
            b.extrude(
                &[
                    Vec2::new(0.0, 0.0),
                    Vec2::new(2.0, 0.0),
                    Vec2::new(2.0, 1.0),
                    Vec2::new(0.0, 1.0),
                ],
                1.0,
                0.8,
            )
        });
        let mesh = b.build();
        assert!(mesh.is_well_formed());
        let (lo, hi) = mesh.bounds().unwrap();
        assert!(lo.x <= -2.0 && hi.x >= 13.0, "{lo} {hi}");
        // For the cuboid (first 12 triangles) every face normal points away from the centre.
        for t in mesh.indices[..36].as_chunks::<3>().0 {
            let v = mesh.vertices[t[0] as usize];
            let centroid: Vec3 = t
                .iter()
                .map(|&i| Vec3::from(mesh.vertices[i as usize].position))
                .sum::<Vec3>()
                / 3.0;
            assert!(centroid.dot(Vec3::from(v.normal)) > 0.0);
        }
    }

    #[test]
    fn span_keeps_a_rib_thin_and_a_plate_broad() {
        let mut rib = MeshBuilder::new();
        rib.cuboid(Vec3::new(8.0, 0.2, 0.2));
        let rib = rib.build();
        let minor = rib
            .vertices
            .iter()
            .map(|v| v.span[0])
            .fold(f32::MAX, f32::min);
        let major = rib.vertices.iter().map(|v| v.span[1]).fold(0.0, f32::max);
        assert!(minor < 0.35, "rib thickness {minor}");
        assert!(major > 7.0, "rib length {major}");

        let mut plate = MeshBuilder::new();
        plate.cuboid(Vec3::new(10.0, 8.0, 0.3));
        let plate = plate.build();
        let plate_minor = plate.vertices.iter().map(|v| v.span[0]).fold(0.0, f32::max);
        assert!(
            plate_minor > 6.0,
            "a flat plate is not a sliver, minor {plate_minor}"
        );
    }

    #[test]
    fn closed_lathe_faces_point_outwards() {
        let mut b = MeshBuilder::new();
        b.lathe(
            &[(0.0, -1.0), (0.9, -0.5), (1.0, 0.2), (0.6, 0.8), (0.0, 1.0)],
            7,
            0.3,
        );
        b.with(Mat4::from_translation(Vec3::Z * 5.0), |b| {
            b.lathe(&[(1.0, -1.0), (1.0, 1.0)], 6, 0.0)
        });
        let mesh = b.build();
        assert!(mesh.is_well_formed());
        for t in mesh.indices.as_chunks::<3>().0 {
            let v = mesh.vertices[t[0] as usize];
            let centroid: Vec3 = t
                .iter()
                .map(|&i| Vec3::from(mesh.vertices[i as usize].position))
                .sum::<Vec3>()
                / 3.0;
            let centre = if centroid.z > 3.0 {
                Vec3::Z * 5.0
            } else {
                Vec3::ZERO
            };
            assert!((centroid - centre).dot(Vec3::from(v.normal)) > 0.0);
        }
    }

    #[test]
    fn shaded_triangles_keep_their_normals() {
        let mut b = MeshBuilder::new();
        let up = Vec3::new(0.0, 0.3, 1.0).normalize();
        assert!(b.tri_shaded(Vec3::ZERO, Vec3::X, Vec3::Y, [Vec3::Z, up, Vec3::Z]));
        assert!(!b.tri_shaded(Vec3::ZERO, Vec3::X, Vec3::X * 2.0, [Vec3::Z; 3]));
        let mesh = b.build();
        assert!(mesh.is_well_formed());
        assert_eq!(mesh.vertices.len(), 3);
        assert!((Vec3::from(mesh.vertices[1].normal) - up).length() < 1e-6);
    }

    #[test]
    fn raycast_finds_the_near_surface() {
        let mut b = MeshBuilder::new();
        b.cuboid(Vec3::splat(2.0));
        let mesh = b.build();
        let t = mesh.raycast(Vec3::new(-5.0, 0.2, 0.1), Vec3::X).unwrap();
        assert!((t - 4.0).abs() < 1e-4, "{t}");
        // An unnormalised direction scales t.
        let t = mesh
            .raycast(Vec3::new(0.0, 0.0, 9.0), Vec3::NEG_Z * 2.0)
            .unwrap();
        assert!((t - 4.0).abs() < 1e-4, "{t}");
        assert!(mesh.raycast(Vec3::new(-5.0, 3.0, 0.0), Vec3::X).is_none());
        assert!(mesh.raycast(Vec3::new(5.0, 0.0, 0.0), Vec3::X).is_none());
    }

    #[test]
    fn outline_normals_weld_corners() {
        let mut b = MeshBuilder::new();
        b.cuboid(Vec3::splat(2.0));
        let mesh = b.build();
        for v in &mesh.vertices {
            let n = Vec3::from(v.outline_normal);
            // At a cube corner the averaged normal points diagonally out.
            assert!((n - Vec3::from(v.position).normalize()).length() < 1e-4);
        }
    }
}
