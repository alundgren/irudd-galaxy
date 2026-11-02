//! A small OpenRouter client for build-time generation, carried over from the slice's
//! music code: the key, one HTTPS POST, a streamed answer folded into one body, and the
//! audio read out of it. The game never links this; only `galaxy-gm` makes calls.
//!
//! The key comes from `OPENROUTER_API_KEY` and nowhere else. It is only ever put in the
//! `Authorization` header. `ApiKey`'s `Debug` is redacted, and every text that leaves
//! this module (an error, a body excerpt, the model's text) goes through
//! [`ApiKey::redact`] first, which removes the key or any run of [`MIN_RUN`] of its
//! characters however JSON spells them.

use std::fmt;
use std::time::Duration;

use serde_json::{Value, json};

/// The chat completions endpoint.
pub const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

/// The OpenRouter API key. Never printed: `Debug` is redacted and there is no `Display`.
#[derive(Clone)]
pub struct ApiKey(String);

impl ApiKey {
    pub const ENV: &'static str = "OPENROUTER_API_KEY";

    /// `OPENROUTER_API_KEY`, or `None` when it is unset or blank.
    pub fn from_env() -> Option<Self> {
        std::env::var(Self::ENV)
            .ok()
            .map(|k| k.trim().to_owned())
            .filter(|k| !k.is_empty())
            .map(Self)
    }

    /// For tests with a fake transport.
    #[cfg(test)]
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }

    fn bearer(&self) -> String {
        format!("Bearer {}", self.0)
    }

    /// `text` with the key, and any run of [`MIN_RUN`] or more of its characters in key
    /// order, replaced by [`REDACTED`]. A character counts however JSON spells it: as
    /// itself or as a `\uXXXX` escape behind any number of backslashes, so decoding the
    /// result can't bring the key back.
    pub fn redact(&self, text: &str) -> String {
        let key: Vec<u32> = self.0.chars().map(u32::from).collect();
        if key.is_empty() {
            return text.to_owned();
        }
        redact_runs(text, &key, MIN_RUN.min(key.len())).unwrap_or_else(|| text.to_owned())
    }
}

impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey(<redacted>)")
    }
}

/// What a redacted key reads as.
pub const REDACTED: &str = "<redacted>";

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
/// backslashes; a run of escaped backslashes stays one piece.
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

/// A raw HTTP answer.
#[derive(Clone, Debug, PartialEq)]
pub struct HttpReply {
    pub status: u16,
    pub body: String,
}

/// Sends one JSON POST. `Err` only for a call that got no HTTP answer at all; its text
/// may echo the request, so callers redact it.
pub trait Transport {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, String>;
}

/// Largest answer read: room for a full generated song as streamed base64.
const MAX_BODY_BYTES: u64 = 64 * 1024 * 1024;

/// The real transport: `ureq` with rustls.
pub struct Https {
    agent: ureq::Agent,
}

impl Https {
    pub fn new(timeout: Duration) -> Self {
        let agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(timeout))
            .build()
            .into();
        Self { agent }
    }
}

impl Transport for Https {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, String> {
        let resp = self
            .agent
            .post(url)
            .header("Authorization", auth)
            .header("Content-Type", "application/json")
            .header("X-Title", "Irudd Galaxy GM")
            .send(body)
            .map_err(|e| e.to_string())?;
        let status = resp.status().as_u16();
        let body = resp
            .into_body()
            .with_config()
            .limit(MAX_BODY_BYTES)
            .read_to_string()
            .map_err(|e| e.to_string())?;
        Ok(HttpReply { status, body })
    }
}

/// Why a call produced no audio. Every text is already redacted.
#[derive(Debug, Clone, PartialEq)]
pub enum CallError {
    /// No HTTP answer: DNS, TLS, a timeout.
    Network(String),
    /// An HTTP error status, or an error inside a 2xx stream; `status` is the HTTP
    /// status, or the error's own code for one inside a stream.
    Refused { status: u16, message: String },
    /// An answer without usable audio. `answer` is its shape, for diagnosis: the folded
    /// body with long strings shortened to a note of their length, redacted.
    Malformed {
        message: String,
        answer: Option<Value>,
        generation_id: Option<String>,
        cost_usd: Option<f64>,
    },
}

