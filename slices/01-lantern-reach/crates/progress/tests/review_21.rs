//! Regressions for the PR #21 review: stale purchase, prestige, rank and deed batches
//! are checked against the committed world, berthed hulls can't be automated, and
//! Thimble can be given a loop before b13 completes.

mod common;

use common::*;
use galaxy_contract::prelude::*;
use galaxy_progress::{prestige, standing};
use galaxy_ship::{ControlError, FlatDesk, ShipSystem, controls};

const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const THIMBLE: ShipId = ShipId::from_static("craft.thimble");
const BERTH: ShipId = ShipId::from_static("ship.berth.wren");

fn inputs(reqs: Vec<ServiceRequest>) -> PlayerInput {
    PlayerInput {
        services: reqs,
        ..PlayerInput::default()
    }
}

/// Docked at the Wharf with twice the fee and b16 started.
fn ready_for_prestige(s: &mut galaxy_core::Sim) {
    set_beat(s, "beat.b16_hullwrights_question", BeatStatus::Active, 0);
    credits(s, 1_600_000);
    dock(s, &WHARF);
}

fn player(s: &galaxy_core::Sim) -> &Ship {
    s.world().ships.get(&PLAYER).unwrap()
}

#[test]
fn prestige_keeps_cargo_and_credits_committed_earlier_in_the_tick() {
    let mut s = sim(21);
    ready_for_prestige(&mut s);
    let before = s.world().player.credits;
    // Ice loaded and a sale paid ahead of the prestige batch, in the same tick.
    s.submit(CommandBatch {
        cause: Cause::system("test", "load and sell"),
        commands: vec![
            Command::AdjustCargo {
                ship: PLAYER,
                ware: ICE,
                delta: 3,
            },
            Command::AdjustCredits { delta: 500 },
        ],
    });
    let r = tick(
        &mut s,
        &inputs(vec![ServiceRequest::Prestige { to: TANDEM }]),
    );
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    let ship = player(&s);
    assert_eq!(ship.hull, TANDEM);
    assert_eq!(ship.name, "Tandem");
    assert_eq!(
        ship.hold.count(&ICE),
        3,
        "the ice loaded first stays aboard"
    );
    assert_eq!(s.world().player.credits, before + 500 - 800_000);
    assert!(s.world().ships.get(&BERTH).unwrap().hold.cargo.is_empty());
}

#[test]
fn a_purchase_asked_for_twice_in_one_tick_is_charged_once() {
    let mut s = sim(22);
    set_beat(&mut s, "beat.b03_first_delivery", BeatStatus::Completed, 0);
    credits(&mut s, 200_000);
    dock(&mut s, &WHARF);
    let before = s.world().player.credits;
    let lens = ServiceRequest::Fit { upgrade: LENS };
    let r = tick(&mut s, &inputs(vec![lens.clone(), lens]));
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    assert_eq!(s.world().player.credits, before - 60_000, "600 cr, once");
    assert_eq!(count(&s, "upgrade_fitted"), 1);

    // Tenders.
    set_beat(&mut s, "beat.b13_first_tender", BeatStatus::Active, 1);
    credits(&mut s, 500_000);
    dock(&mut s, &KETTLE);
    let before = s.world().player.credits;
    let build = ServiceRequest::Build { craft: THIMBLE };
    let r = tick(&mut s, &inputs(vec![build.clone(), build]));
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    assert_eq!(s.world().player.credits, before - 240_000, "2400 cr, once");
    assert_eq!(count(&s, "tender_acquired"), 1);

    // Prestige.
    ready_for_prestige(&mut s);
    let before = s.world().player.credits;
    let up = ServiceRequest::Prestige { to: TANDEM };
    let r = tick(&mut s, &inputs(vec![up.clone(), up]));
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    assert_eq!(s.world().player.credits, before - 800_000, "8000 cr, once");
    assert_eq!(count(&s, "prestige"), 1);
    assert_eq!(player(&s).hull, TANDEM);
}

#[test]
fn a_stripe_due_on_the_prestige_tick_keeps_the_berth_record() {
    let mut s = sim(23);
    // 24 dockings: the Wren's first stripe needs 25.
    for _ in 0..24 {
        dock(&mut s, &KETTLE);
    }
    run(&mut s, 1);
    set_beat(
        &mut s,
        "beat.b16_hullwrights_question",
        BeatStatus::Active,
        0,
    );
    credits(&mut s, 900_000);
    assert_eq!(
        s.world()
            .player
            .hull_records
            .get(&WREN)
            .map_or(0, |r| r.rank),
        0
    );
    // The 25th docking: its stripe comes due on the tick the player trades up.
    dock(&mut s, &WHARF);
    let r = tick(
        &mut s,
        &inputs(vec![ServiceRequest::Prestige { to: TANDEM }]),
    );
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    run(&mut s, 2);
    let w = s.world();
    let wren = w.player.hull_records.get(&WREN).unwrap();
    assert_eq!(
        wren.rank, 1,
        "the stripe is painted before the hull is frozen"
    );
    assert!(wren.until.is_some());
    assert_eq!(wren.berthed_as.as_ref(), Some(&BERTH));
    assert!(prestige::is_berthed(w, &BERTH));
    assert_eq!(w.player.hull_records.get(&TANDEM).unwrap().rank, 0);
    assert_eq!(count(&s, "hull_rank"), 1);

    // A rank batch worked out before the prestige applied fails on the changed record.
    let mut stale = sim(23);
    for _ in 0..24 {
        dock(&mut stale, &KETTLE);
    }
    set_beat(
        &mut stale,
        "beat.b16_hullwrights_question",
        BeatStatus::Active,
        0,
    );
    credits(&mut stale, 900_000);
    dock(&mut stale, &WHARF);
    let rank = galaxy_progress::growth::earned_rank(stale.world(), stale.history());
    assert!(!rank.is_empty(), "a stripe is due");
    let up = prestige::batch(stale.world(), stale.history(), &TANDEM).unwrap();
    stale.submit(up);
    stale.submit(CommandBatch {
        cause: Cause::system("test", "stale stripe"),
        commands: rank,
    });
    // The test's stale batch and the progress system's own both fail; the prestige holds.
    let r = tick(&mut stale, &PlayerInput::default());
    assert_eq!(r.rejected.len(), 2, "{:#?}", r.rejected);
    assert!(
        r.rejected
            .iter()
            .all(|x| x.cause.summary.contains("stripe"))
    );
    assert!(prestige::is_berthed(stale.world(), &BERTH));
    assert_eq!(
        stale.world().player.hull_records.get(&WREN).unwrap().rank,
        1
    );
}

