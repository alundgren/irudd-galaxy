use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn check(path: &Path) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("check")
        .arg(path)
        .current_dir(engine())
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap(),
    )
}

#[test]
fn sound_workshop_passes_and_missing_or_corrupt_sounds_are_findings() {
    let (code, out) = check(&engine().join("tests/play/audio/demo"));
    assert_eq!(code, 0, "{out}");
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("demo");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(
        root.join("manifest.toml"),
        "id='demo'\nversion='0.1.0'\nmin_engine_version='0.4.0'\n",
    )
    .unwrap();
    std::fs::write(
        root.join("audio.toml"),
        "[sounds.missing]\nfile='missing.ogg'\n[sounds.bad]\nfile='bad.wav'\n",
    )
    .unwrap();
    std::fs::write(root.join("bad.wav"), b"broken").unwrap();
    let (code, out) = check(&root);
    assert_eq!(code, 5, "{out}");
    let findings = out["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 2, "{out}");
    assert!(findings.iter().all(|f| f["kind"] == "sound"));
    for path in ["missing.ogg", "bad.wav"] {
        assert!(
            findings
                .iter()
                .any(|f| f["message"].as_str().unwrap().contains(path)),
            "{out}"
        );
    }
}

/// Without OPENROUTER_API_KEY, music says so with exit 2 and kind no_key, prints one
/// JSON object and nothing else, and leaves the module as it was.
#[test]
fn music_without_a_key_says_so_and_writes_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("quiet");
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    let src = engine().join("tests/play/music/quiet");
    for f in ["manifest.toml", "start.toml", "scripts/quiet.luau"] {
        std::fs::copy(src.join(f), root.join(f)).unwrap();
    }
    let (code, out) = check(&root);
    assert_eq!(code, 0, "the quiet module checks clean: {out}");
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(["music", root.to_str().unwrap(), "--name", "calm"])
        .args(["--prompt", "slow warm pads", "--event", "engine.start"])
        .env_remove("OPENROUTER_API_KEY")
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(
        out.stderr.is_empty(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["kind"], "no_key", "{json}");
    assert_eq!(json["env"], "OPENROUTER_API_KEY");
    assert!(!root.join("music").exists() && !root.join("audio.toml").exists());
}
