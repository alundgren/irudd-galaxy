//! The journey with only the core pack installed, headless: the real composition
//! (native systems, the script host running `packs/core`, the fast-model layer on its
//! seeded fake) driven by the player's input and, once unlocked, their orders, along
//! the journey bot's route in `content/07-pacing.md`.
//!
//! One focused test per act, plus the pause and shortage-during-demo regressions.
//! Later acts use explicit fixtures with earlier beats, fittings and flags. These
//! fixtures lack completion facts; the app harness earns actual checkpoint history.
//! Long activity waits are shortened by lowering pacing tunables: pacing is data.

mod common;

use common::pilot::*;
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::Order;

const LENS: &str = "upgrade.mining_lens";

/// The first unspent rock of `field` (the authored layout doesn't depend on the seed).
fn rock_of(p: &Pilot, field: &FieldId) -> RockId {
    p.richest(field).expect("an unspent rock")
}

/// Mines `field` by hand until the hold is full.
fn fill_by_hand(p: &mut Pilot, field: &FieldId) {
    while p.hold_free() > 0 {
        let rock = rock_of(p, field);
        p.mine_by_hand(&rock);
    }
}

#[test]
fn act_one_plays_by_hand_from_undock_to_the_pause() {
    let mut p = Pilot::new(11);
    p.idle(2);
    assert_eq!(p.status("beat.b01_undock"), Some(BeatStatus::Active));
    let goal = galaxy_progress::beats::hud_goal(p.world()).unwrap();
    assert!(goal.goal.contains("Undock"), "{goal:?}");

    // b01, b02: undock, fill the hold with ice by hand.
    p.undock();
    p.idle(2);
    assert!(p.is_done("beat.b01_undock"));
    fill_by_hand(&mut p, &RIMEWATER);
    p.idle(2);
    assert!(p.is_done("beat.b02_mine_by_hand"));

    // b03: Odo takes the ice; Kettle Dock's first pane goes up.
    let credits = p.credits();
    p.dock_by_hand(&KETTLE);
    p.idle(2);
    assert!(p.is_done("beat.b03_first_delivery"));
    assert_eq!(p.cargo(&ICE), 0);
    assert_eq!(p.world().stations.get(&KETTLE).unwrap().build.stage, 1);
    assert_eq!(p.credits(), credits + 120);
    assert!(p.has_fact("core.stage_built"));
    assert!(p.heard("npc.odo_fenn", "20 ice. The beans will hear about this."));

    // b04: mine Tinroot ore, sell it at the Wharf, and buy the lens.
    assert_eq!(p.status("beat.b04_first_sale"), Some(BeatStatus::Active));
    while p.credits() < 600 {
        fill_by_hand(&mut p, &TINROOT);
        p.dock_by_hand(&WHARF);
        p.sell_all(&ORE);
    }
    assert!(p.has_fact("sell"));
    p.fit(LENS);
    p.idle(2);
    assert!(p.is_done("beat.b04_first_sale"));
    assert_eq!(
        galaxy_progress::modifiers::for_ship(p.world(), &PLAYER).mining_rate,
        1.5
    );

    // b05: open and close the PAUSED card.
    p.idle(2);
    assert_eq!(p.status("beat.b05_pause"), Some(BeatStatus::Active));
    p.pause();
    p.idle(2);
    assert!(p.is_done("beat.b05_pause"));

    for b in [
        "beat.b01_undock",
        "beat.b02_mine_by_hand",
        "beat.b03_first_delivery",
        "beat.b04_first_sale",
        "beat.b05_pause",
    ] {
        assert!(p.completed(b), "{b} completed");
    }
    for kind in [
        "undock",
        "mine",
        "dock",
        "deliver",
        "sell",
        "upgrade_fitted",
        "paused",
    ] {
        assert!(p.has_fact(kind), "{kind} written");
    }
    p.assert_healthy();
}

/// `07-pacing.md`: open and close the PAUSED card before b04 starts, buy the lens, then
/// open and close it again. b05 completes on the second; only it is newer than b04.
#[test]
fn pausing_before_b04_does_not_complete_b05_but_a_later_pause_does() {
    let mut p = Pilot::new(3);
    p.idle(2);
    p.undock();
    p.pause();
    fill_by_hand(&mut p, &RIMEWATER);
    p.dock_by_hand(&KETTLE);
    p.idle(2);
    assert!(p.is_done("beat.b03_first_delivery"));

    // The ore runs that pay for the lens, stood in for by their credits.
    p.sim.submit(
        CommandBatch::new(Cause::system("test", "a few ore runs later"))
            .with(Command::AdjustCredits { delta: 60_000 }),
    );
    p.dock_by_hand(&WHARF);
    p.fit(LENS);
    p.idle(4);
    assert!(p.is_done("beat.b04_first_sale"));
    assert_eq!(p.status("beat.b05_pause"), Some(BeatStatus::Active));
    p.idle(30);
    assert_eq!(
        p.status("beat.b05_pause"),
        Some(BeatStatus::Active),
        "the pause before b04 started doesn't count"
    );

    p.pause();
    p.idle(2);
    assert!(p.is_done("beat.b05_pause"));
    assert_eq!(p.facts("paused").len(), 2);
    p.assert_healthy();
}

