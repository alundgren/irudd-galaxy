//! Offline minds. [`FakeMind`] is seeded and deterministic (tests, the journey bot);
//! [`RecordedMind`] replays the decisions a save already made.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use serde::{Deserialize, Serialize};

use crate::mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Question, Reply, wire_id,
};
use crate::situation::{OutcomeNote, Source};

/// What a fake call is said to cost, so budgets are exercised offline.
pub const FAKE_COST_USD: f64 = 0.00002;

/// A seeded stand-in model. Same seed and same situation key, same answer. It favours
/// the utility top choice (about two times in three) and says authored lines as they
/// are, citing the line's own fact.
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

    fn reply<T>(&mut self, value: T) -> Reply<T> {
        self.calls += 1;
        Reply {
            value,
            cost_usd: FAKE_COST_USD,
            model: "fake".into(),
            attempts: 1,
        }
    }
}

impl Mind for FakeMind {
    fn name(&self) -> &str {
        "fake"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Result<Reply<Decision>, MindError> {
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
        Ok(self.reply(value))
    }

    fn line(&mut self, req: &LineRequest) -> Result<Reply<LineDraft>, MindError> {
        let value = LineDraft {
            text: req.authored.clone(),
            cites: vec![req.cite.clone()],
        };
        Ok(self.reply(value))
    }
}

/// Decisions and lines a run made, by situation key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Recording {
    /// Key -> the option ID chosen, with its cost in micro-dollars.
    pub choices: BTreeMap<String, (String, i64)>,
    /// Key -> the line said and its citations.
    pub lines: BTreeMap<String, (LineDraft, i64)>,
}

impl Recording {
    /// Every model answer in a save's history (`mind.*` facts). Defaults are not
    /// recorded: a replay reaches the same default by itself.
    pub fn from_history(facts: &[Fact]) -> Self {
        let mut rec = Self::default();
        for f in facts {
            let Some(note) = OutcomeNote::parse(f) else {
                continue;
            };
            // A line's fact carries the cost of both its stages; replay it at none.
            let cost = if note.line.is_some() {
                0
            } else {
                f.amount.unwrap_or(0)
            };
            if note.source == Source::Model
                && let Some(choice) = note.choice
            {
                rec.choices.insert(note.key, (choice, cost));
            }
            if let Some(line) = note.line
                && line.source == Source::Model
            {
                let draft = LineDraft {
                    text: line.text,
                    cites: line.cites.iter().map(ToString::to_string).collect(),
                };
                rec.lines.insert(line.key, (draft, 0));
            }
        }
        rec
    }
}

/// Replays a [`Recording`]: the recorded answer for a known key, `NotRecorded` (so the
/// host takes the default) for anything else.
#[derive(Clone, Debug)]
pub struct RecordedMind {
    recording: Recording,
}

impl RecordedMind {
    pub fn new(recording: Recording) -> Self {
        Self { recording }
    }
}

fn usd(micro: i64) -> f64 {
    micro as f64 / 1_000_000.0
}

impl Mind for RecordedMind {
    fn name(&self) -> &str {
        "recorded"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Result<Reply<Decision>, MindError> {
        let (choice, cost) = self
            .recording
            .choices
            .get(&req.key)
            .ok_or(MindError::NotRecorded)?;
        let Question::Choice { options, .. } = &req.question else {
            return Err(MindError::NotRecorded);
        };
        let i = options
            .iter()
            .position(|o| o == choice)
            .ok_or(MindError::NotRecorded)?;
        Ok(Reply {
            value: Decision::Choice {
                choice: wire_id(i),
                confidence: None,
            },
            cost_usd: usd(*cost),
            model: "recorded".into(),
            attempts: 1,
        })
    }

    fn line(&mut self, req: &LineRequest) -> Result<Reply<LineDraft>, MindError> {
        let (draft, cost) = self
            .recording
            .lines
            .get(&req.key)
            .ok_or(MindError::NotRecorded)?;
        Ok(Reply {
            value: draft.clone(),
            cost_usd: usd(*cost),
            model: "recorded".into(),
            attempts: 1,
        })
    }
}
