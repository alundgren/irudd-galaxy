//! Ship hulls: a chubby body, swept wings, glowing engines and a canopy.
//!
//! Local axes: +X is forward (the nose), +Y is left, +Z is up. The hull is centred on the
//! origin, so `MeshInstance::at(mesh, pos, heading, 1.0)` points the nose along `heading`.

use super::rng::Rng;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use crate::palette;
use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// The three hulls in the slice.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HullClass {
    /// Tier 0: a small, round, friendly first ship.
    Wren,
    /// Hauler that pairs with automation: boxy body, trailing cargo pods.
    Tandem,
    /// Scout with long price reach: slender body, long wings, a sensor mast.
    Heron,
}

/// Paint for a hull.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct HullColors {
    pub body: Color,
    pub nose: Color,
    pub trim: Color,
    pub canopy: Color,
    pub engine: Color,
}

impl Default for HullColors {
    fn default() -> Self {
        Self {
            body: palette::CORAL,
            nose: palette::HULL_CREAM,
            trim: palette::MUSTARD,
            canopy: palette::GLASS,
            engine: palette::LANTERN,
        }
    }
}

/// Parameters for [`hull`]. Lengths are world units.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HullParams {
    pub class: HullClass,
    /// Nose to tail.
    pub length: f32,
    /// Widest point of the body.
    pub width: f32,
    /// Body thickness.
    pub height: f32,
    /// 0 is a blunt bean, 1 a sharp beak.
    pub nose: f32,
    /// Wing tip to wing tip (0 for no wings).
    pub wing_span: f32,
    /// How far back the wings sweep, `0..=1`.
    pub wing_sweep: f32,
    /// 1 to 3 engines.
    pub engines: u32,
    /// Cargo pods trailing behind (the Tandem's signature).
    pub cargo_pods: u32,
    /// Tail fin height (0 for none).
    pub fin: f32,
    /// Forward sensor mast length (the Heron's signature; 0 for none).
    pub mast: f32,
    /// Facets around the body; fewer reads as more hand-cut.
    pub facets: u32,
    pub colors: HullColors,
}

impl Default for HullParams {
    fn default() -> Self {
        Self::wren()
    }
}

impl HullParams {
    pub fn wren() -> Self {
        Self {
            class: HullClass::Wren,
            length: 6.0,
            width: 3.2,
            height: 2.0,
            nose: 0.35,
            wing_span: 6.4,
            wing_sweep: 0.45,
            engines: 1,
            cargo_pods: 0,
            fin: 1.1,
            mast: 0.0,
            facets: 8,
            colors: HullColors::default(),
        }
    }

    pub fn tandem() -> Self {
        Self {
            class: HullClass::Tandem,
            length: 8.0,
            width: 3.6,
            height: 2.4,
            nose: 0.15,
            wing_span: 5.6,
            wing_sweep: 0.2,
            engines: 2,
            cargo_pods: 2,
            fin: 0.8,
            mast: 0.0,
            facets: 6,
            colors: HullColors {
                body: palette::TEAL,
                nose: palette::HULL_CREAM,
                trim: palette::TERRACOTTA,
                ..HullColors::default()
            },
        }
    }

    pub fn heron() -> Self {
        Self {
            class: HullClass::Heron,
            length: 9.0,
            width: 2.4,
            height: 1.6,
            nose: 0.9,
            wing_span: 10.0,
            wing_sweep: 0.75,
            engines: 1,
            cargo_pods: 0,
            fin: 1.4,
            mast: 3.0,
            facets: 8,
            colors: HullColors {
                body: palette::HULL_CREAM,
                nose: palette::NEBULA_VIOLET,
                trim: palette::TEAL,
                ..HullColors::default()
            },
        }
    }

    pub fn for_class(class: HullClass) -> Self {
        match class {
            HullClass::Wren => Self::wren(),
            HullClass::Tandem => Self::tandem(),
            HullClass::Heron => Self::heron(),
        }
    }

    /// Clamps every field into a range that builds a sane mesh.
    pub fn sanitized(&self) -> Self {
        let f = |v: f32, lo: f32, hi: f32| if v.is_finite() { v.clamp(lo, hi) } else { lo };
        Self {
            class: self.class,
            length: f(self.length, 1.0, 60.0),
            width: f(self.width, 0.4, 30.0),
            height: f(self.height, 0.3, 20.0),
            nose: f(self.nose, 0.0, 1.0),
            wing_span: f(self.wing_span, 0.0, 80.0),
            wing_sweep: f(self.wing_sweep, 0.0, 1.0),
            engines: self.engines.clamp(1, 3),
            cargo_pods: self.cargo_pods.min(4),
            fin: f(self.fin, 0.0, 20.0),
            mast: f(self.mast, 0.0, 40.0),
            facets: self.facets.clamp(5, 16),
            colors: self.colors,
        }
    }
}

