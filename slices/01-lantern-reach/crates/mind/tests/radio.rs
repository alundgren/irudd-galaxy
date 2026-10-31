//! Radio text bits from the live layer (#47, 5.1), headless and offline: a real sim on
//! the core pack's data, the host, `OpenRouterMind` over a mock HTTP layer and a call
//! log in a temporary file. The flagship flies into a station channel's range; the bit
//! is validated against the closed world and lands in station history, or is skipped.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_mind::calllog::{CallLog, CallSink, LogRules};
use galaxy_mind::config::{Limits, Models};
use galaxy_mind::radio::{self, Bits, Channel, RadioNote, Rules, Setup};
use galaxy_mind::{
    ApiKey, HttpReply, Inline, MindConfig, MindError, MindHost, OpenRouterMind, Transport,
};
use serde_json::{Value, json};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const KEY: &str = "sk-or-test-key-0123456789";

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

// --- A mock HTTP layer that keeps what it was sent ---

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<(VecDeque<HttpReply>, Vec<Value>)>>);

impl Mock {
    fn ok(&self, content: &str) {
        let body = json!({
            "id": "gen-1761900000-radio",
            "model": "mistralai/ministral-8b",
            "choices": [{ "message": { "role": "assistant", "content": content } }],
            "usage": { "prompt_tokens": 700, "completion_tokens": 30, "cost": 0.00003 },
        });
        self.0
            .lock()
            .unwrap()
            .0
            .push_back(HttpReply::new(200, body.to_string()));
    }
    /// The radio requests sent (a counted docking may also ask for a live flourish).
    fn posts(&self) -> Vec<Value> {
        self.0
            .lock()
            .unwrap()
            .1
            .iter()
            .filter(|b| b["response_format"]["json_schema"]["name"] == "live_radio")
            .cloned()
            .collect()
    }
}

impl Transport for Mock {
    fn post(&mut self, url: &str, auth: &str, body: &str) -> Result<HttpReply, MindError> {
        self.post_with(url, auth, &[], body)
    }
    fn post_with(
        &mut self,
        _url: &str,
        _auth: &str,
        _headers: &[(&str, &str)],
        body: &str,
    ) -> Result<HttpReply, MindError> {
        let mut s = self.0.lock().unwrap();
        s.1.push(serde_json::from_str(body).unwrap());
        s.0.pop_front()
            .ok_or_else(|| MindError::Transport("no reply queued".into()))
    }
    fn get(&mut self, _url: &str, _auth: &str) -> Result<HttpReply, MindError> {
        Err(MindError::Transport("no reply queued".into()))
    }
    fn sleep(&mut self, _wait: Duration) {}
}

struct TempLog {
    dir: PathBuf,
    path: PathBuf,
    sink: CallSink,
}

impl TempLog {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "galaxy-radio-{name}-{}-{}",
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
}

