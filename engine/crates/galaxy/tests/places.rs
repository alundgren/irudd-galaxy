//! Real command-line proof, including a save taken during an unseen walk.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

const MODULES: &str = "tests/modules/places";
// Shared by Windows and Linux: changing the model deliberately updates this hash.
const GOLDEN: &str = "a6c304441e46d3f13c0b46f5de6e9631923a836e148f11a3d085d45413d3d463";

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn galaxy(args: &[&str]) -> Value {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(args)
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{v}");
    v
}

#[test]
fn places_run_save_load_and_replay_have_the_same_world() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("walk.db");
    let rec = tmp.path().join("walk.json");
    let base = [
        "sim",
        "run",
        "--modules",
        MODULES,
        "--seed",
        "7",
        "--inputs",
        "tests/modules/places/inputs.toml",
    ];
    let mid = galaxy(
        &[
            &base[..],
            &[
                "--ticks",
                "33",
                "--save",
                db.to_str().unwrap(),
                "--record",
                rec.to_str().unwrap(),
                "--state",
                "--all",
            ],
        ]
        .concat(),
    );
    assert_eq!(mid["script_failures"], serde_json::json!([]));
    assert_eq!(
        mid["state"]["entities"]["places.visitor"]["engine.walk"]["active"],
        true
    );
    assert_eq!(
        mid["state"]["entities"]["places.visitor"]["engine.position"]["x"],
        1.0
    );
    let loaded = galaxy(&[
        "sim",
        "load",
        db.to_str().unwrap(),
        "--modules",
        MODULES,
        "--ticks",
        "0",
    ]);
    assert_eq!(loaded["hash"], mid["hash"]);
    let replayed = galaxy(&["sim", "replay", rec.to_str().unwrap()]);
    assert_eq!(replayed["identical"], true);
    assert_eq!(replayed["hash"], mid["hash"]);
    let end = galaxy(
        &[
            &base[..],
            &[
                "--ticks",
                "95",
                "--record",
                rec.to_str().unwrap(),
                "--state",
                "--all",
            ],
        ]
        .concat(),
    );
    let carried = galaxy(&[
        "sim",
        "load",
        db.to_str().unwrap(),
        "--modules",
        MODULES,
        "--ticks",
        "62",
    ]);
    assert_eq!(carried["hash"], end["hash"]);
    let replayed = galaxy(&["sim", "replay", rec.to_str().unwrap()]);
    assert_eq!(replayed["identical"], true);
    assert_eq!(replayed["hash"], end["hash"]);
    let entities = &end["state"]["entities"];
    assert_eq!(entities["places.visitor"]["engine.walk"]["active"], false);
    assert_eq!(
        entities["places.visitor"]["engine.location"]["spot"],
        "places.b_table"
    );
    assert_eq!(
        entities["places.visitor"]["engine.being"]["state"],
        "resting"
    );
    assert_eq!(
        entities["places.player"]["engine.location"]["place"],
        "places.room_a"
    );
    assert_eq!(entities["places.clock"]["engine.clock"]["value"], 5);
    for id in ["places.probe_a", "places.probe_b"] {
        assert_eq!(entities[id]["engine.position"]["x"], 20.0);
        assert_eq!(entities[id]["engine.position"]["y"], 0.0);
    }
    assert_eq!(end["hash"], GOLDEN, "place model hash changed");
}
