//! Regressions from the review of PR #13: snapshot completeness, saving a resumed
//! snapshot, undo of rejected batches, rollback that applies once, non-finite numbers
//! and integer boundaries. All headless.

use std::path::{Path, PathBuf};

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState, TickReport, new_game, state_hash};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

const PLAYER: ShipId = ShipId::from_static("ship.player");
const ICE: WareId = WareId::from_static("ware.ice");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");
const ROCK: RockId = RockId::from_static("rock.shallows.ice.03");

fn fresh() -> Sim {
    Sim::new(new_game(&data_dir(), 7).unwrap())
}

fn step(s: &mut Sim) -> TickReport {
    s.step(&PlayerInput::default()).unwrap()
}

fn script(note: &str) -> CommandBatch {
    CommandBatch::new(Cause::script(PackId::new("gm").unwrap(), "r1", note))
}

fn fact(note: &str) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: FactKind::new("gm.test.note").unwrap(),
            subjects: Subjects::default(),
            amount: None,
            note: note.into(),
        }),
    }
}

fn notes(state: &SimState) -> Vec<String> {
    state
        .history
        .facts()
        .iter()
        .map(|f| match &f.payload {
            FactPayload::Note(n) => n.clone(),
            FactPayload::Event(e) => e.kind().as_str().to_owned(),
        })
        .collect()
}

// --- 1. Snapshots are complete ---

#[test]
fn snapshot_of_unsaved_state_keeps_its_logs() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fresh();
    let mut save = Save::create(dir.path().join("s.sqlite"), s.state()).unwrap();
    s.submit(script("unsaved").with(fact("unsaved")));
    step(&mut s);
    assert_eq!(s.history().len(), 1);

    // Snapshot before the next write: the fact, its cause and journal live only in memory.
    let snap = save.snapshot(s.state(), "unsaved").unwrap();
    let back = save.load_snapshot(snap).unwrap();
    assert_eq!(back, *s.state());
    assert_eq!(back.history.len(), 1);
    assert_eq!(back.history.causes().len(), 1);
    assert_eq!(back.journal.len(), 1);
    assert_eq!(state_hash(&back), s.hash());
}

#[test]
fn restoring_one_snapshot_leaves_the_others_intact() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fresh();
    s.submit(script("a").with(fact("before a")));
    step(&mut s);
    let mut save = Save::create(dir.path().join("s.sqlite"), s.state()).unwrap();
    let a = save.snapshot(s.state(), "a").unwrap();
    let state_a = s.state().clone();

    s.submit(script("b").with(fact("before b")));
    step(&mut s);
    save.write(s.state()).unwrap();
    let b = save.snapshot(s.state(), "b").unwrap();
    let state_b = s.state().clone();

    // Hard-restore A: the file drops B's later rows from its own tables.
    save.write(&save.load_snapshot(a).unwrap()).unwrap();
    assert_eq!(save.load().unwrap(), state_a);
    assert_eq!(save.snapshots().unwrap().len(), 2);

    // B is still listed and still restores exactly.
    assert_eq!(save.load_snapshot(b).unwrap(), state_b);
    save.write(&state_b).unwrap();
    assert_eq!(save.load().unwrap(), state_b);
    assert_eq!(save.load_snapshot(a).unwrap(), state_a);

    // And again after the file moves on to a third timeline.
    let mut c = Sim::new(state_a.clone());
    c.submit(script("c").with(fact("timeline c")));
    step(&mut c);
    save.write(c.state()).unwrap();
    assert_eq!(save.load_snapshot(a).unwrap(), state_a);
    assert_eq!(save.load_snapshot(b).unwrap(), state_b);
    assert_eq!(save.load().unwrap(), *c.state());
}

// --- 2. Saving a resumed snapshot drops the discarded timeline ---

