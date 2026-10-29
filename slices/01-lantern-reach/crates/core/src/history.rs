//! The in-memory history log and recorded causes. Append-only; the save mirrors it in
//! indexed tables.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct History {
    facts: Vec<Fact>,
    causes: BTreeMap<CauseId, RecordedCause>,
}

impl History {
    /// Rebuilds from stored rows (facts in ID order).
    pub fn from_parts(facts: Vec<Fact>, causes: Vec<RecordedCause>) -> Self {
        Self {
            facts,
            causes: causes.into_iter().map(|c| (c.id, c)).collect(),
        }
    }

    /// Every fact, oldest first.
    pub fn facts(&self) -> &[Fact] {
        &self.facts
    }

    pub fn causes(&self) -> &BTreeMap<CauseId, RecordedCause> {
        &self.causes
    }

    pub fn fact(&self, id: FactId) -> Option<&Fact> {
        self.facts
            .binary_search_by_key(&id, |f| f.id)
            .ok()
            .map(|i| &self.facts[i])
    }

    pub fn cause(&self, id: CauseId) -> Option<&RecordedCause> {
        self.causes.get(&id)
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn last_fact_id(&self) -> Option<FactId> {
        self.facts.last().map(|f| f.id)
    }

    /// How many facts match.
    pub fn count(&self, q: &HistoryQuery) -> usize {
        self.facts.iter().filter(|f| q.matches(f)).count()
    }

    /// Sum of `amount` over matching facts.
    pub fn sum(&self, q: &HistoryQuery) -> i64 {
        self.facts
            .iter()
            .filter(|f| q.matches(f))
            .filter_map(|f| f.amount)
            .sum()
    }

    /// Matching facts, oldest first, at most `q.limit`.
    pub fn find(&self, q: &HistoryQuery) -> Vec<&Fact> {
        let limit = q.limit.map_or(usize::MAX, |n| n as usize);
        self.facts
            .iter()
            .filter(|f| q.matches(f))
            .take(limit)
            .collect()
    }

    /// The most recent matching fact.
    pub fn last(&self, q: &HistoryQuery) -> Option<&Fact> {
        self.facts.iter().rev().find(|f| q.matches(f))
    }

    pub(crate) fn record_cause(&mut self, cause: RecordedCause) {
        self.causes.insert(cause.id, cause);
    }

    pub(crate) fn append(
        &mut self,
        tick: Tick,
        kind: FactKind,
        cause: CauseId,
        subjects: Subjects,
        amount: Option<i64>,
        payload: FactPayload,
    ) -> FactId {
        let id = FactId(self.last_fact_id().map_or(1, |f| f.0 + 1));
        self.facts.push(Fact {
            id,
            tick,
            kind,
            cause,
            subjects,
            amount,
            payload,
        });
        id
    }
}
