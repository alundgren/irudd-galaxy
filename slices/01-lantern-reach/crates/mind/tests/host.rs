//! The fast-model layer end to end, headless and offline: a real sim, the core pack's
//! data and `mind.toml`, and either the seeded fake mind or OpenRouterMind over a mock
//! HTTP layer. No test here touches the network.

use std::collections::{BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_mind::config::{Budget, Limits, Models};
use galaxy_mind::executor::{Done, Executor, Job, JobResult};
use galaxy_mind::situation::{OutcomeNote, Source, SpendNote, Stage};
use galaxy_mind::{
    ApiKey, Decision, DecisionRequest, DockTalk, FakeMind, HttpReply, Inline, LineDraft, LinePool,
    LineRequest, Mind, MindConfig, MindError, MindHost, OpenRouterMind, PlanOption, PlanSource,
    PoolLine, RecordedMind, Recording, Reply, TraderMenus, Transport, Worker, probe,
};
use serde_json::json;

const NINEFOLD: ShipId = ShipId::from_static("craft.ninefold");
const PLAYER: ShipId = ShipId::from_static("ship.player");
const HOOK: StationId = StationId::from_static("station.cinder_hook");
const GLASS: StationId = StationId::from_static("station.glass_drift_observatory");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn config() -> MindConfig {
    MindConfig::load_dir(&data_dir()).expect("core mind.toml loads")
}

fn input() -> PlayerInput {
    PlayerInput::default()
}

fn npc_ship(id: ShipId, name: &str) -> Command {
    let ship = Ship {
        id: id.clone(),
        name: name.into(),
        hull: HullId::from_static("hull.wren"),
        owner: Owner::Faction {
            faction: FactionId::from_static("faction.tollway_compact"),
        },
        region: RegionId::from_static("region.cinder_drift"),
        position: Vec2::ZERO,
        velocity: Vec2::ZERO,
        heading: 0.0,
        docked_at: Some(HOOK),
        hold: Hold::default(),
        orders: OrderQueue::default(),
        activity: Default::default(),
    };
    Command::PutShip {
        id,
        value: Some(Box::new(ship)),
    }
}

/// A new game with the Ninefold docked at Cinder Hook.
fn sim(seed: u64) -> Sim {
    let state = galaxy_core::new_game(&data_dir(), seed).expect("core data loads");
    let mut sim = Sim::new(state);
    sim.submit(
        CommandBatch::new(Cause::system("test", "setup")).with(npc_ship(NINEFOLD, "Ninefold")),
    );
    sim.step(&input()).unwrap();
    sim
}

/// The ship docks (one tick), the host reacts, and its batches apply (one more tick).
fn dock(sim: &mut Sim, host: &mut MindHost, ship: &ShipId, station: &StationId) {
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock"))
            .with(Command::SetShipDocked {
                ship: ship.clone(),
                station: Some(station.clone()),
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: ship.clone(),
                    station: station.clone(),
                },
            }),
    );
    settle(sim, host);
}

fn settle(sim: &mut Sim, host: &mut MindHost) {
    sim.step(&input()).unwrap();
    host.pump(sim);
    let report = sim.step(&input()).unwrap();
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    host.pump(sim);
}

/// Three plans: load 1, 2 or 3 ice. `vary` makes each docking a new situation;
/// `talk_only` plans load nothing, so the craft's hold (part of the situation) stays put.
struct Plans {
    vary: bool,
    talk_only: bool,
}

impl PlanSource for Plans {
    fn plans(&self, world: &WorldState, ship: &ShipId) -> Vec<PlanOption> {
        (1..=3)
            .map(|n| PlanOption {
                summary: if self.vary {
                    format!("load {n} ice (tick {})", world.meta.tick)
                } else {
                    format!("load {n} ice")
                },
                score: f64::from(10 - n),
                refs: vec![AnyId::new("ware.ice").unwrap()],
                commands: if self.talk_only {
                    Vec::new()
                } else {
                    vec![Command::AdjustCargo {
                        ship: ship.clone(),
                        ware: ICE,
                        delta: i64::from(n),
                    }]
                },
            })
            .collect()
    }
}

fn notes(sim: &Sim, kind: &str) -> Vec<OutcomeNote> {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == kind)
        .map(|f| OutcomeNote::parse(f).expect("a mind note"))
        .collect()
}

fn decisions(sim: &Sim) -> Vec<OutcomeNote> {
    notes(sim, "mind.decision")
}

/// Every booked call: its note and its cost in micro-dollars.
fn spends(sim: &Sim) -> Vec<(SpendNote, i64)> {
    sim.history()
        .facts()
        .iter()
        .filter_map(|f| SpendNote::parse(f).map(|n| (n, f.amount.unwrap_or(0))))
        .collect()
}

fn attempts(sim: &Sim) -> u32 {
    spends(sim).iter().map(|(n, _)| n.attempts).sum()
}

fn trader_host(sim: &Sim, config: MindConfig, exec: impl Executor + 'static) -> MindHost {
    let mut host = MindHost::new(config, exec, sim.history());
    host.add_provider(TraderMenus {
        plans: Plans {
            vary: true,
            talk_only: false,
        },
    });
    host
}

/// Same-situation tests: plans that load nothing, so the craft's hold stays put, and
/// the Ninefold's brief as shipped (it remembers any fact about itself or its place).
fn steady_host(sim: &Sim, config: MindConfig, exec: impl Executor + 'static) -> MindHost {
    let mut host = MindHost::new(config, exec, sim.history());
    host.add_provider(TraderMenus {
        plans: Plans {
            vary: false,
            talk_only: true,
        },
    });
    host
}

// --- A mock HTTP layer ---

#[derive(Clone, Default)]
struct Mock(Arc<Mutex<MockState>>);

#[derive(Default)]
struct MockState {
    replies: VecDeque<Result<HttpReply, MindError>>,
    calls: Vec<String>,
    slept: Vec<Duration>,
}

impl Mock {
    fn queue(&self, reply: Result<HttpReply, MindError>) {
        self.0.lock().unwrap().replies.push_back(reply);
    }
    fn ok(&self, body: serde_json::Value) {
        self.queue(Ok(HttpReply::new(200, body.to_string())));
    }
    fn status(&self, status: u16) {
        self.queue(Ok(HttpReply::new(status, "{}")));
    }
    fn retry_after(&self, status: u16, ms: u64) {
        self.queue(Ok(HttpReply {
            retry_after_ms: Some(ms),
            ..HttpReply::new(status, "{}")
        }));
    }
    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
    }
    fn slept(&self) -> Vec<Duration> {
        self.0.lock().unwrap().slept.clone()
    }
}

impl Transport for Mock {
    fn post(&mut self, url: &str, _auth: &str, _body: &str) -> Result<HttpReply, MindError> {
        let mut s = self.0.lock().unwrap();
        s.calls.push(url.to_owned());
        s.replies
            .pop_front()
            .unwrap_or_else(|| Err(MindError::Transport("no reply queued".into())))
    }
    fn sleep(&mut self, wait: Duration) {
        self.0.lock().unwrap().slept.push(wait);
    }
}

