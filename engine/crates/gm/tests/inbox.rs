//! Real CLI, headless sessions: receipts outlive processes, later notes and installs.

use std::path::{Path, PathBuf};
use std::process::Command;

use galaxy_sim::{BuildStamp, Session};
use serde_json::{Value, json};

fn record(dir: &Path, count: usize) -> PathBuf {
    std::fs::create_dir_all(dir).unwrap();
    let mut session = Session::new(Vec::new(), 7).unwrap();
    session.start_recording(BuildStamp::current());
    for n in 1..=count {
        session.step(&[]).unwrap();
        session.note(
            &format!("Check the panel spacing {n}"),
            None,
            Some(json!({"file": format!("notes/{n:04}.png")})),
        );
    }
    let path = dir.join("recording.json");
    session.recording().unwrap().write(&path).unwrap();
    path
}

fn gm(root: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(args)
        .env("GALAXY_SESSIONS", root)
        .env("GALAXY_INSTALL_ROOT", root.join("another-engine-install"))
        .output()
        .unwrap();
    let value = serde_json::from_slice(&out.stdout)
        .unwrap_or_else(|e| panic!("one JSON value required: {e}: {:?}", out.stdout));
    (out.status.code().unwrap(), value)
}

fn ok(root: &Path, args: &[&str]) -> Value {
    let (code, out) = gm(root, args);
    assert_eq!(code, 0, "{args:?}: {out}");
    out
}

fn count(out: &Value) -> usize {
    out["sessions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["notes"].as_array().unwrap().len())
        .sum()
}

#[test]
fn inbox_handles_reopens_and_keeps_receipts_with_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    record(&root.join("2026-11-01-120000"), 1);
    let newer = root.join("2026-11-02-120000");
    let rec = record(&newer, 2);
    let original = std::fs::read(&rec).unwrap();
    let before = ok(root, &["notes", "inbox"]);
    assert_eq!(count(&before), 3);
    let note = &before["sessions"][0]["notes"][0];
    assert_eq!(
        before["sessions"][0]["session"],
        newer.display().to_string()
    );
    assert_eq!(note["note"], 2);
    assert_eq!(note["tick"], 2);
    assert_eq!(note["t"], 0.07);
    assert_eq!(note["text"], "Check the panel spacing 2");
    assert_eq!(
        note["seen"],
        newer.join("notes").join("0002.png").display().to_string()
    );

    let receipt = ok(
        root,
        &[
            "notes",
            "handle",
            newer.to_str().unwrap(),
            "--note",
            "2",
            "--reason",
            "Increased the panel spacing",
            "--change",
            "demo@0.2.0",
        ],
    );
    assert_eq!(receipt["handled"], true);
    assert!(Path::new(receipt["mark"].as_str().unwrap()).is_file());
    assert_eq!(std::fs::read(&rec).unwrap(), original);
    assert_eq!(count(&ok(root, &["notes", "inbox"])), 2);
    let answered = ok(root, &["notes", "inbox", "--handled"]);
    assert_eq!(count(&answered), 1);
    assert_eq!(
        answered["sessions"][0]["notes"][0]["handled"],
        json!({"reason": "Increased the panel spacing", "change": "demo@0.2.0"})
    );
    assert_eq!(count(&ok(root, &["notes"])), 3);

    // A running session adds notes and rewrites its recording; receipts still match.
    record(&newer, 3);
    record(&root.join("2026-11-03-120000"), 1);
    assert_eq!(count(&ok(root, &["notes", "inbox"])), 4);
    assert_eq!(count(&ok(root, &["notes", "inbox", "--handled"])), 1);

    // Moving the whole session carries receipts; no absolute path is their identity.
    let moved = root.join("2026-11-02-120000-moved");
    std::fs::rename(&newer, &moved).unwrap();
    let moved_rec = moved.join("recording.json");
    ok(
        root,
        &[
            "notes",
            "handle",
            moved_rec.to_str().unwrap(),
            "--note=2",
            "--reason=Adjusted spacing again",
            "--change=abc123",
        ],
    );
    let answered = ok(
        root,
        &["notes", "inbox", moved.to_str().unwrap(), "--handled"],
    );
    assert_eq!(
        answered["sessions"][0]["notes"][0]["handled"]["change"],
        "abc123"
    );
    for _ in 0..2 {
        ok(
            root,
            &["notes", "unhandle", moved.to_str().unwrap(), "--note", "2"],
        );
    }
    assert_eq!(count(&ok(root, &["notes", "inbox"])), 5);
    assert_eq!(count(&ok(root, &["notes", "inbox", "--handled"])), 0);
}

