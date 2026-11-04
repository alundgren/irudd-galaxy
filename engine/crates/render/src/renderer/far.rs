//! Packs a scene's distant things ([`crate::far`]) for `far.wgsl`.

use crate::camera::Camera;
use crate::far::{Far, Mark, Outline};
use crate::scene::Lights;
use bytemuck::{Pod, Zeroable};
use glam::{Vec2, Vec3};

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub(super) struct FarGlobalsGpu {
    size: [f32; 4],
    centre: [f32; 4],
    inv: [f32; 4],
    fwd: [f32; 4],
    sun: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub(super) struct FarGpu {
    a: [f32; 4],
    b: [f32; 4],
    c: [f32; 4],
    d: [f32; 4],
    e: [f32; 4],
    f: [f32; 4],
}

/// How the play plane meets the screen at the camera's target: where the target is,
/// and the logical pixels one world unit along +X and +Y moves.
pub(super) fn plane_on_screen(cam: &Camera, logical: Vec2) -> (Vec2, Vec2, Vec2) {
    let t = cam.target;
    let centre = cam.world_to_screen(t, logical).unwrap_or(logical * 0.5);
    let step = |d: Vec3| {
        cam.world_to_screen(t + d, logical)
            .map_or(Vec2::ZERO, |p| p - centre)
    };
    (centre, step(Vec3::X), step(Vec3::Y))
}

pub(super) fn globals(cam: &Camera, lights: &Lights, logical: Vec2, raster: Vec2) -> FarGlobalsGpu {
    let (centre, fx, fy) = plane_on_screen(cam, logical);
    let det = fx.x * fy.y - fy.x * fx.y;
    let inv = if det.abs() > 1e-12 {
        [fy.y / det, -fx.y / det, -fy.x / det, fx.x / det]
    } else {
        [0.0; 4]
    };
    let sun = cam
        .view()
        .transform_vector3(lights.sun_direction)
        .normalize_or(Vec3::Z);
    FarGlobalsGpu {
        size: [logical.x, logical.y, raster.x, raster.y],
        centre: [centre.x, centre.y, 0.0, 0.0],
        inv,
        fwd: [fx.x, fx.y, fy.x, fy.y],
        sun: sun.extend(0.0).into(),
    }
}

/// One GPU item for each field and shape in `far`, and for each entry of `far` the
/// index of its item (meshes have none).
pub(super) fn items(far: &[Far], cam: &Camera, logical: Vec2) -> (Vec<FarGpu>, Vec<Option<u32>>) {
    let (_, fx, fy) = plane_on_screen(cam, logical);
    let px_per_unit = fx.length().min(fy.length());
    let per_pixel = cam.world_per_pixel(logical).max(1e-6);
    let mut out = Vec::new();
    let mut index = Vec::with_capacity(far.len());
    for item in far {
        let gpu = match item {
            Far::Field(f) => {
                if !(f.cell.is_finite() && f.cell > 0.0 && f.offset.is_finite()) {
                    index.push(None);
                    continue;
                }
                let mark = match f.mark {
                    Mark::Dust => 0.0,
                    Mark::Speck => 1.0,
                    Mark::Hatch => 2.0,
                };
                FarGpu {
                    a: [f.offset.x, f.offset.y, f.cell, f.chance.clamp(0.0, 1.0)],
                    b: f.color.to_array(),
                    c: [f.size[0], f.size[1], f.angle, mark],
                    d: f.haze.to_array(),
                    e: [
                        (f.seed & 0xff_ffff) as f32,
                        0.5 * f.cell * px_per_unit,
                        0.0,
                        0.0,
                    ],
                    f: [0.0; 4],
                }
            }
            Far::Shape(s) => {
                let Some(centre) = cam.world_to_screen(s.center, logical) else {
                    index.push(None);
                    continue;
                };
                let outline = match s.outline {
                    Outline::Disc => 0.0,
                    Outline::Wash => 1.0,
                };
                FarGpu {
                    a: [centre.x, centre.y, s.radius / per_pixel, s.stretch],
                    b: s.color.to_array(),
                    c: [
                        s.angle,
                        outline,
                        s.outline_px.max(0.0),
                        s.hatch.clamp(0.0, 1.0),
                    ],
                    d: s.haze.to_array(),
                    e: [
                        s.shadow.r,
                        s.shadow.g,
                        s.shadow.b,
                        (s.seed & 0xff_ffff) as f32,
                    ],
                    f: s.ink.to_array(),
                }
            }
            Far::Meshes(_) => {
                index.push(None);
                continue;
            }
        };
        index.push(Some(out.len() as u32));
        out.push(gpu);
    }
    (out, index)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::far::{Field, Shape};
    use galaxy_mesh::Color;

    #[test]
    fn the_plane_maps_both_ways() {
        let cam = Camera::top_down(Vec2::new(40.0, -12.0), 360.0).with_tilt(30.0);
        let logical = Vec2::new(1280.0, 720.0);
        let g = globals(&cam, &Lights::default(), logical, logical * 2.0);
        let (centre, fx, fy) = plane_on_screen(&cam, logical);
        assert!((centre - logical * 0.5).length() < 1e-2);
        // Two world units of +X are four pixels right at 2 pixels a unit.
        assert!((fx - Vec2::new(2.0, 0.0)).length() < 1e-3, "{fx}");
        // +Y goes up the screen, foreshortened by the tilt.
        assert!(fy.y < 0.0 && fy.x.abs() < 1e-4);
        assert!(
            (fy.y.abs() - 2.0 * 30f32.to_radians().cos()).abs() < 1e-3,
            "{fy}"
        );
        // The inverse undoes it.
        let d = Vec2::new(3.0, -7.0);
        let px = Vec2::new(
            g.fwd[0] * d.x + g.fwd[2] * d.y,
            g.fwd[1] * d.x + g.fwd[3] * d.y,
        );
        let back = Vec2::new(
            g.inv[0] * px.x + g.inv[2] * px.y,
            g.inv[1] * px.x + g.inv[3] * px.y,
        );
        assert!((back - d).length() < 1e-4, "{back}");
    }

    #[test]
    fn fields_and_shapes_pack_and_meshes_skip() {
        let cam = Camera::top_down(Vec2::ZERO, 360.0);
        let logical = Vec2::new(1280.0, 720.0);
        let field = Field {
            mark: Mark::Hatch,
            offset: Vec2::new(5.0, 6.0),
            cell: 50.0,
            chance: 0.7,
            size: [2.0, 6.0],
            color: Color::WHITE,
            angle: 0.5,
            seed: 0x1234_5678,
            haze: Color::TRANSPARENT,
        };
        let shape = Shape {
            outline: Outline::Disc,
            center: Vec3::new(90.0, 0.0, 0.0),
            radius: 45.0,
            stretch: 1.0,
            angle: 0.0,
            color: Color::WHITE,
            shadow: Color::WHITE,
            ink: Color::WHITE,
            outline_px: 3.0,
            hatch: 0.5,
            seed: 3,
            haze: Color::TRANSPARENT,
        };
        let far = [
            Far::Field(field),
            Far::Meshes(Vec::new()),
            Far::Shape(shape),
        ];
        let (gpu, index) = items(&far, &cam, logical);
        assert_eq!(index, [Some(0), None, Some(1)]);
        // The largest mark is half a 50-unit cell at 2 pixels a unit.
        assert_eq!(gpu[0].e[1], 50.0);
        // The seed keeps 24 bits, exact in a float.
        assert_eq!(gpu[0].e[0], 0x34_5678 as f32);
        // The disc sits 180 px right of the centre and is 90 px in radius.
        assert!((gpu[1].a[0] - 820.0).abs() < 1e-2 && (gpu[1].a[2] - 90.0).abs() < 1e-3);
    }
}