/// Builds a hull. `seed` adds small, stable variations (fin lean, light placement).
pub fn hull(params: &HullParams, seed: u64) -> MeshData {
    let p = params.sanitized();
    let mut rng = Rng::new(seed).fork(0x5111b);
    let mut b = MeshBuilder::new();
    let (l, w, h) = (p.length, p.width, p.height);
    let c = p.colors;
    let boxy = matches!(p.class, HullClass::Tandem);

    // Body: a lathe along +X, squashed to an oval cross-section. Profile is (radius, x) in
    // units of the half-width, from tail to nose.
    let tip = 0.06 + 0.12 * (1.0 - p.nose);
    let shoulder = if boxy { 0.98 } else { 0.88 };
    let tail_profile = [(0.55, -0.5), (0.86, -0.36), (1.0, -0.12), (shoulder, 0.08)];
    let nose_profile = [
        (shoulder, 0.08),
        (0.72 - 0.25 * p.nose, 0.28),
        (0.42 - 0.2 * p.nose, 0.42),
        (tip, 0.5),
    ];
    let body = Mat4::from_scale(Vec3::new(l, w * 0.5, h * 0.5))
        * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2);
    // After the rotation the lathe axis runs along +X (scaled by the length) and the lathe
    // radius spans Y and Z (scaled by the half-width and half-height).
    let lathe_body = |b: &mut MeshBuilder, prof: &[(f32, f32)], color: Color| {
        b.paint(color, 0.0);
        b.with(body, |b| {
            b.lathe(prof, p.facets, std::f32::consts::PI / p.facets as f32)
        });
    };
    lathe_body(&mut b, &tail_profile, c.body);
    lathe_body(&mut b, &nose_profile, c.nose);

    // A trim band around the waist.
    b.paint(c.trim, 0.0);
    b.with(body, |b| {
        b.lathe(
            &[(1.04, -0.16), (1.04, -0.06)],
            p.facets,
            std::f32::consts::PI / p.facets as f32,
        )
    });

    // Canopy: a small faceted bubble on the upper front.
    b.paint(c.canopy, 0.3);
    let canopy_r = w * 0.27;
    b.with(
        Mat4::from_translation(Vec3::new(l * 0.13, 0.0, h * 0.30))
            * Mat4::from_scale(Vec3::new(1.5, 1.0, 0.8)),
        |b| {
            b.lathe(
                &[
                    (canopy_r, 0.0),
                    (canopy_r * 0.85, canopy_r * 0.55),
                    (0.0, canopy_r),
                ],
                6,
                0.0,
            )
        },
    );

    // Wings, mirrored. Outline in (x forward, y outward).
    if p.wing_span > w {
        let half = p.wing_span * 0.5;
        let root = l * if boxy { 0.34 } else { 0.42 };
        let sweep = p.wing_sweep * l * 0.35;
        let tip_chord = root * 0.35;
        let x0 = l * 0.08;
        let wing = [
            Vec2::new(x0 - root, w * 0.3),
            Vec2::new(x0 - sweep - tip_chord, half),
            Vec2::new(x0 - sweep, half),
            Vec2::new(x0, w * 0.3),
        ];
        // Wings lie mostly flat; ensure counter-clockwise order for `extrude`.
        let mut wing = wing.to_vec();
        if signed_area(&wing) < 0.0 {
            wing.reverse();
        }
        let thick = (h * 0.16).max(0.12);
        for side in [1.0, -1.0] {
            b.paint(c.trim, 0.0);
            b.with(
                Mat4::from_scale(Vec3::new(1.0, side, 1.0))
                    * Mat4::from_translation(Vec3::new(0.0, 0.0, -h * 0.12)),
                |b| {
                    b.extrude(&wing, thick, 0.92);
                    // Wingtip light: warm on the left, teal on the right.
                    let light = if side > 0.0 {
                        palette::BLOSSOM
                    } else {
                        palette::TEAL
                    };
                    b.paint(light, 0.9);
                    b.with(
                        Mat4::from_translation(Vec3::new(
                            x0 - sweep - tip_chord * 0.5,
                            half - 0.15,
                            thick,
                        )),
                        |b| b.cuboid(Vec3::splat(0.35)),
                    );
                },
            );
        }
    }

    // Tail fin.
    if p.fin > 0.05 {
        let lean = rng.range(-0.05, 0.05);
        b.paint(c.trim, 0.0);
        let fin = [
            Vec2::new(-l * 0.46, 0.0),
            Vec2::new(-l * 0.12, 0.0),
            Vec2::new(-l * 0.38, p.fin),
            Vec2::new(-l * 0.5, p.fin * 0.9),
        ];
        b.with(
            Mat4::from_translation(Vec3::new(0.0, 0.12, h * 0.28))
                * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2 + lean),
            |b| b.extrude(&fin, 0.24, 0.85),
        );
    }

    // Engines at the tail with glowing nozzles.
    let n = p.engines as i32;
    let r = (w * if n == 1 { 0.3 } else { 0.2 }).min(h * 0.4);
    for i in 0..n {
        let y = (i as f32 - (n - 1) as f32 * 0.5) * w * 0.42;
        let at = Mat4::from_translation(Vec3::new(-l * 0.44, y, -h * 0.02))
            * Mat4::from_rotation_y(-std::f32::consts::FRAC_PI_2);
        b.with(at, |b| {
            b.paint(palette::BRASS, 0.0);
            b.lathe(
                &[(r * 0.9, -0.1), (r * 1.15, r * 0.9), (r * 1.0, r * 1.6)],
                8,
                0.0,
            );
            b.paint(c.engine, 1.0);
            b.lathe(&[(r * 0.75, r * 1.62), (0.0, r * 1.68)], 8, 0.0);
        });
    }

    // Cargo pods trail behind on a coupling.
    let engine_back = -l * 0.44 - r * 1.7;
    for i in 0..p.cargo_pods {
        let x = engine_back - 0.4 - w * 0.45 - i as f32 * w * 1.15;
        b.paint(palette::BRASS, 0.0);
        b.with(
            Mat4::from_translation(Vec3::new(x + w * 0.5, 0.0, 0.0)),
            |b| b.cuboid(Vec3::new(w * 0.5, w * 0.18, h * 0.18)),
        );
        b.paint(if i % 2 == 0 { c.nose } else { c.trim }, 0.0);
        b.with(Mat4::from_translation(Vec3::new(x, 0.0, 0.0)), |b| {
            b.cuboid(Vec3::new(w * 0.9, w * 0.9, h * 0.8));
            b.paint(c.body, 0.0);
            b.cuboid(Vec3::new(w * 0.95, w * 0.3, h * 0.84));
        });
    }

    // Sensor mast out of the nose.
    if p.mast > 0.05 {
        b.paint(c.trim, 0.0);
        b.with(
            Mat4::from_translation(Vec3::new(l * 0.45, 0.0, 0.0))
                * Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2),
            |b| {
                b.frustum(0.16, 0.08, p.mast, 5);
                b.paint(palette::LANTERN, 1.0);
                b.with(Mat4::from_translation(Vec3::Z * p.mast), |b| {
                    b.lathe(&[(0.0, -0.2), (0.25, 0.0), (0.0, 0.25)], 5, 0.0)
                });
            },
        );
    }

    b.build()
}