/// Act 2 from b05's end: the nav chip, Ilse's plate to Cinder Hook by lane, glassite
/// to Brassmouth twice, the Hook dispute won for the Cooperative, and the rumor
/// visited at the Ember Shoals (traffic may resolve it while the player supplies the Hook).
#[test]
fn act_two_runs_the_lanes_settles_the_hook_and_visits_the_rumored_shoals() {
    let mut p = Pilot::with(21, |s| {
        s.done(&[
            "beat.b01_undock",
            "beat.b02_mine_by_hand",
            "beat.b03_first_delivery",
            "beat.b04_first_sale",
            "beat.b05_pause",
        ]);
        s.fitted(&[LENS]);
        s.counter(CounterKind::Dockings, 6);
        s.credits(4000);
    });
    p.idle(3);
    assert_eq!(p.status("beat.b06_nav_chip"), Some(BeatStatus::Active));
    p.fit("upgrade.nav_chip");
    p.idle(3);
    assert!(p.is_done("beat.b06_nav_chip"));

    // b07: Ilse loads the plate at the Wharf (the player is docked there).
    p.idle(3);
    assert_eq!(p.cargo(&PLATE), 20, "Ilse loaded the consignment");
    assert!(p.has_fact("core.consigned"));
    // By hand through the lane (no lane chart yet), then click-to-dock at the Hook.
    let lane: LaneId = "lane.shallows_cinder".parse().unwrap();
    p.ride_lane_by_hand(&lane);
    assert_eq!(p.ship().region, CINDER);
    p.go(&HOOK);
    p.idle(3);
    assert!(p.is_done("beat.b07_cinder_drift"));
    assert!(p.world().player.fittings.contains("upgrade.lane_chart"));
    assert!(p.world().player.flags.contains("cap.map_travel"));
    assert!(p.heard("npc.rook_adeyemi", "Ilse's plate."));

    // b08: glassite from the Hook to Brassmouth, twice (map travel routes the lanes).
    for _ in 0..2 {
        p.go(&HOOK);
        p.buy(&GLASSITE, 20);
        assert!(p.cargo(&GLASSITE) >= 15, "bought glassite");
        p.go(&BRASS);
        p.sell_all(&GLASSITE);
    }
    p.idle(3);
    assert!(p.is_done("beat.b08_chasing_a_gap"));

    // b09 starts on the third Hook docking: choose the Cooperative and supply plate
    // from Brassmouth in its name until the depot is won.
    p.go(&HOOK);
    p.idle(3);
    assert_eq!(p.status("beat.b09_hook_dispute"), Some(BeatStatus::Active));
    assert!(p.heard("npc.rook_adeyemi", "Two banners want my depot."));
    p.reply("npc.rook_adeyemi", "choice.side_coop");
    assert!(p.has_fact("core.side_chosen"));

    // b10 needs a third Brassmouth docking: Pip tells it then.
    let mut runs = 0;
    while !p.is_done("beat.b09_hook_dispute") {
        runs += 1;
        assert!(
            runs <= 8,
            "the depot should change hands within a few holds: tallies {:?}, plate sells {:?}, stores {:?}",
            p.facts("core.dispute_tally")
                .iter()
                .map(|f| f.amount)
                .collect::<Vec<_>>(),
            p.facts("sell")
                .iter()
                .filter(|f| f.subjects.ware.as_ref() == Some(&PLATE))
                .map(|f| (f.subjects.station.clone(), f.amount))
                .collect::<Vec<_>>(),
            p.world().packs.get("core").map(|s| s.stores.clone()),
        );
        p.go(&BRASS);
        p.buy(&PLATE, 20);
        p.go(&HOOK);
        p.sell_all(&PLATE);
        p.idle(3);
    }
    assert!(p.has_fact("core.territory_changed"));
    let hook = p.world().stations.get(&HOOK).unwrap();
    assert_eq!(
        hook.owner.as_ref().map(|o| o.as_str()),
        Some("faction.wharf_cooperative")
    );
    assert!(hook.build.parts.contains("part.banner.coop"));
    let hook_ore = &p.world().markets.get(&HOOK).unwrap().lines[&ORE];
    assert_eq!(hook_ore.base_price, 800, "miners paid fairly");

    // b10: Pip tells the rumor at Brassmouth; the player flies out to the Shoals.
    p.go(&BRASS);
    p.idle(3);
    assert!(matches!(
        p.status("beat.b10_rumor"),
        Some(BeatStatus::Active | BeatStatus::Completed)
    ));
    assert!(p.has_fact("core.rumor_heard"));
    assert!(p.heard("npc.pip_calloway", "Ember Shoals"));
    let x = p.world().progress.tunables["rumor.ember_shoals.x"]
        .value
        .as_number()
        .unwrap();
    let y = p.world().progress.tunables["rumor.ember_shoals.y"]
        .value
        .as_number()
        .unwrap();
    p.go(&GDO);
    p.go_to(Vec2::new(x as f32, y as f32));
    p.idle(3);
    assert!(p.is_done("beat.b10_rumor"));
    let followed = p.has_fact("core.rumor_followed");
    if !followed {
        assert_eq!(
            p.world()
                .rumors
                .get("rumor.ember_shoals_vein")
                .unwrap()
                .heard_at
                .len(),
            5,
            "the craft carried the ignored story to every station"
        );
    }
    let resolved = p.facts("core.rumor_resolved");
    assert_eq!(resolved.len(), 1);
    // Traffic determines the story now: the vein follows the source's fixed truth.
    let truth = p
        .world()
        .rumors
        .get("rumor.ember_shoals_vein")
        .unwrap()
        .is_true;
    assert_eq!(resolved[0].amount, Some(i64::from(truth)));
    let shoals = p
        .world()
        .rocks
        .values()
        .filter(|r| r.field.as_str() == "field.ember_shoals")
        .count();
    assert_eq!(shoals, if truth && followed { 6 } else { 3 });

    for b in [
        "beat.b06_nav_chip",
        "beat.b07_cinder_drift",
        "beat.b08_chasing_a_gap",
        "beat.b09_hook_dispute",
        "beat.b10_rumor",
    ] {
        assert!(p.completed(b), "{b} completed");
    }
    p.assert_healthy();
}

