//! What `galaxy`, `galaxy-gm` and their launchers share before any engine system runs:
//!
//! - [`manifest`]: a module's `manifest.toml` (id, versions, scripts, events, writes and
//!   component types);
//! - [`module`]: the module loader (a module folder read into memory and its start state)
//!   and [`ids`], module-namespaced IDs;
//! - [`gate`]: the engine version gate, which refuses modules that need a newer engine;
//! - [`discover`]: which modules a command is about;
//! - [`install`]: the layout of installed builds and launchers, and picking a build;
//! - [`failure`] and [`cli`]: exit codes, the JSON error object and the shared commands.
//!
//! `engine/docs/modules-and-install.md` and `engine/docs/exit-codes.md` explain them.

pub mod cli;
pub mod discover;
pub mod failure;
pub mod gate;
pub mod ids;
pub mod install;
pub mod manifest;
pub mod module;
pub mod version;

pub use failure::Failure;
pub use version::Version;
