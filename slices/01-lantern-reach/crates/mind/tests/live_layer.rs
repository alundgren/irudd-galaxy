//! The live world layer and the call log end to end, headless and offline: a real sim
//! on the core pack's data, the host, `OpenRouterMind` over a mock HTTP layer, and a
//! call log in a temporary file. Only the `#[ignore]` test at the end calls OpenRouter.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use galaxy_contract::parts::Ribbon;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::Sim;
use galaxy_mind::calllog::{CallLog, CallSink, Group, LogRules, Range};
use galaxy_mind::config::{Limits, Models};
use galaxy_mind::live::{self, LiveNote};
use galaxy_mind::mind::{LiveDraft, LiveRequest, Purpose};
use galaxy_mind::situation::{SpendNote, Stage};
use galaxy_mind::{
    ApiKey, Decision, DecisionRequest, HttpReply, Inline, LineDraft, LineRequest, Mind, MindConfig,
    MindError, MindHost, OpenRouterMind, RecordedMind, Recording, Reply, Transport,
};
use serde_json::{Value, json};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
/// The Wharf sells rations (and buys ice).
const RATIONS: WareId = WareId::from_static("ware.rations");
const KEY: &str = "sk-or-test-key-0123456789";

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// The shipped config with the given rotation, no cooldown.
fn config(rotation: &[Purpose]) -> MindConfig {
    let mut c = MindConfig::load_dir(&data_dir()).expect("core mind.toml loads");
    c.live.rotation = rotation.to_vec();
    c.live.cooldown_dockings = 0;
    c
}

fn sim() -> Sim {
    let state = galaxy_core::new_game(&data_dir(), 7).expect("core data loads");
    Sim::new(state)
}

// --- A mock HTTP layer that keeps what it was sent ---

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockState>>);

/// A POST as sent: URL, extra headers, body.
type Post = (String, Vec<(String, String)>, Value);

#[derive(Default)]
struct MockState {
    replies: VecDeque<Result<HttpReply, MindError>>,
    gets: VecDeque<HttpReply>,
    posts: Vec<Post>,
    slept: Vec<Duration>,
}

impl Mock {
    fn queue(&self, r: Result<HttpReply, MindError>) {
        self.0.lock().unwrap().replies.push_back(r);
    }
    fn ok(&self, body: Value) {
        self.queue(Ok(HttpReply::new(200, body.to_string())));
    }
    fn get(&self, body: Value) {
        self.0
            .lock()
            .unwrap()
            .gets
            .push_back(HttpReply::new(200, body.to_string()));
    }
    fn posts(&self) -> Vec<Post> {
        self.0.lock().unwrap().posts.clone()
    }
    fn slept(&self) -> Vec<Duration> {
        self.0.lock().unwrap().slept.clone()
    }
}

impl Transport for Mock {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError> {
        self.post_with(url, auth, &[], body)
    }
    fn post_with(
        &mut self,
        url: &str,
        _auth: &str,
        headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpReply, MindError> {
        let mut s = self.0.lock().unwrap();
        s.posts.push((
            url.to_owned(),
            headers
                .iter()
                .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
                .collect(),
            serde_json::from_str(body).unwrap(),
        ));
        s.replies
            .pop_front()
            .unwrap_or_else(|| Err(MindError::Transport("no reply queued".into())))
    }
    fn get(&mut self, _url: &str, _auth: &str) -> Result<HttpReply, MindError> {
        self.0
            .lock()
            .unwrap()
            .gets
            .pop_front()
            .ok_or_else(|| MindError::Transport("no reply queued".into()))
    }
    fn sleep(&mut self, wait: Duration) {
        self.0.lock().unwrap().slept.push(wait);
    }
}

/// A chat completion as the auto router returns it, with `content` as the message.
fn completion(content: &str) -> Value {
    json!({
        "id": "gen-1761900000-abcdef",
        "model": "mistralai/ministral-8b",
        "choices": [{ "message": { "role": "assistant", "content": content } }],
        "usage": { "prompt_tokens": 812, "completion_tokens": 41, "cost": 0.000037 },
        "openrouter_metadata": {
            "requested": "openrouter/auto",
            "strategy": "auto",
            "pipeline": [
                { "type": "plugin", "name": "auto-router", "data": { "task_type": "creative:dialogue" } }
            ]
        }
    })
}

/// A temporary call log, its sink connected.
struct TempLog {
    dir: PathBuf,
    path: PathBuf,
    sink: CallSink,
}

impl TempLog {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "galaxy-live-{name}-{}-{}",
            std::process::id(),
            galaxy_mind::calllog::now_ms()
        ));
        let path = dir.join("save.llm-log.sqlite");
        let sink = CallSink::new();
        sink.open(&path, &LogRules::default()).unwrap();
        Self { dir, path, sink }
    }

    fn rows(&self) -> Vec<Value> {
        assert!(self.sink.flush(Duration::from_secs(5)));
        CallLog::open_read(&self.path).unwrap().rows().unwrap()
    }

    fn log(&self) -> CallLog {
        assert!(self.sink.flush(Duration::from_secs(5)));
        CallLog::open_read(&self.path).unwrap()
    }
}

