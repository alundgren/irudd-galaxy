//! Generated music: Google Lyria 3 through OpenRouter, cached beside the save.
//!
//! The radio asks a [`MusicLibrary`] what a channel should play ([`MusicLibrary::track`])
//! and, when a channel comes into range, asks for tracks for it with the channel's mood
//! line ([`MusicLibrary::want`]). Neither blocks: generation runs on the library's own
//! worker thread, one track at a time, round robin over the channels that want one.
//!
//! - **The call.** Chat completions with `modalities: ["text", "audio"]` and `stream:
//!   true` (OpenRouter streams audio output), through [`Client::call`] with purpose
//!   `music`, so every call is in the call log with its cost as OpenRouter reported it.
//!   The client folds the stream into one body ([`crate::openrouter::fold_stream`]);
//!   [`parse_track`] takes the base64 audio from `choices[0].message.audio.data` (or an
//!   audio content part) and the text from `content` or `audio.transcript`.
//! - **The cache.** `<stem>.music/<channel>/<n>.mp3` beside `<stem>.sqlite`, plus
//!   `index.json` (model, prompt, cost against the expected price, generation ID; never
//!   the key). A cached track is never regenerated: a channel with
//!   `music.tracks_per_channel` tracks on disk asks for nothing, and a restart plays the
//!   cache without a call. A file is only ever written to a free slot.
//! - **Limits.** At most `music.run_generations` generations per run (one launch), and
//!   none once `music.spend_cap_usd` has been spent from this save's cache. HTTP 402
//!   (insufficient credit) stops music for the rest of the run, as does a refused key
//!   (401, 403). The client retries a timeout, a transport error, a 429 and a 5xx, with
//!   a doubling backoff; after `music.max_attempts` the channel gives up for the run.
//! - **Placeholders.** Whenever a channel has no generated track (no key, no save,
//!   capped, failed, or still generating) it plays one of two named loops the audio
//!   layer synthesises: [`PLACEHOLDER_CALM`] or [`PLACEHOLDER_DRIFT`].
//!
//! Nothing here touches the sim or the save: the files are view-only, and the wall
//! clock is used only for the call log and the index.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::calllog::{self, CallId, CallRow, CallSink, Verdict};
use crate::mind::{MindError, Purpose};
use crate::openrouter::{ApiKey, CHAT_PATH, Call, Client, Transport, UreqTransport};

/// The calm placeholder loop, for channels without a generated track.
pub const PLACEHOLDER_CALM: &str = "placeholder.calm";
/// The drifting placeholder loop.
pub const PLACEHOLDER_DRIFT: &str = "placeholder.drift";

/// The index file in a save's music folder.
pub const INDEX_FILE: &str = "index.json";

/// What a channel plays.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrackSource {
    /// A cached generated track (MP3, or another format the model chose, by extension).
    Generated(PathBuf),
    /// A named loop the audio layer makes: [`PLACEHOLDER_CALM`] or [`PLACEHOLDER_DRIFT`].
    Placeholder(&'static str),
}

/// Generated music (`[music]` in `mind.toml`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct MusicRules {
    /// Off: channels play the cache and placeholders, and nothing is generated.
    pub enabled: bool,
    /// `google/lyria-3-clip-preview` (about 30 s) or `google/lyria-3-pro-preview` (a
    /// full song, up to about 3 minutes).
    pub model: String,
    /// Tracks a channel collects; a channel with this many cached asks for no more.
    pub tracks_per_channel: u32,
    /// Generations started per run (one launch of the game), failed ones included.
    pub run_generations: u32,
    /// Spend cap for this save's music, summed over every run (reported cost, or the
    /// expected price where none was reported).
    pub spend_cap_usd: f64,
    /// The prompt; `{mood}` is the channel's mood line.
    pub prompt: String,
    /// Asks for no vocals: [`MusicRules::instrumental_prompt`] is added to the prompt.
    pub instrumental: bool,
    pub instrumental_prompt: String,
    /// The audio format asked for (`audio.format`); the file's own bytes decide the
    /// extension it is saved with.
    pub audio_format: String,
    /// HTTP attempts per generation, retries included (1 to 4).
    pub max_attempts: u8,
    /// First wait before retrying a 429 or 5xx that named none; it doubles.
    pub backoff_ms: u64,
    /// Per-attempt HTTP timeout: a full song can take minutes to stream.
    pub timeout_ms: u64,
    /// No retry starts after this long since the first attempt.
    pub deadline_ms: u64,
    /// The listed price per generation by model, in US dollars, recorded beside the
    /// reported cost.
    pub expected_usd: BTreeMap<String, f64>,
}

