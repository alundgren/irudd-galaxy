//! The bridge in-process, headless: the socket round trip on a temp endpoint and its
//! limits, digest grouping, staging that waits for a calm moment and its snapshot,
//! recovery from a crash between apply and save, inbox readiness. (The sandbox,
//! propose, the inbox's validation and the CLI need the `galaxy` binary:
//! `crates/app/tests/gm.rs`.)

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use galaxy_gm::bot::ActivityBot;
use galaxy_gm::live::{
    CallError, Client, Endpoint, EndpointKind, Limits, Op, Server, ServerOptions,
};
use galaxy_gm::{Bridge, BridgeOptions, NoSnapshots, SnapshotTicket, Snapshots};
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

/// Opens the bridge the way the game does: into a script host that is attached after
/// it has loaded the save's applied packs.
fn open_bridge(path: &Path, sim: &mut Sim) -> Bridge {
    let host = galaxy_script::ScriptHost::new(None);
    let bridge = Bridge::open(path, sim, &host, BridgeOptions::default()).unwrap();
    host.attach(sim);
    bridge
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
        limits: Limits::default(),
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
        // An oversized argument is refused before it reaches the game.
        let long = c
            .call("state.get", json!({ "path": "x.".repeat(20_000) }))
            .unwrap_err();
        assert!(
            matches!(&long, CallError::Remote(e) if e.code == 4 && e.message.contains("the most a request takes")),
            "{long}"
        );
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
    let mut bridge = open_bridge(&path, &mut sim);
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
    assert_eq!(applied_facts(&sim), 0);

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
    assert_eq!(applied_facts(&sim), 1);
    assert!(sim.world().npcs.contains(&ODO));
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
    assert!(
        sim.world().packs[&pack.manifest.id]
            .retired
            .contains(&pack.revision)
    );
    let rel2 = bridge.store().import(&pack).unwrap();
    bridge.stage(&rel2, checked.report, "test", true).unwrap();
    sim.set_paused(true);
    bridge.serve(&mut sim, &mut NoSnapshots);
    assert_eq!(bridge.state().quarantined.len(), 1);
    assert!(bridge.state().quarantined[0].reason.contains("rolled back"));
}

/// Reads one line from a raw client stream, giving up after `wait`. `Some("")` is the
/// server closing the connection.
fn line_within(stream: interprocess::local_socket::Stream, wait: Duration) -> Option<String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let mut r = BufReader::new(stream);
        let _ = r.read_line(&mut line);
        let _ = tx.send(line);
    });
    rx.recv_timeout(wait).ok()
}

