//! The [`Mind`] trait and the values that cross it.
//!
//! A mind answers one request at a time and may block (an HTTP call). It never sees
//! the world: requests carry a grounding packet as JSON, and answers are plain values
//! that [`crate::host::MindHost`] validates and turns into commands.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A typed choice for a decision model (TypeSafe Jev).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DecisionRequest {
    /// The situation key: same key, same situation (see [`crate::situation`]).
    pub key: String,
    pub instructions: String,
    /// The grounding packet.
    pub state: Value,
    pub question: Question,
    /// HTTP attempts the budget allows, the retry included. The mind never sends more.
    pub max_attempts: u8,
    /// Wall time until the host gives up on the answer. A `Retry-After` longer than
    /// what is left skips the moment instead of waiting.
    pub deadline_ms: u64,
}

/// What is asked. Option IDs on the wire are short (`o1`, `o2` ...) and map back to
/// menu options by position.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Question {
    /// Pick one option: wire ID -> what it means. `options` is the menu's option IDs
    /// in the same order, so a recording can map answers back.
    Choice {
        criteria: BTreeMap<String, String>,
        options: Vec<String>,
    },
    /// Yes or no.
    YesNo { yes: String, no: String },
    /// A position on an ordered scale, low to high.
    Score { scale: Vec<String> },
}

impl Question {
    /// A choice over `texts`, wire IDs `o1..`.
    pub fn choice(options: &[(String, String)]) -> Self {
        Question::Choice {
            criteria: options
                .iter()
                .enumerate()
                .map(|(i, (_, text))| (wire_id(i), text.clone()))
                .collect(),
            options: options.iter().map(|(id, _)| id.clone()).collect(),
        }
    }
}

/// The wire ID of option `i` (0-based): `o1`, `o2` ...
pub fn wire_id(i: usize) -> String {
    format!("o{}", i + 1)
}

/// Parses `o<n>` back to a 0-based index.
pub fn wire_index(id: &str) -> Option<usize> {
    id.strip_prefix('o')?.parse::<usize>().ok()?.checked_sub(1)
}

/// A decision model's answer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Decision {
    Choice {
        /// The wire ID picked.
        choice: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        confidence: Option<f64>,
    },
    YesNo {
        /// Probability of yes.
        p_yes: f64,
    },
    Score {
        /// Position on the scale, 0 = first entry.
        score: f64,
    },
}

/// HTTP attempts a request may make at most: one try and one retry.
pub const MAX_ATTEMPTS: u8 = 2;

/// A short line for a chat model with structured output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineRequest {
    pub key: String,
    /// Who speaks and how: built from the NPC's brief.
    pub system: String,
    /// The grounding packet.
    pub packet: Value,
    /// The authored line to say in the NPC's voice, slots filled. Used word for word on
    /// failure.
    pub authored: String,
    /// The same line with its slots (`{qty}`, `{ware}` ...) left in. The model keeps each
    /// slot as written and the game fills it, so facts come from the world, not the model.
    pub template: String,
    /// Slot name (without braces) -> what the game fills in.
    pub slots: BTreeMap<String, String>,
    /// The fact the line is about. The answer must cite it.
    pub cite: String,
    /// Every fact ID in the packet; nothing else may be cited.
    pub allowed_cites: Vec<String>,
    pub max_words: u32,
    /// As for [`DecisionRequest::max_attempts`].
    pub max_attempts: u8,
    /// As for [`DecisionRequest::deadline_ms`].
    pub deadline_ms: u64,
}

/// A model's line, before validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineDraft {
    pub text: String,
    pub cites: Vec<String>,
}

/// What one call came to: the answer or why there was none, and what it cost either
/// way. A failed, malformed or retried call can still be billed, and every billed
/// response is recorded, so the cost travels with the error too.
#[derive(Clone, Debug, PartialEq)]
pub struct Reply<T> {
    pub result: Result<T, MindError>,
    /// US dollars, summed over every attempt's `usage.cost`.
    pub cost_usd: f64,
    /// The model that answered (empty if nothing did).
    pub model: String,
    /// HTTP attempts made, the retry included (0 if nothing was sent).
    pub attempts: u8,
    /// Replays only: ticks after the request before the host may handle the answer, so
    /// a recorded answer lands when the original did. 0 for live minds.
    pub hold_ticks: u64,
}