fn mock_mind(mock: &Mock) -> OpenRouterMind<Mock> {
    OpenRouterMind::with_transport(
        mock.clone(),
        ApiKey::new("test-key"),
        Models::default(),
        &Limits::default(),
    )
}

fn jev_choice(choice: &str) -> serde_json::Value {
    json!({
        "model": "typesafe/jev-1.14-20261014",
        "answers": {"pick": {"type": "choice", "choice": choice, "confidence": 0.9}},
        "usage": {"cost": 0.00002},
    })
}

fn chat_line(text: &str, cite: &str) -> serde_json::Value {
    json!({
        "model": "openai/gpt-oss-20b",
        "choices": [{"message": {"content": json!({"text": text, "cite": cite}).to_string()}}],
        "usage": {"cost": 0.00003},
    })
}

// --- Tests ---

#[test]
fn a_trader_decision_becomes_a_mind_cause_and_a_fact() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o2"));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let d = decisions(&sim);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].source, Source::Model);
    assert_eq!(d[0].choice.as_deref(), Some("plan.2"));
    assert_eq!(d[0].default.as_deref(), Some("plan.1"));
    assert_eq!(d[0].calls, 1);
    assert!(d[0].key.starts_with("sk."));
    // Applied as an ordinary command: plan 2 loads 2 ice.
    assert_eq!(
        sim.world().ships.get(&NINEFOLD).unwrap().hold.count(&ICE),
        2
    );
    let cause = sim
        .history()
        .causes()
        .values()
        .find(|c| matches!(c.cause.kind, CauseKind::Mind { .. }))
        .expect("a Mind cause");
    assert_eq!(
        cause.cause.kind,
        CauseKind::Mind {
            npc: None,
            decision: "trader_route:plan.2".into()
        }
    );
    // The spend is booked in its own fact (micro-dollars), not the outcome's.
    let s = spends(&sim);
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].0.attempts, s[0].1), (1, 20));
    assert_eq!(d[0].cost, 20);
    assert!((host.spent_usd() - 0.00002).abs() < 1e-9);
    assert_eq!(mock.calls(), ["https://openrouter.ai/api/alpha/decisions"]);
}

#[test]
fn a_timeout_is_retried_once() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.queue(Err(MindError::Timeout));
    mock.ok(jev_choice("o3"));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Model);
    assert_eq!(d[0].choice.as_deref(), Some("plan.3"));
    assert_eq!(d[0].calls, 2);
    assert_eq!(mock.calls().len(), 2);
}

#[test]
fn two_server_errors_skip_to_the_default_without_looping() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.status(502);
    mock.status(503);
    mock.ok(jev_choice("o3")); // never asked for
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Default);
    assert_eq!(d[0].choice.as_deref(), Some("plan.1"));
    assert_eq!(d[0].why, ["server"]);
    assert_eq!(mock.calls().len(), 2, "one try and one retry, no more");
    assert_eq!(
        sim.world().ships.get(&NINEFOLD).unwrap().hold.count(&ICE),
        1
    );
}

#[test]
fn malformed_answers_fall_back_without_a_retry() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o9")); // not on the menu
    mock.queue(Ok(HttpReply::new(200, "this is not json")));
    mock.ok(json!({"answers": {}})); // no answer at all
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    for station in [&GLASS, &HOOK, &GLASS] {
        dock(&mut sim, &mut host, &NINEFOLD, station);
    }

    let d = decisions(&sim);
    assert_eq!(d.len(), 3);
    for note in &d {
        assert_eq!(note.source, Source::Default);
        assert_eq!(note.choice.as_deref(), Some("plan.1"));
    }
    assert!(d[0].why[0].starts_with("shape"), "{:?}", d[0].why);
    assert_eq!(d[1].why, ["malformed"]);
    assert_eq!(d[2].why, ["malformed"]);
    assert_eq!(mock.calls().len(), 3, "malformed answers are not retried");
}

#[test]
fn the_budget_stops_calls() {
    let mut sim = sim(1);
    let mut config = config();
    config.budget.region.clear();
    config.budget.default = Budget {
        calls: 2,
        window_dockings: 0,
        spend_cap_usd: 1.0,
    };
    let fake = FakeMind::new(5);
    let mut host = trader_host(&sim, config, Inline::new(fake));
    for i in 0..5 {
        let station = if i % 2 == 0 { &GLASS } else { &HOOK };
        dock(&mut sim, &mut host, &NINEFOLD, station);
    }

    let d = decisions(&sim);
    assert_eq!(d.len(), 5);
    assert_eq!(d.iter().map(|n| n.calls).sum::<u32>(), 2);
    assert_eq!(attempts(&sim), 2);
    for note in &d[2..] {
        assert_eq!(note.source, Source::Default);
        assert_eq!(note.why, ["budget"]);
    }
    assert_eq!(host.stats().requests, 2);
}

#[test]
fn the_spend_cap_stops_calls() {
    let mut sim = sim(1);
    let mut config = config();
    // One fake call costs $0.00002; the cap allows exactly one.
    config.budget.spend_cap_usd = 0.00002;
    let mut host = trader_host(&sim, config, Inline::new(FakeMind::new(5)));
    for station in [&GLASS, &HOOK, &GLASS] {
        dock(&mut sim, &mut host, &NINEFOLD, station);
    }
    let d = decisions(&sim);
    assert_eq!(d.iter().map(|n| n.calls).sum::<u32>(), 1);
    assert_eq!(d[2].why, ["spend_cap"]);
}

#[test]
fn the_same_situation_is_answered_from_the_cache() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o2"));
    let mut host = steady_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Model);
    assert_eq!(d[1].source, Source::Cache);
    assert_eq!(d[0].key, d[1].key);
    assert_eq!(d[1].choice.as_deref(), Some("plan.2"));
    assert_eq!(d[1].calls, 0);
    assert_eq!(mock.calls().len(), 1);
}