impl Drop for TempLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn host(sim: &Sim, config: MindConfig, mock: &Mock, log: &CallSink) -> MindHost {
    let mind = OpenRouterMind::with_transport(
        mock.clone(),
        ApiKey::new(KEY),
        Models::default(),
        &Limits::default(),
    )
    .with_live(config.live.clone())
    .with_log(log.clone());
    let mut host = MindHost::new(config, Inline::new(mind), sim.history());
    host.set_live(true).set_call_log(log.clone());
    host
}

/// The player docks at `station`, then the sim and host run a few ticks: the request
/// goes out, its batch applies, and the line it causes is said.
fn dock(sim: &mut Sim, host: &mut MindHost, station: &StationId) {
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock"))
            .with(Command::SetShipDocked {
                ship: PLAYER,
                station: Some(station.clone()),
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: station.clone(),
                },
            }),
    );
    run(sim, host, 4);
}

fn run(sim: &mut Sim, host: &mut MindHost, ticks: usize) {
    for _ in 0..ticks {
        let report = sim.step(&PlayerInput::default()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        host.pump(sim);
    }
}

fn facts<'a>(sim: &'a Sim, kind: &str) -> Vec<&'a Fact> {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == kind)
        .collect()
}

fn live_facts(sim: &Sim) -> usize {
    live::LIVE_FACTS.iter().map(|k| facts(sim, k).len()).sum()
}

fn said_citing(sim: &Sim, fact: FactId) -> Vec<galaxy_contract::talk::TalkLine> {
    sim.world()
        .talk
        .lines
        .values()
        .filter(|l| l.line.cites.contains(&fact))
        .map(|l| l.line.clone())
        .collect()
}

fn base(sim: &Sim, station: &StationId, ware: &WareId) -> u32 {
    sim.world().markets.get(station).unwrap().lines[ware].base_price
}

#[test]
fn accepted_banter_goes_into_station_history_and_is_said_by_the_keeper() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("banter");
    mock.ok(completion(
        r#"{"speaker":"npc.orrin_vale","text":"I polished the cradle rails twice today, so mind your landing."}"#,
    ));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);

    // The request: the auto router at the low tier, the station's session, metadata on.
    let posts = mock.posts();
    assert_eq!(posts.len(), 1);
    let (url, headers, body) = &posts[0];
    assert!(url.ends_with("/api/v1/chat/completions"));
    assert!(headers.contains(&("X-OpenRouter-Metadata".into(), "enabled".into())));
    assert_eq!(body["model"], "openrouter/auto");
    assert_eq!(
        body["plugins"],
        json!([{ "id": "auto-router", "cost_tier": "low" }])
    );
    assert_eq!(body["session_id"], "station.lantern_wharf");
    let schema = &body["response_format"]["json_schema"]["schema"];
    assert_eq!(
        schema["properties"]["speaker"]["enum"],
        json!(["npc.ilse_marrow", "npc.orrin_vale"]),
        "only this station's keepers"
    );
    assert!(!body.to_string().contains(KEY));

    // Station history: one fact with Cause::Mind, said once by the keeper, citing it.
    let banter = facts(&sim, live::BANTER_FACT);
    assert_eq!(banter.len(), 1);
    let fact = banter[0];
    assert_eq!(fact.subjects.station, Some(WHARF));
    let cause = sim.history().cause(fact.cause).unwrap();
    assert!(
        matches!(&cause.cause.kind, CauseKind::Mind { npc: Some(n), decision }
        if n.as_str() == "npc.orrin_vale" && decision.starts_with("live.banter"))
    );
    let note = LiveNote::parse(fact).unwrap();
    assert_eq!(note.model, "mistralai/ministral-8b");
    let lines = said_citing(&sim, fact.id);
    assert_eq!(lines.len(), 1);
    assert_eq!(lines[0].channel, Channel::DockTalk);
    assert_eq!(
        lines[0].speaker.as_ref().unwrap().as_str(),
        "npc.orrin_vale"
    );
    assert!(lines[0].text.starts_with("I polished"));

    // The log: one row with the model, task type, tokens, cost and the verdict.
    let rows = log.rows();
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["purpose"], "banter");
    assert_eq!(r["requested_model"], "openrouter/auto");
    assert_eq!(r["actual_model"], "mistralai/ministral-8b");
    assert_eq!(r["task_type"], "creative:dialogue");
    assert_eq!(r["session_id"], "station.lantern_wharf");
    assert_eq!(r["prompt_tokens"], 812);
    assert_eq!(r["completion_tokens"], 41);
    assert_eq!(r["cost_source"], "usage");
    assert!((r["cost_usd"].as_f64().unwrap() - 0.000037).abs() < 1e-12);
    assert_eq!(r["verdict"], "accepted");
    assert!(r["prompt"].as_str().unwrap().contains("auto-router"));
    assert!(r["output"].as_str().unwrap().contains("cradle rails"));
    // And the spend is booked in the save's ledger like any other call.
    let spends: Vec<SpendNote> = facts(&sim, "mind.spend")
        .into_iter()
        .filter_map(SpendNote::parse)
        .collect();
    assert_eq!(spends.len(), 1);
    assert_eq!(spends[0].stage, Stage::Live);
    assert_eq!(spends[0].menu, "live.banter");
}

