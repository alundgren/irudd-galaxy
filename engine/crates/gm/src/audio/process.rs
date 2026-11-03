use super::{
    inspect,
    meter::{self, Biquad},
};
use galaxy_audio::Pcm;
use galaxy_module::Failure;
use galaxy_motion::Opts;
use serde_json::{Value, json};
use std::path::Path;

pub const OPTIONS: &[&str] = &[
    "out",
    "target-lufs",
    "peak",
    "gain-db",
    "trim",
    "trim-silence",
    "fade-in",
    "fade-out",
    "remove-dc",
    "declick",
    "high-shelf",
    "low-shelf",
    "notch",
];
pub const LOOP_OPTIONS: &[&str] = &["out", "blend", "search", "curve", "target-lufs", "peak"];

pub(super) fn number(opts: &Opts, name: &str, low: f64, high: f64) -> Result<Option<f64>, Failure> {
    let value = opts.number(name).map_err(Failure::usage)?.map(f64::from);
    if value.is_some_and(|v| !(low..=high).contains(&v)) {
        return Err(Failure::usage(format!("--{name} must be {low}..{high}")));
    }
    Ok(value)
}

pub(super) fn write(path: &Path, pcm: &Pcm) -> Result<(), Failure> {
    if path.extension().and_then(|e| e.to_str()) != Some("wav") {
        return Err(Failure::usage("--out must be a new .wav file"));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(parent)
        .map_err(|e| Failure::bad_input(format!("can't create output: {e}")))?;
    {
        let mut writer = hound::WavWriter::new(
            temp.as_file_mut(),
            hound::WavSpec {
                sample_rate: pcm.rate,
                channels: pcm.channels,
                bits_per_sample: 32,
                sample_format: hound::SampleFormat::Float,
            },
        )
        .map_err(|e| Failure::runtime(e.to_string()))?;
        for frame in &pcm.frames {
            for &s in frame.iter().take(pcm.channels as usize) {
                if !s.is_finite() {
                    return Err(Failure::bad_input(
                        "processing produced a non-finite sample",
                    ));
                }
                writer
                    .write_sample(s)
                    .map_err(|e| Failure::runtime(e.to_string()))?;
            }
        }
        writer
            .finalize()
            .map_err(|e| Failure::runtime(e.to_string()))?;
    }
    // Persist without replacing: existing paths, symlinks and hard links are all refused.
    temp.persist_noclobber(path)
        .map_err(|e| Failure::bad_input(format!("output must be a new file: {}", e.error)))?;
    Ok(())
}

fn flag(opts: &Opts, name: &str) -> Result<bool, Failure> {
    match opts.text(name) {
        None => Ok(false),
        Some("true") => Ok(true),
        _ => Err(Failure::usage(format!("--{name} takes no value"))),
    }
}

fn eq(opts: &Opts, name: &str, rate: u32) -> Result<Option<Biquad>, Failure> {
    let Some(values) = opts.numbers(name).map_err(Failure::usage)? else {
        return Ok(None);
    };
    let notch = name == "notch";
    if values.len() != if notch { 3 } else { 2 } {
        return Err(Failure::usage(format!(
            "--{name} takes Hz,dB{}",
            if notch { ",Q" } else { "" }
        )));
    }
    let (hz, gain) = (f64::from(values[0]), f64::from(values[1]));
    if !(20.0..f64::from(rate) * 0.45).contains(&hz)
        || !(-6.0..=6.0).contains(&gain)
        || (notch && gain > 0.0)
    {
        return Err(Failure::usage(
            "EQ needs 20 Hz..0.45*sample_rate and gentle gain -6..6 dB (notch cuts only)",
        ));
    }
    let a = 10_f64.powf(gain / 40.0);
    let w = std::f64::consts::TAU * hz / f64::from(rate);
    let (s, c) = w.sin_cos();
    let (b, d) = if notch {
        let q = f64::from(values[2]);
        if !(0.3..=20.0).contains(&q) {
            return Err(Failure::usage("notch Q must be 0.3..20"));
        }
        let alpha = s / (2.0 * q);
        (
            [1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a],
            [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a],
        )
    } else {
        let t = s * 2.0_f64.sqrt() * a.sqrt(); // shelf slope S=1
        if name == "high-shelf" {
            (
                [
                    a * ((a + 1.0) + (a - 1.0) * c + t),
                    -2.0 * a * ((a - 1.0) + (a + 1.0) * c),
                    a * ((a + 1.0) + (a - 1.0) * c - t),
                ],
                [
                    (a + 1.0) - (a - 1.0) * c + t,
                    2.0 * ((a - 1.0) - (a + 1.0) * c),
                    (a + 1.0) - (a - 1.0) * c - t,
                ],
            )
        } else {
            (
                [
                    a * ((a + 1.0) - (a - 1.0) * c + t),
                    2.0 * a * ((a - 1.0) - (a + 1.0) * c),
                    a * ((a + 1.0) - (a - 1.0) * c - t),
                ],
                [
                    (a + 1.0) + (a - 1.0) * c + t,
                    -2.0 * ((a - 1.0) + (a + 1.0) * c),
                    (a + 1.0) + (a - 1.0) * c - t,
                ],
            )
        }
    };
    Ok(Some(Biquad::new(
        b.map(|b| b / d[0]),
        [d[1] / d[0], d[2] / d[0]],
    )))
}

fn gain(pcm: &mut Pcm, opts: &Opts) -> Result<Value, Failure> {
    let target = number(opts, "target-lufs", -70.0, 0.0)?;
    let explicit = number(opts, "gain-db", -60.0, 30.0)?.unwrap_or(0.0);
    if target.is_some() && opts.text("gain-db").is_some() {
        return Err(Failure::usage("choose --target-lufs or --gain-db"));
    }
    let mut requested = explicit;
    if let Some(target) = target {
        let current = meter::levels(pcm)["integrated_lufs"]
            .as_f64()
            .ok_or_else(|| {
                Failure::bad_input("no integrated loudness: target needs 400 ms above the gate")
            })?;
        requested = target - current;
    }
    let peak_limit = number(opts, "peak", -30.0, 0.0)?.unwrap_or(-1.0);
    let ceiling = meter::db(meter::true_peak(pcm).powi(2))
        .map(|p| peak_limit - p)
        .unwrap_or(requested);
    let applied = requested.min(ceiling);
    let factor = 10_f64.powf(applied / 20.0) as f32;
    for x in pcm.frames.iter_mut().flatten() {
        *x *= factor;
    }
    Ok(
        json!({"requested_gain_db":requested,"applied_gain_db":applied,"target_lufs":target,"peak_ceiling_dbtp":peak_limit,
        "peak_limited":applied < requested-0.001,"detail":"Gain only, no limiter or compression. If peak-limited, the loudness target may be unreachable; the after measurement is authoritative."}),
    )
}

pub(super) fn run(source: &Path, opts: &Opts, looping: bool) -> Result<Value, Failure> {
    let out = opts
        .text("out")
        .filter(|s| *s != "true")
        .ok_or_else(|| Failure::usage("needs --out <new.wav>"))?;
    if Path::new(out).exists() {
        return Err(Failure::bad_input(
            "output already exists; choose a new file",
        ));
    }
    let mut pcm = super::read(source)?;
    let before = inspect::inspect(&pcm, None);
    let mut edits = vec![];
    if looping {
        let blend = number(opts, "blend", 0.005, 10.0)?.unwrap_or(1.0);
        let search = number(opts, "search", 0.0, 10.0)?.unwrap_or(1.0);
        let curve = opts.text("curve").unwrap_or("linear");
        if !["linear", "equal-power"].contains(&curve) {
            return Err(Failure::usage("--curve is linear or equal-power"));
        }
        edits.push(make_loop(&mut pcm, blend, search, curve)?);
    } else {
        if let Some(trim) = opts.numbers("trim").map_err(Failure::usage)? {
            if trim.len() != 2
                || trim[0] < 0.0
                || trim[1] <= trim[0]
                || f64::from(trim[1]) > inspect::duration(&pcm)
            {
                return Err(Failure::usage(
                    "--trim takes start,end seconds within the clip, end > start",
                ));
            }
            let start = (f64::from(trim[0]) * f64::from(pcm.rate)).round() as usize;
            let end = (f64::from(trim[1]) * f64::from(pcm.rate)).round() as usize;
            if end <= start {
                return Err(Failure::usage("trim must retain at least one sample"));
            }
            pcm.frames = pcm.frames[start..end].to_vec();
            edits.push(json!({"trim_s":trim}));
        }
        if flag(opts, "trim-silence")? {
            let (head, tail) = inspect::silence(&pcm);
            let margin = pcm.rate as usize / 100;
            let start = head.saturating_sub(margin);
            let end = pcm.frames.len() - tail.saturating_sub(margin);
            if end <= start {
                return Err(Failure::bad_input(
                    "trim-silence would remove the entire clip",
                ));
            }
            pcm.frames = pcm.frames[start..end].to_vec();
            edits.push(json!({"trim_silence_frames":[start,end],"margin_s":0.01}));
        }
        if flag(opts, "declick")? {
            let clicks = inspect::clicks(&pcm);
            for &i in &clicks {
                for c in 0..2 {
                    pcm.frames[i][c] = (pcm.frames[i - 1][c] + pcm.frames[i + 1][c]) * 0.5;
                }
            }
            edits.push(json!({"interpolated_clicks":clicks.len()}));
        }
        if flag(opts, "remove-dc")? {
            let mean = [0, 1].map(|c| {
                pcm.frames.iter().map(|f| f64::from(f[c])).sum::<f64>() / pcm.frames.len() as f64
            });
            for frame in &mut pcm.frames {
                for c in 0..2 {
                    frame[c] -= mean[c] as f32;
                }
            }
            edits.push(json!({"removed_dc":mean}));
        }
        for name in ["low-shelf", "high-shelf", "notch"] {
            if let Some(filter) = eq(opts, name, pcm.rate)? {
                let mut filters = [filter; 2];
                for frame in &mut pcm.frames {
                    for c in 0..2 {
                        frame[c] = filters[c].next(f64::from(frame[c])) as f32;
                    }
                }
                edits.push(json!({"eq":name,"parameters":opts.text(name)}));
            }
        }
        let length = pcm.frames.len();
        for (name, reverse) in [("fade-in", false), ("fade-out", true)] {
            if let Some(seconds) = number(opts, name, 0.0, inspect::duration(&pcm))? {
                let n = ((seconds * f64::from(pcm.rate)).round() as usize).min(length);
                for i in 0..n {
                    let t = if n > 1 {
                        i as f32 / (n - 1) as f32
                    } else {
                        0.0
                    };
                    let index = if reverse { length - 1 - i } else { i };
                    for s in &mut pcm.frames[index] {
                        *s *= t;
                    }
                }
                edits.push(json!({"fade":name,"seconds":seconds}));
            }
        }
    }
    let gain = gain(&mut pcm, opts)?;
    write(Path::new(out), &pcm)?;
    // Analyze the exported/decoded data, not just our working buffer.
    let decoded = super::read(Path::new(out))?;
    let after = inspect::inspect(&decoded, number(opts, "target-lufs", -70.0, 0.0)?);
    let boundaries = if looping {
        let mut repeated = decoded.clone();
        repeated.frames.extend_from_slice(&decoded.frames);
        repeated.frames.extend_from_slice(&decoded.frames);
        let at = edits[0]["blend_boundary_frame"]
            .as_u64()
            .expect("loop edit frame") as usize;
        let mut checks = Vec::new();
        for repetition in 0..3 {
            let mut check = boundary(&repeated, repetition * decoded.frames.len() + at);
            check["kind"] = json!("blend_start");
            checks.push(check);
            if repetition > 0 {
                let mut check = boundary(&repeated, repetition * decoded.frames.len());
                check["kind"] = json!("wrap");
                checks.push(check);
            }
        }
        Some(
            json!({"repetitions":3,"wraps_s":[inspect::duration(&decoded),2.0*inspect::duration(&decoded)],
            "clicks":inspect::clicks(&repeated).iter().map(|&i|i as f64/f64::from(pcm.rate)).collect::<Vec<_>>(),
            "seam":inspect::seam(&decoded),"blend_boundary":boundary(&decoded,at),"boundaries":checks,
            "detail":"Checks each wrap and the edit boundary. Numeric continuity does not certify harmony or rhythm; audition three repetitions."}),
        )
    } else {
        None
    };
    Ok(
        json!({"measurement_version":1,"reading":"Wrote a new WAV; source untouched. Compare before and after and audition the edit.",
        "output":out,"before":before,"after":after,"edits":edits,"gain":gain,"loop_validation":boundaries,
        "findings":after["findings"],"detail":"Order: trim, silence trim, isolated click interpolation, DC mean removal, EQ, fades, peak-constrained gain. Loop does endpoint search and overlap before gain."}),
    )
}

fn make_loop(pcm: &mut Pcm, seconds: f64, search: f64, curve: &str) -> Result<Value, Failure> {
    let overlap = (seconds * f64::from(pcm.rate)).round() as usize;
    let n = pcm.frames.len();
    if overlap < 2 || overlap * 4 > n {
        return Err(Failure::usage(
            "loop needs at least four times --blend seconds",
        ));
    }
    let max_search = ((search * f64::from(pcm.rate)).round() as usize).min((n - 4 * overlap) / 2);
    // Search pairs of endpoints on a 20 ms grid, score normalized overlap correlation
    // plus energy mismatch. No beat/harmony inference is claimed for this search.
    let stride = (pcm.rate as usize / 50).max(1);
    let candidates: Vec<_> = (0..=max_search).step_by(stride).collect();
    let mut best = (f64::INFINITY, 0, n);
    for &start in &candidates {
        for &cut in &candidates {
            let end = n - cut;
            let (mut cross, mut a, mut b) = (0.0, 0.0, 0.0);
            for i in (0..overlap).step_by((overlap / 512).max(1)) {
                for c in 0..pcm.channels as usize {
                    let x = f64::from(pcm.frames[start + i][c]);
                    let y = f64::from(pcm.frames[end - overlap + i][c]);
                    cross += x * y;
                    a += x * x;
                    b += y * y;
                }
            }
            let corr = if a * b > 1e-20 {
                cross / (a * b).sqrt()
            } else {
                0.0
            };
            let level = meter::db((a + 1e-20) / (b + 1e-20)).unwrap_or(0.0).abs();
            let score = 1.0 - corr + level / 20.0;
            if score < best.0 {
                best = (score, start, end);
            }
        }
    }
    let (_, start, end) = best;
    // Refine both endpoints by up to 1 ms, choosing matched samples and slopes.
    let radius = (pcm.rate as usize / 1000).max(1);
    let mut refined = (f64::INFINITY, start, end);
    for a in start.saturating_sub(radius)..=(start + radius).min(max_search) {
        for b in end.saturating_sub(radius).max(n - max_search)..=(end + radius).min(n) {
            let jump: f64 = (0..pcm.channels as usize)
                .map(|c| {
                    let head = pcm.frames[a][c];
                    // Match the samples that will actually overlap. Matching the
                    // old hard-wrap endpoints instead can reverse the overlap's
                    // phase (e.g. a blend containing half an extra tone cycle).
                    let tail = pcm.frames[b - overlap][c];
                    let slope =
                        (pcm.frames[a + 1][c] - head) - (pcm.frames[b - overlap + 1][c] - tail);
                    f64::from((head - tail).powi(2) + slope.powi(2))
                })
                .sum();
            if jump < refined.0 {
                refined = (jump, a, b);
            }
        }
    }
    let (_, start, end) = refined;
    let mut frames = pcm.frames[start + overlap..end - overlap].to_vec();
    let boundary_frame = frames.len();
    for i in 0..overlap {
        let t = i as f64 / (overlap - 1) as f64;
        let (a, b) = if curve == "equal-power" {
            (
                (t * std::f64::consts::FRAC_PI_2).cos(),
                (t * std::f64::consts::FRAC_PI_2).sin(),
            )
        } else {
            (1.0 - t, t)
        };
        frames.push([0, 1].map(|c| {
            (f64::from(pcm.frames[end - overlap + i][c]) * a
                + f64::from(pcm.frames[start + i][c]) * b) as f32
        }));
    }
    pcm.frames = frames;
    Ok(
        json!({"source_start_s":start as f64/f64::from(pcm.rate),"source_end_s":end as f64/f64::from(pcm.rate),
        "overlap_s":seconds,"curve":curve,"match_score":best.0,"duration_s":inspect::duration(pcm),
        "blend_boundary_frame":boundary_frame,"blend_boundary":boundary(pcm,boundary_frame),
        "detail":"Waveform/level candidate search and sample/slope refinement; no reliable beat or harmonic ranking. The overlap shortens and rotates the clip; set audio.toml loop_crossfade=0 for this prepared loop."}),
    )
}

fn boundary(pcm: &Pcm, at: usize) -> Value {
    let f = &pcm.frames;
    let w = (pcm.rate as usize / 10).min(at).min(f.len() - at);
    let left = meter::db(inspect::energy(&f[at - w..at]));
    let right = meter::db(inspect::energy(&f[at..at + w]));
    let max = |values: [f32; 2]| values.into_iter().map(f32::abs).fold(0.0_f32, f32::max);
    json!({"at_s":at as f64/f64::from(pcm.rate),"level_jump_db":left.zip(right).map(|(l,r)|(l-r).abs()),
        "sample_jump":max([0,1].map(|c| f[at][c]-f[at-1][c])),
        "slope_jump":max([0,1].map(|c| (f[at+1][c]-f[at][c])-(f[at-1][c]-f[at-2][c]))),
        "undefined_reason":left.zip(right).is_none().then_some("one boundary window is silent")})
}
