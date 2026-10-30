//! The sim kernel: fixed-step ticks, real pause, the command buffer, history, events,
//! counters and rollback by cause.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};

use crate::apply::{Staged, apply, restore};
use crate::bus::{EventBus, Subscriber};
use crate::history::History;
use crate::invariants::{self, Invariant};
use crate::rng::RngBank;
use crate::system::{System, TickCtx};

/// Engine bookkeeping that is saved with the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelState {
    pub rng: RngBank,
    /// Batches submitted between ticks, applied at the end of the next tick.
    pub pending: Vec<CommandBatch>,
    /// Events committed at the end of the last tick; systems see them next tick.
    pub last_events: Vec<EventEnvelope>,
    /// Next cause sequence number.
    pub next_cause: u64,
    /// Journaled causes whose rollback batch has committed. They are never inverted
    /// again, so a repeated or overlapping rollback can't apply the inverse twice.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub rolled_back: BTreeSet<CauseId>,
}

impl KernelState {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: RngBank::new(seed),
            pending: Vec::new(),
            last_events: Vec::new(),
            next_cause: 1,
            rolled_back: BTreeSet::new(),
        }
    }
}

/// Everything a save holds. The determinism hash covers all of it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SimState {
    pub world: WorldState,
    pub kernel: KernelState,
    pub history: History,
    /// Undo records of journaled causes (scripts, fast-model decisions).
    pub journal: Vec<JournalEntry>,
}

impl SimState {
    /// A fresh state around a newly built world.
    pub fn new(world: WorldState) -> Self {
        let seed = world.meta.seed;
        Self {
            world,
            kernel: KernelState::new(seed),
            history: History::default(),
            journal: Vec::new(),
        }
    }
}

/// What one tick did.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct TickReport {
    pub tick: Tick,
    pub applied: usize,
    pub rejected: Vec<Rejection>,
    pub events: usize,
}

/// The result of queueing a rollback.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RollbackPlan {
    /// Inverse batches queued for the next tick.
    pub batches: usize,
    /// Writes that can't be undone exactly and need an in-world compensation.
    pub compensate: Vec<String>,
}

/// A running simulation.
pub struct Sim {
    state: SimState,
    systems: Vec<Box<dyn System>>,
    invariants: Vec<Box<dyn Invariant>>,
    bus: EventBus,
    paused: bool,
}

impl Sim {
    /// A sim with the built-in invariants and no systems.
    pub fn new(state: SimState) -> Self {
        Self {
            state,
            systems: Vec::new(),
            invariants: invariants::builtin(),
            bus: EventBus::default(),
            paused: false,
        }
    }

    /// Appends a system; systems run in the order added.
    pub fn add_system(&mut self, system: Box<dyn System>) -> &mut Self {
        self.systems.push(system);
        self
    }

    pub fn with_systems(mut self, systems: impl IntoIterator<Item = Box<dyn System>>) -> Self {
        self.systems.extend(systems);
        self
    }

    pub fn add_invariant(&mut self, invariant: Box<dyn Invariant>) -> &mut Self {
        self.invariants.push(invariant);
        self
    }

