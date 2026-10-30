//! The proof script: change type 1, Kettle Dock grows from deliveries. Headless.

mod common;

use std::time::{Duration, Instant};

use common::*;
use galaxy_contract::history::HistoryQuery;
use galaxy_contract::pack::ScriptValue;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::{Save, Sim};
use galaxy_script::ScriptHost;

#[test]
fn kettle_grows_from_deliveries_and_odo_cites_them() {
    let (mut sim, _host) = kettle_sim(&kettle_dir());
    let parts_before = sim
        .world()
        .stations
        .get(&KETTLE)
        .unwrap()
        .build
        .parts
        .clone();
    grow_to_stage_2(&mut sim);

    let kettle = sim.world().stations.get(&KETTLE).unwrap();
    assert!(kettle.build.parts.contains("part.kettle.greenhouse_ring_a"));
    assert!(kettle.build.parts.is_superset(&parts_before));

    let talk = lines(&sim, Channel::DockTalk);
    assert_eq!(talk.len(), 1, "{talk:#?}");
    let line = &talk[0].line;
    assert_eq!(line.speaker, Some(ODO));
    assert_eq!(line.place, Some(KETTLE));
    assert!(line.text.starts_with("Second ring's lit"));
    // Every cited fact is a delivery or sale at Kettle.
    for id in &line.cites {
        let f = sim.history().fact(*id).expect("cited fact exists");
        assert!(["deliver", "sell"].contains(&f.kind.as_str()), "{f:?}");
        assert_eq!(f.subjects.station, Some(KETTLE));
    }
    assert_eq!(lines(&sim, Channel::MapNote).len(), 1);
    assert_eq!(store_built(&sim), Some(ScriptValue::Integer(2)));

    // The change is one cause, from this pack revision, citing the deliveries.
    let built = sim
        .history()
        .last(&HistoryQuery::kind(
            "gm.kettle_growth.stage_built".parse().unwrap(),
        ))
        .unwrap();
    assert_eq!(built.amount, Some(2));
    let cause = sim.history().cause(built.cause).unwrap();
    assert!(
        matches!(&cause.cause.kind, CauseKind::Script { pack, .. } if pack.as_str() == KETTLE_PACK)
    );
    assert_eq!(cause.cause.cites, line.cites);

    // More plate and ice: stage 3 adds the orchard lamps and a glassite line.
    deliver(&mut sim, &ICE, 60);
    deliver(&mut sim, &PLATE, 30);
    settle(&mut sim);
    assert_eq!(stage(&sim), 3);
    assert!(
        sim.world()
            .markets
            .get(&KETTLE)
            .unwrap()
            .lines
            .contains_key("ware.glassite")
    );
    assert_eq!(lines(&sim, Channel::DockTalk).len(), 2);
}

#[test]
fn two_deliveries_in_one_tick_grow_the_station_once() {
    let (mut sim, _host) = kettle_sim(&kettle_dir());
    deliver(&mut sim, &ICE, 60);
    settle(&mut sim);
    // Both complete stage 2 on their own; the second handler sees the first's writes.
    deliver(&mut sim, &PLATE, 20);
    deliver(&mut sim, &PLATE, 20);
    settle(&mut sim);
    assert_eq!(stage(&sim), 2);
    assert_eq!(lines(&sim, Channel::DockTalk).len(), 1);
}

