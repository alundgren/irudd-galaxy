//! `track`: how one thing moves, from the film's probes (no pixels needed).
//!
//! - **On screen**: how far it sits from the centre (for the followed thing, the camera's
//!   tracking error), its speed in pixels, and how unevenly it moves from frame to frame.
//! - **In the world**: distance, speed, and the same unevenness.
//! - **The camera**: how unevenly the view itself moves, in pixels.
//! - **A gap** (`--to <thing>`): the distance to another thing, how close it came, whether
//!   it went past (overshoot) or bounced back, and when it settled.
//!
//! Unevenness ("judder") compares each frame's step with the mean of the steps either
//! side of it. Smooth motion, even accelerating, scores near 0. A thing that moves only on
//! ticks while frames come twice as often steps 0, 2, 0, 2: its steps differ from their
//! neighbours' mean by as much as the mean itself, a judder of about 1. A single bump (a
//! collision) moves the median little but shows as a flagged moment.

use serde_json::{Value, json};

use crate::film::{Film, Frame, round, when};
use crate::image::{ACCENT, CALM, FLAG, TEXT};
use crate::opts::Opts;
use crate::plot::{Panel, Series, chart_marked};

/// Inputs and orders listed and marked at most.
const MAX_CHANGES: usize = 12;

/// The film's inputs, orders and arrivals, capped.
fn changes_json(film: &Film) -> Value {
    let all = &film.manifest.changes;
    let mut list: Vec<Value> = all
        .iter()
        .take(MAX_CHANGES)
        .map(|c| json!({ "t": round(c.t, 3), "tick": c.tick, "kind": c.kind, "what": c.what }))
        .collect();
    if all.len() > MAX_CHANGES {
        list.push(json!(format!("{} more", all.len() - MAX_CHANGES)));
    }
    Value::Array(list)
}

/// Unevenness below this reads as smooth; above `JUDDERS`, as judder.
const SMOOTH: f32 = 0.1;
const JUDDERS: f32 = 0.4;

