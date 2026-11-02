//! `galaxy-gm what-if` through the real executable: a scripted session of the stroll test
//! modules, run again on changed modules.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_module::module::Module;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::{BuildStamp, Session};
use serde_json::{Value, json};

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The session `galaxy sim run --modules tests/modules --seed 7 --ticks 450 --inputs
/// tests/sessions/stroll.toml --record` makes: two turns, a note, and a hot-load of a
/// changed stroll at tick 360.
fn record(dir: &Path) -> PathBuf {
    let modules = ["stroll", "tally"]
        .iter()
        .map(|id| Module::read(&engine().join("tests/modules").join(id)).unwrap())
        .collect();
    let inputs = InputsFile::read(&engine().join("tests/sessions/stroll.toml")).unwrap();
    let by_tick = inputs.by_tick();
    let mut session = Session::new(modules, 7).unwrap();
    session.start_recording(BuildStamp::current());
    for tick in 1..=450 {
        for (at, dir) in &inputs.hot_loads {
            if *at == tick {
                session.hot_load(Module::read(dir).unwrap()).unwrap();
            }
        }
        session
            .step(by_tick.get(&tick).map_or(&[][..], Vec::as_slice))
            .unwrap();
        for (at, text) in &inputs.notes {
            if *at == tick {
                session.note(text, None, None);
            }
        }
    }
    let path = dir.join("recording.json");
    session.recording().unwrap().write(&path).unwrap();
    path
}

fn what_if(recording: &Path, modules: &str, more: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("what-if")
        .arg(recording)
        .args(["--modules", modules])
        .args(more)
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

#[test]
fn a_changed_script_value_first_differs_when_the_walkers_count_steps() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let (code, out) = what_if(&rec, "tests/what-if/stroll", &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["differs"], true, "{out}");
    assert_eq!(out["tick"], 30, "{out}");
    assert_eq!(out["time"], "0:01.0");
    assert_eq!(out["modules"]["changed"], json!(["stroll"]));
    assert_eq!(
        out["skipped_hot_loads"],
        json!([{ "tick": 360, "module": "stroll" }])
    );
    assert!(out["start"].as_str().unwrap().starts_with("rebuilt"));
    assert_eq!(out["baseline"]["matches_recording"], true);
    let entities = out["differences"]["entities"].as_array().unwrap();
    for walker in ["stroll.w1", "stroll.w2"] {
        assert!(
            entities.contains(&json!({
                "entity": walker, "component": "stroll.walker", "field": "steps",
                "recorded": 1, "changed": 2,
            })),
            "{out}"
        );
    }
    let report: Vec<&str> = out["report"]
        .as_array()
        .unwrap()
        .iter()
        .map(|l| l.as_str().unwrap())
        .collect();
    assert!(
        report[0].starts_with("first difference after tick 30 (0:01.0)"),
        "{report:?}"
    );
    assert!(
        report.contains(&"stroll.w1 stroll.walker.steps: recorded 1, changed 2"),
        "{report:?}"
    );
    assert!(report.len() <= 10, "a few lines: {report:?}");
}

#[test]
fn the_same_modules_never_differ() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let (code, out) = what_if(&rec, "tests/modules", &[]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["differs"], false, "{out}");
    assert_eq!(out["modules"]["same"], json!(["stroll", "tally"]));
    assert_eq!(out["skipped_hot_loads"], json!([]));
    let last = out["report"]
        .as_array()
        .unwrap()
        .last()
        .unwrap()
        .as_str()
        .unwrap();
    assert!(last.starts_with("no difference"), "{out}");
    assert_eq!(out["baseline"]["matches_recording"], true);
}

#[test]
fn a_changed_start_default_differs_at_the_start() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    // This stroll declares walker.boosts with a default of 7.
    let (code, out) = what_if(&rec, "tests/hot-load/stroll", &["--all"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["tick"], 0, "{out}");
    assert!(
        out["differences"]["entities"]
            .as_array()
            .unwrap()
            .contains(&json!({
                "entity": "stroll.w1", "component": "stroll.walker", "field": "boosts",
                "recorded": null, "changed": 7,
            })),
        "{out}"
    );
    assert!(
        out["report"][0]
            .as_str()
            .unwrap()
            .contains("at the start (tick 0)")
    );
}

#[test]
fn bad_command_lines_are_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("what-if")
        .arg(&rec)
        .current_dir(engine())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let (code, out) = what_if(&rec, "tests/no-such-folder", &[]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
}
