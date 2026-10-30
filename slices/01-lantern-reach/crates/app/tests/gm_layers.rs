//! Re-check 2 of PR #31, finding 2: a pack applied as a game-master layer keeps that
//! layer's restrictions after a save and a reload, whichever path loads it again. The
//! layer is recorded in the world's pack state (`PackState::layer`), so it doesn't
//! depend on the pack's ID or on the loader remembering it.

use std::path::{Path, PathBuf};
use std::process::Command as Process;

use galaxy_app::game::{Game, GameOptions};
use galaxy_contract::pack::PackLayer;
use galaxy_contract::prelude::*;
use galaxy_core::Save;
use galaxy_gm::{Bridge, BridgeOptions};
use galaxy_script::host::HostEvent;

/// Not `gm.`-prefixed: the layer, not the ID, carries the restriction.
const PROBE: PackId = PackId::from_static("probe");
const REWARD: i64 = 123 * CENTS_PER_CREDIT;

fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn packs() -> PathBuf {
    slice().join("packs")
}

fn new_save(dir: &Path) -> PathBuf {
    let save = dir.join("layers.sqlite");
    let out = Process::new(env!("CARGO_BIN_EXE_galaxy"))
        .args(["new", "--save", save.to_str().unwrap(), "--no-gm"])
        .env("GALAXY_PACKS", packs())
        .output()
        .expect("galaxy runs");
    assert!(out.status.success(), "{out:?}");
    save
}

const MANIFEST: &str = "id = \"probe\"\nname = \"Layer probe\"\nversion = \"0.1.0\"\n";

/// The benign revision the game master applies: a dock handler that does nothing.
fn benign(dir: &Path) {
    write(
        dir,
        &format!(
            "{MANIFEST}uses = [\"events.dock\"]\n\
             [[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\nhandlers = [\"on.dock\"]\n"
        ),
        "--!strict\nlocal G = require(\"@galaxy\")\n\
         return G.script({ id = \"main\", on = { dock = function(_ev: G.DockEvent, _ctx: G.Ctx) end } })\n",
    );
}

/// The forbidden revision: the same pack registers a beat that completes on docking
/// and pays 123 credits (the re-check's probe).
fn forbidden(dir: &Path) {
    write(
        dir,
        &format!(
            "{MANIFEST}uses = [\"events.dock\", \"beats.write\"]\n\
             [[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\nhandlers = [\"on.dock\"]\n"
        ),
        "--!strict\nlocal G = require(\"@galaxy\")\n\
         return G.script({ id = \"main\", on = { dock = function(_ev: G.DockEvent, _ctx: G.Ctx) end },\n\
         \tbeats = { G.beat({ id = \"beat.probe_reward\", act = 1, goal = \"Dock anywhere.\",\n\
         \t\tdone = G.docked(), rewards = { credits = 12300 } }) } })\n",
    );
}

fn write(dir: &Path, manifest: &str, script: &str) {
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    std::fs::write(dir.join("scripts/main.luau"), script).unwrap();
}

/// Opens the save the way `play` and `sim run` do: the game's one composition, with
/// the bridge loading the applied packs into its script host.
fn open_game(path: &Path) -> (Game, Bridge) {
    let state = Save::open(path).unwrap().load().unwrap();
    let seed = state.world.meta.seed;
    let (game, bridge) = Game::open_gm(
        state,
        &GameOptions::headless(packs(), seed),
        path,
        BridgeOptions::default(),
    )
    .unwrap();
    (game, bridge.unwrap())
}

fn rejected_as_backbone(report: &galaxy_contract::pack::ValidationReport) {
    assert!(!report.accepted(), "{report:#?}");
    let codes: Vec<_> = report.errors().map(|d| d.code.as_str()).collect();
    assert!(codes.contains(&"gm_backbone"), "{codes:?}");
}

fn credits(game: &Game) -> i64 {
    game.sim().world().player.credits
}

fn no_probe_beat(game: &Game) {
    assert!(
        !game
            .sim()
            .world()
            .progress
            .beats
            .contains_key("beat.probe_reward"),
        "the layer's beat ran"
    );
}

