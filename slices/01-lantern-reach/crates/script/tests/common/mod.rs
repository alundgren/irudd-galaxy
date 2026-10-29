//! Shared helpers for the script host tests. All headless.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, SaidLine};
use galaxy_core::Sim;
use galaxy_script::ScriptHost;

pub const KETTLE: StationId = StationId::from_static("station.kettle_dock");
pub const PLAYER: ShipId = ShipId::from_static("ship.player");
pub const ICE: WareId = WareId::from_static("ware.ice");
pub const PLATE: WareId = WareId::from_static("ware.refined_plate");
pub const ODO: NpcId = NpcId::from_static("npc.odo_fenn");
pub const KETTLE_PACK: &str = "gm.kettle_growth";

pub fn slice() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub fn core_data() -> PathBuf {
    slice().join("packs/core/data")
}

pub fn examples() -> PathBuf {
    slice().join("packs/gm-examples")
}

pub fn kettle_dir() -> PathBuf {
    examples().join("kettle_growth")
}

pub fn bad(name: &str) -> PathBuf {
    examples().join("bad_script").join(name)
}

pub fn pack_id(s: &str) -> PackId {
    s.parse().unwrap()
}

/// A sim over a new game, with no systems yet.
pub fn new_sim(seed: u64) -> Sim {
    Sim::new(galaxy_core::new_game(&core_data(), seed).expect("core data loads"))
}

/// A sim with the kettle pack loaded and attached (Odo installed after one tick).
pub fn kettle_sim(dir: &Path) -> (Sim, ScriptHost) {
    let mut sim = new_sim(1);
    let host = ScriptHost::new(None);
    let report = host.load(dir, sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    step(&mut sim, 1);
    assert!(sim.world().npcs.contains(&ODO), "the pack installs Odo");
    (sim, host)
}

pub fn step(sim: &mut Sim, n: u64) {
    let input = PlayerInput::default();
    for _ in 0..n {
        let r = sim.step(&input).expect("not paused");
        assert!(r.rejected.is_empty(), "rejected batches: {:#?}", r.rejected);
    }
}

/// The player hands `qty` of a ware to Kettle Dock's greenhouse.
pub fn deliver(sim: &mut Sim, ware: &WareId, qty: u32) {
    sim.submit(
        CommandBatch::new(Cause::system("test_player", "delivery")).with(Command::Emit {
            event: Event::Deliver {
                ship: PLAYER,
                station: KETTLE,
                ware: ware.clone(),
                quantity: qty,
                ledger: None,
            },
        }),
    );
}

/// The player sells at Kettle Dock.
pub fn sell(sim: &mut Sim, ware: &WareId, qty: u32) {
    sim.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Sell, "sale")).with(Command::Emit {
            event: Event::Sell {
                ship: PLAYER,
                station: KETTLE,
                ware: ware.clone(),
                quantity: qty,
                unit_price: 10,
            },
        }),
    );
}

pub fn dock(sim: &mut Sim, station: &StationId) {
    sim.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Dock, "dock")).with(Command::Emit {
            event: Event::Dock {
                ship: PLAYER,
                station: station.clone(),
            },
        }),
    );
}

/// Lets submitted events commit (one tick) and scripts react (one more).
pub fn settle(sim: &mut Sim) {
    step(sim, 2);
}

pub fn stage(sim: &Sim) -> u32 {
    sim.world().stations.get(&KETTLE).unwrap().build.stage
}

pub fn lines(sim: &Sim, channel: Channel) -> Vec<SaidLine> {
    sim.world().talk.on(channel).cloned().collect()
}

pub fn store_built(sim: &Sim) -> Option<galaxy_contract::pack::ScriptValue> {
    sim.world()
        .packs
        .get(KETTLE_PACK)?
        .stores
        .get("kettle_growth")?
        .get("progress")?
        .get("built")
        .cloned()
}

/// Grows Kettle to stage 2: 60 ice and 20 plate, half delivered, half sold.
pub fn grow_to_stage_2(sim: &mut Sim) {
    deliver(sim, &ICE, 30);
    sell(sim, &ICE, 30);
    settle(sim);
    assert_eq!(stage(sim), 0, "60 ice alone isn't enough");
    deliver(sim, &PLATE, 20);
    settle(sim);
    assert_eq!(stage(sim), 2);
}

/// Copies a folder tree (for packs a test edits).
pub fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let dest = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &dest);
        } else {
            std::fs::copy(e.path(), dest).unwrap();
        }
    }
}
