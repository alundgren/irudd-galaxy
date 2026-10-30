//! Legibility channels: dock talk, map notes, the chronicle.
//!
//! The rule is fixed: every line names a person or a place (or both) and cites at
//! least one history fact. A line that can't cite one is not said. Lines are offered
//! with `Command::Say` and kept in the world's [`TalkLog`]; the UI (W6) shows them.

use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::*;

/// A script's fitting authored line, with slots already filled from `cite`.
/// The mind may select its ID; it never supplies text for these requests.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DockTalkCandidate {
    pub line: AnyId,
    pub npc: NpcId,
    pub text: String,
    pub cite: FactId,
    pub specificity: u32,
}

/// Saved with the request event, so an unanswered docking survives save/load.
/// Candidate zero is the script's complete default; order is preserved.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DockTalkRequest {
    pub pack: PackId,
    pub revision: String,
    pub docking: FactId,
    pub ship: ShipId,
    pub station: StationId,
    pub candidates: Vec<DockTalkCandidate>,
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Channel {
    /// A line a person offers when the player docks where they are.
    DockTalk,
    /// A note pinned to a place on the map.
    MapNote,
    /// An entry in the chronicle of the save. History: a rollback can't remove it.
    Chronicle,
}

impl Channel {
    pub const fn as_str(self) -> &'static str {
        match self {
            Channel::DockTalk => "dock_talk",
            Channel::MapNote => "map_note",
            Channel::Chronicle => "chronicle",
        }
    }
}

/// One line of text in the world.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TalkLine {
    pub channel: Channel,
    /// Who says it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<NpcId>,
    /// Where it is said, or what place it is about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<StationId>,
    pub text: String,
    /// The facts this line is about. Never empty.
    #[schemars(length(min = 1))]
    pub cites: Vec<FactId>,
    /// Changes this line is about, e.g. the causes a rollback undid.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub about: Vec<CauseId>,
    /// Answers the player may give (dock talk only). Picking one sends a
    /// `PlayerInput::replies` entry naming the speaker and the choice's ID, which the
    /// sim records as a `reply` event.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<TalkChoice>,
}

/// One answer a line offers.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TalkChoice {
    /// Stable ID, e.g. `choice.side_coop`.
    pub id: AnyId,
    /// What the player says, in a few words.
    pub text: String,
}

/// Most choices one line offers.
pub const MAX_CHOICES: usize = 4;
/// Longest choice text, in bytes.
pub const MAX_CHOICE_LEN: usize = 80;

/// Longest line, in bytes.
pub const MAX_LINE_LEN: usize = 400;

impl TalkLine {
    /// Checks the legibility rule: a person or a place, some text, at least one cited
    /// fact. Whether the cited facts exist is checked where history is visible (the
    /// script host and the game-master bridge).
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.cites.is_empty() {
            Err("a line must cite at least one history fact")
        } else if self.speaker.is_none() && self.place.is_none() {
            Err("a line must name a person (`speaker`) or a place (`place`)")
        } else if self.channel == Channel::MapNote && self.place.is_none() {
            Err("a map note must name a place")
        } else if self.text.trim().is_empty() {
            Err("a line needs text")
        } else if self.text.len() > MAX_LINE_LEN {
            Err("a line is at most 400 bytes")
        } else if !self.choices.is_empty() && self.channel != Channel::DockTalk {
            Err("only dock talk offers choices")
        } else if !self.choices.is_empty() && self.speaker.is_none() {
            Err("a line with choices needs a speaker to answer")
        } else if self.choices.len() > MAX_CHOICES {
            Err("a line offers at most 4 choices")
        } else if self
            .choices
            .iter()
            .any(|c| c.text.trim().is_empty() || c.text.len() > MAX_CHOICE_LEN)
        {
            Err("a choice needs text of at most 80 bytes")
        } else {
            Ok(())
        }
    }
}

/// A line as the world keeps it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct SaidLine {
    pub id: TalkId,
    /// When it was said.
    pub tick: Tick,
    pub line: TalkLine,
}

/// Every line said in this save, by ID.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct TalkLog {
    /// The last line's sequence number (0 before the first).
    pub last: u64,
    pub lines: BTreeMap<TalkId, SaidLine>,
}

impl TalkLog {
    /// Lines on one channel, oldest first.
    pub fn on(&self, channel: Channel) -> impl Iterator<Item = &SaidLine> {
        self.lines
            .values()
            .filter(move |l| l.line.channel == channel)
    }
}
