//! `galaxy-ship`: ships, for the player and everyone else (slice systems 3 to 6 and 9).
//!
//! - [`flight`]: arcade flight. Quick to speed, quicker to stop, no coasting; the ship
//!   swings round to face where it goes at its hull's turn rate.
//! - [`controls`]: control tiers as progression. Per-save flags (names are data) unlock
//!   click-to-fly, click-to-mine, click-to-dock, map travel, queues and loops; the
//!   loop API saves, runs and assigns loops.
//! - [`ShipSystem`]: one tick for every ship. Player input becomes motion or orders by
//!   the unlocked tier; the context action docks, undocks, enters a lane or mines; the
//!   order runner executes the same verbs for the player, tenders and NPC craft.
//! - [`RegrowthSystem`]: rocks regrow and reform with region activity, never time.
//! - [`trade`]: the [`TradeDesk`] trait that turns buy and sell orders into commands.
//!   Pricing belongs to the economy crate, which plugs its desk in here.
//!
//! Every tunable is data: hull stats in `hulls.toml`, the rest in `ship_rules.toml`
//! (saved with the world as `WorldState::ship_rules`).

pub mod conditions;
pub mod controls;
pub mod flight;
pub mod space;
pub mod trade;

mod regrowth;
mod step;
mod system;

pub use controls::{Capability, ControlError, Controls};
pub use regrowth::{RegrowthSystem, region_pulses};
pub use system::ShipSystem;
pub use trade::{ClosedDesk, FlatDesk, Side, Trade, TradeDesk, TradeError, TradeRequest};

use galaxy_core::System;

/// The ship systems in tick order, with `desk` handling buy and sell orders.
pub fn systems(desk: Box<dyn TradeDesk>) -> Vec<Box<dyn System>> {
    vec![Box::new(ShipSystem::new(desk)), Box::new(RegrowthSystem)]
}
