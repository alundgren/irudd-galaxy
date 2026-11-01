//! `flicker`: sudden changes. Each frame is compared with the one before (and the one
//! before that), moved into its view so the camera's follow cancels out, in a grid of
//! cells. Movement changes a cell a little on many frames running and is left alone. What
//! is flagged:
//!
//! Pixels are compared as the mean brightness round them, after a one-pixel smoothing:
//! edges re-drawn a fraction of a pixel apart as the camera moves (shimmer) move light
//! between neighbours without changing the sum, and a frame resampled into another's
//! view is blurred no more than the frame it is compared with. A cell's change on a frame is the sum of what changed in it, and is
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
//!
//! The strongest events also get a close-up each (`--event n` or `--at t` any other):
//! the frame before (moved into the later frame's view), the frame it changed on, and
//! what changed between them, enlarged in whole pixel blocks, with the thing nearest the
//! changed pixels named. Every listed event says what its change looks like (edges
//! stepping, a thin line appearing or breaking, an area changing) from its difference
//! mask, and which model parts (glTF nodes, from the film's `parts.json`, drawn as their
//! triangles) lie under it.

use std::collections::BTreeMap;

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
pub const OPTIONS: &[&str] = &[
    "grid",
    "screen",
    "half",
    "threshold",
    "from",
    "to",
    "near",
    "event",
    "at",
];

