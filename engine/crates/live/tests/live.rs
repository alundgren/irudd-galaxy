//! The live socket end to end, headless: a real server and client over a local socket,
//! with a session standing in for the window (`Headless`).

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use galaxy_live::client::{self, Game};
use galaxy_live::{Act, Host, Live, Op, RunningModule, ServerOptions, Step, WireError};
use galaxy_module::module::Module;
use galaxy_render::RgbaImage;
use galaxy_sim::keys::{Hand, Keys};
use galaxy_sim::recording::RecEvent;
use galaxy_sim::{BuildStamp, Input, Session};
use serde_json::{Map, Value, json};

/// Test modules (`tests/<folder>/<name>`), copied so a test can edit them.
fn modules(into: &Path, names: &[(&str, &str)]) -> Vec<PathBuf> {
    let tests = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests");
    let mut dirs = Vec::new();
    for (folder, name) in names {
        let to = into.join(name);
        copy(&tests.join(folder).join(name), &to);
        dirs.push(to);
    }
    dirs
}

fn copy(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A game without a window: the session, and where its recording goes. Its keys work as
/// the play window's do: held through [`Keys`], their inputs run on the next tick.
struct Headless {
    session: Session,
    dir: PathBuf,
    /// An F8 note is open.
    paused: bool,
    keys: Keys,
    pending: Vec<Input>,
}

impl Host for Headless {
    fn session(&self) -> &Session {
        &self.session
    }

    fn presentation(&self) -> Map<String, Value> {
        let Value::Object(m) = json!({ "fps": 0.0, "paused": false }) else {
            unreachable!()
        };
        m
    }

    fn holds_loads(&self) -> Option<&'static str> {
        self.paused.then_some("an F8 note is open")
    }

    fn frame(&mut self) -> Result<RgbaImage, String> {
        Ok(RgbaImage {
            width: 4,
            height: 2,
            pixels: vec![200; 4 * 2 * 4],
        })
    }

    fn hot_load(&mut self, modules: Vec<Module>) -> Vec<Result<(), String>> {
        modules
            .into_iter()
            .map(|m| self.session.hot_load(m))
            .collect()
    }

    fn write_recording(&mut self) -> Option<PathBuf> {
        let path = self.dir.join("recording.json");
        self.session.recording()?.write(&path).ok()?;
        Some(path)
    }

    fn refuses_input(&self) -> Option<String> {
        self.paused.then(|| "an F8 note is open".to_owned())
    }

    fn live_input(&mut self, conn: u64, act: &Act) -> Result<(), String> {
        match act {
            Act::Key { key, down } => {
                let inputs = self.keys.press(key, *down, Hand::Live(conn));
                self.pending.extend(inputs);
                Ok(())
            }
            Act::Move { .. } => Err("no pointer without a window".into()),
        }
    }

    fn live_release(&mut self, conn: u64) {
        let inputs = self.keys.release(Hand::Live(conn));
        self.pending.extend(inputs);
    }

    fn bound(&self, key: &str) -> bool {
        self.keys.bound(key)
    }
}

impl Headless {
    /// One tick, with the live steps due for it and the inputs held keys made.
    fn tick(&mut self, live: &mut Live) {
        live.before_tick(self);
        let inputs = std::mem::take(&mut self.pending);
        self.session.step(&inputs).unwrap();
        live.after_tick(&self.session.world);
    }
}

struct Rig {
    _tmp: tempfile::TempDir,
    live_dir: PathBuf,
    module_dirs: Vec<PathBuf>,
    host: Headless,
    live: Live,
}

fn rig() -> Rig {
    rig_of(&[("modules", "stroll"), ("modules", "tally")])
}

/// A rig with a ship flown by held keys.
fn drift() -> Rig {
    rig_of(&[("play", "drift")])
}

