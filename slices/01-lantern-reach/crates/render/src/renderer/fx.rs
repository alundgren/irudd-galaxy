//! CPU tessellation of lines, beams and glows into camera-facing triangles.

use crate::camera::{Camera, Projection};
use crate::scene::{Glow, Line, LineStyle};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(crate) struct FxVertex {
    pub pos: [f32; 3],
    /// x: distance along the line (world units), y: across, -1..1.
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// x: kind (0 ribbon, 1 dashed, 2 glow, 3 beam). Dashed: y dash, z gap. Beam: y
    /// width, z length.
    pub params: [f32; 4],
}

const KIND_RIBBON: f32 = 0.0;
const KIND_DASHED: f32 = 1.0;
const KIND_GLOW: f32 = 2.0;
const KIND_BEAM: f32 = 3.0;

/// The direction the camera looks at `p` from.
fn view_dir(camera: &Camera, p: Vec3) -> Vec3 {
    match camera.projection {
        Projection::Orthographic { .. } => (camera.target - camera.eye).normalize_or(Vec3::NEG_Z),
        Projection::Perspective { .. } => (p - camera.eye).normalize_or(Vec3::NEG_Z),
    }
}

/// Moves `p` towards the camera by `by` along the line of sight, which keeps it on the
/// same pixel. Glows and beams sit this far in front of the surface they touch, so the
/// depth test can't cut a glow into a half disc or chop a beam where it lands.
fn lift(camera: &Camera, p: Vec3, by: f32) -> Vec3 {
    let d = view_dir(camera, p);
    match camera.projection {
        Projection::Perspective { near, .. } => {
            // Never past the near plane.
            let room = (p - camera.eye).dot(d) - near * 2.0;
            p - d * by.min(room.max(0.0))
        }
        Projection::Orthographic { .. } => p - d * by,
    }
}

