//! The living market: the spec's worked examples, activity-driven recovery, atomic
//! trades, scouting, levers and determinism. All headless.

mod common;

use common::*;
use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_economy::scout::PriceKnowledge;
use galaxy_economy::*;

fn total(world: &WorldState, station: &StationId, ware: &WareId, side: TradeSide, n: u32) -> i64 {
    quote(world, station, ware, side, n).unwrap().total
}

/// content/07-pacing.md, "Number checks". Totals of one 20-unit hold, unit by unit.
#[test]
fn spec_worked_examples() {
    let w = state().world;
    use TradeSide::{Buy, Sell};
    // Opening prices from the market tables.
    let first = |s: &StationId, ware: &WareId, side: TradeSide| {
        quote(&w, s, ware, side, 1).unwrap().unit_prices[0]
    };
    assert!((first(&BRASS, &GLASSITE, Sell) - 50.0).abs() < 0.05);
    assert!((first(&WHARF, &ICE, Sell) - 10.3).abs() < 0.05);
    assert!((first(&HOOK, &PLATE, Sell) - 60.5).abs() < 0.05);
    assert!((first(&HOOK, &ORE, Buy) - 6.9).abs() < 0.05);

    // "Price gaps make trading worthwhile."
    assert_eq!(total(&w, &BRASS, &GLASSITE, Sell, 20), 692);
    assert_eq!(total(&w, &HOOK, &GLASSITE, Buy, 20), 422);
    assert_eq!(total(&w, &BRASS, &PLATE, Buy, 20), 617);
    assert_eq!(total(&w, &HOOK, &PLATE, Sell, 20), 941);
    assert_eq!(total(&w, &WHARF, &ORE, Sell, 20), 241);
    assert_eq!(total(&w, &KETTLE, &ICE, Sell, 20), 201);
    // "Split instead: 10 at Brassmouth, 10 at the Observatory."
    assert_eq!(total(&w, &BRASS, &GLASSITE, Sell, 10), 425);
    assert_eq!(total(&w, &OBSERVATORY, &GLASSITE, Sell, 10), 310);
    // "A Tandem's 50 glassite dumped at Brassmouth": about 1090.
    assert!((1085..=1095).contains(&total(&w, &BRASS, &GLASSITE, Sell, 50)));
}

/// "20 glassite into a fresh Brassmouth: 692. The same 20 again, one trip later
/// (saturation still 0.45): 318."
#[test]
fn a_repeated_full_load_earns_less_than_half() {
    let mut s = state();
    let w = &mut s.world;
    setup(w, dock_cmds(w, &PLAYER, &BRASS));
    setup(
        w,
        [Command::AdjustCargo {
            ship: PLAYER,
            ware: GLASSITE,
            delta: 20,
        }],
    );
    let first = plan_trade(w, &sell(&PLAYER, &BRASS, &GLASSITE, 20)).unwrap();
    assert_eq!(first.total, 692);
    assert!((first.line_after.saturation - 0.6).abs() < 1e-9);
    assert_eq!(first.line_after.stock, 130);
    let credits = w.player.credits;
    setup(w, first.commands);
    assert_eq!(w.player.credits, credits + 692);
    assert_eq!(w.ships.get(&PLAYER).unwrap().hold.count(&GLASSITE), 0);

    // The spec's state one trip later: saturation recovered to 0.45, stock as left.
    let mut later = line(w, &BRASS, &GLASSITE);
    later.saturation = 0.45;
    w.markets
        .get_mut(&BRASS)
        .unwrap()
        .lines
        .insert(GLASSITE, later);
    assert_eq!(total(w, &BRASS, &GLASSITE, TradeSide::Sell, 20), 318);
}