fn rig_of(names: &[(&str, &str)]) -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let module_dirs = modules(&tmp.path().join("modules"), names);
    let loaded: Vec<Module> = module_dirs
        .iter()
        .map(|d| Module::read(d).unwrap())
        .collect();
    let mut session = Session::new(loaded, 7).unwrap();
    session.start_recording(BuildStamp::current());
    for _ in 0..10 {
        session.step(&[]).unwrap();
    }
    let session_dir = tmp.path().join("sessions").join("2026-11-02-120000-stroll");
    std::fs::create_dir_all(&session_dir).unwrap();
    // A short live folder: Unix socket paths are short.
    let live_dir = tmp.path().join("live");
    let live = Live::start(
        ServerOptions {
            dir: live_dir.clone(),
            session_dir: session_dir.clone(),
            build: "test".into(),
            modules: session
                .modules()
                .zip(&module_dirs)
                .map(|(m, d)| RunningModule {
                    id: m.id().to_owned(),
                    dir: d.clone(),
                })
                .collect(),
        },
        None,
    )
    .unwrap();
    Rig {
        _tmp: tmp,
        live_dir,
        module_dirs,
        host: Headless {
            keys: Keys::new(session.modules()),
            session,
            dir: session_dir,
            paused: false,
            pending: Vec::new(),
        },
        live,
    }
}

impl Rig {
    /// Runs `client` on a thread while the game answers between "frames"; the world
    /// steps only while a watch waits for ticks, so reads alone leave it where it was.
    fn with_client<T: Send + 'static>(
        &mut self,
        client: impl FnOnce(PathBuf) -> T + Send + 'static,
    ) -> T {
        let dir = self.live_dir.clone();
        let handle = thread::spawn(move || client(dir));
        let started = Instant::now();
        while !handle.is_finished() {
            assert!(
                started.elapsed() < Duration::from_secs(60),
                "the client hung"
            );
            self.live.serve(&mut self.host);
            if self.live.waiting() {
                self.host.tick(&mut self.live);
            }
            thread::sleep(Duration::from_millis(1));
        }
        handle.join().unwrap()
    }
}

fn the_game(dir: &Path) -> Game {
    let games = client::games(dir);
    client::choose(games, None, &[], Path::new("/nowhere"))
        .unwrap()
        .0
}

fn call(dir: &Path, op: Op) -> Result<Value, WireError> {
    client::call(&the_game(dir).discovery, &op, Duration::from_secs(30))
}

#[test]
fn reads_answer_through_the_socket_and_never_change_the_world() {
    let mut rig = rig();
    let before = rig.host.session.world.hash();
    let replies = rig.with_client(|dir| {
        [
            call(&dir, Op::Status {}),
            call(
                &dir,
                Op::State {
                    entity: Some("w1".into()),
                    component: None,
                    all: false,
                },
            ),
            call(
                &dir,
                Op::State {
                    entity: None,
                    component: None,
                    all: false,
                },
            ),
            call(
                &dir,
                Op::History {
                    last: Some(5),
                    since: None,
                    kind: None,
                    inputs: false,
                },
            ),
            call(&dir, Op::Shot { out: None }),
            call(
                &dir,
                Op::State {
                    entity: Some("nobody".into()),
                    component: None,
                    all: false,
                },
            ),
        ]
    });
    let [status, one, all, history, shot, missing] = replies;
    let status = status.unwrap();
    assert_eq!(status["tick"], 10);
    assert_eq!(status["hash"], before);
    assert_eq!(status["session"], "2026-11-02-120000-stroll");
    assert_eq!(status["modules"][0]["id"], "stroll");
    assert_eq!(status["fps"], 0.0, "the host's presentation is merged in");
    assert!(status["place"].is_null(), "stroll has no player being");
    let one = one.unwrap();
    assert_eq!(one["hash"], before);
    assert_eq!(
        one["state"]["entities"]["stroll.w1"]["stroll.walker"]["steps"],
        0
    );
    let line = all.unwrap()["state"]["entities"]["stroll.w1"].clone();
    assert!(line.as_str().unwrap().starts_with("at ("), "{line}");
    let history = history.unwrap();
    assert!(history["shown"].as_u64().unwrap() <= 5);
    assert!(history["facts"][0]["by"].as_str().is_some(), "{history}");
    let shot = shot.unwrap();
    assert_eq!(shot["size"], json!([4, 2]));
    let out = PathBuf::from(shot["out"].as_str().unwrap());
    assert!(out.ends_with("shots/tick-10.png"), "{out:?}");
    assert!(out.is_file());
    let missing = missing.unwrap_err();
    assert_eq!((missing.code, missing.kind.as_str()), (2, "usage"));
    assert!(
        missing.message.contains("no entity `nobody`"),
        "{}",
        missing.message
    );
    assert_eq!(rig.host.session.world.tick, 10);
    assert_eq!(
        rig.host.session.world.hash(),
        before,
        "reads changed the world"
    );
}