impl CallError {
    fn malformed(key: &ApiKey, message: &str, answer: Option<&Value>) -> Self {
        let shape = answer.map(|v| {
            let text = key.redact(&shorten(v).to_string());
            serde_json::from_str(&text).unwrap_or(Value::String(text))
        });
        CallError::Malformed {
            message: key.redact(message),
            answer: shape,
            generation_id: answer.and_then(generation_id),
            cost_usd: answer.and_then(cost),
        }
    }
}

/// Longest string kept whole by [`shorten`].
const SHAPE_CHARS: usize = 200;

/// `v` with every string longer than [`SHAPE_CHARS`] replaced by a note of its length.
fn shorten(v: &Value) -> Value {
    match v {
        Value::String(s) if s.len() > SHAPE_CHARS => Value::String(format!("<{} chars>", s.len())),
        Value::Array(a) => Value::Array(a.iter().map(shorten).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, v)| (k.clone(), shorten(v))).collect()),
        v => v.clone(),
    }
}

fn generation_id(v: &Value) -> Option<String> {
    v.get("id")
        .and_then(Value::as_str)
        .filter(|id| id.len() <= 128 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        .map(str::to_owned)
}

fn cost(v: &Value) -> Option<f64> {
    v.pointer("/usage/cost")
        .and_then(Value::as_f64)
        .filter(|c| c.is_finite() && *c >= 0.0)
}

/// A finished generation, read from the answer.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub bytes: Vec<u8>,
    pub format: Format,
    /// Text the model sent with it (lyrics, structure, or a tag like `<instrumental>`).
    pub text: String,
    /// The model OpenRouter says answered.
    pub model: Option<String>,
    pub generation_id: Option<String>,
    /// US dollars, as OpenRouter reported it.
    pub cost_usd: Option<f64>,
}

/// The audio file formats the engine's sound system plays.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Mp3,
    Wav,
    Ogg,
}

impl Format {
    /// From the file's first bytes; `None` for anything the sound system can't play.
    pub fn sniff(bytes: &[u8]) -> Option<Self> {
        match bytes {
            [b'I', b'D', b'3', ..] => Some(Self::Mp3),
            [0xFF, b, ..] if b & 0xE0 == 0xE0 => Some(Self::Mp3),
            [
                b'R',
                b'I',
                b'F',
                b'F',
                _,
                _,
                _,
                _,
                b'W',
                b'A',
                b'V',
                b'E',
                ..,
            ] => Some(Self::Wav),
            [b'O', b'g', b'g', b'S', ..] => Some(Self::Ogg),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Ogg => "ogg",
        }
    }
}

/// The chat completions body for one piece of music. Audio output needs a stream.
pub fn music_body(model: &str, prompt: &str, seed: Option<u64>) -> Value {
    let mut body = json!({
        "model": model,
        "modalities": ["text", "audio"],
        "audio": { "format": "mp3" },
        "stream": true,
        "messages": [{ "role": "user", "content": prompt }],
    });
    if let Some(seed) = seed {
        body["seed"] = json!(seed);
    }
    body
}

/// Posts `body` and reads the audio out of the answer. One attempt: a paid call is never
/// retried behind the caller's back.
pub fn generate(
    transport: &mut dyn Transport,
    key: &ApiKey,
    body: &Value,
) -> Result<Audio, CallError> {
    let reply = transport
        .post(CHAT_URL, &key.bearer(), &body.to_string())
        .map_err(|e| CallError::Network(key.redact(&e)))?;
    let parsed = serde_json::from_str::<Value>(&reply.body)
        .ok()
        .or_else(|| fold_stream(&reply.body));
    if !(200..=299).contains(&reply.status) {
        let message = parsed
            .as_ref()
            .and_then(error_message)
            .unwrap_or_else(|| excerpt(&key.redact(&reply.body)));
        return Err(CallError::Refused {
            status: reply.status,
            message: key.redact(&message),
        });
    }
    let Some(v) = parsed else {
        return Err(CallError::malformed(
            key,
            &format!(
                "the answer is neither JSON nor an event stream: {}",
                excerpt(&key.redact(&reply.body))
            ),
            None,
        ));
    };
    if let Some(message) = error_message(&v) {
        let status = v
            .pointer("/error/code")
            .and_then(Value::as_u64)
            .and_then(|c| u16::try_from(c).ok())
            .unwrap_or(reply.status);
        return Err(CallError::Refused {
            status,
            message: key.redact(&message),
        });
    }
    parse_audio(&v, key).map_err(|m| CallError::malformed(key, &m, Some(&v)))
}

