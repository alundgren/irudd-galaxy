//! Every route a pack writes through answers to the one capability table
//! (`galaxy_contract::needs`): the PR #31 re-check's three bypasses, each refused by
//! `galaxy pack check` and again by the host at runtime.

use crate::common;

use std::path::Path;

use common::*;
use galaxy_contract::needs;
use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_economy::levers::{Lever, lever_batch};
use galaxy_script::host::HostEvent;
use galaxy_script::{CheckOptions, ScriptHost, check_dir};

const WHARF_COOP: &str = "faction.wharf_cooperative";

fn write_pack(dir: &Path, manifest: &str, script: Option<&str>, npcs: Option<&str>) {
    std::fs::write(dir.join("manifest.toml"), manifest).unwrap();
    if let Some(src) = script {
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("scripts/main.luau"), src).unwrap();
    }
    if let Some(toml) = npcs {
        std::fs::create_dir_all(dir.join("data")).unwrap();
        std::fs::write(dir.join("data/npcs.toml"), toml).unwrap();
    }
}

fn quoted(uses: &[&str]) -> String {
    uses.iter()
        .map(|u| format!("\"{u}\""))
        .collect::<Vec<_>>()
        .join(", ")
}

fn check(dir: &Path, sim: &Sim, game_master: bool) -> galaxy_contract::pack::ValidationReport {
    check_dir(
        dir,
        CheckOptions {
            world: Some(sim.world()),
            history: Some(sim.history()),
            game_master,
            ..Default::default()
        },
    )
    .report
}

fn codes(r: &galaxy_contract::pack::ValidationReport) -> String {
    format!("{:?}", r.errors().collect::<Vec<_>>())
}

/// The host's aborts so far, as text.
fn aborts(host: &ScriptHost) -> String {
    host.take_events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                HostEvent::Aborted { .. } | HostEvent::InstallRefused { .. }
            )
        })
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The player docked at Kettle Dock, committed.
fn docked(sim: &mut Sim) {
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock")).with(Command::SetShipDocked {
            ship: PLAYER,
            station: Some(KETTLE),
        }),
    );
    step(sim, 1);
}

fn standing(sim: &Sim) -> i32 {
    sim.world().factions.get(WHARF_COOP).unwrap().standing
}

/// The review's probe: a pack whose ID isn't `gm.`-prefixed registers a beat that
/// rewards standing on docking, with only `uses`.
fn beat_probe(dir: &Path, uses: &[&str]) {
    write_pack(
        dir,
        &format!(
            "id = \"gm\"\nname = \"Probe\"\nversion = \"0.1.0\"\nuses = [{}]\n\
             [[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\nhandlers = []\n",
            quoted(uses)
        ),
        Some(&format!(
            "--!strict\nlocal G = require(\"@galaxy\")\n\
             return G.script({{ id = \"main\", beats = {{ G.beat({{\n\
             \tid = \"beat.gm_probe\", act = 1, goal = \"Dock anywhere.\",\n\
             \tdone = G.docked(),\n\
             \trewards = {{ standing = {{ [\"{WHARF_COOP}\"] = 80 }} }},\n\
             }}) }} }})\n"
        )),
        None,
    );
}

#[test]
fn a_beat_standing_reward_needs_a_standing_capability() {
    let dir = tempfile::tempdir().unwrap();
    beat_probe(dir.path(), &["beats.write"]);
    let sim = new_sim(1);

    // `galaxy pack check` derives what the rewards write and refuses it.
    let r = check(dir.path(), &sim, false);
    assert!(!r.accepted(), "{r:#?}");
    let why = codes(&r);
    assert!(
        why.contains("undeclared_capability")
            && why.contains("beat `beat.gm_probe`'s rewards")
            && why.contains("standing.write"),
        "{why}"
    );
    // As a game-master layer it may not register beats at all, whatever its ID.
    let r = check(dir.path(), &sim, true);
    assert!(codes(&r).contains("gm_backbone"), "{r:#?}");

    // The host refuses it at runtime too: the completion is a fault and nothing moves.
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir.path()).unwrap();
    host.attach(&mut sim);
    docked(&mut sim);
    let before = standing(&sim);
    step(&mut sim, 4);
    assert_eq!(
        standing(&sim),
        before,
        "standing moved without standing.write"
    );
    let why = aborts(&host);
    assert!(
        why.contains("undeclared_capability")
            && why.contains("`adjust_standing`")
            && why.contains("`standing.write` or `factions.write`"),
        "{why}"
    );

    // Declared, the same beat pays its deed.
    let dir = tempfile::tempdir().unwrap();
    beat_probe(dir.path(), &["beats.write", "standing.write"]);
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir.path()).unwrap();
    host.attach(&mut sim);
    docked(&mut sim);
    step(&mut sim, 4);
    assert!(aborts(&host).is_empty());
    assert_eq!(standing(&sim), before + 80);
}

fn npc_probe(dir: &Path, uses: &[&str]) {
    write_pack(
        dir,
        &format!(
            "id = \"gm.review_capabilities\"\nname = \"Probe\"\nversion = \"0.1.0\"\n\
             uses = [{}]\ndata = [\"data/npcs.toml\"]\n",
            quoted(uses)
        ),
        None,
        Some(
            "[[npc]]\nid = \"npc.gm.review_capabilities.npc\"\nname = \"Probe\"\n\
             role = \"drifter\"\nlocation = \"station.kettle_dock\"\n",
        ),
    );
}