#[test]
fn an_id_that_was_not_offered_is_rejected_and_logged() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("reject");
    // Saffron keeps Brassmouth's market, not a part at the Wharf.
    mock.ok(completion(
        r#"{"speaker":"npc.saffron_vey","text":"I keep the ledger honest here."}"#,
    ));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(live_facts(&sim), 0, "nothing reaches the world");
    let rows = log.rows();
    assert_eq!(rows[0]["verdict"], "rejected");
    assert!(
        rows[0]["reason"]
            .as_str()
            .unwrap()
            .contains("not one of the IDs offered"),
        "{}",
        rows[0]["reason"]
    );
    let rej = log.log().recent_rejections(5).unwrap();
    assert_eq!(rej.len(), 1);
    assert_eq!(rej[0]["purpose"], "banter");
}

#[test]
fn lines_that_break_the_rules_are_rejected() {
    for (text, why) in [
        ("The cradle is free today.", "first person"),
        ("I sold forty crates at 12 credits each.", "digits"),
        ("I read about it on www.example.org today.", "out-of-world"),
        ("I met Captain Zorbulon by the lamps.", "outside the world"),
        ("I dreamt of the Ancients last night.", "forbidden"),
        ("I saw it.\nThen I left.", "more than one line"),
    ] {
        let mut sim = sim();
        let mock = Mock::default();
        let log = TempLog::new("rules");
        mock.ok(completion(
            &json!({ "speaker": "npc.ilse_marrow", "text": text }).to_string(),
        ));
        let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
        dock(&mut sim, &mut host, &WHARF);
        assert_eq!(live_facts(&sim), 0, "{text}");
        let rows = log.rows();
        assert_eq!(rows[0]["verdict"], "rejected", "{text}");
        assert!(
            rows[0]["reason"].as_str().unwrap().contains(why),
            "{text}: {}",
            rows[0]["reason"]
        );
    }
}

