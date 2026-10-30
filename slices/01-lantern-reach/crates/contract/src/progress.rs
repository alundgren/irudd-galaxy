//! Progress (W4): beats, standing and regard, pilot traits, hull ranks, prestige,
//! station services and upgrades, taught mechanics and pacing tunables.
//!
//! Plain data only. `galaxy-progress` evaluates it and builds the commands; the Luau
//! `core` pack registers beats through `G.beat`. Every threshold here is pack data:
//! nothing in this module assumes a pace, and no requirement can name a time.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::CounterKind;
use crate::history::HistoryQuery;
use crate::ids::*;

/// Everything W4 keeps in the world. Missing parts load empty.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ProgressState {
    /// The progression tables from the core pack's data (upgrades, controls, traits,
    /// ranks, regard, deeds, prestige). Saved with the world.
    pub rules: ProgressRules,
    /// Beats that have started, by stable ID. A beat with no entry is dormant.
    pub beats: BTreeMap<BeatId, BeatState>,
    /// Pacing numbers and choices beats read (`pacing.toml`).
    pub tunables: BTreeMap<TunableId, Tunable>,
    /// Reserved input and UI slots, bound to the mechanic that uses them.
    pub slots: BTreeMap<SlotId, FlagId>,
}

// --- Beats ---------------------------------------------------------------------------

/// Where a started beat is. Dormant beats have no state at all.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct BeatState {
    /// The pack that registered it.
    pub pack: PackId,
    pub act: u32,
    #[serde(default)]
    pub kind: BeatKind,
    pub status: BeatStatus,
    /// The step under way, from 0. Equal to the number of steps once every step is
    /// done and the beat waits on its end state.
    pub step: u32,
    /// The current step's ID, if the beat has steps and one is under way.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    /// The one goal line the HUD shows while the beat is active.
    pub goal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub giver: Option<NpcId>,
    /// Facts after this one happened since the beat started (`None`: from the start of
    /// history). Beats order by facts, never by time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<FactId>,
    /// Facts after this one happened since the current step started.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_since: Option<FactId>,
    /// Facts after this one happened since the beat completed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_since: Option<FactId>,
    /// The tick it started on, for ordering goal lines (newest first).
    pub started: Tick,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BeatStatus {
    Active,
    Completed,
}

/// Directed beats are the authored backbone; situations (a shortage, a dispute, a
/// rumor) fire from world state and history. Both run the same way.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum BeatKind {
    #[default]
    Directed,
    Situation,
}

/// A beat as a pack registers it, without its Luau callbacks.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct BeatDef {
    pub id: BeatId,
    pub act: u32,
    #[serde(default)]
    pub kind: BeatKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub giver: Option<NpcId>,
    /// The beat's goal line while it has no step under way (one readable line).
    pub goal: String,
    /// Preconditions: world state, history and activity, never time.
    #[serde(default)]
    pub requires: Requirement,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<StepDef>,
    /// The end state, checked once every step is done.
    #[serde(default)]
    pub done: Requirement,
    #[serde(default)]
    pub rewards: Rewards,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StepDef {
    /// Stable within the beat (`show`, `build`, `assign`).
    pub id: String,
    pub goal: String,
    pub done: Requirement,
}

/// What a beat pays when it completes. Credits are centicredits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct Rewards {
    pub credits: i64,
    /// Standing changes, written as deeds named after the beat.
    pub standing: BTreeMap<FactionId, i32>,
    /// Flags to set (capabilities write `cap_unlocked`).
    pub flags: Vec<FlagId>,
    /// Upgrades fitted as gifts (they write `upgrade_fitted`).
    pub fit: Vec<UpgradeId>,
}

