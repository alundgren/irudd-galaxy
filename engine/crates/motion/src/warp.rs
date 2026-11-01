//! Camera compensation: where a pixel of one frame was in another, for anything lying on
//! the ground plane. With the camera following a ship, the whole world slides across the
//! screen every frame; comparing each frame with the one before moved into its view leaves
//! only what moved in the world, popped or flickered.
//!
//! The mapping is exact for things on the plane and close for things near it, which in a
//! 2.5D world is nearly everything. Overlays fixed on screen are the exception: tools take
//! `--screen` to compare pixels where they are.

use crate::film::{Cam, invert};
use crate::image::Gray;

/// Maps pixels of frame `to` to where the same plane point is in frame `from`.
pub struct Warp {
    inv: Option<[f32; 16]>,
    from: [f32; 16],
    same: bool,
    w: f32,
    h: f32,
    plane: f32,
}

impl Warp {
    pub fn new(to: &Cam, from: &Cam, w: u32, h: u32, plane: f32) -> Warp {
        Warp {
            inv: invert(&to.view_proj),
            from: from.view_proj,
            same: to.view_proj == from.view_proj,
            w: w as f32,
            h: h as f32,
            plane,
        }
    }

    /// No mapping: pixels are compared where they are.
    pub fn identity(w: u32, h: u32) -> Warp {
        Warp {
            inv: None,
            from: [0.0; 16],
            same: true,
            w: w as f32,
            h: h as f32,
            plane: 0.0,
        }
    }

    /// Where screen point `(x, y)` of `to` (physical pixels) is in `from`.
    pub fn map(&self, x: f32, y: f32) -> Option<[f32; 2]> {
        if self.same {
            return Some([x, y]);
        }
        let inv = self.inv.as_ref()?;
        let nx = x / self.w * 2.0 - 1.0;
        let ny = 1.0 - y / self.h * 2.0;
        let at = |z: f32| {
            let v = [nx, ny, z, 1.0];
            let r = |row: usize| (0..4).map(|c| inv[c * 4 + row] * v[c]).sum::<f32>();
            let w = r(3);
            [r(0) / w, r(1) / w, r(2) / w]
        };
        let (a, b) = (at(0.0), at(1.0));
        let dz = b[2] - a[2];
        if dz.abs() < 1e-9 {
            return None;
        }
        let t = (self.plane - a[2]) / dz;
        let p = [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            self.plane,
            1.0,
        ];
        let m = &self.from;
        let r = |row: usize| (0..4).map(|c| m[c * 4 + row] * p[c]).sum::<f32>();
        let cw = r(3);
        if cw <= 1e-6 {
            return None;
        }
        Some([
            (r(0) / cw + 1.0) * 0.5 * self.w,
            (1.0 - r(1) / cw) * 0.5 * self.h,
        ])
    }
}

/// `from` (a gray image at `1 / scale` of the frame size) seen from the view `warp` maps
/// into; NaN where it saw nothing.
pub fn compensate(from: &Gray, warp: &Warp, scale: f32) -> Vec<f32> {
    let mut out = Vec::with_capacity(from.data.len());
    for y in 0..from.height {
        for x in 0..from.width {
            let sx = (x as f32 + 0.5) * scale;
            let sy = (y as f32 + 0.5) * scale;
            let v = match warp.map(sx, sy) {
                Some([fx, fy]) => {
                    let (gx, gy) = (fx / scale - 0.5, fy / scale - 0.5);
                    if gx < -0.5
                        || gy < -0.5
                        || gx > from.width as f32 - 0.5
                        || gy > from.height as f32 - 0.5
                    {
                        f32::NAN
                    } else {
                        from.sample(gx, gy)
                    }
                }
                None => f32::NAN,
            };
            out.push(v);
        }
    }
    out
}

/// A frame as gray at `1 / scale` size.
pub fn gray(img: &crate::image::Image, scale: u32) -> Gray {
    let g = img.gray();
    if scale >= 2 { g.half() } else { g }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ortho(cx: f32) -> Cam {
        // Clip x = (wx - cx) * 0.1, y = wy * 0.2.
        let mut m = [0.0; 16];
        m[0] = 0.1;
        m[5] = 0.2;
        m[10] = 0.01;
        m[12] = -cx * 0.1;
        m[15] = 1.0;
        Cam {
            eye: [0.0; 3],
            target: [cx, 0.0, 0.0],
            view_proj: m,
            per_pixel: 0.1,
        }
    }

    #[test]
    fn a_camera_that_moved_maps_pixels_back() {
        // 200 px wide shows 20 units: 10 px a unit. The camera moved 2 units right, so a
        // ground point now at x px was at x + 20 px before.
        let w = Warp::new(&ortho(2.0), &ortho(0.0), 200, 100, 0.0);
        let p = w.map(50.0, 30.0).unwrap();
        assert!(
            (p[0] - 70.0).abs() < 1e-3 && (p[1] - 30.0).abs() < 1e-3,
            "{p:?}"
        );
        assert_eq!(
            Warp::new(&ortho(1.0), &ortho(1.0), 200, 100, 0.0).map(5.0, 6.0),
            Some([5.0, 6.0])
        );
    }
}