#[test]
fn an_out_of_bounds_discount_is_clamped_shown_and_ends_after_its_dockings() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("discount");
    let before = base(&sim, &WHARF, &RATIONS);
    let credits = sim.world().player.credits;
    let cargo = sim.world().ships.get(&PLAYER).unwrap().hold.clone();
    mock.ok(completion(
        r#"{"ware":"ware.rations","percent":40,"dockings":9,"text":"We baked more than the crews can eat, so I'm letting it go cheap."}"#,
    ));
    let cfg = config(&[Purpose::Discount]);
    let (max_p, max_d) = (
        cfg.live.discount.max_percent,
        cfg.live.discount.max_dockings,
    );
    let mut host = host(&sim, cfg, &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);

    let d = facts(&sim, live::DISCOUNT_FACT);
    assert_eq!(d.len(), 1);
    let note = LiveNote::parse(d[0]).unwrap();
    assert_eq!((note.percent, note.dockings), (Some(max_p), Some(max_d)));
    assert!(note.clamped);
    let after = base(&sim, &WHARF, &RATIONS);
    let want = (f64::from(before) * (1.0 - f64::from(max_p) / 100.0)).round() as u32;
    assert_eq!(after, want, "the base moved by the clamped percent");
    assert_eq!(facts(&sim, "market_base_changed").len(), 1);
    // The player's own stuff is untouched.
    assert_eq!(sim.world().player.credits, credits);
    assert_eq!(sim.world().ships.get(&PLAYER).unwrap().hold, cargo);
    // A readable notice and the keeper's line, both citing the discount.
    let lines = said_citing(&sim, d[0].id);
    let notice = lines
        .iter()
        .find(|l| l.channel == Channel::MapNote)
        .unwrap();
    assert_eq!(
        notice.text,
        format!("Lantern Wharf: {max_p}% off rations for {max_d} dockings")
    );
    assert!(lines.iter().any(|l| {
        l.channel == Channel::DockTalk
            && l.speaker
                .as_ref()
                .is_some_and(|s| s.as_str() == "npc.ilse_marrow")
    }));
    // The market's key icon carries a "Changed" ribbon.
    let market = sim
        .world()
        .station_parts
        .values()
        .find(|p| p.station == WHARF && p.room == galaxy_contract::parts::RoomKind::Market)
        .unwrap();
    assert_eq!(market.news.map(|n| n.ribbon), Some(Ribbon::Changed));
    let rows = log.rows();
    assert_eq!(rows[0]["verdict"], "accepted");
    assert_eq!(rows[0]["reason"], "clamped into the discount bound");

    // No second discount while one runs, and the rest of the dockings ask for nothing
    // (no replies queued: a call would fail and be logged).
    let mut ended = false;
    for i in 0..max_d {
        let at = if i % 2 == 0 { KETTLE } else { WHARF };
        dock(&mut sim, &mut host, &at);
        if !facts(&sim, live::DISCOUNT_ENDED_FACT).is_empty() {
            ended = true;
            assert_eq!(i + 1, max_d, "ends after its dockings, not before");
        }
    }
    assert!(ended);
    assert_eq!(base(&sim, &WHARF, &RATIONS), before, "the base is back");
    let end = facts(&sim, live::DISCOUNT_ENDED_FACT)[0];
    assert!(
        said_citing(&sim, end.id)
            .iter()
            .any(|l| l.text == "Lantern Wharf: the rations discount has ended")
    );
    assert_eq!(live_facts(&sim), 2 + facts(&sim, live::BANTER_FACT).len());
}

#[test]
fn malformed_answers_are_rejected_without_a_retry() {
    for content in ["I would rather not say.", "{\"ware\": ", "[1, 2, 3]"] {
        let mut sim = sim();
        let mock = Mock::default();
        let log = TempLog::new("malformed");
        mock.ok(completion(content));
        let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
        dock(&mut sim, &mut host, &WHARF);
        assert_eq!(mock.posts().len(), 1, "a malformed answer is not retried");
        assert_eq!(live_facts(&sim), 0);
        let rows = log.rows();
        assert_eq!(rows[0]["verdict"], "rejected", "{content}");
        assert!(rows[0]["reason"].as_str().unwrap().contains("malformed"));
    }
}

#[test]
fn a_wrong_shape_or_a_ware_the_station_buys_is_rejected_not_fixed_up() {
    for (answer, why) in [
        (
            r#"{"ware":"ware.rations","percent":"lots","dockings":3,"text":"I can spare some rations."}"#,
            "shape",
        ),
        // The Wharf buys ice: a cut there would lower what it pays the player.
        (
            r#"{"ware":"ware.ice","percent":10,"dockings":3,"text":"I can spare some ice."}"#,
            "not one of the IDs offered",
        ),
    ] {
        let mut sim = sim();
        let mock = Mock::default();
        let log = TempLog::new("shape");
        mock.ok(completion(answer));
        let mut host = host(&sim, config(&[Purpose::Discount]), &mock, &log.sink);
        let before = (base(&sim, &WHARF, &RATIONS), base(&sim, &WHARF, &ICE));
        dock(&mut sim, &mut host, &WHARF);
        assert_eq!(live_facts(&sim), 0);
        assert_eq!(
            (base(&sim, &WHARF, &RATIONS), base(&sim, &WHARF, &ICE)),
            before
        );
        let reason = log.rows()[0]["reason"].as_str().unwrap().to_owned();
        assert!(reason.contains(why), "{reason}");
    }
    // Only wares the station sells are offered.
    let mock = Mock::default();
    mock.ok(completion("{}"));
    let sim = sim();
    let mut host = host(&sim, config(&[Purpose::Discount]), &mock, &CallSink::new());
    let mut sim = sim;
    dock(&mut sim, &mut host, &WHARF);
    let body = &mock.posts()[0].2;
    let wares = &body["response_format"]["json_schema"]["schema"]["properties"]["ware"]["enum"];
    assert_eq!(wares, &json!(["ware.rations", "ware.refined_plate"]));
}