#[test]
fn inbox_filters_before_capping_and_reports_unreadable_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let rec = record(&root.join("2026-11-02-120000"), 25);
    for note in 21..=25 {
        ok(
            root,
            &[
                "notes",
                "handle",
                rec.to_str().unwrap(),
                "--note",
                &note.to_string(),
                "--reason",
                "Spacing verified",
            ],
        );
    }
    let bad = root.join("2026-11-03-120000");
    std::fs::create_dir_all(&bad).unwrap();
    std::fs::write(bad.join("recording.json"), "unfinished recording").unwrap();
    let out = ok(root, &["notes", "inbox"]);
    assert_eq!(count(&out), 20);
    assert!(out.get("more").is_none());
    assert_eq!(out["sessions"][0]["notes"][0]["note"], 20);
    assert_eq!(out["unreadable"].as_array().unwrap().len(), 1);
    ok(
        root,
        &["notes", "unhandle", rec.to_str().unwrap(), "--note", "25"],
    );
    let out = ok(root, &["notes", "inbox"]);
    assert_eq!(count(&out), 20);
    assert!(out["more"].as_str().unwrap().starts_with("1 more"));
    assert_eq!(count(&ok(root, &["notes", "inbox", "--all"])), 21);
}

#[test]
fn invalid_requests_and_marks_use_shared_failure_codes() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let rec = record(&root.join("session"), 1);
    let path = rec.to_str().unwrap();
    for tail in [
        vec!["--note", "1"],
        vec!["--note", "1", "--reason"],
        vec!["--note", "1", "--reason", " "],
        vec!["--note", "0", "--reason", "Changed"],
        vec!["--note", "1", "--reason", "Changed", "--change"],
        vec![
            "--note",
            "1",
            "--reason",
            "Changed",
            "--image",
            "ignored.png",
        ],
    ] {
        let mut args = vec!["notes", "handle", path];
        args.extend(tail);
        let (code, out) = gm(root, &args);
        assert_eq!((code, out["kind"].as_str()), (2, Some("usage")), "{out}");
    }
    let (code, out) = gm(
        root,
        &[
            "notes", "handle", path, "--note", "2", "--reason", "Changed",
        ],
    );
    assert_eq!((code, out["kind"].as_str()), (4, Some("bad_input")));
    let receipt = ok(
        root,
        &[
            "notes", "handle", path, "--note", "1", "--reason", "Changed",
        ],
    );
    std::fs::write(receipt["mark"].as_str().unwrap(), "invalid JSON").unwrap();
    let (code, out) = gm(root, &["notes", "inbox"]);
    assert_eq!((code, out["kind"].as_str()), (4, Some("bad_input")));
    // Unhandle can recover a damaged receipt without editing the recording.
    ok(root, &["notes", "unhandle", path, "--note", "1"]);
    assert_eq!(count(&ok(root, &["notes", "inbox"])), 1);
}

#[test]
fn a_fresh_install_is_empty_and_a_replaced_note_is_unhandled() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().join("not-played-yet");
    let out = ok(&root, &["notes", "inbox"]);
    assert_eq!(count(&out), 0);
    assert_eq!(out["view"], "unhandled");
    assert!(!root.exists());
    let rec = record(&root.join("session"), 1);
    ok(
        &root,
        &[
            "notes",
            "handle",
            rec.to_str().unwrap(),
            "--note",
            "1",
            "--reason",
            "Changed",
        ],
    );
    let mut recording = galaxy_sim::Recording::read(&rec).unwrap();
    if let galaxy_sim::recording::RecEvent::Note { text, .. } = &mut recording.events[0] {
        *text = "A different concern".into();
    }
    recording.write(&rec).unwrap();
    assert_eq!(count(&ok(&root, &["notes", "inbox"])), 1);
}