/// A condition over world state, history and activity counters. Never time: there is
/// no clock to name. Used by beats, upgrade unlocks and prestige.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Requirement {
    /// Always holds.
    #[default]
    Always,
    All(Vec<Requirement>),
    Any(Vec<Requirement>),
    Not(Box<Requirement>),
    BeatDone(BeatId),
    BeatStarted(BeatId),
    /// Started and not completed.
    BeatActive(BeatId),
    /// The beat has reached at least this step (0 = its first step is under way).
    BeatStep {
        beat: BeatId,
        at_least: u32,
    },
    /// An activity counter for the player, a station or a region.
    Counter {
        kind: CounterKind,
        at_least: Num,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        station: Option<StationId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<RegionId>,
    },
    /// At least `at_least` history facts match (default 1).
    Facts {
        query: HistoryQuery,
        #[serde(default = "one")]
        at_least: Num,
        #[serde(default)]
        since: Since,
    },
    /// The `amount`s of matching facts add up to at least `at_least`.
    Sum {
        query: HistoryQuery,
        at_least: Num,
        #[serde(default)]
        since: Since,
    },
    Flag(FlagId),
    Trait(TraitId),
    Fitted(UpgradeId),
    /// The player's ship is this hull.
    Hull(HullId),
    /// The player owns this craft.
    Owns(ShipId),
    /// Credits, in centicredits.
    Credits {
        at_least: Num,
    },
    /// Units of a ware in the player ship's hold.
    Cargo {
        ware: WareId,
        at_least: Num,
    },
    /// Free room in the player ship's hold.
    HoldFree {
        at_least: Num,
    },
    /// The player ship is docked at this station.
    DockedAt(StationId),
    /// The player ship is docked anywhere.
    Docked,
    /// The player ship is in this region.
    InRegion(RegionId),
    /// The player ship is in `region`, within `within` u of (`x`, `y`): it went
    /// somewhere (a rumored vein, a buoy). Any of the numbers can be a tunable.
    Near {
        region: RegionId,
        x: Num,
        y: Num,
        within: Num,
    },
    /// The player's regard with a faction is at least this tier (by tier ID).
    Regard {
        faction: FactionId,
        at_least: String,
    },
    /// A tunable holds this value.
    TunableIs {
        tunable: TunableId,
        value: TunableValue,
    },
    /// A Luau predicate a script registered with `G.test` (by index within its
    /// script). Data files can't hold one.
    Test(u32),
}

fn one() -> Num {
    Num::Value(1.0)
}

/// Which facts a history requirement counts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Since {
    /// The whole history.
    #[default]
    Ever,
    /// Since the beat started (for beat requirements).
    Start,
    /// Since the current step started.
    Step,
}

/// A number, or the current value of a tunable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Num {
    Value(f64),
    Tunable { tunable: TunableId },
}

impl From<f64> for Num {
    fn from(v: f64) -> Self {
        Num::Value(v)
    }
}

// --- Tunables --------------------------------------------------------------------------

/// A pacing number or choice. `authored` is what the pack data says; a script may
/// change `value` with a cause, and a rollback restores it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Tunable {
    pub value: TunableValue,
    pub authored: TunableValue,
    /// The pack whose script last changed it (`None`: the pack data's value).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub set_by: Option<PackId>,
}