#[allow(dead_code)]
fn sell_order(ware: &WareId) -> Order {
    Order::Sell {
        ware: ware.clone(),
        quantity: ALL,
        floor: None,
    }
}

const ACT_1: [&str; 5] = [
    "beat.b01_undock",
    "beat.b02_mine_by_hand",
    "beat.b03_first_delivery",
    "beat.b04_first_sale",
    "beat.b05_pause",
];
const ACT_2: [&str; 5] = [
    "beat.b06_nav_chip",
    "beat.b07_cinder_drift",
    "beat.b08_chasing_a_gap",
    "beat.b09_hook_dispute",
    "beat.b10_rumor",
];

/// Acts 1 and 2 done, with what they earned: the fittings, the control tiers and a
/// working purse. Where Act 3 starts.
fn after_act_two(s: &mut Setup<'_>) {
    s.done(&ACT_1);
    s.done(&ACT_2);
    s.fitted(&[
        LENS,
        "upgrade.nav_chip",
        "upgrade.lane_chart",
        "upgrade.rock_cradle",
        "upgrade.docking_clamp",
    ]);
    s.flags(&[
        "cap.click_fly",
        "cap.click_mine",
        "cap.click_dock",
        "cap.map_travel",
    ]);
    s.counter(CounterKind::Dockings, 26);
    s.credits(9000);
}

fn dock_order(station: &StationId) -> Order {
    Order::Dock {
        station: station.clone(),
    }
}

fn buy_order(ware: &WareId) -> Order {
    Order::Buy {
        ware: ware.clone(),
        quantity: ALL,
    }
}

/// The Wharf plate run: plate from the Wharf, sold at Kettle Dock. Every lap moves
/// cargo and credits, so it never stalls.
fn plate_run() -> Vec<Order> {
    vec![
        dock_order(&WHARF),
        buy_order(&PLATE),
        dock_order(&KETTLE),
        sell_order(&PLATE),
    ]
}

/// Queues `orders` and saves them as the loop `name`.
fn save_loop(p: &mut Pilot, name: &str, orders: Vec<Order>) {
    p.orders(orders);
    let batch = galaxy_ship::controls::save_loop(p.world(), name).expect("the loop saves");
    p.sim.submit(batch);
    p.idle(1);
    p.orders(Vec::new());
    assert!(p.world().player.loops.contains_key(name));
}

/// Shuttles between the Wharf and Kettle Dock (counted dockings make pulses at
/// Kettle) until `done` holds.
fn shuttle_until(p: &mut Pilot, what: &str, done: impl Fn(&Pilot) -> bool) {
    for _ in 0..60 {
        if done(p) {
            return;
        }
        p.go(&WHARF);
        p.go(&KETTLE);
    }
    assert!(done(p), "shuttled too long waiting for {what}");
}

