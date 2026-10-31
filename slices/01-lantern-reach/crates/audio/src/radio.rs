//! The radio: what a channel sounds like from where the ship is, and the sounds it
//! plays (#47, 5.1).
//!
//! - **Signal** ([`signal`]): 1 near the source, easing down to 0 at the edge of the
//!   channel's range. The volume follows it.
//! - **Static** ([`static_level`]): a soft hiss that creeps in as the signal weakens
//!   towards the edge, and goes with it when the channel drops out. Out of range of
//!   everything the radio is silent.
//! - **Crossfade** ([`crossfade`]): where channels overlap, each plays at its signal
//!   times its share of the signal, so the stronger one takes over smoothly as the ship
//!   flies on.
//! - **Sounds**: two quiet placeholder loops, `placeholder.calm` and
//!   `placeholder.drift` ([`render_loop`]), synthesised offline with the same notes
//!   as the cues (D major, sine fundamentals with soft overtones, a gentle low-pass),
//!   and the static, low-passed noise ([`render_static`]). Each loop folds its tails
//!   back onto its start, so it repeats without a seam. Generated tracks from the
//!   music library are MP3 files on disk; [`Audio::radio`](crate::Audio::radio) streams
//!   them, and falls back to a placeholder if one won't decode.
//!
//! Everything here is a pure function of its inputs, tested headless.

use std::path::PathBuf;

use fundsp::prelude32::{Wave, lowpass_hz, pink};

use crate::cue::{Envelope, Timbre};
use crate::synth::{self, SAMPLE_RATE};

/// The calm placeholder loop: a slow arpeggio over a soft bass.
pub const CALM: &str = "placeholder.calm";
/// The drifting placeholder loop: slow, low chords.
pub const DRIFT: &str = "placeholder.drift";
/// Every placeholder loop's name.
pub const LOOPS: [&str; 2] = [CALM, DRIFT];

/// How the signal and the static fall off with distance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Falloff {
    /// The share of the range (from the source) at full signal.
    pub near: f32,
    /// Static creeps in below this signal.
    pub static_below: f32,
    /// The loudest the static gets, as a gain on the radio track.
    pub static_max: f32,
}

impl Default for Falloff {
    fn default() -> Self {
        Self {
            near: 0.3,
            static_below: 0.5,
            static_max: 0.3,
        }
    }
}

/// The signal at `distance` from a source with `range`: 1 within `near` of the range,
/// easing (smoothstep) to 0 at the edge, 0 beyond it.
pub fn signal(distance: f32, range: f32, falloff: &Falloff) -> f32 {
    if !(range > 0.0) || !distance.is_finite() || distance >= range {
        return 0.0;
    }
    let near = falloff.near.clamp(0.0, 0.99) * range;
    if distance <= near {
        return 1.0;
    }
    let t = 1.0 - (distance - near) / (range - near);
    t * t * (3.0 - 2.0 * t)
}

/// How loud the static is for the strongest signal heard: none at a clear signal or
/// with nothing in range, rising as the signal drops below `static_below`, and fading
/// out again over the last few percent as the channel goes, so dropping out of range
/// is silence, not a burst of hiss.
pub fn static_level(signal: f32, falloff: &Falloff) -> f32 {
    let below = falloff.static_below.clamp(0.01, 1.0);
    if !(signal > 0.0) || signal >= below {
        return 0.0;
    }
    let creep = 1.0 - signal / below;
    let tail = (signal / 0.08).min(1.0);
    falloff.static_max.max(0.0) * creep * tail
}

/// Each heard channel's gain from its signal: its signal times its share of the
/// squared signals. One channel alone plays at its signal; two overlapping crossfade.
pub fn crossfade(signals: &[f32]) -> Vec<f32> {
    let total: f32 = signals.iter().map(|s| s.max(0.0) * s.max(0.0)).sum();
    signals
        .iter()
        .map(|s| {
            let s = s.max(0.0);
            if total > 0.0 { s * s * s / total } else { 0.0 }
        })
        .collect()
}