fn signed_area(pts: &[Vec2]) -> f32 {
    (0..pts.len())
        .map(|i| pts[i].perp_dot(pts[(i + 1) % pts.len()]))
        .sum::<f32>()
        * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_class_builds_sane_meshes() {
        for class in [HullClass::Wren, HullClass::Tandem, HullClass::Heron] {
            let p = HullParams::for_class(class);
            let m = hull(&p, 7);
            assert!(m.is_well_formed(), "{class:?}");
            assert!(m.triangle_count() > 50, "{class:?}");
            let (lo, hi) = m.bounds().unwrap();
            // Nose ahead, tail behind, and nothing absurdly far out.
            assert!(hi.x > p.length * 0.4 && lo.x < -p.length * 0.4, "{class:?}");
            assert!(
                m.radius() < p.length * 2.0 + p.mast + p.wing_span,
                "{class:?}"
            );
            assert!((hi.y - lo.y) >= p.wing_span * 0.9, "{class:?}");
        }
    }

    #[test]
    fn deterministic_for_a_seed() {
        let p = HullParams::wren();
        assert_eq!(hull(&p, 3), hull(&p, 3));
    }

    #[test]
    fn wild_params_are_clamped() {
        let p = HullParams {
            length: f32::NAN,
            width: -3.0,
            engines: 99,
            facets: 1000,
            ..HullParams::tandem()
        };
        assert!(hull(&p, 1).is_well_formed());
    }

    #[test]
    fn params_round_trip_through_json() {
        let p = HullParams::heron();
        let json = serde_json::to_string(&p).unwrap();
        let back: HullParams = serde_json::from_str(&json).unwrap();
        // Colours are stored as 8-bit hex, so compare the stable JSON form.
        assert_eq!(serde_json::to_string(&back).unwrap(), json);
        assert_eq!(back.length, p.length);
        // Missing fields fall back to the Wren defaults.
        let partial: HullParams = serde_json::from_str(r#"{"length": 7.5}"#).unwrap();
        assert_eq!(partial.length, 7.5);
        assert_eq!(partial.class, HullClass::Wren);
    }
}
