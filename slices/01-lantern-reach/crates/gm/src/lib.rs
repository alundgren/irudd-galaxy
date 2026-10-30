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

pub use bridge::{Bridge, BridgeOptions, NoSnapshots, SnapshotTicket, Snapshots};
pub use digest::{Digest, digest};
pub use live::{Client, Discovery, Server, ServerOptions};
pub use propose::{ProposeOptions, validate};
pub use sandbox::{SandboxRules, SandboxRun};
pub use store::{GmState, GmStore};