pub const CLIP_MODEL: &str = "google/lyria-3-clip-preview";
pub const PRO_MODEL: &str = "google/lyria-3-pro-preview";

impl Default for MusicRules {
    fn default() -> Self {
        Self {
            enabled: true,
            model: CLIP_MODEL.into(),
            tracks_per_channel: 2,
            run_generations: 2,
            spend_cap_usd: 0.40,
            prompt: "Background music for a small space station's radio channel, heard \
                     while flying between stations. Mood: {mood}."
                .into(),
            instrumental: true,
            instrumental_prompt: "Instrumental only: no vocals, no singing, no lyrics.".into(),
            audio_format: "mp3".into(),
            max_attempts: 3,
            backoff_ms: 2000,
            timeout_ms: 240_000,
            deadline_ms: 600_000,
            expected_usd: BTreeMap::from([(CLIP_MODEL.into(), 0.04), (PRO_MODEL.into(), 0.08)]),
        }
    }
}

/// Longest mood line put in a prompt.
const MAX_MOOD_CHARS: usize = 300;
/// Longest lyrics or structure text kept in the index.
const MAX_TEXT_CHARS: usize = 2000;

impl MusicRules {
    /// The prompt for a mood line (control characters dropped, length capped).
    pub fn prompt_for(&self, mood: &str) -> String {
        let mood: String = mood
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .take(MAX_MOOD_CHARS)
            .collect();
        let mut prompt = self.prompt.replace("{mood}", mood.trim());
        if self.instrumental && !self.instrumental_prompt.is_empty() {
            prompt.push(' ');
            prompt.push_str(&self.instrumental_prompt);
        }
        prompt
    }

    /// The listed price of one generation with the configured model, if known.
    pub fn expected(&self) -> Option<f64> {
        self.expected_usd.get(&self.model).copied()
    }

    /// Checks the data: a prompt with `{mood}`, 1 to 4 attempts, at most 8 tracks a
    /// channel, finite prices, and timeouts above 0.
    pub fn validate(&self) -> Result<(), String> {
        if !self.prompt.contains("{mood}") {
            return Err("music.prompt must contain {mood}".into());
        }
        if !(1..=4).contains(&self.max_attempts) {
            return Err("music.max_attempts must be 1 to 4".into());
        }
        if self.tracks_per_channel > 8 {
            return Err("music.tracks_per_channel must be at most 8".into());
        }
        if self.model.trim().is_empty() || self.timeout_ms == 0 || self.deadline_ms == 0 {
            return Err("music.model, music.timeout_ms and music.deadline_ms must be set".into());
        }
        let prices = std::iter::once(self.spend_cap_usd).chain(self.expected_usd.values().copied());
        if prices.into_iter().any(|v| !v.is_finite() || v < 0.0) {
            return Err(
                "music.spend_cap_usd and music.expected_usd must be finite amounts >= 0".into(),
            );
        }
        Ok(())
    }

    fn attempts(&self) -> u8 {
        self.max_attempts.clamp(1, 4)
    }
}

/// The chat completions body for one track.
pub fn music_body(rules: &MusicRules, session_id: &str, prompt: &str) -> Value {
    json!({
        "model": rules.model,
        "session_id": session_id,
        "modalities": ["text", "audio"],
        "audio": { "format": rules.audio_format },
        "stream": true,
        "messages": [{ "role": "user", "content": prompt }],
    })
}

/// A file format, from the file's first bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioFormat {
    Mp3,
    Wav,
    Flac,
    Ogg,
}

impl AudioFormat {
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
            [b'f', b'L', b'a', b'C', ..] => Some(Self::Flac),
            [b'O', b'g', b'g', b'S', ..] => Some(Self::Ogg),
            _ => None,
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            Self::Mp3 => "mp3",
            Self::Wav => "wav",
            Self::Flac => "flac",
            Self::Ogg => "ogg",
        }
    }

    fn from_ext(ext: &str) -> Option<Self> {
        [Self::Mp3, Self::Wav, Self::Flac, Self::Ogg]
            .into_iter()
            .find(|f| f.ext() == ext)
    }
}