/// What a channel plays.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    /// A named placeholder loop ([`LOOPS`]).
    Placeholder(String),
    /// A generated track on disk (MP3), streamed and looped.
    File(PathBuf),
}

/// One channel heard now.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    /// The channel's ID: a voice keeps playing while its key stays in the mix.
    pub key: String,
    pub source: Source,
    /// 0..=1, on the radio track.
    pub gain: f32,
}

/// Everything the radio should sound like now: the channels heard and the static.
/// An empty mix is silence.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Mix {
    pub voices: Vec<Voice>,
    pub static_gain: f32,
}

/// A loop's peak level, in dBFS: well under the cues.
const LOOP_PEAK_DB: f32 = -12.0;
/// The static's peak level, in dBFS (the static's gain scales it further).
const STATIC_PEAK_DB: f32 = -14.0;

/// A placeholder loop by name, mono at [`SAMPLE_RATE`]; `None` for a name it doesn't
/// know.
pub fn render_loop(name: &str) -> Option<Vec<f32>> {
    match name {
        CALM => Some(calm()),
        DRIFT => Some(drift()),
        _ => None,
    }
}

/// A soft, warm timbre for the loops: few overtones, low cutoff, no noise onset.
const SOFT: Timbre = Timbre {
    overtones: [(2.0, 0.22), (3.0, 0.06)],
    overtone_damping: 2.5,
    air: 0.0,
    air_hz: 1000.0,
    cutoff_hz: 1400.0,
};

/// MIDI note number to Hz.
fn midi(n: f32) -> f32 {
    440.0 * 2f32.powf((n - 69.0) / 12.0)
}

/// One note in a loop: MIDI pitch, start in seconds, gain, attack and decay.
struct Struck {
    pitch: f32,
    at: f32,
    gain: f32,
    attack: f32,
    decay: f32,
}

/// Renders `notes` into a loop `length` seconds long. Each note rings for six of its
/// decay time constants; what rings past the end is added back onto the start, so the
/// loop's end runs into its start exactly as if it had kept playing.
fn render_notes(notes: &[Struck], length: f32) -> Vec<f32> {
    let len = (length * SAMPLE_RATE as f32).round() as usize;
    let mut mix = vec![0.0f32; len];
    for (i, n) in notes.iter().enumerate() {
        let ring = n.attack + 6.0 * n.decay;
        let env = Envelope {
            attack: n.attack,
            decay: n.decay,
            length: ring,
            fade: 0.0,
        };
        let samples = (ring * SAMPLE_RATE as f32).round();
        let tone = synth::render_note(midi(n.pitch), &SOFT, env, samples, i as u64);
        let start = (n.at * SAMPLE_RATE as f32).round() as usize;
        for (k, s) in tone.into_iter().enumerate() {
            mix[(start + k) % len] += s * n.gain;
        }
    }
    normalise(&mut mix, LOOP_PEAK_DB);
    mix
}

fn normalise(mix: &mut [f32], peak_db: f32) {
    let mean = mix.iter().sum::<f32>() / mix.len().max(1) as f32;
    for s in mix.iter_mut() {
        *s -= mean;
    }
    let peak = mix.iter().fold(0.0f32, |p, s| p.max(s.abs()));
    if peak > 0.0 {
        let target = 10f32.powf(peak_db / 20.0);
        for s in mix.iter_mut() {
            *s *= target / peak;
        }
    }
}

