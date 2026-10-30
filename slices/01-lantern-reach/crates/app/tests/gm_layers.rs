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

// Re-check 3 of PR #31: the layer comes from the load path, never from the pack's ID
// or from a pack being new. A game-master folder whose manifest changes its ID on a
// reload stays a game-master layer, and no game-master path takes the core's place.

const RENAMED: PackId = PackId::from_static("review_renamed");
const CORE: PackId = PackId::from_static("core");

/// Rewrites the pack in `dir` to call itself `id`.
fn rename(dir: &Path, id: &str) {
    let path = dir.join("manifest.toml");
    let manifest = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        manifest.replace("id = \"probe\"", &format!("id = \"{id}\"")),
    )
    .unwrap();
}

fn codes(report: &galaxy_contract::pack::ValidationReport) -> Vec<String> {
    report.errors().map(|d| d.code.clone()).collect()
}

fn dock_and_run(game: &mut Game, ticks: usize) {
    game.sim_mut()
        .submit(
            CommandBatch::new(Cause::system("test", "dock")).with(Command::SetShipDocked {
                ship: ShipId::from_static("ship.player"),
                station: Some(StationId::from_static("station.kettle_dock")),
            }),
        );
    for _ in 0..ticks {
        game.step(&PlayerInput::default());
    }
}

/// The game as `play` opens a save without the game master's packs, with `probe`
/// loaded from `src` as a game-master layer and its layer committed.
fn game_with_probe(path: &Path, src: &Path) -> Game {
    let state = Save::open(path).unwrap().load().unwrap();
    let seed = state.world.meta.seed;
    let mut game = Game::open(state, &GameOptions::headless(packs(), seed)).unwrap();
    benign(src);
    let report = game
        .scripts()
        .load_layer(src, game.sim().world(), game.sim().history());
    assert!(report.accepted(), "{report:#?}");
    game.scripts().clone().install(game.sim_mut());
    for _ in 0..2 {
        game.step(&PlayerInput::default());
    }
    let world = game.sim().world();
    assert_eq!(
        world.packs.get(&PROBE).and_then(|p| p.layer),
        Some(PackLayer::GameMaster)
    );
    assert_eq!(
        world.packs.get(&CORE).and_then(|p| p.layer),
        Some(PackLayer::Core)
    );
    game
}

#[test]
fn a_reload_that_renames_a_layer_never_promotes_it() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_save(dir.path());
    let src = dir.path().join("probe");
    let mut game = game_with_probe(&path, &src);
    let host = game.scripts().clone();
    let core = host.revision(&CORE).unwrap();
    let recorded =
        |game: &Game, id: &PackId| game.sim().world().packs.get(id).and_then(|p| p.layer);

    // The re-check's probe: the same folder now calls itself `review_renamed` and
    // registers a dock-completed beat paying 123 cr. The reload checks it as the
    // game-master layer the folder was loaded as, and refuses it.
    forbidden(&src);
    rename(&src, RENAMED.as_str());
    let report = host
        .reload(&PROBE, game.sim_mut())
        .expect("the probe is loaded");
    assert!(
        codes(&report).contains(&"gm_backbone".to_owned()),
        "{report:#?}"
    );
    assert_eq!(host.layer(&RENAMED), None);
    assert_eq!(host.layer(&PROBE), Some(PackLayer::GameMaster));
    let before = credits(&game);
    dock_and_run(&mut game, 4);
    assert_eq!(credits(&game), before, "no {REWARD} reward");
    no_probe_beat(&game);
    assert_eq!(recorded(&game, &RENAMED), None, "never recorded as core");

    // Calling itself `core`: refused, and the backbone's VM is untouched.
    for revision in [forbidden as fn(&Path), benign] {
        revision(&src);
        rename(&src, "core");
        let report = host
            .reload(&PROBE, game.sim_mut())
            .expect("the probe is loaded");
        let codes = codes(&report);
        assert!(codes.contains(&"gm_core_id".to_owned()), "{codes:?}");
        assert!(codes.contains(&"reload_id_taken".to_owned()), "{codes:?}");
        assert_eq!(host.revision(&CORE), Some(core.clone()));
        assert_eq!(host.layer(&CORE), Some(PackLayer::Core));
        assert_eq!(host.layer(&PROBE), Some(PackLayer::GameMaster));
    }

    // A benign revision under a new ID is a new game-master layer: it replaces the old
    // entry, and the world records it as a layer.
    benign(&src);
    rename(&src, RENAMED.as_str());
    let report = host
        .reload(&PROBE, game.sim_mut())
        .expect("the probe is loaded");
    assert!(report.accepted(), "{report:#?}");
    assert_eq!(host.layer(&RENAMED), Some(PackLayer::GameMaster));
    assert_eq!(host.layer(&PROBE), None);
    for _ in 0..2 {
        game.step(&PlayerInput::default());
    }
    assert_eq!(recorded(&game, &RENAMED), Some(PackLayer::GameMaster));

    // Its forbidden revision is refused on the next reload, and past every check the
    // host still runs it as a layer: the beat never starts and nothing is paid.
    forbidden(&src);
    rename(&src, RENAMED.as_str());
    rejected_as_backbone(&host.reload(&RENAMED, game.sim_mut()).unwrap());
    host.load_unchecked(&src).unwrap();
    assert_eq!(host.layer(&RENAMED), Some(PackLayer::GameMaster));
    let before = credits(&game);
    dock_and_run(&mut game, 4);
    assert_eq!(credits(&game), before, "no {REWARD} reward");
    no_probe_beat(&game);
    assert_eq!(host.revision(&CORE), Some(core));
}

