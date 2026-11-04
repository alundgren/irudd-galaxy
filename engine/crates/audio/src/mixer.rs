use crate::Clip;
use serde::Deserialize;

pub const MAX_VOICES: usize = 32;
/// Named loop channels across all modules.
pub const MAX_CHANNELS: usize = 32;
const EDGE_SECONDS: f32 = 0.005;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Bus {
    #[default]
    Sound,
    Music,
    Ambience,
}

impl Bus {
    fn index(self) -> usize {
        match self {
            Self::Sound => 0,
            Self::Music => 1,
            Self::Ambience => 2,
        }
    }
}

/// A fully prepared change, shared by offline and device playback.
#[derive(Clone, Debug)]
pub enum Action {
    /// A one-shot on the sound bus, or the loop of the music or ambience bus. `pitch`
    /// scales the playback rate (1 as recorded).
    Play {
        clip: Clip,
        bus: Bus,
        volume: f32,
        fade: f32,
        pitch: f32,
    },
    /// A loop on a named channel of the sound bus. If the channel already plays this
    /// clip, it carries on toward the new volume at its own pitch; otherwise the channel
    /// crossfades.
    Loop {
        clip: Clip,
        channel: u8,
        volume: f32,
        fade: f32,
        pitch: f32,
    },
    /// Fades out every voice on the bus, channel loops included on the sound bus.
    Stop { bus: Bus, fade: f32 },
    /// Fades out one channel's loop.
    StopChannel { channel: u8, fade: f32 },
    Volume {
        bus: Option<Bus>,
        volume: f32,
        fade: f32,
    },
    /// Ramps the gain and rate a channel's loops (or, with `None`, the ambience) play
    /// at, on top of their own volume and pitch. Kept until the next tune or reset.
    Tune {
        channel: Option<u8>,
        gain: f32,
        pitch: f32,
        fade: f32,
    },
    /// Puts every tune back to gain 1 and rate 1 at once, for a new bank.
    ResetTunes,
}

#[derive(Clone, Copy)]
struct Ramp {
    value: f32,
    target: f32,
    left: u64,
}
impl Ramp {
    fn fixed(value: f32) -> Self {
        Self {
            value,
            target: value,
            left: 0,
        }
    }
    fn to(&mut self, target: f32, frames: u64) {
        self.target = target;
        self.left = frames;
        if frames == 0 {
            self.value = target;
        }
    }
    fn next(&mut self) -> f32 {
        if self.left > 0 {
            self.value += (self.target - self.value) / self.left as f32;
            self.left -= 1;
        }
        self.value
    }
}

struct Voice {
    clip: Clip,
    bus: Bus,
    /// The named loop channel, on the sound bus.
    channel: Option<u8>,
    position: f64,
    gain: Ramp,
    /// The voice's own rate, from per-cue variation.
    pitch: f32,
    stopping: bool,
}

impl Voice {
    fn looping(&self) -> bool {
        self.bus != Bus::Sound || self.channel.is_some()
    }
    /// Its slot in [`Mixer::tunes`]: 0 is the ambience, then one per channel.
    fn tune(&self) -> Option<usize> {
        match (self.channel, self.bus) {
            (Some(c), _) => Some(usize::from(c) + 1),
            (None, Bus::Ambience) => Some(0),
            _ => None,
        }
    }
}

/// Our linear resampler, bounded voices, ramps, bus gains and final safety clamp.
/// Rendering is allocation-free. Device callbacks use the same `frame` as offline.
pub struct Mixer {
    rate: u32,
    voices: Vec<Voice>,
    master: Ramp,
    buses: [Ramp; 3],
    /// Gain and rate ramps for the ambience and each channel (see [`Voice::tune`]).
    tunes: [[Ramp; 2]; MAX_CHANNELS + 1],
    pub limited_samples: u64,
    pub stolen_voices: u64,
}

