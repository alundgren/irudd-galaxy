//! A camera gliding from one pose to another, for travel and room camera changes.
//!
//! Each camera becomes a framing: the point it looks at, the direction it looks, its up,
//! how many world units it shows top to bottom at that point, and its field of view. An
//! orthographic camera is a perspective one with an almost closed field of view, so a
//! follow camera can glide into a room's posed camera: the framing widens its field of
//! view while it closes in, like a dolly zoom. The framings are blended, so the shown
//! height changes by a steady ratio and the eye never swings through the floor.
use galaxy_render::glam::Vec3;
use galaxy_render::{Camera, Projection};

/// The field of view an orthographic camera glides from, radians.
const FLAT_FOV: f32 = 2.0 * std::f32::consts::PI / 180.0;

#[derive(Clone, Copy, Debug, PartialEq)]
struct Framing {
    focus: Vec3,
    dir: Vec3,
    up: Vec3,
    height: f32,
    fov: f32,
}

impl Framing {
    fn of(c: &Camera) -> Framing {
        let dir = (c.target - c.eye).normalize_or(Vec3::NEG_Z);
        match c.projection {
            Projection::Orthographic { view_height, .. } => Framing {
                focus: c.target,
                dir,
                up: c.up,
                height: view_height.max(0.01),
                fov: FLAT_FOV,
            },
            Projection::Perspective { fov_y, .. } => {
                // Look at the floor (z = 0) where the view meets it, else 10 units ahead.
                let distance = if dir.z < -1e-3 {
                    (-c.eye.z / dir.z).clamp(1.0, 1000.0)
                } else {
                    10.0
                };
                let fov = fov_y.max(FLAT_FOV);
                Framing {
                    focus: c.eye + dir * distance,
                    dir,
                    up: c.up,
                    height: 2.0 * distance * (fov * 0.5).tan(),
                    fov,
                }
            }
        }
    }

    fn camera(&self) -> Camera {
        let distance = self.height * 0.5 / (self.fov * 0.5).tan();
        let eye = self.focus - self.dir * distance;
        Camera {
            eye,
            target: self.focus,
            up: self.up,
            projection: Projection::Perspective {
                fov_y: self.fov,
                near: (distance * 0.05).max(0.1),
                far: distance * 20.0 + 5000.0,
            },
        }
    }
}

/// The camera `t` (0 to 1, already eased) of the way from `from` to `to`. Across places
/// the two cameras live in different coordinates, so the glide keeps `from`'s angle,
/// height and field of view but looks at `to`'s focus throughout. `t = 1` is exactly `to`.
pub fn between(from: Camera, to: Camera, t: f32, across: bool) -> Camera {
    if t >= 1.0 {
        return to;
    }
    let pose = [(from, 1.0 - t), (to, t)];
    let focus: &[(Camera, f32)] = if across { &[(to, 1.0)] } else { &pose };
    blend(&pose, focus, to)
}

/// The camera whose framing blends `pose`'s framings by weight (direction, up, shown
/// height and field of view) and looks at `focus`'s points by weight. A chain of glides
/// that take each other over is one such blend, so it never has to be nested. `to` is
/// the fallback when the weights cancel out.
pub fn blend(pose: &[(Camera, f32)], focus: &[(Camera, f32)], to: Camera) -> Camera {
    /// The framings of `parts` with their weights scaled to a total of one; None for
    /// no weight.
    fn framings(parts: &[(Camera, f32)]) -> Option<Vec<(Framing, f32)>> {
        let total: f32 = parts.iter().map(|(_, w)| w).sum();
        (total > 0.0).then(|| {
            parts
                .iter()
                .map(|(c, w)| (Framing::of(c), w / total))
                .collect()
        })
    }
    let Some(pose) = framings(pose) else {
        return to;
    };
    let b = Framing::of(&to);
    let vec = |parts: &[(Framing, f32)], of: fn(&Framing) -> Vec3| {
        parts.iter().map(|(f, w)| of(f) * *w).sum::<Vec3>()
    };
    let mean = |of: fn(&Framing) -> f32| pose.iter().map(|(f, w)| of(f) * w).sum::<f32>();
    let dir = vec(&pose, |f| f.dir).normalize_or(b.dir);
    let up = vec(&pose, |f| f.up);
    // Keep up square to the view, so the picture never shears.
    let up = (up - dir * up.dot(dir)).normalize_or(b.up);
    Framing {
        focus: framings(focus).map_or(b.focus, |f| vec(&f, |f| f.focus)),
        dir,
        up,
        // The shown height changes by a steady ratio: its logarithm is blended.
        height: mean(|f| f.height.ln()).exp(),
        fov: mean(|f| f.fov),
    }
    .camera()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posed() -> Camera {
        Camera {
            eye: Vec3::new(-14.0, 0.0, 7.0),
            target: Vec3::new(-13.1, 0.0, 6.6),
            up: Vec3::Z,
            projection: Projection::Perspective {
                fov_y: 40f32.to_radians(),
                near: 0.1,
                far: 5000.0,
            },
        }
    }

    #[test]
    fn a_glide_ends_exactly_on_its_destination_and_starts_on_its_framing() {
        let flat = Camera {
            eye: Vec3::new(0.0, -200.0, 400.0),
            target: Vec3::ZERO,
            up: Vec3::Y,
            projection: Projection::Orthographic {
                view_height: 64.0,
                near: 1.0,
                far: 900.0,
            },
        };
        assert_eq!(between(flat, posed(), 1.0, true), posed());
        // It starts looking at the same point, showing the same height there.
        let start = between(flat, posed(), 0.0, false);
        let Projection::Perspective { fov_y, .. } = start.projection else {
            panic!("a glide is drawn in perspective");
        };
        let shown = 2.0 * (start.eye - start.target).length() * (fov_y * 0.5).tan();
        assert!((shown - 64.0).abs() < 0.01, "{shown}");
        assert!(start.target.length() < 0.01);
        // The eye closes in steadily on the destination.
        let to = posed().eye;
        let mut last = f32::MAX;
        for i in 0..=20 {
            let eye = between(flat, posed(), i as f32 / 20.0, true).eye;
            let d = (eye - to).length();
            assert!(d <= last + 1e-3, "step {i}: {d} after {last}");
            last = d;
        }
    }
}