/// A track read from an answer.
#[derive(Clone, Debug, PartialEq)]
pub struct Generated {
    pub audio: Vec<u8>,
    pub format: AudioFormat,
    /// Lyrics or structure text the model sent with it (may be empty).
    pub text: String,
}

/// Reads the track from a chat completion (a plain body, or a stream the client has
/// folded): base64 audio at `choices[0].message.audio.data` (or the first entry of an
/// `audio` array, or an audio content part), optionally as a `data:` URL; the text from
/// `content` or `audio.transcript`. Bytes that aren't a known audio format are rejected.
pub fn parse_track(v: &Value) -> Result<Generated, MindError> {
    let bad = |m: &str| MindError::Malformed(m.to_owned());
    let message = v
        .pointer("/choices/0/message")
        .ok_or_else(|| bad("no message"))?;
    let audio = audio_of(message).ok_or_else(|| {
        if message.pointer("/audio/url").is_some() {
            bad("the audio came as a link, not data")
        } else {
            bad("no audio in the answer")
        }
    })?;
    let data = audio
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| bad("no audio data"))?;
    let data = match data.strip_prefix("data:") {
        Some(url) => url.split_once(',').map_or("", |(_, d)| d),
        None => data,
    };
    let bytes = decode_base64(data).ok_or_else(|| bad("the audio isn't base64"))?;
    if bytes.is_empty() {
        return Err(bad("the audio is empty"));
    }
    let format =
        AudioFormat::sniff(&bytes).ok_or_else(|| bad("the data isn't a known audio format"))?;
    let text = match message.get("content") {
        Some(Value::String(s)) if !s.trim().is_empty() => s.clone(),
        _ => audio
            .get("transcript")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    Ok(Generated {
        audio: bytes,
        format,
        text: text.chars().take(MAX_TEXT_CHARS).collect(),
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

/// The music folder for a save: `<stem>.music` in the save's folder.
pub fn dir_for(save: &Path) -> PathBuf {
    let stem = save
        .file_stem()
        .map_or_else(|| "galaxy".into(), |s| s.to_string_lossy().into_owned());
    save.with_file_name(format!("{stem}.music"))
}

/// A channel's folder name: its ID with anything but ASCII letters, digits, `.`, `_`
/// and `-` replaced by `_`, never empty and never only dots.
pub fn folder(channel: &str) -> String {
    let f: String = channel
        .chars()
        .take(96)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    if f.chars().all(|c| c == '.') {
        format!("_{f}")
    } else {
        f
    }
}

/// The placeholder loop a channel plays, the same one every time.
pub fn placeholder(channel: &str) -> &'static str {
    if fnv(channel).is_multiple_of(2) {
        PLACEHOLDER_CALM
    } else {
        PLACEHOLDER_DRIFT
    }
}

fn fnv(s: &str) -> u64 {
    s.bytes().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// One cached track, as recorded in `index.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CachedTrack {
    pub channel: String,
    /// The folder under the music folder.
    pub folder: String,
    pub n: u32,
    /// The file name in the folder (`0.mp3`).
    pub file: String,
    pub format: String,
    pub bytes: u64,
    /// The model asked for, and the one OpenRouter says answered.
    pub model: String,
    pub actual_model: Option<String>,
    pub mood: String,
    pub prompt: String,
    /// Lyrics or structure text sent with the audio (capped).
    pub text: String,
    /// US dollars as OpenRouter reported it; `None` if it reported none.
    pub cost_usd: Option<f64>,
    pub cost_source: Option<String>,
    /// The listed price for the model.
    pub expected_usd: Option<f64>,
    pub generation_id: Option<String>,
    pub attempts: u8,
    pub latency_ms: u64,
    /// Wall clock, ms since the epoch.
    pub created_ms: i64,
}

/// `index.json`: the cached tracks and what this save's music has cost.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicIndex {
    pub version: u32,
    /// Runs that opened this cache (which cached track a channel plays turns with it).
    pub runs: u64,
    /// Every generation's cost, failed ones included: reported, or the expected price
    /// for a call still out at shutdown.
    pub spent_usd: f64,
    pub generations: u64,
    pub tracks: Vec<CachedTrack>,
}

impl MusicIndex {
    const VERSION: u32 = 1;

    fn count(&self, folder: &str) -> usize {
        self.tracks.iter().filter(|t| t.folder == folder).count()
    }
}

/// Why nothing more is generated this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stopped {
    /// No save to cache beside (headless runs).
    NoSave,
    /// `OPENROUTER_API_KEY` isn't set.
    NoKey,
    /// `music.enabled` is off.
    Disabled,
    /// The music folder couldn't be written.
    NoCache,
    /// `music.run_generations` reached.
    RunCap,
    /// `music.spend_cap_usd` reached for this save.
    SpendCap,
    /// HTTP 402: the key has too little credit left.
    NoCredit,
    /// HTTP 401 or 403: the key was refused.
    KeyRefused,
    /// [`MusicLibrary::shutdown`].
    Shutdown,
}

