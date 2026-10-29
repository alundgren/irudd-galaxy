//! `galaxy-app`: the game around the headless sim.
//!
//! The binary (`src/main.rs`) parses arguments and calls into these modules. The sim
//! never knows any of this exists: the app maps real time onto ticks, turns keys into
//! [`PlayerInput`](galaxy_contract::input::PlayerInput), and turns the world into a
//! [`render::Scene`] each frame.
//!
//! - [`game`]: the game's systems in tick order, and finding the packs folder.
//! - [`session`]: one play session over a `Sim`: real pause with the `paused` fact,
//!   dock-screen trades that are never silently lost, poses for interpolation.
//! - [`trades`]: the dock screen's trade intents and their calm refusal lines.
//! - [`input`]: keys and clicks to `PlayerInput`.
//! - [`hooks`]: what progression and the legibility channels will fill in (goal line,
//!   talk, chronicle, station services), with placeholders until they land.
//! - [`view`]: world to scene: stations, rocks, ships, lanes, the camera, backdrops.
//! - [`ui`]: the HUD, dock screen, map and talk panels.
//! - [`saves`]: the per-user save folder and the background saver.
//! - [`frame`]: one frame's scene and buttons, shared by the window and the shot.
//! - [`play`]: the window. [`shot`]: the same scene, headless, to a PNG, optionally
//!   after a [`script`]ed input run.

pub mod frame;
pub mod game;
pub mod hooks;
pub mod input;
pub mod play;
pub mod saves;
pub mod script;
pub mod session;
pub mod shot;
pub mod trades;
pub mod ui;
pub mod view;