/// `placeholder.calm`: I-vi-IV-V in D, a bar each, an arpeggio on the beat over a
/// soft bass note. 12 seconds.
fn calm() -> Vec<f32> {
    const BEAT: f32 = 0.75;
    // Bass, then the bar's four arpeggio notes (MIDI).
    let bars: [(f32, [f32; 4]); 4] = [
        (50.0, [62.0, 69.0, 66.0, 69.0]), // D: D4 A4 F#4 A4
        (47.0, [59.0, 66.0, 62.0, 66.0]), // Bm: B3 F#4 D4 F#4
        (43.0, [55.0, 62.0, 59.0, 62.0]), // G: G3 D4 B3 D4
        (45.0, [57.0, 64.0, 61.0, 64.0]), // A: A3 E4 C#4 E4
    ];
    let mut notes = Vec::new();
    for (b, (bass, arp)) in bars.iter().enumerate() {
        let bar = b as f32 * 4.0 * BEAT;
        notes.push(Struck {
            pitch: *bass,
            at: bar,
            gain: 0.55,
            attack: 0.08,
            decay: 1.6,
        });
        for (i, p) in arp.iter().enumerate() {
            notes.push(Struck {
                pitch: *p,
                at: bar + i as f32 * BEAT,
                gain: if i == 0 { 0.5 } else { 0.38 },
                attack: 0.03,
                decay: 0.7,
            });
        }
    }
    render_notes(&notes, 16.0 * BEAT)
}

/// `placeholder.drift`: four slow, low chords, each swelling in. 16 seconds.
fn drift() -> Vec<f32> {
    const CHORD: f32 = 4.0;
    let chords: [[f32; 3]; 4] = [
        [50.0, 57.0, 66.0], // D3 A3 F#4
        [43.0, 50.0, 59.0], // G2 D3 B3
        [47.0, 54.0, 62.0], // B2 F#3 D4
        [45.0, 52.0, 61.0], // A2 E3 C#4
    ];
    let mut notes = Vec::new();
    for (c, chord) in chords.iter().enumerate() {
        for (i, p) in chord.iter().enumerate() {
            notes.push(Struck {
                pitch: *p,
                // A slow roll up the chord.
                at: c as f32 * CHORD + i as f32 * 0.35,
                gain: 0.45,
                attack: 0.9,
                decay: 1.8,
            });
        }
    }
    render_notes(&notes, 4.0 * CHORD)
}

