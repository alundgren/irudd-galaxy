//! The GM's live socket: how `galaxy-gm live` reads a running `galaxy play` and hot-loads
//! modules into it. The GM owns all three parts, and `galaxy` only hosts the server:
//!
//! - [`protocol`]: newline-delimited JSON requests and replies, and the discovery file;
//! - [`server`]: the local socket the game listens on, and its threads;
//! - [`host`]: [`Live`], which answers requests between frames from a [`Host`] (the game);
//! - [`reads`]: state, history and watch reads of a world, shared with `galaxy sim`;
//! - [`client`]: finding the running games, choosing one, one request and its reply.
//!
//! `engine/docs/live-socket.md` explains the design.

use std::path::PathBuf;

pub mod client;
pub mod host;
pub mod protocol;
pub mod reads;
pub mod server;

pub use host::{Host, Live};
pub use protocol::{Act, Discovery, Op, PROTOCOL, RunningModule, Step, WireError};
pub use server::{ServerOptions, Wake};

/// Moves the live folder (where running games write their discovery files) to this
/// absolute folder.
pub const LIVE_VAR: &str = "GALAXY_LIVE";

/// The live folder: `GALAXY_LIVE` if it is an absolute folder, else `irudd-galaxy/live` in
/// the per-user data folder, beside the sessions folder.
pub fn live_dir() -> PathBuf {
    std::env::var_os(LIVE_VAR)
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .unwrap_or_else(|| {
            directories::BaseDirs::new()
                .map(|d| d.data_local_dir().to_path_buf())
                .unwrap_or_else(std::env::temp_dir)
                .join("irudd-galaxy")
                .join("live")
        })
}
