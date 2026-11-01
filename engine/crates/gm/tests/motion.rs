//! `galaxy-gm film` and `galaxy-gm motion` on the drift test module, headless: films with
//! probes only need no GPU. Films with pictures are behind `#[ignore]`.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_module::module::Module;
use galaxy_sim::scenario::InputsFile;
use galaxy_sim::{BuildStamp, Session};
use serde_json::Value;

fn play() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play")
}

fn gm(args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    let json = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), json)
}

/// Drift flying its route for 60 ticks, the wheel turned on tick 20 and a note on 40.
fn record(dir: &Path) -> PathBuf {
    let drift = Module::read(&play().join("drift")).unwrap();
    let mut session = Session::new(vec![drift], 1).unwrap();
    let script = InputsFile::read(&play().join("films/inputs/route.toml")).unwrap();
    let inputs = script.by_tick();
    session.start_recording(BuildStamp::current());
    for tick in 1..=60 {
        session
            .step(inputs.get(&tick).map_or(&[][..], Vec::as_slice))
            .unwrap();
        if tick == 20 {
            session.record_zoom(24.0);
            session.record_zoom(18.0);
        }
        if tick == 40 {
            session.note("look at the course", None, None);
        }
    }
    let path = dir.join("recording.json");
    session.recording().unwrap().write(&path).unwrap();
    path
}

#[test]
fn a_recording_is_filmed_round_its_note_with_the_zoom_the_player_had() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let (code, out) = gm(&[
        "film",
        rec.to_str().unwrap(),
        "--note",
        "1",
        "--before",
        "1",
        "--after",
        "0.5",
        "--probes-only",
    ]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["drawn"], false);
    // From a second before the note (tick 10) to half a second after (tick 55), at 60 fps.
    assert_eq!(out["from"]["tick"], 10);
    assert_eq!(out["to"]["tick"], 55);
    assert_eq!(
        out["notes_checked"],
        serde_json::json!([{ "tick": 40, "identical": true }])
    );
    let film = PathBuf::from(out["out"].as_str().unwrap());
    assert_eq!(film, tmp.path().join("films").join("note-0001"));
    // film.json is small: a header; the frames are a line each in frames.jsonl.
    let header: Value =
        serde_json::from_str(&std::fs::read_to_string(film.join("film.json")).unwrap()).unwrap();
    assert_eq!(header["frame_count"], 91);
    assert!(header.get("frames").is_none());
    let lines = std::fs::read_to_string(film.join("frames.jsonl")).unwrap();
    assert_eq!(lines.lines().count(), 91);
    let read = galaxy_motion::Film::read(&film).unwrap();
    let frames = read.frames();
    assert_eq!(frames.len(), 91);
    assert!(frames.iter().any(|f| !f.lines.is_empty()));
    let at = |tick: u64| frames.iter().find(|f| f.tick == tick).unwrap();
    // Two wheel moves on one tick keep where the wheel ended.
    assert_eq!(at(19).zoom, None);
    assert_eq!(at(20).zoom, Some(18.0));
    assert!(at(20).camera.per_pixel < at(19).camera.per_pixel);
    let (code, dots) = gm(&["motion", "dots", film.to_str().unwrap()]);
    assert_eq!(code, 0, "{dots}");
    assert!(
        dots["drift_px_per_frame"].as_f64().unwrap().abs() < 0.01,
        "{dots}"
    );
}

#[test]
fn a_film_of_a_different_world_is_refused() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let mut json: Value = serde_json::from_str(&std::fs::read_to_string(&rec).unwrap()).unwrap();
    let note = json["events"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|e| e["type"] == "note")
        .unwrap();
    note["hash"] = Value::from("not the world");
    std::fs::write(&rec, json.to_string()).unwrap();
    // At the default rate; at one frame a second, where no frame lands on the note's tick,
    // also with the film ending on the note; and filmed on two builds to compare them: each
    // is refused.
    let variants: [&[&str]; 4] = [
        &[],
        &["--fps", "1"],
        &["--fps", "1", "--after", "0"],
        &["--builds", "this,this"],
    ];
    for (k, extra) in variants.iter().enumerate() {
        let dir = tmp.path().join(format!("film-{k}"));
        let mut args = vec![
            "film",
            rec.to_str().unwrap(),
            "--note",
            "1",
            "--probes-only",
            "--out",
            dir.to_str().unwrap(),
        ];
        args.extend_from_slice(extra);
        let (code, out) = gm(&args);
        assert_eq!(
            (code, out["kind"].as_str()),
            (5, Some("replay_diverged")),
            "{extra:?}: {out}"
        );
        let checked = if extra.contains(&"--builds") {
            &out["films"][0]["notes_checked"]
        } else {
            &out["notes_checked"]
        };
        assert_eq!(checked[0]["identical"], false, "{extra:?}: {out}");
    }
}

