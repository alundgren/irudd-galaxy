//! The bridge in-process, headless: the socket round trip on a temp endpoint, digest
//! grouping, staging that waits for a calm moment. (The sandbox, propose, the inbox and
//! the CLI need the `galaxy` binary: `crates/app/tests/gm.rs`.)

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use galaxy_gm::bot::ActivityBot;
use galaxy_gm::live::{CallError, Client, Endpoint, EndpointKind, Op, Server, ServerOptions};
use galaxy_gm::{Bridge, BridgeOptions, NoSnapshots};
use serde_json::{Value, json};

const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ODO: NpcId = NpcId::from_static("npc.odo_fenn");

fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn kettle_pack() -> PathBuf {
    slice().join("packs/gm-examples/kettle_growth")
}

fn new_state() -> SimState {
    galaxy_core::new_game(&slice().join("packs/core/data"), 7).expect("core data loads")
}

/// The game's composition (as `game()` in the app).
fn game(state: SimState) -> Sim {
    galaxy_core::game(state)
        .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
        .with_systems([Box::new(galaxy_economy::EconomySystem) as Box<dyn galaxy_core::System>])
}

/// A fresh endpoint per test, so tests can run at once.
fn temp_endpoint(dir: &Path) -> Endpoint {
    let key = galaxy_gm::live::save_key(dir);
    if cfg!(windows) {
        Endpoint {
            kind: EndpointKind::NamedPipe,
            name: format!("galaxy-test-{key}-{}", std::process::id()),
        }
    } else {
        Endpoint {
            kind: EndpointKind::UnixSocket,
            name: dir.join("t.sock").to_string_lossy().into_owned(),
        }
    }
}

/// Flies the bot until it has docked `n` more times.
fn fly(sim: &mut Sim, bot: &mut ActivityBot, n: u32) {
    let sub = sim.subscribe("test-bot");
    let goal = bot.dockings + n;
    for _ in 0..40_000 {
        if bot.dockings >= goal {
            return;
        }
        let input = bot.input(sim.world());
        sim.step(&input).expect("not paused");
        let events = sim.drain_events(&sub);
        bot.observe(sim.world(), &events);
    }
    panic!("the bot didn't dock {n} times");
}

#[test]
fn socket_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let save = dir.path().join("rt.sqlite");
    std::fs::write(&save, b"").unwrap();
    let gm_dir = dir.path().join("rt.gm");
    let registry = dir.path().join("registry");
    let server = Server::start(ServerOptions {
        save: save.clone(),
        gm_dir: gm_dir.clone(),
        registry: Some(registry.clone()),
        endpoint: Some(temp_endpoint(dir.path())),
    })
    .expect("the server starts");
    // Discoverable both ways: next to the save and in the registry.
    let discovery = galaxy_gm::Discovery::read(&gm_dir.join("live.json")).unwrap();
    assert_eq!(discovery.token.len(), 64);

    // A client on another thread; the "game" answers between its frames here.
    let client_discovery = discovery.clone();
    let client_registry = registry.clone();
    let client = std::thread::spawn(move || {
        // The registry lookup probes with `hello`, which the game below answers.
        let found = galaxy_gm::live::find(None, Some(&client_registry))
            .expect("found through the registry");
        assert_eq!(found, client_discovery);
        let mut c = Client::connect(&client_discovery, None).unwrap();
        let hello = c.call("hello", json!({})).unwrap();
        let state = c.call("state.get", json!({ "path": "a.b" })).unwrap();
        let missing = c.call("state.get", json!({ "path": "nope" })).unwrap_err();
        let unknown = c.call("no.such.op", json!({})).unwrap_err();
        let ack = c.stream("watch", json!({})).unwrap();
        let items: Vec<Value> = (0..3).map(|_| c.next_item().unwrap()).collect();
        // A wrong token is refused and the connection closed.
        let mut bad = client_discovery.clone();
        bad.token = "0".repeat(64);
        let mut b = Client::connect(&bad, None).unwrap();
        let refused = b.call("hello", json!({})).unwrap_err();
        (hello, state, missing, unknown, ack, items, refused)
    });
    let world = json!({ "a": { "b": 42 } });
    let mut sinks = Vec::new();
    let started = Instant::now();
    while !client.is_finished() {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the client hung"
        );
        while let Some(req) = server.poll() {
            match req.op.clone() {
                Op::Hello {} => req.respond(Ok(json!({ "game": "test" }))),
                Op::StateGet { path } => req.respond(
                    galaxy_gm::reads::state_get(&world, path.as_deref())
                        .map_err(galaxy_gm::live::WireError::from),
                ),
                Op::Watch { .. } => sinks.push(req.into_stream(json!({ "watching": true }))),
                _ => req.respond(Err(galaxy_gm::live::WireError::input("unexpected"))),
            }
        }
        for (i, s) in sinks.iter_mut().enumerate() {
            s.send(&json!({ "type": "tick", "n": i }));
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let (hello, state, missing, unknown, ack, items, refused) = client.join().unwrap();
    assert_eq!(hello["game"], "test");
    assert_eq!(state, 42);
    assert!(matches!(missing, CallError::Remote(e) if e.code == 4));
    assert!(
        matches!(unknown, CallError::Remote(e) if e.code == 4 && e.message.contains("no.such.op"))
    );
    assert_eq!(ack["watching"], true);
    assert!(items.iter().all(|i| i["type"] == "tick"));
    assert!(matches!(refused, CallError::Remote(e) if e.kind == "unauthorized"));

    // Dropping the server removes the discovery files.
    drop(server);
    assert!(!gm_dir.join("live.json").exists());
    assert_eq!(std::fs::read_dir(&registry).unwrap().count(), 0);
}