/// The static: pink noise through two low-passes (nothing harsh), three seconds that
/// loop with a crossfaded seam.
pub fn render_static() -> Vec<f32> {
    const SECONDS: f32 = 3.0;
    const SEAM: f32 = 0.25;
    let mut node = pink() >> lowpass_hz(1100.0, 0.5) >> lowpass_hz(1400.0, 0.5);
    node.set_hash(0x5ad10);
    let total = SECONDS + SEAM;
    let wave = Wave::render(f64::from(SAMPLE_RATE), f64::from(total), &mut node);
    let raw = wave.channel(0);
    let len = (SECONDS * SAMPLE_RATE as f32).round() as usize;
    let seam = (SEAM * SAMPLE_RATE as f32).round() as usize;
    // The first `seam` samples settle the filters; then the loop, whose start is
    // crossfaded with what follows its end.
    let body = &raw[seam..];
    let mut out: Vec<f32> = body[..len].to_vec();
    for k in 0..seam.min(body.len().saturating_sub(len)) {
        let x = k as f32 / seam as f32;
        let fade_in = (x * std::f32::consts::FRAC_PI_2).sin();
        let fade_out = (x * std::f32::consts::FRAC_PI_2).cos();
        out[k] = out[k] * fade_in + body[len + k] * fade_out;
    }
    normalise(&mut out, STATIC_PEAK_DB);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analysis::measure;

    #[test]
    fn signal_is_full_near_the_source_and_fades_to_nothing_at_the_edge() {
        let f = Falloff::default();
        let range = 1000.0;
        assert_eq!(signal(0.0, range, &f), 1.0);
        assert_eq!(signal(300.0, range, &f), 1.0, "full within `near`");
        assert_eq!(signal(1000.0, range, &f), 0.0, "nothing at the edge");
        assert_eq!(signal(5000.0, range, &f), 0.0);
        assert_eq!(signal(10.0, 0.0, &f), 0.0, "a channel with no range");
        // Strictly weaker the further out, between near and the edge.
        let mut last = 1.0;
        for d in (310..1000).step_by(10) {
            let s = signal(d as f32, range, &f);
            assert!(s < last && s > 0.0, "{d}: {s} after {last}");
            last = s;
        }
        // Halfway between near and the edge is half the signal.
        assert!((signal(650.0, range, &f) - 0.5).abs() < 1e-4);
    }

    #[test]
    fn static_creeps_in_at_the_edges_and_never_outside_the_range() {
        let f = Falloff::default();
        assert_eq!(static_level(1.0, &f), 0.0, "clear near the source");
        assert_eq!(static_level(0.6, &f), 0.0);
        assert_eq!(static_level(0.0, &f), 0.0, "out of range is silent");
        let edge = static_level(0.15, &f);
        let mid = static_level(0.35, &f);
        assert!(edge > mid && mid > 0.0, "{edge} > {mid} > 0");
        assert!(edge <= f.static_max);
        // Dropping out fades the static too, rather than a burst of hiss.
        assert!(static_level(0.01, &f) < static_level(0.08, &f));
        for d in (0..=1000).step_by(25) {
            let s = static_level(signal(d as f32, 1000.0, &f), &f);
            assert!((0.0..=f.static_max).contains(&s), "{d}: {s}");
        }
    }

    #[test]
    fn overlapping_channels_crossfade_by_signal() {
        assert_eq!(crossfade(&[]), Vec::<f32>::new());
        assert!((crossfade(&[0.8])[0] - 0.8).abs() < 1e-6, "alone: its own signal");
        assert_eq!(crossfade(&[0.0, 0.0]), vec![0.0, 0.0]);
        let even = crossfade(&[0.5, 0.5]);
        assert!((even[0] - 0.25).abs() < 1e-6 && (even[0] - even[1]).abs() < 1e-6);
        // Flying from one source to the other hands over smoothly.
        let f = Falloff::default();
        let mut last = (1.0, 0.0);
        for x in (0..=1000).step_by(50) {
            let a = signal(x as f32, 700.0, &f);
            let b = signal(1000.0 - x as f32, 700.0, &f);
            let g = crossfade(&[a, b]);
            assert!(g[0] <= last.0 + 1e-6 && g[1] >= last.1 - 1e-6, "{x}: {g:?}");
            assert!(g.iter().all(|v| (0.0..=1.0).contains(v)));
            last = (g[0], g[1]);
        }
        let g = crossfade(&[0.9, 0.3]);
        assert!(g[0] > 0.8 && g[1] < 0.05, "the stronger dominates: {g:?}");
    }

    #[test]
    fn the_placeholder_loops_are_quiet_warm_and_seamless() {
        for name in LOOPS {
            let samples = render_loop(name).unwrap();
            let m = measure(&samples, SAMPLE_RATE);
            assert!(m.duration >= 10.0, "{name}: {:.1} s", m.duration);
            assert!(m.peak_db <= -11.9, "{name}: peak {:.1}", m.peak_db);
            assert!(m.rms_db <= -24.0, "{name}: rms {:.1}", m.rms_db);
            assert!(m.centroid_hz <= 900.0, "{name}: centroid {:.0}", m.centroid_hz);
            assert!(m.harsh_share <= 0.01, "{name}: harsh {:.4}", m.harsh_share);
            assert!(m.dc.abs() <= 1e-4, "{name}: dc {}", m.dc);
            // The end runs into the start: no step at the seam.
            let (first, last) = (samples[0], samples[samples.len() - 1]);
            let step = (first - last).abs();
            let typical = samples
                .windows(2)
                .map(|w| (w[1] - w[0]).abs())
                .fold(0.0f32, f32::max);
            assert!(step <= typical, "{name}: seam step {step} > {typical}");
            assert_eq!(samples, render_loop(name).unwrap(), "{name}: deterministic");
        }
        assert!(render_loop("placeholder.loud").is_none());
    }

    #[test]
    fn the_static_is_soft_noise_with_nothing_harsh() {
        let samples = render_static();
        let m = measure(&samples, SAMPLE_RATE);
        assert!(m.peak_db <= -13.9, "peak {:.1}", m.peak_db);
        assert!(m.harsh_share <= 0.05, "harsh {:.4}", m.harsh_share);
        assert!(m.centroid_hz <= 1200.0, "centroid {:.0}", m.centroid_hz);
        assert_eq!(samples, render_static(), "deterministic");
    }
}
