//! Situation keys, outcome and spend notes, and the ledger read back from history.
//!
//! Two kinds of fact record the layer's work:
//!
//! - `mind.spend`, one per call that sent anything, whatever became of it: answered,
//!   malformed, failed, rejected by validation, arriving after its moment passed or
//!   still out at shutdown (booked at a worst-case estimate). Also one per request the
//!   executor refused as busy, with nothing spent, so a replay refuses it too. Its
//!   note is a [`SpendNote`] and its `amount` is the cost in micro-dollars. It is
//!   submitted in a batch of its own, so a rejected outcome never loses its spend.
//! - `mind.decision` and `mind.line`, one per outcome, with an [`OutcomeNote`]. Their
//!   `calls` and `cost` say what the outcome took; budgets don't read them.
//!
//! The [`Ledger`] tallies calls and spend from `mind.spend` facts and the situation
//! cache from outcomes, so both survive save and load without a store of their own:
//! history is the record.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{DECISION_FACT, LINE_FACT};

/// Fact kind of one call's spend.
pub const SPEND_FACT: &str = "mind.spend";

/// A stable key for a situation: `sk.` and 16 hex digits of BLAKE3 over its parts.
pub fn key(parts: &Value) -> String {
    let hash = blake3::hash(parts.to_string().as_bytes());
    format!("sk.{}", &hash.to_hex()[..16])
}

/// Dollars to whole micro-dollars, for fact amounts.
pub fn micros(usd: f64) -> i64 {
    if usd.is_finite() && usd > 0.0 {
        (usd * 1_000_000.0).round() as i64
    } else {
        0
    }
}

/// Where an answer came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// A model answered and the answer passed validation.
    Model,
    /// Reused from an earlier identical situation.
    Cache,
    /// The utility top choice or the authored line.
    Default,
}

/// The JSON note of a `mind.decision` or `mind.line` fact.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct OutcomeNote {
    /// Menu kind: `trader_route`, `dock_talk`, `rumor_spread`.
    pub menu: String,
    /// Whose brief: `npc.*` or `craft.*`.
    pub actor: String,
    /// The request key of the choice.
    pub key: String,
    pub source: Source,
    /// The option applied, or `None` if the moment was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub choice: Option<String>,
    /// The utility top option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    /// Why a fallback happened (`budget`, `busy`, `late`, `timeout`, `invalid: ...`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why: Vec<String>,
    /// Models that answered.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// HTTP attempts made for this outcome, retries included. For reading only: the
    /// budget counts `mind.spend` facts.
    #[serde(default)]
    pub calls: u32,
    /// What this outcome's calls cost, in micro-dollars. For reading only.
    #[serde(default)]
    pub cost: i64,
    /// The budget window the calls were counted in.
    #[serde(default)]
    pub window: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    /// For a spoken line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<LineNote>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineNote {
    /// The authored line ID.
    pub id: String,
    /// The request key of the free phrasing; empty when the line wasn't phrased.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// `Default`: the authored line, slots filled from the cited fact.
    pub source: Source,
    pub text: String,
    pub cites: Vec<FactId>,
}

impl OutcomeNote {
    pub fn parse(fact: &Fact) -> Option<Self> {
        if ![DECISION_FACT, LINE_FACT].contains(&fact.kind.as_str()) {
            return None;
        }
        match &fact.payload {
            FactPayload::Note(n) => serde_json::from_str(n).ok(),
            FactPayload::Event(_) => None,
        }
    }
}

/// Which request of an outcome a call answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// The decision model's pick.
    Pick,
    /// The line model's phrasing.
    Phrase,
    /// A live-layer flourish.
    Live,
}

/// The JSON note of a `mind.spend` fact: one request, everything a replay needs. A
/// request the executor refused as busy is recorded too (no attempts, error `busy`):
/// whether a queue had room is the one thing a replay can't work out for itself.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SpendNote {
    pub menu: String,
    pub actor: String,
    pub stage: Stage,
    /// The request key.
    pub key: String,
    /// The tick the request was made, and its ticket: together they order requests as
    /// they were made, which a replay follows. Tickets carry on across hosts.
    #[serde(default)]
    pub at: u64,
    #[serde(default)]
    pub ticket: u64,
    /// HTTP attempts, retries included.
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    /// The budget window the request was counted in when it was sent.
    pub window: u64,
    /// Ticks from sending the request to handling its answer.
    pub waited: u64,
    /// The answer came after the host had taken the default.
    #[serde(default)]
    pub late: bool,
    /// The host shut down before the answer came. `attempts` is what the request was
    /// allowed and the amount a worst-case estimate (`budget.unsettled_call_usd` per
    /// attempt): a call that may have been billed is never left out.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unsettled: bool,
    /// The raw answer (a `Decision` or `LineDraft`), even one validation dropped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Value>,
    /// Why the call failed: an error tag, with the HTTP status if there was one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
}

