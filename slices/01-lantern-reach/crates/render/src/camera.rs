//! Cameras in world units.
//!
//! World convention: right-handed, **Z up**. The overworld play plane is `z = 0`, so a sim
//! position `(x, y)` maps to world `(x, y, 0)`, with +X to the right and +Y up the screen
//! in the default top-down view. Depth uses wgpu's `0..1` clip range.

use glam::camera::rh::{proj::directx, view::look_at_mat4};
use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};

/// How the camera projects the world.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Parallel projection. `view_height` is the visible height in world units.
    Orthographic {
        view_height: f32,
        near: f32,
        far: f32,
    },
    /// Perspective projection with a vertical field of view in radians.
    Perspective { fov_y: f32, near: f32, far: f32 },
}

/// A look-at camera with an orthographic or perspective projection.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub eye: Vec3,
    pub target: Vec3,
    pub up: Vec3,
    pub projection: Projection,
}

/// Distance from the play plane to a top-down camera. Large enough for tall station parts.
const TOP_DOWN_DISTANCE: f32 = 400.0;

impl Camera {
    /// The overworld camera: orthographic, looking straight down at `center` on the play
    /// plane, showing `view_height` world units from the bottom of the screen to the top.
    pub fn top_down(center: Vec2, view_height: f32) -> Self {
        Self {
            eye: center.extend(TOP_DOWN_DISTANCE),
            target: center.extend(0.0),
            up: Vec3::Y,
            projection: Projection::Orthographic {
                view_height,
                near: 1.0,
                far: TOP_DOWN_DISTANCE * 2.0,
            },
        }
    }

    /// Tilts the camera back by `degrees` around the target, keeping +Y "up the screen".
    /// A small tilt (15–30°) shows the sides of stations and ships; 0 is straight down.
    pub fn with_tilt(mut self, degrees: f32) -> Self {
        let t = degrees.to_radians();
        let dist = (self.eye - self.target).length();
        self.eye = self.target + Vec3::new(0.0, -t.sin(), t.cos()) * dist;
        self.up = Vec3::new(0.0, t.cos(), t.sin());
        self
    }

    /// A perspective camera, for station interiors, planets and other true-3D views.
    pub fn perspective(eye: Vec3, target: Vec3, fov_y_degrees: f32) -> Self {
        Self {
            eye,
            target,
            up: Vec3::Z,
            projection: Projection::Perspective {
                fov_y: fov_y_degrees.to_radians(),
                near: 0.5,
                far: 5000.0,
            },
        }
    }

    pub fn view(&self) -> Mat4 {
        let up = if (self.target - self.eye)
            .normalize()
            .cross(self.up)
            .length_squared()
            < 1e-6
        {
            // Looking along `up`: pick any perpendicular so the matrix stays valid.
            Vec3::Y
        } else {
            self.up
        };
        look_at_mat4(self.eye, self.target, up)
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        match self.projection {
            Projection::Orthographic {
                view_height,
                near,
                far,
            } => {
                let h = view_height * 0.5;
                let w = h * aspect;
                directx::orthographic(-w, w, -h, h, near, far)
            }
            Projection::Perspective { fov_y, near, far } => {
                directx::perspective(fov_y, aspect, near, far)
            }
        }
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// World units covered by one pixel at the target (orthographic: exact everywhere).
    pub fn world_per_pixel(&self, viewport: Vec2) -> f32 {
        let h = match self.projection {
            Projection::Orthographic { view_height, .. } => view_height,
            Projection::Perspective { fov_y, .. } => {
                2.0 * (self.eye - self.target).length() * (fov_y * 0.5).tan()
            }
        };
        h / viewport.y.max(1.0)
    }

    /// Projects a world point to pixel coordinates (origin top-left, +y down).
    /// Returns `None` when the point is behind a perspective camera.
    pub fn world_to_screen(&self, p: Vec3, viewport: Vec2) -> Option<Vec2> {
        let clip = self.view_proj(viewport.x / viewport.y.max(1.0)) * p.extend(1.0);
        if clip.w <= 1e-6 {
            return None;
        }
        let ndc = clip.xy() / clip.w;
        Some(Vec2::new(
            (ndc.x + 1.0) * 0.5 * viewport.x,
            (1.0 - ndc.y) * 0.5 * viewport.y,
        ))
    }

    /// The world-space ray through a pixel: `(origin, unit direction)`.
    pub fn screen_ray(&self, screen: Vec2, viewport: Vec2) -> (Vec3, Vec3) {
        let inv = self.view_proj(viewport.x / viewport.y.max(1.0)).inverse();
        let ndc = Vec2::new(
            screen.x / viewport.x * 2.0 - 1.0,
            1.0 - screen.y / viewport.y * 2.0,
        );
        let near = inv.project_point3(ndc.extend(0.0));
        let far = inv.project_point3(ndc.extend(1.0));
        (near, (far - near).normalize())
    }

    /// Where a pixel lands on the horizontal plane `z = plane_z` (click-to-fly, picking).
    pub fn screen_to_plane(&self, screen: Vec2, viewport: Vec2, plane_z: f32) -> Option<Vec3> {
        let (o, d) = self.screen_ray(screen, viewport);
        if d.z.abs() < 1e-6 {
            return None;
        }
        let t = (plane_z - o.z) / d.z;
        (t >= 0.0).then(|| o + d * t)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_down_maps_plane_to_screen_and_back() {
        let cam = Camera::top_down(Vec2::new(10.0, -5.0), 100.0);
        let vp = Vec2::new(1600.0, 900.0);
        let center = cam.world_to_screen(Vec3::new(10.0, -5.0, 0.0), vp).unwrap();
        assert!((center - vp * 0.5).length() < 1e-3);
        // +Y world is up the screen (smaller pixel y), +X is right.
        let up = cam.world_to_screen(Vec3::new(10.0, 5.0, 0.0), vp).unwrap();
        assert!(up.y < center.y && (up.x - center.x).abs() < 1e-3);
        let px = Vec2::new(1234.0, 210.0);
        let w = cam.screen_to_plane(px, vp, 0.0).unwrap();
        let back = cam.world_to_screen(w, vp).unwrap();
        assert!((back - px).length() < 1e-2, "{back} vs {px}");
        assert!((cam.world_per_pixel(vp) - 100.0 / 900.0).abs() < 1e-6);
    }

    #[test]
    fn tilted_and_perspective_cameras_round_trip() {
        let vp = Vec2::new(800.0, 600.0);
        for cam in [
            Camera::top_down(Vec2::ZERO, 60.0).with_tilt(25.0),
            Camera::perspective(Vec3::new(0.0, -80.0, 60.0), Vec3::ZERO, 45.0),
        ] {
            let p = Vec3::new(7.0, 3.0, 0.0);
            let s = cam.world_to_screen(p, vp).unwrap();
            let back = cam.screen_to_plane(s, vp, 0.0).unwrap();
            assert!((back - p).length() < 1e-2, "{back} vs {p}");
            assert!(cam.view_proj(vp.x / vp.y).is_finite());
        }
    }
}
