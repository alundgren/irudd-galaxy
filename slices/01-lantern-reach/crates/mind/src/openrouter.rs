//! [`OpenRouterMind`]: the real mind, over HTTP.
//!
//! - Typed choices: `POST /api/alpha/decisions` with TypeSafe Jev.
//! - Short lines: `POST /api/v1/chat/completions` with a strict JSON schema and
//!   `provider.require_parameters`, so only providers that honour the schema answer.
//!
//! A timeout, transport error, 429 or 5xx is retried once; then the call fails and the
//! host skips the moment. A malformed answer is not retried. The transport is a trait
//! so tests run against canned responses without a network.

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};

use crate::config::{Limits, Models};
use crate::mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, Mind, MindError, Question, Reply,
};

/// The OpenRouter API key. Never printed: `Debug` and `Display` are redacted.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub const ENV: &'static str = "OPENROUTER_API_KEY";

    /// Reads `OPENROUTER_API_KEY`. The key comes from the environment and nowhere else.
    pub fn from_env() -> Result<Self, MindError> {
        match std::env::var(Self::ENV) {
            Ok(k) if !k.trim().is_empty() => Ok(Self(k.trim().to_owned())),
            _ => Err(MindError::NoKey),
        }
    }

    /// For tests with a fake transport.
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// A raw HTTP answer.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

/// Sends one JSON POST. Errors are only for calls that got no HTTP answer at all.
pub trait Transport: Send {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError>;
}

/// The real transport: `ureq` with rustls.
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl UreqTransport {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self { agent }
    }
}

impl Transport for UreqTransport {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError> {
        let map = |e: ureq::Error| match e {
            ureq::Error::Timeout(_) => MindError::Timeout,
            e => MindError::Transport(e.to_string()),
        };
        let resp = self
            .agent
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .header("X-Title", "Irudd Galaxy")
            .send(body)
            .map_err(map)?;
        let status = resp.status().as_u16();
        let body = resp.into_body().read_to_string().map_err(map)?;
        Ok(HttpReply { status, body })
    }
}

pub struct OpenRouterMind<T: Transport = UreqTransport> {
    transport: T,
    key: ApiKey,
    models: Models,
    base_url: String,
    line_max_tokens: u32,
}

impl OpenRouterMind<UreqTransport> {
    /// The live mind, with the key from `OPENROUTER_API_KEY`.
    pub fn from_env(models: Models, limits: &Limits) -> Result<Self, MindError> {
        let transport = UreqTransport::new(Duration::from_millis(limits.timeout_ms));
        Ok(Self::with_transport(
            transport,
            ApiKey::from_env()?,
            models,
            limits,
        ))
    }
}

impl<T: Transport> OpenRouterMind<T> {
    pub fn with_transport(transport: T, key: ApiKey, models: Models, limits: &Limits) -> Self {
        Self {
            transport,
            key,
            models,
            base_url: "https://openrouter.ai".into(),
            line_max_tokens: limits.line_max_tokens,
        }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// One POST, retried once if the call failed or was slow. Returns the 200 body and
    /// the attempts made.
    fn post(&mut self, path: &str, body: &Value) -> Result<(Value, u8), MindError> {
        let url = format!("{}{path}", self.base_url);
        let body = body.to_string();
        let auth = self.key.bearer();
        let mut attempt = 1;
        loop {
            let result = self
                .transport
                .post(&url, &auth, &body)
                .and_then(|r| match r.status {
                    200..=299 => serde_json::from_str::<Value>(&r.body)
                        .map_err(|e| MindError::Malformed(format!("not JSON: {e}"))),
                    429 => Err(MindError::RateLimited),
                    s @ 500..=599 => Err(MindError::Server(s)),
                    s => Err(MindError::Http(s, short(&r.body))),
                });
            match result {
                Err(e) if e.retryable() && attempt == 1 => attempt += 1,
                Err(e) => return Err(e),
                Ok(v) => {
                    // OpenRouter can report an upstream failure inside a 200.
                    if let Some(err) = v.get("error") {
                        let code = err.get("code").and_then(Value::as_u64).unwrap_or(0);
                        let e = match code {
                            429 => MindError::RateLimited,
                            500..=599 => MindError::Server(code as u16),
                            _ => MindError::Malformed(short(&err.to_string())),
                        };
                        if e.retryable() && attempt == 1 {
                            attempt += 1;
                            continue;
                        }
                        return Err(e);
                    }
                    return Ok((v, attempt));
                }
            }
        }
    }
}

fn short(s: &str) -> String {
    s.chars().take(200).collect()
}

fn cost(v: &Value) -> f64 {
    v.pointer("/usage/cost")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0)
        .unwrap_or(0.0)
}