#[test]
fn a_layer_named_core_never_touches_the_backbone() {
    let dir = tempfile::tempdir().unwrap();
    let path = new_save(dir.path());
    let fake = dir.path().join("fake_core");
    forbidden(&fake);
    rename(&fake, "core");

    // `gm-examples` and the bridge opening a save (`play`, `sim run`) load through
    // `load_layer`.
    let (mut game, mut bridge) = open_game(&path);
    let core = game.scripts().revision(&CORE).unwrap();
    let report = game
        .scripts()
        .load_layer(&fake, game.sim().world(), game.sim().history());
    assert!(
        codes(&report).contains(&"gm_core_id".to_owned()),
        "{report:#?}"
    );
    assert_eq!(game.scripts().revision(&CORE), Some(core.clone()));
    assert_eq!(game.scripts().layer(&CORE), Some(PackLayer::Core));

    // The bridge applying a staged revision called `core`: quarantined, core untouched.
    let mut save = Save::open(&path).unwrap();
    let pack = galaxy_script::read_pack(&fake).unwrap();
    let rel = bridge.store().import(&pack).unwrap();
    let report =
        galaxy_script::ScriptHost::new(None).load(&fake, game.sim().world(), game.sim().history());
    bridge.stage(&rel, report, "test", true).unwrap();
    let out = bridge.apply_staged(game.sim_mut(), &mut save);
    assert_eq!(out[0]["what"], "quarantined", "{out:?}");
    assert_eq!(game.scripts().revision(&CORE), Some(core.clone()));
    assert_eq!(game.scripts().layer(&CORE), Some(PackLayer::Core));
    let before = credits(&game);
    dock_and_run(&mut game, 4);
    assert_eq!(credits(&game), before, "no {REWARD} reward");
    no_probe_beat(&game);
    drop((game, bridge));

    // The sandbox child (`gm propose`, the inbox) refuses it on the fork, too.
    let fork = dir.path().join("fork.sqlite");
    galaxy_core::fork(&path, &fork).unwrap();
    let args = galaxy_gm::sandbox::ChildArgs {
        save: fork,
        packs: vec![fake.clone()],
        dockings: 1,
        ticks_per_docking: 200,
    };
    let run = galaxy_gm::sandbox::run_child(&args, |state, hook| {
        let seed = state.world.meta.seed;
        Game::open_with(state, &GameOptions::headless(packs(), seed), hook)
    })
    .unwrap();
    assert!(
        run.load_failures.iter().any(|d| d.code == "gm_core_id"),
        "{:?}",
        run.load_failures
    );
    assert!(!run.packs.contains(&CORE));

    // `gm propose`'s fast checks refuse it before any sandbox runs.
    let out = Process::new(env!("CARGO_BIN_EXE_galaxy"))
        .args([
            "gm",
            "propose",
            fake.to_str().unwrap(),
            "--save",
            path.to_str().unwrap(),
        ])
        .env("GALAXY_PACKS", packs())
        .output()
        .expect("galaxy runs");
    assert!(!out.status.success(), "{out:?}");
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("gm_core_id"),
        "{out:?}"
    );
}
