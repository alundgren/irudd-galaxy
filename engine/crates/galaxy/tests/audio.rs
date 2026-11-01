//! The installed-style offline entry point uses no device and preserves the sim hash.
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn engine() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn run(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(args)
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
fn startup_music_plays_without_an_injected_event() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("manifest.toml"),
        "id = 'startup'\nversion = '0.1.0'\nmin_engine_version = '0.3.0'\n",
    )
    .unwrap();
    std::fs::copy(
        engine().join("tests/play/audio/demo/sounds/music.wav"),
        dir.path().join("theme.wav"),
    )
    .unwrap();
    std::fs::write(
        dir.path().join("audio.toml"),
        "[sounds.theme]\nfile = 'theme.wav'\n\
         [[events]]\nevent = 'engine.start'\nsound = 'theme'\nbus = 'music'\n",
    )
    .unwrap();
    let wav = dir.path().join("startup.wav");
    let (code, mix) = run(&[
        "audio",
        "--modules",
        dir.path().to_str().unwrap(),
        "--ticks",
        "60",
        "--out",
        wav.to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{mix}");
    assert_eq!(mix["max_voices"], 1, "startup must play exactly once");
    assert!(mix["peak"].as_f64().unwrap() > 0.1, "{mix}");
}

#[test]
fn audio_renders_a_clean_wav_and_does_not_overwrite_files() {
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("mix.wav");
    let path = wav.to_str().unwrap();
    let args = [
        "audio",
        "--modules",
        "tests/play/audio",
        "--inputs",
        "tests/play/audio-inputs.toml",
        "--ticks",
        "300",
        "--out",
        path,
    ];
    let (code, mix) = run(&args);
    assert_eq!(code, 0, "{mix}");
    assert_eq!(mix["frames"], 480000);
    assert_eq!(mix["limited_samples"], 0);
    assert!(mix["peak"].as_f64().unwrap() > 0.1);
    assert!(mix["peak"].as_f64().unwrap() < 0.3);
    let reader = hound::WavReader::open(&wav).unwrap();
    assert_eq!(reader.spec().channels, 2);
    assert_eq!(reader.spec().sample_rate, 48000);
    assert_eq!(reader.duration(), 480000);
    let (_, sim) = run(&[
        "sim",
        "run",
        "--modules",
        "tests/play/audio",
        "--inputs",
        "tests/play/audio-inputs.toml",
        "--ticks",
        "300",
    ]);
    assert_eq!(mix["world_hash"], sim["hash"]);
    let original = std::fs::read(&wav).unwrap();
    let (code, _) = run(&args);
    assert_ne!(code, 0);
    assert_eq!(std::fs::read(wav).unwrap(), original);
}
