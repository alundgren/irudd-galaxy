//! Generated music against a mock HTTP layer: the parser on the documented shape, the
//! cache, the per-run cap, the stop on HTTP 402, retries, and placeholders without a
//! key. No key and no network.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use galaxy_mind::calllog::{CallLog, CallSink, LogRules};
use galaxy_mind::music::{
    self, AudioFormat, MusicLibrary, MusicRules, PLACEHOLDER_CALM, PLACEHOLDER_DRIFT, Stopped,
    TrackSource,
};
use galaxy_mind::openrouter::{Call, Client, fold_stream};
use galaxy_mind::{ApiKey, HttpReply, MindError, Transport};
use serde_json::{Value, json};

const KEY: &str = "sk-or-v1-music-test-key-0123456789abcdef";
const WAIT: Duration = Duration::from_secs(10);

// --- The documented shape, base64 trimmed to a few bytes ---

/// Audio output streams (`stream: true`): `delta.audio.data` carries base64 chunks to
/// join, `delta.audio.transcript` text, `delta.content` the lyrics or structure, and
/// the last chunk `usage` with `cost`. OpenRouter interleaves `: OPENROUTER PROCESSING`
/// comments. The two chunks join to an MP3 start: `ID3` then a frame sync.
const STREAM: &str = concat!(
    ": OPENROUTER PROCESSING\n\n",
    "data: {\"id\":\"gen-1761900000-abcDEF123\",\"object\":\"chat.completion.chunk\",\"created\":1761900000,\"model\":\"google/lyria-3-clip-preview\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"[Intro] slow pads\"},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"gen-1761900000-abcDEF123\",\"object\":\"chat.completion.chunk\",\"created\":1761900000,\"model\":\"google/lyria-3-clip-preview\",\"choices\":[{\"index\":0,\"delta\":{\"audio\":{\"id\":\"audio_1\",\"data\":\"SUQzBAAAAAAA\"}},\"finish_reason\":null}]}\n\n",
    ": OPENROUTER PROCESSING\n\n",
    "data: {\"id\":\"gen-1761900000-abcDEF123\",\"object\":\"chat.completion.chunk\",\"created\":1761900000,\"model\":\"google/lyria-3-clip-preview\",\"choices\":[{\"index\":0,\"delta\":{\"audio\":{\"data\":\"//uQZAAA\",\"transcript\":\"\"}},\"finish_reason\":null}]}\n\n",
    "data: {\"id\":\"gen-1761900000-abcDEF123\",\"object\":\"chat.completion.chunk\",\"created\":1761900000,\"model\":\"google/lyria-3-clip-preview\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":42,\"completion_tokens\":0,\"total_tokens\":42,\"cost\":0.04}}\n\n",
    "data: [DONE]\n\n",
);

const MP3: [u8; 15] = [
    b'I', b'D', b'3', 4, 0, 0, 0, 0, 0, 0xFF, 0xFB, 0x90, 0x64, 0, 0,
];

/// A plain (non-streamed) answer: `choices[0].message.audio.data`.
fn plain() -> Value {
    json!({
        "id": "gen-1761900001-xyz",
        "model": "google/lyria-3-clip-preview",
        "choices": [{
            "index": 0,
            "finish_reason": "stop",
            "message": {
                "role": "assistant",
                "content": "[Verse] harbour lamps",
                "audio": { "id": "audio_2", "data": "SUQzBAAAAAAA//uQZAAA", "transcript": "", "expires_at": 1761903600 }
            }
        }],
        "usage": { "prompt_tokens": 40, "completion_tokens": 0, "cost": 0.04 }
    })
}

// --- A mock HTTP layer ---

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockState>>);

#[derive(Default)]
struct MockState {
    replies: VecDeque<Result<HttpReply, MindError>>,
    bodies: Vec<Value>,
    slept: Vec<Duration>,
    /// Generation stats lookups; each answers 503.
    gets: usize,
}