#[test]
fn a_berthed_hull_never_takes_or_runs_a_loop() {
    let mut s = sim(24);
    ready_for_prestige(&mut s);
    tick(
        &mut s,
        &inputs(vec![ServiceRequest::Prestige { to: TANDEM }]),
    );
    run(&mut s, 1);
    let automation = controls::set_capability(s.world(), galaxy_ship::Capability::Automation, true);
    s.submit(CommandBatch {
        cause: Cause::system("test", "automation"),
        commands: vec![
            automation,
            Command::SetLoop {
                name: "run".into(),
                orders: Some(vec![Order::Undock {}, Order::Dock { station: KETTLE }]),
            },
        ],
    });
    run(&mut s, 1);
    let w = s.world();
    assert_eq!(
        controls::assign_loop(w, &FlatDesk, &BERTH, "run", None).unwrap_err(),
        ControlError::Berthed(BERTH)
    );
    assert_eq!(
        controls::run_loop(w, &BERTH, "run").unwrap_err(),
        ControlError::Berthed(BERTH)
    );

    // Orders it was given anyway (a stale save, a raw write) never run.
    s.add_system(Box::new(ShipSystem::new(Box::new(FlatDesk))));
    let orders = OrderQueue {
        orders: vec![Order::Undock {}],
        ..OrderQueue::default()
    };
    s.submit(
        CommandBatch::new(Cause::system("test", "stale orders")).with(Command::SetOrders {
            ship: BERTH,
            orders: orders.clone(),
        }),
    );
    for _ in 0..5 {
        tick(&mut s, &PlayerInput::default());
    }
    let berth = s.world().ships.get(&BERTH).unwrap();
    assert_eq!(berth.docked_at.as_ref(), Some(&WHARF));
    assert_eq!(berth.orders, orders);
}

#[test]
fn thimble_can_take_a_loop_before_b13_completes() {
    let mut s = sim(25);
    set_beat(&mut s, "beat.b13_first_tender", BeatStatus::Active, 1);
    credits(&mut s, 300_000);
    dock(&mut s, &KETTLE);
    s.submit(
        CommandBatch::new(Cause::system("test", "a saved loop")).with(Command::SetLoop {
            name: "ice".into(),
            orders: Some(vec![Order::Undock {}, Order::Dock { station: KETTLE }]),
        }),
    );
    run(&mut s, 1);
    tick(
        &mut s,
        &inputs(vec![ServiceRequest::Build { craft: THIMBLE }]),
    );
    run(&mut s, 1);
    let w = s.world();
    assert!(
        w.player.flags.contains("cap.automation"),
        "Thimble brings it"
    );
    assert_eq!(
        w.progress.beats["beat.b13_first_tender"].status,
        BeatStatus::Active
    );
    let assign =
        controls::assign_loop(w, &FlatDesk, &THIMBLE, "ice", None).expect("b13 step 3 can be done");
    s.submit(assign);
    run(&mut s, 1);
    assert!(
        !s.world()
            .ships
            .get(&THIMBLE)
            .unwrap()
            .orders
            .orders
            .is_empty()
    );
}

#[test]
fn deeds_worked_out_from_the_same_standing_never_overshoot_together() {
    let mut s = sim(26);
    let start = standing(&s, &COOP);
    s.submit(
        CommandBatch::new(Cause::system("test", "to 90")).with(Command::AdjustStanding {
            faction: COOP,
            delta: 90 - start,
        }),
    );
    run(&mut s, 1);
    let deed = AnyId::from_static("deed.test");
    let batch = |s: &galaxy_core::Sim| {
        let mut d = standing::Deeds::new(s.world());
        d.deed(&COOP, &deed, 8, &[]);
        CommandBatch {
            cause: Cause::system("test", "a deed"),
            commands: d.commands,
        }
    };
    // Two batches from the same world: the second would reach 106, so it fails.
    let (a, b) = (batch(&s), batch(&s));
    s.submit(a);
    s.submit(b);
    let r = tick(&mut s, &PlayerInput::default());
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    assert_eq!(standing(&s, &COOP), 98);

    // Mid-tier and inside the range, neither changes the other's effect: both apply.
    s.submit(
        CommandBatch::new(Cause::system("test", "to 40")).with(Command::AdjustStanding {
            faction: COOP,
            delta: 40 - 98,
        }),
    );
    run(&mut s, 1);
    let (a, b) = (batch(&s), batch(&s));
    s.submit(a);
    s.submit(b);
    let r = tick(&mut s, &PlayerInput::default());
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    assert_eq!(standing(&s, &COOP), 56);
}
