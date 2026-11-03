//! `check` presses the screens' own buttons and targets, so panels and lists that scripts
//! open are checked with everything else on screen, name signs included.
use crate::View;
use galaxy_api::{
    command::{Batch, Cause, Command},
    engine::*,
};
use galaxy_render::glam::Vec2;
use galaxy_sim::{
    Input, Session, World,
    scenario::{Click, ClickTarget},
};
use serde_json::{Value, json};
use std::collections::{BTreeSet, VecDeque};

/// Presses in a row from each start, and distinct states kept per start.
pub const PRESS_DEPTH: usize = 2;
pub const PRESS_STATES: usize = 32;

/// What tells states apart: the screen, and what press handlers changed.
#[derive(Default)]
struct Seen {
    /// Screens already checked: an equal screen reached another way has the same problems.
    screens: BTreeSet<Vec<String>>,
    /// Screen and script state together: a press that changes only a script's state (a
    /// page counter, a selection) leads somewhere new on the next press.
    states: BTreeSet<(Vec<String>, Vec<String>)>,
}

struct State {
    world: World,
    presses: Vec<String>,
    inputs: Vec<(String, Input)>,
}

impl View {
    /// From the first tick, and with the player in each place, press every visible
    /// button and `ui.target` (talk bubbles excepted: talk has its own check) up to
    /// [`PRESS_DEPTH`] presses deep, running the module's handlers and landing any trip
    /// they start. Each new screen state is checked silent, like a life pose: screen
    /// readability and life signs against every panel. Returns findings for `module`
    /// and a summary per start; the session's world is left as it was.
    pub fn check_presses(
        &mut self,
        session: &mut Session,
        module: &str,
        size: Vec2,
    ) -> (Vec<String>, Vec<Value>) {
        let original = session.world.clone();
        let hovers = self.hover_checks.take();
        let mut starts = vec![(String::new(), original.clone())];
        if let Some((player, _)) = galaxy_sim::travel::player(&original) {
            let player = player.to_owned();
            for (place, c) in &original.entities {
                if !c.contains_key(PLACE) {
                    continue;
                }
                let mut staged = original.clone();
                let moved = staged.apply(
                    &Batch {
                        cause: Cause::Engine {
                            service: "press-check".into(),
                        },
                        commands: vec![Command::Move {
                            entity: player.clone(),
                            place: place.clone(),
                            spot: None,
                            state: None,
                        }],
                    },
                    session.registry(),
                );
                if moved.is_err() {
                    continue;
                }
                if let Some(t) = staged.entities.get_mut(&player).unwrap().get_mut(TRAVEL) {
                    t.fields.insert("active".into(), false.into());
                }
                galaxy_sim::services::ground(&mut staged);
                starts.push((place.clone(), staged));
            }
        }
        let mut findings = Vec::new();
        let mut summary = Vec::new();
        // Shared by all starts: a place start equal to the first state adds nothing.
        let mut seen = Seen::default();
        for (start, world) in starts {
            let before = seen.states.len();
            let mut truncated = false;
            let mut queue = VecDeque::new();
            let (first, problems) =
                self.press_state(world, &original, vec![], module, size, &mut seen);
            if problems.is_some() {
                queue.push_back(first);
            }
            while let Some(state) = queue.pop_front() {
                if state.presses.len() >= PRESS_DEPTH {
                    continue;
                }
                for (label, input) in &state.inputs {
                    let mut presses = state.presses.clone();
                    presses.push(label.clone());
                    let world = match press(session, &state.world, input) {
                        Ok(w) => w,
                        Err(e) => {
                            findings.push(format!(
                                "{}pressing {}: {e}",
                                at(&start),
                                presses.join(" > ")
                            ));
                            continue;
                        }
                    };
                    if seen.states.len() - before >= PRESS_STATES {
                        truncated = true;
                        break;
                    }
                    let (next, problems) =
                        self.press_state(world, &original, presses, module, size, &mut seen);
                    let Some(problems) = problems else {
                        continue;
                    };
                    let path = next.presses.join(" > ");
                    findings.extend(
                        problems
                            .into_iter()
                            .map(|p| format!("{}after pressing {path}: {p}", at(&start))),
                    );
                    queue.push_back(next);
                }
            }
            summary.push(json!({
                "place": if start.is_empty() { Value::Null } else { start.into() },
                "states": seen.states.len() - before,
                "truncated": truncated,
            }));
        }
        session.world = original;
        self.hover_checks = hovers;
        findings.sort();
        findings.dedup();
        (findings, summary)
    }