#[test]
fn a_note_that_is_not_there_is_bad_input() {
    let tmp = tempfile::tempdir().unwrap();
    let rec = record(tmp.path());
    let (code, out) = gm(&[
        "film",
        rec.to_str().unwrap(),
        "--note",
        "2",
        "--probes-only",
    ]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
    let (code, out) = gm(&["film", rec.to_str().unwrap(), "--nope", "1"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
}

#[test]
fn the_dock_check_holds_without_a_gpu() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = play().join("films/dock.toml");
    let (code, out) = gm(&[
        "motion",
        "check",
        spec.to_str().unwrap(),
        "--out",
        tmp.path().join("dock").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    let lines = out["expectations"].as_array().unwrap();
    assert_eq!(lines.len(), 3);
    assert!(lines.iter().all(|l| l["pass"] == true), "{out}");
}

#[test]
fn a_check_that_does_not_hold_exits_5_and_says_which() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = tmp.path().join("strict.toml");
    let drift = play().join("drift");
    std::fs::write(
        &spec,
        format!(
            "modules = '{}'\ninputs = '{}'\nfrom = 0\nto = 60\nprobes_only = true\n\
             [[expect]]\ntool = 'track'\nto = 'berth'\nvalue = 'gap.end'\nat_most = 0.5\n\
             [[expect]]\ntool = 'track'\nvalue = 'camera.judder'\nat_most = 0.1\n",
            drift.display(),
            play().join("films/inputs/dock.toml").display()
        ),
    )
    .unwrap();
    let (code, out) = gm(&[
        "motion",
        "check",
        spec.to_str().unwrap(),
        "--out",
        tmp.path().join("f").to_str().unwrap(),
    ]);
    assert_eq!(
        (code, out["kind"].as_str()),
        (5, Some("motion_expectations")),
        "{out}"
    );
    let lines = out["expectations"].as_array().unwrap();
    // Two seconds in, the ship is still on its way to the berth.
    assert_eq!(lines[0]["pass"], false, "{out}");
    assert!(lines[0]["got"].as_f64().unwrap() > 0.5);
    assert_eq!(lines[1]["pass"], true, "{out}");
}

#[test]
fn an_unknown_tool_or_option_is_named() {
    let tmp = tempfile::tempdir().unwrap();
    let (code, out) = gm(&["motion", "nope", tmp.path().to_str().unwrap()]);
    assert_eq!(code, 2, "{out}");
    assert!(
        out["error"]
            .as_str()
            .unwrap()
            .contains("no command `motion nope`"),
        "{out}"
    );
    assert!(out["usage"].as_str().unwrap().contains("motion flicker"));
    let (code, out) = gm(&["motion", "track", tmp.path().to_str().unwrap()]);
    assert_eq!(code, 4, "{out}");
    assert!(
        out["error"].as_str().unwrap().contains("not a film"),
        "{out}"
    );
    let (code, out) = gm(&["motion"]);
    assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
}

#[test]
#[ignore = "needs a GPU: films the course and holds strip, flow, dots and track to it"]
fn the_course_check_holds() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = play().join("films/course.toml");
    let (code, out) = gm(&[
        "motion",
        "check",
        spec.to_str().unwrap(),
        "--out",
        tmp.path().join("course").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["expectations"].as_array().unwrap().len(), 4);
}

/// A copy of drift with `[light]` added to its style, for a defect put back on purpose.
fn drift_with_light(dir: &Path, light: &str) -> PathBuf {
    let from = play().join("drift");
    let to = dir.join("drift");
    for entry in walk(&from) {
        let target = to.join(entry.strip_prefix(&from).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(&entry, &target).unwrap();
    }
    let style = to.join("style.toml");
    let text = std::fs::read_to_string(&style).unwrap();
    std::fs::write(&style, format!("{text}\n[light]\n{light}\n")).unwrap();
    to
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
#[ignore = "needs a GPU: films drift turning, with the default band fade and with a hard step"]
fn the_tone_check_holds_and_finds_a_hard_band_step() {
    let tmp = tempfile::tempdir().unwrap();
    let spec = play().join("films/tone.toml");
    let (code, out) = gm(&[
        "motion",
        "check",
        spec.to_str().unwrap(),
        "--out",
        tmp.path().join("smooth").to_str().unwrap(),
    ]);
    assert_eq!(code, 0, "{out}");
    // The same spec against a copy of drift whose faces step between bands.
    let hard = drift_with_light(tmp.path(), "band_fade_degrees = 0");
    let text = std::fs::read_to_string(&spec)
        .unwrap()
        .replace(
            "modules = \"../drift\"",
            &format!("modules = {:?}", hard.display().to_string()),
        )
        .replace(
            "inputs = \"inputs/circle.toml\"",
            &format!(
                "inputs = {:?}",
                play()
                    .join("films/inputs/circle.toml")
                    .display()
                    .to_string()
            ),
        );
    let hard_spec = tmp.path().join("tone-hard.toml");
    std::fs::write(&hard_spec, text).unwrap();
    let (code, out) = gm(&[
        "motion",
        "check",
        hard_spec.to_str().unwrap(),
        "--out",
        tmp.path().join("hard").to_str().unwrap(),
    ]);
    assert_eq!(code, 5, "{out}");
    let got = out["expectations"][0]["got"].as_u64().unwrap();
    assert!(got >= 3, "{out}");
}