/// Events listed with `--detail`; the compact report keeps the strongest few.
const LISTED: usize = 30;
/// The strongest events that get a close-up.
const CLOSEUPS: usize = 3;
/// A pixel the close-up marks as changed: its colour moved this much (0 to 1, luma).
const SHOWN_CHANGE: f32 = 0.06;
/// With --near, an event is on the thing's model when at least this share of its changed
/// pixels lie on it.
const ON_MODEL: f32 = 1.0 / 3.0;

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
        // Smoothed first: the frames before are resampled between pixels to move them into
        // this one's view, which blurs them by an amount that depends on the fraction of a
        // pixel the camera moved. Unsmoothed, a camera moving 4.5 px a frame makes every
        // sharp edge look blurred, sharp, blurred: flicker that isn't on screen.
        let g = gray(&film.image(i)?, scale).smoothed();
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
    let image_path = opts.image_path(&film.dir, "flicker")?;
    // Every listed event's close-up, made once: what the change looks like, which parts
    // lie under it and, with --near, whether it lies on that thing's model.
    let mut closeups: Vec<Option<Closeup>> = Vec::with_capacity(groups.len());
    for e in &groups {
        closeups.push(if e.from >= 1 {
            Some(closeup(film, e.from, e.bounds, &warp(e.from, e.from - 1))?)
        } else {
            None
        });
    }
    // With --near, events whose changed pixels lie on the thing's model come first, then
    // those only near it (a line passing by), each strongest first.
    let on_share = |c: &Option<Closeup>| {
        let id = near.as_ref()?;
        c.as_ref().map(|c| c.on.get(id).copied().unwrap_or(0.0))
    };
    let is_on = |c: &Option<Closeup>| on_share(c).is_some_and(|s| s >= ON_MODEL);
    let mut split = None;
    if near.is_some() && !film.manifest.parts.is_empty() {
        let mut both: Vec<(Group, Option<Closeup>)> = groups.into_iter().zip(closeups).collect();
        both.sort_by_key(|(_, c)| !is_on(c));
        (groups, closeups) = both.into_iter().unzip();
        let on = closeups.iter().filter(|c| is_on(c)).count();
        split = Some((on, groups.len() - on));
    }
    // `--event n`: the nth event listed and its close-up alone; `--at t`: the strongest
    // event starting on the frame nearest t (near --near), or else --near's box there.
    let event = opts.count("event")?;
    if let Some(n) = event
        && (n == 0 || n > groups.len())
    {
        return Err(format!(
            "--event counts from 1, and {} event(s) are listed here (each event's `n`)",
            groups.len()
        ));
    }
    let at = opts.number("at")?.map(|t| film.frame_at(t).max(1));
    let at_event = at.and_then(|f| groups.iter().position(|g| g.from.abs_diff(f) <= 1));
    let picking = event.is_some() || at.is_some();
    let mut listed: Vec<Value> = Vec::new();
    let mut chosen: Option<Value> = None;
    for (k, (e, c)) in groups.iter().zip(closeups).enumerate() {
        let mut v = json!({
            "n": k + 1,
            "kind": e.kind,
            "from": when(&frames[e.from]),
            "to": when(&frames[e.to]),
            "box_px": e.bounds,
            "cells": e.cells,
            "change": round(e.change, 1),
            "near": things_in(e.from, e.bounds, 8.0),
        });
        if let (Some(id), Some(s)) = (near.as_ref().filter(|_| split.is_some()), on_share(&c)) {
            v["on"] = json!(if s >= ON_MODEL {
                format!("on {id}'s model ({:.0}% of the changed pixels)", s * 100.0)
            } else {
                format!("only near {id}, not on its model ({:.0}%)", s * 100.0)
            });
        }
        if let Some(c) = c {
            v["looks_like"] = c.what["looks_like"]["says"].clone();
            if c.what["parts"].as_array().is_some_and(|p| !p.is_empty()) {
                v["parts"] = c.what["parts"].clone();
            }
            let wanted = event == Some(k + 1) || (event.is_none() && at_event == Some(k));
            if wanted || (!picking && k < CLOSEUPS) {
                let path = sibling(&image_path, &format!("event-{}", k + 1));
                c.image.save_png(&path)?;
                v["changed"] = c.what;
                v["closeup"] = json!(path.display().to_string());
            }
            if wanted {
                chosen = Some(v.clone());
            }
        }
        listed.push(v);
    }
    if chosen.is_none()
        && event.is_none()
        && let Some(f) = at
    {
        let Some(id) = &near else {
            return Err(format!(
                "no event starts at t={:.3}; add --near <thing> for a close-up of that thing \
                 there, or pick an event by its n with --event",
                frames[f].time
            ));
        };
        let t = frames[f]
            .things
            .get(id)
            .and_then(|t| t.screen.map(|s| (s, t.reach.unwrap_or(0.0))))
            .ok_or_else(|| format!("{id} is not on screen at t={:.3}", frames[f].time))?;
        let r = (t.1 / frames[f].camera.per_pixel.max(1e-9)).clamp(16.0, 200.0) + 8.0;
        let x0 = (t.0[0] - r).clamp(0.0, w as f32 - 1.0) as u32;
        let y0 = (t.0[1] - r).clamp(0.0, h as f32 - 1.0) as u32;
        let x1 = ((t.0[0] + r).max(0.0) as u32).clamp(x0 + 1, w);
        let y1 = ((t.0[1] + r).max(0.0) as u32).clamp(y0 + 1, h);
        let b = [x0, y0, x1 - x0, y1 - y0];
        let c = closeup(film, f, b, &warp(f, f - 1))?;
        let path = sibling(&image_path, "closeup");
        c.image.save_png(&path)?;
        chosen = Some(json!({
            "event": null,
            "at": when(&frames[f]),
            "box_px": b,
            "looks_like": c.what["looks_like"]["says"].clone(),
            "changed": c.what,
            "closeup": path.display().to_string(),
        }));
    }
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
    let mut reading = if total == 0 {
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
    match (&near, split) {
        (Some(id), Some((on, off))) if total > 0 => reading.push_str(&format!(
            "; on {id}'s model (a third or more of the changed pixels on it): {on}, listed \
             first; only near it: {off}"
        )),
        (Some(id), None) if total > 0 => reading.push_str(&format!(
            "; this film keeps no model parts, so events on {id} can't be told from those \
             only near it"
        )),
        _ => {}
    }
    // With --event or --at, the report is that event and its close-up alone.
    if let Some(c) = &chosen {
        let says = |k: &str| c[k].as_str().map(str::to_owned);
        let what = match c["n"].as_u64() {
            Some(n) => format!(
                "event {n}: a {} from t={} to t={} in box {} near {} (change {})",
                c["kind"].as_str().unwrap_or("change"),
                c["from"]["t"],
                c["to"]["t"],
                c["box_px"],
                c["near"],
                c["change"]
            ),
            None => format!(
                "the moment at t={}{}",
                c["at"]["t"],
                near.as_ref()
                    .map_or(String::new(), |id| format!(" round {id}"))
            ),
        };
        let mut line = vec![what];
        line.extend(says("on"));
        line.extend(says("looks_like"));
        if let Some(p) = c["changed"]["parts"].as_array().filter(|p| !p.is_empty()) {
            let p: Vec<&str> = p.iter().filter_map(Value::as_str).collect();
            line.push(format!("parts {}", p.join(", ")));
        }
        reading = format!(
            "{} (one of {} event(s) from {span}; without --event and --at, the list)",
            line.join("; "),
            listed.len()
        );
    }
    let events_listed = listed.len();
    if chosen.is_some() {
        listed.clear();
        left_out.clear();
    }
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
    let out = image_path;
    img.save_png(&out)?;
    let mut report = json!({
        "tool": "flicker",
        "frames": n,
        "compensated": !screen,
        "grid": [cols, rows],
        "pops": pops,
        "blips": blips,
        "flicker_runs": flickers,
        "events": listed,
        "events_listed": events_listed,
        "near_split": split.map(|(on, off)| json!({ "on_model": on, "only_near": off })),
        "chosen": chosen,
        "left_out": left_out,
        "warnings": warnings(film, scale),
        "changed_share": { "median": round(median(&changed_share), 4), "max": round(changed_share.iter().fold(0.0, |a: f32, b| a.max(*b)), 4) },
        "reading": reading,
        "image": out.display().to_string(),
    });
    if let Some(m) = report.as_object_mut() {
        for key in ["left_out", "chosen", "near_split"] {
            if m[key].is_null() {
                m.remove(key);
            }
        }
    }
    Ok(report)
}

