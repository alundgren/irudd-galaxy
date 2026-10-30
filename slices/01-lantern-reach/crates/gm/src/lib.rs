//! `galaxy-gm`: the game-master bridge (slice system 16).
//!
//! One contract for a game master, on a save file or on the running game: read state
//! and history, digest what changed, look at the game, propose a pack and get a
//! validation report back, watch it land at a calm moment, roll it back in-world.
//!
//! - [`live`]: the local socket the running game opens (named pipe or Unix socket),
//!   its discovery file, newline-delimited JSON requests and event streams.
//! - [`bridge`]: [`Bridge`], the game's side, called between frames: loads applied
//!   packs, answers requests, applies staged packs at calm moments, works the inbox.
//! - [`store`]: the save's game-master folder (`<save>.gm/`): staged, applied and
//!   quarantined packs and exact copies of every revision.
//! - [`propose`]: the full validation pipeline, the fast checks plus [`sandbox`].
//! - [`sandbox`]: fork, fast-forward with and without the pack in child processes
//!   driven by the [`bot`], compare telemetry.
//! - [`digest`]: what changed since a fact, by place and person, citing facts.
//! - [`reads`]: state by path, store queries, history queries.
//! - [`cli`]: the `galaxy` commands on a save file or `--live`.
//!
//! Nothing here runs inside a tick. Writes go through the kernel like everything else:
//! the batch that applies a pack is caused by its revision, and rollback is
//! `galaxy_script::rollback`.

pub mod bot;
pub mod bridge;
pub mod cli;
pub mod digest;
pub mod live;
pub mod propose;
pub mod reads;
pub mod sandbox;
pub mod store;

use galaxy_contract::input::PlayerInput;
use galaxy_core::{Sim, TickReport};
use galaxy_script::ScriptHost;

pub use bridge::{Bridge, BridgeOptions, NoSnapshots, SnapshotTicket, Snapshots};

/// The game as the app composes it (`galaxy_app::game::Game`): every system in tick
/// order, with one script host running `core`, `gm-examples` and the game master's
/// packs, and whatever runs between ticks (the mind). The save-file rollback and the
/// sandbox child build through it, so packs run in the same tick order as in play.
pub trait Composed {
    fn sim(&self) -> &Sim;
    fn sim_mut(&mut self) -> &mut Sim;
    /// The game's one script host.
    fn scripts(&self) -> &ScriptHost;
    /// One tick, and what the game does after it. `None` while paused.
    fn step(&mut self, input: &PlayerInput) -> Option<TickReport>;
}

/// Builds the game over a state. The hook runs once on the game's script host, after
/// the game's own packs are loaded and before the host is attached to the sim: packs
/// it loads run at the host's place in the tick order and install their data with the
/// rest. [`Bridge::open`] is such a hook.
pub type Hook<'a> = &'a mut dyn FnMut(&ScriptHost, &Sim);
pub use digest::{Digest, digest};
pub use live::{Client, Discovery, Server, ServerOptions};
pub use propose::{ProposeOptions, validate};
pub use sandbox::{SandboxRules, SandboxRun};
pub use store::{GmState, GmStore};
