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

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("flicker", &["grid", "screen", "half", "threshold"])?;
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
    for i in 0..frames.len() {
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
    let n = frames.len();
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
    let total = events.len();
    let count = |k: &str| events.iter().filter(|e| e.kind == k).count();
    let (pops, blips, flickers) = (count("pop"), count("blip"), count("flicker"));
    events.sort_by(|a, b| b.change.total_cmp(&a.change));
    events.truncate(16);
    events.sort_by_key(|e| (e.from, e.cell));
    let cell_box = |c: usize| {
        let (cx, cy) = (c % cols, c / cols);
        let (x0, y0) = (cx as u32 * w / cols as u32, cy as u32 * h / rows as u32);
        let (x1, y1) = (
            (cx as u32 + 1) * w / cols as u32,
            (cy as u32 + 1) * h / rows as u32,
        );
        [x0, y0, x1 - x0, y1 - y0]
    };
    let things_in = |frame: usize, b: [u32; 4]| -> Vec<String> {
        frames[frame]
            .things
            .iter()
            .filter(|(_, t)| {
                t.screen.is_some_and(|s| {
                    s[0] >= b[0] as f32 - 8.0
                        && s[0] <= (b[0] + b[2]) as f32 + 8.0
                        && s[1] >= b[1] as f32 - 8.0
                        && s[1] <= (b[1] + b[3]) as f32 + 8.0
                })
            })
            .map(|(id, _)| id.clone())
            .collect()
    };
    let listed: Vec<Value> = events
        .iter()
        .map(|e| {
            let b = cell_box(e.cell);
            json!({
                "kind": e.kind,
                "from": when(&frames[e.from]),
                "to": when(&frames[e.to]),
                "box_px": b,
                "change": round(e.change, 1),
                "near": things_in(e.from, b),
            })
        })
        .collect();
    let changed_share: Vec<f32> = steps
        .iter()
        .flatten()
        .map(|s| s.mask.count() as f32 / s.mask.len.max(1) as f32)
        .collect();
    let reading = if total == 0 {
        format!(
            "no pops, blips or flicker: changes build and fade over several frames (median {:.2}% of the picture changes a frame)",
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
        let first = events
            .iter()
            .max_by(|a, b| a.change.total_cmp(&b.change))
            .map(|e| {
                let near = things_in(e.from, cell_box(e.cell));
                format!(
                    "; the strongest a {} at t={:.3} in box {:?} near {near:?} (change {:.1})",
                    e.kind,
                    frames[e.from].time,
                    cell_box(e.cell),
                    e.change
                )
            })
            .unwrap_or_default();
        format!("{}{first}", parts.join(", "))
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
    for e in &events {
        let b = cell_box(e.cell);
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
        "changed_share": { "median": round(median(&changed_share), 4), "max": round(changed_share.iter().fold(0.0, |a: f32, b| a.max(*b)), 4) },
        "reading": reading,
        "image": out.display().to_string(),
    }))
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
