use super::meter::{db, levels};
use galaxy_audio::Pcm;
use rustfft::{FftPlanner, num_complex::Complex};
use serde_json::{Value, json};

pub(super) fn finding(kind: &str, start: f64, end: f64, value: Value, action: &str) -> Value {
    json!({"kind": kind, "start_s": start, "end_s": end, "value": value, "suggested_action": action})
}

pub(super) fn duration(pcm: &Pcm) -> f64 {
    pcm.frames.len() as f64 / f64::from(pcm.rate)
}
pub(super) fn energy(frames: &[[f32; 2]]) -> f64 {
    frames
        .iter()
        .flatten()
        .map(|&x| f64::from(x).powi(2))
        .sum::<f64>()
        / (2 * frames.len()).max(1) as f64
}

pub(super) fn seam(pcm: &Pcm) -> Value {
    let f = &pcm.frames;
    let n = f.len();
    let w = (pcm.rate as usize / 10).max(1).min(n / 2);
    let head = db(energy(&f[..w]));
    let tail = db(energy(&f[n - w..]));
    let sample = (0..pcm.channels as usize)
        .map(|c| f64::from((f[0][c] - f[n - 1][c]).abs()))
        .fold(0.0, f64::max);
    let slope = (n >= 2).then(|| {
        (0..pcm.channels as usize)
            .map(|c| f64::from(((f[1][c] - f[0][c]) - (f[n - 1][c] - f[n - 2][c])).abs()))
            .fold(0.0, f64::max)
    });
    json!({"window_s": w as f64 / f64::from(pcm.rate), "head_rms_dbfs": head, "tail_rms_dbfs": tail,
        "level_jump_db": head.zip(tail).map(|(h,t)| (h-t).abs()),
        "sample_jump": sample, "slope_jump": slope,
        "undefined_reason": head.zip(tail).is_none().then_some("one endpoint window is silent or too short; compare sample jump and silence findings")})
}

/// Isolated impulse residual, compared with nearby first differences. Returns sample
/// indices so the repair uses exactly the same detection, never smooths ordinary attacks.
pub(super) fn clicks(pcm: &Pcm) -> Vec<usize> {
    let mut out = vec![];
    let radius = (pcm.rate as usize / 1000).max(4);
    for i in radius..pcm.frames.len().saturating_sub(radius) {
        if out.last().is_some_and(|&last| i < last + radius) {
            continue;
        }
        for c in 0..pcm.channels as usize {
            let f = &pcm.frames;
            let residual = (f[i][c] - (f[i - 1][c] + f[i + 1][c]) * 0.5).abs();
            if residual < 0.08 {
                continue;
            }
            let local: f32 = ((i - radius + 1)..(i + radius))
                .filter(|&j| j < i - 1 || j > i + 2)
                .map(|j| (f[j][c] - f[j - 1][c]).powi(2))
                .sum();
            let baseline = (local / (2 * radius - 5) as f32).sqrt();
            let returns = (f[i - 1][c] - f[i + 1][c]).abs() < residual * 0.5;
            if returns && residual > baseline.max(0.001) * 8.0 {
                out.push(i);
                break;
            }
        }
    }
    out
}

pub(super) fn silence(pcm: &Pcm) -> (usize, usize) {
    let block = (pcm.rate as usize / 100).max(1);
    let head = pcm
        .frames
        .chunks(block)
        .take_while(|f| energy(f) < 1e-6)
        .map(<[_]>::len)
        .sum();
    let tail = pcm
        .frames
        .rchunks(block)
        .take_while(|f| energy(f) < 1e-6)
        .map(<[_]>::len)
        .sum();
    (head, tail)
}