impl Tunable {
    /// Whether a script changed it from the authored value.
    pub fn tuned(&self) -> bool {
        self.set_by.is_some() && self.value != self.authored
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TunableValue {
    Number(f64),
    Text(String),
    List(Vec<String>),
}

impl TunableValue {
    pub fn as_number(&self) -> Option<f64> {
        match self {
            TunableValue::Number(n) => Some(*n),
            _ => None,
        }
    }
}

// --- Rules (from the core pack's data) ------------------------------------------------

/// The progression tables. Money is centicredits.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct ProgressRules {
    /// The pacing profile the tunables came from (`slice`).
    pub profile: String,
    pub upgrades: BTreeMap<UpgradeId, UpgradeDef>,
    /// Craft the player can have built (the tenders).
    pub crafts: BTreeMap<ShipId, CraftDef>,
    pub capabilities: BTreeMap<FlagId, CapabilityDef>,
    pub tiers: Vec<TierDef>,
    pub slots: BTreeMap<SlotId, SlotDef>,
    pub traits: BTreeMap<TraitId, TraitDef>,
    /// Hull ranks, in hull then rank order.
    pub ranks: Vec<RankDef>,
    /// Regard tiers by rising `min_standing`.
    pub regard: Vec<RegardTier>,
    pub deeds: Vec<DeedRule>,
    /// Standing is clamped to this range.
    pub standing_min: i32,
    pub standing_max: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prestige: Option<PrestigeRules>,
}

/// A fitting a station sells. Fittings are never removed and move with the pilot.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct UpgradeDef {
    pub id: UpgradeId,
    pub name: String,
    /// One line for the seller's list.
    pub effect: String,
    /// Centicredits. Zero: a gift.
    pub cost: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seller: Option<NpcId>,
    /// Where it's fitted. `None`: only as a beat reward.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<StationId>,
    /// When it appears on the seller's list.
    #[serde(default)]
    pub unlock: Requirement,
    /// Flags it sets (control-tier capabilities, from `controls.toml`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<FlagId>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

/// An automated craft a station builds for the player (a tender).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CraftDef {
    pub id: ShipId,
    pub name: String,
    pub effect: String,
    pub hull: HullId,
    /// Centicredits.
    pub cost: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seller: Option<NpcId>,
    /// Where it's built and berthed.
    pub station: StationId,
    #[serde(default)]
    pub unlock: Requirement,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub grants: Vec<FlagId>,
}

/// A control capability (`cap.click_fly`). The ship crate reads the same flag names
/// from `ship_rules.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CapabilityDef {
    pub id: FlagId,
    pub name: String,
    /// The presentation tier it belongs to.
    pub tier: u32,
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TierDef {
    pub tier: u32,
    pub name: String,
    pub summary: String,
}

/// A reserved input or UI slot, free until a taught mechanic binds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct SlotDef {
    pub id: SlotId,
    pub kind: SlotKind,
    pub name: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SlotKind {
    Input,
    Ui,
}

/// A pilot trait: earned by doing one deed many times, kept for good (prestige too).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TraitDef {
    pub id: TraitId,
    pub name: String,
    pub deed: Measure,
    /// How many times. Never shown as a bar or a count.
    pub times: u64,
    /// One line naming the effect.
    pub effect: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noticed_by: Vec<NpcId>,
}

/// A hull rank: deeds done in that hull. Ranks are what prestige resets.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RankDef {
    pub hull: HullId,
    /// 1, 2, 3 ... in order.
    pub rank: u32,
    pub name: String,
    /// Counted only over facts since the player started flying this hull. Use
    /// `facts` or `sum` measures on `ship.player`.
    pub deed: Measure,
    pub at_least: u64,
    pub effect: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub modifiers: Vec<Modifier>,
}

/// How many times a deed was done.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Measure {
    /// An activity counter (player scope unless a station or region is given).
    Counter {
        kind: CounterKind,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        station: Option<StationId>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        region: Option<RegionId>,
    },
    /// Matching history facts.
    Facts(HistoryQuery),
    /// The `amount`s of matching facts.
    Sum(HistoryQuery),
}

/// A regard tier: how people treat you at a standing. The number stays hidden.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegardTier {
    /// `cold`, `stranger`, `known`, `trusted`, `kin`.
    pub id: String,
    pub name: String,
    pub min_standing: i32,
    /// How it shows, in one line.
    pub shows: String,
}

