//! The append-only history log: what happened, to whom, why.
//!
//! Facts are never edited or removed. Every committed event becomes a fact; scripts
//! and systems can also record facts of their own kinds with `Command::RecordFact`.
//! Queries filter on the indexed [`Subjects`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::Event;
use crate::ids::*;

pub use crate::ids::FactKind;

/// The indexed subjects of a fact. Every field is optional; queries match on any
/// combination. Add a field here (and a column in the save's `history` table) to
/// index a new kind of subject.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Subjects {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<StationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ware: Option<WareId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rock: Option<RockId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<FactionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    /// Any other entity (a quest, trait, hull, lane, rumor ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<AnyId>,
}

/// One entry in the history log.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Fact {
    pub id: FactId,
    pub tick: Tick,
    pub kind: FactKind,
    pub cause: CauseId,
    pub subjects: Subjects,
    /// A quantity for summing (units mined, sold, delivered ...).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    pub payload: FactPayload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FactPayload {
    /// The event this fact records.
    Event(Event),
    /// A recorded fact of a pack's own kind, with a short note.
    Note(String),
}

/// A fact to record, before the engine assigns its ID, tick and cause.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct FactDraft {
    pub kind: FactKind,
    #[serde(default)]
    pub subjects: Subjects,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    pub note: String,
}

/// A history filter. Empty matches everything. `count`, `find` and `last` take one.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct HistoryQuery {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<FactKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<CauseId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<StationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ware: Option<WareId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rock: Option<RockId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<FactionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<AnyId>,
    /// Facts at or after this tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<Tick>,
    /// Facts at or before this tick.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<Tick>,
    /// Facts with an ID greater than this (for paging).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<FactId>,
    /// Most results `find` returns (oldest first).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl HistoryQuery {
    pub fn kind(kind: FactKind) -> Self {
        Self {
            kind: Some(kind),
            ..Self::default()
        }
    }

    /// Does this fact pass the filter? (`limit` is applied by the caller.)
    pub fn matches(&self, f: &Fact) -> bool {
        fn eq<T: PartialEq>(want: &Option<T>, have: &Option<T>) -> bool {
            want.is_none() || want == have
        }
        let s = &f.subjects;
        self.kind.as_ref().is_none_or(|k| *k == f.kind)
            && self.cause.is_none_or(|c| c == f.cause)
            && eq(&self.ship, &s.ship)
            && eq(&self.station, &s.station)
            && eq(&self.ware, &s.ware)
            && eq(&self.rock, &s.rock)
            && eq(&self.npc, &s.npc)
            && eq(&self.faction, &s.faction)
            && eq(&self.region, &s.region)
            && eq(&self.other, &s.other)
            && self.since.is_none_or(|t| f.tick >= t)
            && self.until.is_none_or(|t| f.tick <= t)
            && self.after.is_none_or(|a| f.id > a)
    }
}