#[test]
fn a_watch_samples_each_tick_and_the_world_runs_as_it_would_unwatched() {
    let mut rig = rig();
    let report = rig
        .with_client(|dir| {
            call(
                &dir,
                Op::Watch {
                    entity: "w1".into(),
                    component: "engine.position".into(),
                    field: Some("x".into()),
                    ticks: 30,
                    every: 10,
                },
            )
        })
        .unwrap();
    assert_eq!(report["entity"], "stroll.w1");
    assert_eq!(report["from_tick"], 10);
    assert_eq!(report["to_tick"], 40);
    let ticks: Vec<u64> = report["samples"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["tick"].as_u64().unwrap())
        .collect();
    assert_eq!(ticks, [10, 20, 30, 40]);
    assert_eq!(report["changes"], 3, "w1 walks: {report}");
    assert!(report["range"]["max"].as_f64() > report["range"]["min"].as_f64());

    // The same 40 ticks without anyone watching end on the same world.
    let loaded: Vec<Module> = rig
        .module_dirs
        .iter()
        .map(|d| Module::read(d).unwrap())
        .collect();
    let mut plain = Session::new(loaded, 7).unwrap();
    for _ in 0..40 {
        plain.step(&[]).unwrap();
    }
    assert_eq!(rig.host.session.world.hash(), plain.world.hash());
}

