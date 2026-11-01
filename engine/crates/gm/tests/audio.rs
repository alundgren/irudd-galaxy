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
        "id='demo'\nversion='0.1.0'\nmin_engine_version='0.3.0'\n",
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
