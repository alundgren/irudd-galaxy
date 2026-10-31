//! The world store and the deterministic sim.
//!
//! - [`world`]: entities, components, facts and causes; batches applied whole;
//! - [`services`]: per-tick engine work (motion, the floor height under things);
//! - [`session`]: modules, scripts and the world, stepped one fixed tick at a time, and
//!   hot-loading;
//! - [`save`]: SQLite saves that list the modules they ran with;
//! - [`recording`]: session recordings and replay;
//! - [`scenario`]: inputs files and scenario tests;
//! - [`step`] and [`watch`]: real-time pacing and module folder watching.
//!
//! `engine/docs/world-and-sim.md` and `engine/docs/saves-and-replay.md` explain the design.

pub mod recording;
pub mod save;
pub mod scenario;
pub mod services;
pub mod session;
pub mod step;
pub mod watch;
pub mod world;

pub use recording::{BuildStamp, Recording, replay};
pub use session::{Input, Session};
pub use world::World;