/// The options `track` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["thing", "to", "tolerance"];

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("track", OPTIONS)?;
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
    let to = opts.text("to").map(|t| film.thing_id(t)).transpose()?;
    let (w, h) = film.size();
    let dt = film.dt();
    let times: Vec<f32> = frames.iter().map(|f| f.time).collect();
    let at = |f: &Frame| f.things.get(&id).cloned();
    let followed = at(&frames[0]).is_some_and(|t| t.followed);

    let world: Vec<Option<[f32; 2]>> = frames.iter().map(|f| at(f).map(|t| [t.x, t.y])).collect();
    let screen: Vec<Option<[f32; 2]>> = frames
        .iter()
        .map(|f| at(f).and_then(|t| t.screen))
        .collect();
    // The camera's path in pixels: each step is where the frame before's target lands in
    // this frame (it sat at the centre). Measured at the centre, a zoom doesn't move it,
    // unlike a far point that a zoom sweeps across the screen.
    let mut camera: Vec<Option<[f32; 2]>> = vec![Some([0.0, 0.0])];
    for k in 1..frames.len() {
        let prev = camera[k - 1];
        let before = frames[k - 1]
            .camera
            .project(frames[k - 1].camera.target, w, h);
        let now = frames[k].camera.project(frames[k - 1].camera.target, w, h);
        camera.push(match (prev, before, now) {
            (Some(p), Some(b), Some(n)) => Some([p[0] + b[0] - n[0], p[1] + b[1] - n[1]]),
            _ => None,
        });
    }
    let zooms = zoom_bursts(frames);
    // Frames a zoom moves the view on, and the frames either side: steps there are the
    // player's wheel, not a fault.
    let zooming: Vec<bool> = (0..frames.len())
        .map(|i| zooms.iter().any(|z| i + 1 >= z.from && i <= z.to + 1))
        .collect();
    let per_pixel = frames[0].camera.per_pixel.max(1e-9);

    let world_px: Vec<Option<[f32; 2]>> = world
        .iter()
        .map(|p| p.map(|p| [p[0] / per_pixel, p[1] / per_pixel]))
        .collect();
    let world_u = unevenness(&world_px);
    let screen_u = unevenness(&screen);
    let camera_u = unevenness(&camera);

    let centre = [w as f32 / 2.0, h as f32 / 2.0];
    let offsets: Vec<Option<f32>> = screen.iter().map(|s| s.map(|s| dist(s, centre))).collect();
    let speeds: Vec<Option<f32>> = steps(&world)
        .iter()
        .map(|s| s.map(|s| len(s) / dt))
        .collect();
    let screen_speed: Vec<f32> = steps(&screen).iter().flatten().map(|s| len(*s)).collect();
    let distance: f32 = steps(&world).iter().flatten().map(|s| len(*s)).sum();

    // Flagged moments: a step far from its neighbours', on screen, in the world or of the
    // camera.
    let mut flags = Vec::new();
    let mut set_aside = 0;
    for (name, u) in [
        ("screen", &screen_u),
        ("world", &world_u),
        ("camera", &camera_u),
    ] {
        let floor =
            (median(&u.residuals.iter().flatten().copied().collect::<Vec<_>>()) * 4.0).max(0.5);
        for (i, r) in u.residuals.iter().enumerate() {
            if let Some(r) = r
                && *r > floor
            {
                if zooming[i] && name != "world" {
                    set_aside += 1;
                } else {
                    flags.push((i, name, *r));
                }
            }
        }
    }
    flags.sort_by(|a, b| b.2.total_cmp(&a.2));
    let flag_count = flags.len();
    flags.truncate(12);
    flags.sort_by_key(|f| f.0);

    let mut report = json!({
        "tool": "track",
        "thing": id,
        "followed": followed,
        "frames": frames.len(),
        "span": [when(&frames[0]), when(frames.last().expect("frames"))],
        "world": {
            "distance": round(distance, 2),
            "speed_max": round(speeds.iter().flatten().fold(0.0, |a: f32, b| a.max(*b)), 2),
            "speed_end": speeds.last().copied().flatten().map(|v| round(v, 2)),
            "judder": world_u.score.map(|v| round(v, 3)),
        },
        "screen": {
            "offset_px": stats(&offsets, frames),
            "speed_px_per_frame": { "median": round(median(&screen_speed), 2), "max": round(screen_speed.iter().fold(0.0, |a: f32, b| a.max(*b)), 2) },
            "wobble_px": { "median": round(median(&screen_u.residuals.iter().flatten().copied().collect::<Vec<_>>()), 3), "max": round(screen_u.residuals.iter().flatten().fold(0.0, |a: f32, b| a.max(*b)), 3) },
            "judder": screen_u.score.map(|v| round(v, 3)),
        },
        "camera": {
            "speed_px_per_frame": round(median(&steps(&camera).iter().flatten().map(|s| len(*s)).collect::<Vec<_>>()), 2),
            "judder": camera_u.score.map(|v| round(v, 3)),
        },
        "changes": changes_json(film),
        "zooms": zooms.iter().map(|z| z.describe(frames)).collect::<Vec<_>>(),
        "flagged": flag_count,
        "flags": flags.iter().map(|(i, name, r)| {
            let mut m = when(&frames[*i]);
            m["kind"] = json!(format!("{name} step off by {r:.2} px"));
            m
        }).collect::<Vec<_>>(),
    });

    let mut panels = vec![];
    let mut reading = Vec::new();
    if followed {
        panels.push(Panel {
            title: "offset from the screen centre (px)".into(),
            series: vec![Series {
                name: "offset".into(),
                values: offsets.clone(),
                color: ACCENT,
            }],
            zero: true,
            min_span: 1.0,
        });
    } else {
        panels.push(Panel {
            title: "on screen (px)".into(),
            series: vec![
                Series {
                    name: "x".into(),
                    values: screen.iter().map(|s| s.map(|s| s[0])).collect(),
                    color: ACCENT,
                },
                Series {
                    name: "y".into(),
                    values: screen.iter().map(|s| s.map(|s| s[1])).collect(),
                    color: CALM,
                },
            ],
            zero: false,
            min_span: 1.0,
        });
    }
    panels.push(Panel {
        title: "speed in the world (units/s)".into(),
        series: vec![Series {
            name: "speed".into(),
            values: speeds.clone(),
            color: CALM,
        }],
        zero: true,
        min_span: 1.0,
    });
    panels.push(Panel {
        title: "step unevenness (px): each step against its neighbours'".into(),
        series: vec![
            Series {
                name: "screen".into(),
                values: screen_u.residuals.clone(),
                color: ACCENT,
            },
            Series {
                name: "camera".into(),
                values: camera_u.residuals.clone(),
                color: TEXT,
            },
        ],
        zero: true,
        min_span: 1.0,
    });

    if !zooms.is_empty() {
        report["zoom_steps_set_aside"] = json!(set_aside);
    }
    reading.push(describe(&id, "on screen", screen_u.score, &screen_u));
    reading.push(describe("the camera", "", camera_u.score, &camera_u));
    if followed {
        let max = offsets.iter().flatten().fold(0.0, |a: f32, b| a.max(*b));
        reading.push(format!(
            "the camera keeps it within {max:.1} px of the centre"
        ));
    }
    if !zooms.is_empty() {
        reading.push(format!(
            "the player zoomed {} time(s) ({}); steps a zoom makes are not flagged",
            zooms.len(),
            zooms
                .iter()
                .take(3)
                .map(|z| z.describe(frames))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    if let Some(other) = &to {
        let tol = opts.number("tolerance")?;
        let gap = gap(film, &id, other, tol);
        panels.push(Panel {
            title: format!("gap to {other} (world units)"),
            series: vec![Series {
                name: "gap".into(),
                values: gap.values.clone(),
                color: FLAG,
            }],
            zero: true,
            min_span: 1.0,
        });
        reading.push(gap.reading.clone());
        if let Some(a) = approach(film, &id, other, &gap.values, &speeds) {
            reading.push(a.reading.clone());
            report["approach"] = a.report;
        }
        report["gap"] = gap.report;
    }
    let flag_times: Vec<f32> = flags.iter().map(|(i, _, _)| frames[*i].time).collect();
    let mut marks: Vec<(f32, String)> = film
        .manifest
        .changes
        .iter()
        .take(MAX_CHANGES)
        .map(|c| (c.t, c.what.clone()))
        .collect();
    marks.extend(
        zooms
            .iter()
            .take(MAX_CHANGES)
            .map(|z| (frames[z.from].time, z.word().to_owned())),
    );
    let img = chart_marked(
        &format!("track {id}"),
        &times,
        &panels,
        &flag_times,
        &marks,
        720,
    );
    let path = opts.image_path(&film.dir, "track")?;
    img.save_png(&path)?;
    report["reading"] = json!(reading.join("; "));
    report["image"] = json!(path.display().to_string());
    Ok(report)
}

/// A run of frames where the player's wheel changed the view.
struct ZoomBurst {
    from: usize,
    to: usize,
    /// World units a pixel covers before and after.
    before: f32,
    after: f32,
}

impl ZoomBurst {
    fn word(&self) -> &'static str {
        if self.after < self.before {
            "zoom in"
        } else {
            "zoom out"
        }
    }

    fn describe(&self, frames: &[Frame]) -> String {
        format!(
            "{} from t={:.3} to t={:.3}, {:.2}x",
            self.word(),
            frames[self.from].time,
            frames[self.to].time,
            self.before / self.after.max(1e-9)
        )
    }
}

/// The zooms in a film: frames whose scale changed from the frame before, a run of them
/// (gaps of up to a quarter second, as the wheel turns in clicks) as one.
fn zoom_bursts(frames: &[Frame]) -> Vec<ZoomBurst> {
    let mut out: Vec<ZoomBurst> = Vec::new();
    for k in 1..frames.len() {
        let (a, b) = (frames[k - 1].camera.per_pixel, frames[k].camera.per_pixel);
        if a <= 0.0 || ((b / a) - 1.0).abs() < 0.002 {
            continue;
        }
        match out.last_mut() {
            Some(z) if frames[k].time - frames[z.to].time <= 0.25 => {
                z.to = k;
                z.after = b;
            }
            _ => out.push(ZoomBurst {
                from: k,
                to: k,
                before: a,
                after: b,
            }),
        }
    }
    out
}

struct Approach {
    report: Value,
    reading: String,
}

/// How `id` came in to `other`: its top speed, when it began to brake, its speed as the
/// gap closed, when it stopped, whether it then turned on the spot, and when it arrived.
fn approach(
    film: &Film,
    id: &str,
    other: &str,
    gap: &[Option<f32>],
    speeds: &[Option<f32>],
) -> Option<Approach> {
    let frames = film.frames();
    let present: Vec<(usize, f32)> = gap
        .iter()
        .enumerate()
        .filter_map(|(i, g)| g.map(|g| (i, g)))
        .collect();
    let &(_, start) = present.first()?;
    let (closest_i, _) = present
        .iter()
        .copied()
        .fold((0, f32::INFINITY), |a, b| if b.1 < a.1 { b } else { a });
    let speed = |i: usize| speeds.get(i).copied().flatten().unwrap_or(0.0);
    let (peak_i, peak) = (0..=closest_i)
        .map(|i| (i, speed(i)))
        .fold((0, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
    if peak <= 0.0 {
        return None;
    }
    let braking = (peak_i..=closest_i)
        .rev()
        .find(|&i| speed(i) >= 0.98 * peak)
        .unwrap_or(peak_i);
    let still = (0.02 * peak).max(0.05);
    let stopped = (braking..frames.len()).find(|&i| speed(i) < still && i > 0);
    let at = |i: usize| format!("t={:.3}", frames[i].time);
    let mut points = Vec::new();
    for share in [0.75f32, 0.5, 0.25, 0.1] {
        if let Some(&(i, g)) = present.iter().find(|(_, g)| *g <= share * start) {
            points.push(format!(
                "gap {g:.2} ({:.0}%) at {}: {:.2} units/s",
                share * 100.0,
                at(i),
                speed(i)
            ));
        }
    }
    let angle = |i: usize| frames[i].things.get(id).map(|t| t.angle);
    let arrived = film
        .manifest
        .changes
        .iter()
        .find(|c| c.kind == "arrived" && c.what.starts_with(&format!("{id}:")));
    let turned = stopped.and_then(|s| {
        let end = arrived.map_or(frames.len() - 1, |c| {
            frames
                .iter()
                .position(|f| f.tick >= c.tick)
                .unwrap_or(frames.len() - 1)
        });
        let (a, b) = (angle(s)?, angle(end)?);
        let mut d = (b - a) % std::f32::consts::TAU;
        if d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        } else if d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        (d.abs() > 0.05).then_some((d.abs(), end))
    });
    let mut reading = format!(
        "approach to {other}: top speed {peak:.2} units/s at {}, braking from {}",
        at(peak_i),
        at(braking)
    );
    match stopped {
        Some(i) => reading.push_str(&format!(", stopped at {}", at(i))),
        None => reading.push_str(", still moving at the end"),
    }
    if let Some((rad, end)) = turned {
        reading.push_str(&format!(
            ", then turned {rad:.2} rad on the spot until {}",
            at(end)
        ));
    }
    if let Some(c) = arrived {
        reading.push_str(&format!(" (engine.arrived at t={:.3})", c.t));
    }
    Some(Approach {
        report: json!({
            "top_speed": { "units_per_s": round(peak, 2), "at": when(&frames[peak_i]) },
            "braking_from": when(&frames[braking]),
            "speeds": points,
            "stopped_at": stopped.map(|i| when(&frames[i])),
            "turned_after_stop": turned.map(|(rad, end)| json!({
                "radians": round(rad, 2),
                "until": when(&frames[end]),
            })),
            "arrived_at": arrived.map(|c| json!({ "t": round(c.t, 3), "tick": c.tick })),
        }),
        reading,
    })
}

fn describe(what: &str, place: &str, score: Option<f32>, u: &Unevenness) -> String {
    let place = if place.is_empty() {
        String::new()
    } else {
        format!(" {place}")
    };
    match score {
        None => format!("{what} barely moves{place}"),
        Some(s) if s < SMOOTH => format!("{what} moves smoothly{place} (judder {s:.2})"),
        Some(s) if s < JUDDERS => format!("{what} moves unevenly{place} (judder {s:.2})"),
        Some(s) => format!(
            "{what} judders{place} (judder {s:.2}: steps alternate, as if it moved only on some \
             frames; median wobble {:.2} px)",
            median(&u.moving)
        ),
    }
}

struct Unevenness {
    /// Per frame: how far its step is from the mean of the steps either side (px).
    residuals: Vec<Option<f32>>,
    /// The residuals where it moves at all.
    moving: Vec<f32>,
    /// The median of those over the median expected step.
    score: Option<f32>,
}

fn steps(p: &[Option<[f32; 2]>]) -> Vec<Option<[f32; 2]>> {
    let mut out = vec![None];
    for w in p.windows(2) {
        out.push(match (w[0], w[1]) {
            (Some(a), Some(b)) => Some([b[0] - a[0], b[1] - a[1]]),
            _ => None,
        });
    }
    out
}

fn unevenness(p: &[Option<[f32; 2]>]) -> Unevenness {
    let d = steps(p);
    let mut residuals = vec![None; p.len()];
    let mut res = Vec::new();
    let mut expect = Vec::new();
    for i in 1..d.len().saturating_sub(1) {
        if let (Some(a), Some(b), Some(c)) = (d[i - 1], d[i], d[i + 1]) {
            let mean = [(a[0] + c[0]) / 2.0, (a[1] + c[1]) / 2.0];
            let r = dist(b, mean);
            residuals[i] = Some(r);
            let m = len(mean).max(len(b));
            if m > 0.05 {
                res.push(r);
                expect.push(len(mean).max(len(b) / 2.0));
            }
        }
    }
    let score = (res.len() >= 3).then(|| median(&res) / median(&expect).max(1e-6));
    Unevenness {
        residuals,
        moving: res,
        score,
    }
}

struct Gap {
    values: Vec<Option<f32>>,
    report: Value,
    reading: String,
}

fn gap(film: &Film, id: &str, other: &str, tolerance: Option<f32>) -> Gap {
    let frames = film.frames();
    let pairs: Vec<Option<([f32; 2], [f32; 2])>> = frames
        .iter()
        .map(|f| {
            Some((
                [f.things.get(id)?.x, f.things.get(id)?.y],
                [f.things.get(other)?.x, f.things.get(other)?.y],
            ))
        })
        .collect();
    let values: Vec<Option<f32>> = pairs.iter().map(|p| p.map(|(a, b)| dist(a, b))).collect();
    let present: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .collect();
    let Some(&(_, start)) = present.first() else {
        return Gap {
            values,
            report: json!(null),
            reading: format!("{other} is not in the film with {id}"),
        };
    };
    let &(_, end) = present.last().expect("present");
    let tol = tolerance.unwrap_or((start * 0.02).max(0.25));
    let (min_i, min) = present.iter().fold(
        (0, f32::INFINITY),
        |a, &(i, v)| if v < a.1 { (i, v) } else { a },
    );
    // Past the other thing along the way it came from.
    let first = pairs.iter().flatten().next().expect("present");
    let away = [first.0[0] - first.1[0], first.0[1] - first.1[1]];
    let away_len = len(away).max(1e-6);
    let u = [away[0] / away_len, away[1] / away_len];
    let overshoot = pairs
        .iter()
        .flatten()
        .map(|(a, b)| -((a[0] - b[0]) * u[0] + (a[1] - b[1]) * u[1]))
        .fold(0.0f32, f32::max);
    let (rebound_i, rebound) = present
        .iter()
        .filter(|(i, _)| *i > min_i)
        .map(|&(i, v)| (i, v - min))
        .fold((min_i, 0.0f32), |a, b| if b.1 > a.1 { b } else { a });
    // An input or order between the closest moment and the furthest rebound: the gap may
    // open because of it, not because the thing overshot.
    let cause = film.manifest.changes.iter().find(|c| {
        c.kind != "arrived" && c.tick > frames[min_i].tick && c.tick <= frames[rebound_i].tick
    });
    // Settled: within the tolerance of where it ends for the last half second at least.
    let run = present
        .iter()
        .rev()
        .take_while(|(_, v)| (v - end).abs() <= tol)
        .count();
    let settled = (run >= (film.manifest.fps as usize / 2).max(2) && present.len() > 1)
        .then(|| present[present.len() - run].0);
    let settled_at = settled
        .filter(|&i| i > present[0].0)
        .map(|i| when(&frames[i]));
    let mut reading =
        format!("the gap to {other} goes from {start:.2} to {end:.2} (closest {min:.2})");
    if overshoot > tol {
        reading.push_str(&format!(", overshooting by {overshoot:.2}"));
    }
    if rebound > tol {
        match cause {
            Some(c) => reading.push_str(&format!(
                ", opening again by {rebound:.2} after its closest, after {} `{}` at t={:.3}: that, not a bounce, may be why",
                c.kind, c.what, c.t
            )),
            None => reading.push_str(&format!(", bouncing back {rebound:.2} after its closest")),
        }
    }
    match &settled_at {
        Some(w) => reading.push_str(&format!(", settled within {tol:.2} from t={}", w["t"])),
        None if settled == Some(present[0].0) => reading.push_str(", steady throughout"),
        None => reading.push_str(", not settled by the end"),
    }
    Gap {
        values,
        report: json!({
            "start": round(start, 3),
            "end": round(end, 3),
            "closest": round(min, 3),
            "closest_at": when(&frames[min_i]),
            "overshoot": round(overshoot + 0.0, 3),
            "rebound": round(rebound, 3),
            "rebound_after": cause.filter(|_| rebound > tol).map(|c| json!({
                "t": round(c.t, 3), "tick": c.tick, "kind": c.kind, "what": c.what,
            })),
            "tolerance": round(tol, 3),
            "settled_at": settled_at,
        }),
        reading,
    }
}

fn stats(values: &[Option<f32>], frames: &[Frame]) -> Value {
    let present: Vec<(usize, f32)> = values
        .iter()
        .enumerate()
        .filter_map(|(i, v)| v.map(|v| (i, v)))
        .collect();
    if present.is_empty() {
        return Value::Null;
    }
    let (max_i, max) =
        present.iter().fold(
            (0, f32::NEG_INFINITY),
            |a, &(i, v)| if v > a.1 { (i, v) } else { a },
        );
    let mean = present.iter().map(|(_, v)| v).sum::<f32>() / present.len() as f32;
    json!({ "mean": round(mean, 2), "max": round(max, 2), "max_at": when(&frames[max_i]) })
}

pub(crate) fn median(v: &[f32]) -> f32 {
    if v.is_empty() {
        return 0.0;
    }
    let mut s = v.to_vec();
    s.sort_by(f32::total_cmp);
    s[s.len() / 2]
}

fn len(v: [f32; 2]) -> f32 {
    (v[0] * v[0] + v[1] * v[1]).sqrt()
}

fn dist(a: [f32; 2], b: [f32; 2]) -> f32 {
    len([a[0] - b[0], a[1] - b[1]])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(xs: &[f32]) -> Vec<Option<[f32; 2]>> {
        xs.iter().map(|&x| Some([x, 0.0])).collect()
    }

    #[test]
    fn smooth_motion_has_no_judder_even_when_it_speeds_up() {
        let xs: Vec<f32> = (0..40).map(|i| 0.02 * (i * i) as f32 + i as f32).collect();
        let u = unevenness(&path(&xs));
        assert!(u.score.unwrap() < 0.05, "{:?}", u.score);
    }

    #[test]
    fn moving_on_every_other_frame_judders() {
        // A thing drawn at its tick position at 60 fps: 0, 0, 2, 2, 4, 4...
        let xs: Vec<f32> = (0..40).map(|i| (i / 2) as f32 * 2.0).collect();
        let u = unevenness(&path(&xs));
        assert!(u.score.unwrap() > 0.8, "{:?}", u.score);
    }

    #[test]
    fn standing_still_is_not_judder() {
        let u = unevenness(&path(&[3.0; 20]));
        assert_eq!(u.score, None);
    }
}
