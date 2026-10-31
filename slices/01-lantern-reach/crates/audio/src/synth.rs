//! Offline synthesis: a [`Patch`] rendered with fundsp into mono takes.
//!
//! This runs once, at startup (or in `galaxy audio render`), never on the audio thread:
//! kira only plays the finished samples. The same patch always renders the same
//! samples, so a cue can be measured once and trusted.

use std::f32::consts::PI;

use fundsp::prelude32::{Wave, bandpass_hz, dcblock_hz, envelope, lowpass_hz, noise, sine_hz};

use crate::cue::{Envelope, Patch, Timbre};

/// Every cue is rendered at this rate; kira resamples to the device.
pub const SAMPLE_RATE: u32 = 48_000;

/// Every take of a patch, in order.
pub fn render(patch: &Patch) -> Vec<Vec<f32>> {
    (0..patch.takes.max(1))
        .map(|take| render_take(patch, take))
        .collect()
}

/// The pitch offset of take `take`, in cents: takes are spread evenly over the patch's
/// spread, so each one sits at its own pitch.
pub fn take_cents(patch: &Patch, take: u32) -> f32 {
    let n = patch.takes.max(1);
    if n == 1 {
        return 0.0;
    }
    let t = take.min(n - 1) as f32 / (n - 1) as f32;
    (t * 2.0 - 1.0) * patch.take_spread_cents
}

/// One take: every note mixed at its start, the closing fade, then scaled so the peak
/// sits exactly at the patch's `peak_db`.
pub fn render_take(patch: &Patch, take: u32) -> Vec<f32> {
    let env = patch.envelope;
    let len = seconds_to_samples(env.length);
    if len == 0 {
        return Vec::new();
    }
    let mut mix = vec![0.0f32; len];
    let detune = 2f32.powf(take_cents(patch, take) / 1200.0);
    for (i, note) in patch.notes.iter().enumerate() {
        let start = seconds_to_samples(note.at).min(len);
        let hz = patch.root_hz * 2f32.powf(note.semitones / 12.0) * detune;
        let seed = u64::from(take) << 16 | i as u64;
        let tone = render_note(hz, &patch.timbre, env, (len - start) as f32, seed);
        for (m, s) in mix[start..].iter_mut().zip(tone) {
            *m += s * note.gain;
        }
    }
    let fade = seconds_to_samples(env.fade).clamp(1, len.max(1));
    for (k, s) in mix[len - fade..].iter_mut().enumerate() {
        let x = (k + 1) as f32 / fade as f32;
        *s *= 0.5 + 0.5 * (PI * x).cos();
    }
    let peak = mix.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    if peak > 0.0 {
        let target = 10f32.powf(patch.peak_db / 20.0);
        for s in &mut mix {
            *s *= target / peak;
        }
    }
    mix
}

fn seconds_to_samples(s: f32) -> usize {
    (s.max(0.0) * SAMPLE_RATE as f32).round() as usize
}

/// A raised-cosine rise over `attack` seconds, then an exponential decay with time
/// constant `tau`.
fn shape(t: f32, attack: f32, tau: f32) -> f32 {
    if t < attack {
        0.5 - 0.5 * (PI * t / attack).cos()
    } else {
        (-(t - attack) / tau.max(1e-4)).exp()
    }
}

/// One struck note, `samples` long: the fundamental and its overtones, each under its
/// own envelope, with the noise onset, through the low-pass and a DC blocker.
pub(crate) fn render_note(hz: f32, t: &Timbre, env: Envelope, samples: f32, seed: u64) -> Vec<f32> {
    let [(r1, a1), (r2, a2)] = t.overtones;
    let (attack, tau, damp) = (env.attack, env.decay, t.overtone_damping.max(1.0));
    let air = t.air;
    // Overtones above the low-pass would only waste energy; drop them.
    let keep = |r: f32, a: f32| if hz * r < t.cutoff_hz * 1.5 { a } else { 0.0 };
    let (a1, a2) = (keep(r1, a1), keep(r2, a2));
    let tone = sine_hz(hz) * envelope(move |x: f32| shape(x, attack, tau))
        + sine_hz(hz * r1) * envelope(move |x: f32| a1 * shape(x, attack, tau / damp))
        + sine_hz(hz * r2) * envelope(move |x: f32| a2 * shape(x, attack, tau / (damp * damp)));
    let onset = (noise() >> bandpass_hz(t.air_hz, 1.5))
        * envelope(move |x: f32| air * shape(x, 0.0015, 0.006));
    let mut node = (tone + onset) >> lowpass_hz(t.cutoff_hz, 0.6) >> dcblock_hz(20.0);
    node.set_hash(seed);
    let wave = Wave::render(
        f64::from(SAMPLE_RATE),
        f64::from(samples) / f64::from(SAMPLE_RATE),
        &mut node,
    );
    wave.channel(0).clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cue::Cue;

    #[test]
    fn rendering_is_deterministic() {
        for cue in Cue::ALL {
            let p = cue.spec().patch;
            assert_eq!(render(&p), render(&p), "{}", cue.name());
        }
    }

    #[test]
    fn takes_differ() {
        let p = Cue::OreTick.spec().patch;
        let takes = render(&p);
        assert_eq!(takes.len(), p.takes as usize);
        for i in 0..takes.len() {
            for j in i + 1..takes.len() {
                assert_ne!(takes[i], takes[j], "takes {i} and {j} are identical");
            }
        }
    }
}