/// `path` with `-<suffix>` before its extension: an event's close-up beside the picture.
fn sibling(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let stem = path
        .file_stem()
        .map_or("flicker".into(), |s| s.to_string_lossy().into_owned());
    path.with_file_name(format!("{stem}-{suffix}.png"))
}

/// An event's close-up: the picture and what it shows.
struct Closeup {
    image: Image,
    what: Value,
    /// Each thing's share of the changed pixels on its model (0 to 1).
    on: BTreeMap<String, f32>,
}

/// One event close up: the frame before moved into the view of the frame it changed on
/// (`back` maps that frame's pixels into the one before), that frame, and what changed
/// (warm: brighter now, blue: darker now) over the frame dimmed, enlarged in whole pixel
/// blocks (nearest neighbour). Also says how many pixels changed, where, the thing
/// nearest them, what kind of change it looks like and which parts of which models lie
/// under it.
fn closeup(film: &Film, at: usize, b: [u32; 4], back: &Warp) -> Result<Closeup, String> {
    let frames = film.frames();
    let (w, h) = film.size();
    let (after, before) = (film.image(at)?, film.image(at - 1)?);
    const PAD: u32 = 12;
    let x0 = b[0].saturating_sub(PAD);
    let y0 = b[1].saturating_sub(PAD);
    let x1 = (b[0] + b[2] + PAD).min(w);
    let y1 = (b[1] + b[3] + PAD).min(h);
    let (bw, bh) = (x1 - x0, y1 - y0);
    let mut was = Image::new(bw, bh, [40, 40, 40, 255]);
    let mut now = Image::new(bw, bh, [40, 40, 40, 255]);
    let mut diff = Image::new(bw, bh, [0, 0, 0, 255]);
    let (mut count, mut sx, mut sy, mut sum) = (0u32, 0.0f32, 0.0f32, 0.0f32);
    let luma = |p: [f32; 4]| (0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]) / 255.0;
    // The camera's move is exact on the ground plane only: a tall thing (a tower, a rim)
    // slides a little against it. A shift of up to a few pixels that best lines the box
    // up takes that out, so what is left is what changed.
    let mapped: Vec<Option<[f32; 2]>> = (0..bh)
        .flat_map(|j| (0..bw).map(move |i| (i, j)))
        .map(|(i, j)| back.map((x0 + i) as f32 + 0.5, (y0 + j) as f32 + 0.5))
        .collect();
    let at_shift = |x: u32, y: u32, s: [f32; 2]| {
        mapped[((y - y0) * bw + (x - x0)) as usize]
            .and_then(|[fx, fy]| before.sample(fx + s[0], fy + s[1]))
    };
    let mismatch = |s: [f32; 2]| {
        let mut e = 0.0f32;
        for j in (0..bh).step_by(2) {
            for i in (0..bw).step_by(2) {
                let (x, y) = (x0 + i, y0 + j);
                let a = after.get(x, y);
                let a = [f32::from(a[0]), f32::from(a[1]), f32::from(a[2]), 255.0];
                e += at_shift(x, y, s).map_or(1.0, |p| (luma(a) - luma(p)).abs());
            }
        }
        e
    };
    let mut shift = [0.0f32; 2];
    let mut best = mismatch(shift);
    for dy in -8..=8 {
        for dx in -8..=8 {
            let s = [dx as f32 * 0.5, dy as f32 * 0.5];
            let e = mismatch(s);
            if e < best * 0.999 {
                (best, shift) = (e, s);
            }
        }
    }
    let n = (bw * bh) as usize;
    let (mut was_l, mut now_l) = (vec![f32::NAN; n], vec![0.0f32; n]);
    let mut changed = vec![false; n];
    for j in 0..bh {
        for i in 0..bw {
            let (x, y) = (x0 + i, y0 + j);
            let k = (j * bw + i) as usize;
            let a = after.get(x, y);
            now.set(i64::from(i), i64::from(j), a);
            let dim = [a[0] / 4, a[1] / 4, a[2] / 4, 255];
            diff.set(i64::from(i), i64::from(j), dim);
            let af = [f32::from(a[0]), f32::from(a[1]), f32::from(a[2]), 255.0];
            now_l[k] = luma(af);
            let Some(p) = at_shift(x, y, shift) else {
                continue;
            };
            was_l[k] = luma(p);
            was.set(
                i64::from(i),
                i64::from(j),
                [p[0] as u8, p[1] as u8, p[2] as u8, 255],
            );
            let d = luma(af) - luma(p);
            let colour = (0..3).map(|c| (af[c] - p[c]).abs()).fold(0.0, f32::max) / 255.0;
            if d.abs().max(colour * 0.5) >= SHOWN_CHANGE {
                let c = if d >= 0.0 {
                    [255, 200, 90, 255]
                } else {
                    [90, 160, 255, 255]
                };
                diff.blend(i64::from(i), i64::from(j), c, (0.4 + colour * 2.0).min(1.0));
                changed[k] = true;
                count += 1;
                let weight = colour;
                sx += weight * x as f32;
                sy += weight * y as f32;
                sum += weight;
            }
        }
    }
    // Whole pixel blocks, so each panel is a few hundred pixels and every pixel stays sharp.
    let k = 320u32.div_ceil(bw.max(bh).max(1)).clamp(2, 12);
    let gap = 6;
    let head = 14;
    let mut out = Image::new(3 * bw * k + 2 * gap, bh * k + head, crate::image::PAPER);
    let f = |i: usize| &frames[i];
    let titles = [
        format!("before t{:.3}", f(at - 1).time),
        format!("after t{:.3} k{}", f(at).time, f(at).tick),
        "changed: warm brighter, blue darker".to_owned(),
    ];
    for (n, (panel, title)) in [&was, &now, &diff].into_iter().zip(&titles).enumerate() {
        let left = i64::from(n as u32 * (bw * k + gap));
        out.blit(&panel.enlarged(k), left, i64::from(head));
        out.text(left + 2, 3, title, 1, TEXT);
    }
    let centre = (sum > 0.0).then(|| [sx / sum, sy / sum]);
    // The thing whose outline on screen is nearest the changed pixels' centre.
    let nearest = centre.and_then(|c| {
        let fr = f(at);
        fr.things
            .iter()
            .filter_map(|(id, t)| {
                let s = t.screen?;
                let reach = t.reach.unwrap_or(0.0) / fr.camera.per_pixel.max(1e-9);
                let d = ((s[0] - c[0]).powi(2) + (s[1] - c[1]).powi(2)).sqrt();
                Some((id.clone(), d, (d - reach).max(0.0)))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2).then(a.1.total_cmp(&b.1)))
    });
    let (parts, on) = parts_under(film, at, x0, y0, bw, &changed);
    let what = json!({
        "pixels": count,
        "aligned_px": [shift[0], shift[1]],
        "centre_px": centre.map(|c| [c[0].round(), c[1].round()]),
        "nearest": nearest.map(|(id, d, outside)| if outside > 0.0 {
            format!("{id} ({outside:.0} px outside its reach)")
        } else {
            format!("{id} (within its reach, {d:.0} px from its centre)")
        }),
        "looks_like": looks_like(&was_l, &now_l, &changed, bw as usize, bh as usize),
        "parts": parts,
    });
    Ok(Closeup {
        image: out,
        what,
        on,
    })
}

