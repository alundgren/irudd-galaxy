//! Kernel tests: determinism, save/load, pause, atomic batches, undo and rollback.
//! All headless.

use std::path::{Path, PathBuf};

use galaxy_contract::pack::{ScriptHealth, ScriptValue};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::{Save, Sim, SimState, System, TickCtx, default_systems, new_game};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const PLAYER: ShipId = ShipId::from_static("ship.player");
const ICE: WareId = WareId::from_static("ware.ice");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");

/// Uses its RNG stream every tick, so the hash covers RNG state too.
struct Noise;

impl System for Noise {
    fn name(&self) -> &'static str {
        "test_noise"
    }
    fn step(&self, ctx: &mut TickCtx<'_>) {
        if ctx.rng().chance(0.2) {
            let delta = i64::from(ctx.rng().range_u32(1, 5));
            ctx.submit(
                CommandBatch::new(Cause::system("test_noise", "noise"))
                    .with(Command::AdjustCredits { delta }),
            );
        }
    }
}

fn sim(seed: u64) -> Sim {
    let state = new_game(&data_dir(), seed).expect("core data loads");
    let mut sim = Sim::new(state).with_systems(default_systems());
    sim.add_system(Box::new(Noise));
    sim
}

fn resume(state: SimState) -> Sim {
    let mut sim = Sim::new(state).with_systems(default_systems());
    sim.add_system(Box::new(Noise));
    sim
}

/// A scripted input stream: undock and fly toward Kettle Dock, then try to dock.
fn input_at(tick: u64) -> PlayerInput {
    let phase = tick % 240;
    PlayerInput {
        thrust: match phase {
            0..=89 => Vec2::new(0.8, -0.6),
            90..=119 => Vec2::new(0.0, 1.0),
            120..=179 => Vec2::new(-1.0, 0.2),
            _ => Vec2::ZERO,
        },
        action: phase >= 200,
        click: None,
    }
}

fn run_to(sim: &mut Sim, until: u64) {
    while sim.tick().0 < until {
        let t = sim.tick().0;
        sim.step(&input_at(t)).expect("not paused");
    }
}

#[test]
fn core_data_builds_the_lantern_reach() {
    let state = new_game(&data_dir(), 1).unwrap();
    let w = &state.world;
    assert_eq!(w.regions.len(), 3);
    assert_eq!(w.stations.len(), 5);
    assert_eq!(w.wares.len(), 6);
    assert_eq!(w.rock_types.len(), 3);
    assert!(w.rocks.contains("rock.shallows.ice.03"));
    assert!(w.lanes.len() >= 2);
    assert_eq!(w.ships.get(&PLAYER).unwrap().docked_at, Some(WHARF));
    assert!(w.markets.contains(&KETTLE));
    // The authored layout doesn't depend on the seed.
    let other = new_game(&data_dir(), 2).unwrap();
    assert_eq!(w.rocks, other.world.rocks);
}

#[test]
fn core_manifest_parses() {
    let text = std::fs::read_to_string(data_dir().join("../manifest.toml")).unwrap();
    let m: galaxy_contract::pack::PackManifest = toml::from_str(&text).unwrap();
    assert_eq!(m.id.as_str(), "core");
}

#[test]
fn same_seed_and_inputs_give_the_same_hash() {
    let mut a = sim(42);
    let mut b = sim(42);
    for checkpoint in [1, 50, 200, 600] {
        run_to(&mut a, checkpoint);
        run_to(&mut b, checkpoint);
        assert_eq!(a.hash(), b.hash(), "diverged by tick {checkpoint}");
    }
    // The run actually did things: moved, docked, made noise.
    assert!(
        a.history()
            .count(&HistoryQuery::kind(EventKind::Undock.fact_kind()))
            > 0
    );
    assert!(a.world().player.credits != 40);

    let mut c = sim(43);
    run_to(&mut c, 600);
    assert_ne!(a.hash(), c.hash(), "seed must matter");
}

