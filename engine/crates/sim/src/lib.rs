//! The world store and the deterministic sim.
//!
//! - [`world`]: entities, components, facts and causes; batches applied whole;
//! - [`services`]: per-tick engine work (motion, the floor height under things);
//! - [`session`]: modules, scripts and the world, stepped one fixed tick at a time, and
//!   hot-loading;
//! - [`edit`]: editing the world from outside its rules (tests, agents, live), recorded;
//! - [`save`]: SQLite saves that list the modules they ran with;
//! - [`recording`]: session recordings and replay;
//! - [`scenario`]: inputs files and scenario tests;
//! - [`keys`]: keys held down by the window or a live connection, and the inputs they make;
//! - [`step`] and [`watch`]: real-time pacing and module folder watching.
//!
//! `engine/docs/world-and-sim.md` and `engine/docs/saves-and-replay.md` explain the design.

pub mod changes;
pub mod collide;
pub mod edit;
pub mod flight;
pub mod keys;
pub mod life;
pub mod places;
pub mod recording;
pub mod rooms;
pub mod save;
pub mod scenario;
pub mod services;
pub mod session;
pub mod shape;
pub mod step;
pub mod talk;
pub mod travel;
pub mod watch;
pub mod world;

pub use recording::{BuildStamp, Recording, Replay, replay};
pub use session::{Input, Prepared, Session};
pub use world::World;