impl Stopped {
    pub fn reason(self) -> &'static str {
        match self {
            Stopped::NoSave => "no save to cache tracks beside",
            Stopped::NoKey => "OPENROUTER_API_KEY isn't set",
            Stopped::Disabled => "music.enabled is off",
            Stopped::NoCache => "the music folder can't be written",
            Stopped::RunCap => "this run's generation cap is reached",
            Stopped::SpendCap => "this save's music spend cap is reached",
            Stopped::NoCredit => "the key is out of credit (HTTP 402)",
            Stopped::KeyRefused => "the key was refused",
            Stopped::Shutdown => "the game is shutting down",
        }
    }
}

/// What the library has done this run.
#[derive(Clone, Debug, PartialEq)]
pub struct MusicStatus {
    /// Generations started this run.
    pub started: u32,
    pub cap: u32,
    pub cached: usize,
    pub spent_usd: f64,
    /// Channels that gave up this run (their generation failed or was rejected).
    pub failed: Vec<String>,
    pub stopped: Option<Stopped>,
    /// Channels queued or generating.
    pub busy: usize,
}

impl MusicStatus {
    /// One line for the game's notes.
    pub fn summary(&self) -> String {
        match self.stopped {
            Some(s) => format!(
                "music: {} cached tracks; generating nothing: {}",
                self.cached,
                s.reason()
            ),
            None => format!(
                "music: {} cached tracks; up to {} generations this run",
                self.cached, self.cap
            ),
        }
    }
}

struct State {
    index: MusicIndex,
    /// The track each channel plays this run, once it has one.
    picked: BTreeMap<String, PathBuf>,
    /// Channels queued or generating.
    queued: BTreeSet<String>,
    failed: BTreeSet<String>,
    started: u32,
    stopped: Option<Stopped>,
    /// The generation out now: its channel and when it started.
    out: Option<(String, i64)>,
}

struct Shared {
    dir: Option<PathBuf>,
    rules: MusicRules,
    log: CallSink,
    state: Mutex<State>,
    changed: Condvar,
}

struct Want {
    channel: String,
    mood: String,
}

/// The radio's music: cached generated tracks and placeholders, and the generation of
/// new tracks in the background. Cheap to clone; clones share one library. The worker
/// thread ends when the last clone is dropped (after the call it is in, if any).
#[derive(Clone)]
pub struct MusicLibrary {
    shared: Arc<Shared>,
    /// To the worker; only the handles hold it, so the worker sees the last one go.
    tx: Arc<Mutex<Option<Sender<Want>>>>,
}

impl std::fmt::Debug for MusicLibrary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("MusicLibrary").field(&self.status()).finish()
    }
}

impl MusicLibrary {
    /// Placeholders only: no cache, no generation (headless runs and tests).
    pub fn placeholders() -> Self {
        Self::build(
            None,
            MusicRules::default(),
            CallSink::new(),
            MusicIndex::default(),
            Some(Stopped::NoSave),
        )
    }

    /// The library for the save at `save`: plays `<stem>.music/` and, with the key from
    /// `OPENROUTER_API_KEY` and `music.enabled`, generates into it. Calls go to `log`.
    pub fn open(save: &Path, rules: &MusicRules, log: CallSink) -> Self {
        match ApiKey::from_env() {
            Ok(key) => {
                let transport = UreqTransport::new(Duration::from_millis(rules.timeout_ms));
                Self::open_with(dir_for(save), rules.clone(), Some((transport, key)), log)
            }
            Err(_) => Self::open_with::<UreqTransport>(dir_for(save), rules.clone(), None, log),
        }
    }

