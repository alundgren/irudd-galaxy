//! Standing, traits, hull ranks, prestige and station services, headless over the core
//! pack's data.

mod common;

use common::*;
use galaxy_contract::prelude::*;
use galaxy_progress::services::{self, Refusal};
use galaxy_progress::{modifiers, prestige, standing};

const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const NAV: UpgradeId = UpgradeId::from_static("upgrade.nav_chip");
const THIMBLE: ShipId = ShipId::from_static("craft.thimble");
const BERTH: ShipId = ShipId::from_static("ship.berth.wren");

fn input(req: ServiceRequest) -> PlayerInput {
    PlayerInput {
        services: vec![req],
        ..PlayerInput::default()
    }
}

#[test]
fn standing_changes_from_deeds_with_per_docking_caps() {
    let mut s = sim(1);
    dock(&mut s, &KETTLE);
    // 25 ice sold at Kettle: two steps of 10 (+1 each).
    sell(&mut s, &ICE, 25);
    run(&mut s, 1);
    assert_eq!(standing(&s, &COOP), 2);
    // The same docking is capped at +2, however much more is sold.
    sell(&mut s, &ICE, 40);
    run(&mut s, 1);
    assert_eq!(standing(&s, &COOP), 2);
    // A new docking opens a new window.
    undock(&mut s);
    dock(&mut s, &KETTLE);
    sell(&mut s, &ICE, 10);
    run(&mut s, 1);
    assert_eq!(standing(&s, &COOP), 3);
    assert_eq!(count(&s, "standing_changed"), 3);
    let f = s
        .history()
        .last(&HistoryQuery::kind(EventKind::StandingChanged.fact_kind()))
        .unwrap();
    assert_eq!(f.subjects.faction, Some(COOP));
    assert_eq!(
        f.subjects.other.as_ref().map(|o| o.as_str()),
        Some("deed.coop.kettle_supply")
    );

    // A once-only deed pays once: the Lamplighters remember the first visit.
    dock(&mut s, &OBSERVATORY);
    run(&mut s, 1);
    dock(&mut s, &OBSERVATORY);
    run(&mut s, 1);
    assert_eq!(standing(&s, &LAMPS), 3);
    // Selling elsewhere moves nobody.
    sell(&mut s, &WareId::from_static("ware.iron_ore"), 5);
    run(&mut s, 1);
    assert_eq!(standing(&s, &COOP), 3);
}

#[test]
fn a_tier_change_writes_regard_changed_and_the_number_stays_hidden() {
    let mut s = sim(2);
    assert_eq!(
        standing::tier(s.world(), &COOP).map(|t| t.id.as_str()),
        Some("stranger")
    );
    let mut deeds = standing::Deeds::new(s.world());
    deeds.deed(&COOP, &AnyId::from_static("deed.test.big_favour"), 12, &[]);
    s.submit(CommandBatch {
        cause: Cause::system("test", "a big favour"),
        commands: deeds.commands,
    });
    run(&mut s, 1);
    assert_eq!(
        standing::tier(s.world(), &COOP).map(|t| t.id.as_str()),
        Some("known")
    );
    let f = s
        .history()
        .last(&HistoryQuery::kind(EventKind::RegardChanged.fact_kind()))
        .unwrap();
    match &f.payload {
        FactPayload::Event(Event::RegardChanged { from, to, .. }) => {
            assert_eq!((from.as_str(), to.as_str()), ("stranger", "known"));
        }
        other => panic!("{other:?}"),
    }
    // Clamped to the standing range.
    let mut deeds = standing::Deeds::new(s.world());
    assert_eq!(
        deeds.deed(&COOP, &AnyId::from_static("deed.test.huge"), 500, &[]),
        Some(88)
    );
}

