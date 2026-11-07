//! `galaxy sim` through the real executable, on the generic test modules: run, save, load,
//! hot-load and replay.
//!
//! [`GOLDEN`] is the hash of the world the scripted session ends with. The same test runs on
//! every OS, so passing on two of them means the sim gives the same bits on both. Change it
//! only when the world's format, the engine's tick or the test modules change on purpose,
//! and say so in the PR.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The world hash after the scripted stroll session (450 ticks, seed 7).
const GOLDEN: &str = "192594704e3d0234af754b608f715da094f8eea3564fcc8f5b19eb10b8352499";

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn galaxy(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(args)
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&out.stdout).into_owned()));
    (out.status.code().unwrap(), json)
}

fn path(p: &Path) -> &str {
    p.to_str().unwrap()
}

#[test]
fn run_save_load_hot_load_and_replay_agree() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("end.db");
    let rec = tmp.path().join("session.json");
    let (code, run) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/modules",
        "--seed",
        "7",
        "--ticks",
        "450",
        "--inputs",
        "tests/sessions/stroll.toml",
        "--save",
        path(&save),
        "--record",
        path(&rec),
    ]);
    assert_eq!(code, 0, "{run}");
    let hash = run["hash"].as_str().unwrap().to_owned();
    assert_eq!(run["tick"], 450);
    assert_eq!(run["script_failures"], Value::Array(Vec::new()), "{run}");
    assert_eq!(run["hot_loads"][0]["module"], "stroll");
    assert_eq!(run["hot_loads"][0]["tick"], 359);

    // The recording names this build and holds the hot-loaded files.
    let recording: Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    assert_eq!(recording["build"]["version"], env!("CARGO_PKG_VERSION"));
    let events = recording["events"].as_array().unwrap();
    let kinds: Vec<&str> = events.iter().filter_map(|e| e["type"].as_str()).collect();
    assert_eq!(kinds, ["input", "note", "input", "hot_load"]);
    assert_eq!(events[0]["source"], "keyboard");
    assert!(
        events[3]["files"]["scripts/rules.luau"]
            .as_str()
            .unwrap()
            .contains("STEP = 10")
    );

    // Replays, twice, end on the same world.
    for _ in 0..2 {
        let (code, replay) = galaxy(&["sim", "replay", path(&rec)]);
        assert_eq!(code, 0, "{replay}");
        assert_eq!(replay["identical"], true);
        assert_eq!(replay["hash"], hash.as_str());
        assert_eq!(replay["notes"][0]["tick"], 150);
    }

    // The save was made after the hot-load: the original stroll can't hold its data, so
    // loading it with the original modules is refused rather than losing a field.
    let (code, load) = galaxy(&["sim", "load", path(&save), "--modules", "tests/modules"]);
    assert_eq!(
        (code, load["kind"].as_str()),
        (4, Some("bad_input")),
        "{load}"
    );
    assert!(load["error"].as_str().unwrap().contains("boosts"), "{load}");

    assert_eq!(
        hash, GOLDEN,
        "the scripted session's world changed; if that was on purpose, update GOLDEN"
    );
}

#[test]
fn a_saved_run_carries_on_like_one_long_run() {
    let tmp = tempfile::tempdir().unwrap();
    let half = tmp.path().join("half.db");
    // Loading the save brings one input, `engine.resumed` on the tick after it; the long
    // run gets the same input, and ends on the same world.
    let resumed = tmp.path().join("resumed.toml");
    std::fs::write(
        &resumed,
        "[[input]]
tick = 121
name = 'engine.resumed'
source = 'resume'
",
    )
    .unwrap();
    let base = ["sim", "run", "--modules", "tests/modules", "--seed", "3"];
    let (_, straight) =
        galaxy(&[&base[..], &["--ticks", "200", "--inputs", path(&resumed)]].concat());
    let (code, _) = galaxy(&[&base[..], &["--ticks", "120", "--save", path(&half)]].concat());
    assert_eq!(code, 0);
    let (code, carried) = galaxy(&[
        "sim",
        "load",
        path(&half),
        "--modules",
        "tests/modules",
        "--ticks",
        "80",
        "--state",
        "--all",
    ]);
    assert_eq!(code, 0, "{carried}");
    assert_eq!(carried["tick"], 200);
    assert_eq!(carried["modules"][0]["version"], "0.1.0");
    assert_eq!(carried["hash"], straight["hash"]);
    let w1 = &carried["state"]["entities"]["stroll.w1"];
    assert_eq!(w1["engine.position"]["layer"], 0);
    assert!(
        w1["engine.position"]["height"].as_f64().unwrap() > 0.0,
        "{w1}"
    );
    assert!(carried["state"]["facts"].is_array(), "{carried}");

    // --state alone: a line an entity, the facts counted, and how to get the rest.
    let (code, compact) = galaxy(&[&base[..], &["--ticks", "200", "--state"]].concat());
    assert_eq!(code, 0, "{compact}");
    let state = &compact["state"];
    let line = state["entities"]["stroll.w1"].as_str().unwrap();
    assert!(
        line.starts_with("at (") && line.contains("engine.position"),
        "{line}"
    );
    assert!(state["facts"]["count"].is_u64(), "{state}");
    assert!(
        state["compact"].as_str().unwrap().contains("--all"),
        "{state}"
    );
    let (code, err) = galaxy(&[&base[..], &["--all"]].concat());
    assert_eq!(code, 2, "{err}");
}