/// A faction's memory rule: a deed the player does moves standing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DeedRule {
    /// Stable, e.g. `deed.coop.kettle_supply`. Written on `standing_changed`.
    pub id: AnyId,
    pub faction: FactionId,
    /// The facts that count. `ship` defaults to the player's ship.
    pub on: DeedFilter,
    /// Standing per step.
    pub delta: i32,
    /// Units (summed `amount`) per step; without it every matching fact is a step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub per: Option<u64>,
    /// Most steps per docking (counted from the player's latest docking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cap_per_docking: Option<u32>,
    /// Pays once, ever.
    #[serde(default)]
    pub once: bool,
    /// Tags traits can scale (`delivery`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// Which facts a deed counts. Each field narrows; a list matches any of its values.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DeedFilter {
    pub kinds: Vec<FactKind>,
    pub stations: Vec<StationId>,
    pub wares: Vec<WareId>,
    /// Matches the fact's `other` subject (a ledger, a beat ...).
    pub others: Vec<AnyId>,
    /// Count any ship's facts, not just the player's.
    pub any_ship: bool,
}

/// Hullwright prestige: trade the hull up for a fee. Only hull ranks reset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PrestigeRules {
    pub station: StationId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seller: Option<NpcId>,
    /// Centicredits, paid and not refunded.
    pub fee: i64,
    pub branches: Vec<PrestigeBranch>,
    /// When the offer appears (beyond being docked at `station` with the fee).
    #[serde(default)]
    pub requires: Requirement,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PrestigeBranch {
    pub from: HullId,
    pub to: Vec<HullId>,
    /// The old hull stays berthed at the station as this ship.
    pub berth_as: ShipId,
}

/// The fixed list of native effects traits, fittings and ranks may have. Each is
/// small and qualitative; `galaxy-progress` adds them up per ship and the ship,
/// economy and app crates read the totals.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "modifier", rename_all = "snake_case")]
pub enum Modifier {
    /// Multiplies mining rate (the mining lens: 1.5).
    MiningRate { factor: f64 },
    /// Mining while facing a rock's richest face yields this much more (Rimesight).
    RichFace { bonus: f64 },
    /// Extra hold units (hold extender, Wren rank 2).
    Hold { units: u32 },
    /// Extra hold units on each of the player's tenders (Tandem rank 3).
    TenderHold { units: u32 },
    /// Multiplies turn rate.
    TurnRate { factor: f64 },
    /// Multiplies top speed.
    Speed { factor: f64 },
    /// Multiplies the camera's zoom (below 1 shows more around the ship).
    Zoom { factor: f64 },
    /// Multiplies docking ring reach (Ringwise: 1.25).
    DockReach { factor: f64 },
    /// Lane mouths catch the ship from this radius (Lanerunner: 105).
    MouthRadius { radius: f32 },
    /// Live prices of stations within this distance (price glass: 1200).
    PriceGlass { range: f32 },
    /// Multiplies standing gains from deeds with this tag (Good neighbour: delivery x1.5).
    DeedBonus { tag: String, factor: f64 },
    /// A named presentation feature for the app and talk (`sell_preview`,
    /// `last_visit_greeting`, `lane_radio`, `rumor_first`, `seam_sense` ...).
    Feature { feature: String },
}

// --- Player records -----------------------------------------------------------------

/// What the player did in one hull. Kept for every hull they've flown.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HullRecord {
    /// Ranks earned in it (0 = none).
    pub rank: u32,
    /// Facts after this one count toward its ranks (`None`: all of history).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<FactId>,
    /// Set when the player left the hull at prestige; its ranks are frozen.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<FactId>,
    /// Where the hull is kept now that the player flies another.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub berthed_as: Option<ShipId>,
}

// --- Services (player requests) -----------------------------------------------------

/// A purchase the player asks for at a dock this tick. The progress system runs each
/// as one atomic batch and drops ones that don't apply; check with
/// `galaxy_progress::services::check` first.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "service", rename_all = "snake_case")]
pub enum ServiceRequest {
    /// Buy and fit an upgrade.
    Fit { upgrade: UpgradeId },
    /// Have a craft built (a tender).
    Build { craft: ShipId },
    /// Hullwright prestige to a new hull.
    Prestige { to: HullId },
}