impl Mock {
    fn queue(&self, reply: Result<HttpReply, MindError>) {
        self.0.lock().unwrap().replies.push_back(reply);
    }
    fn stream(&self) {
        self.queue(Ok(HttpReply::new(200, STREAM)));
    }
    /// The stream without `usage.cost`.
    fn stream_without_cost(&self) {
        let text = STREAM.replace(",\"cost\":0.04", "");
        assert_ne!(text, STREAM);
        self.queue(Ok(HttpReply::new(200, text)));
    }
    fn gets(&self) -> usize {
        self.0.lock().unwrap().gets
    }
    fn status(&self, status: u16) {
        self.queue(Ok(HttpReply::new(
            status,
            json!({"error": {"code": status, "message": "nope"}}).to_string(),
        )));
    }
    fn calls(&self) -> usize {
        self.0.lock().unwrap().bodies.len()
    }
    fn bodies(&self) -> Vec<Value> {
        self.0.lock().unwrap().bodies.clone()
    }
    fn slept(&self) -> Vec<Duration> {
        self.0.lock().unwrap().slept.clone()
    }
}

impl Transport for Mock {
    fn post(&mut self, _url: &str, _auth: &str, body: &str) -> Result<HttpReply, MindError> {
        let mut s = self.0.lock().unwrap();
        s.bodies.push(serde_json::from_str(body).unwrap());
        s.replies
            .pop_front()
            .unwrap_or_else(|| Err(MindError::Transport("no reply queued".into())))
    }
    fn get(&mut self, _url: &str, _auth: &str) -> Result<HttpReply, MindError> {
        self.0.lock().unwrap().gets += 1;
        Ok(HttpReply::new(503, "{\"error\":{\"code\":503}}"))
    }
    fn sleep(&mut self, wait: Duration) {
        self.0.lock().unwrap().slept.push(wait);
    }
}

/// A temporary save folder with a connected call log.
struct Save {
    dir: PathBuf,
    log: CallSink,
}

impl Save {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "galaxy-music-{name}-{}-{}",
            std::process::id(),
            galaxy_mind::calllog::now_ms()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let log = CallSink::new();
        log.open(
            &galaxy_mind::calllog::path_for(&self_save(&dir)),
            &LogRules::default(),
        )
        .unwrap();
        Self { dir, log }
    }

    fn save(&self) -> PathBuf {
        self_save(&self.dir)
    }

    fn music(&self) -> PathBuf {
        music::dir_for(&self.save())
    }

    fn open(&self, rules: &MusicRules, mock: &Mock) -> MusicLibrary {
        MusicLibrary::open_with(
            self.music(),
            rules.clone(),
            Some((mock.clone(), ApiKey::new(KEY))),
            self.log.clone(),
        )
    }

    fn rows(&self) -> Vec<Value> {
        assert!(self.log.flush(WAIT));
        CallLog::open_read(&galaxy_mind::calllog::path_for(&self.save()))
            .unwrap()
            .rows()
            .unwrap()
    }
}

fn self_save(dir: &Path) -> PathBuf {
    dir.join("slot1.sqlite")
}

