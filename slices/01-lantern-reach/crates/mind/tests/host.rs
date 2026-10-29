//! The fast-model layer end to end, headless and offline: a real sim, the core pack's
//! data and `mind.toml`, and either the seeded fake mind or OpenRouterMind over a mock
//! HTTP layer. No test here touches the network.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_mind::config::{Budget, Limits, Models};
use galaxy_mind::executor::{Done, Executor, Job};
use galaxy_mind::situation::{OutcomeNote, Source};
use galaxy_mind::{
    ApiKey, DockTalk, FakeMind, HttpReply, Inline, LinePool, MindConfig, MindError, MindHost,
    OpenRouterMind, PlanOption, PlanSource, PoolLine, RecordedMind, Recording, TraderMenus,
    Transport,
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

/// A new game with the Ninefold docked at Cinder Hook.
fn sim(seed: u64) -> Sim {
    let state = galaxy_core::new_game(&data_dir(), seed).expect("core data loads");
    let mut sim = Sim::new(state);
    let ship = Ship {
        id: NINEFOLD,
        name: "Ninefold".into(),
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
    };
    sim.submit(
        CommandBatch::new(Cause::system("test", "setup")).with(Command::PutShip {
            id: NINEFOLD,
            value: Some(Box::new(ship)),
        }),
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

/// Three plans: load 1, 2 or 3 ice. `vary` makes each docking a new situation.
struct Plans {
    vary: bool,
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
                commands: vec![Command::AdjustCargo {
                    ship: ship.clone(),
                    ware: ICE,
                    delta: i64::from(n),
                }],
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

fn trader_host(sim: &Sim, config: MindConfig, exec: impl Executor + 'static) -> MindHost {
    let mut host = MindHost::new(config, exec, sim.history());
    host.add_provider(TraderMenus {
        plans: Plans { vary: true },
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
}

impl Mock {
    fn queue(&self, reply: Result<HttpReply, MindError>) {
        self.0.lock().unwrap().replies.push_back(reply);
    }
    fn ok(&self, body: serde_json::Value) {
        self.queue(Ok(HttpReply {
            status: 200,
            body: body.to_string(),
        }));
    }
    fn status(&self, status: u16) {
        self.queue(Ok(HttpReply {
            status,
            body: "{}".into(),
        }));
    }
    fn calls(&self) -> Vec<String> {
        self.0.lock().unwrap().calls.clone()
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
    // The spend is tallied from the fact's amount (micro-dollars).
    let fact = sim
        .history()
        .facts()
        .iter()
        .find(|f| f.kind.as_str() == "mind.decision")
        .unwrap();
    assert_eq!(fact.amount, Some(20));
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
    mock.queue(Ok(HttpReply {
        status: 200,
        body: "this is not json".into(),
    }));
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
    let mut host = MindHost::new(config(), Inline::new(mock_mind(&mock)), sim.history());
    host.add_provider(TraderMenus {
        plans: Plans { vary: false },
    });
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);

    let d = decisions(&sim);
    assert_eq!(d[0].source, Source::Model);
    assert_eq!(d[1].source, Source::Cache);
    assert_eq!(d[0].key, d[1].key);
    assert_eq!(d[1].choice.as_deref(), Some("plan.2"));
    assert_eq!(d[1].calls, 0);
    assert_eq!(mock.calls().len(), 1);
}

#[test]
fn the_cache_survives_a_new_host_over_the_same_history() {
    let mut sim = sim(1);
    let mock = Mock::default();
    mock.ok(jev_choice("o3"));
    let mut host = MindHost::new(config(), Inline::new(mock_mind(&mock)), sim.history());
    host.add_provider(TraderMenus {
        plans: Plans { vary: false },
    });
    dock(&mut sim, &mut host, &NINEFOLD, &GLASS);
    // As after a load: a fresh host reads the cache and the tally back from history.
    let fresh = Mock::default();
    let mut host = MindHost::new(config(), Inline::new(mock_mind(&fresh)), sim.history());
    host.add_provider(TraderMenus {
        plans: Plans { vary: false },
    });
    assert!((host.spent_usd() - 0.00002).abs() < 1e-9);
    dock(&mut sim, &mut host, &NINEFOLD, &HOOK);
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
    assert!(!recording.choices.is_empty());
    let replay = scenario(Inline::new(RecordedMind::new(recording)));
    let choices = |s: &Sim| {
        decisions(s)
            .into_iter()
            .map(|n| n.choice)
            .collect::<Vec<_>>()
    };
    assert_eq!(choices(&original), choices(&replay));
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

fn talk_host(sim: &Sim, exec: impl Executor + 'static) -> MindHost {
    let mut host = MindHost::new(config(), exec, sim.history());
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

#[test]
fn dock_talk_phrases_a_line_that_cites_its_fact() {
    let mut sim = sim(1);
    let mock = Mock::default();
    let mut host = talk_host(&sim, Inline::new(mock_mind(&mock)));
    let fact = deliver_and_dock(&mut sim);
    mock.ok(chat_line(
        "Twelve ice. The beans will hear about it, I promise.",
        &fact.to_string(),
    ));
    let report = host.pump(&mut sim);
    sim.step(&input()).unwrap();

    assert_eq!(report.lines.len(), 1);
    let line = &report.lines[0];
    assert_eq!(line.source, Source::Model);
    assert_eq!(
        line.talk.text,
        "Twelve ice. The beans will hear about it, I promise."
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
    let mut host = talk_host(&sim, Inline::new(mock_mind(&mock)));
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
    assert!(n.why.iter().any(|w| w.contains("fact.999")), "{:?}", n.why);
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
    let mut host = talk_host(&sim, Inline::new(mock_mind(&mock)));
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
