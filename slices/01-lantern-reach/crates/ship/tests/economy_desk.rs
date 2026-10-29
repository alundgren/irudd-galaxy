//! `EconomyDesk`: the order runner's "up to" requests fitted to `galaxy_economy`, its
//! errors mapped, its commands passed through with the `ExpectDocked` guard.

use std::path::{Path, PathBuf};

use galaxy_contract::prelude::*;
use galaxy_core::{Sim, new_game};
use galaxy_economy::pricing;
use galaxy_ship::{EconomyDesk, Side, TradeDesk, TradeError, TradeRequest};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const BARGE: ShipId = ShipId::from_static("npc.barge");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const RATIONS: WareId = WareId::from_static("ware.rations");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

/// A new game with the real systems, one tick in so the economy has priced it.
fn game() -> Sim {
    let mut sim = galaxy_core::game(new_game(&data_dir(), 3).unwrap())
        .with_systems(galaxy_ship::systems(Box::new(EconomyDesk)))
        .with_systems([Box::new(galaxy_economy::EconomySystem) as Box<dyn galaxy_core::System>]);
    step(&mut sim);
    sim
}

fn step(sim: &mut Sim) {
    let report = sim.step(&PlayerInput::default()).unwrap();
    assert!(report.rejected.is_empty(), "{:#?}", report.rejected);
}

fn setup(sim: &mut Sim, commands: Vec<Command>) {
    let mut batch = CommandBatch::new(Cause::system("test", "setup"));
    batch.commands = commands;
    sim.submit(batch);
    step(sim);
}

fn request(ship: &ShipId, ware: &WareId, side: Side, quantity: u32) -> TradeRequest {
    TradeRequest {
        ship: ship.clone(),
        station: WHARF,
        ware: ware.clone(),
        side,
        quantity,
        limit: None,
    }
}

fn ask(sim: &Sim, r: &TradeRequest) -> Result<galaxy_ship::Trade, TradeError> {
    EconomyDesk.trade(sim.world(), r)
}

/// The economy's next `n` rations prices at the Wharf.
fn rations_prices(sim: &Sim, n: u32) -> Vec<u32> {
    galaxy_economy::quote(sim.world(), &WHARF, &RATIONS, TradeSide::Buy, n)
        .unwrap()
        .unit_prices
}

/// Changes the Wharf's rations line, priced as the economy would, so its re-pricing
/// doesn't race the change.
fn set_rations(sim: &mut Sim, f: impl FnOnce(&mut MarketLine)) {
    let w = sim.world();
    let mut line = w.markets.get(&WHARF).unwrap().lines[&RATIONS].clone();
    f(&mut line);
    let line = pricing::priced(
        &line,
        &w.economy.rules.pricing,
        pricing::spread_at(w, &WHARF),
    );
    setup(
        sim,
        vec![Command::SetMarketLine {
            station: WHARF,
            ware: RATIONS,
            line: Some(line),
        }],
    );
}

#[test]
fn a_limitless_buy_is_clamped_to_what_the_credits_afford() {
    let sim = game();
    let free = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    let credits = sim.world().player.credits;
    let prices = rations_prices(&sim, free);
    let mut spent = 0;
    let afford = prices
        .iter()
        .take_while(|&&p| {
            spent += i64::from(p);
            spent <= credits
        })
        .count() as u32;
    assert!(
        afford > 0 && afford < free,
        "the start credits buy some: {afford}"
    );

    let trade = ask(&sim, &request(&PLAYER, &RATIONS, Side::Buy, free)).unwrap();
    assert_eq!(trade.units, afford);
    let total: i64 = prices[..afford as usize]
        .iter()
        .map(|&p| i64::from(p))
        .sum();
    assert_eq!(trade.total, total);
    assert!(
        matches!(&trade.commands[0], Command::ExpectDocked { ship, station }
            if *ship == PLAYER && *station == WHARF),
        "the economy's docking guard opens the batch"
    );
    assert!(
        trade
            .commands
            .iter()
            .any(|c| matches!(c, Command::AdjustCredits { delta } if *delta == -total))
    );
}

#[test]
fn a_limitless_buy_is_clamped_to_the_stock_and_the_free_hold() {
    let mut sim = game();
    setup(&mut sim, vec![Command::AdjustCredits { delta: 1_000_000 }]);
    let free = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    let trade = ask(&sim, &request(&PLAYER, &RATIONS, Side::Buy, 500)).unwrap();
    assert_eq!(trade.units, free, "the hold");

    set_rations(&mut sim, |l| l.stock = 3);
    let trade = ask(&sim, &request(&PLAYER, &RATIONS, Side::Buy, free)).unwrap();
    assert_eq!(trade.units, 3, "the stock");
    assert_eq!(
        trade.total,
        rations_prices(&sim, 3)
            .iter()
            .map(|&p| i64::from(p))
            .sum::<i64>()
    );

    set_rations(&mut sim, |l| l.stock = 0);
    assert_eq!(
        ask(&sim, &request(&PLAYER, &RATIONS, Side::Buy, free)).unwrap_err(),
        TradeError::NoStock
    );
}

