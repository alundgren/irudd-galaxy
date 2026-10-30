//! Regressions for the review of PR #16, one test per finding, each reproducing the
//! reviewer's scenario.

mod common;

use common::*;
use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_core::apply::{Staged, apply};
use galaxy_economy::pulse::pulse_market;
use galaxy_economy::scout::current_prices;
use galaxy_economy::*;

fn cause(what: &str) -> Cause {
    Cause::system("test", what)
}

fn station_tally(delta: ActivityTally) -> Command {
    Command::AdjustActivity {
        scope: ActivityScope::Station { station: WHARF },
        delta,
    }
}

/// The world without its tick count, to compare "nothing changed" across a step.
fn untimed(world: &WorldState) -> WorldState {
    let mut w = world.clone();
    w.meta.tick = Tick(0);
    w
}

/// P1: an activity adjustment that overflows, or whose inverse doesn't exist, is
/// refused before anything changes, and its batch is rejected cleanly.
#[test]
fn activity_overflow_rejects_with_the_world_unchanged() {
    // `trades: i64::MIN` on an empty tally: the value fits, the inverse doesn't.
    let mut world = state().world;
    let before = world.clone();
    let min = station_tally(ActivityTally {
        trades: i64::MIN,
        ..ActivityTally::default()
    });
    assert!(apply(&mut world, &min, &mut Staged::default()).is_err());
    assert_eq!(world, before);

    // At `trades: i64::MAX`, one more trade overflows. The pulse field, checked first,
    // mustn't change either.
    setup(
        &mut world,
        [station_tally(ActivityTally {
            trades: i64::MAX,
            ..ActivityTally::default()
        })],
    );
    let before = world.clone();
    let more = station_tally(ActivityTally {
        pulses_milli: 1000,
        trades: 1,
        ..ActivityTally::default()
    });
    assert!(apply(&mut world, &more, &mut Staged::default()).is_err());
    assert_eq!(world, before);
    assert_eq!(world.economy.stations[&WHARF].pulses_milli, 0);

    // Through the sim: both batches are rejected and nothing moves.
    let mut s = state();
    s.world = before;
    let mut sim = Sim::new(s);
    let before = untimed(sim.world());
    sim.submit(CommandBatch::new(cause("min")).with(min));
    sim.submit(CommandBatch::new(cause("overflow")).with(more));
    let report = step(&mut sim);
    assert_eq!(report.applied, 0);
    assert_eq!(report.rejected.len(), 2);
    assert_eq!(untimed(sim.world()), before);
}

/// Sells (or buys) `sizes` units in turn, each as its own trade, and returns what the
/// player was paid (or paid out) and the line afterwards.
fn trade_in_parts(
    station: &StationId,
    ware: &WareId,
    side: TradeSide,
    sizes: &[u32],
) -> (i64, MarketLine) {
    let mut world = state().world;
    let docking = dock_cmds(&world, &PLAYER, station);
    setup(&mut world, docking);
    setup(
        &mut world,
        [
            Command::AdjustCargo {
                ship: PLAYER,
                ware: ware.clone(),
                delta: if side == TradeSide::Sell { 20 } else { 0 },
            },
            Command::AdjustCredits { delta: 1_000_000 },
        ],
    );
    let credits = world.player.credits;
    for &n in sizes {
        let req = TradeRequest {
            side,
            ..sell(&PLAYER, station, ware, n)
        };
        let commands = trade(&world, &req).unwrap();
        setup(&mut world, commands);
    }
    (
        (world.player.credits - credits).abs(),
        line(&world, station, ware),
    )
}

/// P2: each unit is rounded to a centicredit on its own, so how a trade is split
/// never changes what it pays.
#[test]
fn splitting_a_trade_pays_the_same() {
    use TradeSide::{Buy, Sell};
    let cases = [
        (&HOOK, &PLATE, Sell),
        (&WHARF, &ICE, Sell),
        (&HOOK, &ORE, Buy),
    ];
    for (station, ware, side) in cases {
        let (whole, line_whole) = trade_in_parts(station, ware, side, &[20]);
        let (singles, line_singles) = trade_in_parts(station, ware, side, &[1; 20]);
        let (mixed, line_mixed) = trade_in_parts(station, ware, side, &[7, 13]);
        assert_eq!(whole, singles, "{ware} at {station}: 20 vs 20 x 1");
        assert_eq!(whole, mixed, "{ware} at {station}: 20 vs 7 + 13");
        assert_eq!(line_whole, line_singles);
        assert_eq!(line_whole, line_mixed);
    }

    // The reviewer's figures: Cinder Hook plate was 941 or 942 depending on the split;
    // opening Wharf ice is 162.68 by the spec's rule.
    let w = state().world;
    let plate = quote(&w, &HOOK, &PLATE, Sell, 20).unwrap();
    assert_eq!(
        plate.total,
        plate.unit_prices.iter().map(|&p| i64::from(p)).sum::<i64>()
    );
    assert!((94_050..=94_150).contains(&plate.total), "{}", plate.total);
    assert_eq!(quote(&w, &WHARF, &ICE, Sell, 20).unwrap().total, 16_268);
}