/// Builds Thimble at Kettle Dock and gives it the plate run with a floor of 40 cr.
fn thimble_runs_the_plate_run(p: &mut Pilot) {
    p.go(&KETTLE);
    p.service(ServiceRequest::Build {
        craft: "craft.thimble".parse().unwrap(),
    });
    assert!(p.world().ships.contains("craft.thimble"));
    p.idle(3);
    assert_eq!(p.step_of("beat.b13_first_tender"), Some(2));
    if !p.world().player.loops.contains_key("wharf plate run") {
        save_loop(p, "wharf plate run", plate_run());
    }
    let batch = galaxy_ship::controls::assign_loop(
        p.world(),
        &galaxy_ship::EconomyDesk,
        &"craft.thimble".parse().unwrap(),
        "wharf plate run",
        Some(4000),
    )
    .expect("a floor of 40 cr is under Kettle's ceiling");
    p.sim.submit(batch);
    p.idle(2);
    // Thimble's patient sell waits for Kettle's plate price; the player's dockings
    // and Thimble's own make the pulses that bring it back.
    shuttle_until(p, "Thimble's lap with a patient sell", |p| {
        p.is_done("beat.b13_first_tender")
    });
}

#[test]
fn act_three_queues_loops_the_tender_lesson_and_a_new_habit() {
    let mut p = Pilot::with(31, |s| {
        after_act_two(s);
        // Ringwise at 30 dockings, so a habit shows within this act.
        s.world()
            .progress
            .rules
            .traits
            .get_mut("trait.ringwise")
            .unwrap()
            .times = 30;
    });
    p.idle(3);

    // b11: the order slate, four queues for the loop log, then a loop run twice.
    assert_eq!(p.step_of("beat.b11_orders"), Some(0));
    p.fit("upgrade.order_slate");
    p.idle(3);
    assert_eq!(p.step_of("beat.b11_orders"), Some(1));
    for _ in 0..4 {
        let rock = rock_of(&p, &TINROOT);
        p.run(vec![
            Order::Mine { rock },
            dock_order(&WHARF),
            sell_order(&ORE),
        ]);
    }
    assert_eq!(
        p.facts("queue_done")
            .iter()
            .filter(|f| f.subjects.ship.as_ref() == Some(&PLAYER))
            .count(),
        4
    );
    p.fit("upgrade.loop_log");
    p.idle(3);
    assert_eq!(p.step_of("beat.b11_orders"), Some(2));
    save_loop(&mut p, "wharf plate run", plate_run());
    let run = galaxy_ship::controls::run_loop(p.world(), &PLAYER, "wharf plate run").unwrap();
    p.sim.submit(run);
    p.wait("four laps of the player's loop", |p| {
        p.facts("loop_lap").len() >= 4
    });
    p.orders(Vec::new());
    p.idle(3);
    assert!(p.is_done("beat.b11_orders"));

    // b13 step 1: Little Pail's patient sell at Kettle Dock.
    assert_eq!(p.step_of("beat.b13_first_tender"), Some(0));
    assert!(p.world().ships.contains("craft.little_pail"));
    p.go(&WHARF);
    p.go(&KETTLE);
    assert!(
        p.has_fact("core.demo_started"),
        "Kettle's ice is unsaturated: the demo starts"
    );
    shuttle_until(&mut p, "Tamsin's lesson", |p| {
        p.has_fact("core.mechanic_taught")
    });
    assert!(
        p.has_fact("patient_wait"),
        "Little Pail waited for her floor"
    );
    assert!(p.heard("npc.tamsin_oddway", "ships came and went, and she sold at"));
    p.idle(3);

    // Steps 2 and 3: Thimble, and its first lap with a patient sell.
    thimble_runs_the_plate_run(&mut p);
    assert!(p.world().player.flags.contains("cap.automation"));
    assert!(p.facts("patient_sold").iter().any(|f| {
        f.subjects
            .ship
            .as_ref()
            .is_some_and(|s| s.as_str() == "craft.thimble")
    }));

    // b14: Orrin names the habit (Ringwise), asked at the Wharf.
    assert!(p.world().player.traits.contains("trait.ringwise"));
    p.go(&WHARF);
    p.idle(3);
    assert_eq!(p.status("beat.b14_pilot_changes"), Some(BeatStatus::Active));
    p.reply("npc.orrin_vale", "choice.ask_orrin");
    p.idle(3);
    assert!(p.is_done("beat.b14_pilot_changes"));
    assert!(p.heard("npc.orrin_vale", "You don't dock any more, you arrive."));

    for b in [
        "beat.b11_orders",
        "beat.b13_first_tender",
        "beat.b14_pilot_changes",
    ] {
        assert!(p.completed(b), "{b} completed");
    }
    p.assert_healthy();
}