    /// Lays a state out silent. A state already seen gives `None`: it is not pressed
    /// again. A new state whose screen was already seen gives no problems, but its
    /// buttons are pressed.
    fn press_state(
        &mut self,
        world: World,
        original: &World,
        presses: Vec<String>,
        module: &str,
        size: Vec2,
        seen: &mut Seen,
    ) -> (State, Option<Vec<String>>) {
        let mut silent = world.clone();
        for c in silent.entities.values_mut() {
            if let Some(t) = c.get_mut(TALK) {
                t.fields.insert("active".into(), false.into());
            }
        }
        self.before_step(&silent);
        self.scene(&silent, 1.0, size, 0.0);
        let mut signature: Vec<String> = self
            .screens
            .all()
            .iter()
            .map(|b| {
                let w = &b.widget;
                format!("{} {:?} {:?} {:?}", w.path, w.text, w.resting, b.rect)
            })
            .collect();
        signature.push(format!("{:?}", galaxy_sim::travel::player(&silent)));
        signature.sort();
        let new = seen
            .states
            .insert((signature.clone(), script_state(&world, original)));
        let problems = new.then(|| {
            if !seen.screens.insert(signature) {
                return vec![];
            }
            self.screens
                .problems
                .iter()
                .filter(|p| {
                    p.starts_with(&format!("{module}:")) || p.starts_with(&format!("{module}."))
                })
                .cloned()
                .collect()
        });
        let mut inputs = Vec::new();
        if new {
            let tick = silent.tick + 1;
            for b in self.screens.all() {
                let w = &b.widget;
                if w.on.is_none() || !matches!(w.kind.as_str(), "button" | "target") {
                    continue;
                }
                let input = if w.kind == "target" {
                    self.bot_target_click(&silent, &w.path, tick)
                        .and_then(Result::ok)
                } else {
                    self.screens
                        .bot_click(&Click {
                            tick,
                            target: ClickTarget::Widget(w.path.clone()),
                        })
                        .ok()
                };
                if let Some(input) = input.filter(|i| i.source == "ui") {
                    inputs.push((format!("`{}`", w.id.as_deref().unwrap_or(&w.path)), input));
                }
            }
        }
        (
            State {
                world,
                presses,
                inputs,
            },
            problems,
        )
    }
}

/// The state scripts made, apart from the cadence: every component a module's event
/// handler last changed to something other than the start world has, and every component
/// of the start world now gone. Cadence and engine services change things on their own
/// clock, so they would split states that a press did not; a toggle pressed twice is
/// back where it started.
fn script_state(world: &World, original: &World) -> Vec<String> {
    let mut out = Vec::new();
    for (id, components) in &world.entities {
        for (name, c) in components {
            let by_handler = world.causes.get(c.cause as usize).is_some_and(
                |r| matches!(&r.cause, Cause::Script { handler, .. } if handler != "cadence"),
            );
            if by_handler && original.component(id, name).map(|o| &o.fields) != Some(&c.fields) {
                out.push(format!("{id}/{name} {}", Value::Object(c.fields.clone())));
            }
        }
    }
    for (id, components) in &original.entities {
        for name in components.keys() {
            if world.component(id, name).is_none() {
                out.push(format!("{id}/{name} gone"));
            }
        }
    }
    out
}

fn at(start: &str) -> String {
    if start.is_empty() {
        String::new()
    } else {
        format!("place `{start}`, ")
    }
}

/// One press in `world`, through the module's handlers, then any trip it started lands
/// (as play records it) and one quiet tick lets follow-on events run.
/// A handler that fails on the press is a finding too.
fn press(session: &mut Session, world: &World, input: &Input) -> Result<World, String> {
    session.world = world.clone();
    let from = session.world.facts.len();
    session.step(std::slice::from_ref(input))?;
    for _ in 0..4 {
        let Some(arrived) = galaxy_sim::travel::arrival(&session.world, true) else {
            break;
        };
        session.step(&[arrived])?;
    }
    session.step(&[])?;
    if let Some(f) = session.world.facts[from..]
        .iter()
        .find(|f| f.kind == SCRIPT_FAILED)
    {
        return Err(format!(
            "script failed: {}",
            serde_json::Value::Object(f.payload.clone())
        ));
    }
    Ok(session.world.clone())
}
