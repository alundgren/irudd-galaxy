//! Authored data files: the TOML a pack ships under `data/`.
//!
//! The core pack's files are `packs/core/data/<name>.toml`, one per type below. Each
//! file holds an array of tables (`[[station]]`, `[[ware]]` ...). Unknown keys are an
//! error, so typos are caught at load. New game builds the world from these; after
//! that the save is the truth.
//!
//! Credits and prices are written as ordinary decimal credits (`base_price = 14`,
//! `credits = 40.5`). The world keeps them as centicredits ([`crate::money`]); the
//! loader converts them and rejects anything finer than a centicredit.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::economy::MarketRole;
use crate::geom::Vec2;
use crate::ids::*;
use crate::progress::{
    CapabilityDef, DeedRule, Modifier, PrestigeBranch, RankDef, RegardTier, Requirement, SlotDef,
    TierDef, TraitDef, TunableValue,
};
use crate::world::{Faction, Hull, Lane, Npc, Region, RockField, RockType, Service};

/// Core-pack data files that another crate loads itself (galaxy-mind's `mind.toml` and
/// `lines.toml`).
/// The world loader and the script host leave them alone.
pub const CONFIG_FILES: [&str; 2] = ["mind.toml", "lines.toml"];

/// Every data file, in load order. All are optional except `start.toml`.
pub const DATA_FILES: [&str; 18] = [
    "craft.toml",
    "wares.toml",
    "hulls.toml",
    "factions.toml",
    "regions.toml",
    "lanes.toml",
    "stations.toml",
    "station_parts.toml",
    "rocks.toml",
    "npcs.toml",
    "ship_rules.toml",
    "economy.toml",
    "controls.toml",
    "upgrades.toml",
    "traits.toml",
    "pacing.toml",
    "canon.toml",
    "start.toml",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaresFile {
    #[serde(default)]
    pub ware: Vec<WareDef>,
}

/// A ware. The loader turns it into a [`Ware`](crate::world::Ware).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WareDef {
    pub id: WareId,
    pub name: String,
    /// In credits.
    pub base_price: f64,
    /// False for things that can't be bought or sold (the Ancients fragment).
    #[serde(default = "yes")]
    pub tradeable: bool,
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HullsFile {
    #[serde(default)]
    pub hull: Vec<Hull>,
    /// Hull ranks: deeds done in a hull (W4). Prestige resets them.
    #[serde(default)]
    pub rank: Vec<RankDef>,
    /// Each fittable hull's sockets, power cap and free mass (`crate::fit`).
    #[serde(default)]
    pub fit: Vec<crate::fit::HullFit>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FactionsFile {
    #[serde(default)]
    pub faction: Vec<Faction>,
    /// The hidden standing's range (W4). Default -100..=100.
    #[serde(default)]
    pub standing: Option<StandingRange>,
    /// Regard tiers: how people treat you, by standing (W4).
    #[serde(default)]
    pub regard: Vec<RegardTier>,
    /// Memory rules: deeds that move standing (W4).
    #[serde(default)]
    pub deed: Vec<DeedRule>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StandingRange {
    pub min: i32,
    pub max: i32,
}

/// `controls.toml`: control capabilities, the tiers that group them, and the reserved
/// input and UI slots taught mechanics may bind (W4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ControlsFile {
    #[serde(default)]
    pub capability: Vec<CapabilityDef>,
    #[serde(default)]
    pub tier: Vec<TierDef>,
    #[serde(default)]
    pub slot: Vec<SlotDef>,
}

/// `upgrades.toml`: what hullwrights and dockmasters fit and build (W4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpgradesFile {
    #[serde(default)]
    pub upgrade: Vec<UpgradeData>,
    #[serde(default)]
    pub craft: Vec<CraftData>,
}

/// An upgrade as authored; `cost` in credits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct UpgradeData {
    pub id: UpgradeId,
    pub name: String,
    pub effect: String,
    /// In credits; 0 for a gift.
    pub cost: f64,
    #[serde(default)]
    pub seller: Option<NpcId>,
    #[serde(default)]
    pub station: Option<StationId>,
    #[serde(default)]
    pub unlock: Requirement,
    #[serde(default)]
    pub grants: Vec<FlagId>,
    #[serde(default)]
    pub modifiers: Vec<Modifier>,
    /// The socket it fits (`socket.rack` for control modules). See `ship_parts.toml`.
    #[serde(default)]
    pub socket: Option<SocketId>,
    #[serde(default)]
    pub power: u32,
    #[serde(default)]
    pub mass: u32,
    #[serde(default)]
    pub look: Option<String>,
    #[serde(default)]
    pub source: crate::fit::PartSource,
}

/// `ship_parts.toml`: the fit model's sockets, wharves, blueprints and mass rules
/// (`crate::fit`). The parts themselves are upgrades (`upgrades.toml`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ShipPartsFile {
    #[serde(default)]
    pub socket: Vec<crate::fit::SocketDef>,
    #[serde(default)]
    pub wharf: Vec<crate::fit::WharfDef>,
    #[serde(default)]
    pub blueprint: Vec<crate::fit::BlueprintDef>,
    #[serde(default)]
    pub mass: crate::fit::MassRules,
}

