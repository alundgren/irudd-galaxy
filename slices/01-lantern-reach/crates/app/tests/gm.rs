//! The game-master bridge end to end with the real `galaxy` binary: propose on a save
//! (accepted and applied on load; rejected by the sandbox), the sandbox's child
//! processes, rollback through the CLI, the inbox, and the `--live` CLI against an
//! in-process game. Headless; the live screenshot test needs a GPU and is ignored.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim};
use galaxy_gm::{Bridge, BridgeOptions};
use serde_json::Value;

const ODO: NpcId = NpcId::from_static("npc.odo_fenn");

fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn packs() -> PathBuf {
    slice().join("packs")
}

fn example(name: &str) -> PathBuf {
    packs().join("gm-examples").join(name)
}

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_galaxy"))
}

fn galaxy(args: &[&str]) -> (i32, Value) {
    let out = Command::new(exe())
        .args(args)
        .env("GALAXY_PACKS", packs())
        .output()
        .expect("galaxy runs");
    let stdout = String::from_utf8(out.stdout).expect("utf-8");
    let json = serde_json::from_str(&stdout).unwrap_or_else(|e| {
        panic!(
            "stdout is not JSON ({e}): {stdout}\nstderr: {}",
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (out.status.code().unwrap_or(-1), json)
}

fn new_save(dir: &Path, name: &str) -> PathBuf {
    let save = dir.join(name);
    let (code, out) = galaxy(&["new", "--save", save.to_str().unwrap()]);
    assert_eq!(code, 0, "{out}");
    save
}

fn step<'a>(report: &'a Value, name: &str) -> &'a Value {
    report["steps"]
        .as_array()
        .unwrap()
        .iter()
        .find(|s| s["step"] == name)
        .unwrap_or_else(|| panic!("no {name} step in {report}"))
}

fn codes(step: &Value) -> Vec<String> {
    step["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["code"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn propose_on_a_save_then_roll_back() {
    let dir = tempfile::tempdir().unwrap();
    let save = new_save(dir.path(), "p.sqlite");
    let s = save.to_str().unwrap();
    let kettle = example("kettle_growth");

    // A good pack: every step passes, and it waits for the next load.
    let (code, report) = galaxy(&[
        "gm",
        "propose",
        kettle.to_str().unwrap(),
        "--save",
        s,
        "--no-typecheck",
    ]);
    assert_eq!(code, 0, "{report}");
    assert_eq!(report["status"]["status"], "accepted");
    for name in ["smoke", "sandbox", "legibility", "stage"] {
        assert_eq!(step(&report, name)["outcome"], "passed", "{name}: {report}");
    }
    assert_eq!(step(&report, "canon")["outcome"], "skipped");
    assert!(codes(step(&report, "legibility")).contains(&"lines_fired".to_owned()));
    let (_, status) = galaxy(&["gm", "status", "--save", s]);
    assert_eq!(status["staged"][0]["stage"], "stage.1");
    assert_eq!(status["applied"].as_array().unwrap().len(), 0);

    // The next load applies it: snapshot, change ID, the pack's person installed.
    let (code, run) = galaxy(&["sim", "run", "--save", s, "--ticks", "3"]);
    assert_eq!(code, 0, "{run}");
    assert_eq!(run["gm_applied"][0]["stage"], "stage.1");
    let (_, status) = galaxy(&["gm", "status", "--save", s]);
    let applied = &status["applied"][0];
    assert_eq!(applied["state"], "running", "{status}");
    assert_eq!(applied["snapshot"], 1);
    let change = applied["change"].as_str().unwrap().to_owned();
    assert!(change.starts_with("cause."));
    let (_, odo) = galaxy(&[
        "state",
        "get",
        "--save",
        s,
        "--path",
        "npcs.npc.odo_fenn.name",
    ]);
    assert_eq!(odo, "Odo Fenn");
    let (_, facts) = galaxy(&[
        "history",
        "query",
        "--save",
        s,
        "--kind",
        "gm_change_applied",
    ]);
    assert_eq!(facts["facts"][0]["cause"], change.as_str());

    // Bad packs pass the fast checks and are rejected by the sandbox, and the save is
    // never touched while they run.
    let before = std::fs::read(&save).unwrap();
    let silent = example("bad_sandbox/silent_owner");
    let (code, report) = galaxy(&[
        "gm",
        "propose",
        silent.to_str().unwrap(),
        "--save",
        s,
        "--no-typecheck",
    ]);
    assert_eq!(code, 5, "{report}");
    assert_eq!(report["status"]["status"], "rejected");
    assert_eq!(step(&report, "smoke")["outcome"], "passed");
    assert_eq!(step(&report, "legibility")["outcome"], "failed");
    assert!(codes(step(&report, "legibility")).contains(&"silent_change".to_owned()));
    assert_eq!(step(&report, "stage")["outcome"], "skipped");
    let spin = example("bad_sandbox/late_spin");
    let (code, report) = galaxy(&[
        "gm",
        "propose",
        spin.to_str().unwrap(),
        "--save",
        s,
        "--no-typecheck",
    ]);
    assert_eq!(code, 5, "{report}");
    assert_eq!(step(&report, "smoke")["outcome"], "passed");
    assert_eq!(step(&report, "sandbox")["outcome"], "failed");
    let sandbox = codes(step(&report, "sandbox"));
    assert!(sandbox.contains(&"over_budget".to_owned()), "{sandbox:?}");
    assert!(sandbox.contains(&"quarantined".to_owned()), "{sandbox:?}");
    assert_eq!(
        std::fs::read(&save).unwrap(),
        before,
        "the save is untouched"
    );
    let (_, status) = galaxy(&["gm", "status", "--save", s]);
    assert_eq!(status["staged"].as_array().unwrap().len(), 0);

    // Rollback through the CLI, by stage or by cause: in-world, with Odo's line.
    let (code, out) = galaxy(&["gm", "rollback", "stage.1", "--save", s]);
    assert_eq!(code, 0, "{out}");
    assert_eq!(out["causes"][0], change.as_str());
    assert_eq!(out["outcome"]["retired"][0][0], "gm.kettle_growth");
    assert!(
        out["outcome"]["line"]["text"]
            .as_str()
            .unwrap()
            .contains("re-seal")
    );
    let (_, status) = galaxy(&["gm", "status", "--save", s]);
    assert_eq!(status["applied"][0]["state"], "rolled_back");
    let (code, _) = galaxy(&["state", "get", "--save", s, "--path", "npcs.npc.odo_fenn"]);
    assert_eq!(code, 4, "the installed person went with the change");
    let (code, again) = galaxy(&["gm", "rollback", &change, "--save", s]);
    assert_eq!(code, 4, "{again}");
    let (code, nope) = galaxy(&["gm", "rollback", "cause.999", "--save", s]);
    assert_eq!(code, 4, "{nope}");
    // The digest names the change and the line.
    let (code, digest) = galaxy(&["gm", "digest", "--save", s]);
    assert_eq!(code, 0);
    assert!(
        digest["talk"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["text"].as_str().unwrap().contains("re-seal"))
    );
    assert!(
        digest["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["cause"] == change.as_str() && c["rolled_back"] == true)
    );
}

#[test]
fn the_sandbox_runs_in_child_processes_on_copies() {
    let dir = tempfile::tempdir().unwrap();
    let base = new_save(dir.path(), "base.sqlite");
    let before = std::fs::read(&base).unwrap();
    let spin = example("bad_sandbox/late_spin");
    let id = PackId::new("gm.bad_late_spin").unwrap();
    let rules = galaxy_gm::SandboxRules::default();
    let pair = galaxy_gm::sandbox::sandbox(&exe(), &base, &[], (&id, &spin), &rules).unwrap();
    let me = std::process::id();
    assert_eq!(pair.parent_pid, me);
    assert_ne!(pair.with.pid, me, "the runaway ran in another process");
    assert_ne!(pair.without.pid, me);
    assert_ne!(pair.with.pid, pair.without.pid);
    // Each run had its own fork, which is gone now; the base never changed.
    assert_ne!(pair.with.save, base);
    assert_ne!(pair.with.save, pair.without.save);
    assert!(!pair.with.save.exists(), "the forks are cleaned up");
    assert_eq!(std::fs::read(&base).unwrap(), before);
    // Measured in activity: both runs flew the same dockings.
    assert_eq!(pair.with.dockings, rules.dockings);
    assert_eq!(pair.without.dockings, rules.dockings);
    assert!(pair.with.trouble.iter().any(|t| t.code == "over_budget"));
    assert!(pair.with.trouble.iter().any(|t| t.code == "quarantined"));
    assert!(pair.without.trouble.is_empty());
    // Without the pack, the run is the one any other sandbox flies.
    let (_, other) = galaxy(&[
        "sim",
        "sandbox",
        "--save",
        base.to_str().unwrap(),
        "--dockings",
        "6",
    ]);
    assert_eq!(other["hash"], pair.without.hash.as_str());
}

/// Serves `bridge` (ticking `sim`) until `child` exits; returns its exit code and JSON.
fn serve_until_done(
    mut child: std::process::Child,
    bridge: &mut Bridge,
    sim: &mut Sim,
    save: &mut Save,
) -> (i32, String) {
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
        bridge.serve(sim, save);
        sim.step(&PlayerInput::default());
        std::thread::sleep(Duration::from_millis(2));
    };
    bridge.serve(sim, save);
    (status.code().unwrap_or(-1), reader.join().unwrap())
}

fn spawn_cli(args: &[&str]) -> std::process::Child {
    Command::new(exe())
        .args(args)
        .env("GALAXY_PACKS", packs())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn live_cli(args: &[&str], bridge: &mut Bridge, sim: &mut Sim, save: &mut Save) -> (i32, Value) {
    let (code, out) = serve_until_done(spawn_cli(args), bridge, sim, save);
    let json = serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
    (code, json)
}

#[test]
fn the_live_cli_against_a_running_game() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_save(dir.path(), "live.sqlite");
    let mut save = Save::open(&path).unwrap();
    let mut sim = galaxy_app::game::game(save.load().unwrap());
    let mut bridge = Bridge::open(&path, &mut sim, BridgeOptions::default()).unwrap();
    bridge.start_live(None).unwrap();
    let p = path.to_str().unwrap();
    // A new game starts docked at the Wharf: a calm moment.
    assert!(galaxy_gm::bridge::is_calm(&sim));

    let (code, credits) = live_cli(
        &["state", "get", "--live", p, "--path", "player.credits"],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0, "{credits}");
    assert_eq!(credits, sim.world().player.credits);
    let (code, q) = live_cli(
        &[
            "state",
            "query",
            "stations",
            "--live",
            p,
            "--where",
            "region=region.lantern_shallows",
            "--fields",
            "name",
        ],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0, "{q}");
    assert_eq!(q["count"], 2);

    let kettle = example("kettle_growth");
    let (code, report) = live_cli(
        &[
            "gm",
            "propose",
            kettle.to_str().unwrap(),
            "--live",
            p,
            "--wait",
            "--timeout",
            "60",
            "--no-typecheck",
        ],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0, "{report}");
    let change = report["status"]["change"]
        .as_str()
        .expect("applied with a change ID")
        .to_owned();
    assert_eq!(report["status"]["status"], "applied");
    assert!(sim.world().npcs.contains(&ODO));
    assert_eq!(
        bridge.state().applied[0].change.unwrap().to_string(),
        change
    );
    // The snapshot went through the save directly here (the game uses its saver).
    assert_eq!(bridge.state().applied[0].snapshot, Some(1));

    let (code, status) = live_cli(
        &["gm", "status", "--live", p],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0);
    assert_eq!(status["applied"][0]["state"], "running");
    assert_eq!(status["live"], true);
    let (code, facts) = live_cli(
        &[
            "history",
            "query",
            "--live",
            p,
            "--kind",
            "gm_change_applied",
        ],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0);
    assert_eq!(facts["facts"][0]["cause"], change.as_str());
    let (code, digest) = live_cli(
        &["gm", "digest", "--live", p, "--since", "fact.1"],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0, "{digest}");
    assert!(
        digest["changes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| c["cause"] == change.as_str())
    );

    // `watch` replays from a fact, then streams; --count stops it.
    let (code, lines) = serve_until_done(
        spawn_cli(&["watch", "--live", p, "--since", "fact.0", "--count", "2"]),
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0);
    let items: Vec<Value> = lines
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(items[0]["type"], "watching");
    assert_eq!(items[1]["type"], "fact");
    assert_eq!(items[1]["fact"]["id"], "fact.1");

    let (code, out) = live_cli(
        &["gm", "rollback", &change, "--live", p],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 0, "{out}");
    for _ in 0..3 {
        sim.step(&PlayerInput::default());
    }
    assert!(!sim.world().npcs.contains(&ODO));
    let (_, status) = live_cli(
        &["gm", "status", "--live", p],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(status["applied"][0]["state"], "rolled_back");

    // While the game runs, the save-file write path refuses.
    let (code, refused) = live_cli(
        &[
            "gm",
            "propose",
            kettle.to_str().unwrap(),
            "--save",
            p,
            "--no-typecheck",
        ],
        &mut bridge,
        &mut sim,
        &mut save,
    );
    assert_eq!(code, 4, "{refused}");
    // Without a game: a clear error.
    drop(bridge);
    let (code, gone) = galaxy(&["gm", "status", "--live", p]);
    assert_eq!(code, 4, "{gone}");
}

#[test]
fn the_inbox_validates_and_stages_at_calm_moments() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_save(dir.path(), "inbox.sqlite");
    let mut save = Save::open(&path).unwrap();
    let mut sim = galaxy_app::game::game(save.load().unwrap());
    let mut bridge = Bridge::open(
        &path,
        &mut sim,
        BridgeOptions {
            exe: Some(exe()),
            ..BridgeOptions::default()
        },
    )
    .unwrap();
    let inbox = bridge.store().inbox();
    let copy = |from: &Path, to: &Path| {
        let pack = galaxy_script::read_pack(from).unwrap();
        for (f, text) in &pack.files {
            let p = to.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
    };
    copy(&example("kettle_growth"), &inbox.join("a_kettle"));
    copy(
        &example("bad_sandbox/silent_owner"),
        &inbox.join("b_silent"),
    );
    assert_eq!(galaxy_gm::bridge::inbox_entries(&inbox).len(), 2);

    // Paused: calm. The game claims entries one at a time and validates them on a
    // thread; the frames go on meanwhile.
    sim.set_paused(true);
    let started = Instant::now();
    while !inbox.join("b_silent.result.json").exists() {
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "the inbox hung"
        );
        bridge.serve(&mut sim, &mut save);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        galaxy_gm::bridge::inbox_entries(&inbox).is_empty(),
        "both were claimed"
    );
    let good: Value =
        serde_json::from_str(&std::fs::read_to_string(inbox.join("a_kettle.result.json")).unwrap())
            .unwrap();
    assert_eq!(good["report"]["status"]["status"], "accepted", "{good}");
    assert_eq!(good["stage"], "stage.1");
    let bad: Value =
        serde_json::from_str(&std::fs::read_to_string(inbox.join("b_silent.result.json")).unwrap())
            .unwrap();
    assert_eq!(bad["report"]["status"]["status"], "rejected");
    // The good one was staged and, still calm, applied; the change ID follows the
    // first tick after the pause.
    bridge.serve(&mut sim, &mut save);
    assert_eq!(bridge.state().applied.len(), 1);
    assert_eq!(bridge.state().applied[0].source, "inbox:a_kettle");
    sim.set_paused(false);
    sim.step(&PlayerInput::default());
    bridge.after_tick(&sim);
    assert!(bridge.state().applied[0].change.is_some());
    assert!(sim.world().npcs.contains(&ODO));
}

/// Needs a GPU and a window: `cargo test -p galaxy-app --test gm -- --ignored`.
#[test]
#[ignore]
fn shot_live_shows_the_running_game() {
    let dir = tempfile::tempdir().unwrap();
    let saves = dir.path().join("saves");
    std::fs::create_dir_all(&saves).unwrap();
    let save = saves.join("shot.sqlite");
    struct Game(std::process::Child);
    impl Drop for Game {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let game = Command::new(exe())
        .args([
            "play",
            "--save",
            save.to_str().unwrap(),
            "--quit-after-frames",
            "100000",
        ])
        .env("GALAXY_PACKS", packs())
        .env("GALAXY_SAVES", &saves)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(Game)
        .unwrap();
    let live = saves.join("shot.gm/live.json");
    let started = Instant::now();
    while !live.exists() {
        assert!(
            started.elapsed() < Duration::from_secs(60),
            "the game didn't open"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    let png = dir.path().join("live.png");
    let out = Command::new(exe())
        .args(["shot", "--live", "--out", png.to_str().unwrap()])
        .env("GALAXY_SAVES", &saves)
        .output()
        .unwrap();
    drop(game);
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(out.status.success(), "{v}");
    assert!(v["width"].as_u64().unwrap() > 0);
    assert!(std::fs::metadata(&png).unwrap().len() > 1000);
}
