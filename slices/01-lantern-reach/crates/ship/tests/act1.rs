//! Act 1 headless against the real economy: the game's composition (the kernel, the
//! ship systems with `EconomyDesk`, then `galaxy_economy::EconomySystem`), flown by
//! input like a player would.
//!
//! Undock from the Wharf, fly to an ice rock, hold Space until the hold is full, fly to
//! Kettle Dock and dock, sell from the dock screen (`PlayerInput::trades`) for exactly
//! what the economy quoted. Then a tender waits at Kettle with a patient sell above
//! the price the player's sale left, and sells once activity (the player's next
//! counted docking) brings the price back to its floor.

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::{Sim, SimState, new_game};
use galaxy_economy::pricing::{self, spread_at};
use galaxy_economy::pulse::pulse_market;
use galaxy_ship::controls::{self, Capability};
use galaxy_ship::{EconomyDesk, TradeDesk};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const THIMBLE: ShipId = ShipId::from_static("craft.thimble");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const ICE_ROCK: RockId = RockId::from_static("rock.shallows.ice.03");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// The same systems, in the same order, as `game()` in the app.
fn game(state: SimState) -> Sim {
    galaxy_core::game(state)
        .with_systems(galaxy_ship::systems(Box::new(EconomyDesk)))
        .with_systems([Box::new(galaxy_economy::EconomySystem) as Box<dyn galaxy_core::System>])
}

fn step(sim: &mut Sim, input: &PlayerInput) {
    let report = sim.step(input).expect("not paused");
    assert!(report.rejected.is_empty(), "{:#?}", report.rejected);
}

fn hands_off(sim: &mut Sim, ticks: u32) {
    for _ in 0..ticks {
        step(sim, &PlayerInput::default());
    }
}

/// One tick with Space down (a press if it was up last tick).
fn space(sim: &mut Sim) {
    step(
        sim,
        &PlayerInput {
            action: true,
            ..PlayerInput::default()
        },
    );
}

/// One tick of W: docked, that flies off and undocks.
fn fly_off(sim: &mut Sim) {
    step(
        sim,
        &PlayerInput {
            thrust: Vec2::new(0.0, 1.0),
            ..PlayerInput::default()
        },
    );
}

fn player(sim: &Sim) -> &Ship {
    sim.world().ships.get(&PLAYER).unwrap()
}

