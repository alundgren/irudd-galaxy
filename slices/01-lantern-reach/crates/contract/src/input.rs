//! Per-tick player input. The app (keyboard and mouse) and the test bot both build a
//! `PlayerInput` each tick and feed it to the sim the same way.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::economy::TradeRequest;
use crate::geom::Vec2;
use crate::ids::*;
use crate::progress::ServiceRequest;

/// What the player is doing this tick. `Default` is "hands off".
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct PlayerInput {
    /// Thrust axes, each in -1..=1 (WASD). Arcade flight: zero means stop.
    #[serde(default)]
    pub thrust: Vec2,
    /// The context action (Space) is held: mine, dock, whatever is in reach.
    #[serde(default)]
    pub action: bool,
    /// A click this tick, once click controls are unlocked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click: Option<Click>,
    /// Trades the player asks for this tick (the dock screen). The economy system
    /// executes them in order and drops invalid ones, so check them first with
    /// `galaxy_economy::check_trade` or `quote`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub trades: Vec<TradeRequest>,
    /// Purchases asked for at the dock this tick: upgrades, tenders, prestige. The
    /// progress system runs each as one atomic batch; check them first with
    /// `galaxy_progress::services::check`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub services: Vec<ServiceRequest>,
    /// Answers the player gives at the dock this tick: a choice a line offered, or a
    /// standing question. Each becomes a `reply` event when the player is docked
    /// where that person is; others are dropped.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub replies: Vec<Reply>,
}

/// One answer at the dock: to whom, and which choice (a line's `choices` ID, or a
/// standing question such as `choice.what_changed`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reply {
    pub npc: NpcId,
    pub choice: AnyId,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Click {
    pub target: ClickTarget,
    /// Shift held: append to the queue instead of replacing it.
    #[serde(default)]
    pub queue: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ClickTarget {
    Point { at: Vec2 },
    Station { station: StationId },
    Rock { rock: RockId },
    Region { region: RegionId },
}
