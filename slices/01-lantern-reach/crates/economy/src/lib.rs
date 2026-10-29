//! `galaxy-economy`: the living market (W3). Native and headless.
//!
//! - [`pricing`]: the price formula, unit by unit, with saturation and bands.
//! - [`pulse`]: activity pulses. Markets move on dockings and lane arrivals, never on
//!   ticks; saturation only recovers with activity nearby.
//! - [`trade()`] / [`quote`]: validate and price a trade, returning one atomic batch.
//! - [`scout`]: price snapshots dated by activity, and price reach.
//! - [`map`]: the read-only [`MapView`] the app draws.
//! - [`levers`]: market levers for scripts (supply and demand, never the formula).
//! - [`telemetry`]: per-region metrics for the game master and the validator.
//! - [`choice`]: NPC trade choice (utility AI), ranked complete plans.
//! - [`EconomySystem`]: the per-tick system (pulses, scouting, the player's trades).
//!
//! Every rate and threshold is pack data: market lines in `stations.toml`, the rules in
//! `economy.toml`, price reach in `hulls.toml`.

pub mod choice;
pub mod levers;
pub mod map;
pub mod pricing;
pub mod pulse;
pub mod scout;
pub mod system;
pub mod telemetry;
pub mod trade;

pub use choice::{TradePlan, TraderProfile, rank_trades, top_choices};
pub use levers::{Lever, LeverError, LineSpec, lever_batch, lever_commands};
pub use map::{Control, MapView};
pub use scout::{PriceKnowledge, describe_age};
pub use system::EconomySystem;
pub use telemetry::EconomyTelemetry;
pub use trade::{
    Quote, TradeError, TradeOutcome, check_trade, plan_trade, quote, trade, trade_batch,
};