/// The Wharf's market with `ore` iron ore in stock (no fractional carry).
fn wharf_with_ore(world: &mut WorldState, ore: u32) {
    let mut l = line(world, &WHARF, &ORE);
    l.stock = ore;
    l.stock_carry = 0.0;
    setup(
        world,
        [Command::SetMarketLine {
            station: WHARF,
            ware: ORE,
            line: Some(l),
        }],
    );
}

/// P2: the smelter stops when its ore runs out part way through a pulse, so one pulse
/// of weight 2 makes what two pulses of weight 1 make.
#[test]
fn inputs_running_out_stop_production_part_way() {
    let mut world = state().world;
    wharf_with_ore(&mut world, 3);
    let rules = world.economy.rules.pricing.clone();
    let market = world.markets.get(&WHARF).unwrap().clone();
    let plate = market.lines[&PLATE].stock;

    // The reviewer's case: 3 ore, used at 3 a pulse, lasts one pulse of the two.
    let at_once = pulse_market(&market, 2.0, &rules);
    let mut stepped = market.clone();
    for _ in 0..2 {
        stepped.lines = pulse_market(&stepped, 1.0, &rules);
    }
    assert_eq!(at_once[&PLATE].stock, plate + 2, "not 4");
    assert_eq!(at_once[&ORE].stock, 0);
    assert_eq!(at_once, stepped.lines);
    // Fractions too: four quarter pulses and one half and one 1.5.
    let mut quarters = market.clone();
    for w in [0.25, 0.25, 0.5, 1.0] {
        quarters.lines = pulse_market(&quarters, w, &rules);
    }
    assert_eq!(quarters.lines[&PLATE].stock, at_once[&PLATE].stock);
    assert!((quarters.lines[&PLATE].stock_carry - at_once[&PLATE].stock_carry).abs() < 1e-9);
}

/// A second NPC hull for docking two ships in one tick.
const SKIFF: ShipId = ShipId::from_static("craft.second_skiff");

fn add_skiff(sim: &mut Sim) {
    let mut ship = sim.world().ships.get(&BARGE).unwrap().clone();
    ship.id = SKIFF;
    ship.name = "Second skiff".into();
    sim.submit(CommandBatch::new(cause("skiff")).with(Command::PutShip {
        id: SKIFF,
        value: Some(Box::new(ship)),
    }));
    step(sim);
}

/// A sim with two NPC ships and 3 ore at the Wharf.
fn shortage_sim() -> Sim {
    let mut s = state();
    wharf_with_ore(&mut s.world, 3);
    let mut sim =
        Sim::new(s).with_systems([Box::new(EconomySystem) as Box<dyn galaxy_core::System>]);
    add_barge(&mut sim);
    add_skiff(&mut sim);
    sim
}

fn wharf(sim: &Sim, ware: &WareId) -> MarketLine {
    line(sim.world(), &WHARF, ware)
}

/// P2, through the system: two dockings in one tick, and a backlog of pending pulses,
/// make no more plate than the same activity one pulse at a time.
#[test]
fn simultaneous_and_pending_pulses_respect_inputs() {
    // One docking at a time.
    let mut apart = shortage_sim();
    let plate = wharf(&apart, &PLATE).stock;
    dock(&mut apart, &BARGE, &WHARF);
    dock(&mut apart, &SKIFF, &WHARF);

    // Both in the same tick: one pulse of weight 2.
    let mut together = shortage_sim();
    let mut both = dock_cmds(together.world(), &BARGE, &WHARF);
    both.extend(dock_cmds(together.world(), &SKIFF, &WHARF));
    together.submit(CommandBatch {
        cause: cause("two dockings"),
        commands: both,
    });
    step(&mut together);
    step(&mut together);

    for sim in [&apart, &together] {
        assert_eq!(wharf(sim, &PLATE).stock, plate + 2, "3 ore lasts one pulse");
        assert_eq!(wharf(sim, &ORE).stock, 0);
        assert_eq!(sim.world().economy.stations[&WHARF].pending_milli, 0);
    }
    assert_eq!(wharf(&apart, &PLATE), wharf(&together, &PLATE));

    // A backlog: two pulses left pending (as after lost races) apply as one.
    let mut backlog = shortage_sim();
    backlog.submit(
        CommandBatch::new(cause("backlog")).with(station_tally(ActivityTally {
            pulses_milli: 2000,
            pending_milli: 2000,
            ..ActivityTally::default()
        })),
    );
    step(&mut backlog);
    step(&mut backlog);
    assert_eq!(backlog.world().economy.stations[&WHARF].pending_milli, 0);
    assert_eq!(wharf(&backlog, &PLATE).stock, plate + 2);
    assert_eq!(wharf(&backlog, &ORE).stock, 0);
}

