//! Real core Luau candidates -> MindHost/FakeMind -> the script's say/retirement path.
//! No network, window or device. Held executor results exercise asynchronous races.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::rc::Rc;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, DockTalkRequest, TalkLine};
use galaxy_core::{Save, Sim, SimState};
use galaxy_mind::{Done, Executor, FakeMind, Inline, Job, MindConfig, MindHost};
use galaxy_script::ScriptHost;

const PLAYER: ShipId = ShipId::from_static("ship.player");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");

fn core() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core")
}
fn config() -> MindConfig {
    MindConfig::load_dir(&core().join("data")).unwrap()
}

fn tick(sim: &mut Sim) {
    let r = sim.step(&PlayerInput::default()).unwrap();
    assert!(r.rejected.is_empty(), "{:?}", r.rejected);
    assert!(
        sim.world()
            .packs
            .values()
            .all(|p| p.health.values().all(|h| h.aborts == 0)),
        "script fault: {:?}",
        sim.world().packs
    );
}

fn attach(state: SimState) -> Sim {
    let mut sim = Sim::new(state);
    let scripts = ScriptHost::new(None);
    let report = scripts.load(&core(), sim.world(), sim.history());
    assert!(report.accepted(), "{report:?}");
    scripts.attach(&mut sim);
    sim
}

/// Stop with a committed request but no answer: two authored Ilse lines fit.
fn offered() -> Sim {
    let mut sim = attach(galaxy_core::new_game(&core().join("data"), 7).unwrap());
    for _ in 0..3 {
        tick(&mut sim);
    }
    let mut b = CommandBatch::new(Cause::system("test", "news and arrival"));
    for (kind, amount) in [
        ("core.stage_built", Some(1)),
        ("core.shortage_started", None),
    ] {
        b.commands.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::new(kind).unwrap(),
                subjects: Subjects {
                    station: Some(KETTLE),
                    ..Subjects::default()
                },
                amount,
                note: "Kettle news".into(),
            }),
        });
    }
    sim.submit(b);
    tick(&mut sim);
    // News predates the docking: the kernel records emitted events before notes
    // within a batch, so these must be separate commits.
    sim.submit(
        CommandBatch::new(Cause::system("test", "arrival")).with(Command::Emit {
            event: Event::Dock {
                ship: PLAYER,
                station: WHARF,
            },
        }),
    );
    tick(&mut sim);
    tick(&mut sim);
    assert_eq!(request(&sim).1.candidates.len(), 2);
    sim
}

fn request(sim: &Sim) -> (FactId, DockTalkRequest) {
    sim.history()
        .facts()
        .iter()
        .rev()
        .find_map(|f| match &f.payload {
            FactPayload::Event(Event::TalkRequested { request }) => Some((f.id, *request.clone())),
            _ => None,
        })
        .unwrap()
}

fn settle(sim: &mut Sim, host: &mut MindHost) {
    for _ in 0..4 {
        host.pump(sim);
        tick(sim);
    }
}

fn said(sim: &Sim, request: &DockTalkRequest) -> Vec<TalkLine> {
    sim.world()
        .talk
        .on(Channel::DockTalk)
        .filter(|s| {
            request
                .candidates
                .iter()
                .any(|c| s.line.cites.contains(&c.cite))
        })
        .map(|s| s.line.clone())
        .collect()
}

