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
    let base = ["sim", "run", "--modules", "tests/modules", "--seed", "3"];
    let (_, straight) = galaxy(&[&base[..], &["--ticks", "200"]].concat());
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
