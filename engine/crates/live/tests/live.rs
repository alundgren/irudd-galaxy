//! The live socket end to end, headless: a real server and client over a local socket,
//! with a session standing in for the window (`Headless`).

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use galaxy_live::client::{self, Game};
use galaxy_live::{Host, Live, Op, RunningModule, ServerOptions, WireError};
use galaxy_module::module::Module;
use galaxy_render::RgbaImage;
use galaxy_sim::{BuildStamp, Session};
use serde_json::{Map, Value, json};

/// The test modules, copied so a test can edit them.
fn modules(into: &Path) -> Vec<PathBuf> {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/modules");
    let mut dirs = Vec::new();
    for name in ["stroll", "tally"] {
        let to = into.join(name);
        copy(&src.join(name), &to);
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

/// A game without a window: the session, and where its recording goes.
struct Headless {
    session: Session,
    dir: PathBuf,
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
}

struct Rig {
    _tmp: tempfile::TempDir,
    live_dir: PathBuf,
    module_dirs: Vec<PathBuf>,
    host: Headless,
    live: Live,
}

fn rig() -> Rig {
    let tmp = tempfile::tempdir().unwrap();
    let module_dirs = modules(&tmp.path().join("modules"));
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
            session,
            dir: session_dir,
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
            if self.live.watching() {
                self.host.session.step(&[]).unwrap();
                self.live.after_tick(&self.host.session.world);
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
    assert!(
        loaded["recording"]
            .as_str()
            .unwrap()
            .ends_with("recording.json")
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

    // A folder that isn't a running module is a usage mistake.
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