#[test]
fn saturation_recovers_with_activity_not_ticks() {
    let mut sim = sim();
    add_barge(&mut sim);
    dock(&mut sim, &PLAYER, &BRASS);
    give(&mut sim, &PLAYER, &GLASSITE, 20);
    let report = trade_input(&mut sim, vec![sell(&PLAYER, &BRASS, &GLASSITE, 20)]);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    let flooded = line(sim.world(), &BRASS, &GLASSITE);
    assert!((flooded.saturation - 0.6).abs() < 1e-9);
    // 110 at the start, 2 used by the pulse of the player's own docking, 20 sold.
    assert_eq!(flooded.stock, 128);
    assert_eq!(
        flooded.price, 19,
        "the cached price follows: 48 * 0.967 * 0.4"
    );

    // A long stretch with nothing happening: nothing recovers.
    for _ in 0..3000 {
        step(&mut sim);
    }
    assert_eq!(line(sim.world(), &BRASS, &GLASSITE), flooded);

    // An NPC barge docks: one station pulse. Saturation falls by sat_recover, and the
    // station's consumers use 2 glassite.
    dock(&mut sim, &BARGE, &BRASS);
    let after = line(sim.world(), &BRASS, &GLASSITE);
    assert!(
        (after.saturation - 0.55).abs() < 1e-9,
        "{}",
        after.saturation
    );
    assert_eq!(after.stock, 126);

    // Undocking and docking again right away doesn't count.
    undock(&mut sim, &BARGE, Vec2::new(50.0, 0.0));
    dock(&mut sim, &BARGE, &BRASS);
    assert_eq!(line(sim.world(), &BRASS, &GLASSITE), after);
    // After flying 300 u away it does.
    undock(&mut sim, &BARGE, Vec2::new(400.0, 0.0));
    step(&mut sim);
    dock(&mut sim, &BARGE, &BRASS);
    let again = line(sim.world(), &BRASS, &GLASSITE);
    assert!(
        (again.saturation - 0.5).abs() < 1e-9,
        "{}",
        again.saturation
    );

    // A lane arrival into the Narrows is half a pulse at Brassmouth.
    undock(&mut sim, &BARGE, Vec2::new(400.0, 0.0));
    jump(&mut sim, &BARGE, &NARROWS);
    let arrived = line(sim.world(), &BRASS, &GLASSITE);
    assert!(
        (arrived.saturation - 0.475).abs() < 1e-9,
        "{}",
        arrived.saturation
    );

    let eco = &sim.world().economy;
    // The player's docking, two counted barge dockings, half a pulse on arrival.
    assert_eq!(eco.stations[&BRASS].pulses_milli, 3500);
    assert_eq!(eco.stations[&BRASS].pending_milli, 0);
    assert_eq!(eco.regions[&NARROWS].pulses_milli, 4000);
}

#[test]
fn producers_need_their_inputs_and_stop_at_the_cap() {
    let mut sim = sim();
    add_barge(&mut sim);
    let cap = |sim: &Sim| line(sim.world(), &BRASS, &PLATE).stock;
    let start = cap(&sim);
    dock(&mut sim, &BARGE, &BRASS);
    assert_eq!(cap(&sim), start + 2, "the foundry makes 2 plate from ore");

    // No ore: no plate.
    let mut empty = line(sim.world(), &BRASS, &ORE);
    empty.stock = 0;
    sim.submit(CommandBatch::new(Cause::system("test", "drain ore")).with(
        Command::SetMarketLine {
            station: BRASS,
            ware: ORE,
            line: Some(empty),
        },
    ));
    step(&mut sim);
    undock(&mut sim, &BARGE, Vec2::new(500.0, 0.0));
    dock(&mut sim, &BARGE, &BRASS);
    assert_eq!(cap(&sim), start + 2);
    let ore = line(sim.world(), &BRASS, &ORE);
    assert_eq!(ore.price, ore.max_price, "an empty buyer pays its ceiling");

    // Producers stop at target * produce_cap (2 x 150 = 300 plate).
    let rules = &sim.world().economy.rules.pricing;
    let full = MarketLine {
        stock: 299,
        ..line(sim.world(), &BRASS, &PLATE)
    };
    assert_eq!(
        galaxy_economy::pulse::pulse_line(&full, 1.0, true, rules).stock,
        300
    );
    let over = MarketLine { stock: 340, ..full };
    assert_eq!(
        galaxy_economy::pulse::pulse_line(&over, 1.0, true, rules).stock,
        340
    );
}

