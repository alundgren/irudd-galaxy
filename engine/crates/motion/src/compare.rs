//! `compare`: two films of the same moments, frame by frame: an approved one and a new
//! one, a good build and a changed one. FLIP ([`crate::flip`]) scores how different each
//! pair of frames looks, per pixel. The report names the frames and places that differ
//! most; the picture shows the worst frame's error map beside a distogram (time down,
//! across the screen left to right, brighter where the films differ).
//!
//! When both films follow the same thing, `behaviour` puts how it moved in each side by
//! side (path, top speed, arrival, end), so one command says whether it behaves
//! differently as well as whether it looks different.
//!
//! A difference says where to look, not that the new film is wrong: the approved film may
//! carry the flaw. With `--vdp`, ColorVideoVDP ([`crate::vdp`]) also scores the pair as
//! video, with a model of how the eye sees change over time.

use std::path::Path;

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::flip::{PIXELS_PER_DEGREE, flip};
use crate::image::{FLAG, Image, PAPER, TEXT, heat};
use crate::opts::Opts;
use crate::strip::wrap;
use crate::track::median;

/// A pixel differs plainly at this FLIP error.
const PLAIN: f32 = 0.5;
/// A frame differs visibly when at least this many pixels do: a mean over the whole
/// frame would hide a small thing that changed.
const VISIBLE: usize = 16;
const BINS: usize = 64;