/// P2: a snapshot only ever shows prices the market really had, even when the pulse
/// worked out in the same tick loses its race with an earlier sale.
#[test]
fn scouting_never_saves_a_rejected_pulse() {
    let mut s = state();
    // Wharf ice at stock 150 and saturation 0.5 (the reviewer's setup), 20 ice aboard.
    let mut ice = line(&s.world, &WHARF, &ICE);
    ice.saturation = 0.5;
    setup(
        &mut s.world,
        [
            Command::SetMarketLine {
                station: WHARF,
                ware: ICE,
                line: Some(ice),
            },
            Command::AdjustCargo {
                ship: PLAYER,
                ware: ICE,
                delta: 20,
            },
        ],
    );
    let mut sim =
        Sim::new(s).with_systems([Box::new(EconomySystem) as Box<dyn galaxy_core::System>]);
    step(&mut sim); // the start station's snapshot; lines re-priced
    // The player docks: committed this tick, seen by the economy next tick.
    let cmds = dock_cmds(sim.world(), &PLAYER, &WHARF);
    sim.submit(CommandBatch {
        cause: cause("dock"),
        commands: cmds,
    });
    step(&mut sim);
    let docked = sim.world().clone();

    // Next tick: a sale submitted between ticks commits before the economy's pulse,
    // which then loses its compare-and-set on the ice line.
    sim.submit(trade_batch(&docked, &sell(&PLAYER, &WHARF, &ICE, 20)).unwrap());
    let report = step(&mut sim);
    assert!(
        report
            .rejected
            .iter()
            .any(|r| r.cause.summary == "market pulse"),
        "the pulse lost the race: {:?}",
        report.rejected
    );
    let seen = sim.world().player.price_snapshots[&WHARF].prices[&ICE];
    assert_eq!(
        seen,
        current_prices(&docked, &WHARF)[&ICE],
        "the snapshot shows the committed market, not the rejected pulse's"
    );

    // Leaving refreshes it to the market as the ship left it, after its own sale.
    undock(&mut sim, &PLAYER, Vec2::new(400.0, 0.0));
    let w = sim.world();
    assert_eq!(
        w.player.price_snapshots[&WHARF].prices[&ICE],
        line(w, &WHARF, &ICE).price
    );
}

/// P2: a trade worked out while docked is rejected, with no effects at all, if the
/// ship undocks earlier in the same tick.
#[test]
fn a_trade_after_undocking_in_the_same_tick_is_rejected() {
    let mut sim = sim();
    give(&mut sim, &PLAYER, &ICE, 20);
    let planned = trade_batch(sim.world(), &sell(&PLAYER, &WHARF, &ICE, 20)).unwrap();
    let before = sim.world().clone();
    let sales = |sim: &Sim| {
        sim.history()
            .count(&HistoryQuery::kind(EventKind::Sell.fact_kind()))
    };

    sim.submit(
        CommandBatch::new(cause("undock"))
            .with(Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            })
            .with(Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station: WHARF,
                },
            }),
    );
    sim.submit(planned);
    let report = step(&mut sim);
    assert_eq!(report.rejected.len(), 1, "{:?}", report.rejected);
    assert!(
        report.rejected[0].reason.contains("isn't docked"),
        "{}",
        report.rejected[0].reason
    );

    let w = sim.world();
    let ship = w.ships.get(&PLAYER).unwrap();
    assert_eq!(ship.docked_at, None);
    assert_eq!(ship.hold.count(&ICE), 20, "the cargo stays aboard");
    assert_eq!(w.player.credits, before.player.credits);
    assert_eq!(line(w, &WHARF, &ICE), line(&before, &WHARF, &ICE));
    assert_eq!(
        w.economy.stations.get(&WHARF),
        before.economy.stations.get(&WHARF)
    );
    assert_eq!(sales(&sim), 0);
    assert!(matches!(
        check_trade(w, &sell(&PLAYER, &WHARF, &ICE, 20)),
        Err(TradeError::NotDocked { .. })
    ));
}