#[test]
fn survives_a_reload_with_its_state_intact() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kettle_growth");
    copy_dir(&kettle_dir(), &dir);
    let (mut sim, host) = kettle_sim(&dir);
    let id = pack_id(KETTLE_PACK);
    grow_to_stage_2(&mut sim);
    let first = host.revision(&id).unwrap();

    // Edit the script: a new revision.
    let script = dir.join("scripts/kettle_growth.luau");
    let src = std::fs::read_to_string(&script).unwrap();
    std::fs::write(
        &script,
        src.replace(
            "Kettle Dock: a new greenhouse ring",
            "Kettle Dock: another greenhouse ring",
        ),
    )
    .unwrap();
    let report = host.reload(&id, &mut sim).unwrap();
    assert!(report.accepted(), "{report:#?}");
    let second = host.revision(&id).unwrap();
    assert_ne!(first, second);

    // The world kept everything: stage, store, lines.
    assert_eq!(stage(&sim), 2);
    assert_eq!(store_built(&sim), Some(ScriptValue::Integer(2)));
    sell(&mut sim, &ICE, 1);
    settle(&mut sim);
    assert_eq!(
        lines(&sim, Channel::DockTalk).len(),
        1,
        "stage 2 isn't announced twice"
    );

    // A broken edit is rejected; the last good revision keeps running.
    std::fs::write(&script, "this is not luau").unwrap();
    let bad = host.reload(&id, &mut sim).unwrap();
    assert!(!bad.accepted());
    assert_eq!(host.revision(&id).unwrap(), second);

    deliver(&mut sim, &ICE, 60);
    deliver(&mut sim, &PLATE, 30);
    settle(&mut sim);
    assert_eq!(stage(&sim), 3);
    let notes = lines(&sim, Channel::MapNote);
    assert_eq!(
        notes.last().unwrap().line.text,
        "Kettle Dock: another greenhouse ring"
    );
    let stage3 = sim
        .history()
        .last(&HistoryQuery::kind(
            "gm.kettle_growth.stage_built".parse().unwrap(),
        ))
        .unwrap();
    let cause = sim.history().cause(stage3.cause).unwrap();
    assert!(matches!(&cause.cause.kind, CauseKind::Script { revision, .. } if *revision == second));
}

#[test]
fn file_watcher_reloads_between_ticks() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("kettle_growth");
    copy_dir(&kettle_dir(), &dir);
    let (mut sim, host) = kettle_sim(&dir);
    let id = pack_id(KETTLE_PACK);
    let before = host.revision(&id).unwrap();
    host.watch().unwrap();
    let script = dir.join("scripts/ledger.luau");
    let src = std::fs::read_to_string(&script).unwrap();
    std::fs::write(&script, src.replace("ice = 60,", "ice = 50,")).unwrap();

    let deadline = Instant::now() + Duration::from_secs(10);
    let mut reports = Vec::new();
    while reports.is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
        reports = host.poll_reload(&mut sim);
    }
    assert!(!reports.is_empty(), "the watcher saw the edit");
    assert!(reports.iter().all(|r| r.accepted()));
    assert_ne!(host.revision(&id).unwrap(), before);
    // The new threshold is live.
    deliver(&mut sim, &ICE, 50);
    deliver(&mut sim, &PLATE, 20);
    settle(&mut sim);
    assert_eq!(stage(&sim), 2);
}

#[test]
fn activity_trigger_fires_once_after_three_dockings() {
    let (mut sim, _host) = kettle_sim(&kettle_dir());
    for _ in 0..2 {
        dock(&mut sim, &KETTLE);
        settle(&mut sim);
    }
    assert!(lines(&sim, Channel::DockTalk).is_empty());
    dock(&mut sim, &KETTLE);
    settle(&mut sim);
    let talk = lines(&sim, Channel::DockTalk);
    assert_eq!(talk.len(), 1);
    assert!(
        talk[0].line.text.contains("60 more ice and 20 more plate"),
        "{}",
        talk[0].line.text
    );
    let fact = sim.history().fact(talk[0].line.cites[0]).unwrap();
    assert_eq!(fact.kind.as_str(), "dock");
    for _ in 0..4 {
        dock(&mut sim, &KETTLE);
        settle(&mut sim);
    }
    assert_eq!(lines(&sim, Channel::DockTalk).len(), 1, "fires once");
    let fired = sim
        .world()
        .packs
        .get(KETTLE_PACK)
        .unwrap()
        .triggers
        .get("kettle_growth.regulars");
    assert_eq!(fired, Some(&1));
}

