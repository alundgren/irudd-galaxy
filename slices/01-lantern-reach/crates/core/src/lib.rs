//! `galaxy-core`: the headless, deterministic sim kernel.
//!
//! - [`Sim`]: fixed-step ticks (counted, never clocked), real pause, and the command
//!   buffer applied atomically per batch at the end of each tick.
//! - [`System`]: the extension point. Workstream crates (`ship`, `economy`, `script`
//!   ...) implement it and register with [`Sim::add_system`].
//! - [`apply`](apply::apply): the one write path, with undo records.
//! - [`History`]: the append-only fact log with `count`/`find`/`last`.
//! - [`EventBus`](bus::EventBus), activity counters, invariants, seeded RNG streams.
//! - [`load_world`]: new game from the authored TOML data.
//! - [`Save`]: one SQLite file per save; [`state_hash`] for determinism checks.
//!
//! No graphics, audio, network or wall-clock access in here.

pub mod apply;
pub mod bus;
pub mod finite;
pub mod hash;
pub mod history;
pub mod invariants;
pub mod load;
mod load_progress;
pub mod placeholder;
pub mod rng;
pub mod save;
pub mod sim;
pub mod step;
pub mod system;

use std::path::Path;

pub use galaxy_contract as contract;
pub use galaxy_contract::world::WorldState as World;
pub use hash::state_hash;
pub use history::History;
pub use load::{
    CRAFT_BACKFILL, DataError, FIT_BACKFILL, PACING_BACKFILL, craft_backfill, fit_backfill,
    load_world, pacing_backfill,
};
pub use save::{Save, SaveError, fork};
pub use sim::{KernelState, RollbackPlan, Sim, SimState, TickReport};
pub use system::{System, TickCtx};

/// The kernel's own systems, in tick order (none so far).
///
/// Workstream crates depend on this one, so they can't be listed here: the app
/// composes the game from these plus `galaxy_ship::systems(..)` and the others.
/// [`placeholder::PlaceholderFlight`] stays only for the kernel's own tests.
pub fn default_systems() -> Vec<Box<dyn System>> {
    Vec::new()
}

/// A new game's state from authored data.
pub fn new_game(data_dir: &Path, seed: u64) -> Result<SimState, DataError> {
    Ok(SimState::new(load_world(data_dir, seed)?))
}

/// Queues what a save made before newer authored data lacks, as the game does on open:
/// [`pacing_backfill`]'s batch first, then [`craft_backfill`]'s, then
/// [`fit_backfill`]'s. Each skips itself if an earlier open already queued it and the
/// save was written before it applied. Returns how many batches it queued (none on new
/// games and up-to-date saves).
pub fn queue_backfill(sim: &mut Sim, data_dir: &Path) -> Result<usize, DataError> {
    Ok(queue_pacing_backfill(sim, data_dir)?
        + queue_craft_backfill(sim, data_dir)?
        + queue_fit_backfill(sim, data_dir)?)
}

/// Queues [`fit_backfill`]'s batch for `sim`'s world, unless an earlier open already
/// queued it. Returns how many it queued (0 or 1).
pub fn queue_fit_backfill(sim: &mut Sim, data_dir: &Path) -> Result<usize, DataError> {
    if queued(sim, FIT_BACKFILL) {
        return Ok(0);
    }
    let Some(batch) = fit_backfill(data_dir, sim.world())? else {
        return Ok(0);
    };
    sim.submit(batch);
    Ok(1)
}

/// Queues [`pacing_backfill`]'s batch for `sim`'s world, unless an earlier open already
/// queued it. Returns how many it queued (0 or 1).
pub fn queue_pacing_backfill(sim: &mut Sim, data_dir: &Path) -> Result<usize, DataError> {
    if queued(sim, PACING_BACKFILL) {
        return Ok(0);
    }
    let Some(batch) = pacing_backfill(data_dir, sim.world())? else {
        return Ok(0);
    };
    sim.submit(batch);
    Ok(1)
}

fn queued(sim: &Sim, system: &str) -> bool {
    sim.state().kernel.pending.iter().any(|b| {
        matches!(&b.cause.kind, contract::prelude::CauseKind::System { name } if name == system)
    })
}

/// Queues [`craft_backfill`]'s batches for `sim`'s world, unless an earlier open
/// already queued them and the save was written before they applied. Returns how many
/// it queued (none on new games and on saves that have the craft). The game calls it
/// on open, so they apply with the first step.
pub fn queue_craft_backfill(sim: &mut Sim, data_dir: &Path) -> Result<usize, DataError> {
    if queued(sim, CRAFT_BACKFILL) {
        return Ok(0);
    }
    let batches = craft_backfill(data_dir, sim.world())?;
    let n = batches.len();
    for batch in batches {
        sim.submit(batch);
    }
    Ok(n)
}

/// A sim over `state` with the kernel's systems only (see [`default_systems`]).
pub fn game(state: SimState) -> Sim {
    Sim::new(state).with_systems(default_systems())
}
