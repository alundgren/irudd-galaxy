//! Headless ship tests: flight feel, the context action, mining and rocks, docking,
//! lanes, control tiers, the order runner, the tender's patient sell, and determinism.

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState, new_game};
use galaxy_ship::controls::{self, Capability};
use galaxy_ship::{FlatDesk, Trade, TradeDesk, TradeError, TradeRequest};

const PLAYER: ShipId = ShipId::from_static("ship.player");
const THIMBLE: ShipId = ShipId::from_static("craft.thimble");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const SHALLOWS: RegionId = RegionId::from_static("region.lantern_shallows");
const CINDER: RegionId = RegionId::from_static("region.cinder_drift");
const ICE: WareId = WareId::from_static("ware.ice");
const IRON: WareId = WareId::from_static("ware.iron_ore");
const ICE_ROCK: RockId = RockId::from_static("rock.shallows.ice.03");
const IRON_ROCK: RockId = RockId::from_static("rock.shallows.iron.02");

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

fn with_desk(state: SimState, desk: Box<dyn TradeDesk>) -> Sim {
    Sim::new(state).with_systems(galaxy_ship::systems(desk))
}

fn game(seed: u64) -> Sim {
    with_desk(new_game(&data_dir(), seed).unwrap(), Box::new(FlatDesk))
}

fn hands_off() -> PlayerInput {
    PlayerInput::default()
}

fn hold_action() -> PlayerInput {
    PlayerInput {
        action: true,
        ..PlayerInput::default()
    }
}

fn thrust(x: f32, y: f32) -> PlayerInput {
    PlayerInput {
        thrust: Vec2::new(x, y),
        ..PlayerInput::default()
    }
}

fn click(target: ClickTarget, queue: bool) -> PlayerInput {
    PlayerInput {
        click: Some(Click { target, queue }),
        ..PlayerInput::default()
    }
}

fn ship(sim: &Sim, id: &ShipId) -> Ship {
    sim.world().ships.get(id).unwrap().clone()
}

fn player(sim: &Sim) -> Ship {
    ship(sim, &PLAYER)
}

fn rock(sim: &Sim, id: &RockId) -> Rock {
    sim.world().rocks.get(id).unwrap().clone()
}