/// A mind with no key: what the live mind answers when `OPENROUTER_API_KEY` is unset.
struct NoKey;

impl Mind for NoKey {
    fn name(&self) -> &str {
        "no key"
    }
    fn decide(&mut self, _: &DecisionRequest) -> Reply<Decision> {
        Reply::none(MindError::NoKey)
    }
    fn line(&mut self, _: &LineRequest) -> Reply<LineDraft> {
        Reply::none(MindError::NoKey)
    }
    fn live(&mut self, _: &LiveRequest) -> Reply<LiveDraft> {
        Reply::none(MindError::NoKey)
    }
}

#[test]
fn without_a_key_the_flourish_is_skipped_and_play_goes_on() {
    // As the game runs without a key: the live layer is off, nothing is asked.
    let mut sim1 = sim();
    let mut off = MindHost::new(
        config(&[Purpose::Banter]),
        Inline::new(NoKey),
        sim1.history(),
    );
    dock(&mut sim1, &mut off, &WHARF);
    assert_eq!(off.stats().live_requests, 0);
    assert_eq!(live_facts(&sim1), 0);

    // And a mind that has no key when asked skips too: no fact, no spend, no line.
    let mut sim2 = sim();
    let mut host = MindHost::new(
        config(&[Purpose::Banter]),
        Inline::new(NoKey),
        sim2.history(),
    );
    host.set_live(true);
    dock(&mut sim2, &mut host, &WHARF);
    dock(&mut sim2, &mut host, &KETTLE);
    assert_eq!(host.stats().live_requests, 2);
    assert_eq!(live_facts(&sim2), 0);
    assert!(facts(&sim2, "mind.spend").is_empty());
}

#[test]
fn a_429_backs_off_and_then_succeeds() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("429");
    mock.queue(Ok(HttpReply::new(429, r#"{"error":{"code":429}}"#)));
    mock.ok(completion(
        r#"{"speaker":"npc.ilse_marrow","text":"I kept the kettle on for you."}"#,
    ));
    let cfg = config(&[Purpose::Banter]);
    let backoff = cfg.live.backoff_ms;
    let mut host = host(&sim, cfg, &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(mock.posts().len(), 2);
    assert_eq!(mock.slept(), [Duration::from_millis(backoff)]);
    assert_eq!(facts(&sim, live::BANTER_FACT).len(), 1);
    let rows = log.rows();
    assert_eq!(rows.len(), 1, "one row per call");
    assert_eq!(rows[0]["attempts"], 2);
    assert_eq!(rows[0]["verdict"], "accepted");

    // A Retry-After is honoured instead of the backoff.
    let mut sim = self::sim();
    let mock = Mock::default();
    mock.queue(Ok(HttpReply {
        retry_after_ms: Some(2500),
        ..HttpReply::new(503, "{}")
    }));
    mock.ok(completion(
        r#"{"speaker":"npc.ilse_marrow","text":"I kept the kettle on for you."}"#,
    ));
    let mut host = self::host(&sim, config(&[Purpose::Banter]), &mock, &CallSink::new());
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(mock.slept(), [Duration::from_millis(2500)]);
    assert_eq!(facts(&sim, live::BANTER_FACT).len(), 1);
}

#[test]
fn a_timeout_skips_the_flourish_and_logs_the_failure() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("timeout");
    for _ in 0..3 {
        mock.queue(Err(MindError::Timeout));
    }
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(mock.posts().len(), 3, "live.max_attempts");
    assert_eq!(live_facts(&sim), 0);
    let rows = log.rows();
    assert_eq!(rows[0]["verdict"], "failed");
    assert_eq!(rows[0]["error_kind"], "timeout");
    assert_eq!(rows[0]["attempts"], 3);
    let totals = log.log().totals(&Range::default()).unwrap();
    assert_eq!(totals["failed"], 1);
}

#[test]
fn a_bad_key_is_logged_as_failed_with_its_status() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("401");
    mock.queue(Ok(HttpReply::new(
        401,
        format!(r#"{{"error":{{"message":"bad key {KEY}","code":401}}}}"#),
    )));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(live_facts(&sim), 0);
    let rows = log.rows();
    assert_eq!(rows[0]["http_status"], 401);
    assert_eq!(rows[0]["verdict"], "failed");
    // The key never reaches the log, even when the server echoes it.
    let all = rows[0].to_string();
    assert!(!all.contains(KEY), "{all}");
    assert!(all.contains("<redacted>"));
}

/// `KEY` with its first letter as a JSON unicode escape: replacing the literal key
/// can't see it, and decoding the text brings it back.
fn escaped_key() -> String {
    format!(r"\u{:04x}{}", u32::from('s'), &KEY[1..])
}

/// Neither the key nor any 12-character run of it can be read from `text`, however
/// often it is JSON-decoded (a string that is JSON is decoded again).
fn assert_no_key(text: &str) {
    fn strings(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) => decoded(s, out),
            Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
            Value::Object(o) => o.iter().for_each(|(k, v)| {
                decoded(k, out);
                strings(v, out);
            }),
            _ => {}
        }
    }
    fn decoded(text: &str, out: &mut Vec<String>) {
        out.push(text.to_owned());
        if let Ok(v) = serde_json::from_str::<Value>(text) {
            strings(&v, out);
        }
    }
    let mut all = Vec::new();
    decoded(text, &mut all);
    for t in &all {
        for run in KEY.as_bytes().windows(12) {
            let run = std::str::from_utf8(run).unwrap();
            assert!(!t.contains(run), "{run:?} is readable in {t:?}");
        }
    }
}