pub(super) fn spectrum(pcm: &Pcm) -> Value {
    let n = 2048usize;
    let step = (pcm.rate as usize / 10).max(1);
    let fft = FftPlanner::<f64>::new().plan_fft_forward(n);
    let mut bins = vec![0.0; n / 2 + 1];
    let mut timeline = vec![];
    let mut buf = vec![Complex::new(0.0, 0.0); n];
    for start in (0..pcm.frames.len()).step_by(step) {
        let count = n.min(pcm.frames.len() - start);
        if count < n / 2 {
            break;
        }
        let mut power = vec![0.0; bins.len()];
        for c in 0..pcm.channels as usize {
            for (j, x) in buf.iter_mut().enumerate() {
                let window = 0.5 - 0.5 * (std::f64::consts::TAU * j as f64 / (n - 1) as f64).cos();
                *x = Complex::new(
                    pcm.frames.get(start + j).map_or(0.0, |f| f64::from(f[c])) * window,
                    0.0,
                );
            }
            fft.process(&mut buf);
            for (p, b) in power.iter_mut().zip(&buf) {
                *p += b.norm_sqr();
            }
        }
        for (sum, p) in bins.iter_mut().zip(&power) {
            *sum += p;
        }
        let mut features = spectral_features(&power, pcm.rate, n);
        features["start_s"] = json!(start as f64 / f64::from(pcm.rate));
        features["end_s"] = json!((start + count) as f64 / f64::from(pcm.rate));
        timeline.push(features);
    }
    let mut report = spectral_features(&bins, pcm.rate, n);
    let total: f64 = bins.iter().sum();
    let mut resonances = vec![];
    for i in 5..bins.len() - 5 {
        if bins[i] < bins[i - 1] || bins[i] < bins[i + 1] || bins[i] < total * 0.01 {
            continue;
        }
        let neighbors =
            (bins[i - 5..i - 2].iter().sum::<f64>() + bins[i + 3..i + 6].iter().sum::<f64>()) / 6.0;
        let prominence = db(bins[i] / neighbors.max(1e-20)).unwrap_or(0.0);
        if prominence > 12.0 {
            resonances.push(json!({"hz": i as f64 * f64::from(pcm.rate)/n as f64,
                "prominence_db": prominence, "energy_fraction": bins[i]/total,
                "start_s": 0.0, "end_s": duration(pcm),
                "suggested_action": "A tonal partial is not automatically harsh. Compare a loudness-matched reference; if unwanted, process --notch hz,-3,4 or prompt 'muted timbres, rounded attacks'."}));
        }
    }
    report["narrow_resonances"] = json!(resonances);
    report["timeline"] = json!(timeline);
    report["undefined_reason"] =
        json!((total == 0.0).then_some("silence or fewer than 1024 samples"));
    report
}

fn spectral_features(power: &[f64], rate: u32, n: usize) -> Value {
    let hz = |i: usize| i as f64 * f64::from(rate) / n as f64;
    let total: f64 = power.iter().sum();
    let band = |low: f64, high: f64| -> Option<f64> {
        (total > 0.0 && f64::from(rate) / 2.0 > low).then(|| {
            power
                .iter()
                .enumerate()
                .filter(|(i, _)| hz(*i) >= low && hz(*i) < high)
                .map(|(_, p)| p)
                .sum::<f64>()
                / total
        })
    };
    let mut cumulative = 0.0;
    let rolloff = power.iter().enumerate().find_map(|(i, p)| {
        cumulative += p;
        (total > 0.0 && cumulative >= total * 0.95).then(|| hz(i))
    });
    json!({"centroid_hz": (total > 0.0).then(|| power.iter().enumerate().map(|(i,p)| hz(i)*p).sum::<f64>()/total),
        "rolloff_95_hz": rolloff, "low_mid_fraction": band(200.0,500.0),
        "presence_fraction": band(2000.0,5000.0), "air_fraction": band(8000.0,f64::INFINITY),
        "air_undefined_reason": (rate <= 16000).then_some("Nyquist frequency is at or below 8 kHz")})
}

