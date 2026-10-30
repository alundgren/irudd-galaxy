//! Regressions for the journey review (PR #26): each plays the failure the reviewer
//! found through the real systems and `packs/core`, and checks the journey goes on.
//! Headless, like `journey_core.rs`.

mod common;

use common::pilot::*;
use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;
use galaxy_contract::verbs::{Order, OrderQueue, Quantity};
use galaxy_economy::{Lever, lever_batch};

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

/// Act 1 and the given Act 2 beats done, with the fittings and control tiers they earn.
fn into_act_two(s: &mut Setup<'_>, done: &[&str]) {
    s.done(&ACT_1);
    s.done(done);
    s.fitted(&[
        "upgrade.mining_lens",
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

/// Review finding 2: a Glass Drift mined out before b13 must come back with activity,
/// or the fragment (and b15) can never be found. The field regrows 0.15 a region
/// pulse (`01-galaxy.md`); a spent rock reforms, and spending it after b13 finds the
/// fragment.
#[test]
fn a_glass_drift_mined_out_before_b13_regrows_and_still_gives_the_fragment() {
    let mut p = Pilot::with(71, |s| {
        into_act_two(s, &ACT_2);
        s.done(&[
            "beat.b11_orders",
            "beat.b12_ice_shortage",
            "beat.b13_first_tender",
        ]);
        s.tunable("fragment.glassite_mined", 1.0);
        // Every Glass Drift rock is dust: mined out before b13 was done.
        let w = s.world();
        for (_, r) in w.rocks.iter_mut().filter(|(_, r)| r.field == GLASS_DRIFT) {
            r.remaining = 0;
        }
        // Since then, Cinder Drift has been busy: dockings and lane arrivals are its
        // region pulses (ship_rules.toml), and regrowth spends them.
        let cinder = CounterScope::Region { region: CINDER };
        w.counters.bump(CounterKind::Dockings, &cinder, 1200);
    });
    assert_eq!(p.world().fields.get(&GLASS_DRIFT).unwrap().regrow, 0.15);
    p.idle(3);
    assert!(
        p.facts("rock_reformed").iter().any(|f| f
            .subjects
            .rock
            .as_ref()
            .is_some_and(|r| r.as_str().starts_with("rock.cinder.glassite."))),
        "a spent glassite rock reformed"
    );
    let rock = p
        .world()
        .rocks
        .values()
        .filter(|r| r.field == GLASS_DRIFT && r.remaining > 0)
        .min_by_key(|r| (r.remaining, r.id.clone()))
        .map(|r| r.id.clone())
        .expect("a reformed rock");

    // Spend it (selling at the Observatory as the hold fills): the fragment glints.
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
    p.idle(3);
    assert_eq!(p.status("beat.b15_fragment"), Some(BeatStatus::Active));
    p.assert_healthy();
}

/// Review finding 3: b08 counts the Brassmouth dockings with a glassite sale. With a
/// long history of visits since b08 started (more than any one history page), the two
/// visits that sell still count.
#[test]
fn b08_counts_late_glassite_dockings_after_a_long_brassmouth_history() {
    let mut p = Pilot::with(72, |s| {
        into_act_two(s, &["beat.b06_nav_chip", "beat.b07_cinder_drift"]);
    });
    p.idle(3);
    assert_eq!(p.status("beat.b08_chasing_a_gap"), Some(BeatStatus::Active));
    // 220 Brassmouth visits since b08 started, none with a glassite sale.
    let mut batch = CommandBatch::new(Cause::system("test", "many Brassmouth visits"));
    for _ in 0..220 {
        batch.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: FactKind::from_static("dock"),
                subjects: Subjects {
                    ship: Some(PLAYER),
                    station: Some(BRASS),
                    ..Subjects::default()
                },
                amount: None,
                note: "an earlier Brassmouth visit".into(),
            }),
        });
    }
    p.sim.submit(batch);
    p.idle(2);
    // Then glassite from the Hook to Brassmouth, twice.
    for _ in 0..2 {
        p.go(&HOOK);
        p.buy(&GLASSITE, 20);
        assert!(p.cargo(&GLASSITE) >= 15, "bought glassite");
        p.go(&BRASS);
        p.sell_all(&GLASSITE);
    }
    p.idle(3);
    assert!(p.is_done("beat.b08_chasing_a_gap"));
    p.assert_healthy();
}

/// Where b13 starts with no shortage in sight: Act 2 and b11 done.
fn tender_lesson(s: &mut Setup<'_>) {
    into_act_two(s, &ACT_2);
    s.done(&["beat.b11_orders"]);
    s.fitted(&["upgrade.order_slate", "upgrade.loop_log"]);
    s.flags(&["cap.queues", "cap.loops"]);
    s.tunable("b13.loop_laps", 0.0);
    // The Shallows never run short here.
    s.tunable("shortage.rimewater_ice.mined", 1.0e9);
}