#[test]
fn digest_groups_by_place_and_person() {
    let mut sim = game(new_state());
    let host = galaxy_script::ScriptHost::new(None);
    assert!(
        host.load(&kettle_pack(), sim.world(), sim.history())
            .accepted()
    );
    host.attach(&mut sim);
    let mut bot = ActivityBot::new(sim.world()).expect("a flagship and stations");
    fly(&mut sim, &mut bot, 6);

    let d = galaxy_gm::digest(sim.state(), None);
    assert!(d.facts > 0);
    assert_eq!(d.until, sim.history().last_fact_id());
    let kettle = d
        .places
        .iter()
        .find(|p| p.id == KETTLE.as_str())
        .expect("Kettle Dock has a group");
    assert_eq!(kettle.kind, "station");
    let docks = kettle
        .kinds
        .iter()
        .find(|k| k.kind.as_str() == "dock")
        .expect("dockings there");
    assert_eq!(docks.count, 3, "half the loop's dockings were at Kettle");
    assert!(docks.cites.iter().all(|f| {
        let fact = sim.history().fact(*f).unwrap();
        fact.subjects.station.as_ref() == Some(&KETTLE) && fact.kind.as_str() == "dock"
    }));
    assert!(
        kettle.summary.iter().any(|s| s.starts_with("dock x3")),
        "{:?}",
        kettle.summary
    );
    // Odo spoke after the third docking at Kettle: he gets a group with his line, and
    // the line cites a real fact.
    let odo = d
        .people
        .iter()
        .find(|p| p.id == ODO.as_str())
        .expect("Odo has a group");
    assert_eq!(odo.name, "Odo Fenn");
    assert_eq!(odo.talk.len(), 1);
    let line = d.talk.iter().find(|t| t.id == odo.talk[0]).unwrap();
    assert!(line.cites.iter().all(|f| sim.history().fact(*f).is_some()));
    assert!(d.changes.iter().any(|c| {
        c.kind == "script"
            && c.pack
                .as_ref()
                .is_some_and(|p| p.as_str() == "gm.kettle_growth")
    }));

    // Since the newest fact: nothing new.
    let again = galaxy_gm::digest(sim.state(), d.until);
    assert_eq!(again.facts, 0);
    assert!(again.places.is_empty() && again.people.is_empty() && again.changes.is_empty());
    assert_eq!(again.until, d.until);
}