/// An error object's message, if the body carries one.
fn error_message(v: &Value) -> Option<String> {
    let error = v.get("error").filter(|e| !e.is_null())?;
    Some(
        error
            .get("message")
            .and_then(Value::as_str)
            .map_or_else(|| error.to_string(), str::to_owned),
    )
}

/// The start of a body, for a message: base64 and long lines are not worth showing.
fn excerpt(body: &str) -> String {
    let body = body.trim();
    let mut out: String = body.chars().take(300).collect();
    if out.len() < body.len() {
        out.push_str("...");
    }
    out
}

/// A streamed chat completion (server-sent events) folded into one JSON body:
/// `choices[0].message` gathers the deltas (`content`, `audio.data` and
/// `audio.transcript` are joined in order; other fields keep their last value), and
/// `id`, `model`, `usage` and any `error` come from the chunks that carry them. Comment
/// lines (`: OPENROUTER PROCESSING`) and `[DONE]` are skipped. `None` if the text has no
/// JSON chunk.
pub fn fold_stream(text: &str) -> Option<Value> {
    let mut out = serde_json::Map::new();
    let mut message = serde_json::Map::new();
    let mut finish = Value::Null;
    let mut chunks = 0usize;
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else {
            continue;
        };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" {
            continue;
        }
        let Ok(Value::Object(chunk)) = serde_json::from_str::<Value>(data) else {
            continue;
        };
        chunks += 1;
        for (k, v) in &chunk {
            match k.as_str() {
                "choices" => {}
                "usage" | "error" if v.is_null() => {}
                _ => {
                    out.insert(k.clone(), v.clone());
                }
            }
        }
        let Some(choice) = chunk.get("choices").and_then(|c| c.get(0)) else {
            continue;
        };
        if let Some(Value::Object(delta)) = choice.get("delta").or_else(|| choice.get("message")) {
            merge_delta(&mut message, delta);
        }
        if let Some(f) = choice.get("finish_reason").filter(|f| !f.is_null()) {
            finish = f.clone();
        }
    }
    if chunks == 0 {
        return None;
    }
    out.insert("object".into(), json!("chat.completion"));
    out.insert(
        "choices".into(),
        json!([{ "index": 0, "message": message, "finish_reason": finish }]),
    );
    Some(Value::Object(out))
}

/// Adds one delta to the message being gathered: text and base64 are joined, objects
/// merged, arrays appended, anything else replaced.
fn merge_delta(into: &mut serde_json::Map<String, Value>, delta: &serde_json::Map<String, Value>) {
    const JOINED: [&str; 4] = ["content", "data", "transcript", "reasoning"];
    for (k, v) in delta {
        match (into.get_mut(k), v) {
            (_, Value::Null) => {}
            (Some(Value::String(have)), Value::String(more)) if JOINED.contains(&k.as_str()) => {
                have.push_str(more);
            }
            (Some(Value::Object(have)), Value::Object(more)) => merge_delta(have, more),
            (Some(Value::Array(have)), Value::Array(more)) => have.extend(more.iter().cloned()),
            _ => {
                into.insert(k.clone(), v.clone());
            }
        }
    }
}

/// Longest model text kept.
const MAX_TEXT_CHARS: usize = 2000;