/// Review finding 4: a lever that moves Kettle's ice base (here a game master's,
/// outside the shortage) leaves Little Pail's floor above the new price band. Tamsin
/// revises her own tender's floor at the next Kettle docking, citing the change, and
/// the demonstration finishes.
#[test]
fn little_pail_gets_a_new_floor_when_any_lever_moves_kettles_ice_base() {
    let mut p = Pilot::with(73, tender_lesson);
    p.idle(3);
    assert_eq!(p.step_of("beat.b13_first_tender"), Some(0));
    p.go(&KETTLE);
    assert!(p.has_fact("core.demo_started"));
    p.wait("Little Pail to wait for her floor", |p| {
        p.has_fact("patient_wait")
    });
    let floor = |p: &Pilot| -> Option<u32> {
        let pail = p.world().ships.get("craft.little_pail")?;
        pail.orders.orders.iter().find_map(|o| match o {
            Order::Sell { floor, .. } => *floor,
            _ => None,
        })
    };
    let before = floor(&p).expect("Little Pail sells with a floor");

    // Kettle's ice base drops to 1 cr: no fact of the shortage's.
    let batch = lever_batch(
        p.world(),
        Cause::system("test", "the game master floods Kettle with ice"),
        &[Lever::SetBase {
            station: KETTLE,
            ware: ICE,
            base_price: 100,
        }],
    )
    .unwrap();
    p.sim.submit(batch);
    p.idle(2);
    let changed = p
        .facts("market_base_changed")
        .last()
        .map(|f| f.id)
        .expect("the base change is a fact");
    let max = |p: &Pilot| {
        p.world()
            .markets
            .get(&KETTLE)
            .unwrap()
            .lines
            .get(&ICE)
            .unwrap()
            .max_price
    };
    assert!(before > max(&p), "the old floor is out of reach now");

    // The next Kettle docking: a new mark, under the new band, citing the change.
    p.go(&WHARF);
    p.go(&KETTLE);
    p.idle(2);
    assert!(p.has_fact("core.demo_floor_revised"));
    let after = floor(&p).expect("still a floored sell");
    assert!(after <= max(&p), "{after} is reachable under {}", max(&p));
    assert!(p.talk().iter().any(|l| {
        l.line
            .text
            .starts_with("The market moved under Little Pail.")
            && l.line.cites.contains(&changed)
    }));
    for _ in 0..40 {
        if p.has_fact("core.mechanic_taught") {
            break;
        }
        p.go(&WHARF);
        p.go(&KETTLE);
    }
    assert!(p.has_fact("core.mechanic_taught"), "the lesson finished");
    p.assert_healthy();
}

/// Review finding 5: a winning sale by the player's tender while the player flies
/// counts, but the depot changes hands only at a calm moment: the player's next
/// docking (or pause).
#[test]
fn a_tenders_winning_sale_waits_for_the_player_to_dock_before_the_handover() {
    let tender: ShipId = "craft.test_tender".parse().unwrap();
    let mut p = Pilot::with(74, |s| {
        into_act_two(
            s,
            &[
                "beat.b06_nav_chip",
                "beat.b07_cinder_drift",
                "beat.b08_chasing_a_gap",
            ],
        );
        s.station_counter(CounterKind::Dockings, &HOOK, 3);
        s.docked_at(&HOOK);
        // A small dispute: ten plate wins it, and no trader tips it.
        s.tunable("dispute.hook.win", 10.0);
        s.tunable("dispute.hook.lead", 5.0);
        s.tunable("dispute.hook.ninefold", 0.0);
        s.tunable("dispute.hook.little_mercy", 0.0);
        // The player's tender, berthed at the Hook with ten plate.
        let w = s.world();
        let hook = w.stations.get(&HOOK).unwrap().clone();
        let mut ship = w.ships.get(&PLAYER).unwrap().clone();
        ship.id = tender.clone();
        ship.name = "Test tender".into();
        ship.hull = "hull.tender".parse().unwrap();
        ship.owner = Owner::Player;
        ship.region = hook.region.clone();
        ship.position = hook.position;
        ship.docked_at = Some(HOOK);
        ship.hold.cargo.clear();
        ship.hold.cargo.insert(PLATE, 10);
        ship.orders = OrderQueue::default();
        w.ships.insert(tender.clone(), ship);
    });
    p.idle(3);
    assert_eq!(p.status("beat.b09_hook_dispute"), Some(BeatStatus::Active));
    p.reply("npc.rook_adeyemi", "choice.side_coop");
    assert!(p.has_fact("core.side_chosen"));

    // The player leaves; the tender sells the winning plate while they fly.
    p.undock();
    let away = p.ship().position + Vec2::new(-600.0, 0.0);
    p.go_to(away);
    assert!(p.docked().is_none());
    p.sim.submit(
        CommandBatch::new(Cause::verb(
            Actor::Player,
            VerbKind::Fly,
            "the player's tender orders",
        ))
        .with(Command::SetOrders {
            ship: tender.clone(),
            orders: OrderQueue {
                orders: vec![Order::Sell {
                    ware: PLATE,
                    quantity: Quantity::All,
                    floor: None,
                }],
                ..OrderQueue::default()
            },
        }),
    );
    p.wait("the tender to sell its plate", |p| {
        p.facts("core.dispute_tally")
            .iter()
            .any(|f| f.subjects.ship.as_ref() == Some(&tender))
    });
    p.idle(5);
    assert!(p.docked().is_none(), "still flying");
    assert!(
        !p.has_fact("core.territory_changed"),
        "no handover while the player flies"
    );
    assert_eq!(p.status("beat.b09_hook_dispute"), Some(BeatStatus::Active));

    // The player docks: the calm moment.
    p.go(&HOOK);
    p.idle(3);
    assert!(p.has_fact("core.territory_changed"));
    assert!(p.is_done("beat.b09_hook_dispute"));
    assert_eq!(
        p.world().stations.get(&HOOK).unwrap().owner.as_ref(),
        Some(&"faction.wharf_cooperative".parse().unwrap())
    );
    p.assert_healthy();
}