/// Review finding 6: the key covers the place and the remembered facts.
#[test]
fn a_changed_grounding_packet_is_a_new_situation() {
    let mut sim = sim(1);
    let mut host = steady_host(&sim, config(), Inline::new(FakeMind::new(3)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    // Another station: a different place, so a different key.
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    // Back at Glass Drift, but the craft has delivered something there since.
    sim.submit(
        CommandBatch::new(Cause::system("test", "deliver")).with(Command::Emit {
            event: Event::Deliver {
                ship: NINEFOLD,
                station: GLASS,
                ware: ICE,
                quantity: 5,
                ledger: None,
            },
        }),
    );
    sim.step(&input()).unwrap();
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let d = decisions(&sim);
    assert_eq!(d.len(), 3);
    assert_ne!(d[0].key, d[1].key, "stations differ");
    assert_ne!(d[0].key, d[2].key, "a new related delivery");
    assert!(d.iter().all(|n| n.source != Source::Cache), "{d:?}");
}

#[test]
fn the_cache_survives_a_new_host_over_the_same_history() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o3"));
    let mut host = steady_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    // As after a load: a fresh host reads the cache and the tally back from history.
    let fresh = Mock::default();
    let mut host = steady_host(&sim, config(), Inline::new(mock_mind(&fresh)));
    assert!((host.spent_usd() - 0.00002).abs() < 1e-9);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(decisions(&sim)[1].source, Source::Cache);
    assert!(fresh.calls().is_empty());
}

/// Same seed, same events: the same decisions and the same final state.
#[test]
fn same_seed_same_decisions() {
    fn run(seed: u64) -> (Vec<Option<String>>, String) {
        let mut sim = sim(1);
        let mut host = trader_host(&sim, config(), Inline::new(FakeMind::new(seed)));
        for i in 0..8 {
            let station = if i % 2 == 0 { &GLASS } else { &HOOK };
            dock(&mut sim, &mut host, &NINEFOLD, station);
        }
        let choices = decisions(&sim).into_iter().map(|n| n.choice).collect();
        (choices, sim.hash())
    }
    let (a, hash_a) = run(42);
    let (b, hash_b) = run(42);
    assert_eq!(a, b);
    assert_eq!(hash_a, hash_b);
    assert_eq!(a.len(), 8);
    assert!(
        a.iter().any(|c| c.as_deref() != Some("plan.1")),
        "the fake mind sometimes departs from the utility top choice: {a:?}"
    );
}

#[test]
fn a_recording_replays_the_same_decisions() {
    let scenario = |host_mind: Inline| {
        let mut sim = sim(1);
        let mut host = trader_host(&sim, config(), host_mind);
        for i in 0..6 {
            let station = if i % 2 == 0 { &GLASS } else { &HOOK };
            dock(&mut sim, &mut host, &NINEFOLD, station);
        }
        sim
    };
    let original = scenario(Inline::new(FakeMind::new(9)));
    let recording = Recording::from_history(original.history().facts());
    assert!(!recording.is_empty());
    let replay = scenario(Inline::new(RecordedMind::new(recording)));
    let choices = |s: &Sim| {
        decisions(s)
            .into_iter()
            .map(|n| n.choice)
            .collect::<Vec<_>>()
    };
    assert_eq!(choices(&original), choices(&replay));
    assert_eq!(spends(&original), spends(&replay));
}

/// An executor that takes jobs and never answers, like a hung connection.
struct Stalled(Arc<Mutex<Vec<u64>>>);

impl Executor for Stalled {
    fn submit(&mut self, ticket: u64, _job: Job) -> bool {
        self.0.lock().unwrap().push(ticket);
        true
    }
    fn poll(&mut self) -> Vec<Done> {
        Vec::new()
    }
    fn in_flight(&self) -> usize {
        self.0.lock().unwrap().len()
    }
}

#[test]
fn a_slow_answer_takes_the_default_at_the_deadline() {
    let mut sim = sim(1);
    let mut config = config();
    config.limits.deadline_ticks = 3;
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let mut host = trader_host(&sim, config, Stalled(jobs.clone()));
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock")).with(Command::Emit {
            event: Event::Dock {
                ship: NINEFOLD,
                station: HOOK,
            },
        }),
    );
    sim.step(&input()).unwrap();
    assert_eq!(host.pump(&mut sim).in_flight, 1);
    // Nothing waits: the sim keeps ticking while the call is out.
    for _ in 0..2 {
        sim.step(&input()).unwrap();
        assert_eq!(host.pump(&mut sim).batches, 0);
    }
    sim.step(&input()).unwrap();
    let report = host.pump(&mut sim);
    assert_eq!((report.batches, report.in_flight), (1, 0));
    sim.step(&input()).unwrap();
    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Default);
    assert_eq!(d[0].why, ["late"]);
    assert_eq!(jobs.lock().unwrap().len(), 1);
}

#[test]
fn an_answer_that_no_longer_fits_the_world_falls_back() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o2"));
    let mut config = config();
    config.limits.deadline_ticks = 100;
    // The craft leaves in the tick it docked, so the answer no longer fits.
    let mut host = trader_host(&sim, config, Inline::new(mock_mind(&mock)));
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock and leave"))
            .with(Command::Emit {
                event: Event::Dock {
                    ship: NINEFOLD,
                    station: GLASS,
                },
            })
            .with(Command::SetShipDocked {
                ship: NINEFOLD,
                station: None,
            }),
    );
    settle(&mut sim, &mut host);
    let d = decisions(&sim);
    assert_eq!(d[0].choice, None, "the moment is skipped");
    assert!(d[0].why.iter().any(|w| w.contains("no longer docked")));
    assert_eq!(host.stats().skipped, 1);
}

// --- Dock talk ---

/// Dock talk as shipped: pick and fill, no free phrasing.
fn talk_host(sim: &Sim, exec: impl Executor + 'static) -> MindHost {
    talk_host_with(sim, config(), exec)
}

/// Dock talk with the opt-in free phrasing turned on.
fn phrasing_host(sim: &Sim, exec: impl Executor + 'static) -> MindHost {
    let mut config = config();
    config.talk.free_phrasing = true;
    talk_host_with(sim, config, exec)
}

fn talk_host_with(sim: &Sim, config: MindConfig, exec: impl Executor + 'static) -> MindHost {
    let mut host = MindHost::new(config, exec, sim.history());
    host.add_provider(DockTalk {
        lines: LinePool {
            lines: vec![PoolLine {
                id: "line.odo_fenn.01".into(),
                npc: ODO,
                cites: "deliver".into(),
                text: "{qty} ice. The beans will hear about this.".into(),
            }],
        },
    });
    host
}

/// Three authored variants Odo may say about a delivery.
const ODO_LINES: [&str; 3] = [
    "{qty} ice. The beans will hear about this.",
    "Logged {qty} {ware} from you. Kettle thanks you.",
    "{qty} more for the tanks. Go get warm.",
];

fn odo_pool() -> LinePool {
    LinePool {
        lines: ODO_LINES
            .iter()
            .enumerate()
            .map(|(i, text)| PoolLine {
                id: format!("line.odo_fenn.{:02}", i + 1),
                npc: ODO,
                cites: "deliver".into(),
                text: (*text).into(),
            })
            .collect(),
    }
}

/// The player delivers ice at Kettle Dock and docks there. Returns the deliver fact.
fn deliver_and_dock(sim: &mut Sim) -> FactId {
    sim.submit(
        CommandBatch::new(Cause::system("test", "deliver and dock"))
            .with(Command::Emit {
                event: Event::Deliver {
                    ship: PLAYER,
                    station: KETTLE,
                    ware: ICE,
                    quantity: 12,
                    ledger: None,
                },
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: KETTLE,
                },
            }),
    );
    sim.step(&input()).unwrap();
    sim.history()
        .last(&HistoryQuery::kind(FactKind::new("deliver").unwrap()))
        .unwrap()
        .id
}