#[test]
fn a_trait_is_earned_from_deeds_and_kept_through_prestige() {
    let mut s = sim(3);
    for _ in 0..23 {
        jump(&mut s);
    }
    run(&mut s, 1);
    let lanerunner = TraitId::from_static("trait.lanerunner");
    assert!(!s.world().player.traits.contains(&lanerunner));
    jump(&mut s);
    run(&mut s, 1);
    assert!(s.world().player.traits.contains(&lanerunner));
    let earned = s
        .history()
        .last(&HistoryQuery::kind(EventKind::TraitEarned.fact_kind()))
        .unwrap();
    assert_eq!(earned.amount, Some(24), "deed_count");
    assert_eq!(count(&s, "trait_earned"), 1, "earned once");
    let m = modifiers::for_ship(s.world(), &PLAYER);
    assert_eq!(m.mouth_radius, Some(105.0));
    assert!(m.has("lane_radio"));

    ready_for_prestige(&mut s);
    tick(&mut s, &input(ServiceRequest::Prestige { to: TANDEM }));
    run(&mut s, 1);
    assert_eq!(s.world().ships.get(&PLAYER).unwrap().hull, TANDEM);
    assert!(
        s.world().player.traits.contains(&lanerunner),
        "traits survive prestige"
    );
    assert_eq!(
        modifiers::for_ship(s.world(), &PLAYER).mouth_radius,
        Some(105.0)
    );
    assert_eq!(count(&s, "trait_earned"), 1);
}

/// Docked at the Wharf with the fee and b16 started.
fn ready_for_prestige(s: &mut galaxy_core::Sim) {
    set_beat(s, "beat.b16_hullwrights_question", BeatStatus::Active, 0);
    credits(s, 900_000);
    dock(s, &WHARF);
}

