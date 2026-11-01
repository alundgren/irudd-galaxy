//! `strip`: a kymograph. It samples the pixels along one path in every frame and stacks
//! the samples, time running down, into one picture. Something still along the path draws
//! vertical stripes; something sliding along it draws slanted ones; popping and flicker
//! show as broken rows.
//!
//! The path is fixed in the world (`--along course`, the leg being flown, or
//! `--along x,y;x,y` in world units) and projected through each frame's own camera, so the
//! camera following the ship doesn't move it. `--screen x,y;x,y` samples a path fixed on
//! screen instead, for overlays.
//!
//! How far the pattern moves between frames is found by correlating each row with the one
//! before over small shifts, after taking out slow brightness changes (an intended fade is
//! not movement). A periodic pattern can only be tracked up to half its period a frame;
//! faster than that it aliases, and the report says when it is close.

use serde_json::{Value, json};

use crate::film::{Film, round, when};
use crate::image::{ACCENT, FLAG, Image, PAPER, TEXT};
use crate::opts::Opts;
use crate::track::median;

/// Rows whose pattern varies less than this (brightness, 0 to 1) show nothing to track.
const FLAT: f32 = 0.01;
/// Correlation below which a step is not trusted.
const TRUST: f32 = 0.5;
/// A shift smaller than this (px a frame) is still.
const STILL: f32 = 0.1;

/// The options `strip` takes, besides `--image`.
pub const OPTIONS: &[&str] = &["along", "screen", "max-shift", "width"];

