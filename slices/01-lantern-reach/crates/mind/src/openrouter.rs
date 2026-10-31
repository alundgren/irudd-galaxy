//! [`OpenRouterMind`]: the real mind, over HTTP.
//!
//! - Typed choices: `POST /api/alpha/decisions` with TypeSafe Jev.
//! - Short lines: `POST /api/v1/chat/completions` with a strict JSON schema and
//!   `provider.require_parameters`, so only providers that honour the schema answer.
//! - Live flourishes: chat completions through the auto router (`openrouter/auto` with
//!   the `auto-router` plugin at a cost tier), a per-station `session_id` and
//!   `X-OpenRouter-Metadata: enabled`, so the answer names the model the router picked
//!   and, when it classified the prompt, the task type.
//!
//! Every request goes through [`Client::call`]: a purpose, a model, a path and a body
//! in; the raw answer, tokens, cost, actual model and task type out, and one row in the
//! call log ([`crate::calllog`]). The radio and music reuse it.
//!
//! A request makes at most `max_attempts` HTTP attempts (the host's budget decides). A
//! timeout, transport error or 5xx is retried at once. A 429 or 5xx with `Retry-After`
//! is retried after that wait if the wait ends before the request's deadline; a longer
//! wait, or a 429 without one, skips the moment, unless the call has a backoff (the live
//! layer): then it waits that long, doubling, within the deadline. A malformed answer
//! is not retried. A 2xx whose body reports no cost is priced from the generation stats
//! endpoint by its generation ID, on the worker thread.
//! Every attempt's `usage.cost` is summed and returned with the answer or the error, so
//! a billed response is never lost. The transport is a trait so tests run against
//! canned responses without a network.
//!
//! The key never leaves in diagnostics: `ApiKey`'s `Debug` is redacted, and a response
//! body or transport error that echoes the key, or a long run of it, in any JSON
//! spelling has it replaced before it is read, truncated or put in a [`MindError`]
//! ([`ApiKey::redact`]). The call log redacts it again from everything it writes.

use std::fmt;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::calllog::{self, CallId, CallRow, CallSink, Verdict};
use crate::config::{Limits, LiveRules, Models};
use crate::mind::{
    Decision, DecisionRequest, LineDraft, LineRequest, LiveDraft, LiveRequest, Mind, MindError,
    Purpose, Question, Reply, wire_index,
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

    /// `text` with the key, and any run of [`MIN_RUN`] or more of its characters in key
    /// order, replaced by `<redacted>`. A character counts however JSON spells it: as
    /// itself, or as a `\uXXXX` escape behind any number of backslashes (JSON inside a
    /// JSON string), so decoding the result, once or many times, can't bring the key
    /// back. Only whole characters and escapes are replaced, so JSON stays JSON; should
    /// it not (a key that spans JSON punctuation), the parsed value is redacted string
    /// by string and written out again, and if even that would show the key, the text
    /// is kept redacted but marked [`NOT_JSON`].
    pub fn redact(&self, text: &str) -> String {
        let key: Vec<u32> = self.0.chars().map(u32::from).collect();
        if key.is_empty() {
            return text.to_owned();
        }
        let min = MIN_RUN.min(key.len());
        let Some(out) = redact_runs(text, &key, min) else {
            return text.to_owned();
        };
        if serde_json::from_str::<serde::de::IgnoredAny>(&out).is_err()
            && let Ok(v) = serde_json::from_str::<Value>(text)
        {
            let walked = self.redact_value(v).to_string();
            return match redact_runs(&walked, &key, min) {
                None => walked,
                Some(_) => format!("{NOT_JSON}{out}"),
            };
        }
        out
    }

    fn redact_value(&self, v: Value) -> Value {
        match v {
            Value::String(s) => Value::String(self.redact(&s)),
            Value::Array(a) => Value::Array(a.into_iter().map(|v| self.redact_value(v)).collect()),
            Value::Object(o) => Value::Object(
                o.into_iter()
                    .map(|(k, v)| (self.redact(&k), self.redact_value(v)))
                    .collect(),
            ),
            v => v,
        }
    }

    /// The error with the key redacted from any text it carries.
    pub fn redact_error(&self, e: MindError) -> MindError {
        match e {
            MindError::Transport(m) => MindError::Transport(self.redact(&m)),
            MindError::Http(s, m) => MindError::Http(s, self.redact(&m)),
            MindError::Malformed(m) => MindError::Malformed(self.redact(&m)),
            e => e,
        }
    }
}