impl Drop for Save {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn rules() -> MusicRules {
    MusicRules {
        backoff_ms: 10,
        ..MusicRules::default()
    }
}

fn generated(lib: &MusicLibrary, channel: &str) -> PathBuf {
    match lib.track(channel) {
        TrackSource::Generated(p) => p,
        t => panic!("{channel} plays {t:?}"),
    }
}

fn is_placeholder(t: &TrackSource) -> bool {
    matches!(t, TrackSource::Placeholder(p) if *p == PLACEHOLDER_CALM || *p == PLACEHOLDER_DRIFT)
}

// --- The parser ---

#[test]
fn the_documented_stream_folds_and_parses() {
    let v = fold_stream(STREAM).expect("an event stream");
    assert_eq!(v["id"], "gen-1761900000-abcDEF123");
    assert_eq!(v["usage"]["cost"], 0.04);
    assert_eq!(v["choices"][0]["finish_reason"], "stop");
    assert_eq!(v["choices"][0]["message"]["role"], "assistant");
    let track = music::parse_track(&v).unwrap();
    assert_eq!(track.audio, MP3);
    assert_eq!(track.format, AudioFormat::Mp3);
    assert_eq!(track.text, "[Intro] slow pads");
}

#[test]
fn a_plain_answer_parses() {
    let track = music::parse_track(&plain()).unwrap();
    assert_eq!(track.audio, MP3);
    assert_eq!(track.text, "[Verse] harbour lamps");
    // An audio content part, as a data URL.
    let parts = json!({"choices": [{"message": {"content": [
        {"type": "text", "text": "hi"},
        {"type": "output_audio", "audio": {"data": "data:audio/mpeg;base64,SUQzBAAAAAAA//uQZAAA"}}
    ]}}]});
    assert_eq!(music::parse_track(&parts).unwrap().audio, MP3);
}

#[test]
fn answers_without_audio_are_rejected() {
    let text = json!({"choices": [{"message": {"content": "I can't make that."}}]});
    assert!(matches!(
        music::parse_track(&text),
        Err(MindError::Malformed(_))
    ));
    let junk = json!({"choices": [{"message": {"audio": {"data": "eyJlcnJvciI6MX0="}}}]});
    let e = music::parse_track(&junk).unwrap_err();
    assert!(e.to_string().contains("audio format"), "{e}");
}

/// The client folds the stream: cost from the last chunk, the generation ID, the model,
/// and a log row whose output is the text, not the audio.
#[test]
fn the_client_reads_a_streamed_answer() {
    let save = Save::new("client");
    let mock = Mock::default();
    mock.stream();
    let mut client = Client::new(mock.clone(), ApiKey::new(KEY)).with_log(save.log.clone());
    let body = music::music_body(&rules(), "radio.kettle_dock", "calm");
    let answer = client.call(&Call {
        purpose: galaxy_mind::mind::Purpose::Music,
        model: music::CLIP_MODEL,
        path: galaxy_mind::openrouter::CHAT_PATH,
        body: &body,
        session_id: Some("radio.kettle_dock"),
        max_attempts: 1,
        deadline_ms: 60_000,
        backoff_ms: 0,
        metadata: false,
    });
    assert_eq!(answer.usage.cost_usd, Some(0.04));
    assert_eq!(
        answer.generation_id.as_deref(),
        Some("gen-1761900000-abcDEF123")
    );
    assert_eq!(answer.model, music::CLIP_MODEL);
    assert_eq!(answer.content(), Some("[Intro] slow pads"));
    let rows = save.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["purpose"], "music");
    assert_eq!(rows[0]["output"], "[Intro] slow pads");
}

// --- The library ---

#[test]
fn tracks_are_generated_cached_and_logged() {
    let save = Save::new("cache");
    let mock = Mock::default();
    mock.stream();
    mock.stream();
    let lib = save.open(&rules(), &mock);
    assert!(is_placeholder(&lib.track("radio.kettle_dock")));
    lib.want("radio.kettle_dock", "warm greenhouse lamps");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 2, "two tracks per channel");
    let body = &mock.bodies()[0];
    assert_eq!(body["model"], music::CLIP_MODEL);
    assert_eq!(body["session_id"], "radio.kettle_dock");
    assert_eq!(body["modalities"], json!(["text", "audio"]));
    let prompt = body["messages"][0]["content"].as_str().unwrap();
    assert!(prompt.contains("warm greenhouse lamps") && prompt.contains("no vocals"));

    let dir = save.music().join("radio.kettle_dock");
    assert_eq!(std::fs::read(dir.join("0.mp3")).unwrap(), MP3);
    assert_eq!(std::fs::read(dir.join("1.mp3")).unwrap(), MP3);
    let path = generated(&lib, "radio.kettle_dock");
    assert!(path.starts_with(&dir));
    assert_eq!(
        lib.track("radio.kettle_dock"),
        TrackSource::Generated(path),
        "stable"
    );
    assert_eq!(lib.tracks("radio.kettle_dock").len(), 2);

    let index = lib.index();
    assert_eq!(index.tracks.len(), 2);
    assert_eq!(index.tracks[0].cost_usd, Some(0.04));
    assert_eq!(index.tracks[0].expected_usd, Some(0.04));
    assert_eq!(
        index.tracks[0].generation_id.as_deref(),
        Some("gen-1761900000-abcDEF123")
    );
    assert!((index.spent_usd - 0.08).abs() < 1e-9);
    let text = std::fs::read_to_string(save.music().join(music::INDEX_FILE)).unwrap();
    assert!(!text.contains(KEY), "no key in the index");

    let rows = save.rows();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row["purpose"], "music");
        assert_eq!(row["cost_usd"], 0.04);
        assert_eq!(row["verdict"], "accepted");
    }
}