    pub fn system_names(&self) -> Vec<&'static str> {
        self.systems.iter().map(|s| s.name()).collect()
    }

    pub fn world(&self) -> &WorldState {
        &self.state.world
    }

    pub fn history(&self) -> &History {
        &self.state.history
    }

    pub fn journal(&self) -> &[JournalEntry] {
        &self.state.journal
    }

    pub fn state(&self) -> &SimState {
        &self.state
    }

    pub fn into_state(self) -> SimState {
        self.state
    }

    pub fn tick(&self) -> Tick {
        self.state.world.meta.tick
    }

    /// Stable hash of the whole state, see [`crate::hash`].
    pub fn hash(&self) -> String {
        crate::hash::state_hash(&self.state)
    }

    // --- Pause ---

    /// Real pause: no ticks advance and nothing changes until resumed.
    pub fn set_paused(&mut self, paused: bool) {
        self.paused = paused;
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    // --- Events for code outside the tick ---

    pub fn subscribe(&mut self, name: impl Into<String>) -> Subscriber {
        self.bus.subscribe(name)
    }

    pub fn drain_events(&mut self, sub: &Subscriber) -> Vec<EventEnvelope> {
        self.bus.drain(sub)
    }

    // --- Writes ---

    /// Queues a batch from outside the tick (CLI, game-master bridge). It applies at the
    /// end of the next tick, after nothing else; systems' batches follow it.
    pub fn submit(&mut self, batch: CommandBatch) {
        if !batch.commands.is_empty() {
            self.state.kernel.pending.push(batch);
        }
    }

    /// Advances one tick with this input. Returns `None` while paused.
    pub fn step(&mut self, input: &PlayerInput) -> Option<TickReport> {
        if self.paused {
            return None;
        }
        let tick = self.state.world.meta.tick.next();
        let mut batches = std::mem::take(&mut self.state.kernel.pending);
        {
            let s = &mut self.state;
            let mut ctx = TickCtx {
                tick,
                world: &s.world,
                history: &s.history,
                input,
                events: &s.kernel.last_events,
                rng: &mut s.kernel.rng,
                out: &mut batches,
                system: "",
            };
            for system in &self.systems {
                ctx.system = system.name();
                system.step(&mut ctx);
            }
        }
        self.state.world.meta.tick = tick;

        let mut report = TickReport {
            tick,
            ..TickReport::default()
        };
        let mut emitted = Vec::new();
        for batch in batches {
            match self.apply_batch(batch, tick) {
                Ok(mut events) => {
                    report.applied += 1;
                    emitted.append(&mut events);
                }
                Err(rejection) => report.rejected.push(*rejection),
            }
        }
        report.events = emitted.len();
        self.bus.publish(&emitted);
        self.state.kernel.last_events = emitted;
        Some(report)
    }

    /// Runs `ticks` ticks with the same input. Stops early (and returns the count run)
    /// if paused.
    pub fn run(&mut self, ticks: u64, input: &PlayerInput) -> u64 {
        let mut n = 0;
        while n < ticks && self.step(input).is_some() {
            n += 1;
        }
        n
    }

    /// Applies one batch atomically: all commands and invariants pass, or nothing
    /// changes. On commit, records the cause, keeps undo records for journaled
    /// causes, appends facts and bumps counters.
    fn apply_batch(
        &mut self,
        batch: CommandBatch,
        tick: Tick,
    ) -> Result<Vec<EventEnvelope>, Box<Rejection>> {
        let reject = |command, reason| {
            Err(Box::new(Rejection {
                tick,
                cause: batch.cause.clone(),
                command,
                reason,
            }))
        };
        if let CauseKind::Rollback { of } = &batch.cause.kind
            && let Some(done) = of
                .iter()
                .find(|c| self.state.kernel.rolled_back.contains(c))
        {
            return reject(None, format!("{done} is already rolled back"));
        }
        if let Some((i, e)) = batch
            .commands
            .iter()
            .enumerate()
            .find_map(|(i, cmd)| crate::finite::check(cmd).err().map(|e| (i, e)))
        {
            return reject(Some(i), format!("numbers must be finite: {e}"));
        }

        let world = &mut self.state.world;
        let mut undo = Vec::with_capacity(batch.commands.len());
        let mut staged = Staged::default();
        let mut failure = None;
        for (i, cmd) in batch.commands.iter().enumerate() {
            match apply(world, cmd, &mut staged) {
                Ok(u) => undo.push(u),
                Err(e) => {
                    failure = Some((Some(i), e.to_string()));
                    break;
                }
            }
        }
        if failure.is_none() {
            failure = self
                .invariants
                .iter()
                .find_map(|inv| inv.check(world).err().map(|e| (inv.name(), e)))
                .map(|(name, e)| (None, format!("invariant `{name}`: {e}")));
        }
        if let Some((command, reason)) = failure {
            revert(world, &undo);
            return reject(command, reason);
        }

        let s = &mut self.state;
        if let CauseKind::Rollback { of } = &batch.cause.kind {
            s.kernel.rolled_back.extend(of.iter().copied());
        }
        let cause_id = CauseId(s.kernel.next_cause);
        s.kernel.next_cause += 1;
        let journaled = batch.cause.kind.journaled();
        if journaled || !staged.events.is_empty() || !staged.facts.is_empty() {
            s.history.record_cause(RecordedCause {
                id: cause_id,
                tick,
                cause: batch.cause,
            });
        }
        if journaled {
            s.journal.push(JournalEntry {
                tick,
                cause: cause_id,
                undo,
            });
        }
        let mut envelopes = Vec::with_capacity(staged.events.len());
        for event in staged.events {
            bump_counters(&mut s.world, &event);
            let fact = s.history.append(
                tick,
                event.kind().fact_kind(),
                cause_id,
                event.subjects(),
                event.amount(),
                FactPayload::Event(event.clone()),
            );
            envelopes.push(EventEnvelope {
                tick,
                fact,
                cause: cause_id,
                event,
            });
        }
        for draft in staged.facts {
            s.history.append(
                tick,
                draft.kind,
                cause_id,
                draft.subjects,
                draft.amount,
                FactPayload::Note(draft.note),
            );
        }
        Ok(envelopes)
    }

    // --- Rollback ---

    /// Causes matching a filter, e.g. every write by one pack revision.
    pub fn causes_where(&self, mut f: impl FnMut(&RecordedCause) -> bool) -> Vec<CauseId> {
        self.state
            .history
            .causes()
            .values()
            .filter(|c| f(c))
            .map(|c| c.id)
            .collect()
    }

    /// Queues the inverse of every journaled write by these causes, newest first, as
    /// batches caused by a rollback. Each original batch is undone atomically by its
    /// own batch, `Rollback { of: [that cause] }`. Inverse writes restore the earlier
    /// values, so later changes to the same fields are overwritten; compensate-only
    /// writes are listed for an in-world compensation.
    ///
    /// Each cause is rolled back at most once: causes already rolled back, or with a
    /// rollback still pending, are skipped. Pending rollbacks are re-ordered together
    /// with the new ones, newest cause first. `RollbackPlan` counts only the batches
    /// for causes newly requested by this call. A committed batch with a `Rollback` cause
    /// marks every cause in its `of` as rolled back, and a later batch rolling back any
    /// of them again is rejected. A rejected rollback can be queued again.
    pub fn queue_rollback(&mut self, causes: &[CauseId], summary: &str) -> RollbackPlan {
        // Rollback batches still pending are rebuilt together with the new request, so
        // every inverse runs newest cause first even when requests overlap (rolling back
        // A, then A+B, must undo B before A). The rebuilt group takes the place of the
        // first pending rollback, so it never moves past batches queued after it.
        let mut summaries: BTreeMap<CauseId, String> = BTreeMap::new();
        for b in &self.state.kernel.pending {
            if let CauseKind::Rollback { of } = &b.cause.kind {
                for c in of {
                    summaries
                        .entry(*c)
                        .or_insert_with(|| b.cause.summary.clone());
                }
            }
        }
        let mut fresh = BTreeSet::new();
        for c in causes {
            if !self.state.kernel.rolled_back.contains(c) && !summaries.contains_key(c) {
                summaries.insert(*c, summary.to_owned());
                fresh.insert(*c);
            }
        }
        let mut plan = RollbackPlan::default();
        if fresh.is_empty() {
            // Nothing new: leave the queue exactly as it is.
            return plan;
        }
        let mut batches = Vec::new();
        for entry in self
            .state
            .journal
            .iter()
            .rev()
            .filter(|e| summaries.contains_key(&e.cause))
        {
            let mut batch = CommandBatch::new(Cause {
                kind: CauseKind::Rollback {
                    of: vec![entry.cause],
                },
                summary: summaries[&entry.cause].clone(),
                cites: Vec::new(),
            });
            for u in entry.undo.iter().rev() {
                match u {
                    // A check has nothing to undo, and the world it held in is gone.
                    UndoRecord::Invert { command } if command.is_expectation() => {}
                    UndoRecord::Invert { command } => batch.push(command.clone()),
                    UndoRecord::Compensate { note } if fresh.contains(&entry.cause) => {
                        plan.compensate.push(format!("{}: {note}", entry.cause))
                    }
                    UndoRecord::Compensate { .. } => {}
                }
            }
            if !batch.commands.is_empty() {
                if fresh.contains(&entry.cause) {
                    plan.batches += 1;
                }
                batches.push(batch);
            }
        }
        let pending = std::mem::take(&mut self.state.kernel.pending);
        let at = pending
            .iter()
            .position(|b| matches!(b.cause.kind, CauseKind::Rollback { .. }));
        let mut keep: Vec<CommandBatch> = pending
            .into_iter()
            .filter(|b| !matches!(b.cause.kind, CauseKind::Rollback { .. }))
            .collect();
        let at = at.unwrap_or(keep.len());
        keep.splice(at..at, batches);
        self.state.kernel.pending = keep;
        plan
    }
}