#[test]
fn the_runner_buys_all_through_the_economy() {
    let mut sim = game();
    let credits = sim.world().player.credits;
    let price = rations_prices(&sim, 1)[0];
    setup(
        &mut sim,
        vec![Command::SetOrders {
            ship: PLAYER,
            orders: OrderQueue {
                orders: vec![Order::Buy {
                    ware: RATIONS,
                    quantity: Quantity::All,
                }],
                ..OrderQueue::default()
            },
        }],
    );
    for _ in 0..5 {
        step(&mut sim);
    }
    let held = sim.world().ships.get(&PLAYER).unwrap().hold.count(&RATIONS);
    assert!(held > 0);
    let spent = credits - sim.world().player.credits;
    assert!(spent > 0 && spent <= credits);
    let next = rations_prices(&sim, 1)[0];
    assert!(next >= price, "buying raised the price");
    assert!(
        sim.world().player.credits < i64::from(next),
        "spent down to under a unit"
    );
    assert!(
        sim.world()
            .ships
            .get(&PLAYER)
            .unwrap()
            .orders
            .orders
            .is_empty()
    );
}

#[test]
fn errors_map_to_the_ships_trade_errors() {
    let mut sim = game();
    let free = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    let err = |sim: &Sim, r: TradeRequest| ask(sim, &r).unwrap_err();

    assert_eq!(
        err(&sim, request(&BARGE, &ICE, Side::Sell, 1)),
        TradeError::UnknownShip(BARGE)
    );
    assert_eq!(
        err(
            &sim,
            TradeRequest {
                station: KETTLE,
                ..request(&PLAYER, &ICE, Side::Sell, 1)
            }
        ),
        TradeError::NotDocked(KETTLE)
    );
    // The Wharf sells rations: it doesn't buy them.
    assert_eq!(
        err(&sim, request(&PLAYER, &RATIONS, Side::Sell, 1)),
        TradeError::NotTraded {
            station: WHARF,
            ware: RATIONS
        }
    );
    assert_eq!(
        err(&sim, request(&PLAYER, &ICE, Side::Sell, 5)),
        TradeError::NothingToSell
    );
    assert_eq!(
        err(&sim, request(&PLAYER, &RATIONS, Side::Buy, 0)),
        TradeError::HoldFull
    );
    let price = rations_prices(&sim, 1)[0];
    assert_eq!(
        err(
            &sim,
            TradeRequest {
                limit: Some(price - 1),
                ..request(&PLAYER, &RATIONS, Side::Buy, 2)
            }
        ),
        TradeError::PastLimit {
            price,
            limit: price - 1
        }
    );
    let credits = sim.world().player.credits;
    setup(&mut sim, vec![Command::AdjustCredits { delta: -credits }]);
    assert_eq!(
        err(&sim, request(&PLAYER, &RATIONS, Side::Buy, 2)),
        TradeError::NoCredits
    );
    setup(
        &mut sim,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: i64::from(free),
        }],
    );
    assert_eq!(
        err(&sim, request(&PLAYER, &RATIONS, Side::Buy, 2)),
        TradeError::HoldFull
    );
}

#[test]
fn a_floored_sell_fills_while_the_price_holds_and_npcs_have_no_wallet() {
    let mut sim = game();
    setup(
        &mut sim,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 15,
        }],
    );
    let prices = galaxy_economy::quote(sim.world(), &WHARF, &ICE, TradeSide::Sell, 15)
        .unwrap()
        .unit_prices;
    assert!(prices[0] > prices[14], "each unit sold lowers the price");
    let floor = prices[4];
    let filled = prices.iter().take_while(|&&p| p >= floor).count() as u32;
    let trade = ask(
        &sim,
        &TradeRequest {
            limit: Some(floor),
            ..request(&PLAYER, &ICE, Side::Sell, 15)
        },
    )
    .unwrap();
    assert_eq!(trade.units, filled);
    assert!((5..15).contains(&filled));
    assert_eq!(
        ask(
            &sim,
            &TradeRequest {
                limit: Some(prices[0] + 1),
                ..request(&PLAYER, &ICE, Side::Sell, 15)
            },
        )
        .unwrap_err(),
        TradeError::PastLimit {
            price: prices[0],
            limit: prices[0] + 1
        }
    );

    // An NPC hauler buys without a wallet: only the stock and its hold bound it.
    let mut barge = sim.world().ships.get(&PLAYER).unwrap().clone();
    barge.id = BARGE;
    barge.owner = Owner::Faction {
        faction: FactionId::from_static("faction.wharf_cooperative"),
    };
    barge.hold = Hold::default();
    let credits = sim.world().player.credits;
    setup(
        &mut sim,
        vec![
            Command::PutShip {
                id: BARGE,
                value: Some(Box::new(barge)),
            },
            Command::AdjustCredits { delta: -credits },
        ],
    );
    let free = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    let trade = ask(&sim, &request(&BARGE, &RATIONS, Side::Buy, free)).unwrap();
    assert_eq!(trade.units, free);
    assert!(
        !trade
            .commands
            .iter()
            .any(|c| matches!(c, Command::AdjustCredits { .. }))
    );
}