/// A module folder `dir/<id>` with only a manifest.
fn module(dir: &Path, id: &str) -> PathBuf {
    let root = dir.join(id);
    std::fs::create_dir_all(&root).unwrap();
    let manifest = format!(
        "id = \"{id}\"\nversion = \"0.1.0\"\nmin_engine_version = \"{}\"\n",
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(root.join("manifest.toml"), manifest).unwrap();
    root
}

#[test]
fn inbox_for_one_game_leaves_out_sessions_of_other_modules() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let games = root.join("games");
    let garden = module(&games.join("garden"), "garden");
    let other = module(&games.join("other"), "other");
    for (name, dir) in [
        ("2026-11-01-120000", &garden),
        ("2026-11-02-120000", &other),
    ] {
        let session = root.join(name);
        std::fs::create_dir_all(&session).unwrap();
        let m = galaxy_module::module::Module::read(dir).unwrap();
        let mut s = Session::new(vec![m], 7).unwrap();
        s.start_recording(BuildStamp::current());
        s.step(&[]).unwrap();
        s.note("Make it bigger", None, None);
        s.recording()
            .unwrap()
            .write(&session.join("recording.json"))
            .unwrap();
    }
    assert_eq!(count(&ok(root, &["notes", "inbox"])), 2);
    let one = ok(
        root,
        &["notes", "inbox", "--modules", garden.to_str().unwrap()],
    );
    assert_eq!(count(&one), 1);
    assert_eq!(one["sessions"][0]["modules"], json!(["garden"]));
    assert_eq!(one["sessions_of_other_modules"], 1);
    let (code, out) = gm(
        root,
        &[
            "notes",
            "inbox",
            "--modules",
            root.join("none").to_str().unwrap(),
        ],
    );
    assert_eq!(
        (code, out["kind"].as_str()),
        (4, Some("bad_input")),
        "{out}"
    );
}

fn memory(root: &Path, game: &Path, key: &str) -> (i32, Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .args(["memory", game.to_str().unwrap()])
        .env("GALAXY_GM_MEMORY", root.join("memory"))
        .env("OPENROUTER_API_KEY", key)
        .output()
        .unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    let value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{e}: {text}"));
    (out.status.code().unwrap(), value, text)
}

#[test]
fn memory_is_made_once_kept_as_edited_and_never_prints_a_key() {
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path();
    let games = root.join("game");
    module(&games, "garden");
    module(&games, "paths");
    let key = "test-key-0123456789";
    let (code, first, _) = memory(root, &games, key);
    assert_eq!(code, 0, "{first}");
    assert_eq!(first["created"], true);
    assert_eq!(first["game"], json!(["garden", "paths"]));
    let path = PathBuf::from(first["path"].as_str().unwrap());
    assert_eq!(path, root.join("memory").join("garden+paths.md"));
    assert!(
        first["text"]
            .as_str()
            .unwrap()
            .contains("## The player's preferences")
    );

    // The GM and the player edit the file; later runs read it as it is.
    let edited = "# GM memory\n\n## The player's preferences\n- Calm colours.\n";
    std::fs::write(&path, edited).unwrap();
    let (code, again, _) = memory(root, &games, key);
    assert_eq!(code, 0);
    assert_eq!(
        (again["created"].clone(), again["text"].clone()),
        (json!(false), json!(edited))
    );

    // One module of the game is a different game, with its own memory.
    let (_, single, _) = memory(root, &games.join("garden"), key);
    assert!(single["path"].as_str().unwrap().ends_with("garden.md"));

    // A key in the memory is refused and never printed.
    std::fs::write(&path, format!("{edited}- uses {key}\n")).unwrap();
    let (code, refused, text) = memory(root, &games, key);
    assert_eq!(
        (code, refused["kind"].as_str()),
        (4, Some("bad_input")),
        "{refused}"
    );
    assert!(!text.contains(key));
}