pub fn run(film: &Film, opts: &Opts) -> Result<Value, String> {
    opts.only("strip", OPTIONS)?;
    film.need_pictures("strip")?;
    let frames = film.frames();
    let (w, h) = film.size();
    let path = Path::choose(film, opts)?;
    let half_width = opts.count("width")?.unwrap_or(2).min(8) as i32;
    let max_shift = match opts.number("max-shift")? {
        Some(s) => s.max(1.0),
        None => path.period_px.map_or(8.0, |p| (p / 2.0 - 0.5).max(1.0)),
    };

    // Sample every frame along the path.
    let mut rows: Vec<Vec<f32>> = Vec::with_capacity(frames.len());
    let mut colours: Vec<Vec<[u8; 4]>> = Vec::with_capacity(frames.len());
    for (i, frame) in frames.iter().enumerate() {
        let img = film.image(i)?;
        let pts = path.screen_points(frame, w, h);
        let mut row = Vec::with_capacity(pts.len());
        let mut colour = Vec::with_capacity(pts.len());
        for k in 0..pts.len() {
            let Some(p) = pts[k] else {
                row.push(f32::NAN);
                colour.push(PAPER);
                continue;
            };
            let prev = k.checked_sub(1).and_then(|j| pts[j]).unwrap_or(p);
            let next = pts.get(k + 1).copied().flatten().unwrap_or(p);
            let (dx, dy) = (next[0] - prev[0], next[1] - prev[1]);
            let l = (dx * dx + dy * dy).sqrt();
            let normal = if l > 1e-6 {
                [-dy / l, dx / l]
            } else {
                [0.0, 1.0]
            };
            let mut sum = [0.0f32; 4];
            let mut n = 0.0;
            for o in -half_width..=half_width {
                if let Some(c) =
                    img.sample(p[0] + normal[0] * o as f32, p[1] + normal[1] * o as f32)
                {
                    for j in 0..4 {
                        sum[j] += c[j];
                    }
                    n += 1.0;
                }
            }
            if n == 0.0 {
                row.push(f32::NAN);
                colour.push(PAPER);
            } else {
                let c = [sum[0] / n, sum[1] / n, sum[2] / n];
                row.push((0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]) / 255.0);
                colour.push([c[0] as u8, c[1] as u8, c[2] as u8, 255]);
            }
        }
        rows.push(row);
        colours.push(colour);
    }

    let band = path
        .period_px
        .map_or(24, |p| (p * 2.0).ceil() as usize)
        .max(8);
    let detail: Vec<Vec<f32>> = rows.iter().map(|r| high_pass(r, band)).collect();
    let mut shifts: Vec<Option<(f32, f32)>> = vec![None];
    for i in 1..detail.len() {
        shifts.push(shift(&detail[i - 1], &detail[i], max_shift));
    }
    let flat: Vec<bool> = detail.iter().map(|r| spread(r) < FLAT).collect();

    let trusted: Vec<(usize, f32)> = shifts
        .iter()
        .enumerate()
        .filter_map(|(i, s)| {
            s.filter(|s| s.1 >= TRUST && !flat[i] && !flat[i - 1])
                .map(|s| (i, s.0))
        })
        .collect();
    let values: Vec<f32> = trusted.iter().map(|t| t.1).collect();
    let per_frame = median(&values);
    let total: f32 = values.iter().sum();
    let moving = values.iter().filter(|v| v.abs() > STILL).count();
    let confidence = median(&shifts.iter().flatten().map(|s| s.1).collect::<Vec<_>>());
    let pops: Vec<Value> = shifts
        .iter()
        .enumerate()
        .filter(|(i, s)| *i > 0 && !flat[*i] && !flat[*i - 1] && s.is_none_or(|s| s.1 < TRUST))
        .map(|(i, _)| when(&frames[i]))
        .take(12)
        .collect();
    let absent = intervals(&flat, frames);
    let drifting: Vec<bool> = (0..frames.len())
        .map(|i| trusted.iter().any(|t| t.0 == i && t.1.abs() > STILL))
        .collect();
    let runs = intervals(&drifting, frames)
        .into_iter()
        .filter(|r| r["frames"].as_u64().unwrap_or(0) >= 3)
        .map(|mut r| {
            let (a, b) = (
                r["from"]["frame"].as_u64().unwrap_or(0) as usize,
                r["to"]["frame"].as_u64().unwrap_or(0) as usize,
            );
            let seg: Vec<f32> = trusted
                .iter()
                .filter(|t| t.0 >= a && t.0 <= b)
                .map(|t| t.1)
                .collect();
            r["px_per_frame"] = json!(round(median(&seg), 2));
            r
        })
        .take(8)
        .collect::<Vec<_>>();
    let fps = film.manifest.fps as f32;
    let near_alias = values.iter().filter(|v| v.abs() > max_shift * 0.8).count();

    let reading = if values.is_empty() {
        "nothing along the path could be followed from frame to frame (no pattern, or it \
         changes completely every frame)"
            .to_owned()
    } else if per_frame.abs() <= STILL && (moving as f32) < values.len() as f32 * 0.2 {
        format!(
            "the pattern along the path stays still: median {per_frame:+.2} px a frame, moving \
             in {moving} of {} frame steps",
            values.len()
        )
    } else {
        format!(
            "the pattern slides along the path {} at {:.2} px a frame ({:.0} px/s), in {moving} \
             of {} frame steps; {:.0} px in all",
            if per_frame > 0.0 {
                path.forward
            } else {
                path.backward
            },
            per_frame.abs(),
            per_frame.abs() * fps,
            values.len(),
            total.abs()
        )
    };
    let mut reading = reading;
    if near_alias > values.len() / 10 && !values.is_empty() {
        reading.push_str(&format!(
            "; {near_alias} steps are near the limit of {max_shift:.1} px a frame and may alias"
        ));
    }
    if !pops.is_empty() {
        reading.push_str(&format!(
            "; {} frames break from the one before (pop or flicker)",
            pops.len()
        ));
    }

    let img = picture(&colours, &shifts, max_shift, frames, &reading, &path.label);
    let out = opts.image_path(&film.dir, "strip")?;
    img.save_png(&out)?;
    Ok(json!({
        "tool": "strip",
        "along": path.label,
        "length_px": rows.first().map_or(0, Vec::len),
        "period_px": path.period_px.map(|p| round(p, 2)),
        "frames": frames.len(),
        "drift_px_per_frame": round(per_frame, 3),
        "drift_px_per_second": round(per_frame * fps, 1),
        "total_drift_px": round(total, 1),
        "moving_steps": moving,
        "steps": values.len(),
        "confidence": round(confidence, 3),
        "max_shift_px": round(max_shift, 1),
        "positive_is": path.forward,
        "drifting": runs,
        "pops": pops,
        "absent": absent,
        "reading": reading,
        "image": out.display().to_string(),
    }))
}

/// The sampled path: world points (projected each frame) or screen points.
pub(crate) struct Path {
    world: Option<Vec<[f32; 3]>>,
    screen: Option<Vec<[f32; 2]>>,
    /// Samples along it (about one a pixel).
    samples: usize,
    pub(crate) period_px: Option<f32>,
    pub(crate) label: String,
    forward: &'static str,
    backward: &'static str,
}

