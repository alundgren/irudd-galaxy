//! The one write path.
//!
//! Every change to world state is a typed [`Command`] inside a [`CommandBatch`] that
//! carries a [`Cause`]: a player verb, a script pack revision, a fast-model decision or
//! a native system. Systems and scripts never mutate the world directly; they submit
//! batches, and the engine applies them at the end of the tick.
//!
//! A batch is atomic: if any command fails or a runtime invariant breaks, none of it
//! applies and its events never go out. Applying a command yields an [`UndoRecord`],
//! so a later rollback by cause can invert it. Commands are either
//! [`Reversible`](Reversibility::Reversible) (the inverse is exact) or
//! [`CompensateOnly`](Reversibility::CompensateOnly) (history and emitted events can't
//! be unsaid; rollback has to compensate in-world instead).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::economy::{
    ActivityScope, ActivityTally, DockTrack, MarketBase, MarketRates, MarketStock,
};
use crate::events::Event;
use crate::geom::Vec2;
use crate::history::FactDraft;
use crate::ids::*;
use crate::pack::{ScriptHealth, ScriptValue};
use crate::progress::{BeatState, HullRecord, Tunable};
use crate::ship::{RockGrowth, ShipActivity};
use crate::talk::{Channel, SaidLine, TalkLine};
use crate::verbs::{Order, OrderQueue, VerbKind};
use crate::world::*;

/// Why a change happened. Recorded with every batch that leaves a trace.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Cause {
    pub kind: CauseKind,
    /// One line a person could read, e.g. "greenhouse stage 2 from the player's ice".
    pub summary: String,
    /// History facts this change is based on.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<FactId>,
}

impl Cause {
    pub fn system(name: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            kind: CauseKind::System { name: name.into() },
            summary: summary.into(),
            cites: Vec::new(),
        }
    }

    pub fn verb(actor: Actor, verb: VerbKind, summary: impl Into<String>) -> Self {
        Self {
            kind: CauseKind::Verb { actor, verb },
            summary: summary.into(),
            cites: Vec::new(),
        }
    }

    pub fn script(pack: PackId, revision: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            kind: CauseKind::Script {
                pack,
                revision: revision.into(),
            },
            summary: summary.into(),
            cites: Vec::new(),
        }
    }

    pub fn citing(mut self, facts: impl IntoIterator<Item = FactId>) -> Self {
        self.cites.extend(facts);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CauseKind {
    /// A verb by the player, an automated craft or an NPC craft.
    Verb { actor: Actor, verb: VerbKind },
    /// A script in a pack revision (the authored `core` pack or the live `gm` layer).
    Script { pack: PackId, revision: String },
    /// A fast-model decision.
    Mind {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        npc: Option<NpcId>,
        decision: String,
    },
    /// A native engine system (flight, market maths ...).
    System { name: String },
    /// An in-world rollback of earlier causes.
    Rollback { of: Vec<CauseId> },
}

impl CauseKind {
    /// Whether the engine keeps undo records for this cause so it can be rolled back.
    /// Player verbs and native systems are not: things the player earned stay earned.
    pub fn journaled(&self) -> bool {
        matches!(self, CauseKind::Script { .. } | CauseKind::Mind { .. })
    }
}

/// Who performed a verb.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// The player, flying by hand or through their own order queue.
    Player,
    /// A craft running orders (the player's automated craft or an NPC craft).
    Ship {
        ship: ShipId,
    },
    Npc {
        npc: NpcId,
    },
}

/// A cause as stored, with the ID and tick the engine gave it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RecordedCause {
    pub id: CauseId,
    pub tick: Tick,
    pub cause: Cause,
}

/// Commands that share a cause and apply all-or-nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct CommandBatch {
    pub cause: Cause,
    pub commands: Vec<Command>,
}

impl CommandBatch {
    pub fn new(cause: Cause) -> Self {
        Self {
            cause,
            commands: Vec::new(),
        }
    }

    pub fn with(mut self, command: Command) -> Self {
        self.commands.push(command);
        self
    }

