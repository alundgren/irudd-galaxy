//! The world-state schema: everything the sim simulates and a save stores.
//!
//! Plain data only. The maths that changes it (flight, mining, pricing) lives in the
//! workstream crates, and every change goes through a [`Command`](crate::command::Command).
//! Numbers here are whatever the authored data says; nothing in this module assumes a
//! pace.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::ActivityCounters;
use crate::geom::Vec2;
use crate::ids::*;
use crate::store::{Record, Store};
use crate::verbs::OrderQueue;

/// Bump when the world-state JSON changes incompatibly. Saves record it.
pub const WORLD_FORMAT_VERSION: u32 = 1;

/// The whole simulated world. Missing stores load empty, so adding a store keeps
/// older saves loadable.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WorldState {
    pub meta: WorldMeta,
    pub player: Player,
    pub regions: Store<RegionId, Region>,
    pub lanes: Store<LaneId, Lane>,
    pub stations: Store<StationId, Station>,
    /// One market per station that trades, keyed by station.
    pub markets: Store<StationId, Market>,
    pub wares: Store<WareId, Ware>,
    pub hulls: Store<HullId, Hull>,
    pub rock_types: Store<RockTypeId, RockType>,
    pub fields: Store<FieldId, RockField>,
    pub rocks: Store<RockId, Rock>,
    pub ships: Store<ShipId, Ship>,
    pub factions: Store<FactionId, Faction>,
    pub npcs: Store<NpcId, Npc>,
    pub rumors: Store<RumorId, Rumor>,
    /// Activity counters that pace beats and unlocks (never clock time).
    pub counters: ActivityCounters,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct WorldMeta {
    /// [`WORLD_FORMAT_VERSION`] when this state was written.
    pub format: u32,
    /// The new-game seed. Subsystem random generators derive from it.
    pub seed: u64,
    /// Ticks simulated so far.
    pub tick: Tick,
}

/// The player: wallet, flagship, unlocks and what they know.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Player {
    pub name: String,
    pub credits: i64,
    /// The ship the player flies by hand.
    pub ship: Option<ShipId>,
    /// Per-save mechanic flags: control unlocks, script switches.
    #[serde(default)]
    pub flags: BTreeSet<FlagId>,
    #[serde(default)]
    pub traits: BTreeSet<TraitId>,
    /// Scouted prices, dated by the visit they were taken on.
    #[serde(default)]
    pub price_snapshots: BTreeMap<StationId, PriceSnapshot>,
}

/// What the player saw at a station's market on their last visit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PriceSnapshot {
    pub tick: Tick,
    /// The player's docking count when it was taken: "dated by your last visit".
    pub visit: u64,
    pub prices: BTreeMap<WareId, u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Region {
    pub id: RegionId,
    pub name: String,
    pub center: Vec2,
    pub radius: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FactionId>,
}

/// A lane between two regions. Gates are world positions at each end.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lane {
    pub id: LaneId,
    pub a: RegionId,
    pub b: RegionId,
    pub a_gate: Vec2,
    pub b_gate: Vec2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Station {
    pub id: StationId,
    pub name: String,
    pub region: RegionId,
    pub position: Vec2,
    /// Ships within this distance of `position` can dock.
    pub dock_radius: f32,
    pub owner: Option<FactionId>,
    pub services: BTreeSet<Service>,
    pub build: BuildState,
}

/// What a station offers when docked.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Service {
    Market,
    Dockmaster,
    Hullwright,
    Archive,
}

/// A station's visible growth: a stage number plus the kit parts attached.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BuildState {
    pub stage: u32,
    #[serde(default)]
    pub parts: BTreeSet<PartId>,
}

/// A station market as plain data. W3 (economy) owns the maths that moves it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Market {
    pub station: StationId,
    pub lines: BTreeMap<WareId, MarketLine>,
}

/// One ware at one market. Provisional fields; W3 may extend.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketLine {
    pub stock: u32,
    /// Current unit price in credits.
    pub price: u32,
    /// Where the price settles with no pressure.
    pub base_price: u32,
    pub min_price: u32,
    pub max_price: u32,
    /// Units the station makes per market step (W3 defines the step; never a clock).
    #[serde(default)]
    pub production: u32,
    /// Units the station uses per market step.
    #[serde(default)]
    pub consumption: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ware {
    pub id: WareId,
    pub name: String,
    pub base_price: u32,
    /// False for things that can't be bought or sold (the Ancients fragment).
    #[serde(default = "yes")]
    pub tradeable: bool,
}