impl Path {
    pub(crate) fn choose(film: &Film, opts: &Opts) -> Result<Path, String> {
        let first = &film.frames()[0];
        let (w, h) = film.size();
        if let Some(points) = opts.points("screen")? {
            let length: f32 = points.windows(2).map(|p| d2(p[0], p[1])).sum();
            return Ok(Path {
                world: None,
                screen: Some(points.clone()),
                samples: (length.ceil() as usize).clamp(8, 4000),
                period_px: None,
                label: format!("screen path {}", fmt_points(&points)),
                forward: "toward its last point",
                backward: "toward its first point",
            });
        }
        match opts.text("along") {
            Some("course") | None => Path::course(film),
            Some(_) => {
                let points = opts.points("along")?.expect("text is set");
                let world: Vec<[f32; 3]> = points.iter().map(|p| [p[0], p[1], 0.0]).collect();
                let length = screen_length(&world, first, w, h);
                Ok(Path {
                    samples: (length.ceil() as usize).clamp(8, 4000),
                    world: Some(world),
                    screen: None,
                    period_px: None,
                    label: format!("world path {}", fmt_points(&points)),
                    forward: "toward its last point",
                    backward: "toward its first point",
                })
            }
        }
    }

    /// The course leg being flown, from just inside its far end to just short of where
    /// the followed thing gets to by the last frame, so neither the ring nor the thing
    /// passes over it.
    fn course(film: &Film) -> Result<Path, String> {
        let frames = film.frames();
        let first = &frames[0];
        let (w, h) = film.size();
        let followed = first.things.values().find(|t| t.followed).ok_or(
            "no thing is followed, so there is no course; give a path with --along x,y;x,y",
        )?;
        let last_pos = frames
            .last()
            .and_then(|f| f.things.values().find(|t| t.followed))
            .map_or(followed.point(), |t| t.point());
        let leg = first
            .lines
            .iter()
            .filter(|l| l.kind == "dotted" && !l.closed && l.points.len() >= 2)
            .min_by(|a, b| {
                let da = d3(*a.points.last().expect("2"), followed.point());
                let db = d3(*b.points.last().expect("2"), followed.point());
                da.total_cmp(&db)
            })
            .ok_or("the first frame shows no course (no dotted line); film from after the order is given")?;
        let far = leg.points[0];
        let near = *leg.points.last().expect("2");
        let length = d3(far, near);
        let dir = [
            (near[0] - far[0]) / length,
            (near[1] - far[1]) / length,
            (near[2] - far[2]) / length,
        ];
        let spacing = leg.spacing.unwrap_or(1.0);
        // How far along from the far end the thing gets by the end.
        let reach =
            ((last_pos[0] - far[0]) * dir[0] + (last_pos[1] - far[1]) * dir[1]).clamp(0.0, length);
        let start = spacing;
        let end = reach - 3.0 * spacing;
        if end - start < 4.0 * spacing {
            return Err(format!(
                "the followed thing flies over most of the leg it is on during the film, leaving \
                 {:.1} world units of course to watch; film a shorter span (--to), or one where \
                 the leg is longer",
                (end - start).max(0.0)
            ));
        }
        let at = |a: f32| {
            [
                far[0] + dir[0] * a,
                far[1] + dir[1] * a,
                far[2] + dir[2] * a,
            ]
        };
        let world = vec![at(start), at(end)];
        let per_pixel = first.camera.per_pixel.max(1e-9);
        let px = screen_length(&world, first, w, h);
        Ok(Path {
            samples: (px.ceil() as usize).clamp(8, 4000),
            world: Some(world),
            screen: None,
            period_px: Some(spacing / per_pixel),
            label: format!(
                "the course leg toward ({:.1}, {:.1}), {:.1} world units of it",
                far[0],
                far[1],
                end - start
            ),
            forward: "toward the thing flying it",
            backward: "toward where the leg ends (along with the thing, the reeled-in look)",
        })
    }

