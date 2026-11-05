use crate::bank::{Cue, Curve, Source, UiSound};
use crate::mixer::MAX_CHANNELS;
use crate::{Action, Bank, Bus};
use galaxy_api::command::Fields;
use galaxy_api::ui::{UiAction, UiCue};
use galaxy_api::{WorldView, engine};

/// A new fact as the audio scene hears it: its kind, and the tick and `entity` that seed
/// a cue's variation.
#[derive(Clone, Copy, Debug)]
pub struct Heard<'a> {
    pub kind: &'a str,
    pub tick: u64,
    pub entity: &'a str,
}

impl<'a> Heard<'a> {
    /// A recorded fact: its kind, tick and payload (`entity`, when it names one).
    pub fn fact(kind: &'a str, tick: u64, payload: &'a Fields) -> Self {
        Self {
            kind,
            tick,
            entity: payload.get("entity").and_then(|v| v.as_str()).unwrap_or(""),
        }
    }
}

impl<'a> From<&'a str> for Heard<'a> {
    fn from(kind: &'a str) -> Self {
        Self {
            kind,
            tick: 0,
            entity: "",
        }
    }
}

/// Presentation state only. Call after each fixed tick with just its new facts.
pub struct Scene {
    bank: Bank,
    ambience: Option<String>,
    fade: f32,
    /// The gain and rate last sent for the ambience (0) and each channel (1..).
    tuned: [Option<[f32; 2]>; MAX_CHANNELS + 1],
    /// The screen-clock time of the last hover sound played.
    last_hover: Option<f32>,
}

impl Scene {
    pub fn new(bank: Bank) -> Self {
        Self {
            bank,
            ambience: None,
            fade: 1.0,
            tuned: [None; MAX_CHANNELS + 1],
            last_hover: None,
        }
    }

    /// The sound a UI cue plays, if the modules give that widget one: a one-shot on the
    /// sound bus. A hover within `hover_gap` of the last hover sound plays nothing, so a
    /// pointer swept across a list ticks at most that often.
    pub fn ui(&mut self, cue: &UiCue) -> Option<(Action, &str)> {
        let sound = self
            .bank
            .ui
            .sound(cue.action, &cue.kind, cue.look.as_deref())?;
        if cue.action == UiAction::Hover {
            if self
                .last_hover
                .is_some_and(|last| (cue.time - last).abs() < self.bank.ui.hover_gap)
            {
                return None;
            }
            self.last_hover = Some(cue.time);
        }
        let [pitch, gain] = ui_vary(cue, sound);
        Some((
            Action::Play {
                clip: sound.clip.clone(),
                bus: Bus::Sound,
                volume: sound.volume * gain,
                fade: 0.0,
                pitch,
            },
            &sound.name,
        ))
    }

    pub fn update<'a, H: Into<Heard<'a>>>(
        &mut self,
        world: &impl WorldView,
        events: impl IntoIterator<Item = H>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for heard in events {
            let heard = heard.into();
            for (index, cue) in self
                .bank
                .events
                .get(heard.kind)
                .into_iter()
                .flatten()
                .enumerate()
            {
                let [pitch, gain] = vary(&heard, index, cue);
                actions.push(action(cue, pitch, gain));
            }
        }
        let place = world
            .with_component(engine::BEING)
            .into_iter()
            .find_map(|entity| {
                let being = world.get(entity, engine::BEING)?;
                if being.get("player").and_then(|v| v.as_bool()) != Some(true) {
                    return None;
                }
                world.get(entity, engine::LOCATION)?.get("place")?.as_str()
            });
        let next = place.and_then(|p| self.bank.places.get(p));
        let name = next.map(|(id, _)| id.as_str());
        if self.ambience.as_deref() != name {
            if let Some((_, cue)) = next {
                self.fade = cue.fade;
                actions.push(action(cue, 1.0, 1.0));
            } else {
                actions.push(Action::Stop {
                    bus: Bus::Ambience,
                    fade: self.fade,
                });
            }
            self.ambience = name.map(str::to_owned);
        }
        self.follow(world, &mut actions);
        actions
    }

