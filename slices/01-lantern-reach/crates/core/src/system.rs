//! The extension point for workstream crates.
//!
//! A [`System`] runs once per tick, in registration order. It reads the world as it
//! was at the end of the previous tick, plus the player input and the events that
//! tick committed, and submits command batches. All batches apply together at the end
//! of the tick, so systems never see each other's writes within a tick.
//!
//! Systems must be stateless (`&self`): anything that must survive a save lives in the
//! world stores, so save -> load -> continue matches an uninterrupted run.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;

use crate::history::History;
use crate::invariants::Invariant;
use crate::rng::{RngBank, SimRng};

pub trait System {
    /// Stable name. It keys the system's RNG stream, so renaming one changes the run.
    fn name(&self) -> &'static str;
    fn step(&self, ctx: &mut TickCtx<'_>);
}

/// What a system sees and can do during one tick.
pub struct TickCtx<'a> {
    /// The tick being simulated.
    pub tick: Tick,
    pub world: &'a WorldState,
    pub history: &'a History,
    pub input: &'a PlayerInput,
    /// Events committed at the end of the previous tick.
    pub events: &'a [EventEnvelope],
    /// This sim's invariants (built-in and added), for [`TickCtx::preview`].
    pub(crate) invariants: &'a [Box<dyn Invariant>],
    pub(crate) rolled_back: &'a BTreeSet<CauseId>,
    pub(crate) rng: &'a mut RngBank,
    pub(crate) out: &'a mut Vec<CommandBatch>,
    pub(crate) system: &'static str,
}

impl TickCtx<'_> {
    /// Queues a batch to apply at the end of this tick.
    pub fn submit(&mut self, batch: CommandBatch) {
        if !batch.commands.is_empty() {
            self.out.push(batch);
        }
    }

    /// Batches queued so far this tick, in the order they will apply: those submitted
    /// between ticks, then earlier systems'. Lets a system see a pending change (a
    /// rollback, a retirement) before it acts on the old world.
    pub fn queued(&self) -> &[CommandBatch] {
        self.out
    }

    /// The world as the kernel will have it when a batch submitted now applies: the
    /// tick's start plus every batch queued so far that it would commit (counters
    /// included, no history). Start [`TickCtx::preview`] from here.
    pub fn projected(&self) -> WorldState {
        let mut world = self.world.clone();
        world.meta.tick = self.tick;
        let mut rolled_back = self.rolled_back.clone();
        for batch in self.out.iter() {
            // A batch the kernel would reject leaves the world as it was.
            let _ = crate::sim::project(&mut world, batch, &mut rolled_back, self.invariants);
        }
        world
    }

    /// `world` after `batch`, decided the way the kernel decides at the end of the tick
    /// (rollback rules, finite numbers, every command, every invariant of this sim), or
    /// the reason it would reject the batch.
    pub fn preview(&self, world: &WorldState, batch: &CommandBatch) -> Result<WorldState, String> {
        let mut next = world.clone();
        let mut rolled_back = self.rolled_back.clone();
        crate::sim::project(&mut next, batch, &mut rolled_back, self.invariants)?;
        Ok(next)
    }

    /// This system's own random stream.
    pub fn rng(&mut self) -> &mut SimRng {
        self.rng.stream(self.system)
    }

    /// A named sub-stream, e.g. one per NPC.
    pub fn rng_named(&mut self, name: &str) -> &mut SimRng {
        let key = format!("{}/{name}", self.system);
        self.rng.stream(&key)
    }

    /// The name of the system currently running.
    pub fn system(&self) -> &'static str {
        self.system
    }
}