impl SpendNote {
    pub fn parse(fact: &Fact) -> Option<Self> {
        if fact.kind.as_str() != SPEND_FACT {
            return None;
        }
        match &fact.payload {
            FactPayload::Note(n) => serde_json::from_str(n).ok(),
            FactPayload::Event(_) => None,
        }
    }
}

/// A cached answer.
#[derive(Clone, Debug, PartialEq)]
pub enum Cached {
    Choice(String),
    Line { text: String, cites: Vec<FactId> },
}

/// Spend tally and situation cache, folded from `mind.*` facts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Ledger {
    calls: BTreeMap<(Option<RegionId>, u64), u32>,
    spent: BTreeMap<Option<RegionId>, i64>,
    total: i64,
    cache: BTreeMap<String, Cached>,
    seen: Option<FactId>,
    /// The highest request ticket recorded, so a new host carries on after it.
    last_ticket: u64,
}

impl Ledger {
    /// Folds in every fact after the last one seen.
    pub fn catch_up(&mut self, facts: &[Fact]) {
        let from = match self.seen {
            Some(id) => facts.partition_point(|f| f.id <= id),
            None => 0,
        };
        for f in &facts[from..] {
            self.observe(f);
        }
    }

    /// The last fact folded in.
    pub fn cursor(&self) -> Option<FactId> {
        self.seen
    }

    pub fn observe(&mut self, fact: &Fact) {
        self.seen = Some(fact.id);
        if let Some(spend) = SpendNote::parse(fact) {
            let cost = fact.amount.unwrap_or(0).max(0);
            self.last_ticket = self.last_ticket.max(spend.ticket);
            *self
                .calls
                .entry((spend.region.clone(), spend.window))
                .or_default() += spend.attempts;
            *self.spent.entry(spend.region).or_default() += cost;
            self.total += cost;
            return;
        }
        let Some(note) = OutcomeNote::parse(fact) else {
            return;
        };
        if note.source != Source::Default
            && let Some(choice) = &note.choice
        {
            self.cache
                .insert(note.key.clone(), Cached::Choice(choice.clone()));
        }
        if let Some(line) = note.line
            && line.source != Source::Default
        {
            self.cache.insert(
                line.key,
                Cached::Line {
                    text: line.text,
                    cites: line.cites,
                },
            );
        }
    }

    pub fn calls(&self, region: Option<&RegionId>, window: u64) -> u32 {
        self.calls
            .get(&(region.cloned(), window))
            .copied()
            .unwrap_or(0)
    }

    /// Micro-dollars spent in a region.
    pub fn spent(&self, region: Option<&RegionId>) -> i64 {
        self.spent.get(&region.cloned()).copied().unwrap_or(0)
    }

    /// Micro-dollars spent in the whole save.
    pub fn total(&self) -> i64 {
        self.total
    }

    /// The highest request ticket in history (0 if none).
    pub fn last_ticket(&self) -> u64 {
        self.last_ticket
    }

    pub fn cached(&self, key: &str) -> Option<&Cached> {
        self.cache.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keys_are_stable_and_distinct() {
        let a = key(&json!({"menu": "trader_route", "facts": ["fact.1"]}));
        assert_eq!(
            a,
            key(&json!({"menu": "trader_route", "facts": ["fact.1"]}))
        );
        assert_ne!(
            a,
            key(&json!({"menu": "trader_route", "facts": ["fact.2"]}))
        );
        assert!(a.starts_with("sk.") && a.len() == 19);
    }

    #[test]
    fn micros_round() {
        assert_eq!(micros(0.0000185), 19);
        assert_eq!(micros(-1.0), 0);
        assert_eq!(micros(f64::NAN), 0);
    }
}