impl<T> Reply<T> {
    /// An answer from one attempt.
    pub fn ok(value: T, cost_usd: f64, model: impl Into<String>) -> Self {
        Self {
            result: Ok(value),
            cost_usd,
            model: model.into(),
            attempts: 1,
            hold_ticks: 0,
        }
    }

    /// No answer and nothing sent.
    pub fn none(error: MindError) -> Self {
        Self {
            result: Err(error),
            cost_usd: 0.0,
            model: String::new(),
            attempts: 0,
            hold_ticks: 0,
        }
    }

    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> Reply<U> {
        Reply {
            result: self.result.map(f),
            cost_usd: self.cost_usd,
            model: self.model,
            attempts: self.attempts,
            hold_ticks: self.hold_ticks,
        }
    }
}

/// Why a mind gave no usable answer. The host then takes the utility default.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum MindError {
    #[error("timed out")]
    Timeout,
    #[error("server error {0}")]
    Server(u16),
    #[error("rate limited")]
    RateLimited,
    #[error("transport: {0}")]
    Transport(String),
    #[error("http {0}: {1}")]
    Http(u16, String),
    #[error("malformed answer: {0}")]
    Malformed(String),
    #[error("OPENROUTER_API_KEY is not set")]
    NoKey,
    #[error("no recorded answer for this situation")]
    NotRecorded,
}

impl MindError {
    /// The HTTP status, where there is one.
    pub fn status(&self) -> Option<u16> {
        match self {
            MindError::Server(s) | MindError::Http(s, _) => Some(*s),
            _ => None,
        }
    }

    /// Rebuilds an error from its [`MindError::tag`] and status, for replays.
    pub fn from_tag(tag: &str, status: Option<u16>) -> Self {
        match tag {
            "timeout" => MindError::Timeout,
            "server" => MindError::Server(status.unwrap_or(500)),
            "rate_limited" => MindError::RateLimited,
            "transport" => MindError::Transport("recorded".into()),
            "http" => MindError::Http(status.unwrap_or(400), "recorded".into()),
            "malformed" => MindError::Malformed("recorded".into()),
            "no_key" => MindError::NoKey,
            _ => MindError::NotRecorded,
        }
    }
}

impl MindError {
    /// Worth one more try: the call failed or was slow, not the answer.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            MindError::Timeout
                | MindError::Server(_)
                | MindError::RateLimited
                | MindError::Transport(_)
        )
    }

    /// A short tag for facts and stats.
    pub fn tag(&self) -> &'static str {
        match self {
            MindError::Timeout => "timeout",
            MindError::Server(_) => "server",
            MindError::RateLimited => "rate_limited",
            MindError::Transport(_) => "transport",
            MindError::Http(..) => "http",
            MindError::Malformed(_) => "malformed",
            MindError::NoKey => "no_key",
            MindError::NotRecorded => "not_recorded",
        }
    }
}

/// A fast model. Implementations may block; run them on a [`crate::Worker`] to keep
/// the frame thread free.
pub trait Mind: Send {
    /// A short name for logs (`openrouter`, `fake`, `recorded`).
    fn name(&self) -> &str;
    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision>;
    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft>;
}

impl<M: Mind + ?Sized> Mind for Box<M> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision> {
        (**self).decide(req)
    }
    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        (**self).line(req)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_ids_round_trip() {
        assert_eq!(wire_id(0), "o1");
        assert_eq!(wire_index("o1"), Some(0));
        assert_eq!(wire_index("o12"), Some(11));
        assert_eq!(wire_index("o0"), None);
        assert_eq!(wire_index("x1"), None);
    }
}
