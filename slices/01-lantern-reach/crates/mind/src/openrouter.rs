//! [`OpenRouterMind`]: the real mind, over HTTP.
//!
//! - Typed choices: `POST /api/alpha/decisions` with TypeSafe Jev.
//! - Short lines: `POST /api/v1/chat/completions` with a strict JSON schema and
//!   `provider.require_parameters`, so only providers that honour the schema answer.
//!
//! A request makes at most `max_attempts` HTTP attempts (the host's budget decides). A
//! timeout, transport error or 5xx is retried at once. A 429 or 5xx with `Retry-After`
//! is retried after that wait if the wait ends before the request's deadline; a longer
//! wait, or a 429 without one, skips the moment. A malformed answer is not retried.
//! Every attempt's `usage.cost` is summed and returned with the answer or the error, so
//! a billed response is never lost. The transport is a trait so tests run against
//! canned responses without a network.

use std::fmt;
use std::time::{Duration, Instant};

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
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
    /// The `Retry-After` header in milliseconds, if the server sent one. A date, or
    /// anything else that isn't a number of seconds, reads as `u64::MAX`: too long.
    pub retry_after_ms: Option<u64>,
}

impl HttpReply {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
            retry_after_ms: None,
        }
    }
}

/// `Retry-After` in milliseconds: delta-seconds, or `u64::MAX` for an HTTP date.
pub fn parse_retry_after(value: &str) -> u64 {
    value
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|s| s.is_finite() && *s >= 0.0)
        .map_or(u64::MAX, |s| (s * 1000.0).min(u64::MAX as f64) as u64)
}

/// Sends one JSON POST. Errors are only for calls that got no HTTP answer at all.
pub trait Transport: Send {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError>;
    /// Waits out a `Retry-After`. Runs on the worker thread, never the frame thread.
    fn sleep(&mut self, wait: Duration) {
        std::thread::sleep(wait);
    }
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
        let retry_after_ms = resp
            .headers()
            .get("retry-after")
            .map(|v| v.to_str().map_or(u64::MAX, parse_retry_after));
        let body = resp.into_body().read_to_string().map_err(map)?;
        Ok(HttpReply {
            status,
            body,
            retry_after_ms,
        })
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

/// What a POST came to, before its body is read.
struct Posted {
    result: Result<Value, MindError>,
    attempts: u8,
    cost_usd: f64,
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

    /// One POST in at most `max_attempts` attempts. Returns the 2xx body or the last
    /// error, the attempts made and the cost every attempt reported.
    fn post(&mut self, path: &str, body: &Value, max_attempts: u8, deadline_ms: u64) -> Posted {
        let url = format!("{}{path}", self.base_url);
        let body = body.to_string();
        let auth = self.key.bearer();
        let start = Instant::now();
        let mut out = Posted {
            result: Err(MindError::NotRecorded),
            attempts: 0,
            cost_usd: 0.0,
        };
        while out.attempts < max_attempts {
            out.attempts += 1;
            let mut retry_after = None;
            out.result = self.transport.post(&url, &auth, &body).and_then(|r| {
                retry_after = r.retry_after_ms;
                // A billed response carries its cost whatever its status or shape.
                let parsed = serde_json::from_str::<Value>(&r.body);
                if let Ok(v) = &parsed {
                    out.cost_usd += cost(v);
                }
                match r.status {
                    200..=299 => {
                        let v =
                            parsed.map_err(|e| MindError::Malformed(format!("not JSON: {e}")))?;
                        // OpenRouter can report an upstream failure inside a 200.
                        match v.get("error") {
                            Some(err) => Err(match err.get("code").and_then(Value::as_u64) {
                                Some(429) => MindError::RateLimited,
                                Some(c @ 500..=599) => MindError::Server(c as u16),
                                _ => MindError::Malformed(short(&err.to_string())),
                            }),
                            None => Ok(v),
                        }
                    }
                    429 => Err(MindError::RateLimited),
                    s @ 500..=599 => Err(MindError::Server(s)),
                    s => Err(MindError::Http(s, short(&r.body))),
                }
            });
            let Err(e) = &out.result else {
                break;
            };
            if !e.retryable() || out.attempts >= max_attempts {
                break;
            }
            match (e, retry_after) {
                // The server named a wait: honour it if it ends before the deadline,
                // else skip the moment rather than retry inside the interval.
                (MindError::RateLimited | MindError::Server(_), Some(wait)) => {
                    let used = start.elapsed().as_millis() as u64;
                    if used.saturating_add(wait) >= deadline_ms {
                        break;
                    }
                    self.transport.sleep(Duration::from_millis(wait));
                }
                // Rate limited with no word on when to come back: don't hammer.
                (MindError::RateLimited, None) => break,
                _ => {}
            }
        }
        out
    }
}

fn reply<T>(
    posted: Posted,
    model: &str,
    parse: impl FnOnce(&Value) -> Result<T, MindError>,
) -> Reply<T> {
    let (result, model) = match posted.result {
        Ok(v) => (parse(&v), model_of(&v, model)),
        Err(e) => (Err(e), String::new()),
    };
    Reply {
        result,
        cost_usd: posted.cost_usd,
        model,
        attempts: posted.attempts,
        hold_ticks: 0,
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
/// to the packet's fact IDs. The model sees the line with its slots left in.
pub fn line_body(model: &str, max_tokens: u32, req: &LineRequest) -> Value {
    let mut user = json!({
        "packet": req.packet,
        "line": req.template,
        "cite": req.cite,
    });
    if !req.slots.is_empty() {
        user["slots"] = json!(req.slots);
    }
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

    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision> {
        let body = decision_body(&self.models.decision, req);
        let posted = self.post(
            "/api/alpha/decisions",
            &body,
            req.max_attempts,
            req.deadline_ms,
        );
        reply(posted, &self.models.decision, |v| parse_decision(req, v))
    }

    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        let body = line_body(&self.models.line, self.line_max_tokens, req);
        let posted = self.post(
            "/api/v1/chat/completions",
            &body,
            req.max_attempts,
            req.deadline_ms,
        );
        reply(posted, &self.models.line, parse_line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(options: &[(String, String)]) -> DecisionRequest {
        DecisionRequest {
            key: "sk.1".into(),
            instructions: "pick".into(),
            state: json!({"a": 1}),
            question: Question::choice(options),
            max_attempts: 2,
            deadline_ms: 10_000,
        }
    }

    #[test]
    fn key_is_redacted() {
        let k = ApiKey::new("sk-or-secret");
        assert!(!format!("{k:?}").contains("secret"));
    }

    #[test]
    fn decision_body_matches_the_api() {
        let req = request(&[
            ("plan.1".into(), "sell ice".into()),
            ("plan.2".into(), "wait".into()),
        ]);
        let b = decision_body("typesafe/jev-1.14", &req);
        assert_eq!(b["questions"]["pick"]["type"], "choice");
        assert_eq!(b["questions"]["pick"]["criteria"]["o2"], "wait");
        assert_eq!(b["state"]["a"], 1);
    }

    #[test]
    fn parses_the_three_answer_types() {
        // Shapes as returned by the live API.
        let v = json!({"answers":{"pick":{"type":"choice","choice":"o1","confidence":0.83}},"usage":{"cost":0.0000185}});
        let choice = request(&[("a".into(), "a".into())]);
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
    fn reads_retry_after() {
        assert_eq!(parse_retry_after("2"), 2000);
        assert_eq!(parse_retry_after(" 0.5 "), 500);
        assert_eq!(parse_retry_after("Wed, 21 Oct 2026 07:28:00 GMT"), u64::MAX);
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