/// Act 2 done and b11 too: where the shortage can start. Rimewater (provisional data)
/// holds more than the spec's 240 units, so its trigger share is set where 120 mined
/// crosses it.
fn before_the_shortage(s: &mut Setup<'_>) {
    after_act_two(s);
    s.done(&["beat.b11_orders"]);
    s.tunable("shortage.rimewater_ice.trigger_left", 0.9);
}

/// Mines Shallows ice and sells it at the Wharf until the shortage starts.
fn run_rimewater_dry(p: &mut Pilot) {
    for _ in 0..12 {
        if p.status("beat.b12_ice_shortage").is_some() {
            return;
        }
        p.mine_field(&RIMEWATER);
        p.go(&WHARF);
        p.sell_all(&ICE);
        p.idle(3);
    }
    assert_eq!(
        p.status("beat.b12_ice_shortage"),
        Some(BeatStatus::Active),
        "the shortage starts once Rimewater is thin"
    );
}

fn ice_line(p: &Pilot, station: &StationId) -> MarketLine {
    p.world().markets.get(station).unwrap().lines[&ICE].clone()
}

#[test]
fn the_ice_shortage_starts_from_the_players_mining_and_way_a_ends_it() {
    let mut p = Pilot::with(41, before_the_shortage);
    let kettle_base = ice_line(&p, &KETTLE).base_price;
    run_rimewater_dry(&mut p);
    let started = p.facts("core.shortage_started");
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].amount, Some(40), "way A's quota");
    // The shock: ice costs more and is drunk faster; Kettle's lamps dim.
    assert_eq!(ice_line(&p, &KETTLE).base_price, kettle_base * 14 / 10);
    assert_eq!(ice_line(&p, &KETTLE).consumption_scale, 2.0);
    assert_eq!(ice_line(&p, &WHARF).base_price, 1400);
    let kettle = p.world().stations.get(&KETTLE).unwrap();
    assert!(kettle.build.parts.contains("part.light.dim_green"));

    // Odo offers his two ideas.
    p.go(&KETTLE);
    assert!(p.heard("npc.odo_fenn", "I've two ideas"));

    // Way A: ice from Brassmouth, sold at the shortage's stations.
    while !p.is_done("beat.b12_ice_shortage") {
        assert!(p.facts("core.relief").len() < 6, "40 units should do it");
        p.go(&BRASS);
        p.buy(&ICE, 20);
        p.go(&KETTLE);
        p.sell_all(&ICE);
        p.idle(3);
    }
    let resolved = p.facts("core.shortage_resolved");
    assert_eq!(resolved.len(), 1);
    assert_eq!(resolved[0].amount, Some(1), "way A");
    assert!(p.has_fact("core.shortage_helped"));
    assert_eq!(
        ice_line(&p, &KETTLE).base_price,
        kettle_base,
        "the base is back"
    );
    assert_eq!(ice_line(&p, &KETTLE).consumption_scale, 1.0);
    let kettle = p.world().stations.get(&KETTLE).unwrap();
    assert!(kettle.build.parts.contains("part.light.warm_green"));
    assert!(!kettle.build.parts.contains("part.light.dim_green"));
    assert!(p.facts("standing_changed").iter().any(|f| {
        f.subjects
            .other
            .as_ref()
            .is_some_and(|o| o.as_str() == "deed.shortage.ice_road")
    }));
    // Odo names it at the next Kettle docking: his line is the authored default the
    // talk script offers there (the mind may pick another eligible line instead).
    p.go(&WHARF);
    p.go(&KETTLE);
    let offered = p
        .sim
        .history()
        .facts()
        .iter()
        .rev()
        .find_map(|f| match &f.payload {
            FactPayload::Event(Event::TalkRequested { request }) if request.station == KETTLE => {
                request.candidates.first().map(|c| c.text.clone())
            }
            _ => None,
        })
        .unwrap_or_default();
    assert!(
        p.heard("npc.odo_fenn", "Ice from outside the Shallows.")
            || offered.starts_with("Ice from outside the Shallows."),
        "{offered}"
    );
    p.assert_healthy();
}

