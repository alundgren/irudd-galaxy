//! The beat runtime: how a registered beat moves from dormant to active to complete.
//!
//! A pack registers [`BeatDef`]s (the `core` pack through Luau `G.beat`). Each tick
//! the script host asks [`next`] what a beat should do and turns the answer into
//! commands with [`commands`]; this module never runs Luau itself, so `test`
//! predicates come in as a callback. Beat state lives in `world.progress.beats`
//! keyed by stable ID, so it survives save, load and hot reload (a reload swaps the
//! definitions; the state stays).
//!
//! Facts written: `beat_started` when the preconditions hold, `quest_step` when the
//! next step starts (its `quest` is the beat ID), `beat_completed` with the rewards.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use galaxy_contract::progress::{BeatDef, Rewards};
use galaxy_core::History;

use crate::eval::{Eval, EvalError};
use crate::services::{fit_commands, grant_commands};
use crate::standing::Deeds;

/// What a beat does this tick.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transition {
    /// Its preconditions hold: it starts at its first step.
    Start,
    /// The current step is done and another follows (or the end state is awaited).
    Advance { to: u32 },
    /// Every step is done and the end state holds.
    Complete,
    /// Nothing to do but the goal line changed (the pack was reloaded).
    Refresh,
}

/// The goal line for a beat at `step`.
pub fn goal_at(def: &BeatDef, step: u32) -> &str {
    def.steps
        .get(step as usize)
        .map_or(def.goal.as_str(), |s| s.goal.as_str())
}

/// What `def` should do now. `tests` evaluates `G.test` predicates by index.
pub fn next(
    def: &BeatDef,
    eval: Eval<'_>,
    tests: &mut dyn FnMut(u32) -> Result<bool, EvalError>,
) -> Result<Option<Transition>, EvalError> {
    let state = eval.world.progress.beats.get(&def.id);
    let Some(state) = state else {
        return Ok(eval
            .for_beat(None)
            .holds(&def.requires, tests)?
            .then_some(Transition::Start));
    };
    if state.status == BeatStatus::Completed {
        return Ok(None);
    }
    let eval = eval.for_beat(Some(state));
    let steps = def.steps.len() as u32;
    let step = state.step.min(steps);
    if let Some(s) = def.steps.get(step as usize)
        && !eval.holds(&s.done, tests)?
    {
        return Ok((state.goal != s.goal).then_some(Transition::Refresh));
    }
    if step < steps {
        // The current step is done.
        if step + 1 < steps {
            return Ok(Some(Transition::Advance { to: step + 1 }));
        }
        return Ok(Some(if eval.holds(&def.done, tests)? {
            Transition::Complete
        } else {
            Transition::Advance { to: steps }
        }));
    }
    if eval.holds(&def.done, tests)? {
        return Ok(Some(Transition::Complete));
    }
    Ok((state.goal != def.goal).then_some(Transition::Refresh))
}

/// The commands for a transition: the beat's new state and its facts, plus rewards on
/// completion. `head` is the newest history fact (facts after it are "since").
pub fn commands(
    def: &BeatDef,
    world: &WorldState,
    history: &History,
    pack: &PackId,
    tick: Tick,
    t: Transition,
) -> Vec<Command> {
    let head = history.last_fact_id();
    let put = |state: BeatState| Command::PutBeat {
        id: def.id.clone(),
        value: Some(Box::new(state)),
    };
    let step_id = |i: u32| def.steps.get(i as usize).map(|s| s.id.clone());
    let current = world.progress.beats.get(&def.id);
    match (t, current) {
        (Transition::Start, _) => vec![
            put(BeatState {
                pack: pack.clone(),
                act: def.act,
                kind: def.kind,
                status: BeatStatus::Active,
                step: 0,
                step_id: step_id(0),
                goal: goal_at(def, 0).to_owned(),
                giver: def.giver.clone(),
                since: head,
                step_since: head,
                done_since: None,
                started: tick,
            }),
            Command::Emit {
                event: Event::BeatStarted {
                    beat: def.id.clone(),
                    act: def.act,
                },
            },
        ],
        (Transition::Advance { to }, Some(s)) => {
            let mut out = vec![put(BeatState {
                step: to,
                step_id: step_id(to),
                goal: goal_at(def, to).to_owned(),
                step_since: head,
                ..s.clone()
            })];
            if let Some(id) = step_id(to)
                && let Ok(quest) = QuestId::new(def.id.as_str())
            {
                out.push(Command::Emit {
                    event: Event::QuestStep { quest, step: id },
                });
            }
            out
        }
        (Transition::Complete, Some(s)) => {
            let mut out = vec![
                put(BeatState {
                    status: BeatStatus::Completed,
                    step: def.steps.len() as u32,
                    step_id: None,
                    done_since: head,
                    ..s.clone()
                }),
                Command::Emit {
                    event: Event::BeatCompleted {
                        beat: def.id.clone(),
                    },
                },
            ];
            out.extend(reward_commands(world, &def.id, &def.rewards));
            out
        }
        (Transition::Refresh, Some(s)) => vec![put(BeatState {
            goal: goal_at(def, s.step).to_owned(),
            ..s.clone()
        })],
        (_, None) => Vec::new(),
    }
}

