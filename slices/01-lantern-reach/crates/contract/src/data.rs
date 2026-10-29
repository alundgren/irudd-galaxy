//! Authored data files: the TOML a pack ships under `data/`.
//!
//! The core pack's files are `packs/core/data/<name>.toml`, one per type below. Each
//! file holds an array of tables (`[[station]]`, `[[ware]]` ...). Unknown keys are an
//! error, so typos are caught at load. New game builds the world from these; after
//! that the save is the truth.

use std::collections::BTreeSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::Vec2;
use crate::ids::*;
use crate::world::{Faction, Hull, Lane, Npc, Region, RockField, RockType, Service, Ware};

/// Every data file, in load order. All are optional except `start.toml`.
pub const DATA_FILES: [&str; 9] = [
    "wares.toml",
    "hulls.toml",
    "factions.toml",
    "regions.toml",
    "lanes.toml",
    "stations.toml",
    "rocks.toml",
    "npcs.toml",
    "start.toml",
];

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct WaresFile {
    #[serde(default)]
    pub ware: Vec<Ware>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HullsFile {
    #[serde(default)]
    pub hull: Vec<Hull>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct FactionsFile {
    #[serde(default)]
    pub faction: Vec<Faction>,
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

/// A starting market line. Prices default from the ware's `base_price`: price = base,
/// min = base / 2, max = base * 2.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarketLineDef {
    pub ware: WareId,
    pub stock: u32,
    #[serde(default)]
    pub price: Option<u32>,
    #[serde(default)]
    pub base_price: Option<u32>,
    #[serde(default)]
    pub min_price: Option<u32>,
    #[serde(default)]
    pub max_price: Option<u32>,
    #[serde(default)]
    pub production: u32,
    #[serde(default)]
    pub consumption: u32,
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

/// Where a new game starts.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StartDef {
    pub name: String,
    pub credits: i64,
    pub ship: ShipId,
    pub ship_name: String,
    pub hull: HullId,
    /// The station the ship starts docked at.
    pub station: StationId,
}
