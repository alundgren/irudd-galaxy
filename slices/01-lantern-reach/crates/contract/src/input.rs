//! Per-tick player input. The app (keyboard and mouse) and the test bot both build a
//! `PlayerInput` each tick and feed it to the sim the same way.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::geom::Vec2;
use crate::ids::*;

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