fn kind_count(sim: &Sim, kind: &str) -> usize {
    sim.history()
        .count(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
}

fn step(sim: &mut Sim, input: &PlayerInput) {
    let report = sim.step(input).expect("not paused");
    assert!(report.rejected.is_empty(), "{:?}", report.rejected);
}

fn run(sim: &mut Sim, ticks: u64, input: &PlayerInput) {
    for _ in 0..ticks {
        step(sim, input);
    }
}

/// Runs hands-off until `done` holds, at most `limit` ticks.
fn run_until(sim: &mut Sim, limit: u64, mut done: impl FnMut(&Sim) -> bool) {
    for _ in 0..limit {
        if done(sim) {
            return;
        }
        step(sim, &hands_off());
    }
    assert!(done(sim), "not done after {limit} ticks");
}

fn test_cause() -> Cause {
    Cause::system("test", "test setup")
}

/// Moves a ship (undocked, stopped) to `at` in `region`, between ticks.
fn place(sim: &mut Sim, id: &ShipId, region: &RegionId, at: Vec2) {
    let s = ship(sim, id);
    sim.submit(
        CommandBatch::new(test_cause())
            .with(Command::SetShipDocked {
                ship: id.clone(),
                station: None,
            })
            .with(Command::SetShipRegion {
                ship: id.clone(),
                region: region.clone(),
            })
            .with(Command::SetShipMotion {
                ship: id.clone(),
                position: at,
                velocity: Vec2::ZERO,
                heading: s.heading,
            }),
    );
    step(sim, &hands_off());
}

/// A point just inside mining reach of a rock.
fn beside(sim: &Sim, id: &RockId) -> Vec2 {
    let r = rock(sim, id);
    r.position + Vec2::new(galaxy_ship::space::rock_radius(sim.world(), &r) + 10.0, 0.0)
}

/// Moves the player beside a rock (`extra` units further out than mining reach needs).
fn go_beside(sim: &mut Sim, id: &RockId, extra: f32) {
    let at = beside(sim, id) + Vec2::new(extra, 0.0);
    place(sim, &PLAYER, &SHALLOWS, at);
}

fn unlock(sim: &mut Sim, caps: &[Capability]) {
    let mut batch = CommandBatch::new(test_cause());
    for &cap in caps {
        batch.push(controls::set_capability(sim.world(), cap, true));
    }
    sim.submit(batch);
    step(sim, &hands_off());
}

fn submit(sim: &mut Sim, batch: CommandBatch) {
    sim.submit(batch);
    step(sim, &hands_off());
}

fn set_price(sim: &mut Sim, station: &StationId, ware: &WareId, price: u32) {
    let mut line = sim.world().markets.get(station).unwrap().lines[ware].clone();
    line.price = price;
    sim.submit(
        CommandBatch::new(test_cause()).with(Command::SetMarketLine {
            station: station.clone(),
            ware: ware.clone(),
            line: Some(line),
        }),
    );
}

// --- Flight ---

#[test]
fn releasing_input_stops_the_ship_within_a_few_ticks() {
    let mut sim = game(1);
    assert_eq!(player(&sim).docked_at, Some(WHARF));
    step(&mut sim, &thrust(1.0, 0.0)); // WASD undocks
    assert_eq!(player(&sim).docked_at, None);
    run(&mut sim, 30, &thrust(1.0, 0.0));
    let top = sim.world().hulls.get("hull.wren").unwrap().max_speed;
    assert!((player(&sim).velocity.length() - top).abs() < 0.5);

    let mut ticks = 0;
    while player(&sim).velocity != Vec2::ZERO {
        step(&mut sim, &hands_off());
        ticks += 1;
        assert!(ticks <= 4, "still moving after {ticks} ticks");
    }
    let stopped = player(&sim).position;
    run(&mut sim, 60, &hands_off());
    assert_eq!(player(&sim).position, stopped, "no drift after stopping");
}

#[test]
fn the_ship_turns_to_face_its_movement() {
    let mut sim = game(1);
    place(&mut sim, &PLAYER, &SHALLOWS, Vec2::new(0.0, -600.0));
    run(&mut sim, 30, &thrust(0.0, 1.0));
    let h = player(&sim).heading;
    assert!(
        (h - std::f32::consts::FRAC_PI_2).abs() < 0.01,
        "heading {h}"
    );
    assert!(player(&sim).velocity.y > 0.0);
}

// --- Docking ---

#[test]
fn docking_needs_the_ring() {
    let mut sim = game(1);
    let kettle = sim.world().stations.get(&KETTLE).unwrap().clone();
    place(
        &mut sim,
        &PLAYER,
        &SHALLOWS,
        kettle.position + Vec2::new(kettle.dock_radius + 20.0, 0.0),
    );
    step(&mut sim, &hold_action());
    assert_eq!(player(&sim).docked_at, None, "outside the ring");
    assert_eq!(kind_count(&sim, "dock"), 0);

    place(
        &mut sim,
        &PLAYER,
        &SHALLOWS,
        kettle.position + Vec2::new(kettle.dock_radius - 5.0, 0.0),
    );
    step(&mut sim, &hold_action());
    assert_eq!(player(&sim).docked_at, Some(KETTLE));
    assert_eq!(kind_count(&sim, "dock"), 1);
    assert_eq!(player(&sim).velocity, Vec2::ZERO, "parked");

    // Holding Space doesn't flip-flop: a press acts once.
    run(&mut sim, 10, &hold_action());
    assert_eq!(player(&sim).docked_at, Some(KETTLE));
    step(&mut sim, &hands_off());
    step(&mut sim, &hold_action());
    assert_eq!(player(&sim).docked_at, None, "Space undocks");
    assert_eq!(kind_count(&sim, "undock"), 1);
    let w = sim.world();
    assert_eq!(
        w.counters.get(
            CounterKind::Dockings,
            &CounterScope::Station { station: KETTLE }
        ),
        1
    );
}

// --- Mining ---

#[test]
fn mining_shrinks_the_rock_and_fills_the_hold_then_stops_at_capacity() {
    let mut sim = game(1);
    let start = rock(&sim, &ICE_ROCK);
    assert!(start.remaining > 20);
    go_beside(&mut sim, &ICE_ROCK, 0.0);

    run(&mut sim, 90, &hold_action());
    let mined = player(&sim).hold.count(&ICE);
    assert!(mined >= 2, "mined {mined} in 3 s");
    assert_eq!(rock(&sim, &ICE_ROCK).remaining, start.remaining - mined);
    assert!(player(&sim).activity.mining.is_some());

    run(&mut sim, 900, &hold_action());
    let cap = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    assert_eq!(player(&sim).hold.total(), u64::from(cap));
    assert_eq!(rock(&sim, &ICE_ROCK).remaining, start.remaining - cap);
    assert!(player(&sim).activity.mining.is_none(), "the run ended");

    // One `mine` fact for the whole run, with the units in it.
    let q = HistoryQuery::kind(EventKind::Mine.fact_kind());
    assert_eq!(sim.history().count(&q), 1);
    assert_eq!(sim.history().sum(&q), i64::from(cap));
    assert_eq!(
        sim.world()
            .counters
            .get(CounterKind::UnitsMined, &CounterScope::Player),
        u64::from(cap)
    );
}

#[test]
fn harder_rocks_mine_slower() {
    let mut sim = game(1);
    go_beside(&mut sim, &ICE_ROCK, 0.0);
    run(&mut sim, 300, &hold_action());
    let ice = player(&sim).hold.count(&ICE);
    step(&mut sim, &hands_off());

    let mut sim = game(1);
    go_beside(&mut sim, &IRON_ROCK, 0.0);
    run(&mut sim, 300, &hold_action());
    let iron = player(&sim).hold.count(&IRON);
    assert!(iron < ice, "iron {iron} vs ice {ice}");
    assert!(iron > 0);
}

/// Drains a rock to `left` units, between ticks.
fn drain(sim: &mut Sim, id: &RockId, left: u32) {
    let r = rock(sim, id);
    submit(
        sim,
        CommandBatch::new(test_cause()).with(Command::AdjustRock {
            rock: id.clone(),
            delta: i64::from(left) - i64::from(r.remaining),
        }),
    );
}

#[test]
fn depletion_persists_across_save_and_load_and_time_never_regrows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("depleted.sqlite");
    let mut sim = game(3);
    drain(&mut sim, &ICE_ROCK, 3);
    go_beside(&mut sim, &ICE_ROCK, 0.0);
    run(&mut sim, 200, &hold_action());
    assert_eq!(rock(&sim, &ICE_ROCK).remaining, 0);
    assert_eq!(kind_count(&sim, "rock_depleted"), 1);
    assert_eq!(player(&sim).hold.count(&ICE), 3);
    step(&mut sim, &hands_off());
    assert!(rock(&sim, &ICE_ROCK).growth.spent_at.is_some());

    Save::create(&path, sim.state()).unwrap();
    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut resumed = with_desk(loaded, Box::new(FlatDesk));
    assert_eq!(resumed.hash(), sim.hash());
    assert_eq!(rock(&resumed, &ICE_ROCK).remaining, 0);
    // Ticks alone never regrow a rock.
    run(&mut resumed, 3000, &hands_off());
    assert_eq!(rock(&resumed, &ICE_ROCK).remaining, 0);
    // Space by a spent rock can't mine it (a neighbour in reach is fair game).
    run(&mut resumed, 60, &hold_action());
    assert_eq!(rock(&resumed, &ICE_ROCK).remaining, 0);
    assert_ne!(
        player(&resumed).activity.mining.map(|m| m.rock),
        Some(ICE_ROCK)
    );
}