/// What a redacted key reads as.
pub const REDACTED: &str = "<redacted>";

/// Leads a redacted text that was JSON but no longer parses.
pub const NOT_JSON: &str = "<redacted, no longer JSON> ";

/// The shortest run of key characters that is redacted: enough to identify a key, more
/// than its public `sk-or-v1-` prefix. A key shorter than this is redacted whole.
pub const MIN_RUN: usize = 12;

/// One character of a text as JSON may spell it: the bytes `start..end`, and the
/// character they decode to (`None` for an escape that is no character, like `\n`).
struct Spelled {
    start: usize,
    end: usize,
    unit: Option<u32>,
}

/// `text` split into characters and escapes. `\uXXXX` counts behind any number of
/// backslashes; a run of escaped backslashes stays one piece, so nothing is cut inside
/// an escape.
fn spell(text: &str) -> Vec<Spelled> {
    let b = text.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let c = text[i..].chars().next().expect("i is a char boundary");
        if c != '\\' {
            i += c.len_utf8();
            out.push(Spelled {
                start,
                end: i,
                unit: Some(u32::from(c)),
            });
            continue;
        }
        let mut j = i;
        while b.get(j) == Some(&b'\\') {
            j += 1;
        }
        let hex = b.get(j + 1..j + 5);
        let (end, unit) = if b.get(j) == Some(&b'u')
            && let Some(hex) = hex.filter(|h| h.iter().all(u8::is_ascii_hexdigit))
        {
            let hex = std::str::from_utf8(hex).expect("ASCII hex");
            (j + 5, u32::from_str_radix(hex, 16).ok())
        } else if (j - i) % 2 == 1
            && let Some(e) = text[j..].chars().next()
        {
            let unit = (!matches!(e, 'b' | 'f' | 'n' | 'r' | 't')).then_some(u32::from(e));
            (j + e.len_utf8(), unit)
        } else {
            (j, None)
        };
        out.push(Spelled { start, end, unit });
        i = end;
    }
    out
}

