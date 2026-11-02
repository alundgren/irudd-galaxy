//! `galaxy-gm test` through the real executable, on a passing and a failing test module.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn gm(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

#[test]
fn a_module_whose_tests_pass() {
    let (code, out) = gm(&["test", "tests/modules/stroll"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["ok"], true);
    assert_eq!(out["context_modules"], serde_json::json!(["tally"]));
    let tests = out["tests"].as_array().unwrap();
    assert_eq!(tests.len(), 3, "{out}");
    assert!(
        tests
            .iter()
            .all(|t| t["file"] == "tests/rules.luau" && t["passed"] == true)
    );
    assert_eq!(out["scenarios"][0]["file"], "tests/climb.toml");
    assert_eq!(out["scenarios"][0]["passed"], true);
    assert_eq!(
        (out["passed"].as_u64(), out["failed"].as_u64()),
        (Some(4), Some(0))
    );
}

#[test]
fn failing_tests_say_what_failed_and_exit_5() {
    let (code, out) = gm(&["test", "tests/failing/counter"]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (5, Some("tests_failed")),
        "{out}"
    );
    assert_eq!(
        (out["passed"].as_u64(), out["failed"].as_u64()),
        (Some(2), Some(2)),
        "{out}"
    );
    let failures: Vec<&str> = out["failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(failures.len(), 3, "a line a failure: {failures:?}");
    // The Luau test names its file, line and message.
    assert!(
        failures[0].starts_with("tests/sums.luau a_bump_adds_one: tests/sums.luau:11:")
            && failures[0].contains("a bump adds one"),
        "{failures:?}"
    );
    // The scenario names the tick, the value found and the one expected; its later
    // expectation fails too, and the facts expectation passes.
    let bumps = out["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["file"] == "tests/bumps.toml")
        .unwrap();
    assert_eq!(bumps["passed"], false);
    let found: Vec<&str> = bumps["failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert_eq!(
        found,
        [
            "after tick 7: `counter.board` `counter.count.n` is 2, expected 1",
            "after tick 20: `counter.board` `counter.count.n` is 4, expected 2",
        ]
    );
    assert!(
        failures[1].starts_with("tests/bumps.toml: after tick 7"),
        "{failures:?}"
    );
}

#[test]
fn only_runs_the_tests_it_names() {
    let (code, out) = gm(&["test", "tests/failing/counter", "--only", "quiet"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["passed"], 1);
    assert_eq!(out["tests"].as_array().unwrap().len(), 0);
    let (code, out) = gm(&[
        "test",
        "tests/failing/counter",
        "--only",
        "nothing-has-this",
    ]);
    assert_eq!(code, 0, "{out}");
    assert!(
        out["notes"][0]
            .as_str()
            .unwrap()
            .contains("nothing-has-this")
    );
}

#[test]
fn bad_command_lines_and_modules_are_refused() {
    let (code, out) = gm(&["test"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
    let (code, out) = gm(&["test", "tests/no-such-module"]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
}

#[test]
fn malformed_bot_is_reported_once_without_opening_a_window() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("sample");
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        "id = 'sample'\nversion = '0.1.0'\nmin_engine_version = '0.1.0'",
    )
    .unwrap();
    std::fs::write(
        dir.join("tests/run.bot.toml"),
        "inputs = 'missing.inputs'\nticks = 0\nsize = [1280, 720]",
    )
    .unwrap();
    let (code, report) = gm(&["test", dir.to_str().unwrap()]);
    assert_eq!(code, 5, "{report}");
    assert_eq!(report["failed"], 1);
    assert_eq!(report["scenarios"].as_array().unwrap().len(), 0);
    assert_eq!(report["bots"][0]["status"], "failed");
    let (code, report) = gm(&["test", dir.to_str().unwrap(), "--only", "nothing"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["bots"].as_array().unwrap().len(), 0);
}

#[test]
#[ignore = "opens a GPU window; build the matching galaxy binary first"]
fn declared_bot_reports_physical_size_timing_and_replay() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("sample");
    std::fs::create_dir_all(dir.join("tests")).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        "id = 'sample'\nversion = '0.1.0'\nmin_engine_version = '0.1.0'",
    )
    .unwrap();
    std::fs::write(dir.join("tests/run.inputs"), "").unwrap();
    std::fs::write(
        dir.join("tests/run.bot.toml"),
        "inputs = 'tests/run.inputs'\nticks = 60\nsize = [1280, 720]",
    )
    .unwrap();
    let (code, report) = gm(&["test", dir.to_str().unwrap()]);
    assert_eq!(code, 0, "{report}");
    let bot = &report["bots"][0];
    assert_eq!(bot["size"], serde_json::json!([1280, 720]));
    assert_eq!(bot["replay"]["identical"], true);
    assert!(bot["frames"].as_u64().unwrap() > 10);
    assert!(bot["frame_time"]["fps"].as_f64().unwrap() > 0.0);
    for key in ["median", "p95", "p99", "max"] {
        assert!(bot["frame_time"]["interval_ms"][key].as_f64().unwrap() > 0.0);
    }
}
