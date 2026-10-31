//! `galaxy-gm` through its real executable: exit codes, error kinds and the version gate.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const ENGINE: &str = env!("CARGO_PKG_VERSION");

/// The engine version with its minor number raised by one.
fn newer() -> String {
    let parts: Vec<u32> = ENGINE.split('.').map(|p| p.parse().unwrap()).collect();
    format!("{}.{}.0", parts[0], parts[1] + 1)
}

fn module(dir: &Path, min: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!("id = \"demo\"\nversion = \"0.1.0\"\nmin_engine_version = \"{min}\"\n"),
    )
    .unwrap();
}

fn gm(cwd: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .current_dir(cwd)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

#[test]
fn check_refuses_a_module_that_needs_a_newer_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("demo");
    module(&dir, &newer());
    for args in [
        &["check", "demo"][..],
        &["check", "demo/manifest.toml"],
        &["doctor"],
    ] {
        let cwd = if args[0] == "doctor" {
            dir.clone()
        } else {
            tmp.path().to_path_buf()
        };
        let (code, json) = gm(&cwd, args);
        assert_eq!(code, 2, "{args:?}: {json}");
        assert_eq!(json["kind"], "engine_version");
        assert_eq!(json["engine"], ENGINE);
        assert_eq!(json["required"], newer());
        let message = json["error"].as_str().unwrap();
        assert!(
            message.contains(ENGINE) && message.contains(&newer()),
            "{message}"
        );
        assert!(message.contains("cargo xtask install"), "{message}");
    }
}

#[test]
fn check_passes_the_gate_then_checks_the_module() {
    let tmp = tempfile::tempdir().unwrap();
    module(&tmp.path().join("demo"), ENGINE);
    let (code, json) = gm(tmp.path(), &["check", "demo"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["ok"], true);
    assert_eq!(json["module"], "demo");

    let (code, json) = gm(tmp.path(), &["doctor"]);
    assert_eq!(code, 0, "{json}");
    assert_eq!(json["modules"][0]["id"], "demo");
    assert_eq!(json["engine"], ENGINE);
}

#[test]
fn usage_and_input_errors_are_told_apart() {
    let tmp = tempfile::tempdir().unwrap();
    for args in [
        &[][..],
        &["check"],
        &["check", "a", "b"],
        &["frobnicate"],
        &["doctor", "x"],
    ] {
        let (code, json) = gm(tmp.path(), args);
        assert_eq!(
            (code, json["kind"].as_str()),
            (2, Some("usage")),
            "{args:?}: {json}"
        );
    }
    let (code, json) = gm(tmp.path(), &["check", "missing"]);
    assert_eq!(
        (code, json["kind"].as_str()),
        (4, Some("bad_input")),
        "{json}"
    );
    std::fs::create_dir_all(tmp.path().join("broken")).unwrap();
    std::fs::write(tmp.path().join("broken/manifest.toml"), "id = \"x\"\n").unwrap();
    let (code, json) = gm(tmp.path(), &["check", "broken"]);
    assert_eq!(
        (code, json["kind"].as_str()),
        (4, Some("bad_input")),
        "{json}"
    );
}

#[test]
fn version_and_help_print_text() {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("--version")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(&format!("galaxy-gm {ENGINE}")), "{text}");
}