#[test]
fn stalled_clients_are_dropped_and_capped() {
    let dir = tempfile::tempdir().unwrap();
    let save = dir.path().join("stall.sqlite");
    std::fs::write(&save, b"").unwrap();
    let limits = Limits {
        max_connections: 2,
        read_stall: Duration::from_millis(300),
        idle: Duration::from_secs(60),
        write_stall: Duration::from_millis(500),
        max_out: 64 * 1024,
    };
    let server = Server::start(ServerOptions {
        save: save.clone(),
        gm_dir: dir.path().join("stall.gm"),
        registry: None,
        endpoint: Some(temp_endpoint(dir.path())),
        limits,
    })
    .unwrap();
    let d = server.discovery().clone();

    // One client that never says anything, one that stops halfway through a line.
    let silent = d.endpoint.connect().unwrap();
    let mut partial = d.endpoint.connect().unwrap();
    partial.write_all(br#"{"id": 1, "tok"#).unwrap();
    // A third is over the cap: told so, and closed.
    let third = d.endpoint.connect().unwrap();
    let busy = line_within(third, Duration::from_secs(10)).expect("an answer");
    assert!(busy.contains("busy"), "{busy}");
    // Both stalled ones are dropped after the read deadline, not held for ever.
    let started = Instant::now();
    assert_eq!(
        line_within(silent, Duration::from_secs(10)).as_deref(),
        Some("")
    );
    assert_eq!(
        line_within(partial, Duration::from_secs(10)).as_deref(),
        Some("")
    );
    assert!(started.elapsed() < Duration::from_secs(10));

    // With them gone a real client gets in again.
    let hello_d = d.clone();
    let client = std::thread::spawn(move || {
        let mut c = Client::connect(&hello_d, None).unwrap();
        c.call("hello", json!({})).unwrap()
    });
    let started = Instant::now();
    while !client.is_finished() {
        assert!(started.elapsed() < Duration::from_secs(30), "hello hung");
        if let Some(req) = server.poll() {
            req.respond(Ok(json!({ "game": "test" })));
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert_eq!(client.join().unwrap()["game"], "test");

    // A watcher that stops reading: the game never blocks on it, and once its replies
    // have waited past the write deadline its connection (and so its stream) ends.
    let mut watcher = d.endpoint.connect().unwrap();
    let watch = json!({ "id": 1, "token": d.token, "op": "watch", "args": {} });
    watcher.write_all(format!("{watch}\n").as_bytes()).unwrap();
    let started = Instant::now();
    let mut sink = loop {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "no watch request"
        );
        if let Some(req) = server.poll() {
            break req.into_stream(json!({ "watching": true }));
        }
        std::thread::sleep(Duration::from_millis(2));
    };
    let item = json!({ "type": "fact", "pad": "x".repeat(4096) });
    let started = Instant::now();
    loop {
        let sent = Instant::now();
        let open = sink.send(&item);
        assert!(
            sent.elapsed() < Duration::from_millis(100),
            "a send blocked"
        );
        if !open {
            break;
        }
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "the stalled watcher was never dropped"
        );
        std::thread::sleep(Duration::from_millis(1));
    }

    // Stopping the server with a stalled client connected doesn't wait for it.
    let _stuck = d.endpoint.connect().unwrap();
    let started = Instant::now();
    drop(server);
    assert!(started.elapsed() < Duration::from_secs(2));
    drop(watcher);
}

/// A staged accepted copy of kettle_growth, as `gm propose` leaves it.
fn stage_kettle(bridge: &mut Bridge, sim: &Sim, calm: bool) {
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
    bridge.stage(&rel, checked.report, "test", calm).unwrap();
}

fn applied_facts(sim: &Sim) -> usize {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind == galaxy_gm::bridge::APPLIED)
        .count()
}

/// Snapshots that always fail, counting the attempts.
struct Failing(u32);

impl Snapshots for Failing {
    fn snapshot(&mut self, _: &SimState, _: &str) -> SnapshotTicket {
        self.0 += 1;
        SnapshotTicket::Done(Err("the disk is full".into()))
    }
}

/// Snapshots the test finishes by hand.
#[derive(Default)]
struct Later(Vec<mpsc::Sender<Result<i64, String>>>);

impl Snapshots for Later {
    fn snapshot(&mut self, _: &SimState, _: &str) -> SnapshotTicket {
        let (tx, rx) = mpsc::channel();
        self.0.push(tx);
        SnapshotTicket::Pending(rx)
    }
}