/// A restart plays the cache and makes no call for a channel that has its tracks.
#[test]
fn a_cached_channel_makes_no_call_after_a_restart() {
    let save = Save::new("restart");
    let first = Mock::default();
    first.stream();
    first.stream();
    let lib = save.open(&rules(), &first);
    lib.want("radio.lantern_wharf", "dry and warm");
    assert!(lib.idle(WAIT));
    lib.shutdown();
    drop(lib);

    let again = Mock::default();
    let lib = save.open(&rules(), &again);
    let before = generated(&lib, "radio.lantern_wharf");
    lib.want("radio.lantern_wharf", "a different mood now");
    assert!(lib.idle(WAIT));
    assert_eq!(again.calls(), 0, "a cached track is never generated again");
    assert_eq!(lib.status().started, 0);
    assert!(before.is_file());
}

/// A channel with one track cached fills only the free slot; the cached file stays.
#[test]
fn only_free_slots_are_filled() {
    let save = Save::new("slots");
    let first = Mock::default();
    first.stream();
    let lib = save.open(
        &MusicRules {
            tracks_per_channel: 1,
            ..rules()
        },
        &first,
    );
    lib.want("radio.cinder_hook", "dusty");
    assert!(lib.idle(WAIT));
    drop(lib);
    let slot0 = save.music().join("radio.cinder_hook").join("0.mp3");
    std::fs::write(&slot0, b"ID3 the first one").unwrap();
    // The index is lost: the file on disk still counts.
    std::fs::remove_file(save.music().join(music::INDEX_FILE)).unwrap();

    let second = Mock::default();
    second.stream();
    let lib = save.open(&rules(), &second);
    lib.want("radio.cinder_hook", "dusty");
    assert!(lib.idle(WAIT));
    assert_eq!(second.calls(), 1);
    assert_eq!(std::fs::read(&slot0).unwrap(), b"ID3 the first one");
    assert!(
        save.music()
            .join("radio.cinder_hook")
            .join("1.mp3")
            .is_file()
    );
}

#[test]
fn the_run_cap_stops_generation() {
    let save = Save::new("cap");
    let mock = Mock::default();
    for _ in 0..4 {
        mock.stream();
    }
    let lib = save.open(
        &MusicRules {
            tracks_per_channel: 3,
            run_generations: 2,
            ..rules()
        },
        &mock,
    );
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    lib.want("radio.b", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 2);
    assert_eq!(lib.status().stopped, Some(Stopped::RunCap));
    assert_eq!(lib.tracks("radio.a").len(), 2);
    assert!(is_placeholder(&lib.track("radio.b")));
}

