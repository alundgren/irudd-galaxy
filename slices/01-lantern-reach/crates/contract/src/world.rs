//! The world-state schema: everything the sim simulates and a save stores.
//!
//! Plain data only. The maths that changes it (flight, mining, pricing) lives in the
//! workstream crates, and every change goes through a [`Command`](crate::command::Command).
//! Numbers here are whatever the authored data says; nothing in this module assumes a
//! pace.

use std::collections::{BTreeMap, BTreeSet};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::economy::{EconomyState, MarketRole, PriceReach};
use crate::events::ActivityCounters;
use crate::geom::Vec2;
use crate::ids::*;
use crate::pack::PackState;
use crate::progress::{HullRecord, ProgressState};
use crate::ship::{RockGrowth, ShipActivity, ShipRules};
use crate::store::{Record, Store};
use crate::talk::TalkLog;
use crate::verbs::OrderQueue;

/// Bump when the world-state JSON changes incompatibly. Saves record it.
/// 2: money is whole centicredits (it was whole credits in 1).
pub const WORLD_FORMAT_VERSION: u32 = 2;

/// The whole simulated world. Missing stores load empty, so adding a store keeps
/// older saves loadable.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct WorldState {
    pub rock_tags: BTreeSet<RockId>,
    pub survey_buoys: BTreeMap<String, crate::live::SurveyBuoy>,
    pub canon: crate::canon::CanonState,
    pub station_recipes: BTreeMap<StationId, galaxy_kit::StationRecipe>,
    pub map_overlays: BTreeMap<String, crate::live::MapOverlay>,
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
    /// Flight, mining, lane and control-tier tunables (`ship_rules.toml`).
    pub ship_rules: ShipRules,
    /// Market rules and activity tallies (W3).
    pub economy: EconomyState,

    // --- Scripts and legibility (W5) ---
    /// Per installed pack: script stores, trigger counts, script health.
    pub packs: BTreeMap<PackId, PackState>,
    /// Dock talk, map notes and chronicle entries.
    pub talk: TalkLog,
    /// Beats, progression rules, tunables and mechanic slots (W4).
    pub progress: ProgressState,
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
    /// In centicredits (1 cr = 100, [`crate::money`]).
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
    /// Upgrades fitted. They belong to the pilot and move to a new hull (W4).
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub fittings: BTreeSet<UpgradeId>,
    /// Ranks and rank windows for every hull the player has flown (W4).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub hull_records: BTreeMap<HullId, HullRecord>,
    /// Saved loops by the player's name for them: the orders a loop repeats.
    #[serde(default)]
    pub loops: BTreeMap<String, Vec<crate::verbs::Order>>,
}

impl Player {
    /// Whether `ship` is a hull the player left at prestige. A berthed hull is kept for
    /// good and can't be flown, ordered or automated.
    pub fn is_berthed(&self, ship: &ShipId) -> bool {
        self.hull_records
            .values()
            .any(|r| r.berthed_as.as_ref() == Some(ship))
    }
}

/// What the player saw at a station's market on their last visit.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PriceSnapshot {
    pub tick: Tick,
    /// The player's docking count when it was taken: "dated by your last visit".
    pub visit: u64,
    /// Unit prices in centicredits.
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

/// One ware at one market. W3 (`galaxy-economy`) prices it; see
/// [`PricingRules`](crate::economy::PricingRules) for the formula.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MarketLine {
    pub stock: u32,
    /// The current unit price in centicredits: what the station pays for a `buys`
    /// line, what it asks for a `sells` line. Kept up to date by the economy. Every
    /// price here is in centicredits ([`crate::money`]).
    pub price: u32,
    /// Where the price settles with no pressure.
    pub base_price: u32,
    pub min_price: u32,
    pub max_price: u32,
    /// Units the station makes per station pulse (activity, never a clock).
    #[serde(default)]
    pub production: u32,
    /// Units the station uses per station pulse.
    #[serde(default)]
    pub consumption: u32,
    #[serde(default)]
    pub role: MarketRole,
    /// The stock the station aims for; prices rise below it and fall above it. Zero:
    /// stock doesn't move the price.
    #[serde(default)]
    pub target: u32,
    /// 0 (fresh) up to the rules' `saturation_max`. Only `buys` lines saturate.
    #[serde(default)]
    pub saturation: f64,
    /// Saturation added per unit sold to the station.
    #[serde(default)]
    pub sat_per_unit: f64,
    /// Saturation removed per station pulse.
    #[serde(default)]
    pub sat_recover: f64,
    /// Multiplies `production` (market levers).
    #[serde(default = "one")]
    pub production_scale: f64,
    /// Multiplies `consumption` (market levers).
    #[serde(default = "one")]
    pub consumption_scale: f64,
    /// Production runs only while each of these wares is in stock here.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub needs: BTreeSet<WareId>,
    /// The fraction of a unit produced or used but not yet whole, in 0..1.
    #[serde(default)]
    pub stock_carry: f64,
}

impl Default for MarketLine {
    fn default() -> Self {
        Self {
            stock: 0,
            price: 0,
            base_price: 0,
            min_price: 0,
            max_price: 0,
            production: 0,
            consumption: 0,
            role: MarketRole::default(),
            target: 0,
            saturation: 0.0,
            sat_per_unit: 0.0,
            sat_recover: 0.0,
            production_scale: 1.0,
            consumption_scale: 1.0,
            needs: BTreeSet::new(),
            stock_carry: 0.0,
        }
    }
}

fn one() -> f64 {
    1.0
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Ware {
    pub id: WareId,
    pub name: String,
    /// In centicredits.
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
    /// World units per second squared. Missing: top speed in `flight.accel_time`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accel: Option<f32>,
    /// Degrees per second. Missing: `flight.turn_rate`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_rate: Option<f32>,
    /// Docking rings give this hull a standing berth, so it may wait docked for a
    /// price (the tender's patient sell). Crewed hulls trade and clear the ring.
    #[serde(default)]
    pub standing_berth: bool,
    /// The Coupling: the hull can hand cargo to the player's tenders (`transfer`).
    #[serde(default)]
    pub coupling: bool,
    /// How far the hull sees live market prices. Default: docked only.
    #[serde(default)]
    pub price_reach: PriceReach,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RockType {
    pub id: RockTypeId,
    pub name: String,
    pub ware: WareId,
    /// Multiplier on a hull's mining rate (1.0 = soft ice, lower = slower to cut).
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
    /// Units the whole field regrows per region pulse, each pulse's units going to the
    /// rock with the lowest remaining share (spent rocks included). Region pulses are
    /// activity (`ship_rules.regrowth.pulses`), never time. Kept in thousandths.
    #[serde(default)]
    pub regrow: f32,
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
    /// Regrowth bookkeeping (activity-driven).
    #[serde(default)]
    pub growth: RockGrowth,
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
    /// Lane transit, mining run and input edge state.
    #[serde(default)]
    pub activity: ShipActivity,
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
    /// Map and banner colour, e.g. `lantern_green`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colour: Option<String>,
    /// Its mark, in a few words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark: Option<String>,
    /// Stations it claims but may not hold.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<StationId>,
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
