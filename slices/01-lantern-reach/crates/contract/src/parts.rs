//! Station parts (issue #47, 3.1): a station is a list of parts. Each part is a room of
//! a kind the app knows how to show (a market, a hullwright's wharf, the dockmaster's
//! office ...), drawn as a module bolted onto a named anchor of the station's kit
//! recipe, with a keeper and a hanging sign.
//!
//! The core pack authors them in `data/station_parts.toml` ([`StationPartsFile`]); a
//! pack changes them through [`Command::PutStationPart`](crate::command::Command) with
//! the `ctx.parts` hooks, at a calm moment. `galaxy_core::parts::check` validates every
//! part: the anchor exists on the recipe and is free, the module kind is within the art
//! bible's budget, and a keeper at the station has a line citing the cause.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{FactId, NpcId, PackId, StationId, StationPartId};

/// One part of a station.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationPart {
    pub id: StationPartId,
    pub station: StationId,
    /// The kind of room it opens into.
    pub room: RoomKind,
    /// Its name on its sign and key icon ("Archive"), if not its kind's ("Counter").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The asset-kit kind it's drawn as (`galaxy_kit::MODULE_KINDS`); the art bible's
    /// budget applies.
    pub module: String,
    /// A free anchor on the station's kit recipe (`galaxy_kit::anchors`).
    pub anchor: String,
    /// Hubs only: which quarter's sign it sits under.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarter: Option<String>,
    /// Who keeps it: a person at the station. Their bubbles rise from its module.
    pub keeper: NpcId,
    #[serde(default)]
    pub state: PartState,
    /// Upgrades raise it: a bigger module.
    #[serde(default = "first_level")]
    pub level: u32,
    /// The hanging sign: one line of what it is and offers.
    pub sign: String,
    /// The keeper's line about the part as it is now, in the first person (why it's
    /// closed, what's going up). Required whenever there's a cause or it isn't open.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub line: String,
    /// Who changed it and the facts the keeper's line cites. `None`: authored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cause: Option<PartCause>,
    /// What the keeper says if the part is taken away.
    pub rollback_line: String,
    /// The latest change, for the station key's ribbon (set by the write that made it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub news: Option<PartNews>,
}

fn first_level() -> u32 {
    1
}

impl StationPart {
    /// Its name on screen: its own, or its room kind's.
    pub fn title(&self) -> &str {
        self.name.as_deref().unwrap_or(self.room.title())
    }
}

/// What kind of room a part opens into. Only a brand-new kind needs code in the app;
/// every other change is data.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum RoomKind {
    Market,
    /// The hullwright's wharf: the cradle where parts are fitted.
    Wharf,
    /// The dockmaster's office: talk, and "Ask what changed".
    Dockmaster,
    NoticeWall,
    Canteen,
    Greenhouse,
    Berths,
    /// A general counter: a keeper, their offers and a few lines. Covers most shops.
    Counter,
}

impl RoomKind {
    /// Its name on screen.
    pub fn title(self) -> &'static str {
        match self {
            RoomKind::Market => "Market",
            RoomKind::Wharf => "Wharf",
            RoomKind::Dockmaster => "Office",
            RoomKind::NoticeWall => "Notice wall",
            RoomKind::Canteen => "Canteen",
            RoomKind::Greenhouse => "Greenhouse",
            RoomKind::Berths => "Berths",
            RoomKind::Counter => "Counter",
        }
    }
}

/// Whether a part is open for business.
#[derive(
    Clone,
    Copy,
    Debug,
    Default,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum PartState {
    #[default]
    Open,
    /// Shut for now: a scaffold over the module and a closed sign.
    Closed,
    /// Going up: a scaffold and a dashed sign.
    Building,
}

/// Who changed a part and why: the pack and the facts the keeper's line cites.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartCause {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<PackId>,
    pub facts: Vec<FactId>,
}

/// The station key's ribbon for a part that changed: its kind and the player's
/// docking count when it changed. It clears after `station.ribbon_dockings` more
/// dockings (counted dockings, never time).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PartNews {
    pub ribbon: Ribbon,
    pub docking: u64,
}

/// C's news ribbons.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Ribbon {
    New,
    Changed,
    Closed,
}

impl Ribbon {
    pub fn word(self) -> &'static str {
        match self {
            Ribbon::New => "New",
            Ribbon::Changed => "Changed",
            Ribbon::Closed => "Closed",
        }
    }

    /// The ribbon for a write that turns `old` into `new`.
    pub fn of(old: Option<&StationPart>, new: &StationPart) -> Ribbon {
        match (old, new.state) {
            (_, PartState::Closed) => Ribbon::Closed,
            (None, _) => Ribbon::New,
            (Some(_), _) => Ribbon::Changed,
        }
    }
}

/// `packs/core/data/station_parts.toml`: every authored station's parts.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct StationPartsFile {
    #[serde(default)]
    pub part: Vec<StationPart>,
}

/// The pacing tunable (`pacing.toml`) for how many dockings a ribbon stays.
pub const RIBBON_DOCKINGS: &str = "station.ribbon_dockings";
