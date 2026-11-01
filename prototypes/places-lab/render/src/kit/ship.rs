//! Ship hulls: a chubby body, swept wings, glowing engines and a canopy. The recipes
//! ([`HullParams`], its sockets and fit looks) are render-free, in `galaxy_kit::ship`.
//!
//! Local axes: +X is forward (the nose), +Y is left, +Z is up. The hull is centred on the
//! origin, so `MeshInstance::at(mesh, pos, heading, 1.0)` points the nose along `heading`.

use super::rng::Rng;
use crate::color::Color;
use crate::mesh::{MeshBuilder, MeshData};
use crate::palette;
pub use galaxy_kit::ship::*;
use glam::{Mat4, Vec2, Vec3};

/// Builds a hull. `seed` adds small, stable variations (fin lean, light placement).
/// [`super::fitted_hull`] builds one wearing its fitted parts.
pub fn hull(params: &HullParams, seed: u64) -> MeshData {
    let mut b = MeshBuilder::new();
    hull_into(&mut b, &params.sanitized(), seed);
    b.build()
}

/// Adds a sanitized hull to `b`.
pub(super) fn hull_into(b: &mut MeshBuilder, p: &HullParams, seed: u64) {
    let mut rng = Rng::new(seed).fork(0x5111b);
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
    lathe_body(b, &tail_profile, c.body);
    lathe_body(b, &nose_profile, c.nose);

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
                b.frustum(0.22, 0.14, p.mast, 6);
                b.paint(c.canopy, 0.15);
                b.with(Mat4::from_translation(Vec3::Z * p.mast), |b| {
                    // A wide dish, so the Heron reads as a scout even as a small shape.
                    b.lathe(
                        &[(0.0, -0.1), (0.85, 0.0), (0.62, 0.12), (0.0, 0.22)],
                        7,
                        0.0,
                    );
                    b.paint(palette::LANTERN, 1.0);
                    b.with(Mat4::from_translation(Vec3::Z * 0.18), |b| {
                        b.lathe(&[(0.16, 0.0), (0.0, 0.2)], 5, 0.2)
                    });
                });
            },
        );
    }

    // A raised stripe along the back. The colour border inks it; no texture.
    b.paint(c.trim, 0.0);
    b.with(
        Mat4::from_translation(Vec3::new(-l * 0.04, 0.1, h * 0.46)),
        |b| {
            b.cuboid(Vec3::new(
                l * 0.5,
                (w * 0.16).max(0.34),
                (h * 0.18).max(0.22),
            ))
        },
    );

    match p.class {
        HullClass::Wren => {
            // Cheek pods: from above the Wren is a bean with two round ears.
            let cheek = (w * 0.24).max(0.4);
            for side in [1.0_f32, -1.0] {
                b.paint(c.nose, 0.0);
                b.with(
                    Mat4::from_translation(Vec3::new(l * 0.02, side * w * 0.42, h * 0.02)),
                    |b| {
                        b.lathe(
                            &[(0.0, -cheek * 0.65), (cheek, 0.0), (0.0, cheek * 0.8)],
                            6,
                            0.25,
                        );
                        b.paint(c.trim, 0.0);
                        b.with(
                            Mat4::from_translation(Vec3::new(
                                cheek * 0.15,
                                side * cheek * 0.35,
                                cheek * 0.2,
                            )),
                            |b| b.cuboid(Vec3::splat(cheek * 0.28)),
                        );
                    },
                );
            }
        }
        HullClass::Tandem => {
            // A box cab over the nose, so the hauler isn't only a longer Wren.
            b.paint(c.nose, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(l * 0.16, 0.16, h * 0.5)),
                |b| {
                    b.cuboid(Vec3::new(l * 0.36, w * 0.72, h * 0.48));
                    b.paint(c.canopy, 0.3);
                    b.with(
                        Mat4::from_translation(Vec3::new(l * 0.04, 0.0, h * 0.22)),
                        |b| b.cuboid(Vec3::new(l * 0.18, w * 0.52, h * 0.16)),
                    );
                },
            );
        }
        HullClass::Heron => {
            // Canards ahead of the long wings: a second pair, so the scout has a neck.
            let reach = w * 1.2;
            let canard = [
                Vec2::new(l * 0.2, w * 0.22),
                Vec2::new(l * 0.06, reach),
                Vec2::new(l * 0.18, reach),
                Vec2::new(l * 0.32, w * 0.22),
            ];
            let mut canard = canard.to_vec();
            if signed_area(&canard) < 0.0 {
                canard.reverse();
            }
            let thick = (h * 0.2).max(0.18);
            for side in [1.0_f32, -1.0] {
                b.paint(c.body, 0.0);
                b.with(
                    Mat4::from_scale(Vec3::new(1.0, side, 1.0))
                        * Mat4::from_translation(Vec3::Z * h * 0.05),
                    |b| b.extrude(&canard, thick, 0.9),
                );
            }
        }
    }

    // A tilted awning off the tail. Horizontal enough to read from above.
    pennant(
        b,
        (l * 0.32).max(0.9),
        Vec3::new(-l * 0.4, w * 0.08, h * 0.34),
        c.nose,
        c.trim,
    );
    if !p.mark.is_empty() {
        ship_mark(
            b,
            &p.mark,
            (w * 0.28).max(0.55),
            Vec3::new(-l * 0.08, 0.12, h * 0.64),
            c.trim,
            c.nose,
            c.engine,
        );
    }
}