#[test]
fn trades_are_atomic_and_rejected_cleanly() {
    let mut s = state();
    let w = &mut s.world;
    let at_wharf = sell(&PLAYER, &WHARF, &ICE, 5);
    // Docked at the Wharf with an empty hold and 40 credits.
    assert_eq!(
        check_trade(w, &at_wharf),
        Err(TradeError::NotEnoughCargo {
            ware: ICE,
            have: 0,
            want: 5
        })
    );
    assert!(matches!(
        check_trade(w, &sell(&PLAYER, &BRASS, &ICE, 1)),
        Err(TradeError::NotDocked { .. })
    ));
    assert!(matches!(
        check_trade(w, &sell(&PLAYER, &WHARF, &PLATE, 1)),
        Err(TradeError::WrongSide { .. })
    ));
    assert!(matches!(
        check_trade(w, &sell(&PLAYER, &WHARF, &FRAGMENT, 1)),
        Err(TradeError::NotTradeable { .. })
    ));
    assert_eq!(
        check_trade(w, &sell(&PLAYER, &WHARF, &ICE, 0)),
        Err(TradeError::ZeroQuantity)
    );
    assert!(matches!(
        check_trade(w, &buy(&PLAYER, &WHARF, &PLATE, 5)),
        Err(TradeError::NotEnoughCredits { have: 40, .. })
    ));
    assert!(matches!(
        check_trade(w, &buy(&PLAYER, &WHARF, &PLATE, 21)),
        Err(TradeError::HoldFull { free: 20, want: 21 })
    ));
    assert_eq!(
        quote(w, &WHARF, &RATIONS, TradeSide::Buy, 1)
            .unwrap()
            .units(),
        1
    );
    setup(w, [Command::AdjustCredits { delta: 10_000 }]);
    assert!(check_trade(w, &buy(&PLAYER, &WHARF, &PLATE, 20)).is_ok());

    // A price limit fills only the units at or above it.
    setup(
        w,
        [Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 20,
        }],
    );
    let limited = TradeRequest {
        limit: Some(10),
        ..sell(&PLAYER, &WHARF, &ICE, 20)
    };
    let o = plan_trade(w, &limited).unwrap();
    assert!(o.filled > 0 && o.filled < 20, "{}", o.filled);
    assert!(o.unit_prices.iter().all(|p| *p >= 10.0));
    let too_high = TradeRequest {
        limit: Some(99),
        ..limited.clone()
    };
    assert!(matches!(
        check_trade(w, &too_high),
        Err(TradeError::BeyondLimit { limit: 99, .. })
    ));

    // Two trades worked out from the same state: the second loses the race on the
    // market line and its whole batch is rejected, cargo and credits included.
    let mut sim = Sim::new(s);
    let w = sim.world().clone();
    let a = trade_batch(&w, &sell(&PLAYER, &WHARF, &ICE, 10)).unwrap();
    let b = trade_batch(&w, &sell(&PLAYER, &WHARF, &ICE, 10)).unwrap();
    sim.submit(a);
    sim.submit(b);
    let report = step(&mut sim);
    assert_eq!(report.applied, 1);
    assert_eq!(report.rejected.len(), 1);
    assert!(
        report.rejected[0].reason.contains("changed"),
        "{}",
        report.rejected[0].reason
    );
    let after = sim.world();
    assert_eq!(after.ships.get(&PLAYER).unwrap().hold.count(&ICE), 10);
    assert_eq!(line(after, &WHARF, &ICE).stock, 160);
    let sales = sim
        .history()
        .count(&HistoryQuery::kind(EventKind::Sell.fact_kind()));
    assert_eq!(sales, 1, "the rejected sale never happened");

    // An invalid request from the dock screen changes nothing.
    let mut sim = common::sim();
    step(&mut sim);
    let before = sim.world().clone();
    trade_input(&mut sim, vec![sell(&PLAYER, &WHARF, &ICE, 3)]);
    let mut now = sim.world().clone();
    now.meta.tick = before.meta.tick;
    assert_eq!(now, before);
}

