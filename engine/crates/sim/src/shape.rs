//! Solid shapes for collision and steering, from the models things wear.
//!
//! A body's shape is its footprint on the sim plane: how far it reaches in each
//! direction (its support function), sampled in [`DIRECTIONS`] directions. A circle
//! reaches its radius every way; a model reaches as far as its furthest vertex along each
//! direction, which traces its convex outline as drawn. Two shapes overlap when, along
//! every sampled direction, their reaches add up to more than the gap between their
//! centres (separating axes on the sampled outline); the direction with the least overlap
//! is where they push apart.
//!
//! Profiles are made from module files with plain arithmetic and `libm`, so every OS gets
//! the same bits and a replay collides exactly as the session did.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use galaxy_module::module::Module;

/// Directions a shape is sampled in, evenly round the circle starting at +x.
pub const DIRECTIONS: usize = 32;

/// The unit vectors of the sampled directions.
pub fn directions() -> &'static [(f64, f64); DIRECTIONS] {
    static DIRS: OnceLock<[(f64, f64); DIRECTIONS]> = OnceLock::new();
    DIRS.get_or_init(|| {
        std::array::from_fn(|k| {
            let a = std::f64::consts::TAU * k as f64 / DIRECTIONS as f64;
            (libm::cos(a), libm::sin(a))
        })
    })
}

/// How far a model reaches in each sampled direction, in its own frame (facing +x), at
/// morph weight 0 and at 1.
#[derive(Clone, Debug, PartialEq)]
pub struct Profile {
    pub base: [f64; DIRECTIONS],
    pub morphed: [f64; DIRECTIONS],
}

fn reach(points: impl Iterator<Item = (f64, f64)> + Clone) -> [f64; DIRECTIONS] {
    let dirs = directions();
    std::array::from_fn(|k| {
        let (dx, dy) = dirs[k];
        points
            .clone()
            .map(|(x, y)| x * dx + y * dy)
            .fold(0.0, f64::max)
    })
}

impl Profile {
    /// The profile of a model's vertices (world-frame x and y, before any turn or scale).
    pub fn of(mesh: &galaxy_mesh::MeshData) -> Profile {
        let base = mesh
            .vertices
            .iter()
            .map(|v| (f64::from(v.position[0]), f64::from(v.position[1])));
        let morphed = mesh.vertices.iter().map(|v| {
            (
                f64::from(v.morph_position[0]),
                f64::from(v.morph_position[1]),
            )
        });
        Profile {
            base: reach(base),
            morphed: reach(morphed),
        }
    }

    /// The reach along world direction `k` of the model turned by `angle`, scaled by
    /// `scale` and morphed by `morph`.
    fn reach(&self, k: usize, angle: f64, scale: f64, morph: f64) -> f64 {
        // The world direction in the model's own frame, in steps of the sampling.
        let step = std::f64::consts::TAU / DIRECTIONS as f64;
        let at = (k as f64 - angle / step).rem_euclid(DIRECTIONS as f64);
        let i = at.floor() as usize % DIRECTIONS;
        let j = (i + 1) % DIRECTIONS;
        let t = at - at.floor();
        let m = morph.clamp(0.0, 1.0);
        let r = |p: &[f64; DIRECTIONS], n: usize| p[n];
        let mix = |n: usize| r(&self.base, n) + (r(&self.morphed, n) - r(&self.base, n)) * m;
        (mix(i) + (mix(j) - mix(i)) * t) * scale
    }
}

/// The profiles of every model the modules have, by model ID (`<module>.<name>`).
#[derive(Clone, Debug, Default)]
pub struct Shapes {
    pub models: BTreeMap<String, Profile>,
}

impl Shapes {
    /// Reads every `models/<name>.gltf` of `modules`. A model that doesn't read is left
    /// out: things wearing it fall back to a circle (`galaxy-gm check` reports the file).
    pub fn of<'a>(modules: impl IntoIterator<Item = &'a Module>) -> Shapes {
        let mut models = BTreeMap::new();
        for module in modules {
            for (path, text) in &module.files.0 {
                let Some(name) = path
                    .strip_prefix("models/")
                    .and_then(|p| p.strip_suffix(".gltf"))
                else {
                    continue;
                };
                if let Ok(mesh) = galaxy_mesh::gltf::read(text) {
                    models.insert(format!("{}.{name}", module.id()), Profile::of(&mesh));
                }
            }
        }
        Shapes { models }
    }
}

/// One body's outline where it is now.
#[derive(Clone, Debug, PartialEq)]
pub struct Outline {
    /// Reach along each world direction.
    pub reach: [f64; DIRECTIONS],
}