#[test]
fn a_load_hot_loads_changed_files_and_the_session_replays() {
    let mut rig = rig();
    // Nothing changed yet: nothing loads.
    let first = rig
        .with_client(|dir| call(&dir, Op::Load { dirs: vec![] }))
        .unwrap();
    assert_eq!(first["loaded"], json!([]));
    assert_eq!(first["unchanged"], json!(["stroll", "tally"]));

    let rules = rig.module_dirs[0].join("scripts/rules.luau");
    let text = std::fs::read_to_string(&rules).unwrap();
    std::fs::write(&rules, text.replace("rules.STEP = 1", "rules.STEP = 3")).unwrap();
    let loaded = rig
        .with_client(|dir| call(&dir, Op::Load { dirs: vec![] }))
        .unwrap();
    assert_eq!(loaded["loaded"][0]["module"], "stroll", "{loaded}");
    assert_eq!(loaded["loaded"][0]["presentation_only"], false);
    assert_eq!(loaded["unchanged"], json!(["tally"]));
    // The recording written at the load, before another tick ran, replays to the same
    // world: the load made after its last tick is applied at its end.
    let at_load =
        galaxy_sim::Recording::read(Path::new(loaded["recording"].as_str().unwrap())).unwrap();
    assert_eq!(at_load.end.tick, 10);
    let (_, replayed) = galaxy_sim::replay(&at_load).unwrap();
    assert!(
        replayed.identical,
        "the recording written at the load replays"
    );

    for _ in 0..60 {
        rig.host.session.step(&[]).unwrap();
    }
    let steps = &rig.host.session.world.entities["stroll.w1"]["stroll.walker"].fields["steps"];
    assert_eq!(steps, 6, "two cadences at the new step of 3");
    let rec = rig.host.session.recording().unwrap();
    let (_, replayed) = galaxy_sim::replay(&rec).unwrap();
    assert!(replayed.identical, "the hot-load replays");

    // A script that doesn't load is refused, and the world carries on unchanged.
    std::fs::write(&rules, "return {").unwrap();
    let before = rig.host.session.world.hash();
    let refused = rig
        .with_client(|dir| call(&dir, Op::Load { dirs: vec![] }))
        .unwrap_err();
    assert_eq!(
        (refused.code, refused.kind.as_str()),
        (5, "hot_load_refused")
    );
    assert_eq!(refused.fields["failed"][0]["module"], "stroll");
    assert_eq!(rig.host.session.world.hash(), before);

    // While an F8 note is open, loads wait: nothing changes.
    std::fs::write(&rules, &text).unwrap();
    rig.host.paused = true;
    let held = rig
        .with_client(|dir| call(&dir, Op::Load { dirs: vec![] }))
        .unwrap_err();
    assert_eq!((held.code, held.kind.as_str()), (5, "hot_load_refused"));
    assert!(held.message.contains("F8"), "{}", held.message);
    assert_eq!(rig.host.session.world.hash(), before);
    rig.host.paused = false;

    // A named module folder loads even when unchanged.
    let other = rig._tmp.path().join("modules/tally");
    let named = rig
        .with_client(move |dir| call(&dir, Op::Load { dirs: vec![other] }))
        .unwrap();
    assert_eq!(
        named["loaded"][0]["module"], "tally",
        "named folders load anyway"
    );
}

#[test]
fn games_are_found_chosen_and_forgotten_when_they_stop() {
    let rig = rig();
    let empty = tempfile::tempdir().unwrap();
    let none = client::choose(client::games(empty.path()), None, &[], empty.path()).unwrap_err();
    assert_eq!((none.code, none.kind.as_str()), (2, "no_game"));
    assert!(none.message.contains("galaxy play"), "{}", none.message);

    let games = client::games(&rig.live_dir);
    assert_eq!(games.len(), 1);
    let pid = games[0].discovery.pid.to_string();
    // A second game, playing modules somewhere else.
    let mut elsewhere = games[0].clone();
    elsewhere.discovery.pid += 1;
    for m in &mut elsewhere.discovery.modules {
        m.dir = PathBuf::from("elsewhere").join(&m.id);
    }
    let twice = vec![games[0].clone(), elsewhere];
    let several = client::choose(twice.clone(), None, &[], Path::new("/nowhere")).unwrap_err();
    assert_eq!(several.kind, "several_games");
    assert_eq!(several.fields["games"].as_array().unwrap().len(), 2);
    assert!(
        several.fields["games"][0].get("token").is_none(),
        "the token never leaves the discovery file"
    );
    let here = rig.module_dirs[0].parent().unwrap();
    let (_, why) = client::choose(twice, None, &[], here).unwrap();
    assert_eq!(why, "the only game playing modules in this folder");
    let (_, why) = client::choose(games.clone(), Some(&pid), &[], here).unwrap();
    assert_eq!(why, "named by --game");
    let not_running = client::choose(games.clone(), None, &["other".into()], here).unwrap_err();
    assert_eq!(not_running.kind, "no_game");

    // A stopped game's discovery file goes, and the game is no longer found.
    let file = games[0].file.clone();
    let Rig {
        _tmp,
        live,
        live_dir,
        ..
    } = rig;
    drop(live);
    assert!(!file.exists());
    assert!(client::games(&live_dir).is_empty());
}

#[test]
fn a_wrong_token_is_refused() {
    let mut rig = rig();
    let err = rig
        .with_client(|dir| {
            let mut game = the_game(&dir).discovery;
            game.token = "0".repeat(64);
            client::call(&game, &Op::Status {}, Duration::from_secs(10))
        })
        .unwrap_err();
    assert!(err.message.contains("token"), "{}", err.message);
}

