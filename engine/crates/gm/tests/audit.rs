//! `galaxy-gm audit` on recordings made headless (notes without a frame): the world at
//! each note is checked against the hash the note kept.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_module::module::Module;
use galaxy_sim::{BuildStamp, Session};
use serde_json::Value;

fn stroll() -> Vec<Module> {
    let tests = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/modules");
    ["stroll", "tally"]
        .iter()
        .map(|id| Module::read(&tests.join(id)).unwrap())
        .collect()
}

/// A recording of 90 ticks with notes on ticks 30 and 60.
fn record(dir: &Path) -> PathBuf {
    let mut session = Session::new(stroll(), 3).unwrap();
    session.start_recording(BuildStamp::current());
    for tick in 1..=90 {
        session.step(&[]).unwrap();
        if tick % 30 == 0 && tick < 90 {
            session.note(&format!("at {tick}"), None, None);
        }
    }
    let path = dir.join("recording.json");
    session.recording().unwrap().write(&path).unwrap();
    path
}

fn audit(path: &Path) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("audit")
        .arg(path)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

#[test]
fn each_note_s_world_is_identical_on_replay() {
    let tmp = tempfile::tempdir().unwrap();
    let path = record(tmp.path());
    let (code, out) = audit(&path);
    assert_eq!(code, 0, "{out}");
    let notes = out["notes"].as_array().unwrap();
    assert_eq!(notes.len(), 2);
    assert_eq!(notes[0]["tick"], 30);
    assert_eq!(notes[1]["text"], "at 60");
    assert!(notes.iter().all(|n| n["identical"] == true));
}

#[test]
fn a_note_whose_world_differs_is_reported() {
    let tmp = tempfile::tempdir().unwrap();
    let path = record(tmp.path());
    let text = std::fs::read_to_string(&path).unwrap();
    let mut rec: Value = serde_json::from_str(&text).unwrap();
    let note = rec["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["type"] == "note")
        .unwrap();
    note["hash"] = Value::from("not the world");
    std::fs::write(&path, serde_json::to_string(&rec).unwrap()).unwrap();
    let (code, out) = audit(&path);
    assert_eq!(
        (code, out["kind"].as_str()),
        (5, Some("replay_diverged")),
        "{out}"
    );
    assert_eq!(out["notes"][0]["identical"], false);
    assert_eq!(out["notes"][1]["identical"], true);
}