pub(super) fn inspect(pcm: &Pcm, target: Option<f64>) -> Value {
    let seconds = duration(pcm);
    let loudness = levels(pcm);
    let spec = spectrum(pcm);
    let seam = seam(pcm);
    let mut findings = vec![];
    if loudness["true_peak_dbtp"].as_f64().is_some_and(|p| p > 0.0) {
        findings.push(finding("true_peak_overshoot",0.0,seconds,
            loudness["true_peak_dbtp"].clone(),
            "process --peak -1 lowers gain before playback; samples below full scale can still have intersample overshoots."));
    }
    if let Some(target) = target
        && let Some(l) = loudness["integrated_lufs"].as_f64()
        && l > target + 1.0
    {
        findings.push(finding("above_target",0.0,seconds,json!({"lufs":l,"target_lufs":target}),
            "process --target-lufs <target>; use audio mix to choose the source target from playback gain"));
    }
    let mut clipped = 0;
    let mut first = None;
    for (i, f) in pcm.frames.iter().enumerate() {
        let flat = i > 0
            && i + 1 < pcm.frames.len()
            && (0..pcm.channels as usize).any(|c| {
                f[c].abs() > 0.5
                    && (f[c] - pcm.frames[i - 1][c]).abs() < 1e-7
                    && (f[c] - pcm.frames[i + 1][c]).abs() < 1e-7
            });
        let bad = f.iter().any(|v| v.abs() >= 0.9999) || flat;
        if bad {
            clipped += 1;
            first.get_or_insert(i);
        }
        if !bad && let Some(start) = first.take() {
            findings.push(finding("clipping",start as f64/f64::from(pcm.rate),i as f64/f64::from(pcm.rate),json!({"frames":i-start}),
                "For overshoots process --gain-db -3; flat tops may already be damaged: regenerate with 'clean, undistorted, restrained dynamics'."));
        }
    }
    if let Some(start) = first {
        findings.push(finding("clipping",start as f64/f64::from(pcm.rate),seconds,json!({"frames":pcm.frames.len()-start}),
            "process --gain-db -3 prevents further clipping; regenerate flat tops with 'clean, undistorted, restrained dynamics'."));
    }
    let impulses = clicks(pcm);
    for &i in &impulses {
        findings.push(finding("click",i as f64/f64::from(pcm.rate),(i+1) as f64/f64::from(pcm.rate),json!(pcm.frames[i]),
        "process --declick interpolates isolated impulses; for transitions use tiny fades or prompt 'smooth sustained transitions'."));
    }
    let (head, tail) = silence(pcm);
    for (kind, start, end) in [
        ("head_silence", 0, head),
        ("tail_silence", pcm.frames.len() - tail, pcm.frames.len()),
    ] {
        if end - start > pcm.rate as usize / 20 {
            findings.push(finding(kind,start as f64/f64::from(pcm.rate),end as f64/f64::from(pcm.rate),json!({"threshold_dbfs":-60}),
            "process --trim-silence keeps a 10 ms margin; or prompt 'immediate continuous texture, no silent intro/outro'."));
        }
    }
    let mut stereo = vec![];
    let mut dc_max = 0.0_f64;
    for (i, chunk) in pcm.frames.chunks(pcm.rate as usize).enumerate() {
        let n = chunk.len() as f64;
        let dc = [0, 1].map(|c| chunk.iter().map(|f| f64::from(f[c])).sum::<f64>() / n);
        let powers = [0, 1].map(|c| chunk.iter().map(|f| f64::from(f[c]).powi(2)).sum::<f64>() / n);
        let cross = chunk
            .iter()
            .map(|f| f64::from(f[0]) * f64::from(f[1]))
            .sum::<f64>()
            / n;
        let corr = (powers[0] * powers[1] > 0.0).then(|| cross / (powers[0] * powers[1]).sqrt());
        let imbalance = db(powers[0]).zip(db(powers[1])).map(|(l, r)| l - r);
        let mono = (powers[0] + powers[1] + 2.0 * cross).max(0.0) / 4.0;
        let loss = db((powers[0] + powers[1]) / 2.0)
            .zip(db(mono))
            .map(|(s, m)| s - m);
        let start = i as f64;
        let end = (start + 1.0).min(seconds);
        dc_max = dc_max.max(dc.into_iter().map(f64::abs).fold(0.0, f64::max));
        if dc.iter().any(|v| v.abs() > 0.01) {
            findings.push(finding(
                "dc_offset",
                start,
                end,
                json!(dc),
                "process --remove-dc",
            ));
        }
        if pcm.channels == 2 {
            let silent_channel = (powers[0] < 1e-10) != (powers[1] < 1e-10);
            if silent_channel
                || corr.is_some_and(|c| c < -0.25)
                || imbalance.is_some_and(|d| d.abs() > 6.0)
                || loss.is_some_and(|d| d > 6.0)
            {
                findings.push(finding("stereo",start,end,json!({"correlation":corr,"imbalance_db":imbalance,"mono_loss_db":loss,"silent_channel":silent_channel}),
                    "Regenerate with 'stable centred bass, mono-compatible stereo'; severe cancellation cannot be restored by gain or EQ."));
            }
            stereo.push(json!({"start_s":start,"end_s":end,"correlation":corr,"imbalance_db":imbalance,"mono_loss_db":loss,
                "undefined_reason": (loss.is_none() || corr.is_none() || imbalance.is_none()).then_some("a channel or the mono fold-down is silent")}));
        }
    }
    if seam["level_jump_db"].as_f64().is_some_and(|v| v > 3.0)
        || seam["sample_jump"].as_f64().is_some_and(|v| v > 0.05)
        || seam["slope_jump"].as_f64().is_some_and(|v| v > 0.05)
    {
        findings.push(finding("loop_seam",seconds,seconds,seam.clone(),
            "audio loop --blend 1 --search 1; prompt 'continuous repeating phrase, no final cadence, no intro/outro'; inspect and listen across the blend as well as the wrap."));
    }
    for window in spec["timeline"].as_array().into_iter().flatten() {
        let action = if window["air_fraction"].as_f64().is_some_and(|v| v > 0.25)
            || window["presence_fraction"]
                .as_f64()
                .is_some_and(|v| v > 0.65)
        {
            Some((
                "bright_band",
                "Compare a loudness-matched reference; if too sharp, process --high-shelf 4000,-3 or prompt 'rounded attacks, muted timbres, soft upper register'.",
            ))
        } else if window["low_mid_fraction"].as_f64().is_some_and(|v| v > 0.7) {
            Some((
                "low_mid_dominance",
                "Compare a reference; try process --low-shelf 400,-3 or prompt 'sparse voicing, clear low mids, restrained bass'.",
            ))
        } else {
            None
        };
        if let Some((kind, action)) = action {
            findings.push(finding(
                kind,
                window["start_s"].as_f64().unwrap(),
                window["end_s"].as_f64().unwrap(),
                window.clone(),
                action,
            ));
        }
    }
    let count = findings.len();
    json!({"measurement_version":1,"reading":format!("{seconds:.2} s; {count} findings. Spectral flags are clues, not a universal harshness verdict."),
        "duration_s":seconds,"sample_rate":pcm.rate,"channels":pcm.channels,"loudness":loudness,
        "spectrum":spec,"seam":seam,"stereo":stereo,
        "stereo_undefined_reason": (pcm.channels == 1).then_some("mono source"),
        "defects":{"clipped_frames":clipped,"clicks":impulses.len(),"dc_max":dc_max,
            "head_silence_s": head as f64/f64::from(pcm.rate),"tail_silence_s":tail as f64/f64::from(pcm.rate)},
        "findings":findings,"detail":"Source channels are metered before playback's clamp. M=400 ms, S=3 s, 100 ms hop, -70/-10 LU integrated gates; LRA -70/-20 LU gates, 10th to 95th percentile. True peak is 4x sinc estimated. LUFS is not physical listening level; choose a target in the real mix."})
}
