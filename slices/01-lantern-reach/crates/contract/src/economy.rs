//! The living market's shared data: rules, activity tallies, docking tracks, price
//! reach and trade requests.
//!
//! W3 (`galaxy-economy`) owns the maths. These are the plain types other crates and
//! packs agree on. Every number in [`EconomyRules`] is pack data (`economy.toml`); the
//! `Default`s are neutral (prices sit at base and nothing moves), never a pace.
//!
//! Markets advance on **pulses** of activity at a place (dockings, lane arrivals),
//! never on ticks. Pulse totals are kept as integer thousandths so their undo is exact.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

/// Whether a station buys a ware from ships or sells it to them. A station never
/// buys a ware it produces, or sells one it consumes.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum MarketRole {
    /// The station buys it (pays a price that saturation lowers).
    #[default]
    Buys,
    /// The station sells it (asks a price with a spread).
    Sells,
}

/// The producer and consumer rates of one market line, per station pulse. Levers
/// change these without touching stock, saturation or the price formula.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketRates {
    pub production: u32,
    pub consumption: u32,
    pub production_scale: f64,
    pub consumption_scale: f64,
}

/// Economy data and running tallies, saved with the world.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct EconomyState {
    /// From the pack's `economy.toml` at new game.
    pub rules: EconomyRules,
    /// Activity per station (pulses and trades).
    pub stations: BTreeMap<StationId, ActivityTally>,
    /// Activity per region (pulses and trades).
    pub regions: BTreeMap<RegionId, ActivityTally>,
    /// Per ship: where it last docked, for the counted-docking rule.
    pub docks: BTreeMap<ShipId, DockTrack>,
}

/// The market's tunables. Also the schema of a pack's `data/economy.toml`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct EconomyRules {
    pub pulses: PulseRules,
    pub pricing: PricingRules,
    pub reach: ReachRules,
}

/// How much market activity each kind of event makes.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PulseRules {
    /// Station pulse at the station a ship docks at (counted dockings only).
    pub dock_station: f64,
    /// Station pulse at every other station in that region.
    pub dock_neighbours: f64,
    /// Region pulse for that region.
    pub dock_region: f64,
    /// Station pulse at every station in the region a ship arrives in by lane.
    pub arrival_stations: f64,
    /// Region pulse for that region.
    pub arrival_region: f64,
    /// A re-docking at the same station counts only after the ship has been at least
    /// this far from it, mined, or docked somewhere else. Stops undock-redock loops.
    pub recount_distance: f32,
}

/// The price formula's numbers.
///
/// ```text
/// stock_factor = clamp(1 + stock_slope * (1 - stock / target), stock_factor_min, stock_factor_max)
/// pays = base * stock_factor * (1 - saturation)
/// asks = base * stock_factor * spread
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PricingRules {
    pub stock_slope: f64,
    pub stock_factor_min: f64,
    pub stock_factor_max: f64,
    /// Saturation never goes above this.
    pub saturation_max: f64,
    /// Asking-price spread at stations with no owner, or when no regard tier applies.
    pub spread: f64,
    /// Spread by the player's standing with the station's owner. The last tier whose
    /// `min_standing` is at or below the standing applies.
    pub regard: Vec<RegardSpread>,
    /// Producers stop at `target * produce_cap`. Zero means no cap.
    pub produce_cap: f64,
    /// A sale that leaves saturation at or above this records a `market_flooded` fact.
    pub flood_saturation: f64,
}

impl Default for PricingRules {
    /// Neutral: every price sits at base.
    fn default() -> Self {
        Self {
            stock_slope: 0.0,
            stock_factor_min: 1.0,
            stock_factor_max: 1.0,
            saturation_max: 0.0,
            spread: 1.0,
            regard: Vec::new(),
            produce_cap: 0.0,
            flood_saturation: 1.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegardSpread {
    pub min_standing: i32,
    pub spread: f64,
}

/// Price reach granted by player flags (fitted upgrades such as the price glass).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ReachRules {
    /// Flag -> live prices for stations within this many world units of the ship.
    pub flags: BTreeMap<FlagId, f32>,
}

/// How far a hull sees live prices. The default is "docked only".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct PriceReach {
    /// Live prices of stations within this distance. Zero: docked only.
    pub radius: f32,
    /// Live prices of every station in the ship's region.
    pub region: bool,
    /// A lane arrival refreshes the snapshots of every station in the new region.
    pub lane_scout: bool,
}

/// Running activity at a station or region. Integers, so undo is exact.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ActivityTally {
    /// Pulses so far, in thousandths of a pulse.
    pub pulses_milli: i64,
    /// Pulses counted but not yet applied to the market (thousandths).
    pub pending_milli: i64,
    pub trades: i64,
    pub units_bought: i64,
    pub units_sold: i64,
    /// Credits ships paid the market.
    pub credits_spent: i64,
    /// Credits the market paid ships.
    pub credits_earned: i64,
}

impl ActivityTally {
    pub fn is_zero(&self) -> bool {
        *self == Self::default()
    }

    /// Adds `delta` field by field.
    pub fn add(&mut self, delta: &ActivityTally) {
        self.pulses_milli += delta.pulses_milli;
        self.pending_milli += delta.pending_milli;
        self.trades += delta.trades;
        self.units_bought += delta.units_bought;
        self.units_sold += delta.units_sold;
        self.credits_spent += delta.credits_spent;
        self.credits_earned += delta.credits_earned;
    }

    pub fn negated(&self) -> ActivityTally {
        ActivityTally {
            pulses_milli: -self.pulses_milli,
            pending_milli: -self.pending_milli,
            trades: -self.trades,
            units_bought: -self.units_bought,
            units_sold: -self.units_sold,
            credits_spent: -self.credits_spent,
            credits_earned: -self.credits_earned,
        }
    }

    /// Pulses as a number.
    pub fn pulses(&self) -> f64 {
        self.pulses_milli as f64 / 1000.0
    }
}

/// Where an activity tally is kept.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "scope", rename_all = "snake_case")]
pub enum ActivityScope {
    Station { station: StationId },
    Region { region: RegionId },
}

/// A ship's last docking, for the counted-docking rule.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct DockTrack {
    pub station: StationId,
    /// It has since gone far enough or mined, so docking here again counts.
    pub away: bool,
}

/// Buy from or sell to a station's market.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum TradeSide {
    Buy,
    Sell,
}

/// One trade a docked ship asks for. Priced unit by unit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TradeRequest {
    pub ship: ShipId,
    pub station: StationId,
    pub ware: WareId,
    pub side: TradeSide,
    /// Units wanted. Without a `limit` it's all or nothing.
    pub quantity: u32,
    /// Price limit per unit: sell only while the unit price is at least this, buy only
    /// while it's at most this. With a limit the trade fills as many units as it can.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}