/// Free phrasing (opt-in): the model's line, slots filled by the game.
#[test]
fn dock_talk_phrases_a_line_that_cites_its_fact() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = phrasing_host(&sim, Inline::new(mock_mind(&mock)));
    let fact = deliver_and_dock(&mut sim);
    mock.ok(chat_line(
        "{qty} ice. The beans will hear about it, I promise.",
        &fact.to_string(),
    ));
    let report = host.pump(&mut sim);
    sim.step(&input()).unwrap();

    assert_eq!(report.lines.len(), 1);
    let line = &report.lines[0];
    assert_eq!(line.source, Source::Model);
    // The model kept the slot; the game filled in the quantity.
    assert_eq!(
        line.talk.text,
        "12 ice. The beans will hear about it, I promise."
    );
    assert_eq!(line.talk.cites, vec![fact]);
    assert_eq!(line.talk.speaker, Some(ODO));
    assert!(line.talk.validate().is_ok());
    // Only one line qualified, so no pick call: one line call.
    assert_eq!(
        mock.calls(),
        ["https://openrouter.ai/api/v1/chat/completions"]
    );
    let n = &notes(&sim, "mind.line")[0];
    assert_eq!(n.why, ["single"]);
    assert_eq!(n.line.as_ref().unwrap().source, Source::Model);
}

#[test]
fn a_line_citing_a_missing_fact_is_rejected_for_the_authored_line() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = phrasing_host(&sim, Inline::new(mock_mind(&mock)));
    let fact = deliver_and_dock(&mut sim);
    mock.ok(chat_line(
        "Twelve ice, and a ghost ship came too.",
        "fact.999",
    ));
    let report = host.pump(&mut sim);
    sim.step(&input()).unwrap();

    let line = &report.lines[0];
    assert_eq!(line.source, Source::Default);
    assert_eq!(line.talk.text, "12 ice. The beans will hear about this.");
    assert_eq!(line.talk.cites, vec![fact]);
    let n = &notes(&sim, "mind.line")[0];
    // The reason is recorded without quoting the model's text.
    assert!(
        n.why.iter().any(|w| w.contains("not in the packet")),
        "{:?}",
        n.why
    );
    assert!(!n.why.iter().any(|w| w.contains("fact.999")), "{:?}", n.why);
}

#[test]
fn no_eligible_line_means_silence() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = talk_host(&sim, Inline::new(mock_mind(&mock)));
    // Docking at Kettle with nothing to cite.
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock")).with(Command::Emit {
            event: Event::Dock {
                ship: PLAYER,
                station: KETTLE,
            },
        }),
    );
    sim.step(&input()).unwrap();
    let report = host.pump(&mut sim);
    assert!(report.lines.is_empty());
    assert_eq!(report.batches, 0);
    assert!(mock.calls().is_empty());
}

#[test]
fn a_failed_line_call_uses_the_authored_line() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = phrasing_host(&sim, Inline::new(mock_mind(&mock)));
    deliver_and_dock(&mut sim);
    mock.status(500);
    mock.queue(Err(MindError::Timeout));
    let report = host.pump(&mut sim);
    assert_eq!(
        report.lines[0].talk.text,
        "12 ice. The beans will hear about this."
    );
    assert_eq!(report.lines[0].source, Source::Default);
    assert_eq!(mock.calls().len(), 2);
}

// --- Rumor spread ---

#[test]
fn a_craft_carries_a_rumor_with_its_truth() {
    let mut sim = sim(1);
    let rumor = RumorId::from_static("rumor.ember_shoals_vein");
    sim.submit(
        CommandBatch::new(Cause::system("test", "rumor")).with(Command::PutRumor {
            id: rumor.clone(),
            value: Some(Box::new(Rumor {
                id: rumor.clone(),
                claim: "a fat glassite vein at the Ember Shoals".into(),
                is_true: false,
                planted_by: None,
                heard_at: [HOOK].into(),
                heard_by_player: false,
                source: None,
            })),
        }),
    );
    sim.step(&input()).unwrap();
    let mut host = MindHost::new(config(), Inline::new(FakeMind::new(1)), sim.history());
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    assert!(
        notes(&sim, "mind.decision").is_empty(),
        "nothing new to carry"
    );
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);

    let r = sim.world().rumors.get(&rumor).unwrap();
    assert!(r.heard_at.contains(&GLASS));
    assert!(!r.is_true, "truth travels unchanged");
    let spread = sim
        .history()
        .last(&HistoryQuery::kind(
            FactKind::new("mind.rumor_spread").unwrap(),
        ))
        .expect("a spread fact");
    assert_eq!(spread.subjects.station, Some(GLASS));
    let FactPayload::Note(note) = &spread.payload else {
        panic!("a note")
    };
    let note: serde_json::Value = serde_json::from_str(note).unwrap();
    assert_eq!(note["truth"], false);
    assert_eq!(note["from"], "station.cinder_hook");
    let d = decisions(&sim);
    assert_eq!(d[0].menu, "rumor_spread");
    assert_eq!(d[0].why, ["single"]);
    // It's journaled under a Mind cause, so the game master can roll it back.
    assert!(sim.journal().iter().any(|j| {
        matches!(
            sim.history().cause(j.cause).unwrap().cause.kind,
            CauseKind::Mind { .. }
        )
    }));
    // Already there: the next docking carries nothing.
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(decisions(&sim).len(), 1);
}

// --- Review findings (PR #19) ---

/// Finding 1, end to end: a line that cites the real delivery but invents its size
/// and an event is dropped for the authored line.
#[test]
fn a_cited_line_with_invented_facts_falls_back_to_the_authored_line() {
    for invented in [
        "You delivered 900 ice and destroyed the station.",
        "{qty} ice, and you destroyed the station.",
        "{qty} ice. Saffron Vey will want to hear.",
    ] {
        let mut sim = sim(1);
        let mock = Mock::default();
        let mut host = phrasing_host(&sim, Inline::new(mock_mind(&mock)));
        let fact = deliver_and_dock(&mut sim);
        mock.ok(chat_line(invented, &fact.to_string()));
        let report = host.pump(&mut sim);
        sim.step(&input()).unwrap();
        let line = &report.lines[0];
        assert_eq!(line.source, Source::Default, "{invented}");
        assert_eq!(line.talk.text, "12 ice. The beans will hear about this.");
        let n = &notes(&sim, "mind.line")[0];
        assert!(n.why.iter().any(|w| w.starts_with("text")), "{:?}", n.why);
        // The rejected answer was still paid for.
        assert_eq!(spends(&sim)[0].1, 30);
    }
}

/// An executor the test answers by hand, like a slow worker.
#[derive(Clone, Default)]
struct Manual(Arc<Mutex<(Vec<u64>, Vec<Done>)>>);

impl Manual {
    fn tickets(&self) -> Vec<u64> {
        self.0.lock().unwrap().0.clone()
    }
    fn finish(&self, ticket: u64, result: JobResult) {
        self.0.lock().unwrap().1.push(Done {
            ticket,
            result,
            latency_ms: 0,
        });
    }
}