#[test]
fn a_pack_applies_only_after_its_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("snap.sqlite");
    let state = new_state();
    Save::create(&path, &state).unwrap();
    // A new game starts docked at the Wharf: calm.
    let mut sim = game(state);
    let mut bridge = open_bridge(&path, &mut sim);
    assert!(galaxy_gm::bridge::is_calm(&sim));
    stage_kettle(&mut bridge, &sim, true);

    // A failed snapshot: nothing applies, and it isn't retried every frame.
    let mut failing = Failing(0);
    for _ in 0..3 {
        bridge.serve(&mut sim, &mut failing);
    }
    assert_eq!(failing.0, 1);
    assert_eq!(bridge.state().staged.len(), 1);
    assert!(bridge.state().applied.is_empty());
    assert!(sim.state().kernel.pending.is_empty(), "no batch was queued");
    assert!(
        bridge.status(&sim)["staged"][0]["snapshot"]
            .as_str()
            .unwrap()
            .starts_with("failed")
    );
    for _ in 0..5 {
        sim.step(&PlayerInput::default());
    }
    assert_eq!(applied_facts(&sim), 0);

    // The next calm moment tries again. A snapshot still being written holds the pack.
    let flying = PlayerInput {
        action: true,
        ..PlayerInput::default()
    };
    sim.step(&flying);
    for _ in 0..30 {
        sim.step(&PlayerInput {
            thrust: Vec2::new(1.0, 0.0),
            ..PlayerInput::default()
        });
    }
    bridge.serve(&mut sim, &mut failing);
    assert!(!galaxy_gm::bridge::is_calm(&sim));
    sim.set_paused(true);
    let mut later = Later::default();
    bridge.serve(&mut sim, &mut later);
    assert_eq!(later.0.len(), 1);
    assert_eq!(bridge.state().staged.len(), 1, "waiting for its snapshot");
    assert_eq!(bridge.status(&sim)["staged"][0]["snapshot"], "writing");
    // The world moves on before the snapshot lands: it no longer holds the state the
    // pack would apply to, so it's dropped and a new one is taken.
    sim.set_paused(false);
    sim.step(&PlayerInput::default());
    sim.set_paused(true);
    later.0[0].send(Ok(7)).unwrap();
    bridge.serve(&mut sim, &mut later);
    assert!(bridge.state().applied.is_empty());
    assert_eq!(later.0.len(), 2);
    // This one matches: the pack applies, with that snapshot as its restore point.
    later.0[1].send(Ok(8)).unwrap();
    bridge.serve(&mut sim, &mut later);
    assert!(bridge.state().staged.is_empty());
    assert_eq!(bridge.state().applied[0].snapshot, Some(8));
    sim.set_paused(false);
    sim.step(&PlayerInput::default());
    bridge.after_tick(&sim);
    assert!(bridge.state().applied[0].change.is_some());
    assert!(sim.world().npcs.contains(&ODO));
    assert_eq!(applied_facts(&sim), 1);
}

#[test]
fn a_crash_between_apply_and_save_is_recovered() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("crash.sqlite");
    let state = new_state();
    let mut save = Save::create(&path, &state).unwrap();
    let state_json = galaxy_gm::GmStore::for_save(&path)
        .dir()
        .join(galaxy_gm::store::STATE_FILE);

    // Applied and committed, state.json written with its change ID, but the game dies
    // before the save is written.
    {
        let mut sim = game(state.clone());
        let mut bridge = open_bridge(&path, &mut sim);
        stage_kettle(&mut bridge, &sim, true);
        bridge.apply_staged(&mut sim, &mut save);
        sim.step(&PlayerInput::default());
        bridge.after_tick(&sim);
        assert!(bridge.store().load().unwrap().applied[0].change.is_some());
    }
    // Reopened on the save as written: the apply isn't in it, so the pack doesn't run
    // and goes back to staged under its stage ID ...
    let mut sim = game(save.load().unwrap());
    let mut bridge = open_bridge(&path, &mut sim);
    assert!(bridge.state().applied.is_empty());
    assert_eq!(bridge.state().staged[0].stage, "stage.1");
    assert!(bridge.notes.iter().any(|n| n.contains("applies again")));
    assert_eq!(
        bridge
            .host()
            .revision(&PackId::from_static("gm.kettle_growth")),
        None
    );
    sim.step(&PlayerInput::default());
    assert_eq!(applied_facts(&sim), 0);
    // ... and state.json says so too.
    assert!(bridge.store().load().unwrap().applied.is_empty());
    // It applies at the next calm moment, once.
    bridge.apply_staged(&mut sim, &mut save);
    // This time the save is written with the apply batch still queued (paused, say),
    // and state.json is lost behind it, still saying "staged".
    save.write(sim.state()).unwrap();
    std::fs::write(
        &state_json,
        serde_json::to_string(&{
            let mut gm = bridge.store().load().unwrap();
            let a = gm.applied.remove(0);
            gm.staged.push(galaxy_gm::store::StagedPack {
                stage: a.stage,
                pack: a.pack,
                revision: a.revision,
                name: a.name,
                dir: a.dir,
                source: a.source,
                report: a.report,
            });
            gm
        })
        .unwrap(),
    )
    .unwrap();
    drop(bridge);
    // Reopened: the save holds the queued batch, so it counts as applied (not applied
    // twice); its change ID arrives when the batch commits.
    let mut sim = game(save.load().unwrap());
    let mut bridge = open_bridge(&path, &mut sim);
    assert!(bridge.state().staged.is_empty());
    assert_eq!(bridge.state().applied.len(), 1);
    assert_eq!(bridge.state().applied[0].change, None);
    assert!(bridge.apply_staged(&mut sim, &mut save).is_empty());
    sim.step(&PlayerInput::default());
    bridge.after_tick(&sim);
    let change = bridge.state().applied[0].change.expect("committed");
    assert!(sim.world().npcs.contains(&ODO));
    assert_eq!(applied_facts(&sim), 1);
    // A change ID state.json claims must be in the save's history: written, reopened,
    // it's the save's own.
    save.write(sim.state()).unwrap();
    drop(bridge);
    let mut sim = game(save.load().unwrap());
    let bridge = open_bridge(&path, &mut sim);
    assert_eq!(bridge.state().applied[0].change, Some(change));
    assert!(
        bridge
            .host()
            .revision(&PackId::from_static("gm.kettle_growth"))
            .is_some()
    );
}