#[test]
fn saving_a_resumed_snapshot_replaces_the_old_timeline() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fresh();
    let mut save = Save::create(dir.path().join("s.sqlite"), s.state()).unwrap();
    let empty = save.snapshot(s.state(), "empty").unwrap();

    s.submit(
        script("old")
            .with(Command::AdjustCredits { delta: 5 })
            .with(fact("old timeline")),
    );
    step(&mut s);
    save.write(s.state()).unwrap();

    // Restore, then continue before any write: same sequence numbers, other contents.
    let mut resumed = Sim::new(save.load_snapshot(empty).unwrap());
    resumed.submit(
        script("new")
            .with(Command::SetFlag {
                flag: FlagId::new("flag.new").unwrap(),
                on: true,
            })
            .with(fact("new timeline")),
    );
    step(&mut resumed);
    save.write(resumed.state()).unwrap();

    let back = save.load().unwrap();
    assert_eq!(notes(&back), ["new timeline"]);
    assert_eq!(back, *resumed.state(), "logs, causes and journal all match");
    assert_eq!(state_hash(&back), resumed.hash());
    let entry = &back.journal[0].undo[0];
    assert!(
        matches!(
            entry,
            UndoRecord::Invert {
                command: Command::SetFlag { .. }
            }
        ),
        "journal from the new timeline: {entry:?}"
    );
}

// --- 3. Rejecting a batch can always undo it ---

#[test]
fn rejected_batch_with_a_temporary_dangling_reference_is_undone() {
    let mut s = fresh();
    let before = s.world().clone();
    let original = before.ships.get(&PLAYER).unwrap().clone();
    let mut moved = original.clone();
    moved.region = RegionId::new("region.not_yet_created").unwrap();
    s.submit(
        CommandBatch::new(Cause::system("test", "dangling for a moment"))
            .with(Command::PutShip {
                id: PLAYER,
                value: Some(Box::new(moved)),
            })
            .with(Command::SetShipRegion {
                ship: PLAYER,
                region: original.region.clone(),
            })
            .with(Command::AdjustCredits { delta: -100 }),
    );
    s.submit(
        CommandBatch::new(Cause::system("test", "fine")).with(Command::AdjustCredits { delta: 1 }),
    );
    let report = step(&mut s);
    assert_eq!(report.applied, 1);
    assert_eq!(report.rejected.len(), 1);
    assert_eq!(report.rejected[0].command, Some(2));
    assert_eq!(s.world().ships, before.ships);
    assert_eq!(s.world().player.credits, before.player.credits + 1);
}

// --- 4. A rollback applies once ---

fn credits(s: &Sim) -> i64 {
    s.world().player.credits
}

#[test]
fn repeated_rollback_does_not_invert_twice() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = fresh();
    assert_eq!(credits(&s), 40);
    s.submit(script("plus ten").with(Command::AdjustCredits { delta: 10 }));
    step(&mut s);
    assert_eq!(credits(&s), 50);
    let cause = s.journal()[0].cause;

    assert_eq!(s.queue_rollback(&[cause], "undo").batches, 1);
    step(&mut s);
    assert_eq!(credits(&s), 40);

    // Again: nothing to do.
    assert_eq!(s.queue_rollback(&[cause], "again").batches, 0);
    step(&mut s);
    assert_eq!(credits(&s), 40);

    // Not after a save and load either.
    let path = dir.path().join("s.sqlite");
    Save::create(&path, s.state()).unwrap();
    let mut s = Sim::new(Save::open(&path).unwrap().load().unwrap());
    assert_eq!(s.queue_rollback(&[cause], "after load").batches, 0);

    // A hand-made duplicate rollback batch is rejected.
    s.submit(
        CommandBatch::new(Cause {
            kind: CauseKind::Rollback { of: vec![cause] },
            summary: "by hand".into(),
            cites: Vec::new(),
        })
        .with(Command::AdjustCredits { delta: -10 }),
    );
    let report = step(&mut s);
    assert_eq!(report.rejected.len(), 1);
    assert!(report.rejected[0].reason.contains("already rolled back"));
    assert_eq!(credits(&s), 40);
}

#[test]
fn rollback_queued_twice_or_overlapping_applies_once() {
    let mut s = fresh();
    s.submit(script("one").with(Command::AdjustCredits { delta: 10 }));
    s.submit(script("two").with(Command::AdjustCredits { delta: 5 }));
    step(&mut s);
    assert_eq!(credits(&s), 55);
    let (one, two) = (s.journal()[0].cause, s.journal()[1].cause);

    // Twice before stepping, then an overlapping selection.
    assert_eq!(s.queue_rollback(&[one], "first").batches, 1);
    assert_eq!(s.queue_rollback(&[one], "duplicate").batches, 0);
    assert_eq!(s.queue_rollback(&[one, two], "overlap").batches, 1);
    let report = step(&mut s);
    assert_eq!(report.applied, 2);
    assert_eq!(credits(&s), 40);
    assert_eq!(s.queue_rollback(&[one, two], "all again").batches, 0);
}

