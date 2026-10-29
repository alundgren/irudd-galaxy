//! Shared helpers for the progress tests: a new game over the core pack's data with
//! the progress system, and the player's deeds as the batches the ship and economy
//! systems would submit. All headless.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_progress::ProgressSystem;

pub const PLAYER: ShipId = ShipId::from_static("ship.player");
pub const WHARF: StationId = StationId::from_static("station.lantern_wharf");
pub const KETTLE: StationId = StationId::from_static("station.kettle_dock");
pub const OBSERVATORY: StationId = StationId::from_static("station.glass_drift_observatory");
pub const BRASSMOUTH: StationId = StationId::from_static("station.brassmouth_exchange");
pub const ICE: WareId = WareId::from_static("ware.ice");
pub const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");
pub const LAMPS: FactionId = FactionId::from_static("faction.lamplighter_society");
pub const WREN: HullId = HullId::from_static("hull.wren");
pub const TANDEM: HullId = HullId::from_static("hull.tandem");

pub fn core_data() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// A new game with the progress system and nothing else (the tests act for the ship
/// and economy systems).
pub fn sim(seed: u64) -> Sim {
    let mut sim = Sim::new(galaxy_core::new_game(&core_data(), seed).expect("core data loads"));
    sim.add_system(Box::new(ProgressSystem));
    sim
}

pub fn run(sim: &mut Sim, ticks: u64) {
    for _ in 0..ticks {
        let r = sim.step(&PlayerInput::default()).expect("not paused");
        assert!(r.rejected.is_empty(), "rejected: {:#?}", r.rejected);
    }
}

/// Runs one tick with `input` and returns its report.
pub fn tick(sim: &mut Sim, input: &PlayerInput) -> galaxy_core::TickReport {
    sim.step(input).expect("not paused")
}

fn act(summary: &str, commands: Vec<Command>) -> CommandBatch {
    CommandBatch {
        cause: Cause::system("test_player", summary),
        commands,
    }
}

/// The player docks at a station (in its region).
pub fn dock(sim: &mut Sim, station: &StationId) {
    let region = sim.world().stations.get(station).unwrap().region.clone();
    sim.submit(act(
        "dock",
        vec![
            Command::SetShipRegion {
                ship: PLAYER,
                region,
            },
            Command::SetShipDocked {
                ship: PLAYER,
                station: Some(station.clone()),
            },
            Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: station.clone(),
                },
            },
        ],
    ));
    run(sim, 1);
}

pub fn undock(sim: &mut Sim) {
    let station = sim
        .world()
        .ships
        .get(&PLAYER)
        .unwrap()
        .docked_at
        .clone()
        .unwrap();
    sim.submit(act(
        "undock",
        vec![
            Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            },
            Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station,
                },
            },
        ],
    ));
    run(sim, 1);
}

/// The player sells `qty` of a ware where they're docked (the event only).
pub fn sell(sim: &mut Sim, ware: &WareId, qty: u32) {
    let station = sim
        .world()
        .ships
        .get(&PLAYER)
        .unwrap()
        .docked_at
        .clone()
        .unwrap();
    sim.submit(act(
        "sell",
        vec![Command::Emit {
            event: Event::Sell {
                ship: PLAYER,
                station,
                ware: ware.clone(),
                quantity: qty,
                unit_price: 1000,
            },
        }],
    ));
    run(sim, 1);
}

pub fn jump(sim: &mut Sim) {
    let lane = sim.world().lanes.keys().next().unwrap().clone();
    let l = sim.world().lanes.get(&lane).unwrap().clone();
    sim.submit(act(
        "jump",
        vec![Command::Emit {
            event: Event::Jump {
                ship: PLAYER,
                lane,
                from: l.a,
                to: l.b,
            },
        }],
    ));
    run(sim, 1);
}

pub fn credits(sim: &mut Sim, delta_cents: i64) {
    sim.submit(act(
        "credits",
        vec![Command::AdjustCredits { delta: delta_cents }],
    ));
    run(sim, 1);
}

/// Puts a beat in a state directly (as if the core pack had run it).
pub fn set_beat(sim: &mut Sim, beat: &str, status: BeatStatus, step: u32) {
    sim.submit(act(
        "beat",
        vec![Command::PutBeat {
            id: BeatId::new(beat).unwrap(),
            value: Some(Box::new(BeatState {
                pack: PackId::from_static("core"),
                act: 1,
                kind: BeatKind::Directed,
                status,
                step,
                step_id: None,
                goal: "test".into(),
                giver: None,
                since: None,
                step_since: None,
                done_since: None,
                started: Tick(0),
            })),
        }],
    ));
    run(sim, 1);
}

pub fn count(sim: &Sim, kind: &str) -> usize {
    sim.history()
        .count(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
}

pub fn standing(sim: &Sim, f: &FactionId) -> i32 {
    sim.world().factions.get(f).unwrap().standing
}