/// One player docking at the Wharf: a region pulse in the Shallows.
fn pulse(sim: &mut Sim) {
    submit(
        sim,
        CommandBatch::new(test_cause()).with(Command::Emit {
            event: Event::Dock {
                ship: PLAYER,
                station: WHARF,
            },
        }),
    );
}

#[test]
fn rocks_regrow_and_reform_with_activity() {
    let mut sim = game(1);
    let full = rock(&sim, &ICE_ROCK).initial;
    let field = sim
        .world()
        .fields
        .get("field.shallows.ice")
        .unwrap()
        .clone();
    let reform = field.reform.unwrap();

    // A spent rock reforms after `reform` region pulses, at a quarter of its yield.
    drain(&mut sim, &ICE_ROCK, 0);
    step(&mut sim, &hands_off());
    for _ in 0..reform - 1 {
        pulse(&mut sim);
    }
    assert_eq!(rock(&sim, &ICE_ROCK).remaining, 0);
    pulse(&mut sim);
    step(&mut sim, &hands_off());
    let share = sim.world().ship_rules.regrowth.reform_share;
    assert_eq!(
        rock(&sim, &ICE_ROCK).remaining,
        (full as f32 * share).ceil() as u32
    );
    assert_eq!(kind_count(&sim, "rock_reformed"), 1);

    // A partly mined rock regrows with pulses: here one unit per pulse per rock.
    let mut fast = field.clone();
    fast.regrow = field.count as f32;
    submit(
        &mut sim,
        CommandBatch::new(test_cause()).with(Command::PutField {
            id: fast.id.clone(),
            value: Some(Box::new(fast)),
        }),
    );
    let before = rock(&sim, &ICE_ROCK).remaining;
    run(&mut sim, 500, &hands_off());
    assert_eq!(
        rock(&sim, &ICE_ROCK).remaining,
        before,
        "no pulses, no growth"
    );
    for _ in 0..5 {
        pulse(&mut sim);
    }
    step(&mut sim, &hands_off());
    assert_eq!(rock(&sim, &ICE_ROCK).remaining, before + 5);
}

