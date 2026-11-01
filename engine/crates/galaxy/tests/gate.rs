//! `galaxy` through its real executable: exit codes, error kinds and the version gate.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

const ENGINE: &str = env!("CARGO_PKG_VERSION");

/// The engine version with its minor number raised by one.
fn newer() -> String {
    let parts: Vec<u32> = ENGINE.split('.').map(|p| p.parse().unwrap()).collect();
    format!("{}.{}.0", parts[0], parts[1] + 1)
}

fn module(dir: &Path, id: &str, min: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!("id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"{min}\"\n"),
    )
    .unwrap();
}

fn galaxy(cwd: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(args)
        .current_dir(cwd)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

#[test]
fn play_refuses_when_any_module_needs_a_newer_engine() {
    let tmp = tempfile::tempdir().unwrap();
    let modules = tmp.path().join("modules");
    module(&modules.join("a"), "a", ENGINE);
    module(&modules.join("b"), "b", &newer());
    for args in [
        &["play", "--modules", "modules"][..],
        &["play", "--modules=modules"],
        &["play", "--modules", "modules/b"],
    ] {
        let (code, json) = galaxy(tmp.path(), args);
        assert_eq!(code, 2, "{args:?}: {json}");
        assert_eq!(json["kind"], "engine_version");
        assert_eq!(json["module"], "b");
        assert_eq!(json["engine"], ENGINE);
        assert_eq!(json["required"], newer());
    }
}

#[test]
fn usage_and_input_errors_are_told_apart() {
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("empty")).unwrap();
    for args in [
        &[][..],
        &["play"],
        &["play", "modules"],
        &["play", "--modules"],
        &["play", "--modules", "a", "b"],
        &["fly"],
    ] {
        let (code, json) = galaxy(tmp.path(), args);
        assert_eq!(
            (code, json["kind"].as_str()),
            (2, Some("usage")),
            "{args:?}: {json}"
        );
    }
    for args in [
        &["play", "--modules", "missing"][..],
        &["play", "--modules", "empty"],
    ] {
        let (code, json) = galaxy(tmp.path(), args);
        assert_eq!(
            (code, json["kind"].as_str()),
            (4, Some("bad_input")),
            "{args:?}: {json}"
        );
    }
}