    pub fn push(&mut self, command: Command) {
        self.commands.push(command);
    }
}

/// One world change. Add a variant here, its apply/undo arm in `galaxy_core::apply`,
/// and regenerate the schemas (`cargo xtask contract`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Command {
    // --- Ships ---
    SetShipMotion {
        ship: ShipId,
        position: Vec2,
        velocity: Vec2,
        heading: f32,
    },
    SetShipDocked {
        ship: ShipId,
        station: Option<StationId>,
    },
    /// Changes nothing, and fails unless `ship` is docked at `station`. A batch that
    /// only makes sense at a dock (a trade) carries one, so a ship that undocked
    /// earlier in the same tick rejects it. Its inverse is itself.
    ExpectDocked {
        ship: ShipId,
        station: StationId,
    },
    /// Changes nothing, and fails unless a ship `ship` flying `hull` exists, or (with
    /// `hull: null`) unless no ship `ship` exists. A purchase that builds a ship or
    /// trades a hull carries one, so a second request worked out from the same tick's
    /// state is rejected instead of charged again. Its inverse is itself.
    ExpectShip {
        ship: ShipId,
        hull: Option<HullId>,
    },
    /// Replaces a ship's hull and name, leaving everything else (cargo, activity,
    /// orders) as committed. Prestige uses it after an [`Command::ExpectShip`].
    SetShipHull {
        ship: ShipId,
        hull: HullId,
        name: String,
    },
    SetShipRegion {
        ship: ShipId,
        region: RegionId,
    },
    /// Adds (positive) or removes (negative) cargo. Fails below zero.
    AdjustCargo {
        ship: ShipId,
        ware: WareId,
        delta: i64,
    },
    /// Puts (`Some`) or takes away (`None`) a consignment in a ship's hold, keyed by
    /// its delivery tally. Loading one fails if one is already aboard under the key.
    SetConsignment {
        ship: ShipId,
        ledger: LedgerId,
        consignment: Option<Box<crate::world::Consignment>>,
    },
    SetOrders {
        ship: ShipId,
        orders: OrderQueue,
    },
    /// Replaces a ship's orders only if they still equal `from`, so orders run from the
    /// same tick's state can't overwrite orders given since (a loop assigned, a click):
    /// a stale one fails and its batch is rejected. The ship system uses this.
    UpdateOrders {
        ship: ShipId,
        from: Box<OrderQueue>,
        to: Box<OrderQueue>,
    },
    /// Replaces a ship's lane transit, mining run and input edge state.
    SetShipActivity {
        ship: ShipId,
        activity: Box<ShipActivity>,
    },

    // --- Player ---
    /// Adds to the player's credits, in centicredits. Fails below zero.
    AdjustCredits {
        delta: i64,
    },
    SetFlag {
        flag: FlagId,
        on: bool,
    },
    SetTrait {
        trait_id: TraitId,
        on: bool,
    },
    SetPriceSnapshot {
        station: StationId,
        snapshot: Option<PriceSnapshot>,
    },
    /// Saves (or with `orders: null` deletes) one of the player's named loops.
    SetLoop {
        name: String,
        orders: Option<Vec<Order>>,
    },

    // --- Rocks ---
    /// Changes a rock's remaining units. Fails below zero.
    AdjustRock {
        rock: RockId,
        delta: i64,
    },
    /// Replaces a rock's regrowth bookkeeping.
    SetRockGrowth {
        rock: RockId,
        growth: RockGrowth,
    },

    // --- Stations and factions ---
    SetStationStage {
        station: StationId,
        stage: u32,
    },
    SetStationPart {
        station: StationId,
        part: PartId,
        attached: bool,
    },
    SetStationOwner {
        station: StationId,
        owner: Option<FactionId>,
    },
    AdjustStanding {
        faction: FactionId,
        delta: i32,
    },
    /// Changes nothing, and fails unless `faction`'s standing is within `min..=max`.
    /// Deeds carry one ahead of each `AdjustStanding`: the band of standings the deed
    /// would have the same effect from (the same delta after the clamp, the same regard
    /// tiers), so a deed worked out before another batch moved the standing out of it
    /// is rejected, and one it doesn't affect still applies. Its inverse is itself.
    ExpectStanding {
        faction: FactionId,
        min: i32,
        max: i32,
    },

    // --- Markets ---
    /// Sets or removes (`None`) one ware's line at a station's market.
    SetMarketLine {
        station: StationId,
        ware: WareId,
        line: Option<MarketLine>,
    },
    /// Replaces a market line only if it still equals `from`, so trades and pulses
    /// worked out from the same tick's state can't overwrite each other: a stale one
    /// fails and its batch is rejected. The economy's trades and pulses use this.
    UpdateMarketLine {
        station: StationId,
        ware: WareId,
        from: Box<MarketLine>,
        to: Box<MarketLine>,
    },
    /// Sets a line's producer and consumer rates (market levers), leaving its stock,
    /// saturation and prices alone.
    SetMarketRates {
        station: StationId,
        ware: WareId,
        rates: MarketRates,
    },
    /// Sets a line's base price with the band and price that follow from it (market
    /// base levers), leaving its stock, saturation and rates alone.
    SetMarketBase {
        station: StationId,
        ware: WareId,
        base: MarketBase,
    },
    /// Sets a line's stock with the band and price that follow from it (the stock
    /// lever), leaving its base price, saturation and rates alone. Adding or removing
    /// a line is [`Command::SetMarketLine`].
    SetMarketStock {
        station: StationId,
        ware: WareId,
        stock: MarketStock,
    },
    /// Adds to a station's or region's activity tally (pulses, trade volume).
    AdjustActivity {
        scope: ActivityScope,
        delta: ActivityTally,
    },
    /// Sets or clears (`None`) where a ship last docked, for the counted-docking rule.
    SetDockTrack {
        ship: ShipId,
        track: Option<DockTrack>,
    },
    /// Sets or clears (`None`) the station where the player sold part of a ware's
    /// stack and kept the rest, for `split_sale` facts.
    SetSplitStack {
        ware: WareId,
        station: Option<StationId>,
    },

    // --- Whole records: create, replace, or remove with `value: null` ---
    FoundStation {
        station: Box<Station>,
        recipe: Box<galaxy_kit::StationRecipe>,
        /// Captured only when restoring a failed closure; new stations start empty.
        #[serde(default)]
        market: Option<Box<Market>>,
    },
    CloseFoundedStation {
        id: StationId,
    },
    /// Replaces a founded station's record and recipe, keeping its ID, region and
    /// position (ships docked or berthed there stay where they are), and opens
    /// (`market: Some`) or shuts (`None`) its ring. Forward it must toggle: reopening
    /// needs a closed scaffold (no market, no services), shutting needs an open ring.
    /// Its inverse is itself with the old values.
    SetFoundedStation {
        station: Box<Station>,
        recipe: Box<galaxy_kit::StationRecipe>,
        market: Option<Box<Market>>,
    },
    /// Replaces a survey buoy only if it still equals `from`, so a read or a reward
    /// claim worked out from the tick's start loses to a removal or replacement queued
    /// ahead of it. Its inverse swaps `from` and `to`.
    UpdateSurveyBuoy {
        id: String,
        from: Box<crate::live::SurveyBuoy>,
        to: Box<crate::live::SurveyBuoy>,
    },
    /// Changes nothing, and fails unless the per-save flag `flag` is set (`on: true`)
    /// or clear (`false`). A rock lamp carries one, so a lamp worked out before the
    /// mechanic was taken back in the same tick is rejected. Its inverse is itself.
    ExpectFlag {
        flag: FlagId,
        on: bool,
    },
    SetRockTag {
        rock: RockId,
        on: bool,
    },
    PutSurveyBuoy {
        id: String,
        value: Option<Box<crate::live::SurveyBuoy>>,
    },
    SetCanonStatus {
        id: String,
        status: crate::canon::CanonStatus,
    },
    PutStationRecipe {
        id: StationId,
        value: Option<Box<galaxy_kit::StationRecipe>>,
    },
    /// Adds, changes (`Some`) or removes (`None`) a station part
    /// ([`crate::parts::StationPart`]). Checked by `galaxy_core::parts::check`.
    PutStationPart {
        id: StationPartId,
        value: Option<Box<crate::parts::StationPart>>,
    },
    PutMapOverlay {
        id: String,
        value: Option<Box<crate::live::MapOverlay>>,
    },
    PutRegion {
        id: RegionId,
        value: Option<Box<Region>>,
    },
    PutLane {
        id: LaneId,
        value: Option<Box<Lane>>,
    },
    PutStation {
        id: StationId,
        value: Option<Box<Station>>,
    },
    PutMarket {
        id: StationId,
        value: Option<Box<Market>>,
    },
    PutWare {
        id: WareId,
        value: Option<Box<Ware>>,
    },
    PutHull {
        id: HullId,
        value: Option<Box<Hull>>,
    },
    PutRockType {
        id: RockTypeId,
        value: Option<Box<RockType>>,
    },
    PutField {
        id: FieldId,
        value: Option<Box<RockField>>,
    },
    PutRock {
        id: RockId,
        value: Option<Box<Rock>>,
    },
    PutShip {
        id: ShipId,
        value: Option<Box<Ship>>,
    },
    PutFaction {
        id: FactionId,
        value: Option<Box<Faction>>,
    },
    PutNpc {
        id: NpcId,
        value: Option<Box<Npc>>,
    },
    PutRumor {
        id: RumorId,
        value: Option<Box<Rumor>>,
    },
    /// Creates, replaces or removes (`value: null`) a named trader's route policy. Its
    /// ship must exist (put it first in the same batch).
    PutTrader {
        id: ShipId,
        value: Option<Box<crate::data::TraderDef>>,
    },
    /// Adds `station` to a rumor's `heard_at` (or removes it), and fails if it's already
    /// there (or isn't). Spreads compose: two craft carrying one rumor to different
    /// stations in one tick both land, and a second spread to the same station is
    /// rejected instead of overwriting the set. Its inverse flips `heard`.
    SetRumorHeard {
        rumor: RumorId,
        station: StationId,
        heard: bool,
    },

    // --- History and events (compensate-only) ---
    /// Emits an event on commit; the engine records its fact and bumps counters.
    Emit {
        event: Event,
    },
    /// Appends a fact of a pack's own kind on commit.
    RecordFact {
        fact: Box<FactDraft>,
    },

    // --- Scripts and legibility (W5) ---
    /// Sets (or clears, with `None`) one key in a script's declared store.
    SetScriptValue {
        pack: PackId,
        script: String,
        store: String,
        key: String,
        value: Option<ScriptValue>,
    },
    /// Sets how many times an activity trigger (`<script>.<trigger>`) has fired.
    SetTriggerFired {
        pack: PackId,
        trigger: String,
        fired: u64,
    },
    /// Sets a script's abort count and quarantine.
    SetScriptHealth {
        pack: PackId,
        script: String,
        health: ScriptHealth,
    },
    /// Switches a pack revision's handlers off (`retired`) or back on.
    SetRevisionRetired {
        pack: PackId,
        revision: String,
        retired: bool,
    },
    /// Switches a whole pack off (`off`, after a rollback) or back on (the game
    /// master applied a new revision). See `PackState::off`.
    SetPackOff {
        pack: PackId,
        off: bool,
    },
    /// Records the layer a pack runs as (`PackState::layer`). A recorded layer never
    /// changes: setting another one fails (only undo restores the unrecorded state).
    SetPackLayer {
        pack: PackId,
        layer: Option<crate::pack::PackLayer>,
    },
    // --- Progress (W4) ---
    /// Starts, moves or completes (or with `null` resets to dormant) a beat.
    PutBeat {
        id: BeatId,
        value: Option<Box<BeatState>>,
    },
    /// Fits (or removes) an upgrade on the pilot.
    SetFitting {
        upgrade: UpgradeId,
        fitted: bool,
    },
    /// Fails unless `upgrade` is (`fitted: true`) or isn't (`false`) fitted. Changes
    /// nothing; its inverse is itself. A purchase carries one so it can't be charged
    /// twice in one tick.
    ExpectFitting {
        upgrade: UpgradeId,
        fitted: bool,
    },
    /// Sets (or with `null` clears) a hull's rank record.
    SetHullRecord {
        hull: HullId,
        record: Option<HullRecord>,
    },
    /// Changes nothing, and fails unless the hull's rank record is `record` (`null`:
    /// none). Rank earning and prestige carry one ahead of `SetHullRecord`, so neither
    /// writes a record worked out before the other changed it. Its inverse is itself.
    ExpectHullRecord {
        hull: HullId,
        record: Option<HullRecord>,
    },
    /// Sets (or with `null` removes) a tunable.
    SetTunable {
        id: TunableId,
        value: Option<Box<Tunable>>,
    },
    /// Binds a reserved input or UI slot to a mechanic flag, or frees it.
    BindSlot {
        slot: SlotId,
        mechanic: Option<FlagId>,
    },

    /// Says a line on a legibility channel; it gets the next `talk.N` ID. The line
    /// must name a person or place and cite a fact. Chronicle lines are history and
    /// compensate-only.
    Say {
        line: Box<TalkLine>,
    },
    /// Restores or withdraws (`value: null`) a said line. `Say`'s inverse.
    PutTalk {
        id: TalkId,
        value: Option<Box<SaidLine>>,
    },
}