/// Undoes already-applied commands, newest first. Each inverse restores captured
/// values exactly, without the forward checks: a batch can leave a temporarily dangling
/// reference behind that only the invariants catch.
fn revert(world: &mut WorldState, undo: &[UndoRecord]) {
    for u in undo.iter().rev() {
        if let UndoRecord::Invert { command } = u {
            restore(world, command).expect("engine bug: inverting a just-applied command failed");
        }
    }
}

/// Player-owned activity moves the player, station and region counters.
fn bump_counters(world: &mut WorldState, event: &Event) {
    let ship = event.ship().and_then(|id| world.ships.get(id));
    let players = match (event.ship(), ship) {
        (Some(_), Some(s)) => s.owner == Owner::Player,
        (Some(_), None) => false,
        (None, _) => event.is_player_event(),
    };
    if !players {
        return;
    }
    let subjects = event.subjects();
    let station = subjects.station.clone();
    let region = station
        .as_ref()
        .and_then(|s| world.stations.get(s))
        .map(|s| s.region.clone())
        .or(subjects.region)
        .or_else(|| {
            let rock = world.rocks.get(subjects.rock.as_ref()?)?;
            Some(world.fields.get(&rock.field)?.region.clone())
        })
        .or_else(|| ship.map(|s| s.region.clone()));
    let counters = &mut world.counters;
    for (kind, by) in event.counter_bumps() {
        counters.bump(kind, &CounterScope::Player, by);
        if let Some(station) = &station {
            counters.bump(
                kind,
                &CounterScope::Station {
                    station: station.clone(),
                },
                by,
            );
        }
        if let Some(region) = &region {
            counters.bump(
                kind,
                &CounterScope::Region {
                    region: region.clone(),
                },
                by,
            );
        }
    }
}
