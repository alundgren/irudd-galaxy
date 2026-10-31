//! `galaxy-script`: the Luau script host (system 15) and the minimum of the
//! legibility channels (system 18).
//!
//! - [`pack`]: reading a pack folder; the revision is a content hash.
//! - [`vm`]: the sandboxed Luau states (a fresh one per handler call), step budget
//!   and memory limit.
//! - [`meter`]: what native helpers charge to the step budget.
//! - [`ctx`]: the hook API a handler sees, built from the manifest's `uses`.
//! - [`host`]: [`ScriptHost`], the per-tick system, install and dev hot reload.
//! - [`check`]: the fast validation pipeline behind `galaxy pack check`.
//! - [`confirm`]: no silent changes to the player's hold, credits or fitted parts.
//! - [`analyzer`]: the strict typecheck with the pinned `luau-analyze`.
//! - [`rollback`](mod@rollback): in-world rollback by cause with a compensating line.
//! - [`testrun`]: `galaxy pack test`.
//!
//! Scripts never touch the world directly: every write is a `Command` in a batch with
//! `Cause::Script { pack, revision }`, applied atomically by the kernel at the end of
//! the tick.

pub mod analyzer;
pub mod check;
pub mod confirm;
pub mod ctx;
pub mod host;
pub mod meter;
pub mod pack;
pub mod rollback;
pub mod scan;
pub mod testrun;
pub mod vm;
pub mod watch;

pub use check::{CheckOptions, Checked, check_dir};
pub use host::{HostEvent, ScriptHost};
pub use pack::{Pack, read_pack};
pub use rollback::{RollbackOutcome, rollback};
pub use testrun::run_pack_tests;

pub mod canon;
