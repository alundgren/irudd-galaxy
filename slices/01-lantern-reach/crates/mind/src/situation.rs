//! Situation keys, outcome notes and the ledger read back from history.
//!
//! Every outcome is a `mind.*` fact whose note is an [`OutcomeNote`] (JSON) and whose
//! `amount` is what it cost, in micro-dollars. The [`Ledger`] is rebuilt from those
//! facts, so the spend tally and the situation cache survive save and load without a
//! store of their own: history is the record.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A stable key for a situation: `sk.` and 16 hex digits of BLAKE3 over its parts
/// (menu kind, actor, brief revision, the packet's fact IDs, the options).
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
    /// Model calls made for this outcome (attempts included).
    #[serde(default)]
    pub calls: u32,
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
    /// The request key of the phrasing.
    pub key: String,
    pub source: Source,
    pub text: String,
    pub cites: Vec<FactId>,
}

impl OutcomeNote {
    pub fn parse(fact: &Fact) -> Option<Self> {
        if !fact.kind.as_str().starts_with("mind.") {
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
        let Some(note) = OutcomeNote::parse(fact) else {
            return;
        };
        let cost = fact.amount.unwrap_or(0).max(0);
        *self
            .calls
            .entry((note.region.clone(), note.window))
            .or_default() += note.calls;
        *self.spent.entry(note.region.clone()).or_default() += cost;
        self.total += cost;
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