/// Reads the audio from a chat completion: base64 at `choices[0].message.audio.data`
/// (or the first entry of an `audio` array, or an audio content part), optionally as a
/// `data:` URL; the text from `content` or `audio.transcript`. Bytes that are not a
/// format the sound system plays are rejected.
pub fn parse_audio(v: &Value, key: &ApiKey) -> Result<Audio, String> {
    let message = v.pointer("/choices/0/message").ok_or("no message")?;
    let audio = audio_of(message).ok_or_else(|| {
        if message.pointer("/audio/url").is_some() {
            "the audio came as a link, not data".to_owned()
        } else {
            format!(
                "no audio in the answer{}",
                match message.get("content").and_then(Value::as_str) {
                    Some(t) if !t.trim().is_empty() =>
                        format!("; the model said: {}", excerpt(&key.redact(t))),
                    _ => String::new(),
                }
            )
        }
    })?;
    let data = audio
        .get("data")
        .and_then(Value::as_str)
        .ok_or("no audio data")?;
    let data = match data.strip_prefix("data:") {
        Some(url) => url.split_once(',').map_or("", |(_, d)| d),
        None => data,
    };
    let bytes = decode_base64(data).ok_or("the audio isn't base64")?;
    if bytes.is_empty() {
        return Err("the audio is empty".into());
    }
    let format = Format::sniff(&bytes)
        .ok_or("the audio is not MP3, WAV or Ogg, which the sound system plays")?;
    let text = match message.get("content") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => audio
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    Ok(Audio {
        bytes,
        format,
        text: key.redact(&text).chars().take(MAX_TEXT_CHARS).collect(),
        model: v.get("model").and_then(Value::as_str).map(str::to_owned),
        generation_id: generation_id(v),
        cost_usd: cost(v),
    })
}

fn audio_of(message: &Value) -> Option<&Value> {
    let has_data = |a: &&Value| a.get("data").is_some_and(Value::is_string);
    match message.get("audio") {
        Some(a @ Value::Object(_)) if has_data(&a) => return Some(a),
        Some(Value::Array(list)) => {
            if let Some(a) = list.iter().find(has_data) {
                return Some(a);
            }
        }
        _ => {}
    }
    // Content parts: `{ "type": "output_audio", "audio": { "data": ... } }` and the like.
    message
        .get("content")?
        .as_array()?
        .iter()
        .filter(|p| {
            p.get("type")
                .and_then(Value::as_str)
                .is_some_and(|t| t.contains("audio"))
        })
        .find_map(|p| {
            ["audio", "output_audio", "input_audio"]
                .iter()
                .filter_map(|k| p.get(*k))
                .chain(std::iter::once(p))
                .find(has_data)
        })
}