/// What kind of change the close-up's difference mask is, from two simple measures:
/// `core_share`, the changed pixels whose eight neighbours all changed too (a filled
/// area has a core; a band one or two pixels wide has none), and `edge_share`, the
/// changed pixels within two pixels of an edge in both frames (an outline that stepped
/// is an edge before and after; a line that appeared is an edge only after).
fn looks_like(was: &[f32], now: &[f32], changed: &[bool], w: usize, h: usize) -> Value {
    let total = changed.iter().filter(|c| **c).count();
    if total == 0 {
        return json!({
            "kind": "none",
            "says": "nothing changed once the box is lined up: the camera's move alone (a tall thing slides against it)",
        });
    }
    let edges = |img: &[f32]| {
        let at = |x: usize, y: usize| img[y * w + x];
        let mut e = vec![false; w * h];
        for y in 1..h.saturating_sub(1) {
            for x in 1..w.saturating_sub(1) {
                let g = (at(x + 1, y) - at(x - 1, y))
                    .abs()
                    .max((at(x, y + 1) - at(x, y - 1)).abs());
                e[y * w + x] = g / 2.0 > 0.04;
            }
        }
        // Within two pixels of an edge.
        let mut near = vec![false; w * h];
        for y in 0..h {
            for x in 0..w {
                if !e[y * w + x] {
                    continue;
                }
                for ny in y.saturating_sub(2)..(y + 3).min(h) {
                    for nx in x.saturating_sub(2)..(x + 3).min(w) {
                        near[ny * w + nx] = true;
                    }
                }
            }
        }
        near
    };
    let (eb, ea) = (edges(was), edges(now));
    let (mut core, mut both, mut only_after, mut only_before) = (0, 0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let k = y * w + x;
            if !changed[k] {
                continue;
            }
            let full = (y >= 1 && y + 1 < h && x >= 1 && x + 1 < w)
                && (y - 1..=y + 1).all(|ny| (x - 1..=x + 1).all(|nx| changed[ny * w + nx]));
            core += usize::from(full);
            match (eb[k], ea[k]) {
                (true, true) => both += 1,
                (false, true) => only_after += 1,
                (true, false) => only_before += 1,
                _ => {}
            }
        }
    }
    let share = |n: usize| n as f32 / total as f32;
    let (core_share, edge_share) = (share(core), share(both));
    let (kind, says) = if core_share >= 0.3 {
        (
            "area",
            "an area appears, vanishes or changes tone (the changed pixels fill a patch)",
        )
    } else if edge_share >= 0.6 {
        (
            "edges",
            "edges step: outlines move by a pixel or two, nothing appears or vanishes",
        )
    } else if only_after >= only_before {
        (
            "line",
            "a thin line appears or joins (an edge now where there was none)",
        )
    } else {
        (
            "line",
            "a thin line vanishes or breaks (an edge before, none now)",
        )
    };
    json!({
        "kind": kind,
        "says": says,
        "core_share": round(core_share, 2),
        "edge_share": round(edge_share, 2),
    })
}

