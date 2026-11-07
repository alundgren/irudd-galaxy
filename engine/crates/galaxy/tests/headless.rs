//! `galaxy play --headless`: a bot's clicks, keys and resting buttons resolved against the
//! screen a window of the same size and scale would show, with no window, GPU or audio
//! device. The module is `engine/tests/play/headless`, played by `headless-inputs.toml`.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_module::module::Module;
use galaxy_render::Rect;
use galaxy_render::glam::Vec2;
use galaxy_sim::recording::RecEvent;
use galaxy_sim::{Recording, Session};
use galaxy_view::View;
use serde_json::{Value, json};

fn play_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play")
}

/// Plays the fixture headless at 1280x720 physical pixels and 125% scale; the report, and
/// the session folder's recording.
fn play(sessions: &Path, extra: &[&str]) -> (Value, Option<Recording>) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy"))
        .args(["play", "--headless", "--modules"])
        .arg(play_dir().join("headless"))
        .arg("--inputs")
        .arg(play_dir().join("headless-inputs.toml"))
        .args(["--size", "1280x720", "--physical-size", "--scale", "1.25"])
        .args(extra)
        .env("GALAXY_SESSIONS", sessions)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let report: Value = serde_json::from_slice(&out.stdout).unwrap_or_else(|_| {
        panic!(
            "no JSON report: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )
    });
    let recording = report["session"]
        .as_str()
        .map(|dir| Recording::read(&Path::new(dir).join("recording.json")).unwrap());
    (report, recording)
}

fn inputs(recording: &Recording) -> Vec<(u64, String, Value)> {
    recording
        .events
        .iter()
        .filter_map(|e| match e {
            RecEvent::Input(i) => Some((
                i.tick,
                i.name.clone(),
                i.payload.get("down").cloned().unwrap_or(Value::Null),
            )),
            _ => None,
        })
        .collect()
}

/// Where the Catch button is on the 1024x576 logical screen when the world is at `tick`:
/// the layout a bot click before `tick + 1` is resolved against.
fn catch_at(tick: u64) -> Rect {
    fn find(b: &galaxy_view::screens::Boxed) -> Option<Rect> {
        if b.widget.id.as_deref() == Some("catch") {
            return Some(b.rect);
        }
        b.children.iter().find_map(find)
    }
    let module = Module::read(&play_dir().join("headless")).unwrap();
    let mut session = Session::new(vec![module], 1).unwrap();
    let mut view = View::detached(session.modules(), 1);
    while session.world.tick < tick {
        view.before_step(&session.world);
        session.step(&[]).unwrap();
    }
    view.scene(&session.world, 1.0, Vec2::new(1024.0, 576.0), 0.0);
    view.screens
        .boxes
        .iter()
        .find_map(find)
        .expect("Catch is on screen")
}

#[test]
fn a_headless_bot_clicks_a_moving_widget_skips_a_resting_one_and_presses_keys() {
    // The button rides on a marker moving across the screen: by the second click it is
    // nowhere near where it was, so the click has to follow it.
    let (first, second) = (catch_at(9), catch_at(49));
    assert!(
        second.x > first.x + first.w,
        "Catch moved from {first:?} to {second:?}"
    );

    let sessions = tempfile::tempdir().unwrap();
    let (report, recording) = play(sessions.path(), &["--ticks", "80"]);
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["headless"], true);
    assert_eq!(report["tick"], 80);
    assert_eq!(report["size"], json!([1280, 720]));
    assert_eq!(report["scale"], 1.25);
    assert_eq!(report["frames"], 81, "the start, then a frame a tick");
    assert_eq!(report["frame_time"], Value::Null);
    assert_eq!(report["live"]["served"], false);
    assert_eq!(report["problems"], json!([]), "{report}");
    // The click on the resting button is reported with its reason and records nothing.
    assert_eq!(
        report["resting_clicks"],
        json!([{"tick": 5, "widget": "headless:scripts/screen.luau/screen/spend[2]", "resting": "Nothing caught to spend."}])
    );

    let recording = recording.unwrap();
    assert_eq!(
        inputs(&recording),
        [
            (10, "headless.catch".into(), Value::Null),
            (50, "headless.catch".into(), Value::Null),
            (55, "headless.spend".into(), Value::Null),
            (60, "headless.toggle".into(), Value::Bool(true)),
            (61, "headless.toggle".into(), Value::Bool(false)),
            (64, "headless.toggle".into(), Value::Bool(true)),
            (65, "headless.toggle".into(), Value::Bool(false)),
            (70, "headless.spend".into(), Value::Null),
        ]
    );
    let (replayed, replay) = galaxy_sim::replay(&recording).unwrap();
    assert!(replay.identical);
    assert_eq!(json!(replay.hash), report["hash"]);
    let tally = &replayed
        .world
        .component("headless.state", "headless.tally")
        .unwrap()
        .fields;
    assert_eq!(
        (&tally["caught"], &tally["spent"], &tally["open"]),
        (&json!(2), &json!(2), &json!(true))
    );
    let session = PathBuf::from(report["session"].as_str().unwrap());
    assert!(session.join("save.db").is_file());

    // Time is the world's, not the wall clock's: a second run records the same session,
    // presentation and UI sounds included.
    let (again, recording_again) = play(sessions.path(), &["--ticks", "80"]);
    assert_eq!(again["hash"], report["hash"]);
    assert_ne!(again["session"], report["session"], "a folder of its own");
    let same = |r: &Recording| {
        let mut v = serde_json::to_value(r).unwrap();
        v.as_object_mut().unwrap().remove("build");
        v
    };
    assert_eq!(same(&recording_again.unwrap()), same(&recording));
}

#[test]
fn headless_play_needs_ticks_and_refuses_watching() {
    let sessions = tempfile::tempdir().unwrap();
    for extra in [&[][..], &["--ticks", "10", "--watch"]] {
        let (report, _) = play(sessions.path(), extra);
        assert_eq!(report["kind"], "usage", "{report}");
    }
}