#[test]
fn save_load_continue_matches_uninterrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.sqlite");

    let mut straight = sim(7);
    run_to(&mut straight, 500);

    let mut first = sim(7);
    run_to(&mut first, 230);
    let hash_at_save = first.hash();
    Save::create(&path, first.state()).unwrap();
    drop(first);

    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut second = resume(loaded);
    assert_eq!(
        second.hash(),
        hash_at_save,
        "load must restore the exact state"
    );
    run_to(&mut second, 500);
    assert_eq!(second.hash(), straight.hash());

    // Writing again appends only the new history rows.
    let mut save = Save::open(&path).unwrap();
    save.write(second.state()).unwrap();
    let again = save.load().unwrap();
    assert_eq!(galaxy_core::state_hash(&again), straight.hash());
    assert_eq!(
        save.info().unwrap().facts as usize,
        straight.history().len()
    );
}

#[test]
fn pause_stops_ticks() {
    let mut s = sim(1);
    run_to(&mut s, 10);
    let before = s.hash();
    s.set_paused(true);
    assert!(s.step(&input_at(10)).is_none());
    assert_eq!(s.run(100, &input_at(10)), 0);
    assert_eq!(s.tick(), Tick(10));
    assert_eq!(s.hash(), before, "nothing changes while paused");
    s.set_paused(false);
    assert!(s.step(&input_at(10)).is_some());
    assert_eq!(s.tick(), Tick(11));
}

#[test]
fn batches_apply_all_or_nothing() {
    let mut s = Sim::new(new_game(&data_dir(), 1).unwrap());
    let before = s.world().clone();

    // Second command fails: unknown ware. The credit change must not stick.
    s.submit(
        CommandBatch::new(Cause::system("test", "bad"))
            .with(Command::AdjustCredits { delta: 100 })
            .with(Command::AdjustCargo {
                ship: PLAYER,
                ware: WareId::new("ware.nothing").unwrap(),
                delta: 1,
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: WHARF,
                },
            }),
    );
    // An invariant failure: more cargo than the Wren holds.
    s.submit(
        CommandBatch::new(Cause::system("test", "overfull")).with(Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 1000,
        }),
    );
    // A good batch in the same tick still applies.
    s.submit(
        CommandBatch::new(Cause::system("test", "good")).with(Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 3,
        }),
    );
    let report = s.step(&PlayerInput::default()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.rejected.len(), 2);
    assert_eq!(report.rejected[0].command, Some(1));
    assert!(report.rejected[1].reason.contains("hold_within_capacity"));

    let w = s.world();
    assert_eq!(w.player.credits, before.player.credits);
    assert_eq!(w.ships.get(&PLAYER).unwrap().hold.count(&ICE), 3);
    assert!(s.history().is_empty(), "rejected events never go out");
    assert!(w.counters.player.is_empty());
}

