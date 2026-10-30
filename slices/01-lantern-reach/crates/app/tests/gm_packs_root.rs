//! Re-check of PR #25: every `galaxy sim sandbox` child gets the packs folder its
//! parent resolved (`--packs`), so a packs folder outside the default places works for
//! `gm propose` on a save and live, and for the inbox. Its own test binary: it points
//! `GALAXY_PACKS` at nowhere for the whole process, so any child that looks for the
//! packs itself fails.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use galaxy_app::game::{Game, GameOptions};
use galaxy_contract::prelude::*;
use galaxy_core::Save;
use galaxy_gm::{Bridge, BridgeOptions};
use serde_json::Value;

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_galaxy"))
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// Runs the CLI with no usable default packs folder.
fn cli(nowhere: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(exe());
    cmd.args(args).env("GALAXY_PACKS", nowhere);
    cmd
}

fn json(out: &str) -> Value {
    serde_json::from_str(out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"))
}

fn run(nowhere: &Path, args: &[&str]) -> (i32, Value) {
    let out = cli(nowhere, args).output().unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    (out.status.code().unwrap_or(-1), json(&stdout))
}

fn step<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == name)
        .unwrap_or_else(|| panic!("no {name} step in {report}"))
}

#[test]
#[allow(unsafe_code)]
fn sandbox_children_get_the_packs_folder_from_their_parent() {
    let dir = tempfile::tempdir().unwrap();
    // The packs live somewhere no default lookup finds them.
    let root = dir.path().join("content");
    let slice_packs = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs");
    copy_dir(&slice_packs.join("core"), &root.join("core"));
    copy_dir(&slice_packs.join("gm-examples"), &root.join("gm-examples"));
    let nowhere = dir.path().join("nowhere");
    // SAFETY: the only test in this binary, set before any thread or child starts.
    unsafe { std::env::set_var("GALAXY_PACKS", &nowhere) };
    let kettle = root.join("gm-examples").join("kettle_growth");
    let (r, k) = (root.to_str().unwrap(), kettle.to_str().unwrap());

    // `gm propose` on a save, with `--packs`: the children get it.
    let save = dir.path().join("p.sqlite");
    let s = save.to_str().unwrap();
    let (code, out) = run(&nowhere, &["new", "--packs", r, "--save", s]);
    assert_eq!(code, 0, "{out}");
    let (code, report) = run(
        &nowhere,
        &[
            "gm",
            "propose",
            k,
            "--save",
            s,
            "--packs",
            r,
            "--no-typecheck",
        ],
    );
    assert_eq!(code, 0, "{report}");
    assert_eq!(step(&report, "sandbox")["outcome"], "passed", "{report}");
    // Without it the parent finds no packs folder either, and neither do its children.
    let (code, report) = run(
        &nowhere,
        &["gm", "propose", k, "--save", s, "--no-typecheck"],
    );
    assert_eq!(code, 5, "{report}");
    assert_eq!(step(&report, "sandbox")["outcome"], "failed", "{report}");
    // `gm rollback` on a save opens it with `--packs` too.
    let (code, run_out) = run(
        &nowhere,
        &["sim", "run", "--save", s, "--ticks", "2", "--packs", r],
    );
    assert_eq!(code, 0, "{run_out}");
    let (code, out) = run(
        &nowhere,
        &["gm", "rollback", "stage.1", "--save", s, "--packs", r],
    );
    assert_eq!(code, 0, "{out}");

    // A running game over that folder: the live CLI's children and the inbox's use the
    // game's own packs folder.
    let path = dir.path().join("live.sqlite");
    let p = path.to_str().unwrap();
    let (code, out) = run(&nowhere, &["new", "--packs", r, "--save", p, "--no-gm"]);
    assert_eq!(code, 0, "{out}");
    let mut save = Save::open(&path).unwrap();
    let state = save.load().unwrap();
    let seed = state.world.meta.seed;
    let opts = BridgeOptions {
        exe: Some(exe()),
        packs: Some(root.clone()),
        ..BridgeOptions::default()
    };
    let (mut game, bridge) = Game::open_gm(
        state,
        &GameOptions::headless(root.clone(), seed),
        &path,
        opts,
    )
    .unwrap();
    let mut bridge = bridge.unwrap();
    bridge.start_live(None).unwrap();
    assert!(galaxy_gm::bridge::is_calm(game.sim()));

    let mut child = cli(
        &nowhere,
        &["gm", "propose", k, "--live", p, "--no-typecheck"],
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::null())
    .spawn()
    .unwrap();
    let mut stdout = child.stdout.take().unwrap();
    let reader = std::thread::spawn(move || {
        let mut s = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut s).unwrap();
        s
    });
    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(started.elapsed() < Duration::from_secs(180), "the CLI hung");
        serve(&mut bridge, &mut game, &mut save);
    };
    serve(&mut bridge, &mut game, &mut save);
    let report = json(&reader.join().unwrap());
    assert_eq!(status.code(), Some(0), "{report}");
    assert_eq!(step(&report, "sandbox")["outcome"], "passed", "{report}");

    // The inbox validates on the game's own thread, with the same folder.
    let inbox = bridge.store().inbox();
    copy_dir(&kettle, &inbox.join("kettle"));
    std::fs::write(inbox.join("kettle").join(galaxy_gm::store::READY), b"").unwrap();
    game.sim_mut().set_paused(true);
    let result = inbox.join("kettle.result.json");
    let started = Instant::now();
    while !result.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "the inbox hung"
        );
        bridge.serve(game.sim_mut(), &mut save);
        std::thread::sleep(Duration::from_millis(5));
    }
    let result = json(&std::fs::read_to_string(result).unwrap());
    assert_eq!(result["report"]["status"]["status"], "accepted", "{result}");
    assert_eq!(
        step(&result["report"], "sandbox")["outcome"],
        "passed",
        "{result}"
    );
}

fn serve(bridge: &mut Bridge, game: &mut Game, save: &mut Save) {
    bridge.serve(game.sim_mut(), save);
    game.step(&PlayerInput::default());
    std::thread::sleep(Duration::from_millis(2));
}