#[test]
fn dock_screen_sells_and_buys_in_order() {
    let mut sim = sim();
    give(&mut sim, &PLAYER, &ORE, 20);
    let credits = sim.world().player.credits;
    // Sell the ore, then spend the takings on plate in the same tick.
    let report = trade_input(
        &mut sim,
        vec![
            sell(&PLAYER, &WHARF, &ORE, 20),
            buy(&PLAYER, &WHARF, &PLATE, 5),
        ],
    );
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    let w = sim.world();
    let ship = w.ships.get(&PLAYER).unwrap();
    assert_eq!(ship.hold.count(&ORE), 0);
    assert_eq!(ship.hold.count(&PLATE), 5);
    let earned = 241;
    let spent = w.economy.stations[&WHARF].credits_spent;
    assert_eq!(w.player.credits, credits + earned - spent);
    assert_eq!(w.economy.regions[&SHALLOWS].units_sold, 20);
    assert_eq!(w.economy.regions[&SHALLOWS].units_bought, 5);
    let sold = sim
        .history()
        .last(&HistoryQuery::kind(EventKind::Sell.fact_kind()))
        .unwrap();
    assert_eq!(sold.amount, Some(20));
    assert_eq!(w.counters.get(CounterKind::Sales, &CounterScope::Player), 1);
}

#[test]
fn flooding_a_market_is_a_fact() {
    let mut sim = sim();
    dock(&mut sim, &PLAYER, &OBSERVATORY);
    give(&mut sim, &PLAYER, &GLASSITE, 15);
    trade_input(&mut sim, vec![sell(&PLAYER, &OBSERVATORY, &GLASSITE, 15)]);
    let flood = sim
        .history()
        .last(&HistoryQuery::kind(galaxy_economy::trade::MARKET_FLOODED))
        .expect("0.6 saturation is a flood");
    assert_eq!(flood.subjects.station, Some(OBSERVATORY));
}

#[test]
fn snapshots_appear_on_dock_and_remote_prices_start_unknown() {
    let mut sim = sim();
    // Starting docked at the Wharf: it's live, everything else is "?".
    let map = MapView::build(sim.world());
    assert!(matches!(
        map.station(&WHARF).unwrap().prices,
        PriceKnowledge::Live { .. }
    ));
    assert_eq!(map.unknown_stations().len(), 4);
    assert!(sim.world().player.price_snapshots.is_empty());

    step(&mut sim); // the start station counts as visited
    assert!(sim.world().player.price_snapshots.contains_key(&WHARF));
    undock(&mut sim, &PLAYER, Vec2::new(400.0, 0.0));
    let map = MapView::build(sim.world());
    let PriceKnowledge::Seen { label, prices, .. } = &map.station(&WHARF).unwrap().prices else {
        panic!("seen after leaving");
    };
    assert_eq!(label, "as of your last visit");
    assert!(
        prices
            .iter()
            .any(|p| p.ware == ICE && p.role == MarketRole::Buys && p.price == 10)
    );
    assert_eq!(
        map.station(&KETTLE).unwrap().prices,
        PriceKnowledge::Unknown
    );

    // Dock at Kettle: its snapshot is taken, dated by the visit.
    dock(&mut sim, &PLAYER, &KETTLE);
    let w = sim.world();
    let snap = &w.player.price_snapshots[&KETTLE];
    assert_eq!(snap.visit, 1);
    assert_eq!(snap.prices[&ICE], 14, "12 * 1.125 = 13.5 rounds to 14");
    assert_eq!(
        sim.history()
            .count(&HistoryQuery::kind(EventKind::Scout.fact_kind())),
        2
    );
    let map = MapView::build(w);
    let PriceKnowledge::Seen { label, .. } = &map.station(&WHARF).unwrap().prices else {
        panic!();
    };
    assert_eq!(label, "seen 1 docking ago");
    assert!(matches!(
        map.station(&KETTLE).unwrap().prices,
        PriceKnowledge::Live { .. }
    ));
    assert_eq!(map.station(&BRASS).unwrap().prices, PriceKnowledge::Unknown);
    assert_eq!(describe_age(3), "seen 3 dockings ago");

    // The Heron's long glass: a lane arrival scouts the whole region, and the region
    // is live while it's there.
    let mut heron = sim.world().ships.get(&PLAYER).unwrap().clone();
    heron.hull = HullId::new("hull.heron").unwrap();
    sim.submit(
        CommandBatch::new(Cause::system("test", "prestige")).with(Command::PutShip {
            id: PLAYER,
            value: Some(Box::new(heron)),
        }),
    );
    undock(&mut sim, &PLAYER, Vec2::new(400.0, 0.0));
    jump(&mut sim, &PLAYER, &CINDER);
    let w = sim.world();
    assert!(w.player.price_snapshots.contains_key(&HOOK));
    assert!(w.player.price_snapshots.contains_key(&OBSERVATORY));
    assert!(!w.player.price_snapshots.contains_key(&BRASS));
    let map = MapView::build(w);
    assert!(matches!(
        map.station(&HOOK).unwrap().prices,
        PriceKnowledge::Live { .. }
    ));
    assert!(matches!(
        map.station(&WHARF).unwrap().prices,
        PriceKnowledge::Seen { .. }
    ));
}