#[test]
fn the_spend_cap_counts_every_run() {
    let save = Save::new("spend");
    let mock = Mock::default();
    mock.stream();
    let capped = MusicRules {
        spend_cap_usd: 0.04,
        ..rules()
    };
    let lib = save.open(&capped, &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    drop(lib);
    let lib = save.open(&capped, &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 1);
    assert_eq!(lib.status().stopped, Some(Stopped::SpendCap));
}

/// Six launches share one cache, each asking for two tracks for a new channel. Ten
/// $0.04 clips fill the default $0.40 cap exactly (a float sum of them falls just
/// short), so an eleventh never starts.
fn six_launches(save: &Save, mock: &Mock) -> MusicLibrary {
    let mut last = None;
    for run in 0..6 {
        let lib = save.open(&rules(), mock);
        lib.want(&format!("radio.run{run}"), "calm");
        assert!(lib.idle(WAIT));
        lib.shutdown();
        last = Some(lib);
    }
    last.unwrap()
}

fn assert_capped_at_ten_clips(lib: &MusicLibrary, mock: &Mock) {
    assert_eq!(mock.calls(), 10, "ten clips, never an eleventh");
    assert_eq!(lib.status().stopped, Some(Stopped::SpendCap));
    let spent = lib.index().spent_usd;
    assert!(spent <= 0.40 && spent > 0.40 - 1e-9, "spent {spent}");
}

#[test]
fn the_default_cap_buys_ten_clips_not_eleven() {
    let save = Save::new("ten");
    let mock = Mock::default();
    for _ in 0..12 {
        mock.stream();
    }
    let lib = six_launches(&save, &mock);
    assert_capped_at_ten_clips(&lib, &mock);
}

/// Answers without `usage.cost`, and generation stats that answer 503: each call counts
/// its listed price, while the index and the log keep the cost as unreported.
#[test]
fn an_unreported_cost_counts_the_listed_price() {
    let save = Save::new("unreported");
    let mock = Mock::default();
    for _ in 0..12 {
        mock.stream_without_cost();
    }
    let lib = six_launches(&save, &mock);
    assert!(mock.gets() > 0, "the stats were asked");
    assert_capped_at_ten_clips(&lib, &mock);
    let index = lib.index();
    assert_eq!(index.tracks.len(), 10);
    assert!(index.tracks.iter().all(|t| t.cost_usd.is_none()));
    let rows = save.rows();
    assert_eq!(rows.len(), 10);
    assert!(rows.iter().all(|r| r["cost_usd"].is_null()), "{rows:?}");
}

/// A call still out counts against the cap: another run on the same cache, opened while
/// it is in flight, finds no room under a one-clip cap and makes no call.
#[test]
fn a_call_in_flight_counts_against_the_cap() {
    #[derive(Clone, Default)]
    struct Held(Arc<(Mutex<(bool, bool)>, Condvar)>);
    impl Transport for Held {
        fn post(&mut self, _: &str, _: &str, _: &str) -> Result<HttpReply, MindError> {
            let (m, c) = &*self.0;
            let mut s = m.lock().unwrap();
            s.0 = true;
            c.notify_all();
            while !s.1 {
                s = c.wait(s).unwrap();
            }
            Ok(HttpReply::new(200, STREAM))
        }
    }
    impl Held {
        fn wait_started(&self) {
            let (m, c) = &*self.0;
            let mut s = m.lock().unwrap();
            while !s.0 {
                s = c.wait(s).unwrap();
            }
        }
        fn release(&self) {
            let (m, c) = &*self.0;
            m.lock().unwrap().1 = true;
            c.notify_all();
        }
    }
    let save = Save::new("in-flight");
    let capped = MusicRules {
        spend_cap_usd: 0.04,
        tracks_per_channel: 1,
        ..rules()
    };
    let held = Held::default();
    let first = MusicLibrary::open_with(
        save.music(),
        capped.clone(),
        Some((held.clone(), ApiKey::new(KEY))),
        save.log.clone(),
    );
    first.want("radio.a", "calm");
    held.wait_started();

    let mock = Mock::default();
    mock.stream();
    let second = save.open(&capped, &mock);
    second.want("radio.b", "calm");
    let idle = second.idle(WAIT);
    let calls = mock.calls();
    let stopped = second.status().stopped;
    held.release();
    assert!(idle && first.idle(WAIT));
    assert_eq!(calls, 0, "the call in flight leaves no room");
    assert_eq!(stopped, Some(Stopped::SpendCap));
    assert_eq!(first.tracks("radio.a").len(), 1);
    assert!((first.index().spent_usd - 0.04).abs() < 1e-12);
}

/// An index written before spend was kept in whole nano-dollars still loads, and its
/// float sum counts in full.
#[test]
fn an_older_index_keeps_its_spend() {
    let save = Save::new("older");
    std::fs::create_dir_all(save.music()).unwrap();
    std::fs::write(
        save.music().join(music::INDEX_FILE),
        json!({"version": 1, "runs": 6, "spent_usd": 0.43999999999999995,
               "generations": 11, "tracks": []})
        .to_string(),
    )
    .unwrap();
    let mock = Mock::default();
    mock.stream();
    let lib = save.open(&rules(), &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 0);
    assert_eq!(lib.status().stopped, Some(Stopped::SpendCap));
    let index = lib.index();
    assert_eq!(index.runs, 7);
    assert_eq!(index.spent_nanos, 440_000_000);
}

/// A model without a listed price can't be bounded, so it generates nothing.
#[test]
fn a_model_without_a_price_generates_nothing() {
    let save = Save::new("unpriced");
    let mock = Mock::default();
    mock.stream();
    let lib = save.open(
        &MusicRules {
            model: "google/lyria-4-preview".into(),
            ..rules()
        },
        &mock,
    );
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 0);
    assert_eq!(lib.status().stopped, Some(Stopped::Unpriced));
    assert!(is_placeholder(&lib.track("radio.a")));
}