/// Which model part each pixel of the box `x0, y0, bw, bh` of frame `at` shows: every
/// thing's parts drawn as their triangles, the nearest to the camera winning. Indexes
/// into the names (`thing/part`), -1 where no model is drawn, and the distance from the
/// camera of what each pixel shows.
type PartMap = (Vec<i32>, Vec<f32>, Vec<String>);

fn part_map(film: &Film, at: usize, x0: u32, y0: u32, bw: u32, bh: u32) -> PartMap {
    let frame = &film.frames()[at];
    let (w, h) = film.size();
    let n = (bw * bh) as usize;
    let (mut ids, mut depth) = (vec![-1i32; n], vec![f32::INFINITY; n]);
    let mut names = Vec::new();
    let eye = frame.camera.eye;
    for (thing, worn) in &film.manifest.parts.things {
        let (Some(t), Some(parts)) = (
            frame.things.get(thing),
            film.manifest.parts.models.get(&worn.model),
        ) else {
            continue;
        };
        let (s, c) = t.angle.sin_cos();
        let place = |p: &[f32]| {
            let (x, y, z) = (p[0] * worn.scale, p[1] * worn.scale, p[2] * worn.scale);
            let world = [t.x + x * c - y * s, t.y + x * s + y * c, t.height + z];
            let d = (0..3)
                .map(|k| (world[k] - eye[k]).powi(2))
                .sum::<f32>()
                .sqrt();
            frame
                .camera
                .project(world, w, h)
                .map(|p| [p[0] - x0 as f32, p[1] - y0 as f32, d])
        };
        for part in parts {
            let id = names.len() as i32;
            names.push(format!("{thing}/{}", part.name));
            for tri in part.triangles.as_chunks::<9>().0 {
                let (Some(a), Some(b), Some(q)) =
                    (place(&tri[0..3]), place(&tri[3..6]), place(&tri[6..9]))
                else {
                    continue;
                };
                let edge = |u: [f32; 3], v: [f32; 3], p: [f32; 2]| {
                    (v[0] - u[0]) * (p[1] - u[1]) - (v[1] - u[1]) * (p[0] - u[0])
                };
                let area = edge(a, b, [q[0], q[1]]);
                if area.abs() < 1e-6 {
                    continue;
                }
                let lo = |k: usize| a[k].min(b[k]).min(q[k]).floor().max(0.0) as u32;
                let hi =
                    |k: usize, max: u32| (a[k].max(b[k]).max(q[k]).ceil().max(0.0) as u32).min(max);
                for py in lo(1)..hi(1, bh) {
                    for px in lo(0)..hi(0, bw) {
                        let p = [px as f32 + 0.5, py as f32 + 0.5];
                        let (wa, wb) = (edge(b, q, p) / area, edge(q, a, p) / area);
                        let wq = 1.0 - wa - wb;
                        if wa < 0.0 || wb < 0.0 || wq < 0.0 {
                            continue;
                        }
                        let z = wa * a[2] + wb * b[2] + wq * q[2];
                        let k = (py * bw + px) as usize;
                        if z < depth[k] {
                            (depth[k], ids[k]) = (z, id);
                        }
                    }
                }
            }
        }
    }
    (ids, depth, names)
}