#[test]
fn staging_waits_for_a_calm_moment() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("calm.sqlite");
    let mut state = new_state();
    // Undock and fly off, so the player isn't at a calm moment.
    {
        let mut sim = game(state);
        sim.step(&PlayerInput {
            action: true,
            ..PlayerInput::default()
        });
        for _ in 0..30 {
            sim.step(&PlayerInput {
                thrust: Vec2::new(1.0, 0.0),
                ..PlayerInput::default()
            });
        }
        state = sim.into_state();
    }
    let mut save = Save::create(&path, &state).unwrap();
    let mut sim = game(state);
    let mut bridge = Bridge::open(&path, &mut sim, BridgeOptions::default()).unwrap();
    assert!(!galaxy_gm::bridge::is_calm(&sim), "flying is not calm");

    // Stage an accepted copy of kettle_growth, as `gm propose` would.
    let pack = galaxy_script::read_pack(&kettle_pack()).unwrap();
    let checked = galaxy_script::check_dir(
        &kettle_pack(),
        galaxy_script::CheckOptions {
            world: Some(sim.world()),
            history: Some(sim.history()),
            ..Default::default()
        },
    );
    assert!(checked.accepted());
    let rel = bridge.store().import(&pack).unwrap();
    let staged = bridge
        .stage(&rel, checked.report.clone(), "test", false)
        .unwrap();
    assert_eq!(staged.stage, "stage.1");
    assert!(
        staged.report.steps.last().unwrap().diagnostics[0]
            .message
            .contains("next calm moment")
    );

    // Flying: serving and ticking leave it staged.
    for _ in 0..20 {
        assert!(bridge.serve(&mut sim, &mut save).is_empty());
        sim.step(&PlayerInput::default());
    }
    assert_eq!(bridge.state().staged.len(), 1);
    assert!(bridge.state().applied.is_empty());
    assert!(!sim.world().npcs.contains(&ODO));

    // Paused is calm: it applies (snapshot first), and the change ID arrives with the
    // first tick after the pause.
    sim.set_paused(true);
    bridge.serve(&mut sim, &mut save);
    assert!(bridge.state().staged.is_empty());
    let applied = &bridge.state().applied[0];
    assert_eq!(applied.stage, "stage.1");
    assert_eq!(
        applied.snapshot,
        Some(1),
        "a snapshot was taken before applying"
    );
    assert_eq!(save.snapshots().unwrap()[0].tick, sim.tick().0);
    assert_eq!(
        applied.change, None,
        "paused: the batch hasn't committed yet"
    );
    sim.set_paused(false);
    sim.step(&PlayerInput::default());
    bridge.after_tick(&sim);
    let change = bridge.state().applied[0].change.expect("committed");
    let cause = sim.history().cause(change).unwrap();
    assert!(
        matches!(&cause.cause.kind, CauseKind::Script { pack, .. } if pack.as_str() == "gm.kettle_growth")
    );
    assert!(
        sim.world().npcs.contains(&ODO),
        "the pack's data installed with it"
    );
    assert_eq!(
        bridge.host().revision(&pack.manifest.id).as_deref(),
        Some(pack.revision.as_str())
    );
    // state.json has it all, for the next load.
    let on_disk = bridge.store().load().unwrap();
    assert_eq!(on_disk.applied[0].change, Some(change));

    // Rolled back in-world, the revision is retired and never applies again.
    let out = galaxy_gm::bridge::rollback_change(
        &mut sim,
        bridge.store(),
        bridge.state(),
        &change.to_string(),
    )
    .unwrap();
    assert_eq!(out["pack"], "gm.kettle_growth");
    sim.step(&PlayerInput::default());
    assert!(!sim.world().npcs.contains(&ODO));
    let rel2 = bridge.store().import(&pack).unwrap();
    bridge.stage(&rel2, checked.report, "test", true).unwrap();
    sim.set_paused(true);
    bridge.serve(&mut sim, &mut NoSnapshots);
    assert_eq!(bridge.state().quarantined.len(), 1);
    assert!(bridge.state().quarantined[0].reason.contains("rolled back"));
}