/// Appends triangles for every line and glow. Lines first, then glows on top.
pub(crate) fn tessellate(
    lines: &[Line],
    glows: &[Glow],
    camera: &Camera,
    verts: &mut Vec<FxVertex>,
    indices: &mut Vec<u32>,
) {
    for (i, line) in lines.iter().enumerate() {
        line_strip(line, i as u32, camera, verts, indices);
    }
    let fwd = (camera.target - camera.eye).normalize_or(Vec3::NEG_Z);
    let right = fwd.cross(camera.up).normalize_or(Vec3::X);
    let up = right.cross(fwd);
    for glow in glows {
        if !glow.position.is_finite() || !glow.radius.is_finite() || glow.radius <= 0.0 {
            continue;
        }
        let base = verts.len() as u32;
        let centre = lift(camera, glow.position, glow.radius);
        for (u, v) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            verts.push(FxVertex {
                pos: (centre + (right * u + up * v) * glow.radius).into(),
                uv: [u, v],
                color: glow.color.to_array(),
                params: [KIND_GLOW, 0.0, 0.0, 0.0],
            });
        }
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

/// Resamples a polyline so wobble has points to bend, and applies the wobble.
fn resample(line: &Line, seed: u32) -> Vec<Vec3> {
    let mut pts: Vec<Vec3> = line
        .points
        .iter()
        .copied()
        .filter(|p| p.is_finite())
        .collect();
    if line.closed && pts.len() > 2 {
        pts.push(pts[0]);
    }
    if line.wobble <= 0.0 || pts.len() < 2 {
        return pts;
    }
    let step = (line.width * 3.0).max(0.5);
    let mut out = Vec::new();
    for w in pts.windows(2) {
        let n = ((w[1] - w[0]).length() / step).ceil().clamp(1.0, 512.0) as usize;
        for k in 0..n {
            out.push(w[0].lerp(w[1], k as f32 / n as f32));
        }
    }
    out.push(*pts.last().unwrap_or(&Vec3::ZERO));
    // Bend sideways (in the line's own plane, approximated as XY) with two slow sines.
    let p1 = (seed as f32 * 1.618).fract() * std::f32::consts::TAU;
    let p2 = (seed as f32 * 2.414).fract() * std::f32::consts::TAU;
    let mut dist = 0.0;
    let count = out.len();
    let mut bent = out.clone();
    for i in 0..count {
        if i > 0 {
            dist += (out[i] - out[i - 1]).length();
        }
        let prev = out[i.saturating_sub(1)];
        let next = out[(i + 1).min(count - 1)];
        let t = (next - prev).truncate().normalize_or_zero();
        let side = Vec3::new(-t.y, t.x, 0.0);
        let k = 0.6 * (dist * 0.11 + p1).sin() + 0.4 * (dist * 0.37 + p2).sin();
        let closed_end = line.closed && (i == 0 || i == count - 1);
        if !closed_end {
            bent[i] += side * k * line.wobble;
        }
    }
    bent
}

fn line_strip(
    line: &Line,
    seed: u32,
    camera: &Camera,
    verts: &mut Vec<FxVertex>,
    indices: &mut Vec<u32>,
) {
    if !line.width.is_finite() || line.width <= 0.0 {
        return;
    }
    let pts = resample(line, seed);
    if pts.len() < 2 {
        return;
    }
    let total: f32 = pts.windows(2).map(|w| (w[1] - w[0]).length()).sum();
    let (kind, dash, gap) = match line.style {
        LineStyle::Solid => (KIND_RIBBON, 0.0, 0.0),
        LineStyle::Dashed { dash, gap } => (KIND_DASHED, dash.max(0.01), gap.max(0.0)),
        LineStyle::Beam => (KIND_BEAM, line.width, total),
    };
    let pts: Vec<Vec3> = match line.style {
        LineStyle::Beam => pts
            .iter()
            .map(|&p| lift(camera, p, line.width * 0.5))
            .collect(),
        _ => pts,
    };
    let half = line.width * 0.5;
    let n = pts.len();
    let closed = line.closed && n > 3;
    let base = verts.len() as u32;
    let mut along = 0.0;
    for i in 0..n {
        if i > 0 {
            along += (pts[i] - pts[i - 1]).length();
        }
        let (prev, next) = if closed && (i == 0 || i == n - 1) {
            (pts[n - 2], pts[1])
        } else {
            (pts[i.saturating_sub(1)], pts[(i + 1).min(n - 1)])
        };
        let tangent = (next - prev).normalize_or(Vec3::X);
        let view = view_dir(camera, pts[i]);
        let side = tangent.cross(view).normalize_or(Vec3::Y);
        // Miter at joins, limited so sharp corners don't spike.
        let seg = if i > 0 {
            pts[i] - pts[i - 1]
        } else {
            next - pts[i]
        };
        let seg_side = seg.normalize_or(tangent).cross(view).normalize_or(side);
        let miter = 1.0 / side.dot(seg_side).max(0.5);
        for s in [-1.0f32, 1.0] {
            verts.push(FxVertex {
                pos: (pts[i] + side * s * half * miter).into(),
                uv: [along, s],
                color: line.color.to_array(),
                params: [kind, dash, gap, 0.0],
            });
        }
    }
    for i in 0..(n as u32 - 1) {
        let a = base + i * 2;
        indices.extend([a, a + 1, a + 3, a, a + 3, a + 2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::Color;
    use glam::Vec2;

    fn cam() -> Camera {
        Camera::top_down(Vec2::ZERO, 100.0)
    }

    #[test]
    fn straight_line_makes_a_quad_of_the_right_width() {
        let (mut v, mut i) = (vec![], vec![]);
        let line = Line::path(vec![Vec3::ZERO, Vec3::X * 10.0], 2.0, Color::WHITE);
        tessellate(&[line], &[], &cam(), &mut v, &mut i);
        assert_eq!(v.len(), 4);
        assert_eq!(i.len(), 6);
        for vert in &v {
            assert!((vert.pos[1].abs() - 1.0).abs() < 1e-5, "{:?}", vert.pos);
        }
        assert_eq!(v[3].uv[0], 10.0);
    }

    #[test]
    fn circles_and_wobble_stay_finite_and_close() {
        let (mut v, mut i) = (vec![], vec![]);
        let lines = [
            Line::circle(Vec3::ZERO, 20.0, 1.0, Color::WHITE).wobbly(0.6),
            Line::path(
                vec![Vec3::ZERO, Vec3::new(30.0, 5.0, 0.0)],
                0.5,
                Color::WHITE,
            )
            .dashed(2.0, 1.0)
            .wobbly(0.4),
            Line::beam(Vec3::ZERO, Vec3::Y * 8.0, 1.2, Color::WHITE),
        ];
        let glows = [Glow {
            position: Vec3::Z,
            radius: 4.0,
            color: Color::WHITE,
        }];
        tessellate(&lines, &glows, &cam(), &mut v, &mut i);
        assert!(v.iter().all(|x| x.pos.iter().all(|c| c.is_finite())));
        assert!(i.iter().all(|&k| (k as usize) < v.len()));

        // The circle's ribbon stays within radius ± (half width + wobble), allowing miters.
        let (mut v, mut i) = (vec![], vec![]);
        tessellate(&lines[..1], &[], &cam(), &mut v, &mut i);
        assert!(v.len() > 48);
        for vert in &v {
            let r = Vec2::new(vert.pos[0], vert.pos[1]).length();
            assert!((r - 20.0).abs() < 0.5 * 2.0 + 0.6 + 0.1, "{r}");
        }
    }

    #[test]
    fn degenerate_input_is_skipped() {
        let (mut v, mut i) = (vec![], vec![]);
        let lines = [
            Line::path(vec![Vec3::ZERO], 1.0, Color::WHITE),
            Line::path(vec![Vec3::ZERO, Vec3::X], 0.0, Color::WHITE),
            Line::path(vec![Vec3::NAN, Vec3::X], 1.0, Color::WHITE),
        ];
        tessellate(&lines, &[], &cam(), &mut v, &mut i);
        assert!(v.is_empty() && i.is_empty());
    }
}