impl Executor for Manual {
    fn submit(&mut self, ticket: u64, _job: Job) -> bool {
        self.0.lock().unwrap().0.push(ticket);
        true
    }
    fn poll(&mut self) -> Vec<Done> {
        std::mem::take(&mut self.0.lock().unwrap().1)
    }
    fn in_flight(&self) -> usize {
        self.0.lock().unwrap().0.len()
    }
}

fn one_call_budget() -> MindConfig {
    let mut config = config();
    config.budget.region.clear();
    config.budget.default = Budget {
        calls: 1,
        window_dockings: 0,
        spend_cap_usd: 1.0,
    };
    config
}

/// Finding 2: a paid answer that arrives after its moment passed is still booked, and
/// keeps its budget slot until it lands.
#[test]
fn a_late_paid_answer_is_booked_and_holds_its_budget() {
    let mut sim = sim(1);
    let mut config = one_call_budget();
    config.limits.deadline_ticks = 2;
    let exec = Manual::default();
    let mut host = trader_host(&sim, config, exec.clone());
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock")).with(Command::Emit {
            event: Event::Dock {
                ship: NINEFOLD,
                station: HOOK,
            },
        }),
    );
    sim.step(&input()).unwrap();
    assert_eq!(host.pump(&mut sim).in_flight, 1);
    for _ in 0..3 {
        sim.step(&input()).unwrap();
        host.pump(&mut sim);
    }
    assert_eq!(decisions(&sim)[0].why, ["late"]);
    // Still out, so a new menu finds no room in the budget.
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(decisions(&sim)[1].why, ["budget"]);
    assert_eq!(exec.tickets().len(), 1);

    // The answer lands, late and paid for.
    let reply = Reply::ok(
        Decision::Choice {
            choice: "o2".into(),
            confidence: None,
        },
        0.00002,
        "typesafe/jev-1.14",
    );
    exec.finish(exec.tickets()[0], JobResult::Decide(reply));
    host.pump(&mut sim);
    sim.step(&input()).unwrap();
    let s = spends(&sim);
    assert_eq!(s.len(), 1);
    assert!(s[0].0.late);
    assert_eq!((s[0].0.attempts, s[0].1), (1, 20));
    assert!((host.spent_usd() - 0.00002).abs() < 1e-9);
    assert_eq!(host.stats().late_dropped, 1);
    // Its call is now spent, so the budget stays closed.
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    assert_eq!(decisions(&sim)[2].why, ["budget"]);
    assert_eq!(exec.tickets().len(), 1);
}

/// Finding 2: a paid response the client can't read still costs what it reported.
#[test]
fn a_malformed_paid_response_is_booked() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(json!({"answers": {}, "usage": {"cost": 0.00002}}));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    let d = decisions(&sim);
    assert_eq!(d[0].why, ["malformed"]);
    let s = spends(&sim);
    assert_eq!((s[0].0.attempts, s[0].1), (1, 20));
    assert_eq!(s[0].0.error.as_deref(), Some("malformed"));
    assert!((host.spent_usd() - 0.00002).abs() < 1e-9);
}

/// Finding 2: a billed failure before a successful retry is summed in.
#[test]
fn a_retried_call_books_both_attempts_costs() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.queue(Ok(HttpReply::new(
        502,
        json!({"usage": {"cost": 0.00001}}).to_string(),
    )));
    mock.ok(jev_choice("o2"));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    let s = spends(&sim);
    assert_eq!((s[0].0.attempts, s[0].1), (2, 30));
}

/// Finding 3: two craft carry one rumor to two stations in the same pump; both
/// destinations stay.
#[test]
fn concurrent_rumor_spreads_compose() {
    const KEEL: ShipId = ShipId::from_static("craft.even_keel");
    let mut sim = sim(1);
    let rumor = RumorId::from_static("rumor.ember_shoals_vein");
    sim.submit(
        CommandBatch::new(Cause::system("test", "rumor"))
            .with(npc_ship(KEEL, "Even Keel"))
            .with(Command::PutRumor {
                id: rumor.clone(),
                value: Some(Box::new(Rumor {
                    id: rumor.clone(),
                    claim: "a fat glassite vein at the Ember Shoals".into(),
                    is_true: true,
                    planted_by: None,
                    heard_at: [HOOK].into(),
                    heard_by_player: false,
                    source: None,
                })),
            }),
    );
    sim.step(&input()).unwrap();
    let mut host = MindHost::new(config(), Inline::new(FakeMind::new(1)), sim.history());
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    dock(&mut sim, &mut host, &KEEL, &HOOK);
    // Both leave Cinder Hook and dock elsewhere in the same tick.
    let mut batch = CommandBatch::new(Cause::system("test", "two dockings"));
    for (ship, station) in [(&NINEFOLD, &GLASS), (&KEEL, &KETTLE)] {
        batch = batch
            .with(Command::SetShipDocked {
                ship: ship.clone(),
                station: Some(station.clone()),
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: ship.clone(),
                    station: station.clone(),
                },
            });
    }
    sim.submit(batch);
    settle(&mut sim, &mut host);

    let heard = &sim.world().rumors.get(&rumor).unwrap().heard_at;
    assert!(
        heard.contains(&GLASS) && heard.contains(&KETTLE),
        "{heard:?}"
    );
    let spread = notes_of_kind(&sim, "mind.rumor_spread");
    assert_eq!(spread, 2);
}

fn notes_of_kind(sim: &Sim, kind: &str) -> usize {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == kind)
        .count()
}

/// Finding 4: identical situations replay occurrence by occurrence, failures first.
#[test]
fn a_recording_replays_failures_and_their_order() {
    let scenario = |mind: Inline| {
        let mut sim = sim(1);
        let mut host = steady_host(&sim, config(), mind);
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
        sim
    };
    let mock = Mock::default();
    mock.status(400);
    mock.ok(jev_choice("o2"));
    let original = scenario(Inline::new(mock_mind(&mock)));
    let d = decisions(&original);
    assert_eq!(d[0].key, d[1].key, "the same situation twice");
    assert_eq!(
        d.iter().map(|n| n.choice.clone()).collect::<Vec<_>>(),
        [Some("plan.1".to_owned()), Some("plan.2".to_owned())]
    );

    let recording = Recording::from_history(original.history().facts());
    assert_eq!(recording.len(), 2);
    let replay = scenario(Inline::new(RecordedMind::new(recording)));
    let r = decisions(&replay);
    assert_eq!(
        r.iter().map(|n| n.choice.clone()).collect::<Vec<_>>(),
        [Some("plan.1".to_owned()), Some("plan.2".to_owned())]
    );
    assert_eq!(r[0].why, ["http"]);
    assert_eq!(spends(&original), spends(&replay));
}

