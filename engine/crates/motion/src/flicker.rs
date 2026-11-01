//! `flicker`: sudden changes. Each frame is compared with the one before (and the one
//! before that), moved into its view so the camera's follow cancels out, in a grid of
//! cells. Movement changes a cell a little on many frames running and is left alone. What
//! is flagged:
//!
//! Pixels are compared as the mean brightness round them: edges re-drawn a fraction of a
//! pixel apart as the camera moves (shimmer) move light between neighbours without
//! changing the sum. A cell's change on a frame is the sum of what changed in it, and is
//! judged against the usual change of it and its neighbours on the frames round it, so a
//! busy place (a turning ship, drifting stars) needs a bigger jump to count than a quiet
//! one, and something moving from one cell into the next is not a pop.
//!
//! - **pop**: a cell changes far more on one frame than on the frames round it, and stays
//!   changed (something appeared, vanished or jumped in brightness without a fade);
//! - **blip**: it changes and is back the next frame (shown for one frame);
//! - **flicker**: it keeps going back and forth, frame after frame.
//!
//! The picture is a motion history: the last frame, dimmed, with every place that changed
//! coloured by how recently (blue long ago, yellow at the end), and the flagged cells
//! boxed with their times. Fixed overlays need `--screen`, which compares pixels where
//! they are instead of following the world.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::image::{FLAG, Gray, Image, TEXT, heat};
use crate::opts::Opts;
use crate::strip::wrap;
use crate::track::median;
use crate::warp::{Warp, compensate, gray};

/// A pixel changed when the brightness round it moved this much (0 to 1).
const CHANGED: f32 = 0.04;
/// ... and is back when within this of how it was two frames ago.
const BACK: f32 = 0.015;
/// The least change in a cell that counts (brightness times full-size pixels: twenty
/// pixels changing by a tenth).
const FLOOR: f32 = 2.0;
/// How many times its usual change a cell must change on one frame to pop.
const JUMP: f32 = 2.5;
/// Frames running a cell must go back and forth to flicker.
const RUN: usize = 4;

struct Step {
    /// How much changed per cell.
    change: Vec<f32>,
    /// Of that, how much is back as it was two frames ago.
    back: Vec<f32>,
    /// Which pixels changed, for the history.
    mask: Bits,
}

/// The options `flicker` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["grid", "screen", "half", "threshold", "from", "to", "near"];