/// A save resumed headless tells its scripts with `engine.resumed`, and the tune module
/// cues its music again with no new input. The resumed session replays identically and
/// ends where an uninterrupted run given the same `engine.resumed` input does.
#[test]
fn a_resumed_save_tells_scripts_and_its_music_cue_comes_back() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("mid.db");
    let rec = tmp.path().join("resumed.json");
    let modules = ["--modules", "tests/modules/resume", "--seed", "7"];
    let (code, mid) = galaxy(
        &[
            &["sim", "run"][..],
            &modules,
            &[
                "--ticks",
                "60",
                "--inputs",
                "tests/modules/resume/inputs.toml",
                "--save",
                path(&save),
            ],
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{mid}");
    // Loading with no ticks to run announces nothing yet: the world is the saved one.
    let (code, still) = galaxy(
        &[
            &["sim", "load", path(&save)][..],
            &modules[..2],
            &["--ticks", "0"],
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{still}");
    assert_eq!(still["hash"], mid["hash"]);

    let (code, carried) = galaxy(
        &[
            &["sim", "load", path(&save)][..],
            &modules[..2],
            &["--ticks", "30", "--record", path(&rec), "--state", "--all"],
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{carried}");
    assert_eq!(
        carried["script_failures"],
        Value::Array(Vec::new()),
        "{carried}"
    );
    assert_eq!(carried["log"][0]["text"], "resumed on tick 61 from resume");
    assert_eq!(carried["log"][0]["tick"], 61);
    let state = &carried["state"];
    let facts = state["facts"].as_array().unwrap();
    let causes = state["causes"].as_array().unwrap();
    let after: Vec<&Value> = facts
        .iter()
        .filter(|f| f["tick"].as_u64() > Some(60))
        .collect();
    let kinds: Vec<&str> = after.iter().map(|f| f["kind"].as_str().unwrap()).collect();
    assert_eq!(kinds, ["engine.resumed", "tune.theme"], "{after:?}");
    let cause = |f: &Value| &causes[f["cause"].as_u64().unwrap() as usize]["cause"];
    assert_eq!(cause(after[0])["kind"], "input");
    assert_eq!(cause(after[0])["source"], "resume");
    assert_eq!(cause(after[1])["kind"], "script");
    assert_eq!(cause(after[1])["handler"], "engine.resumed");
    // The theme played once before the save: the resumed cue is the only new one.
    let themes = facts.iter().filter(|f| f["kind"] == "tune.theme").count();
    assert_eq!(themes, 2);

    // The recording holds the resume as its one input, and replays to the same world.
    let recording: Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    let events = recording["events"].as_array().unwrap();
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0]["name"], "engine.resumed");
    assert_eq!(events[0]["tick"], 61);
    for _ in 0..2 {
        let (code, replay) = galaxy(&["sim", "replay", path(&rec)]);
        assert_eq!(code, 0, "{replay}");
        assert_eq!(replay["identical"], true);
        assert_eq!(replay["hash"], carried["hash"]);
    }

    // An uninterrupted run with that one input ends on the resumed world.
    let (code, straight) = galaxy(
        &[
            &["sim", "run"][..],
            &modules,
            &[
                "--ticks",
                "90",
                "--inputs",
                "tests/modules/resume/straight.toml",
            ],
        ]
        .concat(),
    );
    assert_eq!(code, 0, "{straight}");
    assert_eq!(straight["hash"], carried["hash"]);
}

#[test]
fn bad_input_and_divergence_are_told_apart() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = tmp.path().join("r.json");
    let (code, _) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/modules",
        "--ticks",
        "40",
        "--record",
        path(&rec),
    ]);
    assert_eq!(code, 0);
    let mut recording: Value =
        serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    recording["end"]["hash"] = Value::String("0".repeat(64));
    std::fs::write(&rec, recording.to_string()).unwrap();
    let (code, out) = galaxy(&["sim", "replay", path(&rec)]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (5, Some("replay_diverged")),
        "{out}"
    );

    let (code, out) = galaxy(&["sim", "load", path(&rec), "--modules", "tests/modules"]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
    let (code, out) = galaxy(&["sim", "run"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
    let (code, out) = galaxy(&["sim", "run", "--modules", "tests/modules", "--ticks", "x"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
    let (code, out) = galaxy(&["sim", "walk"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
    let (code, out) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/modules",
        "--inputs",
        "none.toml",
    ]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
}

#[test]
fn play_load_checks_arguments_and_save_before_opening_a_window() {
    let (code, result) = galaxy(&[
        "play",
        "--modules",
        "tests/modules",
        "--load",
        "missing.db",
        "--seed",
        "1",
    ]);
    assert_eq!(code, 2, "{result}");
    assert!(result["error"].as_str().unwrap().contains("saved seed"));
    let (code, result) = galaxy(&["play", "--modules", "tests/modules", "--load", "missing.db"]);
    assert_eq!(code, 4, "{result}");
    assert!(result["error"].as_str().unwrap().contains("no save"));
}

#[test]
#[ignore = "opens a GPU window"]
fn a_resumed_window_records_the_saved_start_and_final_tick_note() {
    let tmp = tempfile::tempdir().unwrap();
    let save = tmp.path().join("mid.db");
    let inputs = tmp.path().join("run.inputs");
    let record = tmp.path().join("resumed.json");
    std::fs::write(&inputs, "[[note]]\ntick = 60\ntext = 'resumed final tick'").unwrap();
    let (code, first) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/play/audio/demo",
        "--ticks",
        "30",
        "--save",
        path(&save),
    ]);
    assert_eq!(code, 0, "{first}");
    let (code, played) = galaxy(&[
        "play",
        "--modules",
        "tests/play/audio/demo",
        "--load",
        path(&save),
        "--ticks",
        "60",
        "--inputs",
        path(&inputs),
        "--record",
        path(&record),
        "--size",
        "1280x720",
        "--physical-size",
        "--no-live",
    ]);
    assert_eq!(code, 0, "{played}");
    assert_eq!(played["tick"], 60);
    assert_eq!(played["notes"], 1);
    assert_eq!(played["size"], serde_json::json!([1280, 720]));
    let recording = galaxy_sim::Recording::read(&record).unwrap();
    assert_eq!(recording.start.tick, 30);
    assert_eq!(recording.start.hash(), first["hash"]);
    let (code, replay) = galaxy(&["sim", "replay", path(&record)]);
    assert_eq!(code, 0, "{replay}");
    assert_eq!(replay["identical"], true);
}

/// An inputs file's edits are made by `sim run` and by a headless `play` alike, recorded,
/// and replayed to the same world.
#[test]
fn inputs_file_edits_run_record_and_replay_the_same_in_sim_and_play() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = tmp.path().join("session.json");
    let (code, run) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/edits",
        "--ticks",
        "10",
        "--inputs",
        "tests/edits/inputs.toml",
        "--record",
        path(&rec),
    ]);
    assert_eq!(code, 0, "{run}");
    let ticks: Vec<&Value> = run["edits"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| &e["tick"])
        .collect();
    assert_eq!(ticks, [0, 1, 4], "{run}");
    let (code, replayed) = galaxy(&["sim", "replay", path(&rec)]);
    assert_eq!(code, 0, "{replayed}");
    assert_eq!(replayed["identical"], true);

    let sessions = tmp.path().join("sessions");
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args([
            "play",
            "--headless",
            "--modules",
            "tests/edits",
            "--ticks",
            "10",
            "--inputs",
            "tests/edits/inputs.toml",
        ])
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .env("GALAXY_SESSIONS", &sessions)
        .output()
        .unwrap();
    let played: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(out.status.code(), Some(0), "{played}");
    assert_eq!(
        played["hash"], run["hash"],
        "play makes the same edits as sim run"
    );
    let session = std::fs::read_dir(&sessions)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let recording = session.join("recording.json");
    let text = std::fs::read_to_string(&recording).unwrap();
    assert_eq!(text.matches("\"type\":\"edit\"").count(), 3);
    let (code, replayed) = galaxy(&["sim", "replay", path(&recording)]);
    assert_eq!(code, 0, "{replayed}");
    assert_eq!(replayed["identical"], true);

    // A mistyped edit is refused before anything runs, naming what is wrong.
    let bad = tmp.path().join("bad.toml");
    std::fs::write(
        &bad,
        "[[edit]]\nentity = 'purse.buyer'\ncomponent = 'purse.wallet'\nset = { gold = 1 }\n",
    )
    .unwrap();
    let (code, refused) = galaxy(&[
        "sim",
        "run",
        "--modules",
        "tests/edits",
        "--ticks",
        "1",
        "--inputs",
        path(&bad),
    ]);
    assert_eq!(code, 4, "{refused}");
    assert!(
        refused["error"].as_str().unwrap().contains("gold"),
        "{refused}"
    );
}
