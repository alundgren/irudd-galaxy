//! The Luau host. Scripts run on events and on the slow cadence, never every frame; each
//! call runs in a fresh sandboxed state with a step and memory budget, and returns the
//! commands it wrote.
//!
//! - [`vm`]: the sandbox (carried over from the slice), budgets and aborts;
//! - [`meter`]: what native helpers charge to the step budget (carried over);
//! - [`iteration`]: generic `for` loops routed through the checked traversal, confirmed on
//!   the compiled bytecode (`bytecode`);
//! - [`api`]: the `ctx` API, declared once, and the Luau type file made from it;
//! - [`host`]: a module's scripts loaded and called.
//!
//! `engine/docs/script-host.md` explains the design.

pub mod api;
mod bytecode;
pub mod host;
pub mod iteration;
pub mod meter;
pub mod vm;

pub use host::{Event, Outcome, ScriptSet, Target};
pub use vm::{Abort, LIMITS, Limits, Location};

#[cfg(test)]
mod tests;