/// Events listed with `--detail`; the compact report keeps the strongest few.
const LISTED: usize = 30;

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("flicker", OPTIONS)?;
    film.need_pictures("flicker")?;
    let frames = film.frames();
    let (w, h) = film.size();
    let scale: u32 = if opts.flag("half") { 2 } else { 1 };
    let screen = opts.flag("screen");
    let changed_at = opts
        .number("threshold")?
        .unwrap_or(CHANGED)
        .clamp(0.005, 1.0);
    let (cols, rows) = match opts.text("grid") {
        None => (16usize, 9usize),
        Some(g) => g
            .split_once('x')
            .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
            .filter(|&(a, b): &(usize, usize)| (1..=64).contains(&a) && (1..=64).contains(&b))
            .ok_or_else(|| format!("--grid takes columns x rows, such as 16x9, not `{g}`"))?,
    };
    let n = frames.len();
    let from_t = opts.number("from")?;
    let to_t = opts.number("to")?;
    // The frames events are reported for. The whole film is read, so a cell's usual
    // change is known at the edges and the events outside can be counted.
    let first = from_t.map_or(0, |t| film.frame_at(t).max(1));
    let last = to_t.map_or(n - 1, |t| film.frame_at(t));
    if last < first {
        return Err(format!(
            "--from and --to leave no frames: the film runs from t={:.3} to t={:.3}",
            frames[0].time,
            frames[n - 1].time
        ));
    }
    let near = opts.text("near").map(|t| film.thing_id(t)).transpose()?;
    let warp = |to: usize, from: usize| {
        if screen {
            Warp::identity(w, h)
        } else {
            Warp::new(&frames[to].camera, &frames[from].camera, w, h, 0.0)
        }
    };

    let mut grays: Vec<Gray> = Vec::new();
    let mut steps: Vec<Option<Step>> = vec![None];
    let (gw, gh) = ((w / scale).max(1), (h / scale).max(1));
    let cell_of = |x: u32, y: u32| {
        (y as usize * rows / gh as usize) * cols + (x as usize * cols / gw as usize)
    };
    for i in 0..n {
        let g = gray(&film.image(i)?, scale);
        if i >= 1 {
            let p1 = local(
                &compensate(&grays[grays.len() - 1], &warp(i, i - 1), scale as f32),
                gw,
                gh,
            );
            let p2 = (i >= 2).then(|| {
                local(
                    &compensate(&grays[grays.len() - 2], &warp(i, i - 2), scale as f32),
                    gw,
                    gh,
                )
            });
            let g = Gray {
                width: gw,
                height: gh,
                data: local(&g.data, gw, gh),
            };
            let mut step = Step {
                change: vec![0.0; cols * rows],
                back: vec![0.0; cols * rows],
                mask: Bits::new(g.data.len()),
            };
            let area = (scale * scale) as f32;
            for y in 0..g.height {
                for x in 0..g.width {
                    let k = (y * g.width + x) as usize;
                    let d1 = (g.data[k] - p1[k]).abs();
                    // NaN (unseen in the earlier frame) is not a change.
                    if d1.is_nan() || d1 < changed_at {
                        continue;
                    }
                    let c = cell_of(x, y);
                    step.change[c] += d1 * area;
                    step.mask.set(k);
                    if let Some(p2) = &p2
                        && (g.data[k] - p2[k]).abs() < BACK
                    {
                        step.back[c] += d1 * area;
                    }
                }
            }
            steps.push(Some(step));
        }
        grays.push(g);
        if grays.len() > 2 {
            grays.remove(0);
        }
    }

    // Per cell, over time. A cell's usual change is the busiest of it and its neighbours
    // on the frames round it, so a thing moving into a quiet cell (its neighbour was
    // changing a moment before) is not a pop.
    let busiest: Vec<Vec<f32>> = steps
        .iter()
        .map(|s| {
            (0..cols * rows)
                .map(|c| {
                    let Some(s) = s else { return 0.0 };
                    let (cx, cy) = ((c % cols) as i64, (c / cols) as i64);
                    let mut most = 0.0f32;
                    for dy in -1..=1 {
                        for dx in -1..=1 {
                            let (x, y) = (cx + dx, cy + dy);
                            if x >= 0 && y >= 0 && x < cols as i64 && y < rows as i64 {
                                most = most.max(s.change[(y as usize) * cols + x as usize]);
                            }
                        }
                    }
                    most
                })
                .collect()
        })
        .collect();
    let mut events: Vec<Event> = Vec::new();
    #[allow(clippy::needless_range_loop)] // `c` names a cell across several tables
    for c in 0..cols * rows {
        let change: Vec<f32> = steps
            .iter()
            .map(|s| s.as_ref().map_or(0.0, |s| s.change[c]))
            .collect();
        let back: Vec<f32> = steps
            .iter()
            .map(|s| s.as_ref().map_or(0.0, |s| s.back[c]))
            .collect();
        let returns = |i: usize| change[i] >= FLOOR && back[i] >= 0.6 * change[i];
        let mut i = 1;
        while i < n {
            // A run of frames going back and forth.
            let mut j = i;
            while j < n && returns(j) {
                j += 1;
            }
            if j - i >= RUN {
                events.push(Event {
                    kind: "flicker",
                    cell: c,
                    from: i,
                    to: j - 1,
                    change: change[i..j].iter().copied().fold(0.0, f32::max),
                });
                i = j;
                continue;
            }
            let around: Vec<f32> = (i.saturating_sub(4)..(i + 5).min(n))
                .filter(|&k| k != i && k >= 1)
                .map(|k| busiest[k][c])
                .collect();
            let usual = median(&around);
            if change[i] > JUMP * usual + FLOOR {
                let blip = i + 1 < n && returns(i + 1);
                events.push(Event {
                    kind: if blip { "blip" } else { "pop" },
                    cell: c,
                    from: i,
                    to: if blip { i + 1 } else { i },
                    change: change[i],
                });
                i += if blip { 2 } else { 1 };
                continue;
            }
            i += 1;
        }
    }
    // One visual change shows in several cells at once (a line breaking across a cell
    // edge) and over a few frames: neighbouring cells flagged on overlapping frames are one
    // event, as strong as their changes together.
    let cell_box = |c: usize| {
        let (cx, cy) = (c % cols, c / cols);
        let (x0, y0) = (cx as u32 * w / cols as u32, cy as u32 * h / rows as u32);
        let (x1, y1) = (
            (cx as u32 + 1) * w / cols as u32,
            (cy as u32 + 1) * h / rows as u32,
        );
        [x0, y0, x1 - x0, y1 - y0]
    };
    let found = events.len();
    let mut groups = group(&events, cols, &cell_box);
    let mut left_out = Vec::new();
    let before = groups.len();
    groups.retain(|g| (first..=last).contains(&g.from));
    if groups.len() < before {
        left_out.push(format!("{} outside --from/--to", before - groups.len()));
    }
    let things_in = |frame: usize, b: [u32; 4], pad: f32| -> Vec<String> {
        frames[frame]
            .things
            .iter()
            .filter(|(_, t)| {
                let Some(s) = t.screen else { return false };
                let reach = t.reach.unwrap_or(0.0) / frames[frame].camera.per_pixel.max(1e-9);
                let (dx, dy) = (
                    (b[0] as f32 - s[0])
                        .max(s[0] - (b[0] + b[2]) as f32)
                        .max(0.0),
                    (b[1] as f32 - s[1])
                        .max(s[1] - (b[1] + b[3]) as f32)
                        .max(0.0),
                );
                (dx * dx + dy * dy).sqrt() <= reach.min(400.0) + pad
            })
            .map(|(id, _)| id.clone())
            .collect()
    };
    if let Some(id) = &near {
        let before = groups.len();
        groups.retain(|g| things_in(g.from, g.bounds, 24.0).contains(id));
        if groups.len() < before {
            left_out.push(format!("{} not near {id}", before - groups.len()));
        }
    }
    groups.sort_by(|a, b| b.change.total_cmp(&a.change));
    let count = |k: &str| groups.iter().filter(|e| e.kind == k).count();
    let (pops, blips, flickers) = (count("pop"), count("blip"), count("flicker"));
    let total = groups.len();
    if total > LISTED {
        left_out.push(format!(
            "{} weaker past the {LISTED} strongest",
            total - LISTED
        ));
        groups.truncate(LISTED);
    }
    let listed: Vec<Value> = groups
        .iter()
        .map(|e| {
            json!({
                "kind": e.kind,
                "from": when(&frames[e.from]),
                "to": when(&frames[e.to]),
                "box_px": e.bounds,
                "cells": e.cells,
                "change": round(e.change, 1),
                "near": things_in(e.from, e.bounds, 8.0),
            })
        })
        .collect();
    let changed_share: Vec<f32> = steps
        .iter()
        .flatten()
        .map(|s| s.mask.count() as f32 / s.mask.len.max(1) as f32)
        .collect();
    let span = format!(
        "t={:.3} to t={:.3}{}",
        frames[first].time,
        frames[last].time,
        near.as_ref()
            .map_or(String::new(), |id| format!(", near {id}"))
    );
    let reading = if total == 0 {
        format!(
            "no pops, blips or flicker from {span}: changes build and fade over several frames (median {:.2}% of the picture changes a frame)",
            median(&changed_share) * 100.0
        )
    } else {
        let mut parts = Vec::new();
        if pops > 0 {
            parts.push(format!("{pops} pop(s)"));
        }
        if blips > 0 {
            parts.push(format!("{blips} one-frame blip(s)"));
        }
        if flickers > 0 {
            parts.push(format!("{flickers} flicker run(s)"));
        }
        let times: Vec<f32> = groups.iter().map(|g| frames[g.from].time).collect();
        let strongest = &groups[0];
        format!(
            "{} from {span}, at times from t={:.3} to t={:.3}; the strongest a {} at t={:.3} in box {:?} near {:?} (change {:.1})",
            parts.join(", "),
            times.iter().copied().fold(f32::INFINITY, f32::min),
            times.iter().copied().fold(0.0, f32::max),
            strongest.kind,
            frames[strongest.from].time,
            strongest.bounds,
            things_in(strongest.from, strongest.bounds, 8.0),
            strongest.change
        )
    };
    let left_out = if left_out.is_empty() {
        Value::Null
    } else {
        json!(format!(
            "{} of {} event(s) listed ({} flagged cells grouped): {}",
            listed.len(),
            listed.len() + left_out_count(&left_out),
            found,
            left_out.join(", ")
        ))
    };

    // The motion history over the last frame.
    let last = n - 1;
    let base = film.image(last)?;
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
    for (i, step) in steps.iter().enumerate().skip(1) {
        let Some(step) = step else { continue };
        let back = warp(last, i);
        let recency = i as f32 / last.max(1) as f32;
        let colour = heat(0.25 + 0.75 * recency);
        for y in (0..h).step_by(scale as usize) {
            for x in (0..w).step_by(scale as usize) {
                let Some([fx, fy]) = back.map(x as f32 + 0.5, y as f32 + 0.5) else {
                    continue;
                };
                let (gx, gy) = ((fx / scale as f32) as i64, (fy / scale as f32) as i64);
                if gx < 0 || gy < 0 || gx >= i64::from(gw) || gy >= i64::from(gh) {
                    continue;
                }
                if step.mask.get((gy as u32 * gw + gx as u32) as usize) {
                    img.fill_rect(i64::from(x), i64::from(y), scale, scale, colour);
                }
            }
        }
    }
    for e in &groups {
        let b = e.bounds;
        let to_last = warp(e.from, last);
        let centre = [(b[0] + b[2] / 2) as f32, (b[1] + b[3] / 2) as f32];
        let Some(c) = to_last
            .map(centre[0], centre[1])
            .filter(|c| c[0] >= 0.0 && c[1] >= 0.0 && c[0] < w as f32 && c[1] < h as f32)
        else {
            continue;
        };
        let (x, y) = (
            c[0] as i64 - i64::from(b[2] / 2),
            c[1] as i64 - i64::from(b[3] / 2),
        );
        img.outline_rect(x, y, b[2], b[3], FLAG);
        img.label(
            x + 2,
            y + 2,
            &format!("{} {:.2}", e.kind, frames[e.from].time),
            1,
            FLAG,
        );
    }
    for (k, line) in wrap(&format!("flicker: {reading}"), w as usize / 6)
        .iter()
        .take(3)
        .enumerate()
    {
        img.text(6, i64::from(h) + 4 + k as i64 * 10, line, 1, TEXT);
    }
    let out = opts.image_path(&film.dir, "flicker")?;
    img.save_png(&out)?;
    Ok(json!({
        "tool": "flicker",
        "frames": n,
        "compensated": !screen,
        "grid": [cols, rows],
        "pops": pops,
        "blips": blips,
        "flicker_runs": flickers,
        "events": listed,
        "left_out": left_out,
        "warnings": warnings(film, scale),
        "changed_share": { "median": round(median(&changed_share), 4), "max": round(changed_share.iter().fold(0.0, |a: f32, b| a.max(*b)), 4) },
        "reading": reading,
        "image": out.display().to_string(),
    }))
}