/// Every text column of every row, the world and its history: none holds the key.
fn assert_no_key_stored(sim: &Sim, rows: &[Value]) {
    for row in rows {
        for v in row.as_object().unwrap().values() {
            match v.as_str() {
                Some(text) => assert_no_key(text),
                None => assert_no_key(&v.to_string()),
            }
        }
    }
    assert_no_key(&serde_json::to_string(sim.world()).unwrap());
    assert_no_key(&serde_json::to_string(sim.history().facts()).unwrap());
}

/// Review probe: a live answer whose content (itself JSON) echoes the key in a JSON
/// escape. The logged output still decodes, and decoding it, twice, finds no key.
#[test]
fn an_escaped_key_in_a_live_answer_cannot_be_decoded_from_the_log() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("escaped-answer");
    let escaped = escaped_key();
    mock.ok(completion(&format!(
        r#"{{"speaker":"npc.ilse_marrow","text":"I see {escaped}"}}"#
    )));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    let rows = log.rows();
    let output = rows[0]["output"].as_str().unwrap();
    let decoded: Value = serde_json::from_str(output).unwrap();
    let text = decoded["text"].as_str().unwrap();
    assert!(!text.contains(KEY), "{text}");
    assert!(text.contains("<redacted>"), "{text}");
    assert_no_key_stored(&sim, &rows);
}

/// Review probe: a 401 whose error body echoes the key in a JSON escape. The row keeps
/// the body as JSON, with the message but not the key.
#[test]
fn an_escaped_key_in_an_error_body_cannot_be_decoded_from_the_log() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("escaped-401");
    let escaped = escaped_key();
    mock.queue(Ok(HttpReply::new(
        401,
        format!(r#"{{"error":{{"message":"bad key {escaped}","code":401}}}}"#),
    )));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    let rows = log.rows();
    assert_eq!(rows[0]["verdict"], "failed");
    let decoded: Value = serde_json::from_str(rows[0]["output"].as_str().unwrap()).unwrap();
    let message = decoded["error"]["message"].as_str().unwrap();
    assert_eq!(message, "bad key <redacted>");
    assert_eq!(decoded["error"]["code"], 401);
    assert_no_key_stored(&sim, &rows);
}