    pub(crate) fn screen_points(
        &self,
        frame: &crate::film::Frame,
        w: u32,
        h: u32,
    ) -> Vec<Option<[f32; 2]>> {
        let n = self.samples;
        let along = |pts: &[[f32; 3]], t: f32| -> [f32; 3] {
            let lengths: Vec<f32> = pts.windows(2).map(|p| d3(p[0], p[1])).collect();
            let total: f32 = lengths.iter().sum();
            let mut left = t * total;
            for (i, l) in lengths.iter().enumerate() {
                if left <= *l || i == lengths.len() - 1 {
                    let f = if *l > 0.0 { (left / l).min(1.0) } else { 0.0 };
                    let (a, b) = (pts[i], pts[i + 1]);
                    return [
                        a[0] + (b[0] - a[0]) * f,
                        a[1] + (b[1] - a[1]) * f,
                        a[2] + (b[2] - a[2]) * f,
                    ];
                }
                left -= l;
            }
            pts[pts.len() - 1]
        };
        (0..n)
            .map(|k| {
                let t = k as f32 / (n - 1).max(1) as f32;
                match (&self.world, &self.screen) {
                    (Some(world), _) => frame.camera.project(along(world, t), w, h),
                    (None, Some(screen)) => {
                        let pts: Vec<[f32; 3]> = screen.iter().map(|p| [p[0], p[1], 0.0]).collect();
                        let p = along(&pts, t);
                        Some([p[0], p[1]])
                    }
                    _ => None,
                }
            })
            .collect()
    }
}

/// The row with its slow changes taken out: each sample less the mean around it.
fn high_pass(row: &[f32], band: usize) -> Vec<f32> {
    let n = row.len();
    let mut out = vec![f32::NAN; n];
    for i in 0..n {
        if !row[i].is_finite() {
            continue;
        }
        let (a, b) = (i.saturating_sub(band), (i + band + 1).min(n));
        let (mut s, mut c) = (0.0, 0.0);
        for v in &row[a..b] {
            if v.is_finite() {
                s += v;
                c += 1.0;
            }
        }
        out[i] = row[i] - s / c;
    }
    out
}

fn spread(row: &[f32]) -> f32 {
    let v: Vec<f32> = row.iter().copied().filter(|v| v.is_finite()).collect();
    if v.len() < 4 {
        return 0.0;
    }
    let mean = v.iter().sum::<f32>() / v.len() as f32;
    (v.iter().map(|x| (x - mean).powi(2)).sum::<f32>() / v.len() as f32).sqrt()
}

/// How far `b`'s pattern is shifted from `a`'s (positive: further along), and how well
/// they match at that shift (normalised correlation, -1 to 1).
pub(crate) fn shift(a: &[f32], b: &[f32], max: f32) -> Option<(f32, f32)> {
    let m = max.ceil() as i64;
    let score = |s: i64| -> Option<f32> {
        let (mut sa, mut sb, mut saa, mut sbb, mut sab, mut n) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
        for (x, &bv) in b.iter().enumerate() {
            let xa = x as i64 - s;
            if xa < 0 || xa >= a.len() as i64 {
                continue;
            }
            let av = a[xa as usize];
            if !(av.is_finite() && bv.is_finite()) {
                continue;
            }
            sa += av;
            sb += bv;
            saa += av * av;
            sbb += bv * bv;
            sab += av * bv;
            n += 1.0;
        }
        if n < 8.0 {
            return None;
        }
        let cov = sab / n - sa / n * sb / n;
        let va = saa / n - (sa / n).powi(2);
        let vb = sbb / n - (sb / n).powi(2);
        if va <= 1e-10 || vb <= 1e-10 {
            return None;
        }
        Some(cov / (va * vb).sqrt())
    };
    let scores: Vec<(i64, f32)> = (-m..=m).filter_map(|s| score(s).map(|v| (s, v))).collect();
    let &(best, peak) = scores.iter().max_by(|a, b| a.1.total_cmp(&b.1))?;
    let at = |s: i64| scores.iter().find(|x| x.0 == s).map(|x| x.1);
    let refined = match (at(best - 1), at(best + 1)) {
        (Some(l), Some(r)) => {
            let denom = l - 2.0 * peak + r;
            if denom.abs() > 1e-9 {
                best as f32 + (0.5 * (l - r) / denom).clamp(-0.5, 0.5)
            } else {
                best as f32
            }
        }
        _ => best as f32,
    };
    Some((refined, peak))
}

/// Runs of frames where `on` holds, as `from`/`to` moments.
pub(crate) fn intervals(on: &[bool], frames: &[crate::film::Frame]) -> Vec<Value> {
    let mut out = Vec::new();
    let mut start = None;
    for i in 0..=on.len() {
        let here = on.get(i).copied().unwrap_or(false);
        match (start, here) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                out.push(json!({ "from": when(&frames[s]), "to": when(&frames[i - 1]), "frames": i - s }));
                start = None;
            }
            _ => {}
        }
    }
    out
}