/// A short banner in the XY plane, tilted up toward the tail.
fn pennant(b: &mut MeshBuilder, length: f32, at: Vec3, cloth: Color, band: Color) {
    let w = (length * 0.36).max(0.45);
    let mut shape = vec![
        Vec2::new(0.0, w * 0.42),
        Vec2::new(-length * 0.7, w * 0.36),
        Vec2::new(-length, 0.0),
        Vec2::new(-length * 0.7, -w * 0.2),
        Vec2::new(0.0, -w * 0.28),
    ];
    if signed_area(&shape) < 0.0 {
        shape.reverse();
    }
    b.paint(cloth, 0.0);
    b.with(
        Mat4::from_translation(at) * Mat4::from_rotation_y(0.5),
        |b| {
            b.extrude(&shape, 0.18, 1.0);
            b.paint(band, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(-length * 0.32, 0.04, 0.12)),
                |b| b.cuboid(Vec3::new(length * 0.14, w * 0.62, 0.14)),
            );
        },
    );
}

/// A raised faction sigil. Geometry, so the ink pass outlines it.
fn ship_mark(
    b: &mut MeshBuilder,
    mark: &str,
    size: f32,
    at: Vec3,
    trim: Color,
    accent: Color,
    glow: Color,
) {
    let s = size.max(0.4);
    b.with(Mat4::from_translation(at), |b| match mark {
        "toll_key" => {
            b.paint(trim, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(-s * 0.28, 0.0, 0.0)),
                |b| b.torus(s * 0.2, s * 0.1, s * 0.1, 8, 4),
            );
            b.with(Mat4::from_translation(Vec3::new(s * 0.12, 0.0, 0.0)), |b| {
                b.cuboid(Vec3::new(s * 0.62, s * 0.16, s * 0.14))
            });
            for x in [0.28_f32, 0.42] {
                b.with(
                    Mat4::from_translation(Vec3::new(s * x, -s * 0.16, 0.0)),
                    |b| b.cuboid(Vec3::new(s * 0.12, s * 0.18, s * 0.14)),
                );
            }
        }
        "open_lamp" => {
            b.paint(accent, 0.0);
            b.with(
                Mat4::from_translation(Vec3::new(-s * 0.16, 0.0, 0.0)),
                |b| b.cuboid(Vec3::new(s * 0.14, s * 0.55, s * 0.12)),
            );
            b.with(Mat4::from_translation(Vec3::new(s * 0.16, 0.0, 0.0)), |b| {
                b.cuboid(Vec3::new(s * 0.14, s * 0.55, s * 0.12))
            });
            b.with(
                Mat4::from_translation(Vec3::new(0.0, -s * 0.22, 0.0)),
                |b| b.cuboid(Vec3::new(s * 0.55, s * 0.14, s * 0.12)),
            );
            b.paint(glow, 1.0);
            b.with(Mat4::from_translation(Vec3::Z * 0.08), |b| {
                b.lathe(&[(0.0, -s * 0.1), (s * 0.12, 0.0), (0.0, s * 0.1)], 4, 0.4)
            });
        }
        _ => {
            b.paint(trim, 0.0);
            b.lathe(
                &[
                    (s * 0.12, 0.0),
                    (s * 0.32, s * 0.06),
                    (s * 0.28, s * 0.2),
                    (s * 0.14, s * 0.28),
                ],
                8,
                0.0,
            );
            b.paint(glow, 1.0);
            b.with(
                Mat4::from_translation(Vec3::new(s * 0.18, s * 0.16, s * 0.14)),
                |b| {
                    b.lathe(
                        &[(s * 0.1, 0.0), (s * 0.16, s * 0.1), (0.0, s * 0.18)],
                        6,
                        0.0,
                    )
                },
            );
        }
    });
}

pub(super) fn signed_area(pts: &[Vec2]) -> f32 {
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
