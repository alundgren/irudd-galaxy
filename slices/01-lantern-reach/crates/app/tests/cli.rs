//! End-to-end checks of the JSON CLI. Headless: no window, GPU or network.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

fn packs() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs")
}

fn galaxy(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_galaxy"))
        .args(args)
        .env("GALAXY_PACKS", packs())
        .output()
        .expect("galaxy runs");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let json = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {stdout}"));
    (out.status.code().unwrap_or(-1), json)
}

#[test]
fn new_run_inspect() {
    let dir = tempfile::tempdir().unwrap();
    let save = dir.path().join("cli.sqlite");
    let save = save.to_str().unwrap();

    let (code, out) = galaxy(&["new", "--save", save, "--seed", "9"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["tick"], 0);
    assert_eq!(out["stations"], 5);

    let (code, again) = galaxy(&["new", "--save", save]);
    assert_eq!(code, 4, "{again}");

    let (code, run) = galaxy(&["sim", "run", "--save", save, "--ticks", "90"]);
    assert_eq!(code, 0, "{run}");
    assert_eq!(run["tick"], 90);
    let (_, run2) = galaxy(&["sim", "run", "--save", save, "--ticks", "10", "--dry-run"]);
    assert_eq!(run2["from_tick"], 90);
    assert_eq!(run2["tick"], 100);

    // Same seed, same ticks, same hash.
    let other = dir.path().join("other.sqlite");
    let other = other.to_str().unwrap();
    galaxy(&["new", "--save", other, "--seed", "9"]);
    let (_, run_b) = galaxy(&["sim", "run", "--save", other, "--ticks", "90"]);
    assert_eq!(run["hash"], run_b["hash"]);

    let (code, stage) = galaxy(&[
        "state",
        "get",
        "--save",
        save,
        "--path",
        "stations/station.kettle_dock/build/stage",
    ]);
    assert_eq!(code, 0);
    assert_eq!(stage, 0);
    let (_, world) = galaxy(&["state", "get", "--save", save]);
    assert_eq!(world["meta"]["tick"], 90);
    let (code, _) = galaxy(&["state", "get", "--save", save, "--path", "nope"]);
    assert_eq!(code, 4);

    let (code, history) = galaxy(&["history", "query", "--save", save, "--kind", "dock"]);
    assert_eq!(code, 0);
    assert_eq!(history["count"], 0);
    let (code, _) = galaxy(&["history", "query", "--save", save, "--kind", "Not A Kind"]);
    assert_eq!(code, 4);
}

#[test]
fn api_dump_prints_schemas() {
    let (code, all) = galaxy(&["api", "dump"]);
    assert_eq!(code, 0);
    assert!(all["world"]["$defs"]["Station"].is_object());
    let (code, one) = galaxy(&["api", "dump", "--name", "commands"]);
    assert_eq!(code, 0);
    assert_eq!(one["title"], "CommandBatch");
}

#[test]
fn stubs_say_not_implemented() {
    for args in [
        &["play", "--beat", "3"][..],
        &["beat", "jump", "5"],
        &["pack", "check", "packs/core"],
        &["gm", "propose", "./change"],
        &["gm", "rollback", "cause.3"],
    ] {
        let (code, out) = galaxy(args);
        assert_eq!(code, 3, "{args:?}");
        assert_eq!(out["error"], "not implemented yet");
    }
}