/// Standard or URL-safe base64. Whitespace is skipped, and padding may close any group
/// of four, so chunks encoded one by one and joined decode whole. `None` for any other
/// character or a dangling sixth of a byte.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    fn flush(out: &mut Vec<u8>, q: &[u8; 4], n: usize) -> Option<()> {
        match n {
            0 => {}
            2 => out.push(q[0] << 2 | q[1] >> 4),
            3 => {
                out.push(q[0] << 2 | q[1] >> 4);
                out.push(q[1] << 4 | q[2] >> 2);
            }
            _ => return None,
        }
        Some(())
    }
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut q = [0u8; 4];
    let mut n = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => {
                flush(&mut out, &q, n)?;
                n = 0;
                continue;
            }
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        q[n] = v;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[
                q[0] << 2 | q[1] >> 4,
                q[1] << 4 | q[2] >> 2,
                q[2] << 6 | q[3],
            ]);
            n = 0;
        }
    }
    flush(&mut out, &q, n)?;
    Some(out)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Standard base64, for building answers in tests.
    pub(crate) fn encode_base64(bytes: &[u8]) -> String {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in bytes.chunks(3) {
            let n = (u32::from(c[0]) << 16)
                | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
                | u32::from(*c.get(2).unwrap_or(&0));
            for i in 0..4 {
                if i <= c.len() {
                    out.push(A[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// A streamed answer shaped like a recorded Lyria answer through OpenRouter: comment
    /// lines while it works, a text chunk, the audio in one or more chunks, a finish
    /// chunk and a usage chunk with the cost.
    pub(crate) fn stream(audio: &[u8], pieces: usize) -> String {
        let b64 = encode_base64(audio);
        let head = r#""id":"gen-1-abc","object":"chat.completion.chunk","created":1,"model":"vendor/music-clip","provider":"Some Provider""#;
        let mut s = String::from(": OPENROUTER PROCESSING\n\n: OPENROUTER PROCESSING\n\n");
        s += &format!(
            "data: {{{head},\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"<instrumental>\",\"role\":\"assistant\"}},\"finish_reason\":null}}]}}\n\n"
        );
        let size = b64.len().div_ceil(pieces.max(1)).max(4);
        for piece in b64.as_bytes().chunks(size) {
            let piece = std::str::from_utf8(piece).unwrap();
            s += &format!(
                "data: {{{head},\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"\",\"role\":\"assistant\",\"audio\":{{\"data\":\"{piece}\"}}}},\"finish_reason\":null}}]}}\n\n"
            );
        }
        s += &format!(
            "data: {{{head},\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"\",\"role\":\"assistant\"}},\"finish_reason\":\"stop\"}}]}}\n\n"
        );
        s += &format!(
            "data: {{{head},\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"\",\"role\":\"assistant\"}},\"finish_reason\":\"stop\"}}],\"usage\":{{\"prompt_tokens\":34,\"completion_tokens\":4,\"cost\":0.04}}}}\n\ndata: [DONE]\n\n"
        );
        s
    }

    /// Answers every POST with one canned reply, and keeps what was sent.
    pub(crate) struct Canned {
        pub reply: Result<HttpReply, String>,
        pub sent: Vec<(String, String, String)>,
    }

    impl Transport for Canned {
        fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, String> {
            self.sent.push((url.into(), auth.into(), body.into()));
            self.reply.clone()
        }
    }

    pub(crate) fn canned(status: u16, body: impl Into<String>) -> Canned {
        Canned {
            reply: Ok(HttpReply {
                status,
                body: body.into(),
            }),
            sent: Vec::new(),
        }
    }

    const KEY: &str = "sk-or-v1-0123456789abcdef0123456789abcdef";

    #[test]
    fn a_streamed_answer_folds_into_audio_text_model_and_cost() {
        let mp3 = b"ID3\x03\x00\x00\x00\x00\x00\x00rest of a file".to_vec();
        for pieces in [1, 3] {
            let mut t = canned(200, stream(&mp3, pieces));
            let key = ApiKey::new(KEY);
            let body = music_body("vendor/music-clip", "calm", Some(7));
            let audio = generate(&mut t, &key, &body).unwrap();
            assert_eq!(audio.bytes, mp3);
            assert_eq!(audio.format, Format::Mp3);
            assert_eq!(audio.text, "<instrumental>");
            assert_eq!(audio.model.as_deref(), Some("vendor/music-clip"));
            assert_eq!(audio.generation_id.as_deref(), Some("gen-1-abc"));
            assert_eq!(audio.cost_usd, Some(0.04));
            let (url, auth, sent) = &t.sent[0];
            assert_eq!(url, CHAT_URL);
            assert_eq!(auth, &format!("Bearer {KEY}"));
            let sent: Value = serde_json::from_str(sent).unwrap();
            assert_eq!(sent["stream"], true);
            assert_eq!(sent["modalities"], json!(["text", "audio"]));
            assert_eq!(sent["seed"], 7);
            assert!(!sent.to_string().contains(KEY), "the key is only a header");
        }
    }

    #[test]
    fn refusals_and_bad_answers_never_echo_the_key() {
        let key = ApiKey::new(KEY);
        let body = music_body("m", "p", None);
        let echo = format!("{{\"error\":{{\"code\":401,\"message\":\"bad key {KEY}\"}}}}");
        let Err(CallError::Refused { status, message }) =
            generate(&mut canned(401, echo.clone()), &key, &body)
        else {
            panic!("a 401 is a refusal")
        };
        assert_eq!(status, 401);
        assert!(!message.contains("0123456789abcdef"), "{message}");
        assert!(message.contains(REDACTED), "{message}");
        // An error inside a 2xx stream.
        let in_stream = format!("data: {echo}\n\ndata: [DONE]\n\n");
        let Err(CallError::Refused { status, message }) =
            generate(&mut canned(200, in_stream), &key, &body)
        else {
            panic!("an error in a stream is a refusal")
        };
        assert_eq!(status, 401);
        assert!(!message.contains("0123456789abcdef"));
        // Escaped spellings of the key in a body that is not an error object.
        let escaped: String = KEY
            .chars()
            .map(|c| format!("\\u{:04x}", c as u32))
            .collect();
        let Err(CallError::Malformed { message: m, .. }) =
            generate(&mut canned(200, format!("oops {escaped}")), &key, &body)
        else {
            panic!("not JSON is malformed")
        };
        assert!(!m.contains("\\u0030\\u0031\\u0032\\u0033\\u0034"), "{m}");
        let mut t = Canned {
            reply: Err(format!("tls failed for {KEY}")),
            sent: Vec::new(),
        };
        let Err(CallError::Network(m)) = generate(&mut t, &key, &body) else {
            panic!("no answer is a network error")
        };
        assert!(!m.contains("0123456789abcdef"), "{m}");
        assert_eq!(format!("{key:?}"), "ApiKey(<redacted>)");
    }

    #[test]
    fn text_without_audio_or_unplayable_audio_is_malformed() {
        let key = ApiKey::new(KEY);
        let body = music_body("m", "p", None);
        let text = r#"{"choices":[{"message":{"content":"I can only write words"}}]}"#;
        let Err(CallError::Malformed { message: m, .. }) =
            generate(&mut canned(200, text), &key, &body)
        else {
            panic!()
        };
        assert!(m.contains("I can only write words"), "{m}");
        // A stream with text and no audio reports its shape, its ID and its cost.
        let no_audio = stream(b"", 1).replace(r#","audio":{"data":""}"#, "");
        let Err(CallError::Malformed {
            answer,
            generation_id,
            cost_usd,
            ..
        }) = generate(&mut canned(200, no_audio), &key, &body)
        else {
            panic!()
        };
        let answer = answer.unwrap();
        assert_eq!(answer["choices"][0]["message"]["content"], "<instrumental>");
        assert_eq!(generation_id.as_deref(), Some("gen-1-abc"));
        assert_eq!(cost_usd, Some(0.04));
        // Long strings are shortened and the key is redacted in the shape.
        let long = format!(
            r#"{{"id":"gen-2","choices":[{{"message":{{"content":"{KEY}","audio":{{"url":"{}"}}}}}}]}}"#,
            "x".repeat(500)
        );
        let Err(CallError::Malformed { answer, .. }) =
            generate(&mut canned(200, long), &key, &body)
        else {
            panic!()
        };
        let shape = answer.unwrap().to_string();
        assert!(shape.contains("<500 chars>"), "{shape}");
        assert!(!shape.contains("0123456789abcdef"), "{shape}");
        let flac = stream(b"fLaC\x00\x00\x00\x22", 1);
        assert!(matches!(
            generate(&mut canned(200, flac), &key, &body),
            Err(CallError::Malformed { .. })
        ));
        assert!(matches!(
            generate(&mut canned(503, "upstream busy"), &key, &body),
            Err(CallError::Refused { status: 503, .. })
        ));
    }

    #[test]
    fn base64_joined_from_padded_chunks_decodes_whole() {
        let joined = format!("{}{}", encode_base64(b"ab"), encode_base64(b"cde"));
        assert_eq!(decode_base64(&joined).unwrap(), b"abcde");
        assert_eq!(decode_base64("YQ").unwrap(), b"a");
        assert!(decode_base64("Y").is_none());
        assert!(decode_base64("a!").is_none());
    }

    #[test]
    fn formats_are_told_by_their_bytes() {
        assert_eq!(Format::sniff(b"ID3\x04"), Some(Format::Mp3));
        assert_eq!(Format::sniff(&[0xFF, 0xF3, 0x48]), Some(Format::Mp3));
        assert_eq!(Format::sniff(b"RIFF\0\0\0\0WAVEfmt "), Some(Format::Wav));
        assert_eq!(Format::sniff(b"OggS\0"), Some(Format::Ogg));
        assert_eq!(Format::sniff(b"fLaC"), None);
    }
}