    /// The library over the music folder `dir`, generating through `transport` with
    /// `key`, or (`None`) only playing what is cached.
    pub fn open_with<T: Transport + 'static>(
        dir: PathBuf,
        rules: MusicRules,
        generate: Option<(T, ApiKey)>,
        log: CallSink,
    ) -> Self {
        let mut index = read_index(&dir);
        index.runs += 1;
        let mut stopped = match &generate {
            None => Some(Stopped::NoKey),
            Some(_) if !rules.enabled => Some(Stopped::Disabled),
            Some(_) => None,
        };
        // The cache must be writable before anything is bought for it. A cache that
        // only plays needs no write.
        if stopped.is_none()
            && (std::fs::create_dir_all(&dir).is_err() || write_index(&dir, &index).is_err())
        {
            stopped = Some(Stopped::NoCache);
        }
        let lib = Self::build(Some(dir), rules, log.clone(), index, stopped);
        if let (Some((transport, key)), None) = (generate, stopped) {
            let client = Client::new(transport, key).with_log(log);
            let (tx, rx) = mpsc::channel();
            *lock(&lib.tx) = Some(tx);
            let shared = Arc::clone(&lib.shared);
            std::thread::Builder::new()
                .name("galaxy-music".into())
                .spawn(move || work(&shared, client, rx))
                .expect("spawning the music thread");
        }
        lib
    }

    fn build(
        dir: Option<PathBuf>,
        rules: MusicRules,
        log: CallSink,
        index: MusicIndex,
        stopped: Option<Stopped>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                dir,
                rules,
                log,
                state: Mutex::new(State {
                    index,
                    picked: BTreeMap::new(),
                    queued: BTreeSet::new(),
                    failed: BTreeSet::new(),
                    started: 0,
                    stopped,
                    out: None,
                }),
                changed: Condvar::new(),
            }),
            tx: Arc::new(Mutex::new(None)),
        }
    }

    /// What `channel` should play now: a cached generated track if one exists, else a
    /// named placeholder loop. Never blocks and never calls the network.
    ///
    /// Stable within a run: a channel keeps the track it was first given (which of its
    /// cached tracks that is turns with each run), and moves from its placeholder to a
    /// generated track once the first one is cached. [`MusicLibrary::tracks`] lists all.
    pub fn track(&self, channel: &str) -> TrackSource {
        let Some(dir) = &self.shared.dir else {
            return TrackSource::Placeholder(placeholder(channel));
        };
        let f = folder(channel);
        let mut st = lock(&self.shared.state);
        if let Some(p) = st.picked.get(&f) {
            return TrackSource::Generated(p.clone());
        }
        let mine: Vec<&CachedTrack> = st.index.tracks.iter().filter(|t| t.folder == f).collect();
        if mine.is_empty() {
            return TrackSource::Placeholder(placeholder(channel));
        }
        let turn = (st.index.runs.wrapping_add(fnv(&f)) % mine.len() as u64) as usize;
        let path = dir.join(&f).join(&mine[turn].file);
        st.picked.insert(f, path.clone());
        TrackSource::Generated(path)
    }

    /// Every cached track for `channel`, in slot order.
    pub fn tracks(&self, channel: &str) -> Vec<PathBuf> {
        let Some(dir) = &self.shared.dir else {
            return Vec::new();
        };
        let f = folder(channel);
        let st = lock(&self.shared.state);
        let mut mine: Vec<&CachedTrack> =
            st.index.tracks.iter().filter(|t| t.folder == f).collect();
        mine.sort_by_key(|t| t.n);
        mine.iter().map(|t| dir.join(&f).join(&t.file)).collect()
    }

    /// Ask for tracks for `channel` with a mood prompt; generation (if allowed) happens in
    /// the background. Safe to call every time a channel comes into range.
    ///
    /// Does nothing for a channel with its tracks cached, one already queued, one that
    /// gave up this run, or once generation has stopped for the run.
    pub fn want(&self, channel: &str, mood: &str) {
        let f = folder(channel);
        let mut st = lock(&self.shared.state);
        if st.stopped.is_some()
            || st.index.count(&f) >= self.shared.rules.tracks_per_channel as usize
            || st.queued.contains(&f)
            || st.failed.contains(&f)
        {
            return;
        }
        if st.started >= self.shared.rules.run_generations {
            st.stopped = Some(Stopped::RunCap);
            return;
        }
        let tx = lock(&self.tx);
        let Some(tx) = tx.as_ref() else { return };
        let want = Want {
            channel: channel.to_owned(),
            mood: mood.to_owned(),
        };
        if tx.send(want).is_ok() {
            st.queued.insert(f);
        }
    }

    pub fn status(&self) -> MusicStatus {
        let st = lock(&self.shared.state);
        MusicStatus {
            started: st.started,
            cap: self.shared.rules.run_generations,
            cached: st.index.tracks.len(),
            spent_usd: st.index.spent_usd,
            failed: st.failed.iter().cloned().collect(),
            stopped: st.stopped,
            busy: st.queued.len(),
        }
    }

    /// The index as it stands (a copy).
    pub fn index(&self) -> MusicIndex {
        lock(&self.shared.state).index.clone()
    }

    /// Waits up to `wait` until nothing is queued or generating. `true` if idle.
    pub fn idle(&self, wait: Duration) -> bool {
        let end = Instant::now() + wait;
        let mut st = lock(&self.shared.state);
        while !st.queued.is_empty() {
            let left = end.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            st = self
                .shared
                .changed
                .wait_timeout(st, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        true
    }

    /// Stops generating for the rest of the run. A generation still out is booked in
    /// the call log and the index at its expected price, so spend is never
    /// under-counted when the game quits mid-call. Call before the call log's flush.
    pub fn shutdown(&self) {
        *lock(&self.tx) = None;
        let mut st = lock(&self.shared.state);
        if st.stopped.is_none() {
            st.stopped = Some(Stopped::Shutdown);
        }
        let Some((channel, ts_ms)) = st.out.take() else {
            return;
        };
        let rules = &self.shared.rules;
        let expected = rules.expected();
        st.index.spent_usd += expected.unwrap_or(0.0);
        if let Some(dir) = &self.shared.dir {
            let _ = write_index(dir, &st.index);
        }
        drop(st);
        self.shared.log.record(CallRow {
            uid: CallId::fresh(),
            ts_ms,
            purpose: Purpose::Music.as_str().to_owned(),
            requested_model: rules.model.clone(),
            actual_model: None,
            task_type: None,
            session_id: Some(channel),
            generation_id: None,
            prompt_tokens: None,
            completion_tokens: None,
            cost_usd: expected,
            cost_source: expected.map(|_| "expected".to_owned()),
            latency_ms: (calllog::now_ms() - ts_ms).max(0) as u64,
            attempts: 0,
            http_status: None,
            error_kind: Some(MindError::Cancelled.tag().to_owned()),
            verdict: Verdict::Failed,
            reason: Some(
                "the game quit while a track was generating: booked at the expected price"
                    .to_owned(),
            ),
            prompt: String::new(),
            output: String::new(),
        });
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The index in `dir`, with tracks whose file is gone dropped and files on disk that
/// it lacks (an index lost or written over) added back, so a cached file is never
/// written over. An unreadable index starts empty.
fn read_index(dir: &Path) -> MusicIndex {
    let mut index: MusicIndex = std::fs::read_to_string(dir.join(INDEX_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    index.version = MusicIndex::VERSION;
    // An entry names a file in its own folder and nothing else.
    index.tracks.retain(|t| {
        folder(&t.folder) == t.folder
            && AudioFormat::from_ext(&t.format).is_some()
            && t.file == format!("{}.{}", t.n, t.format)
            && dir.join(&t.folder).join(&t.file).is_file()
    });
    let Ok(folders) = std::fs::read_dir(dir) else {
        return index;
    };
    for entry in folders.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if !entry.path().is_dir() || folder(&name) != name {
            continue;
        }
        let Ok(files) = std::fs::read_dir(entry.path()) else {
            continue;
        };
        for file in files.flatten() {
            let fname = file.file_name().to_string_lossy().into_owned();
            let Some((n, ext)) = fname.split_once('.') else {
                continue;
            };
            let (Ok(n), Some(format)) = (n.parse::<u32>(), AudioFormat::from_ext(ext)) else {
                continue;
            };
            if index.tracks.iter().any(|t| t.folder == name && t.n == n) {
                continue;
            }
            index.tracks.push(CachedTrack {
                channel: name.clone(),
                folder: name.clone(),
                n,
                file: fname,
                format: format.ext().to_owned(),
                bytes: file.metadata().map_or(0, |m| m.len()),
                ..CachedTrack::default()
            });
        }
    }
    index
}

/// Writes the index whole, through a temporary file, so a crash leaves the old one.
fn write_index(dir: &Path, index: &MusicIndex) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(index).map_err(std::io::Error::other)?;
    let part = dir.join(format!("{INDEX_FILE}.part"));
    std::fs::write(&part, text)?;
    std::fs::rename(&part, dir.join(INDEX_FILE))
}

/// The first free slot in a channel's folder: no cached track and no file of any
/// format with that number.
fn free_slot(dir: &Path, index: &MusicIndex, f: &str, want: u32) -> Option<u32> {
    (0..want).find(|n| {
        !index.tracks.iter().any(|t| t.folder == f && t.n == *n)
            && [
                AudioFormat::Mp3,
                AudioFormat::Wav,
                AudioFormat::Flac,
                AudioFormat::Ogg,
            ]
            .iter()
            .all(|fmt| !dir.join(f).join(format!("{n}.{}", fmt.ext())).exists())
    })
}

/// The worker: one generation at a time, round robin over the channels that want one.
fn work<T: Transport>(shared: &Shared, mut client: Client<T>, rx: Receiver<Want>) {
    let dir = shared
        .dir
        .clone()
        .expect("a generating library has a folder");
    let rules = &shared.rules;
    let mut queue = VecDeque::new();
    loop {
        if queue.is_empty() {
            match rx.recv() {
                Ok(w) => queue.push_back(w),
                Err(_) => break,
            }
        }
        while let Ok(w) = rx.try_recv() {
            queue.push_back(w);
        }
        let Some(want) = queue.pop_front() else {
            continue;
        };
        let f = folder(&want.channel);
        // The gate: stopped, capped or full channels leave the queue.
        let slot = {
            let mut st = lock(&shared.state);
            if st.stopped.is_none() && st.started >= rules.run_generations {
                st.stopped = Some(Stopped::RunCap);
            }
            if st.stopped.is_none() && st.index.spent_usd >= rules.spend_cap_usd {
                st.stopped = Some(Stopped::SpendCap);
            }
            let slot = match st.stopped {
                Some(_) => None,
                None => free_slot(&dir, &st.index, &f, rules.tracks_per_channel),
            };
            match slot {
                Some(n) => {
                    st.started += 1;
                    st.out = Some((want.channel.clone(), calllog::now_ms()));
                    n
                }
                None => {
                    st.queued.remove(&f);
                    shared.changed.notify_all();
                    continue;
                }
            }
        };
        let outcome = generate(&mut client, rules, &dir, &want, &f, slot);
        let mut st = lock(&shared.state);
        if st.out.take().is_none() {
            // Shut down while this was out: it is booked already.
            st.queued.remove(&f);
            shared.changed.notify_all();
            continue;
        }
        st.index.generations += 1;
        st.index.spent_usd += outcome.cost_usd;
        match outcome.result {
            Ok(track) => st.index.tracks.push(track),
            Err(stop) => {
                st.failed.insert(f.clone());
                if let Some(stop) = stop {
                    st.stopped.get_or_insert(stop);
                }
            }
        }
        let _ = write_index(&dir, &st.index);
        let more = st.stopped.is_none()
            && !st.failed.contains(&f)
            && st.index.count(&f) < rules.tracks_per_channel as usize;
        if more {
            queue.push_back(want);
        } else {
            st.queued.remove(&f);
        }
        shared.changed.notify_all();
    }
}

struct Outcome {
    cost_usd: f64,
    /// The cached track, or why not: `Some(stop)` stops music for the run.
    result: Result<CachedTrack, Option<Stopped>>,
}

/// One generation: the call, the parse, the file, and the verdict in the call log.
fn generate<T: Transport>(
    client: &mut Client<T>,
    rules: &MusicRules,
    dir: &Path,
    want: &Want,
    f: &str,
    n: u32,
) -> Outcome {
    let prompt = rules.prompt_for(&want.mood);
    let body = music_body(rules, &want.channel, &prompt);
    let answer = client.call(&Call {
        purpose: Purpose::Music,
        model: &rules.model,
        path: CHAT_PATH,
        body: &body,
        session_id: Some(&want.channel),
        max_attempts: rules.attempts(),
        deadline_ms: rules.deadline_ms,
        backoff_ms: rules.backoff_ms,
        metadata: false,
    });
    let cost_usd = answer.cost_usd();
    let v = match &answer.result {
        Ok(v) => v,
        Err(e) => {
            let stop = match e.status() {
                Some(402) => Some(Stopped::NoCredit),
                Some(401 | 403) => Some(Stopped::KeyRefused),
                _ => None,
            };
            return Outcome {
                cost_usd,
                result: Err(stop),
            };
        }
    };
    let saved = parse_track(v).and_then(|g| {
        let file = format!("{n}.{}", g.format.ext());
        save_file(&dir.join(f), &file, &g.audio)
            .map_err(|e| MindError::Malformed(format!("couldn't cache the track: {e}")))?;
        Ok((g, file))
    });
    match saved {
        Ok((g, file)) => {
            client.verdict(
                &answer,
                Verdict::Accepted,
                Some(format!("cached as {f}/{file} ({} bytes)", g.audio.len())),
            );
            Outcome {
                cost_usd,
                result: Ok(CachedTrack {
                    channel: want.channel.clone(),
                    folder: f.to_owned(),
                    n,
                    file,
                    format: g.format.ext().to_owned(),
                    bytes: g.audio.len() as u64,
                    model: rules.model.clone(),
                    actual_model: (!answer.model.is_empty()).then(|| answer.model.clone()),
                    mood: want.mood.clone(),
                    prompt,
                    text: g.text,
                    cost_usd: answer.usage.cost_usd,
                    cost_source: answer.usage.cost_source.map(str::to_owned),
                    expected_usd: rules.expected(),
                    generation_id: answer.generation_id.clone(),
                    attempts: answer.attempts,
                    latency_ms: answer.latency_ms,
                    created_ms: calllog::now_ms(),
                }),
            }
        }
        Err(e) => {
            client.verdict(&answer, Verdict::Rejected, Some(e.to_string()));
            Outcome {
                cost_usd,
                result: Err(None),
            }
        }
    }
}

/// Writes `bytes` to `folder/file` through a temporary file, so a half-written track is
/// never mistaken for a cached one.
fn save_file(folder: &Path, file: &str, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(folder)?;
    let part = folder.join(format!("{file}.part"));
    std::fs::write(&part, bytes)?;
    std::fs::rename(&part, folder.join(file))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_decodes_whole_and_in_padded_chunks() {
        assert_eq!(decode_base64("SUQzBAA=").unwrap(), b"ID3\x04\x00");
        assert_eq!(decode_base64("SUQz\nBAA").unwrap(), b"ID3\x04\x00");
        // Two chunks encoded one by one and joined.
        assert_eq!(decode_base64("SUQ=BAA=").unwrap(), b"ID\x04\x00");
        assert_eq!(decode_base64("_-8=").unwrap(), [0xFF, 0xEF]);
        assert!(decode_base64("SUQz!").is_none());
        assert!(decode_base64("S").is_none());
    }

    #[test]
    fn formats_are_sniffed() {
        assert_eq!(AudioFormat::sniff(b"ID3\x04"), Some(AudioFormat::Mp3));
        assert_eq!(
            AudioFormat::sniff(&[0xFF, 0xFB, 0x90]),
            Some(AudioFormat::Mp3)
        );
        assert_eq!(
            AudioFormat::sniff(b"RIFF\0\0\0\0WAVEfmt "),
            Some(AudioFormat::Wav)
        );
        assert_eq!(AudioFormat::sniff(b"OggS"), Some(AudioFormat::Ogg));
        assert_eq!(AudioFormat::sniff(b"{\"error\""), None);
    }

    #[test]
    fn folders_are_safe_names() {
        assert_eq!(folder("radio.kettle_dock"), "radio.kettle_dock");
        assert_eq!(folder("../Up There"), ".._up_there");
        assert_eq!(folder(".."), "_..");
        assert_eq!(folder(""), "_");
    }

    #[test]
    fn placeholders_are_named_and_stable() {
        for ch in [
            "radio.lantern_wharf",
            "radio.kettle_dock",
            "beacon.glass_drift",
        ] {
            let p = placeholder(ch);
            assert!(p == PLACEHOLDER_CALM || p == PLACEHOLDER_DRIFT);
            assert_eq!(p, placeholder(ch));
        }
    }

    #[test]
    fn the_prompt_carries_the_mood_and_asks_for_no_vocals() {
        let rules = MusicRules::default();
        let p = rules.prompt_for("warm lamps,\nslow tide");
        assert!(p.contains("Mood: warm lamps, slow tide."), "{p}");
        assert!(p.contains("no vocals"), "{p}");
        let body = music_body(&rules, "radio.x", &p);
        assert_eq!(body["model"], CLIP_MODEL);
        assert_eq!(body["stream"], true);
        assert_eq!(body["modalities"], json!(["text", "audio"]));
        assert_eq!(body["session_id"], "radio.x");
    }
}
