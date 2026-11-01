//! `dots`: whether the dots of dotted lines (a course, a lane) stay put in the world, from
//! the probes alone. It needs no pixels, so it works on a probes-only film made without a
//! GPU, and it is what the engine's own headless test of the course uses.
//!
//! Each fully visible dot is matched with the nearest dot of the frame before. A pattern
//! that is still in the world matches at zero distance; one that slides moves every dot by
//! the same step along its line. A dot with no match within half a spacing popped in or
//! out instead of fading.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use std::collections::BTreeMap;

use crate::image::{ACCENT, CALM, FLAG, TEXT};
use crate::opts::Opts;
use crate::plot::{Panel, Series, chart};
use crate::track::median;

/// A dot this visible counts; fading ones are expected to come and go.
const SOLID: f32 = 0.95;
/// Below this many pixels a frame, a dot is still.
const STILL: f32 = 0.05;

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("dots", &[])?;
    let frames = film.frames();
    // Each line on its own: the leg being flown may slide while the legs after it stand
    // still, and a median over every dot would hide it. A line is known by where it is
    // drawn from.
    let mut lines: BTreeMap<String, Vec<Option<f32>>> = BTreeMap::new();
    let mut popped = Vec::new();
    let mut matched_total = 0usize;
    for i in 1..frames.len() {
        let (before, now) = (&frames[i - 1], &frames[i]);
        let old: Vec<([f32; 3], f32)> = before
            .lines
            .iter()
            .filter(|l| l.kind == "dotted")
            .flat_map(|l| {
                l.dots
                    .iter()
                    .map(move |d| ([d[0], d[1], d[2]], l.spacing.unwrap_or(1.0)))
            })
            .collect();
        let mut pops = 0;
        for line in now
            .lines
            .iter()
            .filter(|l| l.kind == "dotted" && l.points.len() >= 2)
        {
            let spacing = line.spacing.unwrap_or(1.0);
            let (a, b) = (line.points[0], *line.points.last().expect("2"));
            let len = ((b[0] - a[0]).powi(2) + (b[1] - a[1]).powi(2))
                .sqrt()
                .max(1e-6);
            let dir = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
            let mut steps = Vec::new();
            for d in line.dots.iter().filter(|d| d[3] >= SOLID) {
                let best = old
                    .iter()
                    .filter(|(_, s)| (s - spacing).abs() < spacing * 0.25)
                    .map(|(p, _)| (p, (p[0] - d[0]).powi(2) + (p[1] - d[1]).powi(2)))
                    .min_by(|x, y| x.1.total_cmp(&y.1));
                match best {
                    Some((p, d2)) if d2.sqrt() < spacing * 0.5 => {
                        let along = (d[0] - p[0]) * dir[0] + (d[1] - p[1]) * dir[1];
                        steps.push(along / now.camera.per_pixel.max(1e-9));
                    }
                    _ if !old.is_empty() => pops += 1,
                    _ => {}
                }
            }
            matched_total += steps.len();
            let key = format!("the dotted line from ({:.1}, {:.1})", a[0], a[1]);
            let series = lines.entry(key).or_insert_with(|| vec![None; frames.len()]);
            series[i] = (!steps.is_empty()).then(|| median(&steps));
        }
        if pops > 0 {
            popped.push((i, pops));
        }
    }
    // The line that moves most decides.
    let summary: Vec<(String, f32, usize, usize)> = lines
        .iter()
        .map(|(k, v)| {
            let values: Vec<f32> = v.iter().flatten().copied().collect();
            let moving = values.iter().filter(|m| m.abs() > STILL).count();
            (k.clone(), median(&values), moving, values.len())
        })
        .collect();
    let worst = summary
        .iter()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()).then(a.2.cmp(&b.2)))
        .cloned();
    let fps = film.manifest.fps as f32;
    let (drift, moving_frames, steps) = worst.as_ref().map_or((0.0, 0, 0), |w| (w.1, w.2, w.3));
    let mut reading = match &worst {
        None => "the film shows no dotted line in two frames running".to_owned(),
        Some((_, d, m, n)) if d.abs() <= STILL && m * 5 < *n => format!(
            "the dots of all {} dotted line(s) stay put in the world: the most any moves is a median {d:+.3} px a frame",
            summary.len()
        ),
        Some((line, d, m, n)) => format!(
            "{line} slides {} at {:.2} px a frame ({:.0} px/s, pixels at the camera's scale), in {m} of {n} frame steps",
            if *d > 0.0 {
                "toward where it is drawn to (for a course, the thing flying it)"
            } else {
                "toward where it is drawn from (for a course, the leg's far end)"
            },
            d.abs(),
            d.abs() * fps,
        ),
    };
    if !popped.is_empty() {
        reading.push_str(&format!(
            "; dots popped in or out without fading in {} frames",
            popped.len()
        ));
    }
    let times: Vec<f32> = frames.iter().map(|f| f.time).collect();
    let flags: Vec<f32> = popped.iter().map(|(i, _)| frames[*i].time).collect();
    let colours = [ACCENT, CALM, TEXT, FLAG];
    let img = chart(
        "dots: how far each dotted line's dots move along it each frame",
        &times,
        &[Panel {
            title: "median step along the line (px a frame)".into(),
            series: lines
                .iter()
                .take(4)
                .enumerate()
                .map(|(k, (name, v))| Series {
                    name: name.replace("the dotted line ", ""),
                    values: v.clone(),
                    color: colours[k],
                })
                .collect(),
            zero: true,
        }],
        &flags,
        720,
    );
    let out = opts.image_path(&film.dir, "dots")?;
    img.save_png(&out)?;
    Ok(json!({
        "tool": "dots",
        "frames": frames.len(),
        "dots_matched": matched_total,
        "lines": summary.iter().map(|(k, d, m, n)| json!({ "line": k, "px_per_frame": round(*d, 3), "moving_steps": m, "steps": n })).collect::<Vec<_>>(),
        "drift_px_per_frame": round(drift, 3),
        "drift_px_per_second": round(drift * fps, 1),
        "moving_steps": moving_frames,
        "steps": steps,
        "popped": popped.iter().take(12).map(|(i, n)| {
            let mut m = when(&frames[*i]);
            m["dots"] = json!(n);
            m
        }).collect::<Vec<_>>(),
        "reading": reading,
        "image": out.display().to_string(),
    }))
}