#[test]
fn price_glass_flag_widens_reach() {
    let mut s = state();
    let w = &mut s.world;
    setup(
        w,
        [
            Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            },
            Command::SetFlag {
                flag: FlagId::new("flag.price_glass").unwrap(),
                on: true,
            },
        ],
    );
    let live = galaxy_economy::scout::live_stations(w);
    assert!(live.contains(&WHARF), "within 1200 u");
    assert!(live.contains(&KETTLE));
    assert!(!live.contains(&HOOK));
}

#[test]
fn levers_change_supply_not_the_formula() {
    let mut sim = sim();
    add_barge(&mut sim);
    step(&mut sim);
    let w = sim.world().clone();
    let before = line(&w, &KETTLE, &ICE);
    let pack = PackId::new("gm").unwrap();
    let cause = || Cause::script(pack.clone(), "r1", "the ice shortage");

    // A shortage: Kettle drinks twice the ice, and the greenhouse opens (stage 2).
    let batch = lever_batch(
        &w,
        cause(),
        &[
            Lever::ScaleConsumers {
                station: KETTLE,
                ware: ICE,
                factor: 2.0,
            },
            Lever::AddConsumers {
                station: KETTLE,
                ware: ICE,
                per_pulse: 1,
            },
            Lever::AddLine {
                station: KETTLE,
                ware: RATIONS,
                line: LineSpec {
                    role: MarketRole::Sells,
                    base_price: 15,
                    target: 120,
                    stock: 0,
                    production: 2,
                    consumption: 0,
                    sat_per_unit: 0.0,
                    sat_recover: 0.0,
                    needs: Default::default(),
                },
            },
        ],
    )
    .unwrap();
    sim.submit(batch);
    step(&mut sim);
    step(&mut sim);
    let w = sim.world();
    let levered = line(w, &KETTLE, &ICE);
    // Rates changed; stock, saturation and price did not.
    assert_eq!((levered.consumption, levered.consumption_scale), (4, 2.0));
    assert_eq!(
        (levered.stock, levered.saturation, levered.price),
        (before.stock, before.saturation, before.price)
    );
    // "Rations asks up to 22.4 while empty."
    let empty = quote(w, &KETTLE, &RATIONS, TradeSide::Buy, 1).unwrap();
    assert_eq!(empty.units(), 0, "nothing to buy yet");
    let rl = line(w, &KETTLE, &RATIONS);
    assert_eq!(rl.price, 22);
    assert!(
        (galaxy_economy::pricing::unit_price(&rl, &w.economy.rules.pricing, 1.15) - 22.425).abs()
            < 1e-9
    );

    // A pulse at Kettle now uses 8 ice (4 per pulse, doubled) and makes 2 rations.
    dock(&mut sim, &BARGE, &KETTLE);
    let w = sim.world();
    assert_eq!(line(w, &KETTLE, &ICE).stock, before.stock - 8);
    assert_eq!(line(w, &KETTLE, &RATIONS).stock, 2);

    // Rolling the script back restores the rates and removes the line, but keeps
    // what activity did to stock since.
    let causes = sim.causes_where(|c| matches!(&c.cause.kind, CauseKind::Script { .. }));
    sim.queue_rollback(&causes, "the shortage was a mistake");
    let report = step(&mut sim);
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
    let w = sim.world();
    let restored = line(w, &KETTLE, &ICE);
    assert_eq!((restored.consumption, restored.consumption_scale), (3, 1.0));
    assert_eq!(restored.stock, before.stock - 8);
    assert!(!w.markets.get(&KETTLE).unwrap().lines.contains_key(&RATIONS));

    // Bad levers are refused.
    assert!(matches!(
        lever_commands(
            w,
            &Lever::AddProducers {
                station: KETTLE,
                ware: ICE,
                per_pulse: -1
            }
        ),
        Err(LeverError::NegativeRate(-1))
    ));
    assert!(matches!(
        lever_commands(
            w,
            &Lever::ScaleProducers {
                station: KETTLE,
                ware: GLASSITE,
                factor: 2.0
            }
        ),
        Err(LeverError::NoLine { .. })
    ));
}