fn key(at: u64, key: &str, down: bool) -> Step {
    Step {
        at,
        act: Act::Key {
            key: key.into(),
            down,
        },
    }
}

/// The recorded inputs: tick, event, down, source and via.
type Recorded = (u64, String, bool, String, Option<String>);

fn recorded(session: &Session) -> Vec<Recorded> {
    session
        .recording()
        .unwrap()
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Input(i) => Some((
                i.tick,
                i.name.clone(),
                i.payload["down"] == true,
                i.source.clone(),
                i.via.clone(),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn live_input_holds_keys_together_like_the_keyboard_and_replays() {
    let mut rig = drift();
    let start = rig.host.session.world.tick;
    let ship = |s: &Session| {
        let p = &s.world.entities["drift.ship"]["engine.position"].fields;
        (p["x"].as_f64().unwrap(), p["y"].as_f64().unwrap())
    };
    let before = ship(&rig.host.session);
    // Hold W for two seconds while holding A for the first half second.
    let report = rig
        .with_client(|dir| {
            call(
                &dir,
                Op::Input {
                    steps: vec![
                        key(0, "w", true),
                        key(60, "w", false),
                        key(0, "a", true),
                        key(15, "a", false),
                    ],
                    ticks: 61,
                },
            )
        })
        .unwrap();
    assert_eq!(report["from_tick"], start + 1);
    assert_eq!(report["to_tick"], start + 61);
    assert_eq!(report["held"], json!([]));
    assert_eq!(report["unbound"], json!([]));
    assert_eq!(report["hash"], rig.host.session.world.hash());
    let taken: Vec<(u64, String)> = report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| {
            (
                s["tick"].as_u64().unwrap(),
                s["step"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        taken,
        [
            (start + 1, "down w".into()),
            (start + 1, "down a".into()),
            (start + 16, "up a".into()),
            (start + 61, "up w".into()),
        ]
    );
    let live = || Some("live".to_owned());
    let keyboard = || "keyboard".to_owned();
    let ahead = "drift.thrust_ahead".to_owned();
    let left = "drift.turn_left".to_owned();
    assert_eq!(
        recorded(&rig.host.session),
        [
            (start + 1, ahead.clone(), true, keyboard(), live()),
            (start + 1, left.clone(), true, keyboard(), live()),
            (start + 16, left, false, keyboard(), live()),
            (start + 61, ahead, false, keyboard(), live()),
        ]
    );
    let after = ship(&rig.host.session);
    assert!(
        (after.0 - before.0).hypot(after.1 - before.1) > 1.0,
        "the ship flew: {before:?} to {after:?}"
    );

    // The window's own keys on the same ticks make the same world.
    let loaded: Vec<Module> = rig
        .module_dirs
        .iter()
        .map(|d| Module::read(d).unwrap())
        .collect();
    let mut plain = Session::new(loaded, 7).unwrap();
    let mut keys = Keys::new(plain.modules());
    for _ in 0..start {
        plain.step(&[]).unwrap();
    }
    for tick in start + 1..=start + 61 {
        let mut inputs = Vec::new();
        if tick == start + 1 {
            inputs.extend(keys.press("w", true, Hand::Window));
            inputs.extend(keys.press("a", true, Hand::Window));
        }
        if tick == start + 16 {
            inputs.extend(keys.press("a", false, Hand::Window));
        }
        if tick == start + 61 {
            inputs.extend(keys.press("w", false, Hand::Window));
        }
        assert!(inputs.iter().all(|i| i.via.is_none()));
        plain.step(&inputs).unwrap();
    }
    assert_eq!(
        rig.host.session.world.hash(),
        plain.world.hash(),
        "live keys and the window's keys make the same world"
    );

    // History says which inputs came through the live socket.
    let history = rig
        .with_client(|dir| {
            call(
                &dir,
                Op::History {
                    last: Some(10),
                    since: None,
                    kind: None,
                    inputs: true,
                },
            )
        })
        .unwrap();
    let facts = history["facts"].as_array().unwrap();
    assert_eq!(facts.len(), 4, "{history}");
    for f in facts {
        assert_eq!(f["by"], "input from keyboard via live", "{f}");
        assert_eq!(f["via"], "live");
    }

    // The recording, written and read back, replays to the same world.
    let path = rig.host.write_recording().unwrap();
    let rec = galaxy_sim::Recording::read(&path).unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert_eq!(
        text.matches("\"via\":\"live\"").count(),
        4,
        "recorded via live"
    );
    let (replayed, report) = galaxy_sim::replay(&rec).unwrap();
    assert!(report.identical, "a session played live replays");
    assert_eq!(replayed.world.hash(), rig.host.session.world.hash());
}

#[test]
fn a_closed_connection_lets_go_of_what_it_held() {
    let mut rig = drift();
    let held = rig
        .with_client(|dir| {
            let mut conn = client::Connection::open(&the_game(&dir).discovery).unwrap();
            let reply = conn.call(
                &Op::Input {
                    steps: vec![key(0, "w", true), key(0, "shift", true)],
                    ticks: 1,
                },
                Duration::from_secs(30),
            );
            // The client goes away without letting go, as a killed process would.
            drop(conn);
            reply
        })
        .unwrap();
    assert_eq!(held["held"], json!(["shift", "w"]));
    let down_at = rig.host.session.world.tick;
    // The game hears the connection close and lets go before the next tick.
    let started = Instant::now();
    while rig.host.pending.len() < 2 {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "the held keys were never let go"
        );
        rig.live.serve(&mut rig.host);
        thread::sleep(Duration::from_millis(1));
    }
    rig.host.tick(&mut rig.live);
    let ups: Vec<Recorded> = recorded(&rig.host.session)
        .into_iter()
        .filter(|r| !r.2)
        .collect();
    assert_eq!(ups.len(), 2, "{ups:?}");
    for (tick, _, _, source, via) in ups {
        assert_eq!(tick, down_at + 1);
        assert_eq!(source, "keyboard");
        assert_eq!(via.as_deref(), Some("live"));
    }
    assert!(!rig.host.keys.is_held("w"));
    let (_, report) = galaxy_sim::replay(&rig.host.session.recording().unwrap()).unwrap();
    assert!(report.identical);
}

#[test]
fn bad_input_steps_and_a_closed_game_are_clear_errors() {
    let mut rig = drift();
    let bad = rig
        .with_client(|dir| {
            call(
                &dir,
                Op::Input {
                    steps: vec![key(0, "jump", true)],
                    ticks: 1,
                },
            )
        })
        .unwrap_err();
    assert_eq!((bad.code, bad.kind.as_str()), (2, "usage"));
    assert!(
        bad.message.contains("`jump` is not a key"),
        "{}",
        bad.message
    );
    assert!(bad.message.contains("mouse_left"), "{}", bad.message);
    rig.host.paused = true;
    let paused = rig
        .with_client(|dir| {
            call(
                &dir,
                Op::Input {
                    steps: vec![key(0, "w", true)],
                    ticks: 1,
                },
            )
        })
        .unwrap_err();
    rig.host.paused = false;
    assert_eq!(paused.kind, "runtime");
    assert!(paused.message.contains("F8"), "{}", paused.message);
    assert!(
        recorded(&rig.host.session).is_empty(),
        "nothing was pressed"
    );

    let game = the_game(&rig.live_dir).discovery;
    let Rig { live, .. } = rig;
    drop(live);
    let closed = client::Connection::open(&game).err().unwrap();
    assert_eq!((closed.code, closed.kind.as_str()), (2, "no_game"));
}