/// A craft (tender) as authored; `cost` in credits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CraftData {
    pub id: ShipId,
    pub name: String,
    pub effect: String,
    pub hull: HullId,
    /// In credits.
    pub cost: f64,
    #[serde(default)]
    pub seller: Option<NpcId>,
    pub station: StationId,
    #[serde(default)]
    pub unlock: Requirement,
    #[serde(default)]
    pub grants: Vec<FlagId>,
}

/// `traits.toml`: pilot traits (W4).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraitsFile {
    #[serde(default, rename = "trait")]
    pub traits: Vec<TraitDef>,
}

/// `pacing.toml`: the pacing profile's tunables and the hullwright's prestige terms
/// (W4). Every threshold a beat reads is a tunable here.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PacingFile {
    /// The profile these numbers are for (`slice`).
    #[serde(default)]
    pub profile: String,
    #[serde(default)]
    pub tunables: BTreeMap<TunableId, TunableValue>,
    #[serde(default)]
    pub prestige: Option<PrestigeData>,
}

/// Prestige as authored; `fee` in credits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrestigeData {
    pub station: StationId,
    #[serde(default)]
    pub seller: Option<NpcId>,
    /// In credits.
    pub fee: f64,
    pub branch: Vec<PrestigeBranch>,
    #[serde(default)]
    pub requires: Requirement,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegionsFile {
    #[serde(default)]
    pub region: Vec<Region>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LanesFile {
    #[serde(default)]
    pub lane: Vec<Lane>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationsFile {
    #[serde(default)]
    pub station: Vec<StationDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationDef {
    pub id: StationId,
    pub name: String,
    pub region: RegionId,
    pub position: Vec2,
    pub dock_radius: f32,
    #[serde(default)]
    pub owner: Option<FactionId>,
    #[serde(default)]
    pub services: BTreeSet<Service>,
    /// Starting build stage.
    #[serde(default)]
    pub stage: u32,
    /// Kit parts attached at the start.
    #[serde(default)]
    pub parts: BTreeSet<PartId>,
    /// Market lines; a station with any gets a market.
    #[serde(default)]
    pub market: Vec<MarketLineDef>,
}

/// A starting market line, prices in credits. Prices default from the ware's
/// `base_price`: price = base, min = base / 2, max = base * 2. The economy re-prices every line from the formula on
/// its first tick. The pricing and pulse rules are `economy.toml`
/// ([`EconomyRules`](crate::economy::EconomyRules), schema `data-economy.json`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketLineDef {
    pub ware: WareId,
    pub stock: u32,
    #[serde(default)]
    pub price: Option<f64>,
    #[serde(default)]
    pub base_price: Option<f64>,
    #[serde(default)]
    pub min_price: Option<f64>,
    #[serde(default)]
    pub max_price: Option<f64>,
    #[serde(default)]
    pub production: u32,
    #[serde(default)]
    pub consumption: u32,
    /// Defaults to `sells` for a line with production, else `buys`.
    #[serde(default)]
    pub role: Option<MarketRole>,
    #[serde(default)]
    pub target: u32,
    #[serde(default)]
    pub sat_per_unit: f64,
    #[serde(default)]
    pub sat_recover: f64,
    /// Production runs only while these wares are in stock here.
    #[serde(default)]
    pub needs: BTreeSet<WareId>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RocksFile {
    #[serde(default)]
    pub rock_type: Vec<RockType>,
    #[serde(default)]
    pub field: Vec<RockField>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct NpcsFile {
    #[serde(default)]
    pub npc: Vec<Npc>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartFile {
    pub player: StartDef,
}

/// Named traders and their modest, authored route menus. Saved with the world.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CraftFile {
    #[serde(default)]
    pub craft: Vec<TraderDef>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraderDef {
    pub id: ShipId,
    pub name: String,
    pub faction: FactionId,
    pub hull: HullId,
    pub home: StationId,
    pub stations: BTreeSet<StationId>,
    pub wares: BTreeSet<WareId>,
    pub max_units_per_ware: u32,
    pub max_wares_per_docking: u32,
    pub cost_per_distance: f64,
    /// Utility bonus for returning empty to the supply dock (centicredits).
    pub return_home_bonus: f64,
    #[serde(default)]
    pub preferences: Vec<RoutePreference>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoutePreference {
    pub station: StationId,
    /// Also admits this station to the route when the flag is set.
    pub when_flag: FlagId,
    pub bonus: f64,
}

/// Where a new game starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartDef {
    pub name: String,
    /// In credits.
    pub credits: f64,
    pub ship: ShipId,
    pub ship_name: String,
    pub hull: HullId,
    /// The station the ship starts docked at.
    pub station: StationId,
}
