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
pub use load::{DataError, load_world};
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

/// A sim over `state` with the kernel's systems only (see [`default_systems`]).
pub fn game(state: SimState) -> Sim {
    Sim::new(state).with_systems(default_systems())
}
