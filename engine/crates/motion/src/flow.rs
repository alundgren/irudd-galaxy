//! `flow`: apparent motion in the pixels, where there is no named thing to track.
//!
//! Each frame is compared with the one before, moved into its view so the camera's
//! follow cancels out, and the motion left is measured at textured points on a grid with
//! pyramidal Lucas–Kanade optical flow: at each point, the small shift that best explains
//! how the picture round it changed, found coarse to fine. What is still in the world
//! measures zero. Something fixed on screen moves exactly against the camera's step; a
//! parallax backdrop moves with a share of it, which the tool fits over the film and sets
//! aside. What is left is things moving in the world, and patterns crawling
//! where they should stand still.
//!
//! `--along course` (or `--along x,y;x,y`) also measures the flow at points along that
//! path, in the path's direction: a second way to see a crawling pattern, besides
//! `strip`. Periodic patterns can fool flow into matching the wrong neighbour when they
//! move more than half their period a frame.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::image::{ACCENT, CALM, FLAG, Gray, Image, TEXT};
use crate::opts::Opts;
use crate::strip::{Path, wrap};
use crate::track::median;
use crate::warp::{Warp, compensate, gray};

const HALF: i32 = 3;
const LEVELS: usize = 3;
/// A cell moving at least this (px a frame) beyond the background is reported.
const MOVING: f32 = 0.3;
/// ... in a steady direction (mean vector over mean length).
const COHERENT: f32 = 0.6;