/// Whether a command's undo is exact.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Reversibility {
    Reversible,
    CompensateOnly,
}

impl Command {
    /// The command's `op` tag (its serde name): what [`crate::needs::COMMAND_NEEDS`] is
    /// keyed by.
    pub fn op(&self) -> &'static str {
        match self {
            Command::SetShipMotion { .. } => "set_ship_motion",
            Command::SetShipDocked { .. } => "set_ship_docked",
            Command::ExpectDocked { .. } => "expect_docked",
            Command::ExpectShip { .. } => "expect_ship",
            Command::SetShipHull { .. } => "set_ship_hull",
            Command::SetShipRegion { .. } => "set_ship_region",
            Command::AdjustCargo { .. } => "adjust_cargo",
            Command::SetConsignment { .. } => "set_consignment",
            Command::SetOrders { .. } => "set_orders",
            Command::UpdateOrders { .. } => "update_orders",
            Command::SetShipActivity { .. } => "set_ship_activity",
            Command::AdjustCredits { .. } => "adjust_credits",
            Command::SetFlag { .. } => "set_flag",
            Command::SetTrait { .. } => "set_trait",
            Command::SetPriceSnapshot { .. } => "set_price_snapshot",
            Command::SetLoop { .. } => "set_loop",
            Command::AdjustRock { .. } => "adjust_rock",
            Command::SetRockGrowth { .. } => "set_rock_growth",
            Command::SetStationStage { .. } => "set_station_stage",
            Command::SetStationPart { .. } => "set_station_part",
            Command::SetStationOwner { .. } => "set_station_owner",
            Command::AdjustStanding { .. } => "adjust_standing",
            Command::ExpectStanding { .. } => "expect_standing",
            Command::SetMarketLine { .. } => "set_market_line",
            Command::UpdateMarketLine { .. } => "update_market_line",
            Command::SetMarketRates { .. } => "set_market_rates",
            Command::SetMarketBase { .. } => "set_market_base",
            Command::SetMarketStock { .. } => "set_market_stock",
            Command::AdjustActivity { .. } => "adjust_activity",
            Command::SetDockTrack { .. } => "set_dock_track",
            Command::SetSplitStack { .. } => "set_split_stack",
            Command::FoundStation { .. } => "found_station",
            Command::CloseFoundedStation { .. } => "close_founded_station",
            Command::SetFoundedStation { .. } => "set_founded_station",
            Command::UpdateSurveyBuoy { .. } => "update_survey_buoy",
            Command::ExpectFlag { .. } => "expect_flag",
            Command::SetRockTag { .. } => "set_rock_tag",
            Command::PutSurveyBuoy { .. } => "put_survey_buoy",
            Command::SetCanonStatus { .. } => "set_canon_status",
            Command::PutStationRecipe { .. } => "put_station_recipe",
            Command::PutStationPart { .. } => "put_station_part",
            Command::PutMapOverlay { .. } => "put_map_overlay",
            Command::PutRegion { .. } => "put_region",
            Command::PutLane { .. } => "put_lane",
            Command::PutStation { .. } => "put_station",
            Command::PutMarket { .. } => "put_market",
            Command::PutWare { .. } => "put_ware",
            Command::PutHull { .. } => "put_hull",
            Command::PutRockType { .. } => "put_rock_type",
            Command::PutField { .. } => "put_field",
            Command::PutRock { .. } => "put_rock",
            Command::PutShip { .. } => "put_ship",
            Command::PutFaction { .. } => "put_faction",
            Command::PutNpc { .. } => "put_npc",
            Command::PutRumor { .. } => "put_rumor",
            Command::PutTrader { .. } => "put_trader",
            Command::SetRumorHeard { .. } => "set_rumor_heard",
            Command::Emit { .. } => "emit",
            Command::RecordFact { .. } => "record_fact",
            Command::SetScriptValue { .. } => "set_script_value",
            Command::SetTriggerFired { .. } => "set_trigger_fired",
            Command::SetScriptHealth { .. } => "set_script_health",
            Command::SetRevisionRetired { .. } => "set_revision_retired",
            Command::SetPackOff { .. } => "set_pack_off",
            Command::SetPackLayer { .. } => "set_pack_layer",
            Command::PutBeat { .. } => "put_beat",
            Command::SetFitting { .. } => "set_fitting",
            Command::ExpectFitting { .. } => "expect_fitting",
            Command::SetHullRecord { .. } => "set_hull_record",
            Command::ExpectHullRecord { .. } => "expect_hull_record",
            Command::SetTunable { .. } => "set_tunable",
            Command::BindSlot { .. } => "bind_slot",
            Command::Say { .. } => "say",
            Command::PutTalk { .. } => "put_talk",
        }
    }

    pub fn reversibility(&self) -> Reversibility {
        match self {
            Command::Emit { .. } | Command::RecordFact { .. } => Reversibility::CompensateOnly,
            Command::Say { line } if line.channel == Channel::Chronicle => {
                Reversibility::CompensateOnly
            }
            _ => Reversibility::Reversible,
        }
    }

    /// Whether the command only checks the world (`Expect*`). A check holds when it's
    /// applied; a rollback has nothing of it to undo, so rollback batches leave it out.
    pub fn is_expectation(&self) -> bool {
        matches!(
            self,
            Command::ExpectFlag { .. }
                | Command::ExpectDocked { .. }
                | Command::ExpectShip { .. }
                | Command::ExpectStanding { .. }
                | Command::ExpectFitting { .. }
                | Command::ExpectHullRecord { .. }
        )
    }
}

/// What it takes to undo one applied command.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UndoRecord {
    /// Apply this command to restore the previous state.
    Invert { command: Command },
    /// Can't be undone exactly; a rollback must compensate in-world.
    Compensate { note: String },
}

/// Undo records kept for one committed batch of a journaled cause.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct JournalEntry {
    pub tick: Tick,
    pub cause: CauseId,
    /// In application order; invert in reverse.
    pub undo: Vec<UndoRecord>,
}

/// A batch the engine refused.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rejection {
    pub tick: Tick,
    pub cause: Cause,
    /// Index of the failing command, or `None` for an invariant failure.
    pub command: Option<usize>,
    pub reason: String,
}