/// Which parts of which things' models lie under the changed pixels of a close-up box:
/// each changed pixel counts for the part nearest the camera within two pixels of it, as
/// a part's ink outline lies just outside it, over what is behind. Strongest first, as
/// `thing/part share%`, with what lies on no model; and each thing's share of them.
fn parts_under(
    film: &Film,
    at: usize,
    x0: u32,
    y0: u32,
    bw: u32,
    changed: &[bool],
) -> (Vec<String>, BTreeMap<String, f32>) {
    if film.manifest.parts.is_empty() {
        return (Vec::new(), BTreeMap::new());
    }
    let bh = changed.len() as u32 / bw.max(1);
    let (ids, depth, names) = part_map(film, at, x0, y0, bw, bh);
    let mut tally = vec![0usize; names.len() + 1];
    let mut total = 0usize;
    for (k, _) in changed.iter().enumerate().filter(|(_, c)| **c) {
        total += 1;
        let (x, y) = ((k as u32 % bw) as i64, (k as u32 / bw) as i64);
        let (mut found, mut nearest) = (-1, f32::INFINITY);
        for dy in -2..=2i64 {
            for dx in -2..=2i64 {
                let (nx, ny) = (x + dx, y + dy);
                if nx >= 0 && ny >= 0 && nx < i64::from(bw) && ny < i64::from(bh) {
                    let n = (ny * i64::from(bw) + nx) as usize;
                    if ids[n] >= 0 && depth[n] < nearest {
                        (found, nearest) = (ids[n], depth[n]);
                    }
                }
            }
        }
        let slot = if found >= 0 {
            found as usize
        } else {
            names.len()
        };
        tally[slot] += 1;
    }
    let mut out: Vec<(String, usize)> = tally
        .iter()
        .enumerate()
        .filter(|(_, n)| **n > 0)
        .map(|(i, n)| {
            let name = names
                .get(i)
                .cloned()
                .unwrap_or_else(|| "on no model".to_owned());
            (name, *n)
        })
        .collect();
    let share = |n: usize| n as f32 / total.max(1) as f32;
    let mut things: BTreeMap<String, f32> = BTreeMap::new();
    for (name, n) in &out {
        if let Some((thing, _)) = name.split_once('/') {
            *things.entry(thing.to_owned()).or_default() += share(*n);
        }
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.1));
    let listed = out
        .iter()
        .take(4)
        .map(|(name, n)| format!("{name} {:.0}%", 100.0 * share(*n)))
        .collect();
    (listed, things)
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