/// The options `flow` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["along", "region", "screen", "step"];

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("flow", OPTIONS)?;
    film.need_pictures("flow")?;
    let frames = film.frames();
    let (w, h) = film.size();
    let screen = opts.flag("screen");
    let step = opts.count("step")?.unwrap_or(12).clamp(4, 64) as u32;
    let region = opts.rect("region")?;
    let path = match opts.text("along") {
        Some(_) => Some(Path::choose(film, opts)?),
        None => None,
    };
    let (cols, rows) = (16usize, 9usize);
    let mut cells = vec![Cell::default(); cols * rows];
    // The camera's step each frame, as screen-fixed content seems to move after
    // compensation, and how far textured points went along it.
    let mut camera_step: Vec<[f32; 2]> = vec![[0.0, 0.0]];
    let mut shares: Vec<f32> = Vec::new();
    let mut along: Vec<Option<f32>> = vec![None];
    let mut prev: Option<Gray> = None;
    let mut per_frame: Vec<Vec<([f32; 2], [f32; 2])>> = vec![Vec::new()];
    for i in 0..frames.len() {
        let cur = gray(&film.image(i)?, 1);
        let Some(p) = prev.take() else {
            prev = Some(cur);
            continue;
        };
        let warp = if screen {
            Warp::identity(w, h)
        } else {
            Warp::new(&frames[i].camera, &frames[i - 1].camera, w, h, 0.0)
        };
        let moved = Gray {
            width: w,
            height: h,
            data: compensate(&p, &warp, 1.0),
        };
        let (a, b) = (pyramid(moved), pyramid(cur.clone()));
        let mut found = Vec::new();
        for y in (step / 2..h).step_by(step as usize) {
            for x in (step / 2..w).step_by(step as usize) {
                if let Some(r) = region
                    && !(x as f32 >= r[0]
                        && y as f32 >= r[1]
                        && (x as f32) < r[0] + r[2]
                        && (y as f32) < r[1] + r[3])
                {
                    continue;
                }
                if let Some(v) = lucas_kanade(&a, &b, x as f32, y as f32) {
                    found.push(([x as f32, y as f32], v));
                }
            }
        }
        let centre = [w as f32 / 2.0, h as f32 / 2.0];
        let t = warp
            .map(centre[0], centre[1])
            .map_or([0.0, 0.0], |m| [m[0] - centre[0], m[1] - centre[1]]);
        let tt = t[0] * t[0] + t[1] * t[1];
        if tt > MOVING * MOVING {
            for (_, v) in &found {
                if len(*v) > MOVING {
                    shares.push((v[0] * t[0] + v[1] * t[1]) / tt);
                }
            }
        }
        camera_step.push(t);
        along.push(path.as_ref().and_then(|path| {
            let pts = path.screen_points(&frames[i], w, h);
            let mut v = Vec::new();
            for k in (2..pts.len().saturating_sub(2)).step_by(3) {
                let (Some(p), Some(q0), Some(q1)) = (pts[k], pts[k - 2], pts[k + 2]) else {
                    continue;
                };
                let t = [q1[0] - q0[0], q1[1] - q0[1]];
                let l = (t[0] * t[0] + t[1] * t[1]).sqrt().max(1e-6);
                if let Some(f) = lucas_kanade(&a, &b, p[0], p[1]) {
                    v.push((f[0] * t[0] + f[1] * t[1]) / l);
                }
            }
            (v.len() >= 3).then(|| median(&v))
        }));
        per_frame.push(found);
        prev = Some(cur);
    }
    // The backdrop's share of the camera's step: the share most moving points follow.
    let share = mode(&shares);
    for (i, found) in per_frame.iter().enumerate() {
        let t = camera_step.get(i).copied().unwrap_or([0.0, 0.0]);
        let bg = [t[0] * share, t[1] * share];
        for &(at, v) in found {
            // Explained if it is still in the world or drifts with the backdrop.
            let still = len(v);
            let sky = len([v[0] - bg[0], v[1] - bg[1]]);
            if still.min(sky) < MOVING {
                continue;
            }
            let c = ((at[1] as usize * rows / h as usize) * cols
                + at[0] as usize * cols / w as usize)
                .min(cols * rows - 1);
            let cell = &mut cells[c];
            cell.sum[0] += v[0];
            cell.sum[1] += v[1];
            cell.length += still;
            cell.count += 1;
            if cell.frames.last() != Some(&i) {
                cell.frames.push(i);
            }
        }
    }
    let n = frames.len();
    let mut ranked: Vec<(usize, Cell)> = cells
        .into_iter()
        .enumerate()
        .filter(|(_, c)| c.count > 0)
        .collect();
    ranked.sort_by(|a, b| b.1.length.total_cmp(&a.1.length));
    let cell_box = |c: usize| {
        let (cx, cy) = (c % cols, c / cols);
        [
            cx as u32 * w / cols as u32,
            cy as u32 * h / rows as u32,
            w / cols as u32,
            h / rows as u32,
        ]
    };
    let mid = n / 2;
    let near = |b: [u32; 4]| -> Vec<String> {
        frames[mid]
            .things
            .iter()
            .filter(|(_, t)| {
                t.screen.is_some_and(|s| {
                    s[0] >= b[0] as f32
                        && s[0] <= (b[0] + b[2]) as f32
                        && s[1] >= b[1] as f32
                        && s[1] <= (b[1] + b[3]) as f32
                })
            })
            .map(|(id, _)| id.clone())
            .collect()
    };
    let report_cells: Vec<Value> = ranked
        .iter()
        .take(8)
        .map(|(c, cell)| {
            let mean = [
                cell.sum[0] / cell.count as f32,
                cell.sum[1] / cell.count as f32,
            ];
            let coherence = len(mean) / (cell.length / cell.count as f32).max(1e-6);
            json!({
                "box_px": cell_box(*c),
                "px_per_frame": [round(mean[0], 2), round(mean[1], 2)],
                "coherence": round(coherence, 2),
                "share_of_frames": round(cell.frames.len() as f32 / (n - 1).max(1) as f32, 2),
                "first": cell.frames.first().map(|&i| when(&frames[i])),
                "near_mid_film": near(cell_box(*c)),
            })
        })
        .collect();
    let steady: Vec<&Value> = report_cells
        .iter()
        .filter(|c| {
            c["coherence"].as_f64().unwrap_or(0.0) >= f64::from(COHERENT)
                && c["share_of_frames"].as_f64().unwrap_or(0.0) >= 0.3
        })
        .collect();
    let along_values: Vec<f32> = along.iter().flatten().copied().collect();
    let mut reading = if steady.is_empty() {
        "nothing moves steadily beyond the world and the backdrop".to_owned()
    } else {
        format!(
            "{} area(s) move steadily against the world: the strongest at {} px a frame in box {} near {}",
            steady.len(),
            steady[0]["px_per_frame"],
            steady[0]["box_px"],
            steady[0]["near_mid_film"]
        )
    };
    let cam = median(&camera_step.iter().map(|t| len(*t)).collect::<Vec<_>>());
    reading.push_str(&format!(
        "; the backdrop moves with {:.0}% of the camera's step ({:.2} px a frame), set aside",
        share * 100.0,
        share * cam
    ));
    let along_report = path.as_ref().map(|p| {
        let m = median(&along_values);
        reading.push_str(&format!(
            "; along {} the flow is {:+.2} px a frame",
            p.label, m
        ));
        json!({ "path": p.label, "px_per_frame": round(m, 3), "frames_measured": along_values.len() })
    });

    // The picture: the last frame, dimmed, with each moving cell's mean flow as an arrow.
    let base = film.image(n - 1)?;
    let mut img = Image::new(w, h + 34, [0, 0, 0, 255]);
    for y in 0..h {
        for x in 0..w {
            let p = base.get(x, y);
            img.set(
                i64::from(x),
                i64::from(y),
                [p[0] / 3, p[1] / 3, p[2] / 3, 255],
            );
        }
    }
    let gain = 8.0;
    for (c, cell) in &ranked {
        let b = cell_box(*c);
        let mean = [
            cell.sum[0] / cell.count as f32,
            cell.sum[1] / cell.count as f32,
        ];
        let coherence = len(mean) / (cell.length / cell.count as f32).max(1e-6);
        let colour = if coherence >= COHERENT { FLAG } else { ACCENT };
        let (cx, cy) = ((b[0] + b[2] / 2) as f32, (b[1] + b[3] / 2) as f32);
        let (ex, ey) = (cx + mean[0] * gain, cy + mean[1] * gain);
        img.line(cx, cy, ex, ey, colour);
        img.dot(ex, ey, 2.0, colour);
        img.dot(cx, cy, 1.0, CALM);
    }
    let t = camera_step.last().copied().unwrap_or([0.0, 0.0]);
    let bgx = 40.0;
    img.line(
        bgx,
        40.0,
        bgx + t[0] * share * gain,
        40.0 + t[1] * share * gain,
        CALM,
    );
    img.label(8, 8, "backdrop drift, set aside", 1, CALM);
    for (k, line) in wrap(&format!("flow (arrows x{gain}): {reading}"), w as usize / 6)
        .iter()
        .take(3)
        .enumerate()
    {
        img.text(6, i64::from(h) + 4 + k as i64 * 10, line, 1, TEXT);
    }
    let out = opts.image_path(&film.dir, "flow")?;
    img.save_png(&out)?;
    Ok(json!({
        "tool": "flow",
        "frames": n,
        "compensated": !screen,
        "backdrop_share_of_camera_step": round(share, 3),
        "moving": report_cells,
        "along": along_report,
        "reading": reading,
        "image": out.display().to_string(),
    }))
}