#[test]
fn telemetry_shows_floods_and_volume() {
    let mut sim = sim();
    dock(&mut sim, &PLAYER, &BRASS);
    give(&mut sim, &PLAYER, &GLASSITE, 20);
    let expected = quote(sim.world(), &BRASS, &GLASSITE, TradeSide::Sell, 20).unwrap();
    trade_input(&mut sim, vec![sell(&PLAYER, &BRASS, &GLASSITE, 20)]);
    let t = EconomyTelemetry::build(sim.world());
    let narrows = t.region(&NARROWS).unwrap();
    assert_eq!(narrows.activity.units_sold, 20);
    assert_eq!(narrows.activity.credits_earned, expected.total);
    assert_eq!(narrows.activity.trades, 1);
    assert!((narrows.saturation_max - 0.6).abs() < 1e-9);
    let glass = narrows.stations[0]
        .lines
        .iter()
        .find(|l| l.ware == GLASSITE)
        .unwrap();
    assert!(glass.price_ratio < 0.4, "{}", glass.price_ratio);
    // Telemetry is plain serde.
    let json = serde_json::to_string(&t).unwrap();
    let back: EconomyTelemetry = serde_json::from_str(&json).unwrap();
    assert_eq!(back, t);
}

#[test]
fn territory_follows_station_owners() {
    let mut s = state();
    let w = &mut s.world;
    assert!(matches!(
        galaxy_economy::map::control_of(w, &NARROWS),
        Control::Held { .. }
    ));
    let map = MapView::build(w);
    let before = map.borders.len();
    // Cinder Hook goes to the Compact: Cinder Drift becomes contested between it and
    // the Lamplighters, and the Compact's station list grows.
    setup(
        w,
        [Command::SetStationOwner {
            station: HOOK,
            owner: Some(COMPACT),
        }],
    );
    let map = MapView::build(w);
    let compact = map.factions.iter().find(|f| f.id == COMPACT).unwrap();
    assert!(compact.stations.contains(&HOOK));
    assert!(matches!(
        map.region(&CINDER).unwrap().control,
        Control::Contested { ref factions } if factions.contains(&COMPACT)
    ));
    assert!(map.borders.len() >= before.min(1));
    // The map is plain serde too.
    serde_json::to_string(&map).unwrap();
}

