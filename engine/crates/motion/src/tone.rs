//! `tone`: how one thing's light and shadow change from frame to frame. A turning ship's
//! faces should ease from one tone band to the next; a hard cel step flips a whole face
//! between light and shadow in one frame, which reads as flicker on the ship. `flicker`
//! can't see it (a turning thing is a busy place it sets aside), so this looks at the
//! thing alone.
//!
//! Each frame takes the pixels inside the thing's faces: in a disc round it (its reach
//! on screen, from the probes), not the colour of the sky at the disc's rim, and flat
//! (edges and ink lines move as it turns and would read as tone changing). Their
//! brightness histogram ignores where they are, so the thing turning barely changes it,
//! while a face changing tone moves pixels to another brightness.
//!
//! - **change**: how far the face tones moved since the frame before, as the mean
//!   brightness shift of their pixels (the earth mover's distance between the two
//!   histograms, 0 to 1);
//! - **prominence**: a frame's change over the mean change of the frames round it (eight
//!   either side, leaving out the next ones, which a step over two frames shares). A face
//!   easing over eight frames spreads its change and stands out little; a step in one
//!   frame puts it all there.
//! - **bands**: the share of the faces in the darkest, middle and lightest thirds of what
//!   they show over the film, for the picture. A flat face crosses a band edge in one
//!   frame even while it fades, so bands are not used to find steps.
//!
//! A **hard step** is a frame whose change is at least `STEP_FLOOR` and `PROMINENT` times
//! the usual change round it.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::image::{ACCENT, CALM, TEXT};
use crate::opts::Opts;
use crate::plot::{Panel, Series, chart_marked};
use crate::track::median;

/// The options `tone` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["thing", "radius"];

/// Brightness levels the histogram keeps.
const LEVELS: usize = 64;
/// The least change (mean brightness shift of the faces) a hard step needs.
const STEP_FLOOR: f32 = 0.01;
/// How many times the usual change round it a hard step is.
const PROMINENT: f32 = 5.0;
/// The usual change is taken as at least this, so a still thing's tiny noise can't make
/// a small change look prominent.
const NOISE: f32 = 0.002;
/// Frames either side a step is judged against.
const AROUND: usize = 8;
/// Hard steps listed at most.
const LISTED: usize = 12;

/// A pixel is inside a face when its neighbours are within this of its brightness.
const FLAT: f32 = 0.03;

/// A pixel within this of the background's colour (0 to 255 a channel) is background.
const BACKGROUND: f32 = 20.0;

