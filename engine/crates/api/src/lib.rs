//! The engine's contract: what the world store, the script host and the binaries agree on.
//!
//! - [`command`]: the put, patch, remove and record commands and their [`Cause`];
//! - [`registry`]: component schemas, event kinds and capabilities from the loaded modules;
//! - [`engine`]: the tick rate, the slow cadence and the engine's own component types;
//! - [`explored`]: the canonical text of explored memory (`engine.explored`);
//! - [`view`]: what a script reads;
//! - [`ui`]: UI sound cues, presentation the view hands the game and recordings keep;
//! - [`rng`] and [`geom`]: seeded randomness and portable maths, so the sim is
//!   deterministic on every OS.
//!
//! IDs are namespaced by module (`galaxy_module::ids`). `engine/docs/world-and-sim.md`
//! explains the design.

pub mod command;
pub mod engine;
pub mod explored;
pub mod geom;
pub mod registry;
pub mod rng;
pub mod ui;
pub mod view;

pub use command::{Actor, Batch, Cause, Command, Fields};
pub use registry::Registry;
pub use view::WorldView;