// --- Lanes ---

#[test]
fn lanes_carry_the_ship_and_count_a_jump() {
    let mut sim = game(1);
    let lane = sim
        .world()
        .lanes
        .get("lane.shallows_cinder")
        .unwrap()
        .clone();
    place(
        &mut sim,
        &PLAYER,
        &SHALLOWS,
        lane.a_gate + Vec2::new(20.0, 0.0),
    );
    step(&mut sim, &hold_action());
    assert!(player(&sim).activity.transit.is_some());
    // No steering in a lane.
    run(&mut sim, 5, &thrust(-1.0, 0.0));
    run_until(&mut sim, 600, |s| player(s).activity.transit.is_none());
    let p = player(&sim);
    assert_eq!(p.region, CINDER);
    assert_eq!(p.position, lane.b_gate);
    assert_eq!(p.velocity, Vec2::ZERO);
    assert_eq!(kind_count(&sim, "jump"), 1);
    assert_eq!(
        sim.world()
            .counters
            .get(CounterKind::Jumps, &CounterScope::Player),
        1
    );
}

// --- Control tiers ---

#[test]
fn tier_zero_ignores_clicks_and_flags_unlock_modes() {
    let mut sim = game(1);
    let home = Vec2::new(0.0, -300.0);
    place(&mut sim, &PLAYER, &SHALLOWS, home);
    assert_eq!(galaxy_ship::Controls::of(sim.world()).tier(), 0);
    let near = ClickTarget::Point {
        at: home + Vec2::new(200.0, 0.0),
    };
    step(&mut sim, &click(near.clone(), false));
    assert!(
        player(&sim).orders.orders.is_empty(),
        "tier 0 ignores clicks"
    );
    step(
        &mut sim,
        &click(ClickTarget::Rock { rock: ICE_ROCK }, false),
    );
    step(
        &mut sim,
        &click(ClickTarget::Station { station: KETTLE }, false),
    );
    step(
        &mut sim,
        &click(ClickTarget::Region { region: CINDER }, false),
    );
    assert!(player(&sim).orders.orders.is_empty());

    // Click to fly: a short hop that stops.
    unlock(&mut sim, &[Capability::ClickFly]);
    assert_eq!(galaxy_ship::Controls::of(sim.world()).tier(), 1);
    step(&mut sim, &click(near, false));
    assert_eq!(player(&sim).orders.orders.len(), 1);
    run_until(&mut sim, 600, |s| player(s).orders.orders.is_empty());
    let p = player(&sim);
    assert!(p.position.distance(home + Vec2::new(200.0, 0.0)) <= 2.5);
    assert_eq!(p.velocity, Vec2::ZERO);
    assert_eq!(kind_count(&sim, "order_done"), 1);

    // Too far for a hop, and no map travel yet.
    let far = ClickTarget::Point {
        at: home + Vec2::new(0.0, 800.0),
    };
    step(&mut sim, &click(far.clone(), false));
    assert!(player(&sim).orders.orders.is_empty());

    // Without click-to-mine a rock click is a hop; with it, a mine order.
    let rock_click = ClickTarget::Rock { rock: ICE_ROCK };
    let at = player(&sim).position;
    go_beside(&mut sim, &ICE_ROCK, 150.0);
    step(&mut sim, &click(rock_click.clone(), false));
    assert!(matches!(
        player(&sim).orders.orders[..],
        [Order::Fly { .. }]
    ));
    unlock(&mut sim, &[Capability::ClickMine, Capability::ClickDock]);
    step(&mut sim, &click(rock_click.clone(), false));
    assert!(matches!(
        player(&sim).orders.orders[..],
        [Order::Mine { .. }]
    ));

    // Shift-click replaces until queues unlock, then appends.
    let dock = ClickTarget::Station { station: KETTLE };
    step(&mut sim, &click(dock.clone(), true));
    assert!(matches!(
        player(&sim).orders.orders[..],
        [Order::Dock { .. }]
    ));
    unlock(&mut sim, &[Capability::Queues]);
    step(&mut sim, &click(rock_click, true));
    assert!(matches!(
        player(&sim).orders.orders[..],
        [Order::Dock { .. }, Order::Mine { .. }]
    ));

    // WASD takes the ship back by hand.
    step(&mut sim, &thrust(1.0, 0.0));
    assert!(player(&sim).orders.orders.is_empty());

    // Map travel: any distance, and other regions.
    place(&mut sim, &PLAYER, &SHALLOWS, at);
    unlock(&mut sim, &[Capability::MapTravel]);
    step(&mut sim, &click(far, false));
    assert_eq!(player(&sim).orders.orders.len(), 1);
    step(
        &mut sim,
        &click(ClickTarget::Region { region: CINDER }, false),
    );
    run_until(&mut sim, 3000, |s| player(s).orders.orders.is_empty());
    assert_eq!(player(&sim).region, CINDER, "routed through the lane");
    assert_eq!(kind_count(&sim, "jump"), 1);
}