#[test]
fn the_inbox_takes_only_finished_copies() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("gm-inbox");
    let pack = galaxy_script::read_pack(&kettle_pack()).unwrap();
    for name in ["copying", "done", ".hidden"] {
        for (f, text) in &pack.files {
            let p = inbox.join(name).join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    }
    // Only a folder with its READY marker is complete.
    std::fs::write(inbox.join("done").join(galaxy_gm::store::READY), b"").unwrap();
    std::fs::write(inbox.join(".hidden").join(galaxy_gm::store::READY), b"").unwrap();
    let ready: Vec<String> = galaxy_gm::bridge::inbox_entries(&inbox)
        .into_iter()
        .map(|(n, _)| n)
        .collect();
    assert_eq!(ready, ["done"]);
    assert_eq!(galaxy_gm::bridge::inbox_incomplete(&inbox), ["copying"]);
}

#[test]
fn watchers_that_hang_up_free_their_places_while_paused() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("watch.sqlite");
    let state = new_state();
    Save::create(&path, &state).unwrap();
    let mut sim = game(state);
    let mut bridge = open_bridge(&path, &mut sim);
    bridge.start_live(None).unwrap();
    let d = bridge.server().unwrap().discovery().clone();
    // Paused, so nothing is broadcast that would notice a watcher has gone.
    sim.set_paused(true);
    bridge.serve(&mut sim, &mut NoSnapshots);

    // One more watcher than fit, each hanging up before the next connects.
    for n in 0..=galaxy_gm::bridge::MAX_WATCHERS {
        let watch_d = d.clone();
        let client = std::thread::spawn(move || {
            let mut c = Client::connect(&watch_d, None).unwrap();
            c.stream("watch", json!({}))
        });
        let started = Instant::now();
        while !client.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "watch {n} hung"
            );
            bridge.serve(&mut sim, &mut NoSnapshots);
            std::thread::sleep(Duration::from_millis(2));
        }
        let ack = client.join().unwrap();
        assert!(ack.is_ok(), "watcher {n}: {:?}", ack.err());
        // The client has hung up; serve until the I/O thread has noticed.
        let started = Instant::now();
        loop {
            bridge.serve(&mut sim, &mut NoSnapshots);
            if bridge.watchers() == 0 {
                break;
            }
            assert!(
                started.elapsed() < Duration::from_secs(30),
                "watcher {n} kept its place"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}
