//! FLIP (LDR), our own implementation of the published image-pair metric (Andersson et
//! al., "FLIP: A Difference Evaluator for Alternating Images", 2020): how different two
//! pictures look to a viewer flipping between them, per pixel from 0 (the same) to 1.
//!
//! Two pipelines meet at the end:
//!
//! - **Colour.** Both pictures go to an opponent colour space (YCxCz) and are blurred by
//!   what the eye can resolve at the viewing distance (contrast sensitivity, a sum of
//!   Gaussians per channel), then to L*a*b* with a Hunt adjustment (colour is harder to
//!   see in the dark). The colour error is the HyAB distance, compressed so that most of
//!   the scale goes to the differences that matter.
//! - **Features.** Edges and points are found in the brightness with derivatives of a
//!   Gaussian the size of the eye's edge detectors. Where one picture has an edge or a
//!   point the other lacks, the error is raised.
//!
//! The final error is `colour ^ (1 - feature)`. Viewing is a 0.7 m distance to a 4K
//! screen 0.7 m wide (about 67 pixels a degree), the paper's default.
//!
//! FLIP compares two still pictures; run per frame it says where and when two films
//! differ, not whether motion looks right.

/// Pixels a degree of view: 0.7 m from a 3840-pixel screen 0.7 m wide.
pub const PIXELS_PER_DEGREE: f32 = 3840.0 * std::f32::consts::PI / 180.0;

const QC: f32 = 0.7;
const QF: f32 = 0.5;
const PC: f32 = 0.4;
const PT: f32 = 0.95;

/// The FLIP error per pixel of `test` against `reference` (RGBA 8-bit sRGB, same size).
pub fn flip(reference: &crate::image::Image, test: &crate::image::Image, ppd: f32) -> Vec<f32> {
    let (w, h) = (reference.width as usize, reference.height as usize);
    let r = to_ycxcz(reference);
    let t = to_ycxcz(test);

    // Colour pipeline.
    let fr = filtered_lab(&r, w, h, ppd);
    let ft = filtered_lab(&t, w, h, ppd);
    let green = hunt(lab(xyz([0.0, 1.0, 0.0])));
    let blue = hunt(lab(xyz([0.0, 0.0, 1.0])));
    let cmax = hyab(green, blue).powf(QC);
    let colour: Vec<f32> = fr
        .iter()
        .zip(&ft)
        .map(|(a, b)| redistribute(hyab(*a, *b).powf(QC), cmax))
        .collect();

    // Feature pipeline, on brightness normalised to 0..1.
    let yr: Vec<f32> = r.iter().map(|p| (p[0] + 16.0) / 116.0).collect();
    let yt: Vec<f32> = t.iter().map(|p| (p[0] + 16.0) / 116.0).collect();
    let (er, pr) = features(&yr, w, h, ppd);
    let (et, pt) = features(&yt, w, h, ppd);
    colour
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let f = (er[i] - et[i]).abs().max((pr[i] - pt[i]).abs());
            let f = (f / std::f32::consts::SQRT_2).powf(QF);
            c.powf(1.0 - f)
        })
        .collect()
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn xyz(rgb: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = rgb;
    [
        0.412_390_8 * r + 0.357_584_3 * g + 0.180_480_8 * b,
        0.212_639 * r + 0.715_168_7 * g + 0.072_192_3 * b,
        0.019_330_8 * r + 0.119_194_8 * g + 0.950_532_2 * b,
    ]
}

fn rgb(xyz: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = xyz;
    [
        3.240_97 * x - 1.537_383_2 * y - 0.498_610_8 * z,
        -0.969_243_6 * x + 1.875_967_5 * y + 0.041_555_1 * z,
        0.055_630_1 * x - 0.203_977 * y + 1.056_971_5 * z,
    ]
}

/// The reference white: XYZ of linear RGB (1, 1, 1).
fn white() -> [f32; 3] {
    xyz([1.0, 1.0, 1.0])
}

fn to_ycxcz(img: &crate::image::Image) -> Vec<[f32; 3]> {
    let wt = white();
    img.pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| {
            let lin = [0, 1, 2].map(|k| srgb_to_linear(f32::from(p[k]) / 255.0));
            let c = xyz(lin);
            let n = [c[0] / wt[0], c[1] / wt[1], c[2] / wt[2]];
            [
                116.0 * n[1] - 16.0,
                500.0 * (n[0] - n[1]),
                200.0 * (n[1] - n[2]),
            ]
        })
        .collect()
}

