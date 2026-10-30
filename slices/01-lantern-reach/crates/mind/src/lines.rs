//! Authored dock-talk lines (`data/lines.toml`) and the line source the game uses.
//!
//! A line names its speaker, the fact kind it cites and, optionally, which subjects
//! that fact must have (`ware`, `station`, `other`: an upgrade, a trait, a hull ...).
//! [`TalkLines`] is the [`LineSource`] behind dock talk in play. On top of
//! [`crate::LinePool`]'s rule (a fact of the line's kind, newer than
//! the player's previous docking here, every slot filled) it applies the two rules of
//! ambient talk (`05-people.md`):
//!
//! A line's fact may be about another place (Ilse at the Wharf talks about Kettle Dock's
//! dome); a line that must be about this station says so with `station`.
//!
//! - **Retirement**: a fact that any said line already cites (in the world's talk log)
//!   isn't cited again, so a delivery Odo thanked the player for in a script isn't
//!   thanked for twice.
//! - **Reuse cap**: a line said `max_uses` times (`mind.line` facts naming it) retires,
//!   so stock phrasing doesn't wear thin.
//!
//! [`TalkLines::what_changed`] picks a dockmaster's answer to "What changed?" by the
//! same rules.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use galaxy_contract::prelude::*;
use galaxy_core::History;
use serde::Deserialize;

use crate::LINE_FACT;
use crate::menu::{LineCandidate, LinePool, LineSource, MenuCtx};

/// The data file name under a pack's `data/`.
pub const LINES_FILE: &str = "lines.toml";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct LinesFile {
    #[serde(default = "default_max_uses")]
    max_uses: u32,
    #[serde(default)]
    line: Vec<LineDef>,
}

fn default_max_uses() -> u32 {
    3
}

/// One authored line.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LineDef {
    /// `line.<npc>.<nn>`.
    pub id: String,
    pub npc: NpcId,
    /// The fact kind it cites; `<pack>.<kind>` and `<kind>` match each other.
    pub cites: String,
    /// The cited fact must be about this ware.
    #[serde(default)]
    pub ware: Option<WareId>,
    /// ... at this station.
    #[serde(default)]
    pub station: Option<StationId>,
    /// ... about this entity (an upgrade, trait, hull, beat ...).
    #[serde(default)]
    pub other: Option<String>,
    /// Slots: `{qty}`, `{ware}`, `{station}`, filled from the cited fact.
    pub text: String,
}

impl LineDef {
    fn matches(&self, fact: &Fact) -> bool {
        let s = &fact.subjects;
        same_kind(&self.cites, fact.kind.as_str())
            && self.ware.as_ref().is_none_or(|w| s.ware.as_ref() == Some(w))
            && self
                .station
                .as_ref()
                .is_none_or(|st| s.station.as_ref() == Some(st))
            && self
                .other
                .as_ref()
                .is_none_or(|o| s.other.as_ref().is_some_and(|x| x.as_str() == o))
    }

    /// How specific the line is: one for each subject it names.
    fn specificity(&self, fact: &Fact) -> u32 {
        u32::from(fact.subjects.station.is_some())
            + u32::from(self.ware.is_some())
            + u32::from(self.station.is_some())
            + u32::from(self.other.is_some())
    }
}

fn same_kind(want: &str, have: &str) -> bool {
    want == have
        || have.split_once('.').is_some_and(|(_, k)| k == want)
        || want.split_once('.').is_some_and(|(_, k)| k == have)
}