/// HTTP 402 (out of credit) stops music for the rest of the run, without retries.
#[test]
fn a_402_stops_music_for_the_run() {
    let save = Save::new("credit");
    let mock = Mock::default();
    mock.status(402);
    let lib = save.open(&rules(), &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 1, "a 402 is not retried");
    assert_eq!(lib.status().stopped, Some(Stopped::NoCredit));
    lib.want("radio.b", "calm");
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 1);
    assert!(is_placeholder(&lib.track("radio.a")));
    let rows = save.rows();
    assert_eq!(rows[0]["verdict"], "failed");
    assert_eq!(rows[0]["http_status"], 402);
    assert_eq!(lib.index().spent_usd, 0.0, "a refusal bills nothing");
}

/// The same, reported inside a 200 stream.
#[test]
fn a_402_inside_a_stream_stops_music_too() {
    let save = Save::new("credit-stream");
    let mock = Mock::default();
    mock.queue(Ok(HttpReply::new(
        200,
        ": OPENROUTER PROCESSING\n\ndata: {\"id\":\"gen-1-a\",\"error\":{\"code\":402,\"message\":\"Insufficient credits\"},\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"error\"}]}\n\n",
    )));
    let lib = save.open(&rules(), &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 1);
    assert_eq!(lib.status().stopped, Some(Stopped::NoCredit));
}

/// A flaky provider: retried with a doubling backoff, then the channel gives up for the
/// run and keeps its placeholder; other channels may still try.
#[test]
fn failures_retry_with_backoff_then_give_up() {
    let save = Save::new("retry");
    let mock = Mock::default();
    for _ in 0..3 {
        mock.status(503);
    }
    let lib = save.open(&rules(), &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(mock.calls(), 3);
    assert_eq!(
        mock.slept(),
        vec![Duration::from_millis(10), Duration::from_millis(20)]
    );
    let status = lib.status();
    assert_eq!(status.stopped, None);
    assert_eq!(status.failed, vec!["radio.a".to_owned()]);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert_eq!(
        mock.calls(),
        3,
        "a channel that gave up waits for the next run"
    );
    assert!(is_placeholder(&lib.track("radio.a")));
    let rows = save.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["attempts"], 3);
    // A 5xx without a cost may still bill: each attempt counts the listed price.
    assert!((lib.index().spent_usd - 0.12).abs() < 1e-12);
    assert_eq!(rows[0]["verdict"], "failed");
}

