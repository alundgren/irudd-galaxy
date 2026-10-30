//! The journey with only the core pack installed, headless: the real composition
//! (native systems, the script host running `packs/core`, the fast-model layer on its
//! seeded fake) driven by the player's input and, once unlocked, their orders, along
//! the journey bot's route in `content/07-pacing.md`.
//!
//! One focused test per act, plus the route's special tests (the pause test and the
//! shortage-during-demo test). A later act starts from state set up the way `beat
//! jump` will (earlier beats done, the fittings and flags they earned), and a long
//! activity wait is shortened by lowering its pacing tunable: pacing is data.

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
    assert_eq!(galaxy_progress::modifiers::for_ship(p.world(), &PLAYER).mining_rate, 1.5);

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
    for kind in ["undock", "mine", "dock", "deliver", "sell", "upgrade_fitted", "paused"] {
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
/// followed to the Ember Shoals.
#[test]
fn act_two_runs_the_lanes_settles_the_hook_and_follows_the_rumor() {
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
            p.facts("core.dispute_tally").iter().map(|f| f.amount).collect::<Vec<_>>(),
            p.facts("sell").iter().filter(|f| f.subjects.ware.as_ref() == Some(&PLATE)).map(|f| (f.subjects.station.clone(), f.amount)).collect::<Vec<_>>(),
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
    assert_eq!(hook.owner.as_ref().map(|o| o.as_str()), Some("faction.wharf_cooperative"));
    assert!(hook.build.parts.contains("part.banner.coop"));
    let hook_ore = &p.world().markets.get(&HOOK).unwrap().lines[&ORE];
    assert_eq!(hook_ore.base_price, 800, "miners paid fairly");

    // b10: Pip tells the rumor at Brassmouth; the player flies out to the Shoals.
    p.go(&BRASS);
    p.idle(3);
    assert_eq!(p.status("beat.b10_rumor"), Some(BeatStatus::Active));
    assert!(p.has_fact("core.rumor_heard"));
    assert!(p.heard("npc.pip_calloway", "Ember Shoals"));
    let x = p.world().progress.tunables["rumor.ember_shoals.x"].value.as_number().unwrap();
    let y = p.world().progress.tunables["rumor.ember_shoals.y"].value.as_number().unwrap();
    p.go(&GDO);
    p.go_to(Vec2::new(x as f32, y as f32));
    p.idle(3);
    assert!(p.is_done("beat.b10_rumor"));
    assert!(p.has_fact("core.rumor_followed"));
    let resolved = p.facts("core.rumor_resolved");
    assert_eq!(resolved.len(), 1);
    // No NPC craft has docked at Brassmouth in this run: it's a stale tale, and the
    // Shoals hold three small iron rocks.
    assert_eq!(resolved[0].amount, Some(0));
    let shoals = p
        .world()
        .rocks
        .values()
        .filter(|r| r.field.as_str() == "field.ember_shoals")
        .count();
    assert_eq!(shoals, 3);

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