/// The Decisions API body for one question named `pick`.
pub fn decision_body(model: &str, req: &DecisionRequest) -> Value {
    let question = match &req.question {
        Question::Choice { criteria, .. } => json!({
            "type": "choice",
            "instructions": req.instructions,
            "criteria": criteria,
        }),
        Question::YesNo { yes, no } => json!({
            "type": "noul",
            "instructions": req.instructions,
            "criteria": { "true": yes, "false": no },
        }),
        Question::Score { scale } => json!({
            "type": "score",
            "instructions": req.instructions,
            "criteria": scale,
        }),
    };
    json!({ "model": model, "state": req.state, "questions": { "pick": question } })
}

/// Reads the `pick` answer of a Decisions API response.
pub fn parse_decision(req: &DecisionRequest, v: &Value) -> Result<Decision, MindError> {
    let bad = |m: &str| MindError::Malformed(m.to_owned());
    let a = v
        .pointer("/answers/pick")
        .ok_or_else(|| bad("no answers.pick"))?;
    let ty = a.get("type").and_then(Value::as_str).unwrap_or_default();
    match &req.question {
        Question::Choice { .. } => {
            if ty != "choice" {
                return Err(bad("expected a choice answer"));
            }
            let choice = a
                .get("choice")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("choice is not a string"))?;
            Ok(Decision::Choice {
                choice: choice.to_owned(),
                confidence: a.get("confidence").and_then(Value::as_f64),
            })
        }
        Question::YesNo { .. } => {
            let p = a
                .get("noul")
                .and_then(Value::as_f64)
                .filter(|p| (0.0..=1.0).contains(p))
                .ok_or_else(|| bad("noul is not a probability"))?;
            Ok(Decision::YesNo { p_yes: p })
        }
        Question::Score { .. } => {
            let s = a
                .get("score")
                .and_then(Value::as_f64)
                .filter(|s| s.is_finite())
                .ok_or_else(|| bad("score is not a number"))?;
            Ok(Decision::Score { score: s })
        }
    }
}

/// The chat completions body for one line, with a strict schema that limits `cite`
/// to the packet's fact IDs.
pub fn line_body(model: &str, max_tokens: u32, req: &LineRequest) -> Value {
    let user = json!({
        "packet": req.packet,
        "line": req.authored,
        "cite": req.cite,
    });
    json!({
        "model": model,
        "provider": { "require_parameters": true },
        "max_tokens": max_tokens,
        "reasoning": { "effort": "low" },
        "usage": { "include": true },
        "messages": [
            { "role": "system", "content": req.system },
            { "role": "user", "content": user.to_string() },
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": "npc_line",
                "strict": true,
                "schema": {
                    "type": "object",
                    "properties": {
                        "text": { "type": "string" },
                        "cite": { "type": "string", "enum": req.allowed_cites },
                    },
                    "required": ["text", "cite"],
                    "additionalProperties": false,
                },
            },
        },
    })
}