/// One real mixer frame. Stems include voice, bus and master ramps, before the
/// shared final clamp. Bus order is sound, music, ambience.
pub struct MixFrame {
    pub output: [f32; 2],
    pub pre_clamp: [f32; 2],
    pub buses: [[f32; 2]; 3],
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            voices: Vec::with_capacity(MAX_VOICES),
            master: Ramp::fixed(0.7),
            buses: [Ramp::fixed(1.0); 3],
            tunes: [[Ramp::fixed(1.0); 2]; MAX_CHANNELS + 1],
            limited_samples: 0,
            stolen_voices: 0,
        }
    }
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }
    pub fn voice_count(&self) -> usize {
        self.voices.len()
    }
    fn frames(&self, seconds: f32) -> u64 {
        (finite(seconds).clamp(0.0, 60.0) * self.rate as f32).round() as u64
    }

    pub fn apply(&mut self, action: Action) {
        match action {
            Action::Play {
                clip,
                bus,
                volume,
                fade,
                pitch,
            } => {
                let frames = self.frames(fade.max(EDGE_SECONDS));
                if bus != Bus::Sound {
                    self.stop(bus, frames);
                }
                self.start(clip, bus, None, volume, frames, pitch);
            }
            Action::Loop {
                clip,
                channel,
                volume,
                fade,
                pitch,
            } => {
                let frames = self.frames(fade.max(EDGE_SECONDS));
                let volume = finite(volume).clamp(0.0, 1.0);
                if let Some(v) = self.voices.iter_mut().find(|v| {
                    v.channel == Some(channel) && !v.stopping && v.clip.same_frames(&clip)
                }) {
                    v.gain.to(volume, frames);
                    return;
                }
                self.stop_channel(channel, frames);
                self.start(clip, Bus::Sound, Some(channel), volume, frames, pitch);
            }
            Action::Stop { bus, fade } => self.stop(bus, self.frames(fade.max(EDGE_SECONDS))),
            Action::StopChannel { channel, fade } => {
                self.stop_channel(channel, self.frames(fade.max(EDGE_SECONDS)));
            }
            Action::Volume { bus, volume, fade } => {
                let frames = self.frames(fade);
                let ramp = match bus {
                    Some(bus) => &mut self.buses[bus.index()],
                    None => &mut self.master,
                };
                ramp.to(finite(volume).clamp(0.0, 1.0), frames);
            }
            Action::Tune {
                channel,
                gain,
                pitch,
                fade,
            } => {
                let frames = self.frames(fade);
                let slot = channel.map_or(0, |c| usize::from(c) + 1);
                if let Some([g, p]) = self.tunes.get_mut(slot) {
                    g.to(finite(gain).clamp(0.0, 1.0), frames);
                    p.to(rate(pitch), frames);
                }
            }
            Action::ResetTunes => {
                self.tunes = [[Ramp::fixed(1.0); 2]; MAX_CHANNELS + 1];
            }
        }
    }

    fn start(
        &mut self,
        clip: Clip,
        bus: Bus,
        channel: Option<u8>,
        volume: f32,
        frames: u64,
        pitch: f32,
    ) {
        if self.voices.len() == MAX_VOICES {
            // Oldest one-shot effect first; otherwise the quietest outgoing loop.
            let index = self
                .voices
                .iter()
                .position(|v| !v.looping())
                .unwrap_or_else(|| {
                    self.voices
                        .iter()
                        .enumerate()
                        .min_by(|(_, a), (_, b)| a.gain.value.total_cmp(&b.gain.value))
                        .map(|(i, _)| i)
                        .unwrap_or(0)
                });
            self.voices.remove(index);
            self.stolen_voices += 1;
        }
        let mut gain = Ramp::fixed(0.0);
        gain.to(finite(volume).clamp(0.0, 1.0), frames);
        self.voices.push(Voice {
            clip,
            bus,
            channel,
            position: 0.0,
            gain,
            pitch: rate(pitch),
            stopping: false,
        });
    }

    fn stop(&mut self, bus: Bus, frames: u64) {
        for v in &mut self.voices {
            if v.bus == bus && !v.stopping {
                v.gain.to(0.0, frames);
                v.stopping = true;
            }
        }
    }

    fn stop_channel(&mut self, channel: u8, frames: u64) {
        for v in &mut self.voices {
            if v.channel == Some(channel) && !v.stopping {
                v.gain.to(0.0, frames);
                v.stopping = true;
            }
        }
    }

    pub fn frame(&mut self) -> [f32; 2] {
        self.frame_with_buses().output
    }

    pub fn frame_with_buses(&mut self) -> MixFrame {
        let mut out = [0.0_f32; 2];
        let mut buses = [[0.0_f32; 2]; 3];
        let gains = self.buses.each_mut().map(Ramp::next);
        let tunes = self.tunes.each_mut().map(|[g, p]| [g.next(), p.next()]);
        for v in &mut self.voices {
            let count = v.clip.frames.len();
            if v.position >= count as f64 {
                continue;
            }
            let looping = v.looping();
            let [tune_gain, tune_pitch] = v.tune().map_or([1.0, 1.0], |i| tunes[i]);
            let step = f64::from(v.clip.rate) / f64::from(self.rate)
                * f64::from(v.pitch)
                * f64::from(tune_pitch);
            let seam = if looping { v.clip.seam } else { 0 };
            let mut gain = v.gain.next() * gains[v.bus.index()] * tune_gain;
            if !looping {
                let remaining =
                    (count as f64 - v.position) / f64::from(v.clip.rate) / f64::from(v.pitch);
                gain *= (remaining as f32 / EDGE_SECONDS).min(1.0);
            }
            let mut value = sample(&v.clip.frames, v.position, looping && seam == 0);
            // The last `seam` frames fade out while the start fades in, so the loop
            // continues from frame `seam` with no jump.
            let start = (count - seam) as f64;
            if seam > 0 && v.position >= start {
                let into = v.position - start;
                let t = (into / seam as f64) as f32;
                let head = sample(&v.clip.frames, into, false);
                for (tail, head) in value.iter_mut().zip(head) {
                    *tail += (head - *tail) * t;
                }
            }
            for (c, (out, value)) in out.iter_mut().zip(value).enumerate() {
                *out += value * gain;
                buses[v.bus.index()][c] += value * gain;
            }
            v.position += step;
            if looping && v.position >= count as f64 {
                v.position = seam as f64 + (v.position - count as f64) % start;
            }
        }
        self.voices.retain(|v| {
            !(v.stopping && v.gain.left == 0)
                && (v.looping() || v.position < v.clip.frames.len() as f64)
        });
        let master = self.master.next();
        let pre_clamp = out.map(|s| s * master);
        for sample in buses.iter_mut().flatten() {
            *sample *= master;
        }
        for s in &mut out {
            *s *= master;
            if s.abs() > 1.0 {
                self.limited_samples += 1;
            }
            *s = s.clamp(-1.0, 1.0);
        }
        MixFrame {
            output: out,
            pre_clamp,
            buses,
        }
    }

    pub fn render(&mut self, frames: &mut [[f32; 2]]) {
        for frame in frames {
            *frame = self.frame();
        }
    }
}

/// The frame at a fractional position, linearly interpolated; past the end it
/// wraps to the first frame or holds the last.
fn sample(frames: &[[f32; 2]], position: f64, wrap: bool) -> [f32; 2] {
    let i = position as usize;
    let j = if i + 1 < frames.len() {
        i + 1
    } else if wrap {
        0
    } else {
        i
    };
    let fraction = (position - i as f64) as f32;
    let (a, b) = (frames[i], frames[j]);
    [0, 1].map(|c| a[c] + (b[c] - a[c]) * fraction)
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}

/// A playback rate scale: finite, 1 when it isn't, within 0.25..4.
fn rate(value: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.25, 4.0)
    } else {
        1.0
    }
}