fn ycxcz_to_linear_rgb(p: [f32; 3]) -> [f32; 3] {
    let wt = white();
    let y = (p[0] + 16.0) / 116.0;
    let x = p[1] / 500.0 + y;
    let z = y - p[2] / 200.0;
    rgb([x * wt[0], y * wt[1], z * wt[2]])
}

fn lab(xyz: [f32; 3]) -> [f32; 3] {
    let wt = white();
    let f = |t: f32| {
        let d = 6.0f32 / 29.0;
        if t > d * d * d {
            t.cbrt()
        } else {
            t / (3.0 * d * d) + 4.0 / 29.0
        }
    };
    let (fx, fy, fz) = (f(xyz[0] / wt[0]), f(xyz[1] / wt[1]), f(xyz[2] / wt[2]));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

fn hunt(lab: [f32; 3]) -> [f32; 3] {
    [lab[0], 0.01 * lab[0] * lab[1], 0.01 * lab[0] * lab[2]]
}

fn hyab(a: [f32; 3], b: [f32; 3]) -> f32 {
    (a[0] - b[0]).abs() + ((a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn redistribute(e: f32, cmax: f32) -> f32 {
    let pccmax = PC * cmax;
    if e < pccmax {
        PT / pccmax * e
    } else {
        PT + (e - pccmax) / (cmax - pccmax) * (1.0 - PT)
    }
}

/// The contrast sensitivity filter for one channel: two Gaussians `(a, b)`, as 1-D
/// kernels (each Gaussian separates), weighted so the whole 2-D filter sums to 1.
fn csf(channel: usize, ppd: f32) -> (Vec<f32>, Vec<f32>, usize) {
    let params: [[f32; 4]; 3] = [
        [1.0, 0.0047, 0.0, 1e-5],
        [1.0, 0.0053, 0.0, 1e-5],
        [34.1, 0.04, 13.5, 0.025],
    ];
    let [a1, b1, a2, b2] = params[channel];
    let max_b = 0.04f32;
    let r = (3.0 * (max_b / (2.0 * std::f32::consts::PI.powi(2))).sqrt() * ppd).ceil() as usize;
    let dx = 1.0 / ppd;
    let pi2 = std::f32::consts::PI.powi(2);
    let g1: Vec<f32> = (-(r as i32)..=r as i32)
        .map(|x| (-pi2 * (x as f32 * dx).powi(2) / b1).exp())
        .collect();
    let g2: Vec<f32> = (-(r as i32)..=r as i32)
        .map(|x| (-pi2 * (x as f32 * dx).powi(2) / b2).exp())
        .collect();
    let s1: f32 = g1.iter().sum();
    let s2: f32 = g2.iter().sum();
    let k1 = a1 * (std::f32::consts::PI / b1).sqrt();
    let k2 = a2 * (std::f32::consts::PI / b2).sqrt();
    let total = k1 * s1 * s1 + k2 * s2 * s2;
    // Fold the weights into one kernel of each pair, so filter = sum of the two outer
    // products.
    let w1 = (k1 / total).sqrt();
    let w2 = (k2 / total).sqrt();
    (
        g1.iter().map(|v| v * w1).collect(),
        g2.iter().map(|v| v * w2).collect(),
        r,
    )
}

/// Convolves with `k` along x then y (clamped edges).
fn separable(src: &[f32], w: usize, h: usize, kx: &[f32], ky: &[f32]) -> Vec<f32> {
    let rx = kx.len() / 2;
    let ry = ky.len() / 2;
    let mut tmp = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (i, k) in kx.iter().enumerate() {
                let xx = (x + i).saturating_sub(rx).min(w - 1);
                s += k * src[y * w + xx];
            }
            tmp[y * w + x] = s;
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            for (i, k) in ky.iter().enumerate() {
                let yy = (y + i).saturating_sub(ry).min(h - 1);
                s += k * tmp[yy * w + x];
            }
            out[y * w + x] = s;
        }
    }
    out
}

fn filtered_lab(img: &[[f32; 3]], w: usize, h: usize, ppd: f32) -> Vec<[f32; 3]> {
    let mut channels = Vec::new();
    for c in 0..3 {
        let plane: Vec<f32> = img.iter().map(|p| p[c]).collect();
        let (g1, g2, _) = csf(c, ppd);
        let a = separable(&plane, w, h, &g1, &g1);
        let b = separable(&plane, w, h, &g2, &g2);
        channels.push(a.iter().zip(&b).map(|(a, b)| a + b).collect::<Vec<f32>>());
    }
    (0..w * h)
        .map(|i| {
            let lin = ycxcz_to_linear_rgb([channels[0][i], channels[1][i], channels[2][i]])
                .map(|v| v.clamp(0.0, 1.0));
            hunt(lab(xyz(lin)))
        })
        .collect()
}

/// Edge and point strength per pixel.
fn features(y: &[f32], w: usize, h: usize, ppd: f32) -> (Vec<f32>, Vec<f32>) {
    let sd = 0.5 * 0.082 * ppd;
    let r = (3.0 * sd).ceil() as i32;
    let xs: Vec<f32> = (-r..=r).map(|x| x as f32).collect();
    let g: Vec<f32> = xs
        .iter()
        .map(|x| (-(x * x) / (2.0 * sd * sd)).exp())
        .collect();
    let gsum: f32 = g.iter().sum();
    // Positive weights of the 2-D filter sum to 1 and negative to -1; the 2-D filter is
    // the 1-D one along x times `g` along y, so scale the 1-D one by the sums.
    let norm = |f: Vec<f32>| -> Vec<f32> {
        let pos: f32 = f.iter().filter(|v| **v > 0.0).sum::<f32>() * gsum;
        let neg: f32 = -f.iter().filter(|v| **v < 0.0).sum::<f32>() * gsum;
        f.iter()
            .map(|v| {
                if *v > 0.0 {
                    v / pos
                } else if *v < 0.0 {
                    v / neg
                } else {
                    0.0
                }
            })
            .collect()
    };
    let edge = norm(xs.iter().zip(&g).map(|(x, g)| -x * g).collect());
    let point = norm(
        xs.iter()
            .zip(&g)
            .map(|(x, g)| (x * x / (sd * sd) - 1.0) * g)
            .collect(),
    );
    let ex = separable(y, w, h, &edge, &g);
    let ey = separable(y, w, h, &g, &edge);
    let px = separable(y, w, h, &point, &g);
    let py = separable(y, w, h, &g, &point);
    let e = ex
        .iter()
        .zip(&ey)
        .map(|(a, b)| (a * a + b * b).sqrt())
        .collect();
    let p = px
        .iter()
        .zip(&py)
        .map(|(a, b)| (a * a + b * b).sqrt())
        .collect();
    (e, p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::{BLACK, Image};

    #[test]
    fn identical_pictures_score_zero_and_a_change_scores_where_it_is() {
        let mut a = Image::new(64, 48, BLACK);
        a.fill_rect(10, 10, 12, 12, [200, 120, 60, 255]);
        let e = flip(&a, &a, PIXELS_PER_DEGREE);
        assert!(
            e.iter().all(|v| *v < 1e-4),
            "{}",
            e.iter().fold(0.0f32, |m, v| m.max(*v))
        );
        let mut b = a.clone();
        b.fill_rect(40, 20, 8, 8, [255, 255, 255, 255]);
        let e = flip(&a, &b, PIXELS_PER_DEGREE);
        let at = |x: usize, y: usize| e[y * 64 + x];
        assert!(at(44, 24) > 0.5, "{}", at(44, 24));
        assert!(at(2, 40) < 0.05, "{}", at(2, 40));
    }

    #[test]
    fn the_colour_filter_sums_to_one() {
        for c in 0..3 {
            let (g1, g2, _) = csf(c, PIXELS_PER_DEGREE);
            let s1: f32 = g1.iter().sum();
            let s2: f32 = g2.iter().sum();
            assert!(
                (s1 * s1 + s2 * s2 - 1.0).abs() < 1e-3,
                "{c}: {}",
                s1 * s1 + s2 * s2
            );
        }
    }
}