#[test]
fn an_applied_layer_keeps_its_restrictions_through_every_reload() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_save(dir.path());
    let src = dir.path().join("benign");
    benign(&src);
    let bad = dir.path().join("forbidden");
    forbidden(&bad);

    // The game master applies the benign revision; the apply records its layer.
    let mut save = Save::open(&path).unwrap();
    let (mut game, mut bridge) = open_game(&path);
    let pack = galaxy_script::read_pack(&src).unwrap();
    let rel = bridge.store().import(&pack).unwrap();
    let report =
        galaxy_script::ScriptHost::new(None).load(&src, game.sim().world(), game.sim().history());
    assert!(report.accepted(), "{report:#?}");
    bridge.stage(&rel, report, "test", true).unwrap();
    let applied = bridge.apply_staged(game.sim_mut(), &mut save);
    assert_eq!(applied[0]["what"], "applied", "{applied:?}");
    for _ in 0..2 {
        game.step(&PlayerInput::default());
    }
    bridge.after_tick(game.sim());
    let layer = |game: &Game| game.sim().world().packs.get(&PROBE).and_then(|p| p.layer);
    assert_eq!(layer(&game), Some(PackLayer::GameMaster));
    save.write(game.sim().state()).unwrap();
    let applied_dir = bridge.store().abs(&bridge.state().applied[0].dir.clone());
    drop((game, bridge));

    // 1. `play` / `sim run`: the bridge loads the applied revision as a layer, and the
    //    world still says what it is.
    let (mut game, bridge) = open_game(&path);
    assert_eq!(layer(&game), Some(PackLayer::GameMaster));
    assert_eq!(game.scripts().layer(&PROBE), Some(PackLayer::GameMaster));
    let before = credits(&game);

    // 2. A plain load of the forbidden revision (what a dev reload of a changed folder
    //    does) is checked as a layer and refused.
    rejected_as_backbone(
        &game
            .scripts()
            .load(&bad, game.sim().world(), game.sim().history()),
    );

    // 3. `reload` and the dev watcher: the same folder, changed on disk, re-read.
    forbidden(&applied_dir);
    let report = game
        .scripts()
        .clone()
        .reload(&PROBE, game.sim_mut())
        .expect("the probe is loaded");
    rejected_as_backbone(&report);
    for _ in 0..3 {
        game.step(&PlayerInput::default());
    }
    assert_eq!(credits(&game), before, "no reward");
    no_probe_beat(&game);
    drop((game, bridge));

    // 4. A host that never loaded it as a layer (no bridge, the game opened without the
    //    game master's packs): the world's record alone makes the check a layer's.
    let state = Save::open(&path).unwrap().load().unwrap();
    let seed = state.world.meta.seed;
    let mut game = Game::open(state, &GameOptions::headless(packs(), seed)).unwrap();
    assert_eq!(game.scripts().layer(&PROBE), None);
    rejected_as_backbone(
        &game
            .scripts()
            .load(&bad, game.sim().world(), game.sim().history()),
    );

    // 5. Run anyway, past every check: the host runs it as the layer the world records.
    //    The beat never starts and nothing is paid.
    game.scripts().load_unchecked(&bad).unwrap();
    let before = credits(&game);
    game.sim_mut()
        .submit(
            CommandBatch::new(Cause::system("test", "dock")).with(Command::SetShipDocked {
                ship: ShipId::from_static("ship.player"),
                station: Some(StationId::from_static("station.kettle_dock")),
            }),
        );
    for _ in 0..4 {
        game.step(&PlayerInput::default());
    }
    assert_eq!(credits(&game), before, "no {REWARD} reward");
    no_probe_beat(&game);
    let aborts: Vec<String> = game
        .scripts()
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic.message),
            _ => None,
        })
        .collect();
    assert!(
        aborts
            .iter()
            .any(|m| m.contains("game-master layer never drives journey beats")),
        "{aborts:?}"
    );
    drop(game);

    // 6. The sandbox child on a fork of the save: the candidate loads as a layer and
    //    is refused there too.
    let fork = dir.path().join("fork.sqlite");
    galaxy_core::fork(&path, &fork).unwrap();
    let args = galaxy_gm::sandbox::ChildArgs {
        save: fork,
        packs: vec![bad.clone()],
        dockings: 1,
        ticks_per_docking: 200,
    };
    let run = galaxy_gm::sandbox::run_child(&args, |state, hook| {
        let seed = state.world.meta.seed;
        Game::open_with(state, &GameOptions::headless(packs(), seed), hook)
    })
    .unwrap();
    assert!(
        run.load_failures.iter().any(|d| d.code == "gm_backbone"),
        "{:?}",
        run.load_failures
    );
    assert!(!run.packs.contains(&PROBE));
}
