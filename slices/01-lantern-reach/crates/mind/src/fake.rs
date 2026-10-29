//! Offline minds. [`FakeMind`] is seeded and deterministic (tests, the journey bot);
//! [`RecordedMind`] replays the calls a save already made.

use std::collections::{BTreeMap, VecDeque};

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Question, Reply, wire_id,
};
use crate::situation::{SpendNote, Stage};

/// What a fake call is said to cost, so budgets are exercised offline.
pub const FAKE_COST_USD: f64 = 0.00002;

/// A seeded stand-in model. Same seed and same situation key, same answer. It favours
/// the utility top choice (about two times in three) and says authored lines as they
/// are, slots kept, citing the line's own fact.
#[derive(Clone, Debug)]
pub struct FakeMind {
    seed: u64,
    calls: u64,
}

impl FakeMind {
    pub fn new(seed: u64) -> Self {
        Self { seed, calls: 0 }
    }

    /// Calls answered so far.
    pub fn calls(&self) -> u64 {
        self.calls
    }

    fn roll(&self, key: &str) -> u64 {
        let mut h = blake3::Hasher::new();
        h.update(&self.seed.to_le_bytes());
        h.update(key.as_bytes());
        let b = h.finalize();
        u64::from_le_bytes(b.as_bytes()[..8].try_into().expect("8 bytes"))
    }

    fn reply<T>(&mut self, max_attempts: u8, value: T) -> Reply<T> {
        if max_attempts == 0 {
            return Reply::none(MindError::NotRecorded);
        }
        self.calls += 1;
        Reply::ok(value, FAKE_COST_USD, "fake")
    }
}

impl Mind for FakeMind {
    fn name(&self) -> &str {
        "fake"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision> {
        let r = self.roll(&req.key);
        let value = match &req.question {
            Question::Choice { criteria, .. } => {
                let n = criteria.len().max(1) as u64;
                let i = if r % 3 < 2 { 0 } else { (r / 3) % n };
                Decision::Choice {
                    choice: wire_id(i as usize),
                    confidence: Some(0.5),
                }
            }
            Question::YesNo { .. } => Decision::YesNo {
                p_yes: (r % 101) as f64 / 100.0,
            },
            Question::Score { scale } => Decision::Score {
                score: (r % scale.len().max(1) as u64) as f64,
            },
        };
        self.reply(req.max_attempts, value)
    }

    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        let value = LineDraft {
            text: req.template.clone(),
            cites: vec![req.cite.clone()],
        };
        self.reply(req.max_attempts, value)
    }
}

/// One recorded call: what came back, what it cost and when it was handled.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Occurrence {
    /// The raw answer (a `Decision` or `LineDraft` as JSON), if one parsed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<Value>,
    /// The error tag and HTTP status if the call failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    pub attempts: u8,
    /// Micro-dollars.
    pub cost: i64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// Ticks between the request and the host handling its answer.
    pub waited: u64,
}

/// Every call a run made, in order, per stage and request key. Failed, malformed,
/// rejected and late calls are in it too, so a replay meets the same answer, the same
/// fallback and the same spend on each occurrence of a situation. Calls that sent
/// nothing (no key, budget, cache) aren't: a replay reaches those by itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recording {
    pub calls: BTreeMap<(Stage, String), VecDeque<Occurrence>>,
}

impl Recording {
    /// Every call in a save's history (`mind.spend` facts), in the order made.
    pub fn from_history(facts: &[Fact]) -> Self {
        let mut rec = Self::default();
        for f in facts {
            let Some(note) = SpendNote::parse(f) else {
                continue;
            };
            rec.calls
                .entry((note.stage, note.key))
                .or_default()
                .push_back(Occurrence {
                    answer: note.answer,
                    error: note.error,
                    status: note.status,
                    attempts: note.attempts.min(u32::from(u8::MAX)) as u8,
                    cost: f.amount.unwrap_or(0),
                    model: note.model,
                    waited: note.waited,
                });
        }
        rec
    }

    /// Occurrences recorded in all.
    pub fn len(&self) -> usize {
        self.calls.values().map(VecDeque::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// Replays a [`Recording`]: the next recorded occurrence for the request's stage and
/// key, held back as many ticks as the original took; `NotRecorded` (nothing sent, so
/// the host takes the default) once a situation's occurrences run out.
#[derive(Clone, Debug)]
pub struct RecordedMind {
    recording: Recording,
}

impl RecordedMind {
    pub fn new(recording: Recording) -> Self {
        Self { recording }
    }

    fn next<T: for<'de> Deserialize<'de>>(&mut self, stage: Stage, key: &str) -> Reply<T> {
        let Some(o) = self
            .recording
            .calls
            .get_mut(&(stage, key.to_owned()))
            .and_then(VecDeque::pop_front)
        else {
            return Reply::none(MindError::NotRecorded);
        };
        let result = match (&o.error, o.answer) {
            (Some(tag), _) => Err(MindError::from_tag(tag, o.status)),
            (None, Some(v)) => serde_json::from_value(v)
                .map_err(|e| MindError::Malformed(format!("recorded answer: {e}"))),
            (None, None) => Err(MindError::NotRecorded),
        };
        Reply {
            result,
            cost_usd: o.cost as f64 / 1_000_000.0,
            model: o.model,
            attempts: o.attempts,
            hold_ticks: o.waited,
        }
    }
}

impl Mind for RecordedMind {
    fn name(&self) -> &str {
        "recorded"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision> {
        self.next(Stage::Pick, &req.key)
    }

    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        self.next(Stage::Phrase, &req.key)
    }
}
