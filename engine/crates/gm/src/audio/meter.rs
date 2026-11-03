//! Authoring measurements. BS.1770 K weighting and gates are ours; FFT is plumbing.
use galaxy_audio::Pcm;
use serde_json::{Value, json};

#[derive(Clone, Copy)]
pub(super) struct Biquad {
    pub b: [f64; 3],
    pub a: [f64; 2],
    z: [f64; 2],
}
impl Biquad {
    pub fn new(b: [f64; 3], a: [f64; 2]) -> Self {
        Self { b, a, z: [0.0; 2] }
    }
    pub fn next(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

fn weighting(rate: u32) -> [Biquad; 2] {
    // Bilinear transform of the BS.1770 shelf and RLB high pass. At 48 kHz
    // these reproduce the published coefficients (tested below).
    let k = (std::f64::consts::PI * 1681.974450955533 / f64::from(rate)).tan();
    let q = 0.7071752369554196;
    let vh = 10_f64.powf(3.999843853973347 / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let d = 1.0 + k / q + k * k;
    let shelf = Biquad::new(
        [
            (vh + vb * k / q + k * k) / d,
            2.0 * (k * k - vh) / d,
            (vh - vb * k / q + k * k) / d,
        ],
        [2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d],
    );
    let k = (std::f64::consts::PI * 38.13547087602444 / f64::from(rate)).tan();
    let q = 0.5003270373238773;
    let d = 1.0 + k / q + k * k;
    let high = Biquad::new(
        [1.0, -2.0, 1.0],
        [2.0 * (k * k - 1.0) / d, (1.0 - k / q + k * k) / d],
    );
    [shelf, high]
}

pub(super) fn db(power: f64) -> Option<f64> {
    (power > 0.0 && power.is_finite()).then(|| 10.0 * power.log10())
}
fn lufs(power: f64) -> Option<f64> {
    db(power).map(|d| d - 0.691)
}
fn mean(v: &[f64]) -> f64 {
    v.iter().sum::<f64>() / v.len().max(1) as f64
}

/// Streaming 100 ms energies; a render can run for many minutes without retaining PCM.
pub(super) struct Meter {
    rate: u32,
    channels: usize,
    filters: [[Biquad; 2]; 2],
    sum: f64,
    n: usize,
    total: usize,
    energies: Vec<f64>,
    squares: f64,
    peak: f64,
}
impl Meter {
    pub fn new(rate: u32, channels: u16) -> Self {
        Self {
            rate,
            channels: channels as usize,
            filters: [weighting(rate); 2],
            sum: 0.0,
            n: 0,
            total: 0,
            energies: vec![],
            squares: 0.0,
            peak: 0.0,
        }
    }
    pub fn push(&mut self, frame: [f32; 2]) {
        for (c, x) in frame.into_iter().enumerate().take(self.channels) {
            let x = f64::from(x);
            self.squares += x * x;
            self.peak = self.peak.max(x.abs());
            let y = self.filters[c][0].next(x);
            let y = self.filters[c][1].next(y);
            self.sum += y * y;
        }
        self.n += 1;
        self.total += 1;
        // Boundaries use cumulative sample positions, including rates not divisible by 10.
        if self.total >= ((self.energies.len() + 1) as u64 * u64::from(self.rate) / 10) as usize {
            self.energies.push(self.sum / self.n as f64);
            self.sum = 0.0;
            self.n = 0;
        }
    }
    pub fn report(&self) -> Value {
        let blocks = |n: usize| -> Vec<f64> { self.energies.windows(n).map(mean).collect() };
        let momentary = blocks(4);
        let short = blocks(30);
        let absolute: Vec<_> = momentary
            .iter()
            .copied()
            .filter(|&e| lufs(e).is_some_and(|l| l >= -70.0))
            .collect();
        let relative = mean(&absolute) * 0.1;
        let gated: Vec<_> = absolute.into_iter().filter(|&e| e >= relative).collect();
        let integrated = (!gated.is_empty()).then(|| lufs(mean(&gated))).flatten();
        let abs_short: Vec<_> = short
            .iter()
            .copied()
            .filter(|&e| lufs(e).is_some_and(|l| l >= -70.0))
            .collect();
        let gate = mean(&abs_short) * 0.01;
        let mut distribution: Vec<_> = abs_short
            .into_iter()
            .filter(|&e| e >= gate)
            .filter_map(lufs)
            .collect();
        distribution.sort_by(f64::total_cmp);
        let percentile = |p: f64| {
            distribution[((distribution.len() as f64 * p).ceil() as usize).saturating_sub(1)]
        };
        let range = (!distribution.is_empty()).then(|| percentile(0.95) - percentile(0.10));
        let series = |v: &[f64], window: f64| -> Vec<Value> {
            v.iter().enumerate().map(|(i, &e)|
            json!({"start_s": i as f64 / 10.0, "end_s": i as f64 / 10.0 + window, "lufs": lufs(e)})).collect()
        };
        let rms = self.squares / (self.total * self.channels).max(1) as f64;
        json!({
            "integrated_lufs": integrated,
            "momentary_max_lufs": momentary.iter().copied().filter_map(lufs).reduce(f64::max),
            "short_term_max_lufs": short.iter().copied().filter_map(lufs).reduce(f64::max),
            "loudness_range_lu": range,
            "rms_dbfs": db(rms), "sample_peak_dbfs": db(self.peak * self.peak),
            "crest_db": db(self.peak * self.peak).zip(db(rms)).map(|(p,r)| p-r),
            "momentary": series(&momentary, 0.4), "short_term": series(&short, 3.0),
            "undefined_reason": {
                "integrated_lufs": integrated.is_none().then_some("needs a full 400 ms block above the -70 LUFS gate"),
                "momentary_max_lufs": momentary.is_empty().then_some("needs 400 ms; silence is null"),
                "short_term_max_lufs": short.is_empty().then_some("needs 3 s; silence is null"),
                "loudness_range_lu": range.is_none().then_some("needs 3 s above the gate; short clips give an unstable range"),
                "db_values": (rms == 0.0).then_some("digital silence has no finite dB level")
            }
        })
    }
}

pub(super) fn levels(pcm: &Pcm) -> Value {
    let mut meter = Meter::new(pcm.rate, pcm.channels);
    for &frame in &pcm.frames {
        meter.push(frame);
    }
    let mut report = meter.report();
    report["true_peak_dbtp"] = json!(db(true_peak(pcm).powi(2)));
    report
}

/// Four-phase windowed sinc interpolation, 49 taps per phase. Include original
/// samples and zero-padded edges. An estimate, not a certified true-peak meter.
pub(super) fn true_peak(pcm: &Pcm) -> f64 {
    const R: isize = 24;
    let mut kernels = [[0.0; 49]; 3];
    for (phase, kernel) in kernels.iter_mut().enumerate() {
        for (j, w) in kernel.iter_mut().enumerate() {
            let x = (j as isize - R) as f64 - (phase + 1) as f64 / 4.0;
            let sinc = (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x);
            *w = sinc * (0.5 + 0.5 * (std::f64::consts::PI * x / 25.0).cos());
        }
        let sum: f64 = kernel.iter().sum();
        for w in kernel {
            *w /= sum;
        }
    }
    let mut peak = pcm
        .frames
        .iter()
        .flatten()
        .fold(0.0_f64, |p, &v| p.max(f64::from(v).abs()));
    for i in 0..pcm.frames.len() {
        for kernel in &kernels {
            let mut values = [0.0; 2];
            for (j, w) in kernel.iter().enumerate() {
                let index = i as isize + j as isize - R;
                if index >= 0
                    && let Some(frame) = pcm.frames.get(index as usize)
                {
                    for c in 0..pcm.channels as usize {
                        values[c] += f64::from(frame[c]) * w;
                    }
                }
            }
            for v in values {
                peak = peak.max(v.abs());
            }
        }
    }
    peak
}

#[cfg(test)]
mod tests {
    use super::*;
    pub fn tone(rate: u32, seconds: f64, hz: f64, amplitude: f64, channels: u16) -> Pcm {
        Pcm {
            rate,
            channels,
            frames: (0..(f64::from(rate) * seconds) as usize)
                .map(|i| {
                    let v = (amplitude
                        * (std::f64::consts::TAU * hz * i as f64 / f64::from(rate)).sin())
                        as f32;
                    [v, v]
                })
                .collect(),
        }
    }
    #[test]
    fn itu_coefficients_and_ebu_reference_sine() {
        let [shelf, high] = weighting(48_000);
        for (got, expected) in
            shelf
                .b
                .into_iter()
                .zip([1.53512485958697, -2.69169618940638, 1.19839281085285])
        {
            assert!((got - expected).abs() < 1e-10);
        }
        assert!((high.a[0] + 1.99004745483398).abs() < 1e-10);
        // EBU-style stereo 1 kHz sine at -23 dBFS amplitude: -23 LUFS +/-0.1.
        for rate in [44_100, 48_000, 96_000] {
            let pcm = tone(rate, 4.0, 1000.0, 10_f64.powf(-23.0 / 20.0), 2);
            let l = levels(&pcm);
            assert!(
                (l["integrated_lufs"].as_f64().unwrap() + 23.0).abs() < 0.1,
                "{l}"
            );
            assert!(l["loudness_range_lu"].as_f64().unwrap() < 0.01);
            let mut mono = pcm.clone();
            mono.channels = 1;
            assert!((levels(&mono)["integrated_lufs"].as_f64().unwrap() + 26.01).abs() < 0.1);
        }
    }
    #[test]
    fn relative_gate_silence_range_and_intersample_peak() {
        let mut pcm = tone(48_000, 10.0, 1000.0, 0.1, 2);
        let quiet = tone(48_000, 10.0, 1000.0, 0.001, 2);
        pcm.frames.extend(quiet.frames);
        let mut meter = Meter::new(pcm.rate, 2);
        for &f in &pcm.frames {
            meter.push(f);
        }
        assert!((meter.report()["integrated_lufs"].as_f64().unwrap() + 20.0).abs() < 0.2);
        let silence = Pcm {
            rate: 48_000,
            channels: 2,
            frames: vec![[0.0; 2]; 48_000],
        };
        assert!(levels(&silence)["integrated_lufs"].is_null());
        let mut inter = tone(48_000, 0.1, 12_000.0, 1.0, 2);
        for (i, f) in inter.frames.iter_mut().enumerate() {
            *f = [(std::f64::consts::TAU * i as f64 / 4.0 + std::f64::consts::FRAC_PI_4).sin()
                as f32; 2];
        }
        assert!(inter.frames.iter().all(|f| f[0].abs() < 0.71));
        assert!(true_peak(&inter) > 0.98);
        // EBU LRA test: two steady sections ten LU apart, each much longer than 3 s.
        let mut range = Meter::new(48_000, 2);
        for a in [0.1, 0.316227766] {
            for f in tone(48_000, 10.0, 1000.0, a, 2).frames {
                range.push(f);
            }
        }
        assert!((range.report()["loudness_range_lu"].as_f64().unwrap() - 10.0).abs() < 0.1);
    }
}
