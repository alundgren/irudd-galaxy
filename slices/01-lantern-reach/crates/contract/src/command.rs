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

use crate::events::Event;
use crate::geom::Vec2;
use crate::history::FactDraft;
use crate::ids::*;
use crate::ship::{RockGrowth, ShipActivity};
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

    // --- Whole records: create, replace, or remove with `value: null` ---
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

    // --- History and events (compensate-only) ---
    /// Emits an event on commit; the engine records its fact and bumps counters.
    Emit {
        event: Event,
    },
    /// Appends a fact of a pack's own kind on commit.
    RecordFact {
        fact: Box<FactDraft>,
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
    pub fn reversibility(&self) -> Reversibility {
        match self {
            Command::Emit { .. } | Command::RecordFact { .. } => Reversibility::CompensateOnly,
            _ => Reversibility::Reversible,
        }
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