#[test]
fn fake_pick_changes_the_authored_line_and_never_adds_text() {
    let state = offered().state().clone();
    let mut choices = BTreeSet::new();
    for seed in 0..32 {
        let mut sim = attach(state.clone());
        let (_, req) = request(&sim);
        let mut cfg = config();
        cfg.talk.free_phrasing = true; // Script candidates remain pick-only even then.
        let mut mind = MindHost::new(cfg, Inline::new(FakeMind::new(seed)), sim.history());
        settle(&mut sim, &mut mind);
        let lines = said(&sim, &req);
        assert_eq!(lines.len(), 1);
        let chosen = req
            .candidates
            .iter()
            .find(|c| c.text == lines[0].text)
            .expect("only authored text");
        assert_eq!(lines[0].cites, vec![chosen.cite]);
        assert_eq!(lines[0].speaker.as_ref(), Some(&chosen.npc));
        choices.insert(chosen.line.clone());
        assert_eq!(mind.stats().model, 1);
        let decision = sim
            .history()
            .facts()
            .iter()
            .find(|f| f.kind.as_str() == "mind.decision")
            .unwrap();
        assert!(matches!(
            sim.history().cause(decision.cause).unwrap().cause.kind,
            CauseKind::Mind { .. }
        ));
        assert!(
            sim.history()
                .facts()
                .iter()
                .any(|f| f.kind.as_str() == "core.npc_line"
                    && f.subjects.other.as_ref() == Some(&chosen.line))
        );
        if choices.len() == 2 {
            break;
        }
    }
    assert_eq!(
        choices.len(),
        2,
        "Jev-style choice must affect what the script says"
    );
}

#[test]
fn disabled_or_exhausted_budget_uses_the_scripts_first_candidate() {
    for disabled in [false, true] {
        let mut sim = offered();
        let (_, req) = request(&sim);
        let mut cfg = config();
        if disabled {
            cfg.talk.pick = false;
        } else {
            cfg.budget.spend_cap_usd = 0.0;
        }
        let mut mind = MindHost::new(cfg, Inline::new(FakeMind::new(1)), sim.history());
        settle(&mut sim, &mut mind);
        assert_eq!(said(&sim, &req)[0].text, req.candidates[0].text);
        assert_eq!(mind.stats().requests, 0);
        assert_eq!(mind.stats().defaults, 1);
    }
}

#[test]
fn candidate_slots_are_filled_from_the_cited_fact_before_selection() {
    let mut sim = attach(galaxy_core::new_game(&core().join("data"), 7).unwrap());
    for _ in 0..3 {
        tick(&mut sim);
    }
    sim.submit(
        CommandBatch::new(Cause::system("test", "a recorded delivery"))
            .with(Command::RecordFact {
                fact: Box::new(FactDraft {
                    kind: FactKind::from_static("deliver"),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        other: Some(AnyId::from_static("ledger.kettle_greenhouse")),
                        ..Subjects::default()
                    },
                    amount: Some(37),
                    note: "37 ice delivered".into(),
                }),
            })
            .with(Command::SetShipDocked {
                ship: PLAYER,
                station: Some(KETTLE),
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: KETTLE,
                },
            }),
    );
    tick(&mut sim);
    tick(&mut sim);
    let (_, req) = request(&sim);
    assert_eq!(req.candidates.len(), 1);
    assert_eq!(
        req.candidates[0].text,
        "37 ice. The beans will hear about this."
    );
    assert_eq!(
        sim.history().fact(req.candidates[0].cite).unwrap().amount,
        Some(37)
    );
    let mut mind = MindHost::new(config(), Inline::new(FakeMind::new(1)), sim.history());
    settle(&mut sim, &mut mind);
    assert_eq!(said(&sim, &req)[0].text, req.candidates[0].text);
}

struct Held {
    inner: Inline,
    ready: Rc<RefCell<bool>>,
}
impl Executor for Held {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        self.inner.submit(ticket, job)
    }
    fn poll(&mut self) -> Vec<Done> {
        if *self.ready.borrow() {
            self.inner.poll()
        } else {
            vec![]
        }
    }
    fn in_flight(&self) -> usize {
        self.inner.in_flight()
    }
}

fn held(sim: &Sim, deadline: u64) -> (MindHost, Rc<RefCell<bool>>) {
    let ready = Rc::new(RefCell::new(false));
    let exec = Held {
        inner: Inline::new(FakeMind::new(5)),
        ready: ready.clone(),
    };
    let mut cfg = config();
    cfg.limits.deadline_ticks = deadline;
    (MindHost::new(cfg, exec, sim.history()), ready)
}

