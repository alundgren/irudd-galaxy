use crate::bank::Cue;
use crate::{Action, Bank, Bus};
use galaxy_api::{WorldView, engine};

/// Presentation state only. Call after each fixed tick with just its new event kinds.
pub struct Scene {
    bank: Bank,
    ambience: Option<String>,
    fade: f32,
}

impl Scene {
    pub fn new(bank: Bank) -> Self {
        Self {
            bank,
            ambience: None,
            fade: 1.0,
        }
    }

    pub fn update<'a>(
        &mut self,
        world: &impl WorldView,
        events: impl IntoIterator<Item = &'a str>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        for event in events {
            for cue in self.bank.events.get(event).into_iter().flatten() {
                actions.push(action(cue));
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
                actions.push(action(cue));
            } else {
                actions.push(Action::Stop {
                    bus: Bus::Ambience,
                    fade: self.fade,
                });
            }
            self.ambience = name.map(str::to_owned);
        }
        actions
    }
}

fn action(cue: &Cue) -> Action {
    match &cue.sound {
        Some((clip, volume)) => Action::Play {
            clip: clip.clone(),
            bus: cue.bus,
            volume: *volume,
            fade: cue.fade,
        },
        None => Action::Stop {
            bus: cue.bus,
            fade: cue.fade,
        },
    }
}