/// Settings that can hide what flicker looks for.
pub(crate) fn warnings(film: &Film, scale: u32) -> Vec<String> {
    let mut out = Vec::new();
    let fps = film.manifest.fps;
    if fps < 60 {
        out.push(format!(
            "filmed at {fps} fps: the window draws at 60 or more, so a frame shown between two of these is never seen; film at 60"
        ));
    }
    let (w, h) = film.size();
    if w < 1280 || h < 720 {
        out.push(format!(
            "filmed at {w}x{h}: thin lines (an ink line is about 2 px) can vanish or merge at this size; film at the player's size, 1280x720 or more"
        ));
    }
    if scale > 1 {
        out.push("--half: thin lines are lost at half size".to_owned());
    }
    out
}

/// Each pixel as the mean of the nine round it (NaN where any is unseen). An edge drawn a
/// fraction of a pixel further along moves light between neighbours but keeps their sum,
/// so this sees through edge shimmer; something appearing or vanishing changes the sum.
fn local(p: &[f32], w: u32, h: u32) -> Vec<f32> {
    let (w, h) = (w as i64, h as i64);
    let mut out = vec![f32::NAN; p.len()];
    for y in 0..h {
        for x in 0..w {
            let mut s = 0.0;
            let mut n = 0.0;
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = ((x + dx).clamp(0, w - 1), (y + dy).clamp(0, h - 1));
                    s += p[(ny * w + nx) as usize];
                    n += 1.0;
                }
            }
            out[(y * w + x) as usize] = s / n;
        }
    }
    out
}