/// Every other way an answer can carry the key: a 5xx body with the key escaped twice
/// (JSON in a JSON string), a 200 reporting an upstream error, a transport error (what
/// a timed-out read reports), a model name, and part of the key. None is stored.
#[test]
fn no_response_path_stores_the_key_in_any_encoding() {
    let escaped = escaped_key();
    let twice = format!(r"\\u{:04x}{}", u32::from('s'), &KEY[1..]);
    let every = KEY
        .chars()
        .map(|c| format!(r"\u{:04X}", c as u32))
        .collect::<String>();
    let part = &KEY[KEY.len() - 14..];
    let cases: Vec<Vec<Result<HttpReply, MindError>>> = vec![
        vec![Ok(HttpReply::new(
            502,
            format!(r#"{{"error":{{"message":"{{\"inner\":\"{twice}\"}}","code":502}}}}"#),
        ))],
        vec![Ok(HttpReply::new(
            200,
            format!(r#"{{"error":{{"message":"Bearer {every}","code":403}}}}"#),
        ))],
        vec![Err(MindError::Transport(format!(
            "timed out reading {escaped}"
        )))],
        vec![Ok(HttpReply::new(200, {
            let mut body = completion(&format!(
                r#"{{"speaker":"npc.ilse_marrow","text":"tail {part}"}}"#
            ));
            body["model"] = json!(format!("vendor/{escaped}"));
            body.to_string()
        }))],
    ];
    for (n, replies) in cases.into_iter().enumerate() {
        let mut sim = sim();
        let mock = Mock::default();
        let log = TempLog::new(&format!("paths-{n}"));
        for r in replies {
            for _ in 0..3 {
                mock.queue(r.clone());
            }
        }
        let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
        dock(&mut sim, &mut host, &WHARF);
        let rows = log.rows();
        assert!(!rows.is_empty(), "case {n}");
        assert_no_key_stored(&sim, &rows);
        if let Ok(v) = serde_json::from_str::<Value>(rows[0]["output"].as_str().unwrap()) {
            assert!(v.is_object(), "case {n}: {v}");
        }
    }
}

#[test]
fn a_missing_cost_is_read_from_the_generation_endpoint() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("generation");
    let mut body = completion(r#"{"speaker":"npc.ilse_marrow","text":"I kept the kettle on."}"#);
    body["usage"].as_object_mut().unwrap().remove("cost");
    mock.ok(body);
    mock.get(json!({ "data": { "id": "gen-1761900000-abcdef", "total_cost": 0.000052 } }));
    let mut host = host(&sim, config(&[Purpose::Banter]), &mock, &log.sink);
    dock(&mut sim, &mut host, &WHARF);
    let rows = log.rows();
    assert_eq!(rows[0]["cost_source"], "generation");
    assert!((rows[0]["cost_usd"].as_f64().unwrap() - 0.000052).abs() < 1e-12);
    // The ledger books the looked-up cost, so the budget sees it.
    let spend = facts(&sim, "mind.spend")[0].amount;
    assert_eq!(spend, Some(52));
}

#[test]
fn traffic_and_brags_are_notices_about_the_station() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("traffic");
    // Rotation is by the station's docking count: Kettle's first is entry 1 (brag),
    // its second entry 0 (traffic).
    mock.ok(completion(
        r#"{"craft":"craft.ninefold","ware":"ware.glassite","text":"We filled every rack with glassite and sold it all."}"#,
    ));
    mock.ok(completion(
        r#"{"craft":"craft.little_mercy","bound_for":"station.lantern_wharf","ware":"ware.ice","text":"I've ice for the lamps and I'm in a hurry."}"#,
    ));
    let mut host = host(
        &sim,
        config(&[Purpose::Traffic, Purpose::Brag]),
        &mock,
        &log.sink,
    );
    dock(&mut sim, &mut host, &KETTLE);
    dock(&mut sim, &mut host, &KETTLE);
    let t = facts(&sim, live::TRAFFIC_FACT);
    let b = facts(&sim, live::BRAG_FACT);
    assert_eq!((t.len(), b.len()), (1, 1), "{:?}", log.rows());
    let traffic = said_citing(&sim, t[0].id);
    assert_eq!(traffic[0].channel, Channel::MapNote);
    assert_eq!(
        traffic[0].text,
        "Little Mercy, bound for Lantern Wharf: \"I've ice for the lamps and I'm in a hurry.\""
    );
    let brag = said_citing(&sim, b[0].id);
    assert!(brag[0].text.starts_with("Ninefold: \"We filled"));
    let by_purpose = log
        .log()
        .cost_by(Group::Purpose, &Range::default())
        .unwrap();
    assert_eq!(by_purpose.len(), 2);
}

#[test]
fn cooldowns_and_the_run_budget_are_counted_in_dockings() {
    let mut sim = sim();
    let mock = Mock::default();
    let mut cfg = config(&[Purpose::Banter]);
    cfg.live.cooldown_dockings = 2;
    cfg.live.run_calls = 2;
    for _ in 0..4 {
        mock.ok(completion(
            r#"{"speaker":"npc.ilse_marrow","text":"I kept the kettle on for you."}"#,
        ));
    }
    let mut host = host(&sim, cfg, &mock, &CallSink::new());
    dock(&mut sim, &mut host, &WHARF); // asks (docking 1)
    dock(&mut sim, &mut host, &WHARF); // cooling down
    dock(&mut sim, &mut host, &WHARF); // asks (docking 3)
    dock(&mut sim, &mut host, &WHARF); // cooling down
    dock(&mut sim, &mut host, &WHARF); // run budget spent
    assert_eq!(mock.posts().len(), 2);
}

#[test]
fn a_recorded_live_answer_replays_exactly() {
    let mut sim1 = sim();
    let mock = Mock::default();
    mock.ok(completion(
        r#"{"speaker":"npc.orrin_vale","text":"I polished the cradle rails twice today."}"#,
    ));
    let mut original = host(&sim1, config(&[Purpose::Banter]), &mock, &CallSink::new());
    dock(&mut sim1, &mut original, &WHARF);
    let recording = Recording::from_history(sim1.history().facts());
    assert_eq!(recording.len(), 1);

    let mut sim2 = sim();
    let mut replay = MindHost::new(
        config(&[Purpose::Banter]),
        Inline::new(RecordedMind::new(recording)),
        sim2.history(),
    );
    replay.set_live(true);
    dock(&mut sim2, &mut replay, &WHARF);
    assert_eq!(
        galaxy_core::state_hash(sim1.state()),
        galaxy_core::state_hash(sim2.state())
    );
}

/// One live call per flourish (banter, traffic, discount) through the real path: the
/// host, OpenRouterMind over HTTP, the call log. Needs `OPENROUTER_API_KEY`.
/// `cargo xtask cargo test -p galaxy-mind --test live_layer -- --ignored --nocapture`
#[test]
#[ignore]
fn live_flourishes_against_openrouter() {
    let log = TempLog::new("openrouter");
    let mut out = Vec::new();
    for (purpose, station) in [
        (Purpose::Banter, WHARF),
        (Purpose::Traffic, KETTLE),
        (Purpose::Discount, WHARF),
    ] {
        let mut sim = sim();
        let mut cfg = config(&[purpose]);
        cfg.live.deadline_ticks = 100_000;
        let mind = OpenRouterMind::from_env(cfg.models.clone(), &cfg.limits)
            .expect("OPENROUTER_API_KEY is set")
            .with_live(cfg.live.clone())
            .with_log(log.sink.clone());
        let mut host = MindHost::new(cfg, Inline::new(mind), sim.history());
        host.set_live(true).set_call_log(log.sink.clone());
        dock(&mut sim, &mut host, &station);
        let said: Vec<String> = sim
            .world()
            .talk
            .lines
            .values()
            .filter(|l| {
                l.line.cites.iter().any(|c| {
                    sim.history()
                        .fact(*c)
                        .is_some_and(|f| f.kind.as_str().starts_with("mind."))
                })
            })
            .map(|l| l.line.text.clone())
            .collect();
        out.push(json!({ "purpose": purpose.as_str(), "said": said }));
    }
    let rows = log.rows();
    let summary: Vec<Value> = rows
        .iter()
        .map(|r| {
            json!({
                "purpose": r["purpose"],
                "actual_model": r["actual_model"],
                "task_type": r["task_type"],
                "cost_usd": r["cost_usd"],
                "cost_source": r["cost_source"],
                "tokens": [r["prompt_tokens"], r["completion_tokens"]],
                "attempts": r["attempts"],
                "status": r["http_status"],
                "verdict": r["verdict"],
                "reason": r["reason"],
                "output": r["output"],
            })
        })
        .collect();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({ "results": out, "log": summary })).unwrap()
    );
    assert_eq!(rows.len(), 3);
}

