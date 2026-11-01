use crate::Clip;
use serde::Deserialize;

pub const MAX_VOICES: usize = 32;
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
    Play {
        clip: Clip,
        bus: Bus,
        volume: f32,
        fade: f32,
    },
    Stop {
        bus: Bus,
        fade: f32,
    },
    Volume {
        bus: Option<Bus>,
        volume: f32,
        fade: f32,
    },
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
    position: f64,
    gain: Ramp,
    stopping: bool,
}

/// Our linear resampler, bounded voices, ramps, bus gains and final safety clamp.
/// Rendering is allocation-free. Device callbacks use the same `frame` as offline.
pub struct Mixer {
    rate: u32,
    voices: Vec<Voice>,
    master: Ramp,
    buses: [Ramp; 3],
    pub limited_samples: u64,
    pub stolen_voices: u64,
}

impl Mixer {
    pub fn new(rate: u32) -> Self {
        Self {
            rate: rate.max(1),
            voices: Vec::with_capacity(MAX_VOICES),
            master: Ramp::fixed(0.7),
            buses: [Ramp::fixed(1.0); 3],
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
            } => {
                let frames = self.frames(fade.max(EDGE_SECONDS));
                if bus != Bus::Sound {
                    self.stop(bus, frames);
                }
                if self.voices.len() == MAX_VOICES {
                    // Oldest effect first; otherwise the quietest outgoing loop.
                    let index = self
                        .voices
                        .iter()
                        .position(|v| v.bus == Bus::Sound)
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
                    position: 0.0,
                    gain,
                    stopping: false,
                });
            }
            Action::Stop { bus, fade } => self.stop(bus, self.frames(fade.max(EDGE_SECONDS))),
            Action::Volume { bus, volume, fade } => {
                let frames = self.frames(fade);
                let ramp = match bus {
                    Some(bus) => &mut self.buses[bus.index()],
                    None => &mut self.master,
                };
                ramp.to(finite(volume).clamp(0.0, 1.0), frames);
            }
        }
    }

    fn stop(&mut self, bus: Bus, frames: u64) {
        for v in &mut self.voices {
            if v.bus == bus && !v.stopping {
                v.gain.to(0.0, frames);
                v.stopping = true;
            }
        }
    }

    pub fn frame(&mut self) -> [f32; 2] {
        let mut out = [0.0_f32; 2];
        let gains = self.buses.each_mut().map(Ramp::next);
        for v in &mut self.voices {
            let count = v.clip.frames.len();
            let i = v.position as usize;
            if i >= count {
                continue;
            }
            let j = if i + 1 < count {
                i + 1
            } else if v.bus != Bus::Sound {
                0
            } else {
                i
            };
            let fraction = (v.position - i as f64) as f32;
            let mut gain = v.gain.next() * gains[v.bus.index()];
            if v.bus == Bus::Sound {
                let remaining = (count as f64 - v.position) / f64::from(v.clip.rate);
                gain *= (remaining as f32 / EDGE_SECONDS).min(1.0);
            }
            for (channel, sample) in out.iter_mut().enumerate() {
                let a = v.clip.frames[i][channel];
                let b = v.clip.frames[j][channel];
                *sample += (a + (b - a) * fraction) * gain;
            }
            v.position += f64::from(v.clip.rate) / f64::from(self.rate);
            if v.bus != Bus::Sound {
                v.position %= count as f64;
            }
        }
        self.voices.retain(|v| {
            !(v.stopping && v.gain.left == 0)
                && (v.bus != Bus::Sound || v.position < v.clip.frames.len() as f64)
        });
        let master = self.master.next();
        for s in &mut out {
            *s *= master;
            if s.abs() > 1.0 {
                self.limited_samples += 1;
            }
            *s = s.clamp(-1.0, 1.0);
        }
        out
    }

    pub fn render(&mut self, frames: &mut [[f32; 2]]) {
        for frame in frames {
            *frame = self.frame();
        }
    }
}

fn finite(value: f32) -> f32 {
    if value.is_finite() { value } else { 0.0 }
}
