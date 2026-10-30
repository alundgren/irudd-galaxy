//! `galaxy-ship`: ships, for the player and everyone else (slice systems 3 to 6 and 9).
//!
//! - [`flight`]: arcade flight. Quick to speed, quicker to stop, no coasting; the ship
//!   swings round to face where it goes at its hull's turn rate.
//! - [`controls`]: control tiers as progression. Per-save flags (names are data) unlock
//!   click-to-fly, click-to-mine, click-to-dock, map travel, queues and loops; the
//!   loop API saves, runs and assigns loops.
//! - [`ShipSystem`]: one tick for every ship. Player input becomes motion or orders by
//!   the unlocked tier; flying off undocks; the context action docks, enters a lane or mines; the
//!   order runner executes the same verbs for the player, tenders and NPC craft. A
//!   ship's tick is one atomic batch, so a rejected mine or trade simply retries.
//! - [`RegrowthSystem`]: rocks regrow and reform with region activity, never time:
//!   one pool per field, given to the rock with the lowest remaining share.
//! - [`trade`]: the [`TradeDesk`] trait that turns buy and sell orders into commands.
//! - [`market`]: [`EconomyDesk`], the real desk: `galaxy_economy`'s prices, checks and
//!   trade commands behind the trait. [`FlatDesk`] (fixed prices) is for tests.
//!
//! Every tunable is data: hull stats in `hulls.toml`, the rest in `ship_rules.toml`
//! (saved with the world as `WorldState::ship_rules`).

pub mod conditions;
pub mod controls;
pub mod flight;
pub mod market;
pub mod space;
pub mod trade;

mod live;
mod regrowth;
mod step;
mod system;

pub use controls::{Capability, ControlError, Controls};
pub use market::EconomyDesk;
pub use regrowth::{RegrowthSystem, region_pulses};
pub use system::ShipSystem;
pub use trade::{ClosedDesk, FlatDesk, Side, Trade, TradeDesk, TradeError, TradeRequest};

use galaxy_core::System;

/// The ship systems in tick order, with `desk` handling buy and sell orders. The game
/// passes [`EconomyDesk`] and registers `galaxy_economy::EconomySystem` after these.
pub fn systems(desk: Box<dyn TradeDesk>) -> Vec<Box<dyn System>> {
    vec![
        Box::new(ShipSystem::new(desk)),
        Box::new(RegrowthSystem),
        Box::new(live::LiveInteractions),
    ]
}