/// Reads the line out of a chat completions response.
pub fn parse_line(v: &Value) -> Result<LineDraft, MindError> {
    let bad = |m: &str| MindError::Malformed(m.to_owned());
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| bad("no message content"))?;
    let line: Value =
        serde_json::from_str(content).map_err(|_| bad("message content is not JSON"))?;
    let text = line
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("no text"))?;
    let cites = match line.get("cite").or_else(|| line.get("cites")) {
        Some(Value::String(s)) => vec![s.clone()],
        Some(Value::Array(a)) => a
            .iter()
            .map(|c| c.as_str().map(str::to_owned).ok_or_else(|| bad("cite")))
            .collect::<Result<_, _>>()?,
        _ => return Err(bad("no cite")),
    };
    Ok(LineDraft {
        text: text.to_owned(),
        cites,
    })
}

fn model_of(v: &Value, fallback: &str) -> String {
    v.get("model")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
        .to_owned()
}

impl<T: Transport> Mind for OpenRouterMind<T> {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Result<Reply<Decision>, MindError> {
        let body = decision_body(&self.models.decision, req);
        let (v, attempts) = self.post("/api/alpha/decisions", &body)?;
        Ok(Reply {
            value: parse_decision(req, &v)?,
            cost_usd: cost(&v),
            model: model_of(&v, &self.models.decision),
            attempts,
        })
    }

    fn line(&mut self, req: &LineRequest) -> Result<Reply<LineDraft>, MindError> {
        let body = line_body(&self.models.line, self.line_max_tokens, req);
        let (v, attempts) = self.post("/api/v1/chat/completions", &body)?;
        Ok(Reply {
            value: parse_line(&v)?,
            cost_usd: cost(&v),
            model: model_of(&v, &self.models.line),
            attempts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_redacted() {
        let k = ApiKey::new("sk-or-secret");
        assert!(!format!("{k:?}").contains("secret"));
    }

    #[test]
    fn decision_body_matches_the_api() {
        let req = DecisionRequest {
            key: "sk.1".into(),
            instructions: "pick".into(),
            state: json!({"a": 1}),
            question: Question::choice(&[
                ("plan.1".into(), "sell ice".into()),
                ("plan.2".into(), "wait".into()),
            ]),
        };
        let b = decision_body("typesafe/jev-1.14", &req);
        assert_eq!(b["questions"]["pick"]["type"], "choice");
        assert_eq!(b["questions"]["pick"]["criteria"]["o2"], "wait");
        assert_eq!(b["state"]["a"], 1);
    }

    #[test]
    fn parses_the_three_answer_types() {
        // Shapes as returned by the live API.
        let v = json!({"answers":{"pick":{"type":"choice","choice":"o1","confidence":0.83}},"usage":{"cost":0.0000185}});
        let choice = DecisionRequest {
            key: String::new(),
            instructions: String::new(),
            state: Value::Null,
            question: Question::choice(&[("a".into(), "a".into())]),
        };
        assert_eq!(
            parse_decision(&choice, &v).unwrap(),
            Decision::Choice {
                choice: "o1".into(),
                confidence: Some(0.83)
            }
        );
        assert!((cost(&v) - 0.0000185).abs() < 1e-12);

        let yes_no = DecisionRequest {
            question: Question::YesNo {
                yes: "y".into(),
                no: "n".into(),
            },
            ..choice.clone()
        };
        let v = json!({"answers":{"pick":{"type":"noul","noul":0.38}}});
        assert_eq!(
            parse_decision(&yes_no, &v).unwrap(),
            Decision::YesNo { p_yes: 0.38 }
        );

        let score = DecisionRequest {
            question: Question::Score {
                scale: vec!["low".into(), "high".into()],
            },
            ..choice
        };
        let v = json!({"answers":{"pick":{"type":"score","score":1.47}}});
        assert_eq!(
            parse_decision(&score, &v).unwrap(),
            Decision::Score { score: 1.47 }
        );
    }

    #[test]
    fn parses_a_line() {
        let v = json!({"choices":[{"message":{"content":"{\"text\":\"Hello.\",\"cite\":\"fact.4\"}"}}]});
        let l = parse_line(&v).unwrap();
        assert_eq!(l.text, "Hello.");
        assert_eq!(l.cites, vec!["fact.4".to_owned()]);
        assert!(parse_line(&json!({"choices":[]})).is_err());
    }
}
