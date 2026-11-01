//! `sheet`: frames side by side, each labelled with its time and tick. A contact sheet
//! shows an approach, the order things happen in, and transitions at a glance.
//!
//! - Frames: `--count n` spread evenly (12 by default), plus any `--at t,t` (seconds, such
//!   as a moment another tool flagged), outlined in red.
//! - `--crop <thing>` cuts each frame round a thing at full size (`--crop-size` px) instead
//!   of shrinking whole frames, for detail.
//! - `--onion <thing>` draws one picture instead: a few poses (`--poses`, 5 by default) of
//!   the thing laid over each other in a window fixed in the world, coloured from blue
//!   (early) to red (late), with its path. Spacing between poses shows speeding up and
//!   slowing down; a pose past the end shows overshoot; the path's bends show bounces.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::image::{FLAG, Image, PAPER, TEXT, time_color};
use crate::opts::Opts;
use crate::strip::wrap;

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only(
        "sheet",
        &[
            "count",
            "at",
            "crop",
            "crop-size",
            "onion",
            "poses",
            "columns",
        ],
    )?;
    film.need_pictures("sheet")?;
    if let Some(thing) = opts.text("onion") {
        return onion(film, opts, &film.thing_id(thing)?);
    }
    let frames = film.frames();
    let count = opts.count("count")?.unwrap_or(12).clamp(1, 60);
    let mut chosen: Vec<(usize, bool)> = spread(frames.len(), count)
        .into_iter()
        .map(|i| (i, false))
        .collect();
    for t in opts.numbers("at")?.unwrap_or_default() {
        let i = film.frame_at(t);
        match chosen.iter_mut().find(|c| c.0 == i) {
            Some(c) => c.1 = true,
            None => chosen.push((i, true)),
        }
    }
    chosen.sort_by_key(|c| c.0);
    let crop = opts.text("crop").map(|t| film.thing_id(t)).transpose()?;
    let crop_size = opts.count("crop-size")?.unwrap_or(256).clamp(32, 1024) as u32;
    let (w, h) = film.size();
    let (cell_w, cell_h) = match crop {
        Some(_) => (crop_size, crop_size),
        None => {
            let cw = w.min(320);
            (cw, (h as f32 * cw as f32 / w as f32).round() as u32)
        }
    };
    let columns = opts.count("columns")?.unwrap_or(4).clamp(1, 12) as u32;
    let rows = (chosen.len() as u32).div_ceil(columns);
    let gap = 6;
    let head = 18;
    let mut sheet = Image::new(
        columns * (cell_w + gap) + gap,
        head + rows * (cell_h + gap) + gap,
        PAPER,
    );
    let title = match &crop {
        Some(t) => format!("sheet: {} frames round {t}", chosen.len()),
        None => format!("sheet: {} frames", chosen.len()),
    };
    sheet.text(6, 5, &title, 1, TEXT);
    let mut listed = Vec::new();
    for (n, &(i, asked)) in chosen.iter().enumerate() {
        let frame = &frames[i];
        let img = film.image(i)?;
        let cell = match &crop {
            Some(id) => {
                let at = frame
                    .things
                    .get(id)
                    .and_then(|t| t.screen)
                    .unwrap_or([w as f32 / 2.0, h as f32 / 2.0]);
                img.crop(
                    at[0] as i64 - i64::from(crop_size / 2),
                    at[1] as i64 - i64::from(crop_size / 2),
                    crop_size,
                    crop_size,
                    PAPER,
                )
            }
            None => img.resized(cell_w, cell_h),
        };
        let (cx, cy) = (n as u32 % columns, n as u32 / columns);
        let x = i64::from(gap + cx * (cell_w + gap));
        let y = i64::from(head + gap + cy * (cell_h + gap));
        sheet.blit(&cell, x, y);
        if asked {
            sheet.outline_rect(x - 2, y - 2, cell_w + 4, cell_h + 4, FLAG);
        }
        sheet.label(
            x + 2,
            y + 2,
            &format!("t{:.3} #{} k{}", frame.time, frame.index, frame.tick),
            1,
            TEXT,
        );
        let mut m = when(frame);
        if asked {
            m["asked"] = json!(true);
        }
        listed.push(m);
    }
    let out = opts.image_path(&film.dir, "sheet")?;
    sheet.save_png(&out)?;
    Ok(json!({
        "tool": "sheet",
        "frames": listed,
        "crop": crop,
        "image": out.display().to_string(),
        "reading": format!("{} frames from t={:.3} to t={:.3}; full frames are in {}", chosen.len(), frames[chosen[0].0].time, frames[chosen[chosen.len() - 1].0].time, film.dir.join("frames").display()),
    }))
}