// --- Orders and loops ---

/// A desk that counts sales and pays a fixed 9 credits a unit.
fn counting_desk(sales: Rc<Cell<u32>>) -> Box<dyn TradeDesk> {
    Box::new(
        move |w: &WorldState, r: &TradeRequest| -> Result<Trade, TradeError> {
            let held = w.ships.get(&r.ship).unwrap().hold.count(&r.ware);
            let units = r.quantity.min(held);
            if units == 0 {
                return Err(TradeError::NothingToSell);
            }
            sales.set(sales.get() + units);
            Ok(Trade {
                units,
                commands: vec![
                    Command::AdjustCargo {
                        ship: r.ship.clone(),
                        ware: r.ware.clone(),
                        delta: -i64::from(units),
                    },
                    Command::AdjustCredits {
                        delta: 9 * i64::from(units),
                    },
                    Command::Emit {
                        event: Event::Sell {
                            ship: r.ship.clone(),
                            station: r.station.clone(),
                            ware: r.ware.clone(),
                            quantity: units,
                            unit_price: 9,
                        },
                    },
                ],
            })
        },
    )
}

fn ice_run() -> Vec<Order> {
    vec![
        Order::Mine { rock: ICE_ROCK },
        Order::Dock { station: KETTLE },
        Order::Sell {
            ware: ICE,
            quantity: Quantity::All,
            floor: None,
        },
    ]
}

#[test]
fn the_order_runner_completes_a_saved_loop_with_a_stub_desk() {
    let sales = Rc::new(Cell::new(0));
    let mut sim = with_desk(
        new_game(&data_dir(), 1).unwrap(),
        counting_desk(sales.clone()),
    );
    unlock(&mut sim, &[Capability::Loops]);
    // The player's queue becomes a saved loop through the loop API.
    submit(
        &mut sim,
        CommandBatch::new(test_cause()).with(Command::SetOrders {
            ship: PLAYER,
            orders: OrderQueue {
                orders: ice_run(),
                ..OrderQueue::default()
            },
        }),
    );
    let save = controls::save_loop(sim.world(), "ice run").unwrap();
    let start = controls::run_loop(sim.world(), &PLAYER, "ice run");
    submit(&mut sim, save);
    assert_eq!(sim.world().player.loops["ice run"], ice_run());
    assert!(start.is_err(), "not saved yet when asked");
    let start = controls::run_loop(sim.world(), &PLAYER, "ice run").unwrap();
    submit(&mut sim, start);

    let credits = sim.world().player.credits;
    run_until(&mut sim, 20_000, |s| kind_count(s, "loop_lap") >= 2);
    let cap = sim.world().hulls.get("hull.wren").unwrap().hold_capacity;
    assert_eq!(sales.get(), 2 * cap);
    assert_eq!(sim.world().player.credits, credits + 9 * 2 * i64::from(cap));
    assert_eq!(kind_count(&sim, "sell"), 2);
    // mine, dock, sell per lap
    assert!(kind_count(&sim, "order_done") >= 6);
    assert_eq!(player(&sim).orders.progress.laps, 2);
}