#[test]
fn letting_rimewater_rest_and_a_condenser_is_way_b() {
    let mut p = Pilot::with(42, before_the_shortage);
    run_rimewater_dry(&mut p);
    let regrow = p.world().fields.get(&RIMEWATER).unwrap().regrow;
    p.go(&KETTLE);
    p.reply("npc.odo_fenn", "choice.rimewater_rest");
    assert!(p.has_fact("core.shortage_rest"));
    assert_eq!(
        p.world().fields.get(&RIMEWATER).unwrap().regrow,
        regrow * 4.0
    );

    // 20 plate for the condenser, sold at Kettle's plate price.
    let drink = ice_line(&p, &KETTLE).consumption;
    p.go(&WHARF);
    p.buy(&PLATE, 20);
    p.go(&KETTLE);
    p.sell_all(&PLATE);
    p.idle(3);
    assert!(p.is_done("beat.b12_ice_shortage"));
    assert_eq!(
        p.facts("core.shortage_resolved")[0].amount,
        Some(2),
        "way B"
    );
    let kettle = p.world().stations.get(&KETTLE).unwrap();
    assert!(kettle.build.parts.contains("part.kettle.condenser"));
    assert_eq!(
        ice_line(&p, &KETTLE).consumption,
        drink - 2,
        "the condenser's drink"
    );

    // Rimewater was already past 80% full: the rest ends at the next docking.
    p.go(&WHARF);
    assert_eq!(p.world().fields.get(&RIMEWATER).unwrap().regrow, regrow);
    p.assert_healthy();
}

#[test]
fn an_ignored_shortage_ends_when_the_compact_brings_ice() {
    let mut p = Pilot::with(43, |s| {
        before_the_shortage(s);
        // A few Kettle pulses stand in for the forty (pacing is data).
        s.tunable("shortage.rimewater_ice.ignored_pulses", 3.0);
    });
    run_rimewater_dry(&mut p);
    let stock = ice_line(&p, &KETTLE).stock;
    shuttle_until(&mut p, "the Compact's ice", |p| {
        p.is_done("beat.b12_ice_shortage")
    });
    let resolved = p.facts("core.shortage_resolved");
    assert_eq!(resolved[0].amount, Some(0), "ignored");
    assert_eq!(
        resolved[0].subjects.faction.as_ref().map(|f| f.as_str()),
        Some("faction.tollway_compact")
    );
    assert!(
        ice_line(&p, &KETTLE).stock > stock,
        "the Compact's barge brought ice"
    );
    p.go(&WHARF);
    assert!(p.heard("npc.ilse_marrow", "The Compact brought ice in."));
    p.assert_healthy();
}

/// Act 3 done: Thimble built, the habit named. Where Act 4 starts.
fn after_act_three(s: &mut Setup<'_>) {
    after_act_two(s);
    s.done(&[
        "beat.b11_orders",
        "beat.b12_ice_shortage",
        "beat.b13_first_tender",
        "beat.b14_pilot_changes",
    ]);
    s.fitted(&["upgrade.order_slate", "upgrade.loop_log"]);
    s.flags(&["cap.queues", "cap.loops", "cap.automation"]);
    s.counter(CounterKind::Dockings, 60);
    s.credits(20_000);
    // A shorter glassite apprenticeship before the fragment (pacing is data).
    s.tunable("fragment.glassite_mined", 20.0);
}

/// The unspent Glass Drift rock with the least left: the quickest to spend.
fn smallest_glassite(p: &Pilot) -> RockId {
    p.world()
        .rocks
        .values()
        .filter(|r| r.field == GLASS_DRIFT && r.remaining > 0)
        .min_by_key(|r| (r.remaining, r.id.clone()))
        .map(|r| r.id.clone())
        .expect("a glassite rock")
}