#[derive(Clone, Default)]
struct Cell {
    sum: [f32; 2],
    length: f32,
    count: usize,
    frames: Vec<usize>,
}

/// The most common value, to a twentieth: the share of the camera's step that most
/// moving points follow (0 when too few move to say).
fn mode(values: &[f32]) -> f32 {
    let mut bins = std::collections::BTreeMap::new();
    for v in values
        .iter()
        .filter(|v| v.is_finite() && (-0.5..=1.5).contains(*v))
    {
        *bins.entry((v * 20.0).round() as i64).or_insert(0usize) += 1;
    }
    let Some((&bin, &count)) = bins.iter().max_by_key(|(_, c)| **c) else {
        return 0.0;
    };
    if count * 5 < values.len() {
        return 0.0;
    }
    // The mean of the values in and beside that bin.
    let near: Vec<f32> = values
        .iter()
        .copied()
        .filter(|v| ((v * 20.0).round() as i64 - bin).abs() <= 1)
        .collect();
    near.iter().sum::<f32>() / near.len().max(1) as f32
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

/// The image and halves of it, finest first. Unseen pixels (NaN) stay unseen.
fn pyramid(g: Gray) -> Vec<Gray> {
    let mut out = vec![g];
    for _ in 1..LEVELS {
        let next = out.last().expect("one").half();
        out.push(next);
    }
    out
}

/// How far the picture round `(x, y)` (screen pixels, centred on `i + 0.5`) moved from
/// `a` to `b`, in pixels; none where there is too little texture or the fit fails.
pub(crate) fn lucas_kanade(a: &[Gray], b: &[Gray], x: f32, y: f32) -> Option<[f32; 2]> {
    let mut guess = [0.0f32; 2];
    for level in (0..a.len()).rev() {
        let s = (1 << level) as f32;
        let (ia, ib) = (&a[level], &b[level]);
        let (px, py) = ((x - 0.5) / s, (y - 0.5) / s);
        if px < (HALF + 1) as f32
            || py < (HALF + 1) as f32
            || px > (ia.width as i32 - HALF - 2) as f32
            || py > (ia.height as i32 - HALF - 2) as f32
        {
            return None;
        }
        // The gradient round the point in `a`.
        let mut grads = Vec::with_capacity(((2 * HALF + 1) * (2 * HALF + 1)) as usize);
        let (mut gxx, mut gxy, mut gyy) = (0.0f32, 0.0f32, 0.0f32);
        for dy in -HALF..=HALF {
            for dx in -HALF..=HALF {
                let (qx, qy) = (px + dx as f32, py + dy as f32);
                let ix = (ia.sample(qx + 1.0, qy) - ia.sample(qx - 1.0, qy)) * 0.5;
                let iy = (ia.sample(qx, qy + 1.0) - ia.sample(qx, qy - 1.0)) * 0.5;
                let v = ia.sample(qx, qy);
                if !(ix.is_finite() && iy.is_finite() && v.is_finite()) {
                    return None;
                }
                gxx += ix * ix;
                gxy += ix * iy;
                gyy += iy * iy;
                grads.push((qx, qy, ix, iy, v));
            }
        }
        let det = gxx * gyy - gxy * gxy;
        let trace = gxx + gyy;
        let min_eig = (trace - ((gxx - gyy).powi(2) + 4.0 * gxy * gxy).sqrt()) / 2.0;
        if det.abs() < 1e-12 || min_eig / (grads.len() as f32) < 2e-4 {
            if level == 0 {
                return None;
            }
            guess = [guess[0] * 2.0, guess[1] * 2.0];
            continue;
        }
        let mut v = [0.0f32; 2];
        for _ in 0..8 {
            let (mut bx, mut by) = (0.0f32, 0.0f32);
            for &(qx, qy, ix, iy, va) in &grads {
                let vb = ib.sample(qx + guess[0] + v[0], qy + guess[1] + v[1]);
                if !vb.is_finite() {
                    return None;
                }
                let d = va - vb;
                bx += d * ix;
                by += d * iy;
            }
            let ex = (gyy * bx - gxy * by) / det;
            let ey = (gxx * by - gxy * bx) / det;
            v = [v[0] + ex, v[1] + ey];
            if ex * ex + ey * ey < 1e-4 {
                break;
            }
        }
        guess = [guess[0] + v[0], guess[1] + v[1]];
        if level > 0 {
            guess = [guess[0] * 2.0, guess[1] * 2.0];
        }
    }
    (len(guess) < 16.0 && guess[0].is_finite()).then_some(guess)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Soft blobs, shifted by `(dx, dy)`.
    fn blobs(w: u32, h: u32, dx: f32, dy: f32) -> Gray {
        let mut g = Gray::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let (fx, fy) = (x as f32 - dx, y as f32 - dy);
                g.data[(y * w + x) as usize] = 0.5
                    + 0.25 * (fx * 0.31).sin() * (fy * 0.23).cos()
                    + 0.1 * ((fx + fy) * 0.11).sin();
            }
        }
        g
    }

    #[test]
    fn a_small_shift_is_measured() {
        let a = pyramid(blobs(96, 96, 0.0, 0.0));
        let b = pyramid(blobs(96, 96, 0.8, -0.4));
        let v = lucas_kanade(&a, &b, 48.5, 48.5).unwrap();
        assert!(
            (v[0] - 0.8).abs() < 0.1 && (v[1] + 0.4).abs() < 0.1,
            "{v:?}"
        );
    }

    #[test]
    fn a_larger_shift_is_found_coarse_to_fine() {
        let a = pyramid(blobs(128, 128, 0.0, 0.0));
        let b = pyramid(blobs(128, 128, 4.0, 2.5));
        let v = lucas_kanade(&a, &b, 64.5, 64.5).unwrap();
        assert!(
            (v[0] - 4.0).abs() < 0.3 && (v[1] - 2.5).abs() < 0.3,
            "{v:?}"
        );
    }

    #[test]
    fn a_flat_patch_has_no_flow() {
        let mut g = Gray::new(64, 64);
        g.data.fill(0.5);
        let a = pyramid(g.clone());
        assert!(lucas_kanade(&a, &pyramid(g), 32.5, 32.5).is_none());
    }
}