#[test]
fn overlapping_rollback_requests_undo_newest_first() {
    // Re-check finding: A sets stage 0 -> 1, B sets 1 -> 2. Rolling back A, then A+B,
    // must invert B before A and end at stage 0.
    let kettle = StationId::from_static("station.kettle_dock");
    let stage = |s: &Sim| s.world().stations.get(&kettle).unwrap().build.stage;
    let mut s = fresh();
    s.submit(script("a").with(Command::SetStationStage {
        station: kettle.clone(),
        stage: 0,
    }));
    step(&mut s);
    s.submit(script("stage one").with(Command::SetStationStage {
        station: kettle.clone(),
        stage: 1,
    }));
    step(&mut s);
    s.submit(script("stage two").with(Command::SetStationStage {
        station: kettle.clone(),
        stage: 2,
    }));
    step(&mut s);
    assert_eq!(stage(&s), 2);
    let n = s.journal().len();
    let (a, b) = (s.journal()[n - 2].cause, s.journal()[n - 1].cause);

    assert_eq!(s.queue_rollback(&[a], "undo a").batches, 1);
    assert_eq!(s.queue_rollback(&[a, b], "undo both").batches, 1);
    let report = step(&mut s);
    assert_eq!(report.applied, 2);
    assert_eq!(stage(&s), 0);
}

#[test]
fn rejected_rollback_can_be_retried() {
    let mut s = fresh();
    s.submit(script("plus ten").with(Command::AdjustCredits { delta: 10 }));
    step(&mut s);
    let cause = s.journal()[0].cause;
    // The player spends most of it, so the inverse (-10) can't apply yet.
    s.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Buy, "buy"))
            .with(Command::AdjustCredits { delta: -45 }),
    );
    step(&mut s);
    assert_eq!(credits(&s), 5);
    assert_eq!(s.queue_rollback(&[cause], "undo").batches, 1);
    assert_eq!(step(&mut s).rejected.len(), 1);
    assert_eq!(credits(&s), 5);

    s.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Sell, "sell"))
            .with(Command::AdjustCredits { delta: 20 }),
    );
    step(&mut s);
    assert_eq!(s.queue_rollback(&[cause], "retry").batches, 1);
    assert_eq!(step(&mut s).applied, 1);
    assert_eq!(credits(&s), 15);
    assert_eq!(s.queue_rollback(&[cause], "once more").batches, 0);
}

// --- 5. Non-finite numbers never reach a save ---

#[test]
fn non_finite_commands_are_rejected() {
    let mut s = fresh();
    let before = s.world().clone();
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        s.submit(CommandBatch::new(Cause::system("test", "bad motion")).with(
            Command::SetShipMotion {
                ship: PLAYER,
                position: Vec2::new(bad, 0.0),
                velocity: Vec2::ZERO,
                heading: 0.0,
            },
        ));
        let mut ship = before.ships.get(&PLAYER).unwrap().clone();
        ship.heading = bad;
        s.submit(
            CommandBatch::new(Cause::system("test", "bad ship")).with(Command::PutShip {
                id: PLAYER,
                value: Some(Box::new(ship)),
            }),
        );
        let report = step(&mut s);
        assert_eq!(report.applied, 0);
        assert_eq!(report.rejected.len(), 2);
        assert!(
            report.rejected[0].reason.contains("finite"),
            "{:?}",
            report.rejected[0]
        );
        assert_eq!(report.rejected[0].command, Some(0));
    }
    assert_eq!(s.world().ships, before.ships);

    // What the sim accepted saves and loads.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sqlite");
    Save::create(&path, s.state()).unwrap();
    assert_eq!(Save::open(&path).unwrap().load().unwrap(), *s.state());
}

