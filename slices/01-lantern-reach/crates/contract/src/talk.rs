//! Legibility channels: dock talk, map notes, the chronicle.
//!
//! Placeholder owned by W1, filled in by W5. The rule is fixed: every line cites at
//! least one history fact. A line that can't cite one is not said.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    DockTalk,
    MapNote,
    Chronicle,
}

/// One line of text in the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TalkLine {
    pub channel: Channel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<StationId>,
    pub text: String,
    /// The facts this line is about. Never empty.
    #[schemars(length(min = 1))]
    pub cites: Vec<FactId>,
}

impl TalkLine {
    /// Checks the citation rule.
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.cites.is_empty() {
            Err("a talk line must cite at least one history fact")
        } else {
            Ok(())
        }
    }
}
