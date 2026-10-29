//! Shared helpers: a core-pack world, and stand-ins for what W2's ship systems will
//! do (dock, undock, jump) so the economy can be driven headless.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::apply::{Staged, apply};
use galaxy_core::{Sim, SimState, new_game};
use galaxy_economy::EconomySystem;

pub const WHARF: StationId = StationId::from_static("station.lantern_wharf");
pub const KETTLE: StationId = StationId::from_static("station.kettle_dock");
pub const HOOK: StationId = StationId::from_static("station.cinder_hook");
pub const OBSERVATORY: StationId = StationId::from_static("station.glass_drift_observatory");
pub const BRASS: StationId = StationId::from_static("station.brassmouth_exchange");
pub const SHALLOWS: RegionId = RegionId::from_static("region.lantern_shallows");
pub const CINDER: RegionId = RegionId::from_static("region.cinder_drift");
pub const NARROWS: RegionId = RegionId::from_static("region.brassmouth_narrows");
pub const PLAYER: ShipId = ShipId::from_static("ship.player");
pub const BARGE: ShipId = ShipId::from_static("craft.ninefold");
pub const ICE: WareId = WareId::from_static("ware.ice");
pub const ORE: WareId = WareId::from_static("ware.iron_ore");
pub const PLATE: WareId = WareId::from_static("ware.refined_plate");
pub const GLASSITE: WareId = WareId::from_static("ware.glassite");
pub const RATIONS: WareId = WareId::from_static("ware.rations");
pub const FRAGMENT: WareId = WareId::from_static("ware.ancients_fragment");
pub const COMPACT: FactionId = FactionId::from_static("faction.tollway_compact");

pub fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

pub fn state() -> SimState {
    new_game(&data_dir(), 7).expect("core data loads")
}

/// A sim running only the economy.
pub fn sim() -> Sim {
    Sim::new(state()).with_systems([Box::new(EconomySystem) as Box<dyn galaxy_core::System>])
}

/// Applies setup commands straight to a world (test setup only).
pub fn setup(world: &mut WorldState, commands: impl IntoIterator<Item = Command>) {
    for c in commands {
        apply(world, &c, &mut Staged::default()).expect("setup applies");
    }
}

pub fn line(world: &WorldState, station: &StationId, ware: &WareId) -> MarketLine {
    world.markets.get(station).unwrap().lines[ware].clone()
}

fn test_cause(what: &str) -> Cause {
    Cause::system("test_ship", what)
}

/// Moves a ship to a station and docks it, emitting `dock` like W2 will.
pub fn dock_cmds(world: &WorldState, ship: &ShipId, station: &StationId) -> Vec<Command> {
    let s = world.stations.get(station).unwrap();
    vec![
        Command::SetShipRegion {
            ship: ship.clone(),
            region: s.region.clone(),
        },
        Command::SetShipMotion {
            ship: ship.clone(),
            position: s.position,
            velocity: Vec2::ZERO,
            heading: 0.0,
        },
        Command::SetShipDocked {
            ship: ship.clone(),
            station: Some(station.clone()),
        },
        Command::Emit {
            event: Event::Dock {
                ship: ship.clone(),
                station: station.clone(),
            },
        },
    ]
}

/// Docks, then lets the economy see it (two ticks).
pub fn dock(sim: &mut Sim, ship: &ShipId, station: &StationId) {
    let cmds = dock_cmds(sim.world(), ship, station);
    sim.submit(CommandBatch {
        cause: test_cause("dock"),
        commands: cmds,
    });
    step(sim);
    step(sim);
}

/// Undocks and moves `offset` from where it was.
pub fn undock(sim: &mut Sim, ship: &ShipId, offset: Vec2) {
    let s = sim.world().ships.get(ship).unwrap().clone();
    let mut batch = CommandBatch::new(test_cause("undock")).with(Command::SetShipMotion {
        ship: ship.clone(),
        position: s.position + offset,
        velocity: Vec2::ZERO,
        heading: 0.0,
    });
    if let Some(station) = s.docked_at {
        batch.push(Command::SetShipDocked {
            ship: ship.clone(),
            station: None,
        });
        batch.push(Command::Emit {
            event: Event::Undock {
                ship: ship.clone(),
                station,
            },
        });
    }
    sim.submit(batch);
    step(sim);
    step(sim);
}

/// A lane arrival into `to`.
pub fn jump(sim: &mut Sim, ship: &ShipId, to: &RegionId) {
    let s = sim.world().ships.get(ship).unwrap().clone();
    let center = sim.world().regions.get(to).unwrap().center;
    sim.submit(
        CommandBatch::new(test_cause("jump"))
            .with(Command::SetShipRegion {
                ship: ship.clone(),
                region: to.clone(),
            })
            .with(Command::SetShipMotion {
                ship: ship.clone(),
                position: center,
                velocity: Vec2::ZERO,
                heading: 0.0,
            })
            .with(Command::Emit {
                event: Event::Jump {
                    ship: ship.clone(),
                    lane: LaneId::new("lane.shallows_cinder").unwrap(),
                    from: s.region,
                    to: to.clone(),
                },
            }),
    );
    step(sim);
    step(sim);
}

pub fn give(sim: &mut Sim, ship: &ShipId, ware: &WareId, n: i64) {
    sim.submit(
        CommandBatch::new(test_cause("give")).with(Command::AdjustCargo {
            ship: ship.clone(),
            ware: ware.clone(),
            delta: n,
        }),
    );
    step(sim);
}

pub fn step(sim: &mut Sim) -> galaxy_core::TickReport {
    sim.step(&PlayerInput::default()).expect("not paused")
}

/// The player asks the economy for trades this tick (the dock screen).
pub fn trade_input(sim: &mut Sim, trades: Vec<TradeRequest>) -> galaxy_core::TickReport {
    sim.step(&PlayerInput {
        trades,
        ..PlayerInput::default()
    })
    .expect("not paused")
}

pub fn sell(ship: &ShipId, station: &StationId, ware: &WareId, n: u32) -> TradeRequest {
    TradeRequest {
        ship: ship.clone(),
        station: station.clone(),
        ware: ware.clone(),
        side: TradeSide::Sell,
        quantity: n,
        limit: None,
    }
}

pub fn buy(ship: &ShipId, station: &StationId, ware: &WareId, n: u32) -> TradeRequest {
    TradeRequest {
        side: TradeSide::Buy,
        ..sell(ship, station, ware, n)
    }
}

/// An NPC barge (Compact) in the world, undocked in the Narrows.
pub fn add_barge(sim: &mut Sim) {
    let ship = Ship {
        id: BARGE,
        name: "Ninefold".into(),
        hull: HullId::new("hull.tandem").unwrap(),
        owner: Owner::Faction { faction: COMPACT },
        region: NARROWS,
        position: Vec2::new(0.0, 0.0),
        velocity: Vec2::ZERO,
        heading: 0.0,
        docked_at: None,
        hold: Hold::default(),
        orders: OrderQueue::default(),
    };
    sim.submit(
        CommandBatch::new(test_cause("barge")).with(Command::PutShip {
            id: BARGE,
            value: Some(Box::new(ship)),
        }),
    );
    step(sim);
}