#[test]
fn undo_records_invert_commands() {
    let mut world = new_game(&data_dir(), 1).unwrap().world;
    let original = world.clone();
    let mut kettle = world.stations.get(&KETTLE).unwrap().clone();
    kettle.id = StationId::new("gm.sixth_station").unwrap();
    kettle.name = "The sixth station".into();
    let commands = vec![
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 5,
        },
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: -5,
        },
        Command::AdjustCredits { delta: 25 },
        Command::SetStationStage {
            station: KETTLE,
            stage: 2,
        },
        Command::SetStationPart {
            station: KETTLE,
            part: PartId::new("part.kettle.dome_b").unwrap(),
            attached: true,
        },
        Command::SetStationOwner {
            station: KETTLE,
            owner: None,
        },
        Command::AdjustStanding {
            faction: COOP,
            delta: 3,
        },
        Command::SetFlag {
            flag: FlagId::new("flag.nav_chip").unwrap(),
            on: true,
        },
        Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        },
        Command::SetShipMotion {
            ship: PLAYER,
            position: Vec2::new(10.0, 20.0),
            velocity: Vec2::new(1.0, 0.0),
            heading: 0.5,
        },
        Command::AdjustRock {
            rock: RockId::new("rock.shallows.ice.03").unwrap(),
            delta: -7,
        },
        Command::SetMarketLine {
            station: KETTLE,
            ware: ICE,
            line: None,
        },
        Command::PutStation {
            id: kettle.id.clone(),
            value: Some(Box::new(kettle)),
        },
        Command::PutRumor {
            id: RumorId::new("rumor.far_belt_glassite").unwrap(),
            value: Some(Box::new(Rumor {
                id: RumorId::new("rumor.far_belt_glassite").unwrap(),
                claim: "a rich glassite vein in the far belt".into(),
                is_true: false,
                planted_by: Some(FactionId::new("faction.tollway_compact").unwrap()),
                heard_at: [KETTLE].into(),
                heard_by_player: false,
                source: None,
            })),
        },
        Command::SetScriptValue {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            script: "kettle_growth".into(),
            store: "progress".into(),
            key: "announced".into(),
            value: Some(ScriptValue::Integer(2)),
        },
        Command::SetTriggerFired {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            trigger: "kettle_growth.regulars".into(),
            fired: 1,
        },
        Command::SetScriptHealth {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            script: "kettle_growth".into(),
            health: ScriptHealth {
                revision: "r1".into(),
                aborts: 1,
                quarantined: false,
                quarantined_before: ["r0".to_owned()].into(),
            },
        },
        Command::SetRevisionRetired {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            revision: "r1".into(),
            retired: true,
        },
        Command::Say {
            line: Box::new(TalkLine {
                channel: Channel::DockTalk,
                speaker: Some(NpcId::new("npc.ilse").unwrap()),
                place: Some(WHARF),
                text: "Kettle lit another ring.".into(),
                cites: vec![FactId(1)],
                about: Vec::new(),
            }),
        },
    ];
    let mut staged = galaxy_core::apply::Staged::default();
    let mut undo = Vec::new();
    for c in &commands {
        assert_eq!(c.reversibility(), Reversibility::Reversible);
        undo.push(galaxy_core::apply::apply(&mut world, c, &mut staged).unwrap());
    }
    assert_ne!(world, original);
    for u in undo.iter().rev() {
        let UndoRecord::Invert { command } = u else {
            panic!("reversible commands invert");
        };
        galaxy_core::apply::apply(&mut world, command, &mut staged).unwrap();
    }
    assert_eq!(world, original);

    let emit = Command::Emit {
        event: Event::SessionStart {},
    };
    assert_eq!(emit.reversibility(), Reversibility::CompensateOnly);
    assert!(matches!(
        galaxy_core::apply::apply(&mut world, &emit, &mut staged).unwrap(),
        UndoRecord::Compensate { .. }
    ));
}

#[test]
fn rollback_by_cause_restores_script_writes() {
    let mut s = Sim::new(new_game(&data_dir(), 1).unwrap());
    let before = s.world().clone();
    let pack = PackId::new("gm").unwrap();
    s.submit(
        CommandBatch::new(Cause::script(pack.clone(), "r1", "kettle grows"))
            .with(Command::SetStationStage {
                station: KETTLE,
                stage: 2,
            })
            .with(Command::SetStationPart {
                station: KETTLE,
                part: PartId::new("part.kettle.dome_b").unwrap(),
                attached: true,
            })
            .with(Command::RecordFact {
                fact: Box::new(galaxy_contract::history::FactDraft {
                    kind: FactKind::new("gm.kettle.stage_up").unwrap(),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        ..Subjects::default()
                    },
                    amount: None,
                    note: "greenhouse stage 2".into(),
                }),
            }),
    );
    // A player write in the same tick is not journaled and survives the rollback.
    s.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Sell, "sell"))
            .with(Command::AdjustCredits { delta: 9 }),
    );
    s.step(&PlayerInput::default()).unwrap();
    assert_eq!(s.world().stations.get(&KETTLE).unwrap().build.stage, 2);
    assert_eq!(s.journal().len(), 1);

    let causes = s.causes_where(
        |c| matches!(&c.cause.kind, CauseKind::Script { pack: p, revision } if *p == pack && revision == "r1"),
    );
    assert_eq!(causes.len(), 1);
    let plan = s.queue_rollback(&causes, "the dome order fell through");
    assert_eq!(plan.batches, 1);
    assert_eq!(plan.compensate.len(), 1, "the fact can't be unsaid");
    s.step(&PlayerInput::default()).unwrap();

    let w = s.world();
    assert_eq!(w.stations, before.stations);
    assert_eq!(w.player.credits, before.player.credits + 9);
    let fact = s.history().facts().last().unwrap();
    assert_eq!(fact.kind.as_str(), "gm.kettle.stage_up");
    assert_eq!(
        s.history().cause(fact.cause).unwrap().cause.summary,
        "kettle grows"
    );
}

