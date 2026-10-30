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
    // Dotted paths keep IDs whole.
    let (_, dotted) = galaxy(&[
        "state",
        "get",
        "--save",
        save,
        "--path",
        "stations.station.kettle_dock.build.stage",
    ]);
    assert_eq!(dotted, 0);
    // Reads need a target.
    let (code, _) = galaxy(&["state", "get"]);
    assert_eq!(code, 2);

    let (code, history) = galaxy(&["history", "query", "--save", save, "--kind", "dock"]);
    assert_eq!(code, 0);
    // The four traders arrive at their home docks even while the player stays put.
    assert_eq!(history["count"], 4);
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
    for args in [&["play", "--beat", "3"][..], &["beat", "jump", "5"]] {
        let (code, out) = galaxy(args);
        assert_eq!(code, 3, "{args:?}");
        assert_eq!(out["error"], "not implemented yet");
    }
}

#[test]
fn pack_check_and_pack_test() {
    let examples = packs().join("gm-examples");
    let kettle = examples.join("kettle_growth");
    let kettle = kettle.to_str().unwrap();

    // `--no-typecheck` keeps this independent of `cargo xtask fetch-luau`.
    let (code, report) = galaxy(&["pack", "check", kettle, "--no-typecheck"]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["pack"], "gm.kettle_growth");
    assert_eq!(report["status"]["status"], "accepted");
    let steps: Vec<_> = report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["step"].clone())
        .collect();
    assert_eq!(
        steps,
        [
            "manifest",
            "types",
            "hooks",
            "references",
            "invariants",
            "smoke"
        ]
    );

    let bad = examples.join("bad_script/os_time");
    let (code, report) = galaxy(&["pack", "check", bad.to_str().unwrap(), "--no-typecheck"]);
    assert_eq!(code, 5, "{report}");
    assert_eq!(report["status"]["status"], "rejected");
    let invariants = &report["steps"][4];
    assert_eq!(invariants["outcome"], "failed");
    assert_eq!(invariants["diagnostics"][0]["code"], "clock_api");
    assert_eq!(
        invariants["diagnostics"][0]["location"]["file"],
        "scripts/night_shift.luau"
    );
    assert_eq!(invariants["diagnostics"][0]["location"]["line"], 8);

    let (code, tests) = galaxy(&["pack", "test", kettle]);
    assert_eq!(code, 0, "{tests}");
    assert_eq!(tests["passed"], true);
    assert_eq!(tests["tests"].as_array().unwrap().len(), 2);

    let (code, _) = galaxy(&["pack", "check", "no/such/pack"]);
    assert_eq!(code, 4);
}

#[test]
fn shot_and_play_refuse_bad_input_before_opening_anything() {
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("missing.sqlite");
    let out = dir.path().join("x.png");
    let (code, v) = galaxy(&[
        "shot",
        "--save",
        missing.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ]);
    assert_eq!(code, 4, "{v}");
    let (code, _) = galaxy(&[
        "shot",
        "--save",
        missing.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
        "--size",
        "huge",
    ]);
    assert_eq!(code, 4);
    // --new on an existing save is refused rather than overwriting it.
    let existing = dir.path().join("e.sqlite");
    std::fs::write(&existing, b"x").unwrap();
    let (code, v) = galaxy(&["play", "--save", existing.to_str().unwrap(), "--new"]);
    assert_eq!(code, 4, "{v}");
}