/// The kymograph, time down, with the measured shift beside each row.
fn picture(
    colours: &[Vec<[u8; 4]>],
    shifts: &[Option<(f32, f32)>],
    max_shift: f32,
    frames: &[crate::film::Frame],
    reading: &str,
    label: &str,
) -> Image {
    let n = colours.first().map_or(0, Vec::len).max(1);
    // Short paths are stretched to stay readable; long ones squeezed.
    let width = n.clamp(360, 720) as u32;
    let row_h = if colours.len() < 200 { 2 } else { 1 };
    let top = 50;
    let left = 56;
    let bar = 80;
    let height = top + colours.len() as u32 * row_h + 10;
    let mut img = Image::new(left + width + bar + 20, height, PAPER);
    img.text(6, 4, &format!("strip: {label}"), 1, TEXT);
    for (i, line) in wrap(reading, (left + width + bar) as usize / 6)
        .iter()
        .take(3)
        .enumerate()
    {
        img.text(6, 16 + i as i64 * 10, line, 1, ACCENT);
    }
    for (r, row) in colours.iter().enumerate() {
        for x in 0..width {
            let k = (x as usize * n / width as usize).min(row.len().saturating_sub(1));
            let c = row.get(k).copied().unwrap_or(PAPER);
            for dy in 0..row_h {
                img.set(
                    i64::from(left + x),
                    i64::from(top + r as u32 * row_h + dy),
                    c,
                );
            }
        }
        // The shift bar: centre is still, right is forward.
        let cx = (left + width + 10 + bar / 2) as f32;
        let y = (top + r as u32 * row_h) as f32;
        if let Some((s, c)) = shifts.get(r).copied().flatten() {
            let x = cx + s / max_shift * (bar as f32 / 2.0);
            let colour = if c < TRUST { FLAG } else { ACCENT };
            img.line(cx, y, x, y, colour);
        }
        img.set(cx as i64, y as i64, TEXT);
    }
    let step = (frames.len() / 8).max(1);
    for (r, f) in frames.iter().enumerate().step_by(step) {
        img.text(
            4,
            i64::from(top + r as u32 * row_h),
            &format!("{:.2}s", f.time),
            1,
            TEXT,
        );
    }
    img
}

/// `text` in lines of at most `chars` characters.
pub(crate) fn wrap(text: &str, chars: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        if !line.is_empty() && line.len() + word.len() + 1 > chars {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn d2(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}

fn d3(a: [f32; 3], b: [f32; 3]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2)).sqrt()
}

fn screen_length(world: &[[f32; 3]], frame: &crate::film::Frame, w: u32, h: u32) -> f32 {
    world
        .windows(2)
        .filter_map(|p| {
            Some(d2(
                frame.camera.project(p[0], w, h)?,
                frame.camera.project(p[1], w, h)?,
            ))
        })
        .sum()
}

fn fmt_points(p: &[[f32; 2]]) -> String {
    p.iter()
        .map(|p| format!("{},{}", p[0], p[1]))
        .collect::<Vec<_>>()
        .join(";")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Dots `period` apart, shifted `offset` along.
    fn dots(n: usize, period: f32, offset: f32) -> Vec<f32> {
        (0..n)
            .map(|x| {
                let p = ((x as f32 - offset) / period).rem_euclid(1.0);
                if (p - 0.5).abs() < 0.2 { 1.0 } else { 0.1 }
            })
            .collect()
    }

    #[test]
    fn a_crawling_pattern_is_measured_and_a_still_one_is_not() {
        let a = high_pass(&dots(300, 16.0, 0.0), 32);
        let b = high_pass(&dots(300, 16.0, 0.8), 32);
        let (s, c) = shift(&a, &b, 7.5).unwrap();
        assert!((s - 0.8).abs() < 0.25, "{s}");
        assert!(c > 0.8);
        let (s, _) = shift(&a, &a, 7.5).unwrap();
        assert!(s.abs() < 1e-3);
        let back = high_pass(&dots(300, 16.0, -3.0), 32);
        let (s, _) = shift(&a, &back, 7.5).unwrap();
        assert!((s + 3.0).abs() < 0.3, "{s}");
    }

    #[test]
    fn a_fade_is_not_movement() {
        let a = high_pass(&dots(300, 16.0, 0.0), 32);
        let faded: Vec<f32> = dots(300, 16.0, 0.0).iter().map(|v| v * 0.4).collect();
        let (s, c) = shift(&a, &high_pass(&faded, 32), 7.5).unwrap();
        assert!(s.abs() < 1e-3 && c > 0.99);
    }
}
