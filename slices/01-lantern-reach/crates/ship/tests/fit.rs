//! The hullwright's fit in flight (#47 Phase 3): mass past the hull's free allowance
//! lowers top speed and turn rate through the modifier path, and the long-throw drill
//! mines from further off. Headless, over the core pack's data.

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::{Sim, new_game};
use galaxy_ship::FlatDesk;

const PLAYER: ShipId = ShipId::from_static("ship.player");
const SHALLOWS: RegionId = RegionId::from_static("region.lantern_shallows");
const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const PODS: UpgradeId = UpgradeId::from_static("upgrade.hold_pods");
const LONG_DRILL: UpgradeId = UpgradeId::from_static("upgrade.long_drill");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn game() -> Sim {
    let mut state = new_game(&data_dir(), 1).unwrap();
    for (id, _) in std::mem::take(&mut state.world.traders) {
        state.world.ships.remove(&id);
    }
    Sim::new(state).with_systems(galaxy_ship::systems(Box::new(FlatDesk)))
}

fn player(sim: &Sim) -> Ship {
    sim.world().ships.get(&PLAYER).unwrap().clone()
}

fn step(sim: &mut Sim, input: &PlayerInput) {
    let report = sim.step(input).expect("not paused");
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
}

fn submit(sim: &mut Sim, commands: Vec<Command>) {
    sim.submit(CommandBatch {
        cause: Cause::system("test", "test setup"),
        commands,
    });
    step(sim, &PlayerInput::default());
}

/// Fits these parts (as the hullwright's batch would), then puts the Wren afloat and
/// stopped at `at`, nose along +x.
fn fitted_afloat(parts: &[UpgradeId], at: Vec2) -> Sim {
    let mut sim = game();
    let mut c: Vec<Command> = parts
        .iter()
        .map(|u| Command::SetFitting {
            upgrade: u.clone(),
            fitted: true,
        })
        .collect();
    c.extend([
        Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        },
        Command::SetShipRegion {
            ship: PLAYER,
            region: SHALLOWS,
        },
        Command::SetShipMotion {
            ship: PLAYER,
            position: at,
            velocity: Vec2::ZERO,
            heading: 0.0,
        },
    ]);
    submit(&mut sim, c);
    sim
}

/// Top speed after a second of W, and degrees turned in half a second of D.
fn flown(parts: &[UpgradeId]) -> (f32, f32) {
    let mut sim = fitted_afloat(parts, Vec2::new(-300.0, -1200.0));
    let d = PlayerInput {
        thrust: Vec2::new(1.0, 0.0),
        ..PlayerInput::default()
    };
    for _ in 0..15 {
        step(&mut sim, &d);
    }
    let turned = -player(&sim).heading.to_degrees();
    let w = PlayerInput {
        thrust: Vec2::new(0.0, 1.0),
        ..PlayerInput::default()
    };
    for _ in 0..30 {
        step(&mut sim, &w);
    }
    (player(&sim).velocity.length(), turned)
}

#[test]
fn a_heavy_fit_flies_slower_and_turns_wider_than_a_light_one() {
    let (bare_speed, bare_turn) = flown(&[]);
    assert!((bare_speed - 165.0).abs() < 0.01, "{bare_speed}");
    assert!((bare_turn - 90.0).abs() < 0.1, "{bare_turn}");

    // The lens alone weighs 1 of the Wren's 3 free: no change.
    let (speed, turn) = flown(&[LENS]);
    assert!((speed - bare_speed).abs() < 0.01 && (turn - bare_turn).abs() < 0.1);

    // A hauler: hold pods (4) and the long-throw drill (2) are 6, three over the free
    // mass: 4% of top speed and 6% of turn rate each.
    let (speed, turn) = flown(&[PODS, LONG_DRILL]);
    assert!((speed - 165.0 * 0.88).abs() < 0.05, "{speed}");
    assert!((turn - 90.0 * 0.82).abs() < 0.1, "{turn}");
    assert!(bare_speed - speed > 15.0 && bare_turn - turn > 15.0);
}

#[test]
fn the_long_throw_drill_mines_from_further_off() {
    let world = game().world().clone();
    let reach = world.ship_rules.mining.reach;
    // A rock in the Shallows and a point 15 u past the rules' reach from its edge, with
    // no other rock within the drill's reach of it.
    let clear = |p: Vec2, id: &RockId| {
        world
            .rocks
            .values()
            .all(|r| &r.id == id || !galaxy_ship::space::within_reach(&world, r, p, reach + 40.0))
    };
    let (target, at) = world
        .rocks
        .values()
        .filter(|r| world.fields.get(&r.field).unwrap().region == SHALLOWS)
        .flat_map(|r| {
            let edge = galaxy_ship::space::rock_radius(&world, r);
            (0..16).map(move |i| {
                let a = i as f32 * std::f32::consts::TAU / 16.0;
                (
                    r.id.clone(),
                    r.position + Vec2::new(libm::cosf(a), libm::sinf(a)) * (edge + reach + 15.0),
                )
            })
        })
        .find(|(id, p)| clear(*p, id))
        .expect("a rock with a clear side");
    let full = world.rocks.get(&target).unwrap().remaining;
    let left = |sim: &Sim| sim.world().rocks.get(&target).unwrap().remaining;
    let hold = PlayerInput {
        action: true,
        ..PlayerInput::default()
    };

    let mut bare = fitted_afloat(&[], at);
    assert_eq!(
        galaxy_ship::space::mining_reach(bare.world(), &PLAYER),
        reach
    );
    for _ in 0..60 {
        step(&mut bare, &hold);
    }
    assert_eq!(left(&bare), full, "out of reach without it");
    assert_eq!(player(&bare).hold.total(), 0);

    let mut drill = fitted_afloat(&[LONG_DRILL], at);
    assert_eq!(
        galaxy_ship::space::mining_reach(drill.world(), &PLAYER),
        reach + 30.0
    );
    let r = drill.world().rocks.get(&target).unwrap().clone();
    assert!(galaxy_ship::space::in_reach_of(
        drill.world(),
        &PLAYER,
        &r,
        at
    ));
    for _ in 0..60 {
        step(&mut drill, &hold);
    }
    assert!(left(&drill) < full, "the drill reaches it");
}