#[derive(Debug, thiserror::Error)]
pub enum LinesError {
    #[error("{LINES_FILE}: {0}")]
    Io(#[from] std::io::Error),
    #[error("{LINES_FILE}: {0}")]
    Toml(#[from] Box<toml::de::Error>),
    #[error("{LINES_FILE}: line `{0}` is listed twice")]
    Duplicate(String),
}

/// The authored lines, with the ambient-talk rules.
#[derive(Clone, Debug, Default)]
pub struct TalkLines {
    pub lines: Vec<LineDef>,
    /// A line is said at most this many times in a save.
    pub max_uses: u32,
}

impl TalkLines {
    /// Reads `lines.toml` from a data folder; no file means no lines.
    pub fn load_dir(data_dir: &Path) -> Result<Self, LinesError> {
        let path = data_dir.join(LINES_FILE);
        if !path.exists() {
            return Ok(Self::default());
        }
        Self::from_toml(&std::fs::read_to_string(path)?)
    }

    pub fn from_toml(text: &str) -> Result<Self, LinesError> {
        let file: LinesFile = toml::from_str(text).map_err(Box::new)?;
        let mut seen = BTreeSet::new();
        for l in &file.line {
            if !seen.insert(l.id.clone()) {
                return Err(LinesError::Duplicate(l.id.clone()));
            }
        }
        Ok(Self {
            lines: file.line,
            max_uses: file.max_uses,
        })
    }

    /// Facts some said line already cites: ambient talk never cites them again.
    fn retired_facts(world: &WorldState) -> BTreeSet<FactId> {
        world
            .talk
            .lines
            .values()
            .flat_map(|l| l.line.cites.iter().copied())
            .collect()
    }

    /// How often each line has been said, from the `mind.line` facts naming it.
    fn uses(history: &History) -> BTreeMap<String, u32> {
        let mut out = BTreeMap::new();
        for f in history
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == LINE_FACT)
        {
            if let Some(o) = &f.subjects.other {
                *out.entry(o.as_str().to_owned()).or_default() += 1;
            }
        }
        out
    }

    /// The candidates for lines by `speakers` (all lines when `None`) that cite one of
    /// `facts`, newest fact first per line, under the ambient rules.
    fn eligible<'f>(
        &self,
        world: &WorldState,
        history: &History,
        facts: impl Iterator<Item = &'f Fact> + Clone,
        speaker: &dyn Fn(&LineDef) -> bool,
    ) -> Vec<LineCandidate> {
        let retired = Self::retired_facts(world);
        let uses = Self::uses(history);
        let mut out = Vec::new();
        for line in self.lines.iter().filter(|l| speaker(l)) {
            if uses.get(&line.id).copied().unwrap_or(0) >= self.max_uses {
                continue;
            }
            let fact = facts.clone().find(|f| {
                !f.kind.as_str().starts_with("mind.") && !retired.contains(&f.id) && line.matches(f)
            });
            if let Some(f) = fact
                && let Some((text, slots)) = LinePool::fill(world, &line.text, f)
            {
                out.push(LineCandidate {
                    line: line.id.clone(),
                    npc: line.npc.clone(),
                    cites: f.id,
                    text,
                    template: line.text.clone(),
                    slots,
                    specificity: line.specificity(f),
                });
            }
        }
        out
    }

    /// The player's last docking at `station` before `before`.
    fn previous_docking(history: &History, world: &WorldState, station: &StationId, before: FactId) -> FactId {
        let ship = world.player.ship.as_ref();
        history
            .facts()
            .iter()
            .rev()
            .filter(|f| f.id < before)
            .find(|f| match &f.payload {
                FactPayload::Event(Event::Dock { ship: s, station: at }) => {
                    Some(s) == ship && at == station
                }
                _ => false,
            })
            .map_or(FactId(0), |f| f.id)
    }

    /// A dockmaster's answer to "What changed?": up to `limit` of `speaker`'s lines
    /// citing facts after `since` (the player's previous docking there), at most one
    /// per place, most specific first, then the newest fact.
    pub fn what_changed(
        &self,
        world: &WorldState,
        history: &History,
        since: FactId,
        speaker: &NpcId,
        limit: usize,
    ) -> Vec<LineCandidate> {
        let facts = history.facts().iter().rev().take_while(|f| f.id > since);
        let mut lines = self.eligible(world, history, facts, &|l| &l.npc == speaker);
        sort(&mut lines);
        let mut places = BTreeSet::new();
        lines
            .into_iter()
            .filter(|l| {
                let place = history.fact(l.cites).and_then(|f| f.subjects.station.clone());
                places.insert(place)
            })
            .take(limit)
            .collect()
    }
}

/// Most specific first, then the newest fact.
fn sort(lines: &mut [LineCandidate]) {
    lines.sort_by(|a, b| {
        b.specificity
            .cmp(&a.specificity)
            .then(b.cites.cmp(&a.cites))
            .then_with(|| a.line.cmp(&b.line))
    });
}

impl LineSource for TalkLines {
    fn candidates(&self, cx: &MenuCtx<'_>, station: &StationId, dock: &Fact) -> Vec<LineCandidate> {
        let since = Self::previous_docking(cx.history, cx.world, station, dock.id);
        let facts = cx
            .history
            .facts()
            .iter()
            .rev()
            .take_while(|f| f.id > since);
        let here = |l: &LineDef| {
            cx.world
                .npcs
                .get(&l.npc)
                .is_some_and(|n| n.location.as_ref() == Some(station))
        };
        self.eligible(cx.world, cx.history, facts, &here)
    }
}