impl Drop for TempLog {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn channel(id: &str, station: StationId, name: &str) -> Channel {
    Channel {
        id: id.into(),
        name: name.into(),
        station,
        range: 600.0,
    }
}

fn setup(rules: Rules) -> Setup {
    Setup {
        channels: vec![
            channel("radio.lantern_wharf", WHARF, "Lantern Wharf radio"),
            channel("radio.kettle_dock", KETTLE, "Kettle Dock radio"),
        ],
        rules,
    }
}

/// A new game with the flagship flying in open space, far from every station.
fn sim() -> Sim {
    let mut state = galaxy_core::new_game(&data_dir(), 7).expect("core data loads");
    let ship = state.world.ships.get_mut(&PLAYER).unwrap();
    ship.docked_at = None;
    ship.position = Vec2::new(250.0, 1200.0);
    Sim::new(state)
}

fn host(sim: &Sim, mock: &Mock, log: &CallSink, rules: Rules) -> MindHost {
    let config = MindConfig::load_dir(&data_dir()).expect("core mind.toml loads");
    let mind = OpenRouterMind::with_transport(
        mock.clone(),
        ApiKey::new(KEY),
        Models::default(),
        &Limits::default(),
    )
    .with_live(config.live.clone())
    .with_log(log.clone());
    let mut host = MindHost::new(config, Inline::new(mind), sim.history());
    host.set_live(true)
        .set_call_log(log.clone())
        .set_radio(setup(rules));
    host
}

fn run(sim: &mut Sim, host: &mut MindHost, ticks: usize) {
    for _ in 0..ticks {
        let report = sim.step(&PlayerInput::default()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        host.pump(sim);
    }
}

/// Puts the flagship at `at`, still, and runs a few ticks.
fn fly_to(sim: &mut Sim, host: &mut MindHost, at: Vec2) {
    sim.submit(
        CommandBatch::new(Cause::system("test", "move")).with(Command::SetShipMotion {
            ship: PLAYER,
            position: at,
            velocity: Vec2::ZERO,
            heading: 0.0,
        }),
    );
    run(sim, host, 3);
}

fn near(sim: &Sim, station: &StationId) -> Vec2 {
    sim.world().stations.get(station).unwrap().position + Vec2::new(120.0, 0.0)
}

fn radio_facts(sim: &Sim) -> Vec<&Fact> {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == radio::RADIO_FACT)
        .collect()
}

fn dock_count(sim: &mut Sim, host: &mut MindHost, n: u64) {
    for _ in 0..n {
        sim.submit(
            CommandBatch::new(Cause::system("test", "count a docking")).with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: KETTLE,
                },
            }),
        );
        run(sim, host, 1);
    }
}

#[test]
fn entering_a_channels_range_asks_once_and_the_bit_goes_into_station_history() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("accept");
    mock.ok(r#"{"speaker":"npc.ilse_marrow","text":"I have the kettle on, so come in out of the lanes."}"#);
    let mut host = host(&sim, &mock, &log.sink, Rules::default());
    run(&mut sim, &mut host, 2);
    assert!(mock.posts().is_empty(), "out of range: nothing asked");
    let at = near(&sim, &WHARF);
    fly_to(&mut sim, &mut host, at);

    // The request: the station's session and only its keepers to pick from.
    let posts = mock.posts();
    assert_eq!(posts.len(), 1);
    let body = &posts[0];
    assert_eq!(body["session_id"], "station.lantern_wharf");
    assert_eq!(body["response_format"]["json_schema"]["name"], "live_radio");
    let schema = &body["response_format"]["json_schema"]["schema"];
    assert_eq!(
        schema["properties"]["speaker"]["enum"],
        json!(["npc.ilse_marrow", "npc.orrin_vale"])
    );
    assert!(!body.to_string().contains(KEY));

    // One fact, Cause::Mind, about the station and the speaker, the channel in `other`.
    let facts = radio_facts(&sim);
    assert_eq!(facts.len(), 1);
    let f = facts[0];
    assert_eq!(f.subjects.station, Some(WHARF));
    assert_eq!(f.subjects.npc.as_ref().unwrap().as_str(), "npc.ilse_marrow");
    assert_eq!(
        f.subjects.other.as_ref().unwrap().as_str(),
        "radio.lantern_wharf"
    );
    let cause = sim.history().cause(f.cause).unwrap();
    assert!(matches!(&cause.cause.kind, CauseKind::Mind { decision, .. }
        if decision == "live.radio:radio.lantern_wharf"));
    let note = RadioNote::parse(f).unwrap();
    assert!(note.text.starts_with("I have the kettle on"));
    assert_eq!(note.model, "mistralai/ministral-8b");

    // Logged with purpose `radio` and the verdict.
    let rows = log.rows();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["purpose"], "radio");
    assert_eq!(rows[0]["verdict"], "accepted");
    assert_eq!(rows[0]["session_id"], "station.lantern_wharf");

    // The HUD's ticker reads it back.
    let mut bits = Bits::default();
    bits.observe(sim.history().facts());
    assert_eq!(bits.on("radio.lantern_wharf").len(), 1);
    assert!(bits.on("radio.kettle_dock").is_empty());

    // Staying in range asks nothing more.
    run(&mut sim, &mut host, 5);
    assert_eq!(mock.posts().len(), 1);
}