#[test]
fn act_four_the_fragment_sings_the_hull_changes_and_ilse_hears_it_all() {
    let mut p = Pilot::with(51, after_act_three);
    p.idle(3);
    // b16 waits at the Wharf: Orrin asks the hullwright's question.
    assert_eq!(
        p.status("beat.b16_hullwrights_question"),
        Some(BeatStatus::Active)
    );
    // Orrin's question cites b14's `beat_completed`; this act starts from set-up state
    // with no such fact, so he has nothing to cite and says nothing (text is important
    // or absent). The demonstration test below plays b14, and he asks there.

    // The fragment: mine one Glass Drift rock to dust, selling at the Observatory.
    let rock = smallest_glassite(&p);
    for _ in 0..10 {
        if p.has_fact("core.fragment_found") {
            break;
        }
        if p.world().rocks.get(&rock).unwrap().remaining > 0 {
            p.run(vec![Order::Mine { rock: rock.clone() }]);
        }
        p.go(&GDO);
        p.sell_all(&GLASSITE);
        p.idle(3);
    }
    assert!(p.has_fact("core.fragment_found"));
    assert_eq!(p.cargo(&FRAGMENT), 1);

    // b15 step 1: Nell sees it (the shard node opens) and asks the player to listen.
    p.go(&HOOK);
    p.go(&GDO);
    p.idle(3);
    assert!(p.heard(
        "npc.nell_quillon",
        "You found it in a rock you mined yourself?"
    ));
    assert!(p.facts("core.shown").iter().any(|f| {
        f.subjects
            .npc
            .as_ref()
            .is_some_and(|n| n.as_str() == "npc.nell_quillon")
    }));
    assert_eq!(p.step_of("beat.b15_fragment"), Some(1));
    assert!(p.heard("npc.nell_quillon", "carry it through a lane for me"));

    // Step 2: through a lane with it aboard: it sings.
    p.go(&KETTLE);
    let opened: Vec<String> = p
        .facts("core.canon_opened")
        .iter()
        .filter_map(|f| f.subjects.other.as_ref().map(|o| o.to_string()))
        .collect();
    assert_eq!(
        opened,
        ["canon.fragment_is_lens_shard", "canon.fragments_sing"]
    );

    // Step 3: tell Nell, and leave it with the Observatory.
    let credits = p.credits();
    p.go(&GDO);
    assert!(p.heard("npc.nell_quillon", "It sang in the lane."));
    p.reply("npc.nell_quillon", "choice.fragment_leave");
    p.idle(3);
    assert!(p.is_done("beat.b15_fragment"));
    assert_eq!(p.cargo(&FRAGMENT), 0);
    assert_eq!(p.credits(), credits + 400);
    let gdo = p.world().stations.get(&GDO).unwrap();
    assert!(gdo.build.parts.contains("part.gdo.fragment_case"));

    // b16: Hearthbound prestige to the Tandem.
    p.go(&WHARF);
    p.service(ServiceRequest::Prestige {
        to: "hull.tandem".parse().unwrap(),
    });
    p.idle(3);
    assert!(p.is_done("beat.b16_hullwrights_question"));
    assert_eq!(p.ship().hull.as_str(), "hull.tandem");

    // b17: three dockings elsewhere, one outside the Shallows, then home.
    p.go(&KETTLE);
    p.go(&HOOK);
    p.go(&BRASS);
    p.go(&WHARF);
    p.idle(3);
    assert_eq!(p.status("beat.b17_coming_home"), Some(BeatStatus::Active));
    let before = p.lines_said().len();
    p.reply("npc.ilse_marrow", "choice.what_changed");
    p.idle(3);
    assert!(p.is_done("beat.b17_coming_home"));
    assert!(p.has_fact("core.asked_what_changed"));
    let answer: Vec<String> = p.lines_said()[before..]
        .iter()
        .filter(|l| l.starts_with("line.ilse_marrow"))
        .cloned()
        .collect();
    assert!(
        (2..=4).contains(&answer.len()),
        "a change or more, then the closing line: {answer:?}"
    );
    assert_eq!(answer.last().unwrap(), "line.ilse_marrow.17");
    assert!(p.heard("npc.ilse_marrow", "Welcome home, Pilot."));

    for b in [
        "beat.b15_fragment",
        "beat.b16_hullwrights_question",
        "beat.b17_coming_home",
    ] {
        assert!(p.completed(b), "{b} completed");
    }
    p.assert_healthy();
}

/// Where b13 and b12 can both start: Act 2 and b11 done, and the tender lesson open
/// straight away.
fn tender_and_shortage(s: &mut Setup<'_>) {
    after_act_two(s);
    s.done(&["beat.b11_orders"]);
    s.fitted(&["upgrade.order_slate", "upgrade.loop_log"]);
    s.flags(&["cap.queues", "cap.loops"]);
    s.tunable("b13.loop_laps", 0.0);
    s.tunable("shortage.rimewater_ice.trigger_left", 1.0);
}

/// `07-pacing.md`: start b13 step 1 with the shortage active. Little Pail must not
/// start, and Tamsin says `.12`.
#[test]
fn no_demonstration_while_the_shallows_are_short_of_ice() {
    let mut p = Pilot::with(61, |s| {
        tender_and_shortage(s);
        s.tunable("shortage.rimewater_ice.mined", 0.0);
    });
    p.idle(3);
    assert_eq!(p.status("beat.b12_ice_shortage"), Some(BeatStatus::Active));
    assert_eq!(p.step_of("beat.b13_first_tender"), Some(0));
    p.go(&KETTLE);
    assert!(
        !p.has_fact("core.demo_started"),
        "Little Pail stays berthed"
    );
    assert!(p.heard(
        "npc.tamsin_oddway",
        "Not while the Shallows are short of ice."
    ));
    let pail = p.world().ships.get("craft.little_pail").unwrap();
    assert!(pail.orders.orders.is_empty());
    p.assert_healthy();
}