fn kind_count(sim: &Sim, kind: &str) -> usize {
    sim.history()
        .count(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
}

/// Flies the player by the tank keys until `arrived`, easing off near `to`, then lets
/// go until it stops.
fn fly(sim: &mut Sim, to: Vec2, arrived: impl Fn(&Sim) -> bool) {
    for _ in 0..3000 {
        if arrived(sim) {
            break;
        }
        let ship = player(sim);
        let feel = galaxy_ship::flight::Feel::of_ship(sim.world(), ship).unwrap();
        let motion = galaxy_ship::flight::Motion::of(ship);
        let keys = galaxy_ship::flight::steer_to(&feel, motion, to, 4.0);
        step(
            sim,
            &PlayerInput {
                thrust: keys,
                ..PlayerInput::default()
            },
        );
    }
    assert!(arrived(sim), "didn't get there: {:?}", player(sim).position);
    for _ in 0..60 {
        if player(sim).velocity == Vec2::ZERO {
            return;
        }
        hands_off(sim, 1);
    }
    panic!("still moving");
}

/// Flies to the ice rock and holds Space until the hold is full.
fn mine_ice(sim: &mut Sim) {
    let rock = sim.world().rocks.get(&ICE_ROCK).unwrap().clone();
    let radius = galaxy_ship::space::rock_radius(sim.world(), &rock);
    let from = player(sim).position;
    let spot = rock.position + (from - rock.position).normalize_or_zero() * (radius + 15.0);
    fly(sim, spot, |s| {
        let r = s.world().rocks.get(&ICE_ROCK).unwrap();
        galaxy_ship::space::in_reach(s.world(), r, player(s).position)
            && player(s).position.distance(spot) < 10.0
    });
    let cap = sim
        .world()
        .hulls
        .get(&player(sim).hull)
        .unwrap()
        .hold_capacity;
    for _ in 0..3000 {
        if player(sim).hold.total() >= u64::from(cap) {
            break;
        }
        space(sim);
    }
    assert_eq!(player(sim).hold.count(&ICE), cap, "a full hold of ice");
    hands_off(sim, 1); // let go of Space: the run ends
}

/// Flies into Kettle Dock's ring and presses Space.
fn dock_at_kettle(sim: &mut Sim) {
    let kettle = sim.world().stations.get(&KETTLE).unwrap().clone();
    fly(sim, kettle.position, |s| {
        player(s).position.distance(kettle.position) < kettle.dock_radius - 10.0
    });
    space(sim);
    assert_eq!(player(sim).docked_at, Some(KETTLE));
    // The economy counts the docking next tick and applies its pulse.
    hands_off(sim, 3);
}

/// The dock screen's "sell `quantity` ice" for the flagship.
fn sell_ice(quantity: u32) -> TradeRequest {
    TradeRequest {
        ship: PLAYER,
        station: KETTLE,
        ware: ICE,
        side: TradeSide::Sell,
        quantity,
        limit: None,
    }
}

fn trading(request: TradeRequest) -> PlayerInput {
    PlayerInput {
        trades: vec![request],
        ..PlayerInput::default()
    }
}

/// What Kettle pays for the next unit of ice now.
fn kettle_pays(sim: &Sim) -> u32 {
    galaxy_economy::quote(sim.world(), &KETTLE, &ICE, TradeSide::Sell, 1)
        .unwrap()
        .unit_prices[0]
}

#[test]
fn act_one_mines_ice_and_sells_it_at_kettle_through_the_real_economy() {
    let mut sim = game(new_game(&data_dir(), 7).unwrap());
    hands_off(&mut sim, 2); // the economy prices every line on its first tick
    assert_eq!(player(&sim).docked_at, Some(WHARF));

    // Fly off to undock, mine, fly to Kettle, dock.
    space(&mut sim);
    assert_eq!(player(&sim).docked_at, Some(WHARF), "Space doesn't undock");
    fly_off(&mut sim);
    assert_eq!(player(&sim).docked_at, None, "flying off undocks");
    hands_off(&mut sim, 1);
    mine_ice(&mut sim);
    dock_at_kettle(&mut sim);
    let held = player(&sim).hold.count(&ICE);
    assert!(held > 0);

    // Sell the lot from the dock screen for what the economy quotes.
    let quote = galaxy_economy::quote(sim.world(), &KETTLE, &ICE, TradeSide::Sell, held).unwrap();
    assert_eq!(quote.units(), held);
    galaxy_economy::check_trade(sim.world(), &sell_ice(held)).unwrap();
    let credits = sim.world().player.credits;
    let before = kettle_pays(&sim);
    step(&mut sim, &trading(sell_ice(held)));
    assert_eq!(player(&sim).hold.count(&ICE), 0);
    assert_eq!(sim.world().player.credits, credits + quote.total);
    assert!(quote.total > 0);
    assert_eq!(kind_count(&sim, "sell"), 1);
    // Selling saturates the line: Kettle now pays less.
    let after = kettle_pays(&sim);
    assert!(after < before, "{after} < {before}");
}

#[test]
fn a_tenders_patient_sell_waits_for_the_real_market_to_recover() {
    let mut sim = game(new_game(&data_dir(), 7).unwrap());
    hands_off(&mut sim, 2);
    fly_off(&mut sim);
    hands_off(&mut sim, 1);
    mine_ice(&mut sim);
    dock_at_kettle(&mut sim);
    let held = player(&sim).hold.count(&ICE);
    step(&mut sim, &trading(sell_ice(held)));
    assert_eq!(player(&sim).hold.count(&ICE), 0);

    // The price one docking pulse at Kettle will bring back, by the economy's maths.
    let low = kettle_pays(&sim);
    let floor = {
        let w = sim.world();
        let rules = &w.economy.rules.pricing;
        let next = pulse_market(w.markets.get(&KETTLE).unwrap(), 1.0, rules);
        pricing::unit_cents(&next[&ICE], rules, spread_at(w, &KETTLE))
    };
    assert!(floor > low, "a pulse raises the price: {low} -> {floor}");

    // A tender docked at Kettle with one unit of ice.
    let kettle = sim.world().stations.get(&KETTLE).unwrap().clone();
    let mut hold = Hold::default();
    hold.cargo.insert(ICE, 1);
    let tender = Ship {
        id: THIMBLE,
        name: "Thimble".into(),
        hull: HullId::from_static("hull.tender"),
        owner: Owner::Player,
        region: kettle.region.clone(),
        position: kettle.position,
        velocity: Vec2::ZERO,
        heading: 0.0,
        docked_at: Some(KETTLE),
        hold,
        orders: OrderQueue::default(),
        activity: ShipActivity::default(),
    };
    let mut setup =
        CommandBatch::new(Cause::system("test", "a tender at Kettle")).with(Command::PutShip {
            id: THIMBLE,
            value: Some(Box::new(tender)),
        });
    for cap in [Capability::Loops, Capability::Automation] {
        setup.push(controls::set_capability(sim.world(), cap, true));
    }
    setup.push(Command::SetLoop {
        name: "sell ice".into(),
        orders: Some(vec![Order::Sell {
            ware: ICE,
            quantity: Quantity::All,
            floor: None,
        }]),
    });
    sim.submit(setup);
    hands_off(&mut sim, 1);

    // The economy's ceiling is the top of Kettle's price band for ice.
    let ceiling = EconomyDesk.ceiling(sim.world(), &KETTLE, &ICE).unwrap();
    let line = &sim.world().markets.get(&KETTLE).unwrap().lines[&ICE];
    assert_eq!(ceiling, line.max_price);
    assert!(
        controls::assign_loop(
            sim.world(),
            &EconomyDesk,
            &THIMBLE,
            "sell ice",
            Some(ceiling + 1)
        )
        .is_err(),
        "a floor above the ceiling is refused"
    );
    let assign =
        controls::assign_loop(sim.world(), &EconomyDesk, &THIMBLE, "sell ice", Some(floor))
            .unwrap();
    sim.submit(assign);

    // Below the floor it waits docked, however long: no clock moves the market.
    let credits = sim.world().player.credits;
    hands_off(&mut sim, 300);
    let t = sim.world().ships.get(&THIMBLE).unwrap();
    assert_eq!(t.docked_at, Some(KETTLE));
    assert_eq!(t.hold.count(&ICE), 1);
    assert_eq!(kind_count(&sim, "patient_wait"), 1);
    assert_eq!(kind_count(&sim, "sell"), 1, "only the player's sale");
    assert_eq!(sim.world().player.credits, credits);
    assert_eq!(kettle_pays(&sim), low);

    // Activity: the player's next lap ends in a counted docking at Kettle, and its pulse
    // lifts the price to the floor.
    fly_off(&mut sim);
    assert_eq!(player(&sim).docked_at, None);
    hands_off(&mut sim, 1);
    mine_ice(&mut sim);
    let credits = sim.world().player.credits;
    dock_at_kettle(&mut sim);
    for _ in 0..10 {
        if kind_count(&sim, "patient_sold") == 1 {
            break;
        }
        hands_off(&mut sim, 1);
    }
    assert_eq!(kind_count(&sim, "patient_sold"), 1);
    let t = sim.world().ships.get(&THIMBLE).unwrap();
    assert_eq!(t.hold.count(&ICE), 0);
    assert_eq!(t.docked_at, Some(KETTLE));
    assert_eq!(kind_count(&sim, "sell"), 2);
    // The tender is the player's: its sale pays the player exactly the floor price.
    assert_eq!(sim.world().player.credits, credits + i64::from(floor));
}
