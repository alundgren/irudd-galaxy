use galaxy_module::module::Module;
use galaxy_render::glam::Vec2;
use galaxy_sim::{BuildStamp, Session, scenario::Pointer};
use galaxy_view::View;
use serde_json::Value;
use std::{path::Path, process::Command};

fn run(path: &Path, args: &[&str]) -> (i32, Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_engine-galaxy-gm"))
        .arg("look")
        .arg(path)
        .args(args)
        .env_remove("GALAXY_INSTALL_ROOT")
        .output()
        .unwrap();
    (
        out.status.code().unwrap(),
        serde_json::from_slice(&out.stdout).unwrap(),
    )
}

#[test]
fn look_reconstructs_exact_note_pointer_and_mesh_part_headlessly() {
    let module =
        Module::read(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/play/life/demo"))
            .unwrap();
    let mut s = Session::new(vec![module], 1).unwrap();
    s.start_recording(BuildStamp::current());
    s.step(&[]).unwrap();
    let mut v = View::detached(s.modules(), 1);
    let size = Vec2::new(1280.0, 720.0);
    v.scene(&s.world, 1.0, size, 0.0);
    v.bot_pointer(
        &s.world,
        &Pointer {
            tick: 2,
            action: "move".into(),
            widget: "walker-help".into(),
            delta: 0.0,
        },
    )
    .unwrap();
    v.scene(&s.world, 1.0, size, 0.0);
    let moment = v.moment(1.0, 0.0, (1600, 900), 1.25).unwrap();
    // A pointer event arriving after the drawn frame must not change F8's moment.
    v.screens.pointer_move(Vec2::ZERO, 2);
    s.note(
        "Hover on a being",
        Some(serde_json::to_value(&moment).unwrap()),
        None,
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("recording.json");
    s.recording().unwrap().write(&path).unwrap();
    let (code, out) = run(&path, &["--note", "1"]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["being"], "life.walker");
    assert_eq!(out["part"], "face");
    assert_eq!(out["place"], "life.room");
    assert_eq!(out["hover"]["readable"], true);
    assert_eq!(out["size"], serde_json::json!([1280.0, 720.0]));
    assert_eq!(
        out["point"],
        serde_json::json!(moment.screens.pointer.unwrap())
    );
    let (code, out) = run(&path, &["--note", "1", "--point", "1260,700"]);
    assert_eq!(code, 0, "{out}");
    assert!(out["hover"].is_null());
    let (code, out) = run(
        &path,
        &["--tick", "1", "--point", "1260,700", "--size", "1280x720"],
    );
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["alpha"], 1.0);
    for args in [
        vec!["--note", "0"],
        vec!["--tick", "99", "--point", "0,0"],
        vec!["--note", "1", "--tick", "1"],
        vec!["--note", "1", "--size", "800x600"],
        vec!["--note", "1", "--point", "NaN,0"],
    ] {
        let (code, out) = run(&path, &args);
        assert_ne!(code, 0, "{args:?}: {out}");
    }
}