/// An answer without audio is rejected in the log; nothing is cached.
#[test]
fn an_answer_without_audio_is_rejected() {
    let save = Save::new("reject");
    let mock = Mock::default();
    mock.queue(Ok(HttpReply::new(
        200,
        json!({"id": "gen-2-b", "model": music::CLIP_MODEL,
               "choices": [{"message": {"content": "Sorry."}}], "usage": {"cost": 0.0}})
        .to_string(),
    )));
    let lib = save.open(&rules(), &mock);
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert!(is_placeholder(&lib.track("radio.a")));
    assert!(lib.index().tracks.is_empty());
    let rows = save.rows();
    assert_eq!(rows[0]["verdict"], "rejected");
    assert!(rows[0]["reason"].as_str().unwrap().contains("no audio"));
}

#[test]
fn without_a_key_channels_play_placeholders() {
    let save = Save::new("nokey");
    let lib = MusicLibrary::open_with::<Mock>(save.music(), rules(), None, save.log.clone());
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    assert!(is_placeholder(&lib.track("radio.a")));
    assert_eq!(lib.status().stopped, Some(Stopped::NoKey));
    assert!(save.rows().is_empty());
    assert!(!save.music().exists(), "nothing is written without a key");

    let headless = MusicLibrary::placeholders();
    headless.want("radio.a", "calm");
    assert!(is_placeholder(&headless.track("radio.a")));
}

/// Without a key, tracks cached in an earlier run still play.
#[test]
fn without_a_key_the_cache_still_plays() {
    let save = Save::new("nokey-cache");
    let mock = Mock::default();
    mock.stream();
    let lib = save.open(
        &MusicRules {
            tracks_per_channel: 1,
            ..rules()
        },
        &mock,
    );
    lib.want("radio.a", "calm");
    assert!(lib.idle(WAIT));
    drop(lib);
    let lib = MusicLibrary::open_with::<Mock>(save.music(), rules(), None, save.log.clone());
    assert!(generated(&lib, "radio.a").is_file());
}

/// A call still out at shutdown is booked at the expected price.
#[test]
fn shutdown_books_a_call_still_out() {
    #[derive(Clone)]
    struct Stuck(Arc<Mutex<bool>>);
    impl Transport for Stuck {
        fn post(&mut self, _: &str, _: &str, _: &str) -> Result<HttpReply, MindError> {
            *self.0.lock().unwrap() = true;
            std::thread::sleep(Duration::from_secs(3));
            Err(MindError::Timeout)
        }
    }
    let save = Save::new("shutdown");
    let stuck = Stuck(Arc::new(Mutex::new(false)));
    let lib = MusicLibrary::open_with(
        save.music(),
        MusicRules {
            max_attempts: 1,
            ..rules()
        },
        Some((stuck.clone(), ApiKey::new(KEY))),
        save.log.clone(),
    );
    lib.want("radio.a", "calm");
    while !*stuck.0.lock().unwrap() {
        std::thread::sleep(Duration::from_millis(5));
    }
    lib.shutdown();
    let rows = save.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["cost_usd"], 0.04);
    assert_eq!(rows[0]["cost_source"], "expected");
    assert_eq!(rows[0]["error_kind"], "cancelled");
    assert!((lib.index().spent_usd - 0.04).abs() < 1e-9);
    assert_eq!(lib.status().stopped, Some(Stopped::Shutdown));
}

#[test]
fn the_core_pack_music_rules_load() {
    let config = galaxy_mind::MindConfig::load_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data"),
    )
    .unwrap();
    let m = &config.music;
    assert_eq!(m.model, music::CLIP_MODEL);
    assert_eq!(m.tracks_per_channel, 2);
    assert_eq!(m.run_generations, 2);
    assert!(m.instrumental);
    assert_eq!(m.expected(), Some(0.04));
}