/// `text` with every run of at least `min` spelled characters that appears in `key`
/// replaced by [`REDACTED`], or `None` if there is none.
fn redact_runs(text: &str, key: &[u32], min: usize) -> Option<String> {
    let spelled = spell(text);
    // `run[j + 1]`: how many characters up to this one match the key up to `key[j]`.
    let mut run = vec![0usize; key.len() + 1];
    let mut cut = vec![false; spelled.len()];
    let mut next = 0;
    for (i, s) in spelled.iter().enumerate() {
        let mut best = 0;
        for j in (0..key.len()).rev() {
            run[j + 1] = if s.unit == Some(key[j]) {
                run[j] + 1
            } else {
                0
            };
            best = best.max(run[j + 1]);
        }
        if best >= min {
            for c in &mut cut[(i + 1 - best).max(next)..=i] {
                *c = true;
            }
            next = i + 1;
        }
    }
    if next == 0 {
        return None;
    }
    let mut out = String::with_capacity(text.len());
    for (i, s) in spelled.iter().enumerate() {
        if !cut[i] {
            out.push_str(&text[s.start..s.end]);
        } else if i == 0 || !cut[i - 1] {
            out.push_str(REDACTED);
        }
    }
    Some(out)
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

/// Sends JSON requests. Errors are only for calls that got no HTTP answer at all.
pub trait Transport: Send {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError>;
    /// A POST with extra headers (`X-OpenRouter-Metadata`). A transport that can't send
    /// headers sends the plain POST.
    fn post_with(
        &mut self,
        url: &str,
        auth: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpReply, MindError> {
        let _ = headers;
        self.post(url, auth, body)
    }
    /// A GET (the generation stats endpoint). Unsupported unless implemented.
    fn get(&mut self, _url: &str, _auth: &str) -> Result<HttpReply, MindError> {
        Err(MindError::Transport("GET is not supported".into()))
    }
    /// Waits out a `Retry-After` or a backoff. Runs on the worker thread, never the
    /// frame thread.
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

fn ureq_error(e: ureq::Error) -> MindError {
    match e {
        ureq::Error::Timeout(_) => MindError::Timeout,
        e => MindError::Transport(e.to_string()),
    }
}

fn read_reply(resp: ureq::http::Response<ureq::Body>) -> Result<HttpReply, MindError> {
    let status = resp.status().as_u16();
    let retry_after_ms = resp
        .headers()
        .get("retry-after")
        .map(|v| v.to_str().map_or(u64::MAX, parse_retry_after));
    let body = resp.into_body().read_to_string().map_err(ureq_error)?;
    Ok(HttpReply {
        status,
        body,
        retry_after_ms,
    })
}

impl Transport for UreqTransport {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError> {
        self.post_with(url, auth, &[], body)
    }

    fn post_with(
        &mut self,
        url: &str,
        auth: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpReply, MindError> {
        let mut req = self
            .agent
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .header("X-Title", "Irudd Galaxy");
        for (name, value) in headers {
            req = req.header(*name, *value);
        }
        read_reply(req.send(body).map_err(ureq_error)?)
    }

    fn get(&mut self, url: &str, auth: &str) -> Result<HttpReply, MindError> {
        let resp = self
            .agent
            .get(url)
            .header("Authorization", auth)
            .header("X-Title", "Irudd Galaxy")
            .call()
            .map_err(ureq_error)?;
        read_reply(resp)
    }
}

/// The chat completions path.
pub const CHAT_PATH: &str = "/api/v1/chat/completions";
/// The Decisions API path.
pub const DECISIONS_PATH: &str = "/api/alpha/decisions";

/// One OpenRouter call for a purpose: a model, a path and a request body. The live
/// layer, the decision and line calls, and later the radio and music, all go through
/// [`Client::call`], so every one is logged the same way.
#[derive(Clone, Debug)]
pub struct Call<'a> {
    pub purpose: Purpose,
    /// The model asked for (also in `body`), for the log.
    pub model: &'a str,
    pub path: &'a str,
    pub body: &'a Value,
    /// The `session_id` in `body`, if any, for the log.
    pub session_id: Option<&'a str>,
    pub max_attempts: u8,
    pub deadline_ms: u64,
    /// Before retrying a 429 or 5xx that named no wait: this long, doubling each time.
    /// 0 keeps the plain rule: a 5xx is retried at once and a bare 429 is not.
    pub backoff_ms: u64,
    /// Sends `X-OpenRouter-Metadata: enabled`, for the router's task type.
    pub metadata: bool,
}

/// Tokens and cost as OpenRouter reported them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Usage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    /// US dollars, summed over attempts; `None` if no response reported any.
    pub cost_usd: Option<f64>,
    /// `usage` (in the response) or `generation` (looked up by generation ID).
    pub cost_source: Option<&'static str>,
}

/// What a [`Call`] came to.
#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
    /// The 2xx body, or why there was none.
    pub result: Result<Value, MindError>,
    pub attempts: u8,
    pub usage: Usage,
    /// The model that answered, from the response (empty if nothing did).
    pub model: String,
    /// The auto router's task type, when the metadata named one.
    pub task_type: Option<String>,
    pub generation_id: Option<String>,
    pub latency_ms: u64,
    /// The log row, if the client has a log.
    pub call: Option<CallId>,
}

impl Answer {
    /// Cost for the budget: what OpenRouter reported, 0 if nothing.
    pub fn cost_usd(&self) -> f64 {
        self.usage.cost_usd.unwrap_or(0.0)
    }

    /// The first choice's message content, if the body has one.
    pub fn content(&self) -> Option<&str> {
        self.result
            .as_ref()
            .ok()?
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
    }
}

/// The OpenRouter HTTP client: the key, the transport and the call log. Every request
/// goes through [`Client::call`], which retries, reads the metadata, prices the call
/// and writes one log row.
pub struct Client<T: Transport = UreqTransport> {
    transport: T,
    key: ApiKey,
    base_url: String,
    log: CallSink,
}

impl<T: Transport> Client<T> {
    pub fn new(transport: T, key: ApiKey) -> Self {
        Self {
            transport,
            key,
            base_url: "https://openrouter.ai".into(),
            log: CallSink::new(),
        }
    }

    /// Logs every call to `log`, which redacts the key from all it writes.
    pub fn with_log(mut self, log: CallSink) -> Self {
        log.redact_with(&self.key);
        self.log = log;
        self
    }

    pub fn log(&self) -> &CallSink {
        &self.log
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    /// Records the game's verdict on a call's answer.
    pub fn verdict(&self, answer: &Answer, verdict: Verdict, reason: Option<String>) {
        if let Some(id) = &answer.call {
            self.log.verdict(id, verdict, reason);
        }
    }

    /// One call in at most `max_attempts` attempts. A timeout, transport error or 5xx
    /// is retried; a 429 or 5xx with `Retry-After` after that wait, if it ends before
    /// the deadline (a longer wait gives up); without one, after the backoff, if the
    /// call has one. A malformed answer is not retried. Every attempt's `usage.cost`
    /// counts; a 2xx without one is priced from the generation stats endpoint while
    /// the deadline allows. Then the call is logged: its verdict is `pending` for an
    /// answer (the caller settles it) and `failed` for none.
    pub fn call(&mut self, call: &Call<'_>) -> Answer {
        let url = format!("{}{}", self.base_url, call.path);
        let body = call.body.to_string();
        let auth = self.key.bearer();
        let start = Instant::now();
        let ts_ms = calllog::now_ms();
        let headers: &[(&str, &str)] = if call.metadata {
            &[("X-OpenRouter-Metadata", "enabled")]
        } else {
            &[]
        };
        let mut attempts = 0u8;
        let mut result = Err(MindError::NotRecorded);
        let mut cost_usd = None::<f64>;
        let mut status = None::<u16>;
        let mut raw = String::new();
        while attempts < call.max_attempts {
            attempts += 1;
            let mut retry_after = None;
            let key = &self.key;
            let sent = self.transport.post_with(&url, &auth, headers, &body);
            result = sent.map_err(|e| key.redact_error(e)).and_then(|mut r| {
                retry_after = r.retry_after_ms;
                status = Some(r.status);
                // An upstream error may echo the key: drop it before the body is read,
                // truncated or logged, so not even part of it reaches an error.
                r.body = key.redact(&r.body);
                // A billed response carries its cost whatever its status or shape.
                let parsed = serde_json::from_str::<Value>(&r.body);
                if let Ok(v) = &parsed
                    && let Some(c) = reported_cost(v)
                {
                    *cost_usd.get_or_insert(0.0) += c;
                }
                raw = std::mem::take(&mut r.body);
                match r.status {
                    200..=299 => {
                        let v =
                            parsed.map_err(|e| MindError::Malformed(format!("not JSON: {e}")))?;
                        // OpenRouter can report an upstream failure inside a 200.
                        // Upstream error text is never copied into an error: it could
                        // echo the key in a form redaction misses (a JSON escape, say).
                        // Only the numeric code is kept.
                        match v.get("error") {
                            Some(err) => Err(match err.get("code").and_then(Value::as_u64) {
                                Some(429) => MindError::RateLimited,
                                Some(c @ 500..=599) => MindError::Server(c as u16),
                                Some(c) => MindError::Malformed(format!("upstream error {c}")),
                                None => MindError::Malformed("upstream error".into()),
                            }),
                            None => Ok(v),
                        }
                    }
                    429 => Err(MindError::RateLimited),
                    s @ 500..=599 => Err(MindError::Server(s)),
                    s => Err(MindError::Http(s, "upstream error".into())),
                }
            });
            let Err(e) = &result else {
                break;
            };
            if !e.retryable() || attempts >= call.max_attempts {
                break;
            }
            let used = start.elapsed().as_millis() as u64;
            let wait = match (e, retry_after) {
                // The server named a wait: honour it if it ends before the deadline,
                // else skip the moment rather than retry inside the interval.
                (MindError::RateLimited | MindError::Server(_), Some(wait)) => Some(wait),
                // No word on when to come back: back off, doubling, if the call does.
                (MindError::RateLimited | MindError::Server(_), None) if call.backoff_ms > 0 => {
                    Some(call.backoff_ms.saturating_mul(1 << (attempts - 1).min(16)))
                }
                // Rate limited and no backoff: don't hammer.
                (MindError::RateLimited, None) => break,
                _ => None,
            };
            if let Some(wait) = wait {
                if used.saturating_add(wait) >= call.deadline_ms {
                    break;
                }
                self.transport.sleep(Duration::from_millis(wait));
            }
        }
        let ok = result.as_ref().ok();
        let generation_id = ok
            .and_then(|v| v.get("id"))
            .and_then(Value::as_str)
            .filter(|id| is_generation_id(id))
            .map(str::to_owned);
        let mut usage = Usage {
            prompt_tokens: ok.and_then(|v| v.pointer("/usage/prompt_tokens")?.as_u64()),
            completion_tokens: ok.and_then(|v| v.pointer("/usage/completion_tokens")?.as_u64()),
            cost_usd,
            cost_source: cost_usd.map(|_| "usage"),
        };
        if usage.cost_usd.is_none()
            && let Some(id) = &generation_id
            && let Some(c) = self.generation_cost(id, start, call.deadline_ms)
        {
            usage.cost_usd = Some(c);
            usage.cost_source = Some("generation");
        }
        let answer = Answer {
            model: ok.map_or_else(String::new, |v| model_of(v, call.model)),
            task_type: ok.and_then(task_type),
            generation_id,
            usage,
            attempts,
            latency_ms: start.elapsed().as_millis() as u64,
            call: self.log.is_connected().then(CallId::fresh),
            result,
        };
        if let Some(uid) = &answer.call {
            let error = answer.result.as_ref().err();
            let output = answer
                .content()
                .map(str::to_owned)
                .unwrap_or_else(|| match error {
                    Some(e) if raw.is_empty() => e.to_string(),
                    _ => raw,
                });
            self.log.record(CallRow {
                uid: uid.clone(),
                ts_ms,
                purpose: call.purpose.as_str().to_owned(),
                requested_model: call.model.to_owned(),
                actual_model: (!answer.model.is_empty()).then(|| answer.model.clone()),
                task_type: answer.task_type.clone(),
                session_id: call.session_id.map(str::to_owned),
                generation_id: answer.generation_id.clone(),
                prompt_tokens: answer.usage.prompt_tokens,
                completion_tokens: answer.usage.completion_tokens,
                cost_usd: answer.usage.cost_usd,
                cost_source: answer.usage.cost_source.map(str::to_owned),
                latency_ms: answer.latency_ms,
                attempts,
                http_status: status,
                error_kind: error.map(|e| e.tag().to_owned()),
                verdict: if error.is_some() {
                    Verdict::Failed
                } else {
                    Verdict::Pending
                },
                reason: None,
                // The sink redacts the key from these, as from every text it writes.
                prompt: body,
                output,
            });
        }
        answer
    }

    /// The cost of a generation from the stats endpoint, tried twice (it can lag the
    /// answer a little) while the deadline allows.
    fn generation_cost(&mut self, id: &str, start: Instant, deadline_ms: u64) -> Option<f64> {
        let url = format!("{}/api/v1/generation?id={id}", self.base_url);
        let auth = self.key.bearer();
        for (i, wait) in [0u64, 500].into_iter().enumerate() {
            let used = start.elapsed().as_millis() as u64;
            if used.saturating_add(wait) >= deadline_ms {
                return None;
            }
            if i > 0 {
                self.transport.sleep(Duration::from_millis(wait));
            }
            let Ok(r) = self.transport.get(&url, &auth) else {
                continue;
            };
            if !(200..=299).contains(&r.status) {
                continue;
            }
            let cost = serde_json::from_str::<Value>(&r.body)
                .ok()
                .and_then(|v| v.pointer("/data/total_cost")?.as_f64())
                .filter(|c| c.is_finite() && *c >= 0.0);
            if cost.is_some() {
                return cost;
            }
        }
        None
    }
}

fn is_generation_id(id: &str) -> bool {
    id.starts_with("gen-")
        && id.len() <= 128
        && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The router stage's task type: `data.task_type` on any stage of
/// `openrouter_metadata.pipeline` (the docs name no stage type, so any stage counts).
/// Kept only if it looks like a tag (`code:debugging`).
pub fn task_type(v: &Value) -> Option<String> {
    v.pointer("/openrouter_metadata/pipeline")?
        .as_array()?
        .iter()
        .filter_map(|stage| stage.pointer("/data/task_type")?.as_str())
        .find(|t| {
            (1..=64).contains(&t.len())
                && !t.contains("sk-")
                && t.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b" :_-./".contains(&b))
        })
        .map(str::to_owned)
}

pub struct OpenRouterMind<T: Transport = UreqTransport> {
    client: Client<T>,
    models: Models,
    line_max_tokens: u32,
    live: LiveRules,
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
            client: Client::new(transport, key),
            models,
            line_max_tokens: limits.line_max_tokens,
            live: LiveRules::default(),
        }
    }

    /// Logs every call to `log`.
    pub fn with_log(mut self, log: CallSink) -> Self {
        self.client = self.client.with_log(log);
        self
    }

    /// The live layer's model, router tier and limits.
    pub fn with_live(mut self, live: LiveRules) -> Self {
        self.live = live;
        self
    }

    pub fn transport(&self) -> &T {
        self.client.transport()
    }

    pub fn client(&mut self) -> &mut Client<T> {
        &mut self.client
    }
}