/// The background round a thing: the median colour of a ring just outside its disc.
fn background(img: &crate::image::Image, c: [f32; 2], radius: f32) -> [f32; 3] {
    let mut ch: [Vec<u8>; 3] = Default::default();
    let (outer, inner) = (radius + 6.0, radius + 1.0);
    let (w, h) = (img.width as f32, img.height as f32);
    let steps = (outer * 8.0) as usize;
    for k in 0..steps {
        let a = k as f32 / steps as f32 * std::f32::consts::TAU;
        for r in [inner, (inner + outer) / 2.0, outer] {
            // Plain maths is fine here: this is a measurement, not the sim.
            let (x, y) = (c[0] + r * a.cos(), c[1] + r * a.sin());
            if x >= 0.0 && y >= 0.0 && x < w && y < h {
                let p = img.get(x as u32, y as u32);
                for j in 0..3 {
                    ch[j].push(p[j]);
                }
            }
        }
    }
    let mut out = [0f32; 3];
    for j in 0..3 {
        ch[j].sort_unstable();
        out[j] = ch[j].get(ch[j].len() / 2).copied().map_or(0.0, f32::from);
    }
    out
}

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("tone", OPTIONS)?;
    film.need_pictures("tone")?;
    let frames = film.frames();
    let id = match opts.text("thing") {
        Some(t) => film.thing_id(t)?,
        None => frames[0]
            .things
            .iter()
            .find(|(_, t)| t.followed)
            .map(|(id, _)| id.clone())
            .ok_or("no thing is followed by the camera; name one with --thing")?,
    };
    let radius = match opts.number("radius")? {
        Some(r) => r.max(2.0),
        None => {
            let t = &frames[0].things[&id];
            let per_pixel = frames[0].camera.per_pixel.max(1e-6);
            t.reach.map_or(48.0, |r| r / per_pixel + 3.0)
        }
    };
    let (w, h) = film.size();

    // Each frame's histogram of brightness in the disc.
    let mut hists: Vec<Option<Vec<f32>>> = Vec::with_capacity(frames.len());
    let mut pooled = vec![0f64; LEVELS];
    let mut means: Vec<Option<f32>> = Vec::with_capacity(frames.len());
    for (i, f) in frames.iter().enumerate() {
        let Some(c) = f.things.get(&id).and_then(|t| t.screen) else {
            hists.push(None);
            means.push(None);
            continue;
        };
        let img = film.image(i)?;
        let mut hist = vec![0f32; LEVELS];
        let (mut n, mut sum) = (0f32, 0f32);
        let r2 = radius * radius;
        let bg = background(&img, c, radius);
        let (x0, x1) = (
            (c[0] - radius).floor().max(0.0) as u32,
            (c[0] + radius).ceil().min(w as f32 - 1.0).max(0.0) as u32,
        );
        let (y0, y1) = (
            (c[1] - radius).floor().max(0.0) as u32,
            (c[1] + radius).ceil().min(h as f32 - 1.0).max(0.0) as u32,
        );
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (dx, dy) = (x as f32 + 0.5 - c[0], y as f32 + 0.5 - c[1]);
                if dx * dx + dy * dy > r2 {
                    continue;
                }
                // The sky between the thing's parts is not the thing.
                let p = img.get(x, y);
                let off = (0..3)
                    .map(|k| (f32::from(p[k]) - bg[k]).abs())
                    .fold(0.0, f32::max);
                if off < BACKGROUND {
                    continue;
                }
                let l = img.luma(x, y);
                // Only the flat insides of faces: edges and ink lines move as the thing
                // turns and would read as tone changing.
                if x == 0 || y == 0 || x + 1 >= w || y + 1 >= h {
                    continue;
                }
                let flat = [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)]
                    .iter()
                    .all(|&(a, b)| (img.luma(a, b) - l).abs() < FLAT);
                if !flat {
                    continue;
                }
                hist[((l * LEVELS as f32) as usize).min(LEVELS - 1)] += 1.0;
                n += 1.0;
                sum += l;
            }
        }
        if n < 16.0 {
            hists.push(None);
            means.push(None);
            continue;
        }
        for (p, v) in pooled.iter_mut().zip(&hist) {
            *p += f64::from(*v / n);
        }
        hists.push(Some(hist.iter().map(|v| v / n).collect()));
        means.push(Some(sum / n));
    }
    if hists.iter().flatten().count() < 3 {
        return Err(format!(
            "{id} is on screen in fewer than three frames; film it in view, or name another \
             --thing"
        ));
    }

    // Band edges: the thirds of what the disc shows over the film.
    let total: f64 = pooled.iter().sum();
    let mut edges = Vec::new();
    let mut acc = 0.0;
    for (level, p) in pooled.iter().enumerate() {
        acc += p;
        while edges.len() < 2 && acc >= total * (edges.len() + 1) as f64 / 3.0 {
            edges.push(level + 1);
        }
    }
    let band_of = |level: usize| edges.iter().filter(|&&e| level >= e).count();
    let shares: Vec<Option<[f32; 3]>> = hists
        .iter()
        .map(|h| {
            h.as_ref().map(|h| {
                let mut s = [0f32; 3];
                for (level, v) in h.iter().enumerate() {
                    s[band_of(level)] += v;
                }
                s
            })
        })
        .collect();

    // Frame to frame: how far the face tones moved, and how far that stands out from the
    // frames round it.
    let n = frames.len();
    let mut change: Vec<Option<f32>> = vec![None; n];
    for i in 1..n {
        if let (Some(a), Some(b)) = (&hists[i - 1], &hists[i]) {
            let (mut ca, mut cb, mut emd) = (0f32, 0f32, 0f32);
            for k in 0..LEVELS {
                ca += a[k];
                cb += b[k];
                emd += (ca - cb).abs();
            }
            change[i] = Some(emd / LEVELS as f32);
        }
    }
    let prominence: Vec<Option<f32>> = (0..n)
        .map(|i| {
            let c = change[i]?;
            let around: Vec<f32> = (i.saturating_sub(AROUND)..(i + AROUND + 1).min(n))
                .filter(|&j| j.abs_diff(i) > 1)
                .filter_map(|j| change[j])
                .collect();
            (around.len() >= 4)
                .then(|| c / (around.iter().sum::<f32>() / around.len() as f32).max(NOISE))
        })
        .collect();
    let is_step = |i: usize| {
        change[i].is_some_and(|c| c >= STEP_FLOOR) && prominence[i].is_some_and(|p| p >= PROMINENT)
    };
    // A step can land on two frames in a row (a face crossing between them); each run of
    // step frames is one step, at its largest change.
    let mut steps: Vec<usize> = Vec::new();
    for i in (0..n).filter(|&i| is_step(i)) {
        match steps.last_mut() {
            Some(last) if i - *last <= 2 => {
                if change[i] > change[*last] {
                    *last = i;
                }
            }
            _ => steps.push(i),
        }
    }
    let hard = steps.len();
    steps.sort_by(|a, b| {
        change[*b]
            .unwrap_or(0.0)
            .total_cmp(&change[*a].unwrap_or(0.0))
    });
    steps.truncate(LISTED);
    steps.sort_unstable();

    let present: Vec<f32> = change.iter().flatten().copied().collect();
    let (max_i, max_change) = change
        .iter()
        .enumerate()
        .filter_map(|(i, c)| c.map(|c| (i, c)))
        .fold((0, 0f32), |a, b| if b.1 > a.1 { b } else { a });
    let (prom_i, max_prominence) = prominence
        .iter()
        .enumerate()
        .filter(|(i, _)| change[*i].is_some_and(|c| c >= STEP_FLOOR))
        .filter_map(|(i, c)| c.map(|c| (i, c)))
        .fold((max_i, 0f32), |a, b| if b.1 > a.1 { b } else { a });
    let mean_values: Vec<f32> = means.iter().flatten().copied().collect();
    let mean_steps: Vec<f32> = means
        .windows(2)
        .filter_map(|p| Some((p[1]? - p[0]?).abs()))
        .collect();
    let band_range = |k: usize| {
        let v: Vec<f32> = shares.iter().flatten().map(|s| s[k]).collect();
        json!({
            "min": round(v.iter().copied().fold(f32::INFINITY, f32::min), 3),
            "max": round(v.iter().copied().fold(0.0, f32::max), 3),
        })
    };
    let level = |e: usize| round(e as f32 / LEVELS as f32, 3);

    let reading = if hard == 0 {
        format!(
            "{id}'s faces change tone smoothly: no frame stands out (largest change {max_change:.4} a frame, {:.1} times the frames round it; a hard step is {PROMINENT} times or more)",
            prominence[max_i].unwrap_or(0.0)
        )
    } else {
        let s = steps
            .iter()
            .max_by(|a, b| {
                change[**a]
                    .unwrap_or(0.0)
                    .total_cmp(&change[**b].unwrap_or(0.0))
            })
            .copied()
            .unwrap_or(max_i);
        format!(
            "{hard} hard tone step(s) on {id}: faces flip between light and shadow in one frame; the largest at t={:.3}, a change of {:.4}, {:.1} times the frames round it",
            frames[s].time,
            change[s].unwrap_or(0.0),
            prominence[s].unwrap_or(0.0)
        )
    };

    let times: Vec<f32> = frames.iter().map(|f| f.time).collect();
    let band = |k: usize| shares.iter().map(|s| s.map(|s| s[k])).collect::<Vec<_>>();
    let panels = vec![
        Panel {
            title: "share of the thing in each tone band".into(),
            series: vec![
                Series {
                    name: "dark".into(),
                    values: band(0),
                    color: CALM,
                },
                Series {
                    name: "mid".into(),
                    values: band(1),
                    color: TEXT,
                },
                Series {
                    name: "light".into(),
                    values: band(2),
                    color: ACCENT,
                },
            ],
            zero: true,
            min_span: 0.02,
        },
        Panel {
            title: "tone change a frame (brightness shift)".into(),
            series: vec![Series {
                name: "change".into(),
                values: change.clone(),
                color: ACCENT,
            }],
            zero: true,
            min_span: 0.02,
        },
    ];
    let flags: Vec<f32> = steps.iter().map(|&i| frames[i].time).collect();
    let marks: Vec<(f32, String)> = film
        .manifest
        .changes
        .iter()
        .take(12)
        .map(|c| (c.t, c.what.clone()))
        .collect();
    let img = chart_marked(&format!("tone {id}"), &times, &panels, &flags, &marks, 720);
    let path = opts.image_path(&film.dir, "tone")?;
    img.save_png(&path)?;

    Ok(json!({
        "tool": "tone",
        "thing": id,
        "radius_px": round(radius, 1),
        "frames": n,
        "band_edges": [level(edges[0]), level(*edges.get(1).unwrap_or(&edges[0]))],
        "bands": { "dark": band_range(0), "mid": band_range(1), "light": band_range(2) },
        "brightness": {
            "mean": round(mean_values.iter().sum::<f32>() / mean_values.len().max(1) as f32, 4),
            "max_change_per_frame": round(mean_steps.iter().copied().fold(0.0, f32::max), 4),
        },
        "change": {
            "median": round(median(&present), 5),
            "max": round(max_change, 5),
            "max_at": when(&frames[max_i]),
            "prominence_at_max": round(prominence[max_i].unwrap_or(0.0), 2),
        },
        "prominence": {
            "max": round(max_prominence, 2),
            "max_at": when(&frames[prom_i]),
        },
        "hard_steps": hard,
        "steps": steps.iter().map(|&i| {
            let mut m = when(&frames[i]);
            m["change"] = json!(round(change[i].unwrap_or(0.0), 5));
            m["prominence"] = json!(round(prominence[i].unwrap_or(0.0), 2));
            m
        }).collect::<Vec<_>>(),
        "reading": reading,
        "image": path.display().to_string(),
    }))
}