#[test]
fn locked_loops_and_automation_are_refused() {
    let sim = game(1);
    assert_eq!(
        controls::save_loop(sim.world(), "x").unwrap_err(),
        galaxy_ship::ControlError::Locked(Capability::Loops)
    );
    assert_eq!(
        controls::assign_loop(sim.world(), &THIMBLE, "x", None).unwrap_err(),
        galaxy_ship::ControlError::Locked(Capability::Automation)
    );
}

#[test]
fn a_loop_that_moves_nothing_stalls_calmly() {
    let mut sim = game(1);
    submit(
        &mut sim,
        CommandBatch::new(test_cause()).with(Command::SetOrders {
            ship: PLAYER,
            orders: OrderQueue {
                orders: vec![Order::Loop {
                    orders: vec![Order::Undock {}, Order::Dock { station: WHARF }],
                    until: None,
                }],
                ..OrderQueue::default()
            },
        }),
    );
    run_until(&mut sim, 2000, |s| player(s).orders.orders.is_empty());
    assert_eq!(kind_count(&sim, "loop_stalled"), 1);
    assert_eq!(player(&sim).docked_at, Some(WHARF));
}

/// Adds the first tender, berthed at Kettle Dock.
fn add_tender(sim: &mut Sim) {
    let kettle = sim.world().stations.get(&KETTLE).unwrap().clone();
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
        hold: Hold::default(),
        orders: OrderQueue::default(),
        activity: ShipActivity::default(),
    };
    submit(
        sim,
        CommandBatch::new(test_cause()).with(Command::PutShip {
            id: THIMBLE,
            value: Some(Box::new(tender)),
        }),
    );
}

#[test]
fn a_tender_runs_a_saved_loop_and_sells_patiently() {
    let mut sim = game(5);
    add_tender(&mut sim);
    unlock(&mut sim, &[Capability::Loops, Capability::Automation]);
    submit(
        &mut sim,
        CommandBatch::new(test_cause()).with(Command::SetLoop {
            name: "ice run".into(),
            orders: Some(ice_run()),
        }),
    );
    let floor = 10;
    let assign = controls::assign_loop(sim.world(), &THIMBLE, "ice run", Some(floor)).unwrap();
    submit(&mut sim, assign);
    let Order::Loop { orders, .. } = &ship(&sim, &THIMBLE).orders.orders[0] else {
        panic!("a loop");
    };
    assert!(matches!(
        orders[2],
        Order::Sell {
            floor: Some(10),
            ..
        }
    ));
    set_price(&mut sim, &KETTLE, &ICE, 5);

    // The tender works while the player stays berthed at the Wharf.
    run_until(&mut sim, 20_000, |s| kind_count(s, "patient_wait") == 1);
    let t = ship(&sim, &THIMBLE);
    assert_eq!(t.docked_at, Some(KETTLE));
    let cap = sim.world().hulls.get("hull.tender").unwrap().hold_capacity;
    assert_eq!(t.hold.count(&ICE), cap);
    assert_eq!(player(&sim).docked_at, Some(WHARF));

    // It waits docked on the price, not on a clock: nothing sells however long.
    let credits = sim.world().player.credits;
    run(&mut sim, 3000, &hands_off());
    assert_eq!(ship(&sim, &THIMBLE).docked_at, Some(KETTLE));
    assert_eq!(kind_count(&sim, "sell"), 0);
    assert_eq!(kind_count(&sim, "patient_wait"), 1, "written once");

    // The price recovers to the floor: it sells and carries on with the loop.
    set_price(&mut sim, &KETTLE, &ICE, floor);
    run_until(&mut sim, 10, |s| kind_count(s, "patient_sold") == 1);
    assert_eq!(kind_count(&sim, "sell"), 1);
    assert_eq!(
        sim.world().player.credits,
        credits + i64::from(cap) * i64::from(floor)
    );
    assert_eq!(ship(&sim, &THIMBLE).hold.total(), 0);
    run_until(&mut sim, 200, |s| ship(s, &THIMBLE).docked_at.is_none());
    assert_eq!(kind_count(&sim, "loop_lap"), 1);
    // The tender's work counts as the player's activity.
    assert!(
        sim.world()
            .counters
            .get(CounterKind::UnitsMined, &CounterScope::Player)
            >= u64::from(cap)
    );
}

