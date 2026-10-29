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

/// A short line for a chat model with structured output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineRequest {
    pub key: String,
    /// Who speaks and how: built from the NPC's brief.
    pub system: String,
    /// The grounding packet.
    pub packet: Value,
    /// The authored line to say in the NPC's voice. Used word for word on failure.
    pub authored: String,
    /// The fact the line is about. The answer must cite it.
    pub cite: String,
    /// Every fact ID in the packet; nothing else may be cited.
    pub allowed_cites: Vec<String>,
    pub max_words: u32,
}

/// A model's line, before validation.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineDraft {
    pub text: String,
    pub cites: Vec<String>,
}

/// A successful answer with what it cost.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Reply<T> {
    pub value: T,
    /// US dollars, from the response's `usage.cost`.
    pub cost_usd: f64,
    /// The model that answered.
    pub model: String,
    /// HTTP attempts made (1, or 2 after a retry).
    pub attempts: u8,
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
    fn decide(&mut self, req: &DecisionRequest) -> Result<Reply<Decision>, MindError>;
    fn line(&mut self, req: &LineRequest) -> Result<Reply<LineDraft>, MindError>;
}

impl<M: Mind + ?Sized> Mind for Box<M> {
    fn name(&self) -> &str {
        (**self).name()
    }
    fn decide(&mut self, req: &DecisionRequest) -> Result<Reply<Decision>, MindError> {
        (**self).decide(req)
    }
    fn line(&mut self, req: &LineRequest) -> Result<Reply<LineDraft>, MindError> {
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