/// Finding 4: a phrased line replays with its spend.
#[test]
fn a_recorded_line_replays_with_its_spend() {
    let run = |mind: Inline| {
        let mut sim = sim(1);
        let mut host = phrasing_host(&sim, mind);
        deliver_and_dock(&mut sim);
        let report = host.pump(&mut sim);
        sim.step(&input()).unwrap();
        (sim, report.lines[0].talk.text.clone(), host.spent_usd())
    };
    // The deliver fact's ID is the same in every run; find it with a dry run.
    let fact = deliver_and_dock(&mut sim(1));
    let mock = Mock::default();
    mock.ok(chat_line(
        "{qty} ice! The beans will be glad.",
        &fact.to_string(),
    ));
    let (original, text, spent) = run(Inline::new(mock_mind(&mock)));
    assert_eq!(text, "12 ice! The beans will be glad.");
    let recording = Recording::from_history(original.history().facts());
    let (replay, replay_text, replay_spent) = run(Inline::new(RecordedMind::new(recording)));
    assert_eq!(replay_text, text);
    assert!((replay_spent - spent).abs() < 1e-12 && spent > 0.0);
    assert_eq!(spends(&original), spends(&replay));
}

/// Finding 4: a recorded answer is handled on the tick the original was.
#[test]
fn a_recorded_answer_lands_when_the_original_did() {
    let mut sim = sim(1);
    let exec = Manual::default();
    let mut host = steady_host(&sim, config(), exec.clone());
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    for _ in 0..3 {
        sim.step(&input()).unwrap();
        host.pump(&mut sim);
    }
    let reply = Reply::ok(
        Decision::Choice {
            choice: "o3".into(),
            confidence: None,
        },
        0.00002,
        "jev",
    );
    exec.finish(exec.tickets()[0], JobResult::Decide(reply));
    host.pump(&mut sim);
    sim.step(&input()).unwrap();
    let waited = spends(&sim)[0].0.waited;
    assert!(waited > 0);
    let applied = |s: &Sim| {
        s.history()
            .facts()
            .iter()
            .find(|f| f.kind.as_str() == "mind.decision")
            .map(|f| f.tick)
    };

    let recording = Recording::from_history(sim.history().facts());
    let mut replay = self::sim(1);
    let mut host = steady_host(&replay, config(), Inline::new(RecordedMind::new(recording)));
    dock(&mut replay, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(applied(&replay), None, "held back");
    for _ in 0..4 {
        replay.step(&input()).unwrap();
        host.pump(&mut replay);
    }
    replay.step(&input()).unwrap();
    assert_eq!(applied(&replay), applied(&sim));
    assert_eq!(decisions(&replay)[0].choice.as_deref(), Some("plan.3"));
}

/// Finding 5: a pick and its phrasing share the budget; the phrasing is skipped when
/// the pick used the last call.
#[test]
fn a_two_stage_line_stays_within_the_call_budget() {
    let mut sim = sim(1);
    let mut config = one_call_budget();
    config.talk.pick = true;
    config.talk.free_phrasing = true;
    let mock = Mock::default();
    let mut host = MindHost::new(config, Inline::new(mock_mind(&mock)), sim.history());
    host.add_provider(DockTalk {
        lines: LinePool {
            lines: ["01", "02"]
                .map(|n| PoolLine {
                    id: format!("line.odo_fenn.{n}"),
                    npc: ODO,
                    cites: "deliver".into(),
                    text: format!("{{qty}} ice, line {n}."),
                })
                .into(),
        },
    });
    deliver_and_dock(&mut sim);
    mock.ok(jev_choice("o1"));
    mock.ok(chat_line("never asked for", "fact.1"));
    let report = host.pump(&mut sim);
    sim.step(&input()).unwrap();
    assert_eq!(report.lines[0].source, Source::Default);
    assert_eq!(mock.calls(), ["https://openrouter.ai/api/alpha/decisions"]);
    assert_eq!(attempts(&sim), 1);
    let n = &notes(&sim, "mind.line")[0];
    assert!(n.why.iter().any(|w| w == "budget"), "{:?}", n.why);
}

/// Finding 5: a retry is an attempt too; with one call left there is no retry.
#[test]
fn a_retry_needs_budget_of_its_own() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.queue(Err(MindError::Timeout));
    mock.ok(jev_choice("o2")); // never sent
    let mut host = trader_host(&sim, one_call_budget(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(mock.calls().len(), 1);
    assert_eq!(attempts(&sim), 1);
    assert_eq!(decisions(&sim)[0].why, ["timeout"]);
}

/// Finding 7: `Retry-After` within the deadline is waited out, then retried once.
#[test]
fn retry_after_within_the_deadline_is_honoured() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.retry_after(429, 1_000);
    mock.ok(jev_choice("o2"));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(mock.slept(), [Duration::from_secs(1)]);
    assert_eq!(mock.calls().len(), 2);
    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Model);
    assert_eq!(d[0].calls, 2);
}

/// Finding 7: a wait past the deadline, or a 429 with no wait given, skips the moment
/// instead of retrying at once.
#[test]
fn retry_after_past_the_deadline_skips_the_moment() {
    // The core pack's deadline is 300 ticks, 10 s.
    for reply in [Some(60_000), Some(u64::MAX), None] {
        let mut sim = sim(1);
        let mock = Mock::default();
        match reply {
            Some(ms) => mock.retry_after(429, ms),
            None => mock.status(429),
        }
        mock.ok(jev_choice("o2")); // never sent
        let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
        assert_eq!(mock.calls().len(), 1, "{reply:?}");
        assert!(mock.slept().is_empty());
        let d = decisions(&sim);
        assert_eq!(d[0].source, Source::Default);
        assert_eq!(d[0].why, ["rate_limited"]);
    }
    // A 503 with a short wait is retried after it.
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.retry_after(503, 500);
    mock.ok(jev_choice("o2"));
    let mut host = trader_host(&sim, config(), Inline::new(mock_mind(&mock)));
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(mock.slept(), [Duration::from_millis(500)]);
    assert_eq!(decisions(&sim)[0].source, Source::Model);
}

// --- Re-check findings (PR #19) ---