#[test]
fn non_finite_state_is_never_written() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sqlite");
    let mut s = fresh();
    step(&mut s);
    let mut save = Save::create(&path, s.state()).unwrap();
    let good = save.load().unwrap();

    let mut bad = s.state().clone();
    bad.world.ships.get_mut(&PLAYER).unwrap().position.x = f32::NAN;
    let err = save.write(&bad).unwrap_err().to_string();
    assert!(err.contains("non-finite"), "{err}");
    assert!(save.snapshot(&bad, "bad").is_err());
    assert!(Save::create(dir.path().join("other.sqlite"), &bad).is_err());
    assert!(!dir.path().join("other.sqlite").exists());

    // A pending command with NaN is refused too.
    let mut pending = s.state().clone();
    pending
        .kernel
        .pending
        .push(
            CommandBatch::new(Cause::system("test", "nan")).with(Command::SetShipMotion {
                ship: PLAYER,
                position: Vec2::ZERO,
                velocity: Vec2::new(0.0, f32::INFINITY),
                heading: 0.0,
            }),
        );
    assert!(save.write(&pending).is_err());

    // The good save is untouched and still loads.
    assert_eq!(save.load().unwrap(), good);
    assert!(save.snapshots().unwrap().is_empty());
}

#[test]
fn non_finite_authored_data_is_rejected() {
    for bad in ["nan", "inf", "-inf"] {
        let dir = tempfile::tempdir().unwrap();
        for f in galaxy_contract::data::DATA_FILES {
            std::fs::copy(data_dir().join(f), dir.path().join(f)).unwrap();
        }
        let stations = std::fs::read_to_string(dir.path().join("stations.toml")).unwrap();
        let edited = stations.replacen("x = -200.0", &format!("x = {bad}"), 1);
        assert_ne!(edited, stations, "the start station's x is in the data");
        std::fs::write(dir.path().join("stations.toml"), edited).unwrap();
        let err = new_game(dir.path(), 1).unwrap_err().to_string();
        assert!(err.contains("finite"), "{bad}: {err}");
    }
}

// --- 6. Integer boundaries reject atomically ---

#[test]
fn integer_boundaries_reject_without_panicking() {
    let mut s = fresh();
    let before = s.world().clone();
    assert!(before.rocks.get(&ROCK).unwrap().remaining > 0);
    let bad = [
        Command::AdjustRock {
            rock: ROCK,
            delta: i64::MAX,
        },
        Command::AdjustRock {
            rock: ROCK,
            delta: i64::MIN,
        },
        Command::AdjustStanding {
            faction: COOP,
            delta: i32::MIN,
        },
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: i64::MAX,
        },
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: i64::MIN,
        },
        Command::AdjustCredits { delta: i64::MIN },
        Command::AdjustCredits { delta: i64::MAX },
    ];
    for cmd in &bad {
        // Each bad command follows a good one in its batch, which must be undone too.
        s.submit(
            CommandBatch::new(Cause::system("test", "boundary"))
                .with(Command::AdjustStanding {
                    faction: COOP,
                    delta: 1,
                })
                .with(cmd.clone()),
        );
    }
    s.submit(
        CommandBatch::new(Cause::system("test", "fine")).with(Command::AdjustRock {
            rock: ROCK,
            delta: -1,
        }),
    );
    let report = step(&mut s);
    // `AdjustCredits { delta: i64::MAX }` from 40 overflows and is refused like the rest.
    assert_eq!(report.rejected.len(), bad.len(), "{:?}", report.rejected);
    assert!(report.rejected.iter().all(|r| r.command == Some(1)));
    assert_eq!(report.applied, 1, "other batches keep going");

    let w = s.world();
    assert_eq!(w.factions, before.factions);
    assert_eq!(w.player, before.player);
    assert_eq!(w.ships, before.ships);
    assert_eq!(
        w.rocks.get(&ROCK).unwrap().remaining,
        before.rocks.get(&ROCK).unwrap().remaining - 1
    );
}

#[test]
fn standing_at_the_minimum_still_inverts() {
    // i32::MIN is reachable in steps whose inverses exist; only `delta: i32::MIN` has none.
    let mut s = fresh();
    assert_eq!(s.world().factions.get(&COOP).unwrap().standing, 0);
    s.submit(script("down").with(Command::AdjustStanding {
        faction: COOP,
        delta: -i32::MAX,
    }));
    s.submit(script("down more").with(Command::AdjustStanding {
        faction: COOP,
        delta: -1,
    }));
    assert_eq!(step(&mut s).applied, 2);
    assert_eq!(s.world().factions.get(&COOP).unwrap().standing, i32::MIN);
    let causes: Vec<_> = s.journal().iter().map(|e| e.cause).collect();
    assert_eq!(s.queue_rollback(&causes, "undo").batches, 2);
    assert_eq!(step(&mut s).applied, 2);
    assert_eq!(s.world().factions.get(&COOP).unwrap().standing, 0);
}