#[test]
fn a_crewed_ship_does_not_wait_for_a_price() {
    let mut sim = game(1);
    let kettle = sim.world().stations.get(&KETTLE).unwrap().position;
    place(&mut sim, &PLAYER, &SHALLOWS, kettle);
    step(&mut sim, &hold_action());
    assert_eq!(player(&sim).docked_at, Some(KETTLE));
    set_price(&mut sim, &KETTLE, &ICE, 5);
    submit(
        &mut sim,
        CommandBatch::new(test_cause())
            .with(Command::AdjustCargo {
                ship: PLAYER,
                ware: ICE,
                delta: 6,
            })
            .with(Command::SetOrders {
                ship: PLAYER,
                orders: OrderQueue {
                    orders: vec![
                        Order::Sell {
                            ware: ICE,
                            quantity: Quantity::All,
                            floor: Some(10),
                        },
                        Order::Wait {
                            until: Condition::PriceAtLeast {
                                station: KETTLE,
                                ware: ICE,
                                price: 10,
                            },
                        },
                    ],
                    ..OrderQueue::default()
                },
            }),
    );
    run_until(&mut sim, 10, |s| player(s).orders.orders.is_empty());
    assert_eq!(player(&sim).hold.count(&ICE), 6, "kept the rest");
    assert_eq!(kind_count(&sim, "patient_wait"), 0);
    assert_eq!(
        kind_count(&sim, "order_done"),
        2,
        "sell skipped, wait cleared"
    );
}

// --- Determinism ---

fn scripted(tick: u64) -> PlayerInput {
    match tick {
        0..=1 => hands_off(),
        2..=60 => thrust(0.9, 0.3),
        61..=80 => thrust(0.0, -1.0),
        81..=120 => hold_action(),
        121 => click(ClickTarget::Rock { rock: ICE_ROCK }, false),
        122..=150 => thrust(-0.5, 1.0),
        151..=900 => hold_action(),
        901 => click(ClickTarget::Station { station: KETTLE }, false),
        _ => hands_off(),
    }
}

fn scripted_run(sim: &mut Sim, until: u64) {
    while sim.tick().0 < until {
        let input = scripted(sim.tick().0);
        step(sim, &input);
    }
}

#[test]
fn same_inputs_give_the_same_hash_and_save_load_continues_the_same() {
    let setup = |sim: &mut Sim| {
        add_tender(sim);
        unlock(
            sim,
            &[
                Capability::ClickMine,
                Capability::ClickDock,
                Capability::Automation,
            ],
        );
        submit(
            sim,
            CommandBatch::new(test_cause()).with(Command::SetOrders {
                ship: THIMBLE,
                orders: OrderQueue {
                    orders: vec![Order::Loop {
                        orders: ice_run(),
                        until: None,
                    }],
                    ..OrderQueue::default()
                },
            }),
        );
    };
    let mut a = game(9);
    let mut b = game(9);
    setup(&mut a);
    setup(&mut b);
    for checkpoint in [100, 400, 1200, 2500] {
        scripted_run(&mut a, checkpoint);
        scripted_run(&mut b, checkpoint);
        assert_eq!(a.hash(), b.hash(), "diverged by tick {checkpoint}");
    }
    assert!(kind_count(&a, "mine") > 0);
    assert!(kind_count(&a, "dock") > 0);

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mid.sqlite");
    let mut c = game(9);
    setup(&mut c);
    scripted_run(&mut c, 700);
    Save::create(&path, c.state()).unwrap();
    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut d = with_desk(loaded, Box::new(FlatDesk));
    scripted_run(&mut d, 2500);
    assert_eq!(d.hash(), a.hash());
}