/// A mind that picks the second option and, if ever asked for a line, writes one full
/// of invented facts. It notes which calls it got.
#[derive(Clone, Default)]
struct Hostile(Arc<Mutex<Vec<&'static str>>>);

impl Mind for Hostile {
    fn name(&self) -> &str {
        "hostile"
    }
    fn decide(&mut self, _req: &DecisionRequest) -> Reply<Decision> {
        self.0.lock().unwrap().push("decide");
        let pick = Decision::Choice {
            choice: "o2".into(),
            confidence: None,
        };
        Reply::ok(pick, 0.00002, "jev")
    }
    fn line(&mut self, req: &LineRequest) -> Reply<LineDraft> {
        self.0.lock().unwrap().push("line");
        let draft = LineDraft {
            text: "You delivered 900 ice and destroyed the station.".into(),
            cites: vec![req.cite.clone()],
        };
        Reply::ok(draft, 0.00003, "gpt-oss")
    }
}

/// Finding 1: by default dock talk is pick and fill. The model picks among authored
/// lines; the game fills the slots. Not one word the model could write is shown, said
/// or recorded, and no line model is ever asked.
#[test]
fn the_default_dock_talk_never_shows_model_written_text() {
    let mut sim = sim(1);
    let mind = Hostile::default();
    let mut host = MindHost::new(config(), Inline::new(mind.clone()), sim.history());
    host.add_provider(DockTalk { lines: odo_pool() });
    let authored: BTreeSet<String> = ODO_LINES
        .iter()
        .map(|t| t.replace("{qty}", "12").replace("{ware}", "ice"))
        .collect();

    let mut spoken = Vec::new();
    for _ in 0..3 {
        deliver_and_dock(&mut sim);
        spoken.extend(host.pump(&mut sim).lines);
        let report = sim.step(&input()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    }
    assert_eq!(spoken.len(), 3);
    for line in &spoken {
        assert!(authored.contains(&line.talk.text), "{}", line.talk.text);
        assert_eq!(line.source, Source::Default, "the text is authored");
        assert_eq!(line.pick, Source::Model, "the model picked it");
        assert_eq!(line.line, "line.odo_fenn.02");
        assert!(line.talk.validate().is_ok());
    }
    assert_eq!(
        spoken[0].talk.text,
        "Logged 12 ice from you. Kettle thanks you."
    );
    // What history keeps: the authored text, in the fact and in the cause.
    for f in sim.history().facts() {
        if f.kind.as_str() == "mind.line" {
            let note = OutcomeNote::parse(f).unwrap();
            assert!(authored.contains(&note.line.unwrap().text));
            let cause = sim.history().cause(f.cause).unwrap();
            assert!(
                !cause.cause.summary.contains("900"),
                "{}",
                cause.cause.summary
            );
        }
    }
    let calls = mind.0.lock().unwrap().clone();
    assert!(calls.iter().all(|c| *c == "decide"), "{calls:?}");
    assert!(spends(&sim).iter().all(|(n, _)| n.stage == Stage::Pick));
}

/// Finding 1: one cheap Jev choice picks the variant; the game fills it.
#[test]
fn dock_talk_picks_an_authored_variant_and_fills_it() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = MindHost::new(config(), Inline::new(mock_mind(&mock)), sim.history());
    host.add_provider(DockTalk { lines: odo_pool() });
    let fact = deliver_and_dock(&mut sim);
    mock.ok(jev_choice("o3"));
    let report = host.pump(&mut sim);
    sim.step(&input()).unwrap();
    let line = &report.lines[0];
    assert_eq!(line.talk.text, "12 more for the tanks. Go get warm.");
    assert_eq!(line.talk.cites, vec![fact]);
    assert_eq!((line.source, line.pick), (Source::Default, Source::Model));
    assert_eq!(mock.calls(), ["https://openrouter.ai/api/alpha/decisions"]);
    let n = &notes(&sim, "mind.line")[0];
    assert!(n.why.is_empty(), "{:?}", n.why);
    assert_eq!(n.choice.as_deref(), Some("line.odo_fenn.03"));
}

/// A mind that blocks each call until the test lets it go, like a slow request.
struct Gate {
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    calls: Arc<AtomicUsize>,
}

impl Mind for Gate {
    fn name(&self) -> &str {
        "gate"
    }
    fn decide(&mut self, _req: &DecisionRequest) -> Reply<Decision> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.started.send(());
        let _ = self.release.recv();
        let pick = Decision::Choice {
            choice: "o2".into(),
            confidence: None,
        };
        Reply::ok(pick, 0.00002, "jev")
    }
    fn line(&mut self, _req: &LineRequest) -> Reply<LineDraft> {
        Reply::none(MindError::NotRecorded)
    }
}

struct Gated {
    host: MindHost,
    started: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    calls: Arc<AtomicUsize>,
}

/// A trader host on a worker thread whose calls wait for `release`.
fn gated_host(sim: &Sim) -> Gated {
    let (started_tx, started) = mpsc::channel();
    let (release, release_rx) = mpsc::channel();
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = Gate {
        started: started_tx,
        release: release_rx,
        calls: calls.clone(),
    };
    Gated {
        host: steady_host(sim, config(), Worker::spawn(gate, 8)),
        started,
        release,
        calls,
    }
}

/// Finding 2: shutting down waits (bounded) for the running call and books its billed
/// answer; a queued call is cancelled unsent. A fresh host over the saved history
/// sees the spend.
#[test]
fn shutdown_books_the_running_call_and_cancels_queued_ones() {
    let mut sim = sim(1);
    let Gated {
        mut host,
        started,
        release,
        calls,
    } = gated_host(&sim);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    // A second menu queues behind the running call.
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    assert_eq!(host.in_flight(), 2);

    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        release.send(()).unwrap();
    });
    let bound = host.drain_bound();
    assert!(bound >= Duration::from_secs(16), "{bound:?}");
    host.shutdown(&mut sim, bound);
    releaser.join().unwrap();
    let report = sim.step(&input()).unwrap();
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);

    assert_eq!(calls.load(Ordering::SeqCst), 1, "the queued call never ran");
    let s = spends(&sim);
    assert_eq!(s.len(), 1);
    assert_eq!((s[0].0.attempts, s[0].1, s[0].0.unsettled), (1, 20, false));
    let d = decisions(&sim);
    assert_eq!(d.len(), 2);
    assert_eq!(
        (d[0].source, d[0].choice.as_deref()),
        (Source::Model, Some("plan.2"))
    );
    assert_eq!(d[1].why, ["cancelled"]);
    // As after a load: the spend is in the record.
    let fresh = steady_host(&sim, config(), Inline::new(FakeMind::new(1)));
    assert!((fresh.spent_usd() - 0.00002).abs() < 1e-9);
    // And the host takes no new requests (a new situation: the same one is cached).
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
    assert_eq!(decisions(&sim)[2].why, ["shutdown"]);
}

/// Finding 2: a call still out when the wait ends is booked at its worst case, so
/// spend is never under-counted, and its moment takes the default.
#[test]
fn a_call_still_out_at_shutdown_is_booked_at_its_worst_case() {
    let mut sim = sim(1);
    let Gated {
        mut host,
        started,
        release,
        ..
    } = gated_host(&sim);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    host.shutdown(&mut sim, Duration::from_millis(20));
    release.send(()).unwrap();
    sim.step(&input()).unwrap();

    let s = spends(&sim);
    assert_eq!(s.len(), 1);
    let (note, cost) = &s[0];
    assert!(note.unsettled);
    // Both attempts the request was allowed, at $0.001 each.
    assert_eq!((note.attempts, *cost), (2, 2_000));
    assert_eq!(decisions(&sim)[0].why, ["shutdown"]);
    let fresh = steady_host(&sim, config(), Inline::new(FakeMind::new(1)));
    assert!((fresh.spent_usd() - 0.002).abs() < 1e-9);
}

/// An executor that refuses its first job, as a full queue would.
struct BusyOnce {
    inner: Inline,
    refused: bool,
}

impl Executor for BusyOnce {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        if !self.refused {
            self.refused = true;
            return false;
        }
        self.inner.submit(ticket, job)
    }
    fn poll(&mut self) -> Vec<Done> {
        self.inner.poll()
    }
    fn in_flight(&self) -> usize {
        self.inner.in_flight()
    }
}

