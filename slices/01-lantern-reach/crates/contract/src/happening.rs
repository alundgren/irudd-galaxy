//! Happenings (issue #47, 4.1 item 4): a short piece of visible life at a station, the
//! "place" half of a change. A festival of lamps when Kettle's greenhouse ring goes
//! up, a crowd at the notice wall, barges queueing in a shortage.
//!
//! Each one is data: the station, what the view shows (`show`), how many of the
//! player's dockings there it lasts (`lasts`, counted, never timed), the facts that
//! cause it (`cause`) and the line its speaker says when it starts (`line`). The core
//! pack authors them in `data/happenings.toml` ([`HappeningsFile`]); a game-master pack
//! writes them through [`Command::PutHappening`](crate::command::Command) with
//! `ctx.happenings:put`.
//!
//! The runtime (`galaxy_progress::happenings`) starts an upcoming happening once every
//! cause fact exists and the player is docked at its station with no other happening
//! running there (at most one at a time per station; a new one waits). The speaker
//! says the line in dock talk and a `happening_started` fact is recorded. It runs for
//! `lasts.dockings` counted dockings at the station (the kernel's guarded dockings
//! counter, so tapping out of the ring and back in doesn't burn it), then ends with a
//! `happening_ended` fact. `galaxy_core::happenings::check` validates every one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::ids::{FactId, HappeningId, NpcId, PackId, StationId, Tick};

/// The fact kind recorded when a happening starts: `station`, `npc` = the speaker,
/// `other` = the happening, `amount` = the dockings it lasts.
pub const STARTED_FACT: &str = "happening_started";
/// The fact kind recorded when a happening ends: `station`, `other` = the happening,
/// `amount` = the dockings it ran.
pub const ENDED_FACT: &str = "happening_ended";

/// What a `show` entry may name: `<kind>:<value>`. The view draws the kinds it knows
/// and skips the rest.
pub const SHOW_KINDS: &[&str] = &[
    "banner_string",
    "crowd",
    "residents",
    "lamps",
    "prop",
    "craft",
    "sound",
];

/// One happening.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Happening {
    pub id: HappeningId,
    pub station: StationId,
    /// What the docked view shows while it runs, as `<kind>:<value>` entries
    /// ([`SHOW_KINDS`]): `"banner_string:lamps"`, `"crowd:pier.north"`,
    /// `"residents:+6"`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub show: Vec<String>,
    pub lasts: Lasts,
    pub cause: HappeningCause,
    /// What its speaker says when it starts.
    pub line: HappeningLine,
    /// The pack that wrote it, if a pack did. `None`: the core pack's data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<PackId>,
    /// Where it is in its run. Written by the runtime only; data and packs leave it
    /// upcoming.
    #[serde(default, skip_serializing_if = "HappeningRun::is_upcoming")]
    pub run: HappeningRun,
}

impl Happening {
    pub fn is_active(&self) -> bool {
        matches!(self.run, HappeningRun::Active { .. })
    }
}

/// How long a happening runs: counted player dockings at its station.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Lasts {
    #[schemars(range(min = 1))]
    pub dockings: u32,
}

/// The facts a happening waits for. It starts once each one exists.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HappeningCause {
    #[schemars(length(min = 1))]
    pub facts: Vec<CauseFact>,
}

/// One fact a happening waits for: a fact by ID (it must exist when the happening is
/// written), or a pattern a fact has to match. A pattern names the fact's `kind`
/// (`stage_built` matches `core.stage_built` and `gm.kettle_growth.stage_built`, as
/// line citations do) and any subjects or amount it must have.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CauseFact {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<FactId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub station: Option<StationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npc: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub faction: Option<crate::ids::FactionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ware: Option<crate::ids::WareId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub other: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
}

impl CauseFact {
    /// Whether `fact` is this one: the same ID, or it matches the pattern.
    pub fn matches(&self, fact: &crate::history::Fact) -> bool {
        if let Some(id) = self.id {
            return fact.id == id;
        }
        let s = &fact.subjects;
        self.kind
            .as_deref()
            .is_some_and(|k| same_kind(k, fact.kind.as_str()))
            && self.amount.is_none_or(|a| fact.amount == Some(a))
            && self
                .station
                .as_ref()
                .is_none_or(|x| s.station.as_ref() == Some(x))
            && self.npc.as_ref().is_none_or(|x| s.npc.as_ref() == Some(x))
            && self
                .faction
                .as_ref()
                .is_none_or(|x| s.faction.as_ref() == Some(x))
            && self
                .ware
                .as_ref()
                .is_none_or(|x| s.ware.as_ref() == Some(x))
            && self
                .other
                .as_ref()
                .is_none_or(|x| s.other.as_ref().is_some_and(|o| o.as_str() == x))
    }
}

/// Fact kinds match whole, or by their last segment (`stage_built` and
/// `core.stage_built`), the way authored lines cite them.
pub fn same_kind(want: &str, have: &str) -> bool {
    want == have
        || have.rsplit_once('.').is_some_and(|(_, k)| k == want)
        || want.rsplit_once('.').is_some_and(|(_, k)| k == have)
}

/// The line a happening's speaker says when it starts, in the first person.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HappeningLine {
    /// `line.<speaker>.<name>`, stable, so a line said twice can be told apart.
    pub id: String,
    /// A person at the station.
    pub speaker: NpcId,
    pub text: String,
    /// Answers the player may give (at most four). A pack's `on.reply` handler acts on
    /// the pick, the way any dock-talk offer works (the core pack's harvest gift).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<crate::talk::TalkChoice>,
}

/// Where a happening is in its run.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum HappeningRun {
    /// Waiting for its cause facts, or for the station to be free.
    #[default]
    Upcoming,
    /// Running. `first_docking` is the station's counted dockings when it started
    /// (the docking it started in counts as its first); it ends when the count
    /// reaches `first_docking + lasts.dockings`. `cites` are the cause facts that
    /// started it.
    Active {
        since: Tick,
        first_docking: u64,
        cites: Vec<FactId>,
    },
    /// Over.
    Ended {
        since: Tick,
        first_docking: u64,
        at: Tick,
    },
}

impl HappeningRun {
    pub fn is_upcoming(&self) -> bool {
        matches!(self, HappeningRun::Upcoming)
    }
}

/// `packs/core/data/happenings.toml`: the authored happenings.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HappeningsFile {
    #[serde(default)]
    pub happening: Vec<Happening>,
}
