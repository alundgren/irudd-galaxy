//! The `Expect*` checks the PR #21 review fixes rely on: each fails on a world that
//! changed since its batch was worked out, and a rollback leaves them out.

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::{Sim, TickReport, new_game};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const ICE: WareId = WareId::from_static("ware.ice");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");
const WREN: HullId = HullId::from_static("hull.wren");
const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn step(s: &mut Sim) -> TickReport {
    s.step(&PlayerInput::default()).unwrap()
}

fn batch(commands: Vec<Command>) -> CommandBatch {
    CommandBatch {
        cause: Cause::system("test", "check"),
        commands,
    }
}

#[test]
fn expectations_hold_or_reject_their_batch() {
    let mut s = Sim::new(new_game(&data_dir(), 3).unwrap());
    let coop = s.world().factions.get(&COOP).unwrap().standing;
    let held = vec![
        Command::ExpectShip {
            ship: PLAYER,
            hull: Some(WREN),
        },
        Command::ExpectShip {
            ship: ShipId::from_static("ship.berth.wren"),
            hull: None,
        },
        Command::ExpectStanding {
            faction: COOP,
            min: coop,
            max: coop,
        },
        Command::ExpectFitting {
            upgrade: LENS,
            fitted: false,
        },
        Command::ExpectHullRecord {
            hull: WREN,
            record: s.world().player.hull_records.get(&WREN).cloned(),
        },
    ];
    for c in &held {
        s.submit(batch(vec![c.clone()]));
    }
    let r = step(&mut s);
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    assert_eq!(r.applied, held.len());

    let broken = [
        Command::ExpectShip {
            ship: PLAYER,
            hull: Some(HullId::from_static("hull.tandem")),
        },
        Command::ExpectShip {
            ship: PLAYER,
            hull: None,
        },
        Command::ExpectStanding {
            faction: COOP,
            min: coop + 1,
            max: coop + 50,
        },
        Command::ExpectFitting {
            upgrade: LENS,
            fitted: true,
        },
        Command::ExpectHullRecord {
            hull: WREN,
            record: Some(HullRecord {
                rank: 9,
                ..HullRecord::default()
            }),
        },
    ];
    let before = s.world().clone();
    for c in &broken {
        s.submit(batch(vec![
            Command::AdjustCredits { delta: 100 },
            c.clone(),
        ]));
    }
    let r = step(&mut s);
    assert_eq!(r.rejected.len(), broken.len(), "{:#?}", r.rejected);
    assert_eq!(s.world().player, before.player, "nothing was paid");
}

#[test]
fn a_rollback_leaves_the_expectations_out() {
    let mut s = Sim::new(new_game(&data_dir(), 4).unwrap());
    // A scripted handover, made while docked at the Wharf.
    s.submit(CommandBatch {
        cause: Cause::script(PackId::new("gm").unwrap(), "r1", "a handover"),
        commands: vec![
            Command::ExpectDocked {
                ship: PLAYER,
                station: WHARF,
            },
            Command::AdjustCargo {
                ship: PLAYER,
                ware: ICE,
                delta: 4,
            },
        ],
    });
    step(&mut s);
    let cause = s.journal()[0].cause;
    s.submit(batch(vec![Command::SetShipDocked {
        ship: PLAYER,
        station: None,
    }]));
    step(&mut s);
    // Undocked now, but undoing the handover doesn't depend on where the ship is.
    assert_eq!(s.queue_rollback(&[cause], "undo").batches, 1);
    let r = step(&mut s);
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    assert_eq!(s.world().ships.get(&PLAYER).unwrap().hold.count(&ICE), 0);
}
