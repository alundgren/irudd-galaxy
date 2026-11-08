//! `galaxy-gm live` argument handling through the real executable, with no game running:
//! a command that takes its arguments gets as far as choosing a game.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

/// Runs `galaxy-gm live <args>` against an empty live folder: its exit code and the error
/// kind it reports.
fn live(args: &[&str]) -> (i32, String) {
    let empty = tempfile::tempdir().unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("live")
        .args(args)
        .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")))
        .env("GALAXY_LIVE", empty.path())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let kind = text
        .lines()
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
        .or_else(|| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| v["kind"].as_str().map(str::to_owned))
        .unwrap_or_else(|| text.clone());
    (out.status.code().unwrap(), kind)
}

#[test]
fn live_debug_takes_its_event_and_payload() {
    // The event and payload are accepted; with no game running, choosing one fails.
    let (_, kind) = live(&["debug", "grant", "coins=5"]);
    assert_eq!(kind, "no_game");
    let (_, kind) = live(&["debug", "grant"]);
    assert_eq!(kind, "no_game");
    // Without an event it is a usage error, as is a payload that isn't field=value.
    let (code, kind) = live(&["debug"]);
    assert_eq!((code, kind.as_str()), (2, "usage"));
    let (_, kind) = live(&["debug", "grant", "coins"]);
    assert_eq!(kind, "usage");
}

#[test]
fn live_edit_takes_its_edits() {
    let (_, kind) = live(&["edit", "set walker pockets coins=5"]);
    assert_eq!(kind, "no_game");
}