#[test]
fn prestige_charges_the_fee_and_resets_only_hull_ranks() {
    let mut s = sim(4);
    // Wren rank 1 ("Settled"): 25 dockings in this hull.
    for _ in 0..25 {
        dock(&mut s, &KETTLE);
    }
    run(&mut s, 1);
    assert_eq!(s.world().player.hull_records.get(&WREN).unwrap().rank, 1);
    assert_eq!(count(&s, "hull_rank"), 1);
    // A fitting, a capability flag and some standing to keep.
    set_beat(&mut s, "beat.b03_first_delivery", BeatStatus::Completed, 0);
    credits(&mut s, 60_000);
    dock(&mut s, &WHARF);
    tick(&mut s, &input(ServiceRequest::Fit { upgrade: LENS }));
    run(&mut s, 1);
    assert!(s.world().player.fittings.contains(&LENS));
    let coop = standing(&s, &COOP);

    // No offer before b16 has started.
    assert_eq!(
        services::check(
            s.world(),
            s.history(),
            &ServiceRequest::Prestige { to: TANDEM }
        ),
        Err(Refusal::Locked)
    );
    ready_for_prestige(&mut s);
    let offers = services::offers(s.world(), s.history(), &WHARF);
    let hulls: Vec<_> = offers
        .iter()
        .filter_map(|o| match &o.request {
            ServiceRequest::Prestige { to } => Some(to.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(hulls, vec![TANDEM, HullId::from_static("hull.heron")]);
    let o = prestige::offer(s.world(), s.history()).unwrap();
    let (stays, resets) = prestige::summary(s.world(), &o);
    assert_eq!(resets.len(), 1);
    assert!(stays.iter().any(|l| l.contains("8000 cr")));

    let before = s.world().player.credits;
    let r = tick(&mut s, &input(ServiceRequest::Prestige { to: TANDEM }));
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    run(&mut s, 1);
    let w = s.world();
    assert_eq!(w.player.credits, before - 800_000, "the fee is 8000 cr");
    assert_eq!(w.ships.get(&PLAYER).unwrap().hull, TANDEM);
    let berth = w.ships.get(&BERTH).expect("the Wren stays berthed");
    assert_eq!(
        (&berth.hull, berth.docked_at.as_ref()),
        (&WREN, Some(&WHARF))
    );
    assert!(prestige::is_berthed(w, &BERTH));
    let wren = w.player.hull_records.get(&WREN).unwrap();
    assert_eq!(wren.rank, 1, "the Wren keeps its stripes on record");
    assert!(wren.until.is_some());
    assert_eq!(w.player.hull_records.get(&TANDEM).unwrap().rank, 0);
    assert!(w.player.fittings.contains(&LENS), "fittings move across");
    assert_eq!(standing(&s, &COOP), coop, "standing stays");
    let f = s
        .history()
        .last(&HistoryQuery::kind(EventKind::Prestige.fact_kind()))
        .unwrap();
    assert_eq!(f.amount, Some(800_000));
    // The lens's modifier now applies to the Tandem.
    assert_eq!(modifiers::for_ship(s.world(), &PLAYER).mining_rate, 1.5);

    // The Tandem's ranks count only its own deeds: 24 more dockings are not enough.
    for _ in 0..24 {
        dock(&mut s, &KETTLE);
    }
    run(&mut s, 1);
    assert_eq!(s.world().player.hull_records.get(&TANDEM).unwrap().rank, 0);
    dock(&mut s, &KETTLE);
    run(&mut s, 1);
    assert_eq!(s.world().player.hull_records.get(&TANDEM).unwrap().rank, 1);
    // A second prestige from the Tandem has no branch in the slice.
    assert!(prestige::offer(s.world(), s.history()).is_none());
}

#[test]
fn prestige_without_the_fee_is_refused_and_changes_nothing() {
    let mut s = sim(5);
    set_beat(
        &mut s,
        "beat.b16_hullwrights_question",
        BeatStatus::Active,
        0,
    );
    credits(&mut s, 700_000);
    dock(&mut s, &WHARF);
    assert!(matches!(
        services::check(
            s.world(),
            s.history(),
            &ServiceRequest::Prestige { to: TANDEM }
        ),
        Err(Refusal::Credits { .. })
    ));
    let hash = s.world().player.clone();
    tick(&mut s, &input(ServiceRequest::Prestige { to: TANDEM }));
    assert_eq!(s.world().player, hash);
    assert_eq!(s.world().ships.get(&PLAYER).unwrap().hull, WREN);
}

#[test]
fn prestige_into_a_smaller_hold_is_refused_while_the_cargo_would_not_fit() {
    let mut s = sim(7);
    ready_for_prestige(&mut s);
    s.submit(
        CommandBatch::new(Cause::system("test", "a full hold")).with(Command::AdjustCargo {
            ship: PLAYER,
            ware: "ware.ice".parse().unwrap(),
            delta: 20,
        }),
    );
    run(&mut s, 1);
    let heron = ServiceRequest::Prestige {
        to: "hull.heron".parse().unwrap(),
    };
    assert!(matches!(
        services::check(s.world(), s.history(), &heron),
        Err(Refusal::HoldTooFull {
            holds: 20,
            room: 16
        })
    ));
    // The Tandem holds 40: that one is fine.
    assert!(
        services::check(
            s.world(),
            s.history(),
            &ServiceRequest::Prestige { to: TANDEM }
        )
        .is_ok()
    );
}

#[test]
fn upgrade_purchases_are_atomic() {
    let mut s = sim(6);
    set_beat(&mut s, "beat.b03_first_delivery", BeatStatus::Completed, 0);
    dock(&mut s, &WHARF);
    credits(&mut s, 70_000 - 4_000); // 700 cr
    assert_eq!(s.world().player.credits, 70_000);
    // The purchase is worked out from 700 cr, but 200 cr leave earlier in the same
    // tick: the lens batch's payment fails, so none of it applies.
    s.submit(
        CommandBatch::new(Cause::system("test", "spend"))
            .with(Command::AdjustCredits { delta: -20_000 }),
    );
    let r = tick(&mut s, &input(ServiceRequest::Fit { upgrade: LENS }));
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    run(&mut s, 1);
    let w = s.world();
    assert_eq!(w.player.credits, 50_000);
    assert!(w.player.fittings.is_empty());
    assert_eq!(count(&s, "upgrade_fitted"), 0);

    // Undocking earlier in the tick also rejects the whole purchase.
    credits(&mut s, 10_000);
    let before = s.world().player.clone();
    s.submit(
        CommandBatch::new(Cause::system("test", "undock")).with(Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        }),
    );
    let r = tick(&mut s, &input(ServiceRequest::Fit { upgrade: LENS }));
    assert_eq!(r.rejected.len(), 1);
    assert_eq!(s.world().player, before);

    // Docked with the credits: paid, fitted, written.
    dock(&mut s, &WHARF);
    tick(&mut s, &input(ServiceRequest::Fit { upgrade: LENS }));
    run(&mut s, 1);
    let w = s.world();
    assert_eq!(w.player.credits, 0);
    assert!(w.player.fittings.contains(&LENS));
    assert_eq!(count(&s, "upgrade_fitted"), 1);
    assert_eq!(
        services::check(w, s.history(), &ServiceRequest::Fit { upgrade: LENS }),
        Err(Refusal::AlreadyFitted)
    );
}

#[test]
fn an_upgrade_unlocks_from_activity_and_grants_its_capability() {
    let mut s = sim(7);
    credits(&mut s, 100_000);
    dock(&mut s, &WHARF);
    let nav = ServiceRequest::Fit { upgrade: NAV };
    assert_eq!(
        services::check(s.world(), s.history(), &nav),
        Err(Refusal::Locked)
    );
    set_beat(&mut s, "beat.b05_pause", BeatStatus::Completed, 0);
    // b05 done, but only 1 docking so far: still locked.
    assert_eq!(
        services::check(s.world(), s.history(), &nav),
        Err(Refusal::Locked)
    );
    for _ in 0..5 {
        dock(&mut s, &WHARF);
    }
    assert!(
        services::offers(s.world(), s.history(), &WHARF)
            .iter()
            .any(|o| o.request == nav && o.affordable)
    );
    // Not sold elsewhere.
    assert!(
        !services::offers(s.world(), s.history(), &KETTLE)
            .iter()
            .any(|o| o.request == nav)
    );
    tick(&mut s, &input(nav));
    run(&mut s, 1);
    let cap = FlagId::from_static("cap.click_fly");
    assert!(s.world().player.flags.contains(&cap));
    let f = s
        .history()
        .last(&HistoryQuery::kind(EventKind::CapUnlocked.fact_kind()))
        .unwrap();
    assert_eq!(
        f.subjects.other.as_ref().map(|o| o.as_str()),
        Some(cap.as_str())
    );
}

#[test]
fn a_tender_is_built_at_its_station_and_the_cooperative_notices() {
    let mut s = sim(8);
    credits(&mut s, 300_000);
    dock(&mut s, &KETTLE);
    let build = ServiceRequest::Build { craft: THIMBLE };
    assert_eq!(
        services::check(s.world(), s.history(), &build),
        Err(Refusal::Locked)
    );
    set_beat(&mut s, "beat.b13_first_tender", BeatStatus::Active, 1);
    let coop = standing(&s, &COOP);
    tick(&mut s, &input(build.clone()));
    run(&mut s, 2);
    let t = s.world().ships.get(&THIMBLE).expect("built");
    assert_eq!(t.owner, Owner::Player);
    assert_eq!(t.docked_at.as_ref(), Some(&KETTLE));
    assert_eq!(t.hull, HullId::from_static("hull.tender"));
    assert_eq!(count(&s, "tender_acquired"), 1);
    assert_eq!(standing(&s, &COOP), coop + 3);
    assert_eq!(
        services::check(s.world(), s.history(), &build),
        Err(Refusal::AlreadyOwned)
    );
}

#[test]
fn progress_runs_are_deterministic_and_survive_save_and_load() {
    fn script(s: &mut galaxy_core::Sim, from: usize, to: usize) {
        for i in from..to {
            match i % 4 {
                0 => dock(s, &KETTLE),
                1 => sell(s, &ICE, 7),
                2 => jump(s),
                _ => undock(s),
            }
        }
    }
    let mut a = sim(9);
    script(&mut a, 0, 120);
    let mut b = sim(9);
    script(&mut b, 0, 120);
    assert_eq!(a.hash(), b.hash());

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.sqlite");
    let mut c = sim(9);
    script(&mut c, 0, 60);
    galaxy_core::Save::create(&path, c.state()).unwrap();
    let state = galaxy_core::Save::open(&path).unwrap().load().unwrap();
    let mut d = galaxy_core::Sim::new(state);
    d.add_system(Box::new(galaxy_progress::ProgressSystem::default()));
    script(&mut d, 60, 120);
    assert_eq!(d.hash(), a.hash());
    assert!(a.world().player.traits.contains("trait.lanerunner"));
    assert!(standing(&a, &COOP) > 0);
}