fn yes() -> bool {
    true
}

/// A hull class. Provisional numbers live in the core pack's `hulls.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Hull {
    pub id: HullId,
    pub name: String,
    pub tier: u32,
    pub hold_capacity: u32,
    /// World units per second at nominal tick rate.
    pub max_speed: f32,
    /// Mining units per second at nominal tick rate.
    pub mining_rate: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RockType {
    pub id: RockTypeId,
    pub name: String,
    pub ware: WareId,
    /// Multiplier on a hull's mining rate (1.0 = normal, higher = slower).
    pub hardness: f32,
    /// Units regrown per regrowth step (W2 defines the step from activity, never a clock).
    #[serde(default)]
    pub regrowth: u32,
}

/// An authored field. Rocks are generated from it once, at new game, with stable IDs
/// `<rock_prefix>.01`, `<rock_prefix>.02`, ...
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RockField {
    pub id: FieldId,
    pub name: String,
    pub region: RegionId,
    pub rock_type: RockTypeId,
    /// Prefix for generated rock IDs, e.g. `rock.shallows.ice`.
    pub rock_prefix: String,
    pub center: Vec2,
    pub radius: f32,
    pub count: u32,
    /// Units per rock, inclusive range.
    pub amount_min: u32,
    pub amount_max: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rock {
    pub id: RockId,
    pub field: FieldId,
    pub rock_type: RockTypeId,
    pub position: Vec2,
    /// Units left. Depletion persists.
    pub remaining: u32,
    /// Units when generated; `remaining / initial` drives the visible shrink.
    pub initial: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Ship {
    pub id: ShipId,
    pub name: String,
    pub hull: HullId,
    pub owner: Owner,
    pub region: RegionId,
    pub position: Vec2,
    pub velocity: Vec2,
    /// Facing, radians, 0 = +x, counter-clockwise.
    pub heading: f32,
    pub docked_at: Option<StationId>,
    pub hold: Hold,
    /// The ship's order queue (same verbs for player, automated and NPC craft).
    #[serde(default)]
    pub orders: OrderQueue,
}

/// Who owns a ship. Player-owned ships (flagship and automated craft) feed the
/// player's activity counters.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Owner {
    Player,
    Npc { npc: NpcId },
    Faction { faction: FactionId },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Hold {
    pub cargo: BTreeMap<WareId, u32>,
}

impl Hold {
    pub fn total(&self) -> u64 {
        self.cargo.values().map(|&n| u64::from(n)).sum()
    }

    pub fn count(&self, ware: &WareId) -> u32 {
        self.cargo.get(ware).copied().unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Faction {
    pub id: FactionId,
    pub name: String,
    /// The player's standing. Shown through how people treat you, not a bar.
    #[serde(default)]
    pub standing: i32,
    /// Wares this faction wants.
    #[serde(default)]
    pub wants: Vec<WareId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Npc {
    pub id: NpcId,
    pub name: String,
    /// Free-form role, e.g. `dockmaster`, `hullwright`, `scholar`.
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<FactionId>,
    /// Where they can be found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub location: Option<StationId>,
}

/// A rumor with a truth value. It may be true, or planted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rumor {
    pub id: RumorId,
    /// What it claims, in one line.
    pub claim: String,
    pub is_true: bool,
    /// Who planted it, if it's a plant.
    pub planted_by: Option<FactionId>,
    /// Stations where it's being told.
    pub heard_at: BTreeSet<StationId>,
    pub heard_by_player: bool,
    /// The fact it grew from, if any.
    pub source: Option<FactId>,
}

macro_rules! record {
    ($($ty:ty => $id:ty),* $(,)?) => {$(
        impl Record for $ty {
            type Id = $id;
            fn id(&self) -> &$id { &self.id }
        }
    )*};
}

record!(
    Region => RegionId,
    Lane => LaneId,
    Station => StationId,
    Ware => WareId,
    Hull => HullId,
    RockType => RockTypeId,
    RockField => FieldId,
    Rock => RockId,
    Ship => ShipId,
    Faction => FactionId,
    Npc => NpcId,
    Rumor => RumorId,
);

impl Record for Market {
    type Id = StationId;
    fn id(&self) -> &StationId {
        &self.station
    }
}