/// A beat's rewards: credits, standing (as deeds named after the beat), flags and
/// gifted fittings.
pub fn reward_commands(world: &WorldState, beat: &BeatId, r: &Rewards) -> Vec<Command> {
    let mut out = Vec::new();
    if r.credits != 0 {
        out.push(Command::AdjustCredits { delta: r.credits });
    }
    let mut deeds = Deeds::new(world);
    let deed = AnyId::new(beat.as_str()).unwrap_or_else(|_| AnyId::from_static("beat.reward"));
    for (faction, delta) in &r.standing {
        deeds.deed(faction, &deed, *delta, &[]);
    }
    out.extend(deeds.commands);
    let mut seen = BTreeSet::new();
    for u in &r.fit {
        if world.player.fittings.contains(u) {
            continue;
        }
        if let Some(def) = world.progress.rules.upgrades.get(u) {
            out.extend(fit_commands(world, def, &mut seen));
        }
    }
    out.extend(grant_commands(world, &r.flags, &mut seen));
    out
}

/// A goal line for the HUD or the dockmaster's "anything for me?" list.
#[derive(Clone, Debug, PartialEq)]
pub struct GoalLine {
    pub beat: BeatId,
    pub act: u32,
    pub goal: String,
    pub giver: Option<NpcId>,
    pub step: Option<String>,
}

/// Active beats' goal lines, newest first. The HUD shows the first; the rest wait in
/// the dockmaster's list.
pub fn goal_lines(world: &WorldState) -> Vec<GoalLine> {
    let mut active: Vec<(&BeatId, &BeatState)> = world
        .progress
        .beats
        .iter()
        .filter(|(_, b)| b.status == BeatStatus::Active)
        .collect();
    active.sort_by(|a, b| b.1.started.cmp(&a.1.started).then(b.0.cmp(a.0)));
    active
        .into_iter()
        .map(|(id, b)| GoalLine {
            beat: id.clone(),
            act: b.act,
            goal: b.goal.clone(),
            giver: b.giver.clone(),
            step: b.step_id.clone(),
        })
        .collect()
}

/// The one goal line the HUD shows (the newest active beat's).
pub fn hud_goal(world: &WorldState) -> Option<GoalLine> {
    goal_lines(world).into_iter().next()
}

/// The act the journey is in: the highest act of any started beat (0 before any).
pub fn current_act(world: &WorldState) -> u32 {
    world
        .progress
        .beats
        .values()
        .map(|b| b.act)
        .max()
        .unwrap_or(0)
}

/// Problems with a beat as registered (reported by `galaxy pack check`).
pub fn validate(def: &BeatDef) -> Vec<String> {
    let mut out = Vec::new();
    let line = |what: &str, s: &str, out: &mut Vec<String>| {
        if s.trim().is_empty() {
            out.push(format!("{what} is empty"));
        } else if s.contains('\n') {
            out.push(format!("{what} must be one line"));
        } else if s.len() > 200 {
            out.push(format!("{what} is longer than 200 bytes"));
        }
    };
    if !def.id.as_str().starts_with("beat.") {
        out.push(format!("beat IDs start with `beat.` (got `{}`)", def.id));
    }
    line("the goal line", &def.goal, &mut out);
    let mut ids = BTreeSet::new();
    for s in &def.steps {
        if !ids.insert(s.id.as_str()) {
            out.push(format!("step `{}` is listed twice", s.id));
        }
        if s.id.is_empty()
            || !s
                .id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        {
            out.push(format!(
                "step IDs are lowercase words (`show`, `build`), got `{}`",
                s.id
            ));
        }
        line(&format!("step `{}`'s goal line", s.id), &s.goal, &mut out);
    }
    if def.steps.is_empty() && def.done == Requirement::Always {
        out.push("a beat needs steps or an end state (`done`)".into());
    }
    out
}

/// Every tunable and beat a requirement names, for `galaxy pack check`.
pub fn references(req: &Requirement, tunables: &mut BTreeSet<TunableId>, beats: &mut BTreeSet<BeatId>) {
    use galaxy_contract::progress::Num;
    use Requirement as R;
    let mut num = |n: &Num| {
        if let Num::Tunable { tunable } = n {
            tunables.insert(tunable.clone());
        }
    };
    match req {
        R::All(list) | R::Any(list) => {
            for r in list {
                references(r, tunables, beats);
            }
        }
        R::Not(r) => references(r, tunables, beats),
        R::BeatDone(b) | R::BeatStarted(b) | R::BeatActive(b) | R::BeatStep { beat: b, .. } => {
            beats.insert(b.clone());
        }
        R::Counter { at_least, .. }
        | R::Facts { at_least, .. }
        | R::Sum { at_least, .. }
        | R::Credits { at_least }
        | R::Cargo { at_least, .. }
        | R::HoldFree { at_least } => num(at_least),
        R::TunableIs { tunable, .. } => {
            tunables.insert(tunable.clone());
        }
        _ => {}
    }
}

/// [`references`] over a whole beat: its preconditions, steps and end state.
pub fn def_references(def: &BeatDef) -> (BTreeSet<TunableId>, BTreeSet<BeatId>) {
    let (mut t, mut b) = (BTreeSet::new(), BTreeSet::new());
    references(&def.requires, &mut t, &mut b);
    for s in &def.steps {
        references(&s.done, &mut t, &mut b);
    }
    references(&def.done, &mut t, &mut b);
    (t, b)
}