/// The answer as a [`Reply`]: parsed, with the cost and the actual model. A body that
/// doesn't parse is a rejected call in the log.
fn reply<T, U: Transport>(
    client: &Client<U>,
    answer: Answer,
    parse: impl FnOnce(&Value) -> Result<T, MindError>,
) -> Reply<T> {
    let result = match &answer.result {
        Ok(v) => parse(v),
        Err(e) => Err(e.clone()),
    };
    if let (Ok(_), Err(e)) = (&answer.result, &result) {
        client.verdict(&answer, Verdict::Rejected, Some(e.to_string()));
    }
    Reply {
        result,
        cost_usd: answer.cost_usd(),
        model: answer.model,
        attempts: answer.attempts,
        hold_ticks: 0,
        call: answer.call,
    }
}

/// `usage.cost`, if the body reports it.
fn reported_cost(v: &Value) -> Option<f64> {
    v.pointer("/usage/cost")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0)
}

#[cfg(test)]
fn cost(v: &Value) -> f64 {
    reported_cost(v).unwrap_or(0.0)
}

/// The chat completions body for a live flourish: the auto router at its cost tier,
/// the station's session, a JSON schema for the answer (honoured where the chosen
/// model takes it; the answer is parsed defensively either way).
pub fn live_body(rules: &LiveRules, req: &LiveRequest) -> Value {
    json!({
        "model": rules.model,
        "plugins": [{ "id": "auto-router", "cost_tier": rules.cost_tier }],
        "session_id": req.session_id,
        "max_tokens": rules.max_tokens,
        // The router may pick a reasoning model; a flourish needs little thought.
        "reasoning": { "effort": rules.reasoning_effort },
        "messages": [
            { "role": "system", "content": req.system },
            { "role": "user", "content": req.user.to_string() },
        ],
        "response_format": {
            "type": "json_schema",
            "json_schema": {
                "name": format!("live_{}", req.purpose.as_str()),
                "strict": true,
                "schema": req.schema,
            },
        },
    })
}