#[test]
fn timeout_says_default_once_and_books_the_late_fake_answer() {
    let mut sim = offered();
    let (_, req) = request(&sim);
    let (mut mind, ready) = held(&sim, 1);
    settle(&mut sim, &mut mind);
    assert_eq!(said(&sim, &req)[0].text, req.candidates[0].text);
    *ready.borrow_mut() = true;
    settle(&mut sim, &mut mind);
    assert_eq!(said(&sim, &req).len(), 1);
    assert_eq!(mind.stats().late_dropped, 1);
    assert!(mind.spent_usd() > 0.0);
}

#[test]
fn old_docking_and_already_cited_answers_are_not_spoken() {
    for redock in [false, true] {
        let mut sim = offered();
        let (_, req) = request(&sim);
        let (mut mind, ready) = held(&sim, 100);
        mind.pump(&mut sim);
        let mut b = CommandBatch::new(Cause::system("test", "the moment changes"));
        if redock {
            // Even returning to the same station must not revive an old answer.
            b.commands.push(Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station: WHARF,
                },
            });
            b.commands.push(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: WHARF,
                },
            });
        } else {
            for c in &req.candidates {
                b.commands.push(Command::Say {
                    line: Box::new(TalkLine {
                        channel: Channel::DockTalk,
                        speaker: Some(c.npc.clone()),
                        place: Some(WHARF),
                        text: c.text.clone(),
                        cites: vec![c.cite],
                        about: vec![],
                        choices: vec![],
                    }),
                });
            }
        }
        sim.submit(b);
        tick(&mut sim);
        *ready.borrow_mut() = true;
        settle(&mut sim, &mut mind);
        assert_eq!(
            said(&sim, &req).len(),
            if redock { 0 } else { 2 },
            "lines: {:?}",
            said(&sim, &req)
        );
    }
}

#[test]
fn save_load_resumes_requests_and_does_not_repeat_pending_results() {
    for phase in 0..3 {
        let mut sim = offered();
        let (_, req) = request(&sim);
        if phase > 0 {
            let mut mind = MindHost::new(config(), Inline::new(FakeMind::new(1)), sim.history());
            mind.pump(&mut sim); // Result queued but not yet committed.
            if phase == 2 {
                tick(&mut sim);
            } // Result committed; script has not read it.
        }
        let dir = tempfile::tempdir().unwrap();
        let save = Save::create(dir.path().join("talk.sqlite"), sim.state()).unwrap();
        let mut restored = attach(save.load().unwrap());
        let mut mind = MindHost::new(config(), Inline::new(FakeMind::new(1)), restored.history());
        settle(&mut restored, &mut mind);
        assert_eq!(said(&restored, &req).len(), 1);
        assert_eq!(
            restored
                .history()
                .facts()
                .iter()
                .filter(|f| f.kind.as_str() == "mind.decision")
                .count(),
            1
        );
        assert_eq!(mind.stats().requests, u64::from(phase == 0));
    }
}

#[test]
fn same_tick_departure_prevents_speech_and_retirement() {
    let mut sim = offered();
    let (_, req) = request(&sim);
    let mut mind = MindHost::new(config(), Inline::new(FakeMind::new(1)), sim.history());
    mind.pump(&mut sim);
    tick(&mut sim); // Result is now the script's next input.
    sim.submit(
        CommandBatch::new(Cause::system("test", "depart before the line applies"))
            .with(Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            })
            .with(Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station: WHARF,
                },
            }),
    );
    settle(&mut sim, &mut mind);
    assert!(said(&sim, &req).is_empty());
    assert!(!sim.history().facts().iter().any(|f| {
        f.kind.as_str() == "core.npc_line"
            && req
                .candidates
                .iter()
                .any(|c| f.subjects.other.as_ref() == Some(&c.line))
    }));
}