/// Finding 3: a busy default is part of the recording, so a replay takes the default
/// at the same occurrence and the later answer lands where it did, state for state.
#[test]
fn a_replay_keeps_busy_defaults_in_place() {
    fn scenario(exec: impl Executor + 'static) -> Sim {
        let mut sim = sim(1);
        let mut host = steady_host(&sim, config(), exec);
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
        sim
    }
    let choices = |s: &Sim| {
        decisions(s)
            .into_iter()
            .map(|n| n.choice)
            .collect::<Vec<_>>()
    };
    let mock = Mock::default();
    mock.ok(jev_choice("o2"));
    let original = scenario(BusyOnce {
        inner: Inline::new(mock_mind(&mock)),
        refused: false,
    });
    let d = decisions(&original);
    assert_eq!(d[0].key, d[1].key, "the same situation twice");
    assert_eq!(d[0].why, ["busy"]);
    assert_eq!(
        choices(&original),
        [Some("plan.1".to_owned()), Some("plan.2".to_owned())]
    );

    let recording = Recording::from_history(original.history().facts());
    assert_eq!(recording.len(), 2, "the busy request and the call");
    let replay = scenario(Inline::new(RecordedMind::new(recording)));
    assert_eq!(choices(&replay), choices(&original));
    assert_eq!(spends(&replay), spends(&original));
    assert_eq!(replay.hash(), original.hash());
}

/// Finding 3: requests replay in the order they were made, not the order their
/// answers were booked.
#[test]
fn a_recording_follows_request_order() {
    let mut sim = sim(1);
    let exec = Manual::default();
    let mut host = steady_host(&sim, config(), exec.clone());
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    let tickets = exec.tickets();
    assert_eq!(tickets.len(), 2);
    // The second request is answered first.
    for (ticket, choice) in [(tickets[1], "o3"), (tickets[0], "o2")] {
        let pick = Decision::Choice {
            choice: choice.into(),
            confidence: None,
        };
        exec.finish(ticket, JobResult::Decide(Reply::ok(pick, 0.00002, "jev")));
        host.pump(&mut sim);
        sim.step(&input()).unwrap();
    }
    let key = decisions(&sim)[0].key.clone();
    let recording = Recording::from_history(sim.history().facts());
    let picks: Vec<_> = recording.calls[&(Stage::Pick, key)]
        .iter()
        .map(|o| o.answer.as_ref().unwrap()["choice"].clone())
        .collect();
    assert_eq!(picks, [json!("o2"), json!("o3")]);
}

/// Finding 4: with the Ninefold's brief as shipped, coming and going at one station
/// with nothing else changed is the same situation every time.
#[test]
fn routine_dockings_keep_the_cache_key() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o2"));
    let mut host = steady_host(&sim, config(), Inline::new(mock_mind(&mock)));
    for _ in 0..4 {
        sim.submit(
            CommandBatch::new(Cause::system("test", "undock")).with(Command::Emit {
                event: Event::Undock {
                    ship: NINEFOLD,
                    station: GLASS,
                },
            }),
        );
        sim.step(&input()).unwrap();
        dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    }
    let d = decisions(&sim);
    assert_eq!(d.len(), 4);
    assert_eq!(d[0].source, Source::Model);
    for n in &d[1..] {
        assert_eq!((n.source, &n.key), (Source::Cache, &d[0].key));
        assert_eq!(n.choice.as_deref(), Some("plan.2"));
    }
    assert_eq!(mock.calls().len(), 1);
}

/// Finding 5: an upstream error that echoes the key never shows it: not in the error's
/// text or `Debug`, not in the probe's output, not even cut in half by truncation.
#[test]
fn an_upstream_error_that_echoes_the_key_is_redacted() {
    const KEY: &str = "sk-or-v1-0123456789abcdefSECRET";
    let echo = |m: &str| format!("{m} {KEY}");
    // The key with its first letter as a JSON unicode escape: raw replacement can't see
    // it, and parsing restores it. Upstream text is never copied into errors, so it
    // can't come back out either.
    let escaped = format!(
        r#"{{"error":{{"code":400,"message":"Bearer s{}"}}}}"#,
        &KEY[1..]
    );
    let cases: [Vec<Result<HttpReply, MindError>>; 5] = [
        vec![Ok(HttpReply::new(200, escaped))],
        vec![Ok(HttpReply::new(
            401,
            json!({"error": {"message": echo("No auth credentials for")}}).to_string(),
        ))],
        vec![Ok(HttpReply::new(
            200,
            json!({"error": {"code": 400, "message": echo("Bearer")}}).to_string(),
        ))],
        // The key straddles the 200-character cut.
        vec![Ok(HttpReply::new(403, format!("{}{KEY}", "x".repeat(190))))],
        vec![
            Err(MindError::Transport(echo("reset after"))),
            Err(MindError::Transport(echo("reset again after"))),
        ],
    ];
    for replies in cases {
        let mock = Mock::default();
        for r in replies.clone() {
            mock.queue(r);
        }
        let mut mind = OpenRouterMind::with_transport(
            mock.clone(),
            ApiKey::new(KEY),
            Models::default(),
            &Limits::default(),
        );
        let err = mind.decide(&probe::sample_decision()).result.unwrap_err();
        for r in replies {
            mock.queue(r);
        }
        let probed = probe::decision(&mut mind);
        let shown = format!(
            "{err} {err:?} {probed:?} {}",
            serde_json::to_string(&probed).unwrap()
        );
        assert!(!shown.contains("sk-or-v1"), "{shown}");
        assert!(!shown.contains("SECRET"), "{shown}");
    }
}

/// The live line-pick probe, offline: whatever the pick, the text is an authored line.
#[test]
fn the_line_pick_probe_says_authored_lines() {
    let (_, said) = probe::sample_line_pick();
    for seed in 0..8 {
        let r = probe::line_pick(&mut FakeMind::new(seed));
        let answer = r.answer.unwrap();
        assert!(
            said.iter()
                .any(|(id, text)| answer["line"] == id.as_str() && answer["said"] == text.as_str()),
            "{answer}"
        );
    }
}

/// A drafted line (opt-in free phrasing) that echoes the key in a JSON escape: the key
/// is redacted from the decoded text and cites before anything stores or shows them.
#[test]
fn an_escaped_key_in_a_drafted_line_is_redacted_after_decoding() {
    const KEY: &str = "sk-or-v1-0123456789abcdefSECRET";
    let escaped = String::from(r"s") + &KEY[1..];
    let content = format!(r#"{{"text":"Ilse says {escaped} hi","cite":"{escaped}"}}"#);
    let mock = Mock::default();
    mock.ok(json!({ "choices": [{ "message": { "content": content } }] }));
    let mut mind = OpenRouterMind::with_transport(
        mock.clone(),
        ApiKey::new(KEY),
        Models::default(),
        &Limits::default(),
    );
    let sample = probe::sample_line(&config());
    let r = mind.line(&sample.req);
    let shown = format!(
        "{:?} {}",
        r.result,
        serde_json::to_string(&r.result.as_ref().ok()).unwrap()
    );
    assert!(!shown.contains("SECRET"), "{shown}");
}