/// Reads a live answer: the message content as one JSON object. Tolerates a code
/// fence or a sentence around it; anything else is malformed.
pub fn parse_live(v: &Value) -> Result<LiveDraft, MindError> {
    let bad = |m: &str| MindError::Malformed(m.to_owned());
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|c| !c.trim().is_empty())
        .ok_or_else(|| bad("no message content"))?;
    let (Some(open), Some(close)) = (content.find('{'), content.rfind('}')) else {
        return Err(bad("message content is not a JSON object"));
    };
    if close < open {
        return Err(bad("message content is not a JSON object"));
    }
    let value: Value = serde_json::from_str(&content[open..=close])
        .map_err(|_| bad("message content is not a JSON object"))?;
    if !value.is_object() {
        return Err(bad("message content is not a JSON object"));
    }
    Ok(LiveDraft { value })
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
            // Keep only a well-formed option ID: anything else (which could echo
            // text we never want stored) is malformed, and never quoted.
            if wire_index(choice).is_none() || choice.len() > 8 {
                return Err(bad("choice is not an option ID"));
            }
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

/// The model the response names, if it looks like a model ID (`typesafe/jev-1.14-20261014`),
/// else the one we asked for. Upstream text is untrusted: a field that could echo the
/// key, in any encoding, never reaches a spend note, `Debug` or a probe.
fn model_of(v: &Value, fallback: &str) -> String {
    v.get("model")
        .and_then(Value::as_str)
        .filter(|m| is_model_id(m))
        .unwrap_or(fallback)
        .to_owned()
}