#[test]
fn rollback_restores_the_station_and_odo_explains() {
    let (mut sim, host) = kettle_sim(&kettle_dir());
    let before = sim.world().stations.get(&KETTLE).unwrap().clone();
    let market_before = sim.world().markets.get(&KETTLE).unwrap().clone();
    grow_to_stage_2(&mut sim);
    let built = sim
        .history()
        .last(&HistoryQuery::kind(
            "gm.kettle_growth.stage_built".parse().unwrap(),
        ))
        .unwrap()
        .clone();
    let change = built.cause;

    let id = pack_id(KETTLE_PACK);
    let out = galaxy_script::rollback(&mut sim, &[change], host.rollback_line(&id).as_ref());
    assert_eq!(out.plan.batches, 1);
    assert!(
        !out.plan.compensate.is_empty(),
        "the stage_built fact can't be unsaid"
    );
    step(&mut sim, 1);

    // The station is back, Odo's ring line and the map note are withdrawn.
    assert_eq!(sim.world().stations.get(&KETTLE).unwrap(), &before);
    assert_eq!(sim.world().markets.get(&KETTLE).unwrap(), &market_before);
    assert_eq!(store_built(&sim), None);
    assert!(lines(&sim, Channel::MapNote).is_empty());

    // A compensating line from Odo cites the change.
    let talk = lines(&sim, Channel::DockTalk);
    assert_eq!(talk.len(), 1, "{talk:#?}");
    let line = &talk[0].line;
    assert_eq!(line.speaker, Some(ODO));
    assert!(line.text.starts_with("We've taken the new ring down"));
    assert_eq!(line.about, vec![change]);
    assert!(
        line.cites.contains(&built.id),
        "cites the fact the change recorded"
    );
    let chronicle = lines(&sim, Channel::Chronicle);
    assert_eq!(chronicle.len(), 1);
    assert_eq!(chronicle[0].line.about, vec![change]);

    // The revision is retired: the ledger still holds, but it doesn't regrow.
    let revision = host.revision(&id).unwrap();
    assert!(
        sim.world()
            .packs
            .get(KETTLE_PACK)
            .unwrap()
            .retired
            .contains(&revision)
    );
    sell(&mut sim, &PLATE, 5);
    settle(&mut sim);
    assert_eq!(stage(&sim), 0);
}

/// One scripted session: deliveries, sales and dockings over a few hundred ticks.
fn session(sim: &mut Sim, from: u64, to: u64) {
    for t in from..to {
        match t % 40 {
            0 => deliver(sim, &ICE, 7),
            10 => sell(sim, &PLATE, 3),
            20 => dock(sim, &KETTLE),
            _ => {}
        }
        step(sim, 1);
    }
}

fn hosted(state: galaxy_core::SimState) -> Sim {
    let mut sim = Sim::new(state).with_systems(galaxy_core::default_systems());
    let host = ScriptHost::new(None);
    assert!(
        host.load(&kettle_dir(), sim.world(), sim.history())
            .accepted()
    );
    host.attach(&mut sim);
    sim
}

#[test]
fn determinism_same_inputs_same_hash_across_save_and_load() {
    let run = || {
        let mut sim = hosted(galaxy_core::new_game(&core_data(), 7).unwrap());
        session(&mut sim, 0, 800);
        sim
    };
    let a = run();
    let b = run();
    assert_eq!(a.hash(), b.hash());
    assert_eq!(stage(&a), 3, "the session grows Kettle all the way");

    // Save halfway, load into a fresh host, continue: same hash.
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("half.sqlite");
    let mut first = hosted(galaxy_core::new_game(&core_data(), 7).unwrap());
    session(&mut first, 0, 400);
    Save::create(&path, first.state()).unwrap();
    let mut second = hosted(Save::open(&path).unwrap().load().unwrap());
    session(&mut second, 400, 800);
    assert_eq!(second.hash(), a.hash());
}