/// Flagged cells that are one visual change: neighbours (eight-way) whose change starts
/// on the same frame or the next.
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
            // One change starts in all its cells at once (within a frame); runs that merely
            // overlap in time are separate things.
            let overlap = ea.from.abs_diff(eb.from) <= 1;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A 20 x 20 frame, black, with what `paint` lights.
    fn frame(paint: impl Fn(usize, usize) -> bool) -> Vec<f32> {
        (0..400)
            .map(|k| if paint(k % 20, k / 20) { 1.0 } else { 0.0 })
            .collect()
    }

    fn kind(was: &[f32], now: &[f32]) -> String {
        let changed: Vec<bool> = was
            .iter()
            .zip(now)
            .map(|(a, b)| (a - b).abs() > 0.1)
            .collect();
        looks_like(was, now, &changed, 20, 20)["kind"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn a_change_says_whether_edges_stepped_a_line_came_or_an_area_did() {
        // An edge moving one pixel to the right.
        let a = frame(|x, _| x < 10);
        let b = frame(|x, _| x < 11);
        assert_eq!(kind(&a, &b), "edges");
        // A one-pixel line appearing across a flat field.
        let line = frame(|_, y| y == 10);
        assert_eq!(kind(&frame(|_, _| false), &line), "line");
        // A filled square appearing.
        let square = frame(|x, y| (5..15).contains(&x) && (5..15).contains(&y));
        assert_eq!(kind(&frame(|_, _| false), &square), "area");
        assert_eq!(kind(&a, &a), "none");
    }
}