fn is_model_id(m: &str) -> bool {
    (1..=96).contains(&m.len())
        && !m.contains("sk-")
        && m.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._:/~-".contains(&b))
}

#[cfg(test)]
mod model_id_tests {
    use super::*;

    #[test]
    fn only_model_like_ids_are_kept() {
        let fallback = "typesafe/jev-1.14";
        let named = |m: &str| model_of(&json!({ "model": m }), fallback);
        assert_eq!(
            named("typesafe/jev-1.14-20261014"),
            "typesafe/jev-1.14-20261014"
        );
        assert_eq!(named("openai/gpt-oss-20b"), "openai/gpt-oss-20b");
        // A key, plain or with any character a model ID can't hold, falls back.
        assert_eq!(named("sk-or-v1-0123456789abcdef"), fallback);
        assert_eq!(named("Bearer sk-or-v1-0123"), fallback);
        assert_eq!(named("echo-sk-or-v1-0123"), fallback);
        assert_eq!(named("sk-or-v1-0123"), fallback);
        assert_eq!(named(&"a".repeat(200)), fallback);
    }
}

impl<T: Transport> Mind for OpenRouterMind<T> {
    fn name(&self) -> &str {
        "openrouter"
    }

    fn decide(&mut self, req: &DecisionRequest) -> Reply<Decision> {
        let body = decision_body(&self.models.decision, req);
        let answer = self.client.call(&Call {
            purpose: Purpose::Decision,
            model: &self.models.decision,
            path: DECISIONS_PATH,
            body: &body,
            session_id: None,
            max_attempts: req.max_attempts,
            deadline_ms: req.deadline_ms,
            backoff_ms: 0,
            metadata: false,
        });
        reply(&self.client, answer, |v| parse_decision(req, v))
    }

    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        let body = line_body(&self.models.line, self.line_max_tokens, req);
        let answer = self.client.call(&Call {
            purpose: Purpose::Line,
            model: &self.models.line,
            path: CHAT_PATH,
            body: &body,
            session_id: None,
            max_attempts: req.max_attempts,
            deadline_ms: req.deadline_ms,
            backoff_ms: 0,
            metadata: false,
        });
        // Model-written text is kept for exact replay (opt-in free phrasing only).
        // Redact the key from the *decoded* text and cites, so an echo in any JSON
        // encoding is caught before it is stored, shown or quoted anywhere.
        let key = self.client.key.clone();
        reply(&self.client, answer, |v| {
            parse_line(v).map(|d| LineDraft {
                text: key.redact(&d.text),
                cites: d.cites.iter().map(|c| key.redact(c)).collect(),
            })
        })
    }

    fn live(&mut self, req: &LiveRequest) -> Reply<LiveDraft> {
        let body = live_body(&self.live, req);
        let answer = self.client.call(&Call {
            purpose: req.purpose,
            model: &self.live.model,
            path: CHAT_PATH,
            body: &body,
            session_id: Some(&req.session_id),
            max_attempts: req.max_attempts,
            deadline_ms: req.deadline_ms,
            backoff_ms: self.live.backoff_ms,
            metadata: true,
        });
        // As for lines: the key is redacted from the decoded answer before anything
        // stores or validates it.
        let key = self.client.key.clone();
        reply(&self.client, answer, |v| {
            let draft = parse_live(v)?;
            let text = key.redact(&draft.value.to_string());
            Ok(LiveDraft {
                value: serde_json::from_str(&text)
                    .map_err(|_| MindError::Malformed("redacted answer".into()))?,
            })
        })
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

    /// Every text `text` decodes to, decoding again wherever a string is JSON.
    fn decoded(text: &str, out: &mut Vec<String>) {
        out.push(text.to_owned());
        fn walk(v: &Value, out: &mut Vec<String>) {
            match v {
                Value::String(s) => decoded(s, out),
                Value::Array(a) => a.iter().for_each(|v| walk(v, out)),
                Value::Object(o) => o.iter().for_each(|(k, v)| {
                    decoded(k, out);
                    walk(v, out);
                }),
                _ => {}
            }
        }
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            walk(&v, out);
        }
    }

    #[test]
    fn the_key_is_redacted_however_json_spells_it() {
        const KEY: &str = "sk-or-v1-0123456789abcdef";
        let k = ApiKey::new(KEY);
        let esc = |c: char| format!(r"\u{:04x}", u32::from(c));
        let first = format!("{}{}", esc('s'), &KEY[1..]);
        let every: String = KEY.chars().map(esc).collect();
        let upper = format!(r"\u{:04X}{}", u32::from('s'), &KEY[1..]);
        let inner = format!(r#"{{"text":"hi {first}","n":1}}"#);
        let once = json!({ "content": inner }).to_string();
        let twice = json!({ "message": once }).to_string();
        let cases = [
            format!(r#"{{"error":{{"message":"bad key {KEY}","code":401}}}}"#),
            format!(r#"{{"error":{{"message":"bad key {first}","code":401}}}}"#),
            format!(r#"{{"m":"{every}"}}"#),
            format!(r#"{{"m":"{upper}"}}"#),
            format!(r#"{{"m":"line\n{KEY}\t"}}"#),
            once,
            twice,
            format!("transport error: {first} (plain text)"),
            format!("cut at the end: {}", &KEY[..14]),
        ];
        for text in cases {
            let out = k.redact(&text);
            assert!(out.contains(REDACTED), "{out}");
            if serde_json::from_str::<Value>(&text).is_ok() {
                serde_json::from_str::<Value>(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
            }
            let mut all = Vec::new();
            decoded(&out, &mut all);
            for t in all {
                for run in KEY.as_bytes().windows(MIN_RUN) {
                    let run = std::str::from_utf8(run).unwrap();
                    assert!(!t.contains(run), "{run} in {t} from {text}");
                }
            }
        }
    }

    #[test]
    fn redaction_keeps_what_is_not_the_key() {
        let k = ApiKey::new("sk-or-v1-0123456789abcdef");
        // The public prefix and short runs identify nothing; the text around a key and
        // its escapes stays as it was.
        for text in [
            "keys look like sk-or-v1-...",
            "0123456789a",
            r#"{"a":"b\\n"}"#,
        ] {
            assert_eq!(k.redact(text), text);
        }
        assert_eq!(
            k.redact(r#"{"m":"bad sk-or-v1-0123456789abcdef, sorry"}"#),
            r#"{"m":"bad <redacted>, sorry"}"#
        );
        assert_eq!(k.redact("x 456789abcdef y"), "x <redacted> y");
    }

    #[test]
    fn a_key_across_json_punctuation_still_leaves_json_or_a_marked_text() {
        // A key that spans JSON punctuation: the parsed strings are redacted instead.
        let k = ApiKey::new(r#"a","b"#);
        let out = k.redact(r#"{"x":"a","b":"c"}"#);
        assert_eq!(out, r#"{"b":"c","x":"a"}"#);
        // Written out again they show it, so the text is kept redacted and marked.
        let k = ApiKey::new(r#"a","#);
        let out = k.redact(r#"["a","b"]"#);
        assert_eq!(out, format!(r#"{NOT_JSON}["{REDACTED}"b"]"#));
        // A key that is a whole string's worth inside escapes stays JSON.
        let k = ApiKey::new(r#"quote"in-the-key"#);
        let text = json!({ "m": r#"quote"in-the-key"# }).to_string();
        assert_eq!(k.redact(&text), format!(r#"{{"m":"{REDACTED}"}}"#));
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

#[cfg(test)]
mod choice_id_tests {
    use super::*;

    #[test]
    fn a_choice_that_is_not_an_option_id_is_malformed_and_not_quoted() {
        let req = crate::probe::sample_decision();
        for bad in ["sk-or-v1-0123", "o1 sk-or", "o99999999999", "x"] {
            let v = json!({ "answers": { "pick": { "type": "choice", "choice": bad } } });
            let err = parse_decision(&req, &v).unwrap_err();
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains(bad), "{shown}");
        }
    }
}