#[test]
fn npc_trade_choice_ranks_complete_plans() {
    let mut sim = sim();
    add_barge(&mut sim);
    dock(&mut sim, &BARGE, &HOOK);
    let profile = TraderProfile {
        ship: BARGE,
        stations: [HOOK, BRASS].into(),
        wares: [ORE, PLATE, GLASSITE, RATIONS].into(),
        max_units_per_ware: 10,
        max_wares_per_docking: 2,
        cost_per_distance: 0.0,
    };
    let plans = rank_trades(sim.world(), &profile).unwrap();
    assert!(plans.len() >= 3);
    assert!(plans.windows(2).all(|p| p[0].score >= p[1].score));
    // The best haul from Cinder Hook to Brassmouth is glassite (about 30 a unit).
    let top = &plans[0];
    assert_eq!(top.destination, Some(BRASS));
    assert_eq!(top.trades.len(), 1);
    assert_eq!(top.trades[0].ware, GLASSITE);
    assert_eq!(top.trades[0].side, TradeSide::Buy);
    assert_eq!(top.trades[0].quantity, 10);
    // 10 bought for about 206, sold into a fresh Brassmouth for about 425.
    assert!(top.expected_credits > 200, "{}", top.summary);
    // The plan is executable as it stands.
    for req in &top.trades {
        check_trade(sim.world(), req).unwrap();
    }
    let menu = top_choices(sim.world(), &profile, 3).unwrap();
    assert_eq!(menu[0], *top);

    // With glassite aboard at Brassmouth, the top plan sells it here.
    for req in top.trades.clone() {
        let b = trade_batch(sim.world(), &req).unwrap();
        sim.submit(b);
        step(&mut sim);
    }
    undock(&mut sim, &BARGE, Vec2::new(500.0, 0.0));
    dock(&mut sim, &BARGE, &BRASS);
    let plans = rank_trades(sim.world(), &profile).unwrap();
    let sells: Vec<_> = plans[0]
        .trades
        .iter()
        .filter(|t| t.side == TradeSide::Sell)
        .collect();
    assert_eq!(sells.len(), 1, "{}", plans[0].summary);
    assert_eq!(sells[0].ware, GLASSITE);
}

/// A scripted session: dockings, trades, an NPC, a lever.
fn session(sim: &mut Sim) {
    add_barge(sim);
    dock(sim, &PLAYER, &KETTLE);
    give(sim, &PLAYER, &ICE, 20);
    trade_input(sim, vec![sell(&PLAYER, &KETTLE, &ICE, 20)]);
    dock(sim, &BARGE, &BRASS);
    undock(sim, &PLAYER, Vec2::new(0.0, 500.0));
    dock(sim, &PLAYER, &WHARF);
    trade_input(sim, vec![buy(&PLAYER, &WHARF, &RATIONS, 3)]);
    for _ in 0..50 {
        step(sim);
    }
    jump(sim, &BARGE, &CINDER);
    dock(sim, &BARGE, &HOOK);
}

#[test]
fn same_inputs_give_the_same_hash() {
    let mut a = common::sim();
    let mut b = common::sim();
    session(&mut a);
    session(&mut b);
    assert_eq!(a.hash(), b.hash());
    assert!(a.world().economy.regions.len() >= 2);
    assert!(
        a.history()
            .count(&HistoryQuery::kind(EventKind::Sell.fact_kind()))
            == 1
    );

    // Save, load, continue: the same as never stopping.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("eco.sqlite");
    galaxy_core::Save::create(&path, a.state()).unwrap();
    let loaded = galaxy_core::Save::open(&path).unwrap().load().unwrap();
    let mut c =
        Sim::new(loaded).with_systems([Box::new(EconomySystem) as Box<dyn galaxy_core::System>]);
    assert_eq!(c.hash(), a.hash());
    dock(&mut a, &PLAYER, &KETTLE);
    dock(&mut c, &PLAYER, &KETTLE);
    assert_eq!(c.hash(), a.hash());
}
