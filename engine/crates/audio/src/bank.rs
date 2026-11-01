use crate::{Bus, Clip};
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sound {
    file: String,
    #[serde(default = "one")]
    volume: f32,
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
}

#[derive(Clone)]
pub(crate) struct Cue {
    pub sound: Option<(Clip, f32)>,
    pub bus: Bus,
    pub fade: f32,
}

/// Loaded presentation assets. Errors are module-prefixed and safe to show as GM findings.
#[derive(Default)]
pub struct Bank {
    pub problems: Vec<String>,
    pub(crate) events: BTreeMap<String, Vec<Cue>>,
    pub(crate) places: BTreeMap<String, (String, Cue)>,
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
                if registry.as_ref().is_some_and(|r| !r.events.contains(&kind)) {
                    bank.problems
                        .push(bad(format!("event `{kind}` is not declared")));
                    continue;
                }
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
                        },
                    ),
                );
            }
        }
        bank
    }
}

fn seconds(value: f32) -> bool {
    value.is_finite() && (0.0..=60.0).contains(&value)
}