/// The options `compare` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["with", "half", "vdp", "ppd"];

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("compare", OPTIONS)?;
    let other_dir = opts
        .text("with")
        .ok_or("compare needs the second film: galaxy-gm motion compare <film> <other film>")?;
    let other = Film::read(Path::new(other_dir))?;
    film.need_pictures("compare")?;
    other.need_pictures("compare")?;
    if film.size() != other.size() {
        return Err(format!(
            "the films are different sizes ({}x{} and {}x{}); film them at one --size",
            film.size().0,
            film.size().1,
            other.size().0,
            other.size().1
        ));
    }
    let n = film.frames().len().min(other.frames().len());
    let half = opts.flag("half");
    let ppd = opts.number("ppd")?.unwrap_or(PIXELS_PER_DEGREE) / if half { 2.0 } else { 1.0 };
    let (w, h) = film.size();
    let (cw, ch) = if half { (w / 2, h / 2) } else { (w, h) };

    // Score every pair, spread over the cores.
    let threads = std::thread::available_parallelism()
        .map_or(4, |n| n.get())
        .min(16);
    let mut results: Vec<Option<Pair>> = (0..n).map(|_| None).collect();
    let chunk = n.div_ceil(threads);
    std::thread::scope(|scope| -> Result<(), String> {
        let mut handles = Vec::new();
        for (c, slot) in results.chunks_mut(chunk.max(1)).enumerate() {
            let (film, other) = (&film, &other);
            handles.push(scope.spawn(move || -> Result<(), String> {
                for (k, out) in slot.iter_mut().enumerate() {
                    let i = c * chunk + k;
                    let (mut a, mut b) = (film.image(i)?, other.image(i)?);
                    if half {
                        a = a.resized(cw, ch);
                        b = b.resized(cw, ch);
                    }
                    let e = flip(&a, &b, ppd);
                    *out = Some(Pair::of(&e, cw as usize, ch as usize));
                }
                Ok(())
            }));
        }
        for hnd in handles {
            hnd.join()
                .map_err(|_| "a comparison thread failed".to_owned())??;
        }
        Ok(())
    })?;
    let pairs: Vec<Pair> = results
        .into_iter()
        .map(|p| p.expect("every pair scored"))
        .collect();
    let frames = film.frames();
    let means: Vec<f32> = pairs.iter().map(|p| p.mean).collect();
    let same_world = (0..n)
        .filter(|&i| frames[i].hash == other.frames()[i].hash)
        .count();
    let plain: Vec<usize> = pairs.iter().map(|p| p.plain).collect();
    let visible = plain.iter().filter(|p| **p >= VISIBLE).count();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| {
        plain[*b]
            .cmp(&plain[*a])
            .then(means[*b].total_cmp(&means[*a]))
    });
    let worst = order[0];
    let worst_list: Vec<Value> = order
        .iter()
        .take(5)
        .filter(|&&i| plain[i] > 0 || means[i] > 0.0)
        .map(|&i| {
            let mut m = when(&frames[i]);
            m["mean"] = json!(round(means[i], 4));
            m["pixels_differing"] = json!(plain[i] * if half { 4 } else { 1 });
            m["box_px"] = json!(
                pairs[i]
                    .bbox
                    .map(|b| b.map(|v| if half { v * 2 } else { v }))
            );
            m
        })
        .collect();
    let near: Vec<String> = pairs[worst]
        .bbox
        .map(|b| {
            let s = if half { 2.0 } else { 1.0 };
            frames[worst]
                .things
                .iter()
                .filter(|(_, t)| {
                    t.screen.is_some_and(|p| {
                        p[0] >= b[0] as f32 * s - 10.0
                            && p[0] <= (b[0] + b[2]) as f32 * s + 10.0
                            && p[1] >= b[1] as f32 * s - 10.0
                            && p[1] <= (b[1] + b[3]) as f32 * s + 10.0
                    })
                })
                .map(|(id, _)| id.clone())
                .collect()
        })
        .unwrap_or_default();
    let name = |f: &Film| {
        f.dir
            .file_name()
            .map_or("film".to_owned(), |n| n.to_string_lossy().into_owned())
    };
    let moved = behaviour([film, &other], [&name(film), &name(&other)]);
    let mut reading = if visible == 0 {
        format!(
            "the films look the same: no frame has {VISIBLE} pixels with FLIP {PLAIN} or more (the most differing frame, t={:.3}, has {} and a mean of {:.4})",
            frames[worst].time, plain[worst], means[worst]
        )
    } else {
        format!(
            "{visible} of {n} frames differ visibly; most at t={:.3} ({} pixels with FLIP {PLAIN} or more) in box {:?} near {:?}; the worlds are {} in {same_world} of {n} frames",
            frames[worst].time,
            plain[worst] * if half { 4 } else { 1 },
            pairs[worst]
                .bbox
                .map(|b| b.map(|v| if half { v * 2 } else { v }))
                .unwrap_or_default(),
            near,
            if same_world == n {
                "identical"
            } else {
                "identical only"
            }
        )
    };

    if let Some(m) = &moved {
        reading = format!("{reading}; {}", m["reading"].as_str().unwrap_or(""));
    }

    // The picture: the worst frame's error map, and the distogram.
    let map_w = 480u32;
    let map_h = (ch as f32 * map_w as f32 / cw as f32) as u32;
    let row_h = if n < 240 { 2 } else { 1 };
    let dist_w = 256u32;
    let top = 40u32;
    let height = top + map_h.max(n as u32 * row_h) + 30;
    let mut img = Image::new(map_w + dist_w + 30, height, PAPER);
    for (k, line) in wrap(
        &format!("compare: {reading}"),
        (map_w + dist_w) as usize / 6,
    )
    .iter()
    .take(3)
    .enumerate()
    {
        img.text(6, 4 + k as i64 * 10, line, 1, TEXT);
    }
    // Scored again rather than kept: a map for every frame would be hundreds of MB.
    let (mut a, mut b) = (film.image(worst)?, other.image(worst)?);
    if half {
        a = a.resized(cw, ch);
        b = b.resized(cw, ch);
    }
    let worst_map = flip(&a, &b, ppd);
    for y in 0..map_h {
        for x in 0..map_w {
            let (sx, sy) = (
                x as usize * cw as usize / map_w as usize,
                y as usize * ch as usize / map_h as usize,
            );
            img.set(
                i64::from(x) + 6,
                i64::from(y + top),
                heat(worst_map[sy * cw as usize + sx]),
            );
        }
    }
    img.label(
        8,
        i64::from(top) + 2,
        &format!("worst t{:.3}", frames[worst].time),
        1,
        FLAG,
    );
    let x0 = i64::from(map_w) + 20;
    let peak = pairs
        .iter()
        .flat_map(|p| p.bins.iter())
        .fold(1e-6f32, |a, b| a.max(*b));
    for (i, p) in pairs.iter().enumerate() {
        for x in 0..dist_w {
            let v = p.bins[x as usize * BINS / dist_w as usize] / peak;
            for dy in 0..row_h {
                img.set(
                    x0 + i64::from(x),
                    i64::from(top + i as u32 * row_h + dy),
                    heat(v.sqrt()),
                );
            }
        }
    }
    img.text(
        x0,
        i64::from(top) - 10,
        "distogram: time down, x across",
        1,
        TEXT,
    );
    let out = opts.image_path(&film.dir, "compare")?;
    img.save_png(&out)?;

    let mut report = json!({
        "tool": "compare",
        "with": other_dir,
        "frames": n,
        "same_world_frames": same_world,
        "flip": {
            "mean": round(means.iter().sum::<f32>() / n.max(1) as f32, 4),
            "median_frame": round(median(&means), 4),
            "worst_frames": worst_list,
            "visibly_different_frames": visible,
        },
        "reading": reading,
        "image": out.display().to_string(),
    });
    if let Some(m) = moved {
        report["behaviour"] = m;
    }
    if opts.flag("vdp") {
        report["colorvideovdp"] = crate::vdp::run(film, &other);
    }
    Ok(report)
}