    /// Tunes each followed loop to its curve, sending only what changed. Several curves
    /// on one loop multiply.
    fn follow(&mut self, world: &impl WorldView, actions: &mut Vec<Action>) {
        let mut wanted: [Option<([f32; 2], f32)>; MAX_CHANNELS + 1] = [None; MAX_CHANNELS + 1];
        for curve in &self.bank.follows {
            let slot = curve.target.map_or(0, |c| usize::from(c) + 1);
            let [gain, pitch] = curve.at(value(world, curve));
            let ([g, p], ramp) = wanted[slot].get_or_insert(([1.0, 1.0], 0.0));
            *g *= gain;
            *p *= pitch;
            *ramp = ramp.max(curve.ramp);
        }
        for (slot, want) in wanted.into_iter().enumerate() {
            let Some((tune, ramp)) = want else { continue };
            let first = self.tuned[slot].is_none();
            if self.tuned[slot]
                .is_some_and(|[g, p]| (g - tune[0]).abs() < 1e-4 && (p - tune[1]).abs() < 1e-4)
            {
                continue;
            }
            self.tuned[slot] = Some(tune);
            actions.push(Action::Tune {
                channel: slot.checked_sub(1).map(|c| c as u8),
                gain: tune[0],
                pitch: tune[1],
                fade: if first { 0.0 } else { ramp },
            });
        }
    }
}

impl Curve {
    /// Gain and rate where `value` falls on the curve.
    fn at(&self, value: Option<f64>) -> [f32; 2] {
        let from = f64::from(self.from);
        let t = value.map_or(0.0, |v| {
            ((v - from) / (f64::from(self.to) - from)).clamp(0.0, 1.0)
        });
        let t = t.powf(f64::from(self.power)) as f32;
        let along = |pair: Option<[f32; 2]>| pair.map_or(1.0, |[a, b]| a + (b - a) * t);
        [along(self.gain), along(self.pitch)]
    }
}

/// The world value a curve reads: a field, a vector's length or a distance.
fn value(world: &impl WorldView, curve: &Curve) -> Option<f64> {
    match &curve.source {
        Source::Fields {
            component,
            fields,
            vector,
        } => {
            let fields_of = world.get(&curve.entity, component)?;
            let values: Option<Vec<f64>> =
                fields.iter().map(|f| fields_of.get(f)?.as_f64()).collect();
            match values?.as_slice() {
                [one] if !vector => Some(*one),
                many => Some(many.iter().map(|v| v * v).sum::<f64>().sqrt()),
            }
        }
        Source::Distance(other) => {
            let at = |entity: &str| {
                let p = world.get(entity, engine::POSITION)?;
                Some((p.get("x")?.as_f64()?, p.get("y")?.as_f64()?))
            };
            let (a, b) = (at(&curve.entity)?, at(other)?);
            Some((a.0 - b.0).hypot(a.1 - b.1))
        }
    }
    .filter(|v| v.is_finite())
}

/// A UI cue's rate and gain, drawn like a fact's from its time, widget and action.
fn ui_vary(cue: &UiCue, sound: &UiSound) -> [f32; 2] {
    let heard = Heard {
        kind: cue.action.name(),
        tick: u64::from(cue.time.to_bits()),
        entity: &cue.widget,
    };
    draw(&heard, 0, sound.vary_pitch, sound.vary_gain)
}

/// A cue's random rate and gain: the same fact always gets the same draw, on every OS.
fn vary(heard: &Heard, index: usize, cue: &Cue) -> [f32; 2] {
    draw(heard, index, cue.vary_pitch, cue.vary_gain)
}

fn draw(heard: &Heard, index: usize, vary_pitch: f32, vary_gain: f32) -> [f32; 2] {
    if vary_pitch == 0.0 && vary_gain == 0.0 {
        return [1.0, 1.0];
    }
    // FNV-1a over what identifies the cue, then two SplitMix64 draws.
    let mut seed = 0xcbf2_9ce4_8422_2325_u64;
    let bytes = heard
        .tick
        .to_le_bytes()
        .into_iter()
        .chain(heard.entity.bytes())
        .chain([0])
        .chain(heard.kind.bytes())
        .chain((index as u64).to_le_bytes());
    for byte in bytes {
        seed = (seed ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    let mut draw = || {
        seed = seed.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = seed;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        ((z ^ (z >> 31)) >> 40) as f32 / (1u64 << 24) as f32
    };
    let (a, b) = (draw(), draw());
    [1.0 + vary_pitch * (2.0 * a - 1.0), 1.0 - vary_gain * b]
}

fn action(cue: &Cue, pitch: f32, gain: f32) -> Action {
    match (&cue.sound, cue.channel) {
        (Some((clip, volume)), Some(channel)) => Action::Loop {
            clip: clip.clone(),
            channel,
            volume: volume * gain,
            fade: cue.fade,
            pitch,
        },
        (Some((clip, volume)), None) => Action::Play {
            clip: clip.clone(),
            bus: cue.bus,
            volume: volume * gain,
            fade: cue.fade,
            pitch,
        },
        (None, Some(channel)) => Action::StopChannel {
            channel,
            fade: cue.fade,
        },
        (None, None) => Action::Stop {
            bus: cue.bus,
            fade: cue.fade,
        },
    }
}