/// `count` frame indices spread evenly over `n` frames, first and last included.
fn spread(n: usize, count: usize) -> Vec<usize> {
    if n <= count {
        return (0..n).collect();
    }
    let mut out: Vec<usize> = (0..count)
        .map(|k| (k as f32 * (n - 1) as f32 / (count - 1).max(1) as f32).round() as usize)
        .collect();
    out.dedup();
    out
}

fn onion(film: &Film, opts: &Opts, id: &str) -> Result<Value, String> {
    let frames = film.frames();
    let poses = opts.count("poses")?.unwrap_or(5).clamp(2, 12);
    let mut chosen = spread(frames.len(), poses);
    for t in opts.numbers("at")?.unwrap_or_default() {
        chosen.push(film.frame_at(t));
    }
    chosen.sort();
    chosen.dedup();
    let present: Vec<usize> = chosen
        .into_iter()
        .filter(|&i| frames[i].things.contains_key(id))
        .collect();
    if present.len() < 2 {
        return Err(format!("{id} is in fewer than two of the frames chosen"));
    }
    let path: Vec<(usize, [f32; 3])> = frames
        .iter()
        .enumerate()
        .filter_map(|(i, f)| f.things.get(id).map(|t| (i, t.point())))
        .collect();
    let plane = path[0].1[2];
    // A window fixed in the world round everywhere the thing goes.
    let (mut lo, mut hi) = ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]);
    for (_, p) in &path {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let reach = ((hi[0] - lo[0]).max(hi[1] - lo[1]) * 0.15).max(frames[0].camera.per_pixel * 120.0);
    let (lo, hi) = (
        [lo[0] - reach, lo[1] - reach],
        [hi[0] + reach, hi[1] + reach],
    );
    let mut per_px = frames[0].camera.per_pixel.max(1e-6);
    let longest = (hi[0] - lo[0]).max(hi[1] - lo[1]);
    if longest / per_px > 900.0 {
        per_px = longest / 900.0;
    }
    let ow = ((hi[0] - lo[0]) / per_px).ceil() as u32;
    let oh = ((hi[1] - lo[1]) / per_px).ceil() as u32;
    let (w, h) = film.size();
    let world_at = |u: u32, v: u32| {
        [
            lo[0] + (u as f32 + 0.5) * per_px,
            hi[1] - (v as f32 + 0.5) * per_px,
            plane,
        ]
    };
    // Each pose's frame, seen through the window.
    let mut warped: Vec<Vec<Option<[f32; 3]>>> = Vec::new();
    for &i in &present {
        let img = film.image(i)?;
        let cam = frames[i].camera;
        let mut px = Vec::with_capacity((ow * oh) as usize);
        for v in 0..oh {
            for u in 0..ow {
                px.push(
                    cam.project(world_at(u, v), w, h)
                        .and_then(|s| img.sample(s[0], s[1]))
                        .map(|c| [c[0], c[1], c[2]]),
                );
            }
        }
        warped.push(px);
    }
    // What stays: each pixel's median over the poses. What moves: far from it.
    let mut out = Image::new(ow, oh + 40, PAPER);
    let n = present.len();
    let mut background = vec![None; (ow * oh) as usize];
    for (k, bg) in background.iter_mut().enumerate() {
        let mut seen: Vec<[f32; 3]> = warped.iter().filter_map(|p| p[k]).collect();
        if seen.is_empty() {
            continue;
        }
        seen.sort_by(|a, b| (a[0] + a[1] + a[2]).total_cmp(&(b[0] + b[1] + b[2])));
        *bg = Some(seen[seen.len() / 2]);
    }
    for v in 0..oh {
        for u in 0..ow {
            let k = (v * ow + u) as usize;
            if let Some(c) = background[k] {
                out.set(
                    i64::from(u),
                    i64::from(v),
                    [
                        (c[0] * 0.5) as u8,
                        (c[1] * 0.5) as u8,
                        (c[2] * 0.5) as u8,
                        255,
                    ],
                );
            }
        }
    }
    // Only what moved near the thing counts: a backdrop drifting with the camera moves
    // too, everywhere.
    let near = (ow.max(oh) as f32 * 0.12).max(60.0);
    for (p, pose) in warped.iter().enumerate() {
        let tint = time_color(p as f32 / (n - 1) as f32);
        let at = frames[present[p]].things[id].point();
        let centre = [(at[0] - lo[0]) / per_px, (hi[1] - at[1]) / per_px];
        for v in 0..oh {
            for u in 0..ow {
                if (u as f32 - centre[0]).powi(2) + (v as f32 - centre[1]).powi(2) > near * near {
                    continue;
                }
                let k = (v * ow + u) as usize;
                let (Some(c), Some(bg)) = (pose[k], background[k]) else {
                    continue;
                };
                let diff = (c[0] - bg[0]).abs() + (c[1] - bg[1]).abs() + (c[2] - bg[2]).abs();
                if diff > 60.0 {
                    let mix = |a: f32, t: u8| (a * 0.65 + f32::from(t) * 0.35) as u8;
                    out.blend(
                        i64::from(u),
                        i64::from(v),
                        [
                            mix(c[0], tint[0]),
                            mix(c[1], tint[1]),
                            mix(c[2], tint[2]),
                            255,
                        ],
                        0.9,
                    );
                }
            }
        }
    }
    let to_px = |p: [f32; 3]| [(p[0] - lo[0]) / per_px, (hi[1] - p[1]) / per_px];
    let span = (path.len() - 1).max(1) as f32;
    for (k, pair) in path.windows(2).enumerate() {
        let (a, b) = (to_px(pair[0].1), to_px(pair[1].1));
        out.line(a[0], a[1], b[0], b[1], time_color(k as f32 / span));
    }
    let mut listed = Vec::new();
    let mut last: Option<[f32; 3]> = None;
    let mut gaps = Vec::new();
    for (p, &i) in present.iter().enumerate() {
        let at = frames[i].things[id].point();
        let s = to_px(at);
        let c = time_color(p as f32 / (n - 1) as f32);
        out.dot(s[0], s[1], 3.0, c);
        out.label(
            s[0] as i64 + 5,
            s[1] as i64 + 4,
            &format!("{:.2}", frames[i].time),
            1,
            c,
        );
        if let Some(l) = last {
            gaps.push(round(
                ((at[0] - l[0]).powi(2) + (at[1] - l[1]).powi(2)).sqrt(),
                2,
            ));
        }
        last = Some(at);
        listed.push(when(&frames[i]));
    }
    // A legend: early to late.
    for x in 0..ow.min(300) {
        let c = time_color(x as f32 / ow.min(300) as f32);
        for y in 0..6 {
            out.set(i64::from(x) + 8, i64::from(oh) + 26 + y, c);
        }
    }
    let title = format!("onion: {n} poses of {id}, early blue to late red");
    for (k, line) in wrap(&title, ow as usize / 6).iter().take(2).enumerate() {
        out.text(8, i64::from(oh) + 4 + k as i64 * 10, line, 1, TEXT);
    }
    let image = opts.image_path(&film.dir, "onion")?;
    out.save_png(&image)?;
    Ok(json!({
        "tool": "sheet",
        "onion": id,
        "poses": listed,
        "gaps_between_poses": gaps,
        "window": { "from": [round(lo[0], 2), round(lo[1], 2)], "to": [round(hi[0], 2), round(hi[1], 2)], "units_per_px": round(per_px, 4) },
        "reading": format!("{n} poses of {id}; world distance between poses in order: {gaps:?} (even spacing is steady speed, shrinking is slowing down)"),
        "image": image.display().to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::spread;

    #[test]
    fn frames_are_spread_from_first_to_last() {
        assert_eq!(spread(100, 5), [0, 25, 50, 74, 99]);
        assert_eq!(spread(3, 12), [0, 1, 2]);
    }
}