/// How the thing both films follow moves in each, side by side: its path length, top
/// speed and when, when it arrived and where it ended. None when the first film follows
/// nothing or the second doesn't have it. `names` label the films.
pub fn behaviour(films: [&Film; 2], names: [&str; 2]) -> Option<Value> {
    let id = films[0].frames()[0]
        .things
        .iter()
        .find(|(_, t)| t.followed)
        .map(|(id, _)| id.clone())?;
    films[1].frames()[0].things.get(&id)?;
    struct Moved {
        path: f32,
        top: f32,
        top_t: f32,
        arrived: Option<(f32, u64)>,
        end: [f32; 2],
    }
    let measure = |film: &Film| -> Option<Moved> {
        let frames = film.frames();
        let at: Vec<[f32; 2]> = frames
            .iter()
            .filter_map(|f| f.things.get(&id).map(|t| [t.x, t.y]))
            .collect();
        let end = *at.last()?;
        let (mut path, mut top, mut top_t) = (0.0f32, 0.0f32, frames[0].time);
        for (k, w) in frames.windows(2).enumerate() {
            let (Some(a), Some(b)) = (w[0].things.get(&id), w[1].things.get(&id)) else {
                continue;
            };
            let step = ((b.x - a.x).powi(2) + (b.y - a.y).powi(2)).sqrt();
            path += step;
            let speed = step / (w[1].time - w[0].time).max(1e-6);
            if speed > top {
                (top, top_t) = (speed, frames[k + 1].time);
            }
        }
        let arrived = film
            .manifest
            .changes
            .iter()
            .find(|c| c.kind == "arrived" && c.what.starts_with(&format!("{id}:")))
            .map(|c| (c.t, c.tick));
        Some(Moved {
            path,
            top,
            top_t,
            arrived,
            end,
        })
    };
    let (a, b) = (measure(films[0])?, measure(films[1])?);
    let row = |name: &str, m: &Moved| {
        format!(
            "{name}: path {:.3}, top speed {:.3} at t {:.3}, {}, ends at ({:.3}, {:.3})",
            m.path,
            m.top,
            m.top_t,
            m.arrived
                .map_or("never arrived".to_owned(), |(t, tick)| format!(
                    "arrived t {t:.3} (tick {tick})"
                )),
            m.end[0],
            m.end[1]
        )
    };
    let close = |x: f32, y: f32| (x - y).abs() <= 1e-3 * x.abs().max(y.abs()).max(1.0);
    let same = close(a.path, b.path)
        && close(a.top, b.top)
        && a.arrived.map(|x| x.1) == b.arrived.map(|x| x.1)
        && close(a.end[0], b.end[0])
        && close(a.end[1], b.end[1]);
    let reading = if same {
        format!(
            "{id} moves the same in both (path {:.3}, top speed {:.3}{})",
            a.path,
            a.top,
            a.arrived
                .map_or(String::new(), |(t, _)| format!(", arrived t {t:.3}"))
        )
    } else {
        let mut d = Vec::new();
        if !close(a.path, b.path) {
            d.push(format!("path {:.3} against {:.3}", a.path, b.path));
        }
        if !close(a.top, b.top) {
            d.push(format!("top speed {:.3} against {:.3}", a.top, b.top));
        }
        if a.arrived.map(|x| x.1) != b.arrived.map(|x| x.1) {
            let w = |m: &Moved| {
                m.arrived
                    .map_or("never".to_owned(), |(t, _)| format!("t {t:.3}"))
            };
            d.push(format!("arrived {} against {}", w(&a), w(&b)));
        }
        if !close(a.end[0], b.end[0]) || !close(a.end[1], b.end[1]) {
            d.push(format!(
                "ends at ({:.3}, {:.3}) against ({:.3}, {:.3})",
                a.end[0], a.end[1], b.end[0], b.end[1]
            ));
        }
        format!("{id} moves differently: {}", d.join(", "))
    };
    Some(json!({
        "thing": id,
        "same": same,
        "films": [row(names[0], &a), row(names[1], &b)],
        "reading": reading,
    }))
}

/// One pair of frames, scored.
struct Pair {
    mean: f32,
    /// Mean error across the screen, left to right.
    bins: Vec<f32>,
    /// Pixels scoring [`PLAIN`] or more, and the box round them.
    plain: usize,
    bbox: Option<[u32; 4]>,
}

impl Pair {
    fn of(e: &[f32], w: usize, h: usize) -> Pair {
        let mut bins = vec![0.0; BINS];
        let mut counts = vec![0usize; BINS];
        let (mut lo, mut hi) = ([usize::MAX; 2], [0usize; 2]);
        let mut plain = 0;
        for y in 0..h {
            for x in 0..w {
                let v = e[y * w + x];
                let b = x * BINS / w;
                bins[b] += v;
                counts[b] += 1;
                if v >= PLAIN {
                    plain += 1;
                    lo = [lo[0].min(x), lo[1].min(y)];
                    hi = [hi[0].max(x), hi[1].max(y)];
                }
            }
        }
        for (b, c) in bins.iter_mut().zip(&counts) {
            *b /= (*c).max(1) as f32;
        }
        Pair {
            mean: e.iter().sum::<f32>() / e.len().max(1) as f32,
            bins,
            plain,
            bbox: (lo[0] != usize::MAX).then(|| {
                [
                    lo[0] as u32,
                    lo[1] as u32,
                    (hi[0] - lo[0] + 1) as u32,
                    (hi[1] - lo[1] + 1) as u32,
                ]
            }),
        }
    }
}
