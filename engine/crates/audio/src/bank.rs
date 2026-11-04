use crate::{Bus, Clip, mixer::MAX_CHANNELS};
use galaxy_api::Registry;
use galaxy_module::module::Module;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Component, Path};

fn one() -> f32 {
    1.0
}
fn event_fade() -> f32 {
    0.02
}
fn loop_crossfade() -> f32 {
    2.0
}
fn follow_ramp() -> f32 {
    0.1
}

/// Every key `audio.toml` takes: its table, the key, an example value as TOML and what it
/// does. `galaxy-gm fields audio` prints it; a test loads a table made of every example.
pub const FILE_KEYS: &[(&str, &str, &str, &str)] = &[
    (
        "",
        "crossfade",
        "1.0",
        "seconds a place change crossfades its ambience, 0..60; default 1",
    ),
    (
        "sounds.<name>",
        "file",
        "\"sounds/hum.wav\"",
        "WAV, Ogg Vorbis or MP3 inside the module, written with /",
    ),
    ("sounds.<name>", "volume", "0.5", "0..1; default 1"),
    (
        "sounds.<name>",
        "loop_crossfade",
        "0.0",
        "seconds a loop's end fades into its start, 0..60; default 2, off for clips under four fades; 0 for prepared loops",
    ),
    (
        "places",
        "<place>",
        "\"hum\"",
        "a place of this module (local or full ID) and its ambience sound",
    ),
    (
        "[[events]]",
        "event",
        "\"beam_on\"",
        "the event kind (local, or full such as engine.start) that cues it",
    ),
    (
        "[[events]]",
        "sound",
        "\"hum\"",
        "the sound it plays; left out with stop = true",
    ),
    (
        "[[events]]",
        "bus",
        "\"sound\"",
        "sound (a one-shot, or a loop with channel) or music (a loop that replaces the music); default sound",
    ),
    (
        "[[events]]",
        "fade",
        "0.2",
        "seconds it fades in, or out with stop, 0..60; default 0.02",
    ),
    (
        "[[events]]",
        "stop",
        "false",
        "fades out the bus, or only the channel when channel is set, instead of playing",
    ),
    (
        "[[events]]",
        "channel",
        "\"beam\"",
        "loops the sound on this named channel of the sound bus until a stop on it; the same sound again keeps playing, another crossfades",
    ),
    (
        "[[events]]",
        "vary_pitch",
        "0.05",
        "each cue plays at a random rate within 1 +/- this, 0..0.5, seeded by the fact's tick and entity; default 0",
    ),
    (
        "[[events]]",
        "vary_gain",
        "0.2",
        "each cue plays at a random gain from 1 - this to 1, 0..1, seeded the same way; default 0",
    ),
    (
        "[[follow]]",
        "channel",
        "\"beam\"",
        "the channel whose loop follows the value; or ambience = true",
    ),
    (
        "[[follow]]",
        "ambience",
        "false",
        "the place ambience follows the value instead of a channel",
    ),
    (
        "[[follow]]",
        "entity",
        "\"ship\"",
        "the entity it reads (local or full ID)",
    ),
    (
        "[[follow]]",
        "component",
        "\"engine.motion\"",
        "with field: the component it reads (local or full ID)",
    ),
    (
        "[[follow]]",
        "field",
        "[\"vx\", \"vy\"]",
        "a number field, or a list of fields whose vector length is the value",
    ),
    (
        "[[follow]]",
        "distance_to",
        "\"beacon\"",
        "instead of component and field: the distance to this entity (engine.position)",
    ),
    (
        "[[follow]]",
        "from",
        "0.0",
        "the value where the curve starts; a missing value counts as from",
    ),
    (
        "[[follow]]",
        "to",
        "12.0",
        "the value where the curve ends; from above to runs it backwards",
    ),
    (
        "[[follow]]",
        "gain",
        "[0.4, 1.0]",
        "gain at from and at to, 0..1, times the cue's volume",
    ),
    (
        "[[follow]]",
        "pitch",
        "[0.9, 1.2]",
        "playback rate at from and at to, 0.25..4",
    ),
    (
        "[[follow]]",
        "power",
        "1.0",
        "curve shape: the position along from..to raised to this, 0.1..10; default 1 (straight)",
    ),
    (
        "[[follow]]",
        "ramp",
        "0.1",
        "seconds each change glides over, 0..10; default 0.1",
    ),
];

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct Definition {
    #[serde(default)]
    sounds: BTreeMap<String, Sound>,
    #[serde(default)]
    events: Vec<Event>,
    #[serde(default)]
    places: BTreeMap<String, String>,
    #[serde(default = "one")]
    crossfade: f32,
    #[serde(default)]
    follow: Vec<Follow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sound {
    file: String,
    #[serde(default = "one")]
    volume: f32,
    #[serde(default = "loop_crossfade")]
    loop_crossfade: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Event {
    event: String,
    #[serde(default)]
    sound: Option<String>,
    #[serde(default)]
    bus: Bus,
    #[serde(default)]
    stop: bool,
    #[serde(default = "event_fade")]
    fade: f32,
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    vary_pitch: f32,
    #[serde(default)]
    vary_gain: f32,
}

/// One field, or the length of the vector several fields make.
#[derive(Deserialize)]
#[serde(untagged)]
enum FieldList {
    One(String),
    Vector(Vec<String>),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Follow {
    #[serde(default)]
    channel: Option<String>,
    #[serde(default)]
    ambience: bool,
    entity: String,
    #[serde(default)]
    component: Option<String>,
    #[serde(default)]
    field: Option<FieldList>,
    #[serde(default)]
    distance_to: Option<String>,
    from: f32,
    to: f32,
    #[serde(default)]
    gain: Option<[f32; 2]>,
    #[serde(default)]
    pitch: Option<[f32; 2]>,
    #[serde(default = "one")]
    power: f32,
    #[serde(default = "follow_ramp")]
    ramp: f32,
}

#[derive(Clone)]
pub(crate) struct Cue {
    pub sound: Option<(Clip, f32)>,
    pub bus: Bus,
    pub fade: f32,
    /// A channel loop, by slot.
    pub channel: Option<u8>,
    pub vary_pitch: f32,
    pub vary_gain: f32,
}

/// What a follow reads from the world.
pub(crate) enum Source {
    Fields {
        component: String,
        fields: Vec<String>,
    },
    Distance(String),
}

/// A loop's gain and rate following a world value, already checked.
pub(crate) struct Curve {
    /// A channel slot, or `None` for the place ambience.
    pub target: Option<u8>,
    pub entity: String,
    pub source: Source,
    pub from: f32,
    pub to: f32,
    pub gain: Option<[f32; 2]>,
    pub pitch: Option<[f32; 2]>,
    pub power: f32,
    pub ramp: f32,
}

/// Loaded presentation assets. Errors are module-prefixed and safe to show as GM findings.
#[derive(Default)]
pub struct Bank {
    pub problems: Vec<String>,
    pub(crate) events: BTreeMap<String, Vec<Cue>>,
    pub(crate) places: BTreeMap<String, (String, Cue)>,
    pub(crate) follows: Vec<Curve>,
    /// Channel slots by full name (`<module>.<channel>`), in order of first use.
    channels: BTreeMap<String, u8>,
}

impl Bank {
    pub fn load<'a>(modules: impl IntoIterator<Item = &'a Module>) -> Self {
        let modules: Vec<_> = modules.into_iter().collect();
        let registry = Registry::new(&modules).ok();
        let mut bank = Self::default();
        for module in &modules {
            let Some(text) = module.files.get("audio.toml") else {
                continue;
            };
            let bad = |message: String| format!("{}: audio.toml: {message}", module.id());
            let def: Definition = match toml::from_str(text) {
                Ok(def) => def,
                Err(e) => {
                    bank.problems.push(bad(e.to_string()));
                    continue;
                }
            };
            if !seconds(def.crossfade) {
                bank.problems
                    .push(bad("crossfade must be finite, from 0 to 60 seconds".into()));
                continue;
            }
            let mut sounds = BTreeMap::new();
            for (name, sound) in def.sounds {
                let result = (|| {
                    if !galaxy_module::ids::valid_field(&name) {
                        return Err(galaxy_module::ids::name_rule(&name));
                    }
                    if !sound.volume.is_finite() || !(0.0..=1.0).contains(&sound.volume) {
                        return Err("volume must be finite, from 0 to 1".into());
                    }
                    if !seconds(sound.loop_crossfade) {
                        return Err("loop_crossfade must be finite, from 0 to 60 seconds".into());
                    }
                    // Keep asset names portable and within this module for authoring mistakes.
                    let path = Path::new(&sound.file);
                    if sound.file.is_empty()
                        || sound.file.contains(['\\', ':'])
                        || path
                            .components()
                            .any(|c| !matches!(c, Component::Normal(_)))
                    {
                        return Err(
                            "file must be a relative module path using /, without ..".into()
                        );
                    }
                    Clip::read(&module.manifest.dir.join(path))
                        .map(|clip| clip.with_loop_crossfade(sound.loop_crossfade))
                })();
                match result {
                    Ok(clip) => {
                        sounds.insert(name, (clip, sound.volume));
                    }
                    Err(e) => bank
                        .problems
                        .push(bad(format!("sound `{name}` ({}): {e}", sound.file))),
                }
            }
            for event in def.events {
                let kind = if event.event.contains('.') {
                    event.event.clone()
                } else {
                    module.full(&event.event)
                };
                if !seconds(event.fade) || event.bus == Bus::Ambience {
                    bank.problems.push(bad(format!("event `{kind}`: bus must be sound or music; fade must be from 0 to 60 seconds")));
                    continue;
                }
                if !(0.0..=0.5).contains(&event.vary_pitch)
                    || !(0.0..=1.0).contains(&event.vary_gain)
                {
                    bank.problems.push(bad(format!(
                        "event `{kind}`: vary_pitch must be from 0 to 0.5 and vary_gain from 0 to 1"
                    )));
                    continue;
                }
                if registry.as_ref().is_some_and(|r| !r.events.contains(&kind)) {
                    bank.problems
                        .push(bad(format!("event `{kind}` is not declared")));
                    continue;
                }
                let channel = match &event.channel {
                    None => None,
                    Some(name) if event.bus != Bus::Sound => {
                        bank.problems.push(bad(format!(
                            "event `{kind}`: channel `{name}` loops on the sound bus; leave bus out"
                        )));
                        continue;
                    }
                    Some(name) => match bank.channel(module, name) {
                        Ok(slot) => Some(slot),
                        Err(e) => {
                            bank.problems.push(bad(format!("event `{kind}`: {e}")));
                            continue;
                        }
                    },
                };
                let sound = if event.stop && event.sound.is_none() {
                    None
                } else if !event.stop
                    && event
                        .sound
                        .as_ref()
                        .is_some_and(|id| sounds.contains_key(id))
                {
                    event.sound.as_ref().and_then(|id| sounds.get(id)).cloned()
                } else {
                    bank.problems.push(bad(format!(
                        "event `{kind}` needs a declared sound, or stop = true without sound"
                    )));
                    continue;
                };
                bank.events.entry(kind).or_default().push(Cue {
                    sound,
                    bus: event.bus,
                    fade: event.fade,
                    channel,
                    vary_pitch: event.vary_pitch,
                    vary_gain: event.vary_gain,
                });
            }
            for (place, sound) in def.places {
                let place = if place.starts_with(&format!("{}.", module.id())) {
                    place
                } else {
                    module.full(&place)
                };
                if !place.starts_with(&format!("{}.", module.id()))
                    || !galaxy_module::ids::is_full_id(&place)
                {
                    bank.problems.push(bad(format!(
                        "place `{place}` must be in this module's namespace"
                    )));
                    continue;
                }
                let Some(clip) = sounds.get(&sound) else {
                    bank.problems.push(bad(format!(
                        "place `{place}` references unknown sound `{sound}`"
                    )));
                    continue;
                };
                bank.places.insert(
                    place,
                    (
                        module.full(&sound),
                        Cue {
                            sound: Some(clip.clone()),
                            bus: Bus::Ambience,
                            fade: def.crossfade,
                            channel: None,
                            vary_pitch: 0.0,
                            vary_gain: 0.0,
                        },
                    ),
                );
            }
            for follow in def.follow {
                match bank.curve(module, follow) {
                    Ok(curve) => bank.follows.push(curve),
                    Err(e) => bank.problems.push(bad(format!("follow: {e}"))),
                }
            }
        }
        bank
    }

    /// The slot of this module's channel `name`, made on first use.
    fn channel(&mut self, module: &Module, name: &str) -> Result<u8, String> {
        if !galaxy_module::ids::valid_field(name) {
            return Err(galaxy_module::ids::name_rule(name));
        }
        let full = module.full(name);
        if let Some(slot) = self.channels.get(&full) {
            return Ok(*slot);
        }
        if self.channels.len() >= MAX_CHANNELS {
            return Err(format!(
                "at most {MAX_CHANNELS} channels, across all modules"
            ));
        }
        let slot = self.channels.len() as u8;
        self.channels.insert(full, slot);
        Ok(slot)
    }

    fn curve(&self, module: &Module, f: Follow) -> Result<Curve, String> {
        let id = |name: &str| {
            if name.contains('.') {
                name.to_owned()
            } else {
                module.full(name)
            }
        };
        let target = match (&f.channel, f.ambience) {
            (Some(name), false) => Some(
                self.channels
                    .get(&module.full(name))
                    .copied()
                    .ok_or_else(|| {
                        format!("channel `{name}` is started by no [[events]] cue of this module")
                    })?,
            ),
            (None, true) => None,
            _ => return Err("give one of channel or ambience = true".into()),
        };
        let source = match (f.component, f.field, f.distance_to) {
            (Some(component), Some(field), None) => Source::Fields {
                component: id(&component),
                fields: match field {
                    FieldList::One(name) => vec![name],
                    FieldList::Vector(names) if !names.is_empty() => names,
                    FieldList::Vector(_) => return Err("field lists at least one field".into()),
                },
            },
            (None, None, Some(other)) => Source::Distance(id(&other)),
            _ => return Err("read component and field, or distance_to".into()),
        };
        if !f.from.is_finite() || !f.to.is_finite() || f.from == f.to {
            return Err("from and to must be finite and differ".into());
        }
        if f.gain.is_none() && f.pitch.is_none() {
            return Err("give gain, pitch or both".into());
        }
        if f.gain
            .is_some_and(|g| g.iter().any(|v| !(0.0..=1.0).contains(v)))
        {
            return Err("gain values must be from 0 to 1".into());
        }
        if f.pitch
            .is_some_and(|p| p.iter().any(|v| !(0.25..=4.0).contains(v)))
        {
            return Err("pitch values must be from 0.25 to 4".into());
        }
        if !(0.1..=10.0).contains(&f.power) || !(0.0..=10.0).contains(&f.ramp) {
            return Err("power must be from 0.1 to 10, and ramp from 0 to 10 seconds".into());
        }
        Ok(Curve {
            target,
            entity: id(&f.entity),
            source,
            from: f.from,
            to: f.to,
            gain: f.gain,
            pitch: f.pitch,
            power: f.power,
            ramp: f.ramp,
        })
    }
}

fn seconds(value: f32) -> bool {
    value.is_finite() && (0.0..=60.0).contains(&value)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every documented key is one the parser takes, with its example value.
    #[test]
    fn every_file_key_parses_with_its_example() {
        let mut text = String::new();
        for table in ["", "sounds.<name>", "places", "[[events]]", "[[follow]]"] {
            text.push_str(&match table {
                "" => String::new(),
                "sounds.<name>" => "[sounds.hum]\n".into(),
                t if t.starts_with('[') => format!("{t}\n"),
                t => format!("[{t}]\n"),
            });
            for (_, key, example, _) in FILE_KEYS.iter().filter(|k| k.0 == table) {
                let key = if *key == "<place>" { "first" } else { key };
                text.push_str(&format!("{key} = {example}\n"));
            }
        }
        let def: Definition = toml::from_str(&text).unwrap_or_else(|e| panic!("{e}\n{text}"));
        assert_eq!(def.events.len(), 1);
        assert_eq!(def.follow.len(), 1);
        assert!(
            FILE_KEYS.iter().all(
                |k| ["", "sounds.<name>", "places", "[[events]]", "[[follow]]"].contains(&k.0)
            )
        );
    }
}