/// One bit a pixel: a film's worth of change masks stays small.
struct Bits {
    words: Vec<u64>,
    len: usize,
}

impl Bits {
    fn new(len: usize) -> Bits {
        Bits {
            words: vec![0; len.div_ceil(64)],
            len,
        }
    }

    fn set(&mut self, i: usize) {
        self.words[i / 64] |= 1 << (i % 64);
    }

    fn get(&self, i: usize) -> bool {
        self.words[i / 64] & (1 << (i % 64)) != 0
    }

    fn count(&self) -> usize {
        self.words.iter().map(|w| w.count_ones() as usize).sum()
    }
}

struct Event {
    kind: &'static str,
    cell: usize,
    from: usize,
    to: usize,
    /// Brightness times pixels.
    change: f32,
}

/// Flagged cells that are one visual change: neighbours (eight-way) flagged on frames
/// that overlap or touch.
struct Group {
    kind: &'static str,
    from: usize,
    to: usize,
    /// The box round its cells, pixels: x, y, w, h.
    bounds: [u32; 4],
    cells: usize,
    /// Its cells' changes together.
    change: f32,
}

fn group(events: &[Event], cols: usize, cell_box: &dyn Fn(usize) -> [u32; 4]) -> Vec<Group> {
    let n = events.len();
    let mut parent: Vec<usize> = (0..n).collect();
    fn root(p: &mut [usize], mut i: usize) -> usize {
        while p[i] != i {
            p[i] = p[p[i]];
            i = p[i];
        }
        i
    }
    for a in 0..n {
        for b in a + 1..n {
            let (ea, eb) = (&events[a], &events[b]);
            let (ax, ay) = ((ea.cell % cols) as i64, (ea.cell / cols) as i64);
            let (bx, by) = ((eb.cell % cols) as i64, (eb.cell / cols) as i64);
            let touching = (ax - bx).abs() <= 1 && (ay - by).abs() <= 1;
            let overlap = ea.from <= eb.to + 1 && eb.from <= ea.to + 1;
            if touching && overlap {
                let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
                parent[ra] = rb;
            }
        }
    }
    let mut by_root: std::collections::BTreeMap<usize, Vec<usize>> = Default::default();
    for i in 0..n {
        let r = root(&mut parent, i);
        by_root.entry(r).or_default().push(i);
    }
    by_root
        .values()
        .map(|members| {
            let strongest = members
                .iter()
                .map(|&i| &events[i])
                .max_by(|a, b| a.change.total_cmp(&b.change))
                .expect("a group has members");
            let boxes: Vec<[u32; 4]> = members.iter().map(|&i| cell_box(events[i].cell)).collect();
            let x0 = boxes.iter().map(|b| b[0]).min().unwrap_or(0);
            let y0 = boxes.iter().map(|b| b[1]).min().unwrap_or(0);
            let x1 = boxes.iter().map(|b| b[0] + b[2]).max().unwrap_or(0);
            let y1 = boxes.iter().map(|b| b[1] + b[3]).max().unwrap_or(0);
            Group {
                kind: strongest.kind,
                from: members.iter().map(|&i| events[i].from).min().unwrap_or(0),
                to: members.iter().map(|&i| events[i].to).max().unwrap_or(0),
                bounds: [x0, y0, x1 - x0, y1 - y0],
                cells: members.len(),
                change: members.iter().map(|&i| events[i].change).sum(),
            }
        })
        .collect()
}

/// How many events the `left_out` reasons count.
fn left_out_count(reasons: &[String]) -> usize {
    reasons
        .iter()
        .filter_map(|r| r.split_whitespace().next()?.parse::<usize>().ok())
        .sum()
}
