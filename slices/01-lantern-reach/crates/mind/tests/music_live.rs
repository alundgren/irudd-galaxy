//! One live clip from Lyria 3 through OpenRouter, the way the game asks for one.
//! Ignored by default; it runs only with `OPENROUTER_API_KEY` set, costs about $0.04,
//! and makes one generation (retries within its backoff included):
//!
//! ```sh
//! cargo test -p galaxy-mind --test music_live -- --ignored --nocapture
//! ```
//!
//! It prints the response's shape (base64 elided), the index entry and the call log
//! row. `GALAXY_MUSIC_LIVE_DIR` keeps the save folder there to listen to the clip.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use galaxy_mind::calllog::{CallLog, CallSink, LogRules};
use galaxy_mind::music::{self, MusicLibrary, MusicRules, TrackSource};
use galaxy_mind::openrouter::elide_blobs;
use galaxy_mind::{ApiKey, HttpReply, MindError, Transport, UreqTransport};
use serde_json::Value;

/// The real transport, keeping each response's status and a copy of its body (the key
/// redacted, should a response echo it).
struct Tap {
    inner: UreqTransport,
    key: ApiKey,
    seen: Arc<Mutex<Vec<(u16, String)>>>,
}

impl Transport for Tap {
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
        let r = self.inner.post_with(url, auth, headers, body)?;
        let body = self.key.redact(&r.body);
        self.seen.lock().unwrap().push((r.status, body));
        Ok(r)
    }
    fn get(&mut self, url: &str, auth: &str) -> Result<HttpReply, MindError> {
        self.inner.get(url, auth)
    }
}

/// The body's shape: each JSON event with long strings elided, the first few and last
/// few of a stream.
fn shape(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        return serde_json::to_string_pretty(&elide_blobs(&v)).unwrap();
    }
    let lines: Vec<String> = body
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match l.strip_prefix("data:") {
            Some(d) => match serde_json::from_str::<Value>(d.trim()) {
                Ok(v) => format!("data: {}", elide_blobs(&v)),
                Err(_) => l.to_owned(),
            },
            None => l.to_owned(),
        })
        .collect();
    let n = lines.len();
    let mut out = format!("{n} non-empty lines, {} bytes\n", body.len());
    for (i, l) in lines.iter().enumerate() {
        if i < 6 || i + 4 >= n {
            out.push_str(l);
            out.push('\n');
        } else if i == 6 {
            out.push_str("...\n");
        }
    }
    out
}

#[test]
#[ignore = "calls OpenRouter (about $0.04); needs OPENROUTER_API_KEY"]
fn live_lyria_clip_is_generated_and_cached() {
    let Ok(key) = ApiKey::from_env() else {
        eprintln!("skipped: {} is not set", ApiKey::ENV);
        return;
    };
    let dir = std::env::var_os("GALAXY_MUSIC_LIVE_DIR")
        .map(Into::into)
        .unwrap_or_else(|| std::env::temp_dir().join("galaxy-music-live"));
    std::fs::create_dir_all(&dir).unwrap();
    let save = dir.join("live.sqlite");
    let log_path = galaxy_mind::calllog::path_for(&save);
    let log = CallSink::new();
    log.open(&log_path, &LogRules::default()).unwrap();

    // The shipped rules, held to one clip.
    let shipped = galaxy_mind::MindConfig::load_dir(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data"),
    )
    .unwrap()
    .music;
    let rules = MusicRules {
        model: music::CLIP_MODEL.into(),
        tracks_per_channel: 1,
        run_generations: 1,
        ..shipped
    };
    let seen = Arc::new(Mutex::new(Vec::new()));
    let tap = Tap {
        inner: UreqTransport::new(Duration::from_millis(rules.timeout_ms)),
        key: key.clone(),
        seen: Arc::clone(&seen),
    };
    let lib = MusicLibrary::open_with(music::dir_for(&save), rules, Some((tap, key)), log.clone());
    let channel = "station.kettle_dock";
    lib.want(
        channel,
        "soft greenhouse lamps, warm and patient, water ticking in the ice tanks",
    );
    let started = std::time::Instant::now();
    assert!(
        lib.idle(Duration::from_secs(15 * 60)),
        "no answer in 15 minutes"
    );
    eprintln!("took {:.1} s", started.elapsed().as_secs_f64());

    for (status, body) in seen.lock().unwrap().iter() {
        eprintln!("--- HTTP {status}\n{}", shape(body));
    }
    eprintln!("--- status\n{:?}", lib.status());
    eprintln!(
        "--- index\n{}",
        serde_json::to_string_pretty(&lib.index()).unwrap()
    );
    assert!(log.flush(Duration::from_secs(5)));
    let rows = CallLog::open_read(&log_path).unwrap().rows().unwrap();
    for row in &rows {
        let mut row = row.clone();
        row["prompt"] = Value::String("(elided)".into());
        eprintln!(
            "--- log row\n{}",
            serde_json::to_string_pretty(&row).unwrap()
        );
    }
    assert_eq!(rows.len(), 1, "one call, logged");
    match lib.track(channel) {
        TrackSource::Generated(p) => {
            let bytes = std::fs::metadata(&p).unwrap().len();
            eprintln!("--- cached {} ({bytes} bytes)", p.display());
        }
        t => {
            eprintln!("--- no track: {t:?}");
            assert!(
                rows[0]["verdict"] != "pending",
                "a failure is logged with its verdict"
            );
        }
    }
}