#[test]
fn events_become_facts_and_counters() {
    let mut s = sim(3);
    // Undock, drift a little, come back, dock.
    s.step(&PlayerInput {
        thrust: Vec2::new(1.0, 0.0),
        ..PlayerInput::default()
    })
    .unwrap();
    for _ in 0..5 {
        s.step(&PlayerInput {
            thrust: Vec2::new(1.0, 0.0),
            ..PlayerInput::default()
        });
    }
    let dock = PlayerInput {
        action: true,
        ..PlayerInput::default()
    };
    s.step(&dock).unwrap();
    let sub = s.subscribe("test");
    s.step(&dock).unwrap(); // already docked: nothing
    assert!(s.drain_events(&sub).is_empty());

    let w = s.world();
    assert_eq!(w.ships.get(&PLAYER).unwrap().docked_at, Some(WHARF));
    let dockings = |scope| w.counters.get(CounterKind::Dockings, &scope);
    assert_eq!(dockings(CounterScope::Player), 1);
    assert_eq!(dockings(CounterScope::Station { station: WHARF }), 1);
    assert_eq!(
        dockings(CounterScope::Region {
            region: RegionId::new("region.lantern_shallows").unwrap()
        }),
        1
    );
    let q = HistoryQuery {
        kind: Some(EventKind::Dock.fact_kind()),
        station: Some(WHARF),
        ..HistoryQuery::default()
    };
    let fact = s.history().last(&q).unwrap();
    assert!(matches!(
        fact.payload,
        FactPayload::Event(Event::Dock { .. })
    ));
    // The envelope systems saw carries the fact id.
    let last = &s.state().kernel.last_events;
    assert!(last.is_empty(), "the second dock tick emitted nothing");
}

#[test]
fn history_sql_matches_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.sqlite");
    let mut s = sim(11);
    run_to(&mut s, 720);
    let save = Save::create(&path, s.state()).unwrap();
    assert!(!s.history().is_empty());
    let queries = [
        HistoryQuery::default(),
        HistoryQuery::kind(EventKind::Dock.fact_kind()),
        HistoryQuery {
            station: Some(KETTLE),
            ..HistoryQuery::default()
        },
        HistoryQuery {
            ship: Some(PLAYER),
            since: Some(Tick(300)),
            limit: Some(2),
            ..HistoryQuery::default()
        },
    ];
    for q in &queries {
        let mem: Vec<Fact> = s.history().find(q).into_iter().cloned().collect();
        assert_eq!(save.query_history(q).unwrap(), mem, "{q:?}");
    }
}

#[test]
fn snapshots_restore_and_forks_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sqlite");
    let mut s = sim(5);
    run_to(&mut s, 100);
    let mut save = Save::create(&path, s.state()).unwrap();
    let snap = save.snapshot(s.state(), "before change").unwrap();
    let hash_at_snap = s.hash();
    run_to(&mut s, 700);
    save.write(s.state()).unwrap();

    let fork = dir.path().join("fork.sqlite");
    galaxy_core::fork(&path, &fork).unwrap();
    assert!(galaxy_core::fork(&path, &fork).is_err(), "never overwrite");
    assert_eq!(Save::open(&fork).unwrap().info().unwrap().hash, s.hash());

    let restored = save.load_snapshot(snap).unwrap();
    assert_eq!(galaxy_core::state_hash(&restored), hash_at_snap);
    save.write(&restored).unwrap();
    let back = save.load().unwrap();
    assert_eq!(galaxy_core::state_hash(&back), hash_at_snap);
    assert_eq!(save.snapshots().unwrap().len(), 1);
}

#[test]
fn bad_data_is_rejected_with_reasons() {
    let dir = tempfile::tempdir().unwrap();
    for f in galaxy_contract::data::DATA_FILES {
        std::fs::copy(data_dir().join(f), dir.path().join(f)).unwrap();
    }
    let stations = std::fs::read_to_string(dir.path().join("stations.toml")).unwrap();
    std::fs::write(
        dir.path().join("stations.toml"),
        stations.replace("region.cinder_drift", "region.nowhere"),
    )
    .unwrap();
    let err = new_game(dir.path(), 1).unwrap_err().to_string();
    assert!(err.contains("region.nowhere"), "{err}");

    std::fs::write(
        dir.path().join("wares.toml"),
        "[[ware]]\nid = \"ware.x\"\nnmae = \"typo\"\n",
    )
    .unwrap();
    let err = new_game(dir.path(), 1).unwrap_err().to_string();
    assert!(err.contains("wares.toml"), "{err}");
}
