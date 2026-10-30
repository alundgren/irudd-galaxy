//! Measuring a cue without ears, and writing it to a WAV file to listen to.
//!
//! [`measure`] gives the numbers the design rules are checked against: peak and RMS
//! level, length, spectral centroid (low means warm), the share of energy in the harsh
//! 2 to 5 kHz band, DC offset, and how close to silence the cue starts and ends.

use std::f64::consts::PI;
use std::io::{self, Write};
use std::path::Path;

/// The measurements of one mono cue.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Metrics {
    /// Seconds.
    pub duration: f32,
    /// The loudest sample, in dBFS (0 is full scale).
    pub peak_db: f32,
    /// RMS level over the whole cue, in dBFS.
    pub rms_db: f32,
    /// The power-weighted mean frequency, in Hz.
    pub centroid_hz: f32,
    /// The share of the power between 2 and 5 kHz, 0..1.
    pub harsh_share: f32,
    /// The mean sample value.
    pub dc: f32,
    /// Seconds until the cue first reaches half its peak: a soft attack takes a while.
    pub rise: f32,
    /// The larger of the first and last sample: a cue that starts or stops away from
    /// zero clicks.
    pub edge: f32,
}

/// Linear amplitude to dBFS (silence floors at -120).
pub fn db(amplitude: f32) -> f32 {
    20.0 * amplitude.max(1e-6).log10()
}

pub fn measure(samples: &[f32], sample_rate: u32) -> Metrics {
    let n = samples.len().max(1);
    let peak = samples.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    let sum_sq: f64 = samples.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let mean: f64 = samples.iter().map(|&s| f64::from(s)).sum::<f64>() / n as f64;
    let edge = match (samples.first(), samples.last()) {
        (Some(a), Some(b)) => a.abs().max(b.abs()),
        _ => 0.0,
    };
    let rise = samples
        .iter()
        .position(|s| s.abs() >= peak * 0.5)
        .unwrap_or(0) as f32
        / sample_rate as f32;
    let (centroid_hz, harsh_share) = spectrum(samples, sample_rate);
    Metrics {
        duration: samples.len() as f32 / sample_rate as f32,
        peak_db: db(peak),
        rms_db: db((sum_sq / n as f64).sqrt() as f32),
        centroid_hz,
        harsh_share,
        dc: mean as f32,
        rise,
        edge,
    }
}

/// Spectral centroid and harsh-band share from Hann-windowed frames, power-weighted
/// over the whole cue.
fn spectrum(samples: &[f32], sample_rate: u32) -> (f32, f32) {
    const N: usize = 2048;
    const HOP: usize = N / 2;
    let window: Vec<f64> = (0..N)
        .map(|i| 0.5 - 0.5 * (2.0 * PI * i as f64 / N as f64).cos())
        .collect();
    let bin_hz = f64::from(sample_rate) / N as f64;
    let (mut total, mut weighted, mut harsh) = (0.0f64, 0.0f64, 0.0f64);
    let mut start = 0;
    loop {
        let mut re = vec![0.0f64; N];
        let mut im = vec![0.0f64; N];
        for (i, r) in re.iter_mut().enumerate() {
            let s = samples.get(start + i).copied().unwrap_or(0.0);
            *r = f64::from(s) * window[i];
        }
        fft(&mut re, &mut im);
        for k in 1..N / 2 {
            let p = re[k] * re[k] + im[k] * im[k];
            let f = k as f64 * bin_hz;
            total += p;
            weighted += p * f;
            if (2000.0..5000.0).contains(&f) {
                harsh += p;
            }
        }
        start += HOP;
        if start >= samples.len() {
            break;
        }
    }
    if total <= 0.0 {
        return (0.0, 0.0);
    }
    ((weighted / total) as f32, (harsh / total) as f32)
}

/// In-place iterative radix-2 FFT; `re.len()` must be a power of two.
fn fft(re: &mut [f64], im: &mut [f64]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * PI / len as f64;
        let (wr, wi) = (ang.cos(), ang.sin());
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0, 0.0);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let next = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = next;
            }
        }
        len <<= 1;
    }
}

/// Writes mono samples as a 16-bit PCM WAV file.
pub fn write_wav(path: &Path, samples: &[f32], sample_rate: u32) -> io::Result<()> {
    let mut out = io::BufWriter::new(std::fs::File::create(path)?);
    let data_len = (samples.len() * 2) as u32;
    out.write_all(b"RIFF")?;
    out.write_all(&(36 + data_len).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&1u16.to_le_bytes())?; // mono
    out.write_all(&sample_rate.to_le_bytes())?;
    out.write_all(&(sample_rate * 2).to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data_len.to_le_bytes())?;
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        out.write_all(&v.to_le_bytes())?;
    }
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(hz: f32, secs: f32, amp: f32) -> Vec<f32> {
        let sr = 48_000.0;
        (0..(secs * sr) as usize)
            .map(|i| amp * (2.0 * std::f32::consts::PI * hz * i as f32 / sr).sin())
            .collect()
    }

    #[test]
    fn a_sine_measures_as_itself() {
        let m = measure(&sine(500.0, 0.5, 0.5), 48_000);
        assert!((m.peak_db - db(0.5)).abs() < 0.01, "{m:?}");
        // A sine's RMS is 3 dB under its peak.
        assert!((m.rms_db - (db(0.5) - 3.01)).abs() < 0.05, "{m:?}");
        assert!((m.centroid_hz - 500.0).abs() < 25.0, "{m:?}");
        assert!(m.harsh_share < 0.01, "{m:?}");
        assert!(m.dc.abs() < 1e-3);
        let m = measure(&sine(3000.0, 0.5, 0.5), 48_000);
        assert!(m.harsh_share > 0.95, "{m:?}");
    }

    #[test]
    fn wav_files_have_the_right_size() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.wav");
        write_wav(&p, &sine(440.0, 0.1, 0.5), 48_000).unwrap();
        let bytes = std::fs::read(&p).unwrap();
        assert_eq!(bytes.len(), 44 + 4800 * 2);
        assert_eq!(&bytes[..4], b"RIFF");
    }
}