/// Writes a save with a live banter and a running discount at Lantern Wharf (mocked
/// answers through the host), for a look with `galaxy shot`. Offline.
/// `GALAXY_LIVE_SAVE=PATH cargo xtask cargo test -p galaxy-mind --test live_layer write_a_live_save -- --ignored`
#[test]
#[ignore]
fn write_a_live_save() {
    let Some(path) = std::env::var_os("GALAXY_LIVE_SAVE").map(PathBuf::from) else {
        return;
    };
    let mut sim = sim();
    let mock = Mock::default();
    mock.ok(completion(
        r#"{"speaker":"npc.orrin_vale","text":"I polished the cradle rails twice today, so mind your landing."}"#,
    ));
    mock.ok(completion(
        r#"{"ware":"ware.rations","percent":12,"dockings":3,"text":"We baked more than the crews can eat, so I'm letting it go cheap."}"#,
    ));
    let mut host = host(
        &sim,
        config(&[Purpose::Discount, Purpose::Banter]),
        &mock,
        &CallSink::new(),
    );
    dock(&mut sim, &mut host, &WHARF);
    dock(&mut sim, &mut host, &WHARF);
    assert_eq!(facts(&sim, live::BANTER_FACT).len(), 1);
    assert_eq!(facts(&sim, live::DISCOUNT_FACT).len(), 1);
    let _ = std::fs::remove_file(&path);
    galaxy_core::Save::create(&path, sim.state()).unwrap();
}