/// `07-pacing.md`: start the demonstration, then the shortage starts and ends while
/// Little Pail waits with cargo aboard. Her floor is revised at the next Kettle
/// docking each time, `patient_sold` follows, and b13 and prestige still complete.
#[test]
fn the_demonstration_survives_a_shortage_and_the_journey_still_reaches_prestige() {
    let mut p = Pilot::with(62, |s| {
        tender_and_shortage(s);
        // The shortage starts once 20 Shallows ice is mined, and is ignored briefly.
        s.tunable("shortage.rimewater_ice.mined", 20.0);
        s.tunable("shortage.rimewater_ice.ignored_pulses", 3.0);
        s.world()
            .progress
            .rules
            .traits
            .get_mut("trait.ringwise")
            .unwrap()
            .times = 27;
        s.tunable("b16.dockings", 27.0);
        s.credits(20_000);
    });
    p.idle(3);
    assert_eq!(p.step_of("beat.b13_first_tender"), Some(0));
    assert!(p.status("beat.b12_ice_shortage").is_none());

    // The demonstration starts; Little Pail sells a little, then waits.
    p.go(&KETTLE);
    assert!(p.has_fact("core.demo_started"));
    p.wait("Little Pail to wait for her floor", |p| {
        p.has_fact("patient_wait")
    });
    let pail = |p: &Pilot| {
        p.world()
            .ships
            .get("craft.little_pail")
            .unwrap()
            .hold
            .count(&ICE)
    };
    assert!(pail(&p) > 0, "cargo still aboard");

    // The shortage starts (20 Shallows ice mined, then a calm docking).
    p.mine_field(&RIMEWATER);
    p.go(&WHARF);
    p.idle(3);
    assert_eq!(p.status("beat.b12_ice_shortage"), Some(BeatStatus::Active));
    if pail(&p) > 0 {
        // The market moved under Little Pail: a new mark at the next Kettle docking.
        p.go(&KETTLE);
        assert!(p.has_fact("core.demo_floor_revised"));
        assert!(p.heard("npc.tamsin_oddway", "The market moved under Little Pail."));
    }
    // Ignored, it ends when the Compact brings ice; Tamsin revises again if needed.
    shuttle_until(&mut p, "the shortage to end", |p| {
        p.is_done("beat.b12_ice_shortage")
    });
    shuttle_until(&mut p, "Tamsin's lesson", |p| {
        p.has_fact("core.mechanic_taught")
    });
    assert!(p.facts("patient_sold").iter().any(|f| {
        f.subjects
            .ship
            .as_ref()
            .is_some_and(|s| s.as_str() == "craft.little_pail")
    }));

    // b13 completes, then b14 and the prestige.
    thimble_runs_the_plate_run(&mut p);
    p.go(&WHARF);
    p.idle(3);
    p.reply("npc.orrin_vale", "choice.ask_orrin");
    p.idle(3);
    assert!(p.is_done("beat.b14_pilot_changes"));
    p.go(&KETTLE);
    p.go(&WHARF);
    p.idle(3);
    // The Heron holds 16: Orrin won't move a bigger load across.
    let heron = ServiceRequest::Prestige {
        to: "hull.heron".parse().unwrap(),
    };
    p.sell_all(&ICE);
    p.sell_all(&PLATE);
    assert!(galaxy_progress::services::check(p.world(), p.sim.history(), &heron).is_ok());
    p.service(heron);
    p.idle(3);
    assert!(
        p.is_done("beat.b16_hullwrights_question"),
        "{:?} {:?} {:?} rejected {:#?}",
        p.world()
            .progress
            .beats
            .get("beat.b16_hullwrights_question"),
        p.facts("prestige"),
        p.ship().hull,
        p.rejections
    );
    p.assert_healthy();
}

/// `galaxy pack check packs/core` with the strict typecheck: every step passes with no
/// diagnostics at all. `cargo xtask check` fetches the pinned analyzer first.
#[test]
fn the_core_pack_passes_every_check_with_the_strict_typecheck() {
    let Some(analyzer) = galaxy_script::analyzer::find(None) else {
        println!(
            "SKIPPED: luau-analyze not found; run `cargo xtask fetch-luau` (cargo xtask check does)"
        );
        return;
    };
    let p = Pilot::new(1);
    let checked = galaxy_script::check_dir(
        &common::slice().join("packs/core"),
        galaxy_script::CheckOptions {
            world: Some(p.world()),
            history: Some(p.sim.history()),
            analyzer: Some(&analyzer),
            require_typecheck: true,
            ..Default::default()
        },
    );
    let r = checked.report;
    assert!(r.accepted(), "{r:#?}");
    for s in &r.steps {
        assert!(
            s.diagnostics.is_empty(),
            "{:?}: {:#?}",
            s.step,
            s.diagnostics
        );
    }
}