impl Outline {
    pub fn circle(radius: f64) -> Outline {
        Outline {
            reach: [radius.max(0.0); DIRECTIONS],
        }
    }

    pub fn of_profile(profile: &Profile, angle: f64, scale: f64, morph: f64) -> Outline {
        Outline {
            reach: std::array::from_fn(|k| profile.reach(k, angle, scale, morph)),
        }
    }

    /// The furthest it reaches any way: a circle that holds it.
    pub fn bound(&self) -> f64 {
        self.reach.iter().copied().fold(0.0, f64::max)
    }
}

/// How two outlines at `a` and `b` overlap: the depth and the direction (unit, from `a`
/// towards `b`) to push them apart, or `None` when they don't touch.
pub fn overlap(
    a: &Outline,
    pa: (f64, f64),
    b: &Outline,
    pb: (f64, f64),
) -> Option<(f64, (f64, f64))> {
    let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
    let dirs = directions();
    let half = DIRECTIONS / 2;
    let mut best: Option<(f64, usize)> = None;
    for (k, &(nx, ny)) in dirs.iter().enumerate() {
        let depth = a.reach[k] + b.reach[(k + half) % DIRECTIONS] - (dx * nx + dy * ny);
        if depth <= 0.0 {
            return None;
        }
        if best.is_none_or(|(d, _)| depth < d) {
            best = Some((depth, k));
        }
    }
    best.map(|(depth, k)| (depth, dirs[k]))
}

/// The gap between two outlines along the line between their centres: how far `b`'s
/// centre is from where it would just touch `a`. Negative when they overlap.
pub fn gap(a: &Outline, pa: (f64, f64), b: &Outline, pb: (f64, f64)) -> f64 {
    let (dx, dy) = (pb.0 - pa.0, pb.1 - pa.1);
    let distance = libm::sqrt(dx * dx + dy * dy);
    distance - touching(a, b, libm::atan2(dy, dx))
}

/// How far apart two centres are when the outlines just touch along direction `angle`
/// (from `a` towards `b`).
pub fn touching(a: &Outline, b: &Outline, angle: f64) -> f64 {
    let step = std::f64::consts::TAU / DIRECTIONS as f64;
    let at = (angle / step).rem_euclid(DIRECTIONS as f64);
    let i = at.floor() as usize % DIRECTIONS;
    let j = (i + 1) % DIRECTIONS;
    let t = at - at.floor();
    let half = DIRECTIONS / 2;
    let along = |o: &Outline, k: usize| o.reach[k];
    let ra = along(a, i) + (along(a, j) - along(a, i)) * t;
    let rb = along(b, (i + half) % DIRECTIONS)
        + (along(b, (j + half) % DIRECTIONS) - along(b, (i + half) % DIRECTIONS)) * t;
    ra + rb
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circles_overlap_by_how_far_they_reach_into_each_other() {
        let a = Outline::circle(2.0);
        let b = Outline::circle(1.0);
        assert!(overlap(&a, (0.0, 0.0), &b, (3.5, 0.0)).is_none());
        let (depth, (nx, ny)) = overlap(&a, (0.0, 0.0), &b, (2.5, 0.0)).unwrap();
        assert!((depth - 0.5).abs() < 1e-9, "{depth}");
        assert!((nx - 1.0).abs() < 1e-9 && ny.abs() < 1e-9);
        assert!((gap(&a, (0.0, 0.0), &b, (3.5, 0.0)) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_turned_profile_reaches_where_the_model_points() {
        // A long thin model: 4 ahead (+x), 1 to each side, 1 behind.
        let mut base = [0.0; DIRECTIONS];
        let dirs = directions();
        for (k, r) in base.iter_mut().enumerate() {
            let (dx, dy) = dirs[k];
            *r = [(4.0, 0.0), (-1.0, 1.0), (-1.0, -1.0)]
                .iter()
                .map(|(x, y)| x * dx + y * dy)
                .fold(0.0, f64::max);
        }
        let p = Profile {
            base,
            morphed: base.map(|r| r * 0.5),
        };
        let ahead = Outline::of_profile(&p, 0.0, 1.0, 0.0);
        assert!((ahead.reach[0] - 4.0).abs() < 1e-9);
        // Turned a quarter left, it reaches 4 along +y.
        let up = Outline::of_profile(&p, std::f64::consts::FRAC_PI_2, 1.0, 0.0);
        assert!((up.reach[DIRECTIONS / 4] - 4.0).abs() < 1e-9);
        // Scaled and fully morphed, half of twice.
        let small = Outline::of_profile(&p, 0.0, 2.0, 1.0);
        assert!((small.reach[0] - 4.0).abs() < 1e-9);
    }
}