#[test]
fn pack_data_needs_npcs_write_to_install_people() {
    let npc: NpcId = "npc.gm.review_capabilities.npc".parse().unwrap();
    let dir = tempfile::tempdir().unwrap();
    npc_probe(dir.path(), &[]);
    let sim = new_sim(1);
    let r = check(dir.path(), &sim, false);
    assert!(!r.accepted(), "{r:#?}");
    let why = codes(&r);
    assert!(
        why.contains("undeclared_capability")
            && why.contains("data/npcs.toml")
            && why.contains("`put_npc`")
            && why.contains("`npcs.write`"),
        "{why}"
    );

    // Loaded past the check, the host still installs nothing.
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir.path()).unwrap();
    host.attach(&mut sim);
    step(&mut sim, 2);
    assert!(
        !sim.world().npcs.contains(&npc),
        "installed without npcs.write"
    );
    let why = aborts(&host);
    assert!(
        why.contains("InstallRefused") && why.contains("`npcs.write`"),
        "{why}"
    );

    let dir = tempfile::tempdir().unwrap();
    npc_probe(dir.path(), &["npcs.write"]);
    let r = check(dir.path(), &sim, false);
    assert!(r.accepted(), "{r:#?}");
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    assert!(host.load(dir.path(), sim.world(), sim.history()).accepted());
    host.attach(&mut sim);
    step(&mut sim, 2);
    assert!(sim.world().npcs.contains(&npc));
}

fn lever_probe(dir: &Path, uses: &[&str], lever: &str) {
    write_pack(
        dir,
        &format!(
            "id = \"gm.lever_probe\"\nname = \"Probe\"\nversion = \"0.1.0\"\n\
             uses = [{}]\n[[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\n\
             handlers = [\"on.dock\"]\n",
            quoted(uses)
        ),
        Some(&format!(
            "--!strict\nlocal G = require(\"@galaxy\")\n\
             return G.script({{ id = \"main\", on = {{ dock = function(ev: G.DockEvent, ctx: G.Ctx)\n\
             \tlocal why = ctx:cause({{ summary = \"a probe lever\", cites = {{ ev.fact }} }})\n\
             \tctx.market:levers({{ {lever} }}, why)\n\
             end }} }})\n"
        )),
        None,
    );
}

const REMOVE_ICE: &str =
    "{ lever = \"remove_line\", station = \"station.kettle_dock\", ware = \"ware.ice\" }";
const STOCK_ICE: &str =
    "{ lever = \"set_stock\", station = \"station.kettle_dock\", ware = \"ware.ice\", stock = 3 }";

fn ice_line(sim: &Sim) -> Option<MarketLine> {
    sim.world()
        .markets
        .get(&KETTLE)
        .and_then(|m| m.lines.get(&ICE).cloned())
}

fn run_dock(dir: &Path) -> (Sim, ScriptHost) {
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir).unwrap();
    host.attach(&mut sim);
    dock(&mut sim, &KETTLE);
    settle(&mut sim);
    (sim, host)
}

#[test]
fn bulk_levers_need_market_lines_to_add_or_remove_a_line() {
    let levers_only = ["events.dock", "market.levers"];
    let dir = tempfile::tempdir().unwrap();
    lever_probe(dir.path(), &levers_only, REMOVE_ICE);
    let sim = new_sim(1);
    let r = check(dir.path(), &sim, false);
    assert!(!r.accepted(), "{r:#?}");
    assert!(codes(&r).contains("market.lines"), "{r:#?}");

    let (sim, host) = run_dock(dir.path());
    assert!(
        ice_line(&sim).is_some(),
        "the line went without market.lines"
    );
    let why = aborts(&host);
    assert!(why.contains("market.lines"), "{why}");

    // A restock is a lever, not a line: `market.levers` is enough.
    let dir = tempfile::tempdir().unwrap();
    lever_probe(dir.path(), &levers_only, STOCK_ICE);
    let (sim, host) = run_dock(dir.path());
    assert!(aborts(&host).is_empty());
    assert_eq!(ice_line(&sim).unwrap().stock, 3);

    // Declared, the line goes.
    let dir = tempfile::tempdir().unwrap();
    lever_probe(
        dir.path(),
        &["events.dock", "market.levers", "market.lines"],
        REMOVE_ICE,
    );
    let (sim, host) = run_dock(dir.path());
    assert!(aborts(&host).is_empty());
    assert!(ice_line(&sim).is_none());
}

/// The table on its own: whatever the hooks do, the commands a bulk lever writes for
/// a new or removed line (or a first line, which founds a market) need
/// `market.lines`, and a restock doesn't.
#[test]
fn the_table_tells_line_levers_from_stock_levers() {
    let sim = new_sim(1);
    let pack = pack_id("gm.lever_probe");
    let levers = vec!["market.levers".to_owned()];
    let caller = needs::Caller {
        pack: &pack,
        uses: &levers,
        layer: galaxy_contract::pack::PackLayer::GameMaster,
    };
    let batch = |l: Lever| {
        lever_batch(sim.world(), Cause::system("test", "levers"), &[l])
            .unwrap()
            .commands
    };
    let remove = batch(Lever::RemoveLine {
        station: KETTLE,
        ware: ICE,
    });
    let why = needs::check(&remove, &caller, needs::Route::Hooks).unwrap_err();
    assert!(
        why.contains("`set_market_line`") && why.contains("`market.lines`"),
        "{why}"
    );
    let stock = batch(Lever::SetStock {
        station: KETTLE,
        ware: ICE,
        stock: 3,
    });
    assert!(matches!(stock[..], [Command::SetMarketStock { .. }]));
    needs::check(&stock, &caller, needs::Route::Hooks).unwrap();
}