#[test]
fn bits_that_break_the_rules_or_name_someone_else_are_rejected_and_skipped() {
    for (answer, why) in [
        (
            json!({ "speaker": "npc.saffron_vey", "text": "I keep the ledger honest here." }),
            "not one of the IDs offered",
        ),
        (
            json!({ "speaker": "npc.ilse_marrow", "text": "The cradle is free today." }),
            "first person",
        ),
        (
            json!({ "speaker": "npc.ilse_marrow", "text": "I sold 40 crates today." }),
            "digits",
        ),
        (
            json!({ "speaker": "npc.ilse_marrow", "text": "I met Captain Zorbulon by the lamps." }),
            "outside the world",
        ),
        (json!({ "text": "I am on the air." }), "speaker"),
    ] {
        let mut sim = sim();
        let mock = Mock::default();
        let log = TempLog::new("reject");
        mock.ok(&answer.to_string());
        let mut host = host(&sim, &mock, &log.sink, Rules::default());
        let at = near(&sim, &WHARF);
        fly_to(&mut sim, &mut host, at);
        assert!(
            radio_facts(&sim).is_empty(),
            "{answer}: nothing reaches the world"
        );
        let rows = log.rows();
        assert_eq!(rows.len(), 1, "{answer}");
        assert_eq!(rows[0]["verdict"], "rejected", "{answer}");
        assert!(
            rows[0]["reason"].as_str().unwrap().contains(why),
            "{answer}: {}",
            rows[0]["reason"]
        );
    }
}

#[test]
fn the_cooldown_counts_dockings_per_channel_and_the_run_has_a_cap() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("cooldown");
    for _ in 0..8 {
        mock.ok(
            r#"{"speaker":"npc.ilse_marrow","text":"I have lamps lit along the pier for you."}"#,
        );
    }
    let rules = Rules {
        cooldown_dockings: 2,
        run_calls: 2,
    };
    let mut host = host(&sim, &mock, &log.sink, rules);
    let wharf = near(&sim, &WHARF);
    let away = Vec2::new(250.0, 1200.0);
    fly_to(&mut sim, &mut host, wharf);
    assert_eq!(mock.posts().len(), 1);
    // Out and back in with no docking between: still cooling down.
    fly_to(&mut sim, &mut host, away);
    fly_to(&mut sim, &mut host, wharf);
    assert_eq!(mock.posts().len(), 1, "cooling down: counted in dockings");
    // Two dockings later, the next entry asks again.
    fly_to(&mut sim, &mut host, away);
    dock_count(&mut sim, &mut host, 2);
    fly_to(&mut sim, &mut host, wharf);
    assert_eq!(mock.posts().len(), 2);
    // The run's cap: no more, however many dockings pass.
    fly_to(&mut sim, &mut host, away);
    dock_count(&mut sim, &mut host, 4);
    fly_to(&mut sim, &mut host, wharf);
    assert_eq!(mock.posts().len(), 2, "the run's cap");
    assert_eq!(radio_facts(&sim).len(), 2);
}

#[test]
fn the_cooldown_survives_a_new_host_from_history() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("resume");
    mock.ok(r#"{"speaker":"npc.orrin_vale","text":"I keep the cradle warm for anyone passing."}"#);
    mock.ok(r#"{"speaker":"npc.orrin_vale","text":"I keep the cradle warm for anyone passing."}"#);
    let mut first = host(&sim, &mock, &log.sink, Rules::default());
    let wharf = near(&sim, &WHARF);
    fly_to(&mut sim, &mut first, wharf);
    assert_eq!(radio_facts(&sim).len(), 1);
    // A new session (as after a load) reads the last ask from history.
    let mut second = host(&sim, &mock, &log.sink, Rules::default());
    run(&mut sim, &mut second, 3);
    assert_eq!(
        mock.posts().len(),
        1,
        "no docking since: still cooling down"
    );
}

#[test]
fn without_the_live_layer_nothing_is_asked() {
    let mut sim = sim();
    let mock = Mock::default();
    let log = TempLog::new("off");
    let mut host = host(&sim, &mock, &log.sink, Rules::default());
    host.set_live(false);
    let at = near(&sim, &WHARF);
    fly_to(&mut sim, &mut host, at);
    assert!(mock.posts().is_empty());
    assert!(radio_facts(&sim).is_empty());
}
