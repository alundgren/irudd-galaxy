//! The hullwright's fit model (#47 Phase 3): parts in sockets, the power cap, the
//! hull's locker, Ready / Make / Friends, the read API, prestige, and a save made
//! before the fit model. Headless, over the core pack's data.

use crate::common;

use common::*;
use galaxy_contract::fit;
use galaxy_contract::prelude::*;
use galaxy_progress::hullwright::{self, FitRefusal, Held};
use galaxy_progress::services::{self, Refusal};

const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");
const PODS: UpgradeId = UpgradeId::from_static("upgrade.hold_pods");
const EXTENDER: UpgradeId = UpgradeId::from_static("upgrade.hold_extender");
const LONG_DRILL: UpgradeId = UpgradeId::from_static("upgrade.long_drill");
const FINS: UpgradeId = UpgradeId::from_static("upgrade.swift_fins");
const LAMP: UpgradeId = UpgradeId::from_static("upgrade.seam_lamp");
const NAV: UpgradeId = UpgradeId::from_static("upgrade.nav_chip");
const HOOK: StationId = StationId::from_static("station.cinder_hook");
const IRON: WareId = WareId::from_static("ware.iron_ore");
const PLATE: WareId = WareId::from_static("ware.refined_plate");
const DRILL: SocketId = SocketId::from_static("socket.drill");
const HOLD: SocketId = SocketId::from_static("socket.hold");
const RACK: SocketId = SocketId::from_static("socket.rack");

fn input(req: ServiceRequest) -> PlayerInput {
    PlayerInput {
        services: vec![req],
        ..PlayerInput::default()
    }
}

/// Asks for a service and runs the tick; it must go through.
fn service(s: &mut galaxy_core::Sim, req: ServiceRequest) {
    services::check(s.world(), s.history(), &req).unwrap_or_else(|e| panic!("{req:?}: {e}"));
    let r = tick(s, &input(req));
    assert!(r.rejected.is_empty(), "{:#?}", r.rejected);
    run(s, 1);
}

fn refused(s: &galaxy_core::Sim, req: ServiceRequest) -> Refusal {
    services::check(s.world(), s.history(), &req).expect_err("refused")
}

fn locker(s: &galaxy_core::Sim) -> Vec<UpgradeId> {
    fit::locker(s.world(), &PLAYER).cloned().collect()
}

/// The note of the latest fact of `kind`.
fn last_note(s: &galaxy_core::Sim, kind: &str) -> String {
    let f = s
        .history()
        .last(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
        .unwrap_or_else(|| panic!("no {kind} fact"));
    match &f.payload {
        FactPayload::Note(n) => n.clone(),
        p => panic!("{p:?}"),
    }
}

fn act(s: &mut galaxy_core::Sim, commands: Vec<Command>) {
    s.submit(CommandBatch {
        cause: Cause::system("test_player", "test"),
        commands,
    });
    run(s, 1);
}

fn cargo(s: &mut galaxy_core::Sim, ware: &WareId, units: i64) {
    act(
        s,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ware.clone(),
            delta: units,
        }],
    );
}

/// At the Wharf with the lens and hold pods on offer and money to spend.
fn at_the_wharf() -> galaxy_core::Sim {
    let mut s = sim(1);
    set_beat(&mut s, "beat.b03_first_delivery", BeatStatus::Completed, 0);
    dock(&mut s, &KETTLE);
    sell(&mut s, &ICE, 60);
    credits(&mut s, 1_000_000);
    dock(&mut s, &WHARF);
    s
}

#[test]
fn ready_parts_fit_their_sockets_and_a_swap_puts_the_old_part_in_the_locker() {
    let mut s = at_the_wharf();
    let start = s.world().player.credits;
    service(&mut s, ServiceRequest::Fit { upgrade: LENS });
    assert!(s.world().player.fittings.contains(&LENS));
    assert_eq!(s.world().player.credits, start - 60_000);
    assert_eq!(last_note(&s, "part_fitted"), "Fitted mining lens · −600 cr");
    // Orrin still says so himself (the upgrade_fitted event).
    assert_eq!(count(&s, "upgrade_fitted"), 1);

    service(&mut s, ServiceRequest::Fit { upgrade: PODS });
    assert_eq!(last_note(&s, "part_fitted"), "Fitted hold pods · −450 cr");
    let hold = fit::flagship(s.world()).unwrap().clone();
    assert_eq!(
        galaxy_contract::modifiers::hold_capacity(s.world(), &hold),
        30
    );

    // The extender goes into the hold socket: the pods come off into the locker.
    dock(&mut s, &KETTLE);
    sell(&mut s, &ICE, 100);
    dock(&mut s, &WHARF);
    service(&mut s, ServiceRequest::Fit { upgrade: EXTENDER });
    let w = s.world();
    assert!(w.player.fittings.contains(&EXTENDER) && !w.player.fittings.contains(&PODS));
    assert_eq!(locker(&s), vec![PODS]);
    assert_eq!(
        last_note(&s, "part_fitted"),
        "Fitted hold extender · −1500 cr; your hold pods went to the Wren's locker"
    );
    // Owned parts leave the shop list, but stay in the wharf's tray as the player's.
    let offers = services::offers(s.world(), s.history(), &WHARF);
    assert!(!offers.iter().any(|o| o.name == "Hold pods"));
    let room = hullwright::wharf(s.world(), s.history(), &WHARF).unwrap();
    let pods = room.ready.iter().find(|o| o.part == PODS).unwrap();
    assert_eq!(pods.held, Some(Held::Locker));
    assert_eq!(room.locker.len(), 1);
    assert_eq!(
        room.locker[0].request,
        ServiceRequest::Refit { upgrade: PODS }
    );
    assert!(room.locker[0].blocked.is_none());

    // Buying the pods again is refused: they're in the locker.
    assert_eq!(
        refused(&s, ServiceRequest::Fit { upgrade: PODS }),
        Refusal::Fit(FitRefusal::InLocker)
    );
}

#[test]
fn removing_and_refitting_go_through_the_hulls_locker_at_any_wharf_for_that_socket() {
    let mut s = at_the_wharf();
    service(&mut s, ServiceRequest::Fit { upgrade: LENS });
    service(&mut s, ServiceRequest::Fit { upgrade: PODS });
    // Controls stay fitted.
    act(
        &mut s,
        vec![Command::SetFitting {
            upgrade: NAV,
            fitted: true,
        }],
    );
    assert_eq!(
        refused(&s, ServiceRequest::Remove { upgrade: NAV }),
        Refusal::Fit(FitRefusal::Module)
    );

    service(&mut s, ServiceRequest::Remove { upgrade: LENS });
    assert!(!s.world().player.fittings.contains(&LENS));
    assert_eq!(locker(&s), vec![LENS]);
    assert_eq!(
        last_note(&s, "part_removed"),
        "Removed mining lens to the Wren's locker"
    );
    // The beat that waits for the lens reads what's fitted now.
    assert_eq!(
        galaxy_contract::modifiers::for_ship(s.world(), &PLAYER).mining_rate,
        1.0
    );

    // Juno's rough rig at Cinder Hook works on drill mounts only.
    dock(&mut s, &HOOK);
    assert_eq!(
        refused(&s, ServiceRequest::Remove { upgrade: PODS }),
        Refusal::Fit(FitRefusal::NoWharf {
            socket: "hold".into()
        })
    );
    service(&mut s, ServiceRequest::Refit { upgrade: LENS });
    assert!(s.world().player.fittings.contains(&LENS));
    assert!(locker(&s).is_empty());
    assert_eq!(
        last_note(&s, "part_refitted"),
        "Refitted mining lens from the Wren's locker"
    );
    // Kettle Dock has no wharf at all.
    dock(&mut s, &KETTLE);
    assert_eq!(
        refused(&s, ServiceRequest::Remove { upgrade: LENS }),
        Refusal::Fit(FitRefusal::NoWharf {
            socket: "drill mount".into()
        })
    );

    // Pods off with a full hold: the hold would be too small, so it's refused.
    dock(&mut s, &WHARF);
    cargo(&mut s, &ICE, 30);
    assert_eq!(
        refused(&s, ServiceRequest::Remove { upgrade: PODS }),
        Refusal::Fit(FitRefusal::HoldFull {
            holds: 30,
            room: 20
        })
    );
}

#[test]
fn a_fit_over_the_power_cap_is_refused_and_the_preview_shows_it() {
    let mut s = at_the_wharf();
    service(&mut s, ServiceRequest::Fit { upgrade: LENS });
    // The seam lamp (2) and the swift fins (1) wait in the locker.
    act(
        &mut s,
        [LAMP, FINS]
            .into_iter()
            .map(|p| Command::SetLocker {
                ship: PLAYER,
                part: p,
                stored: true,
            })
            .collect(),
    );
    service(&mut s, ServiceRequest::Refit { upgrade: LAMP });
    let b = hullwright::budget(s.world()).unwrap();
    assert_eq!((b.power_used, b.power_cap), (4, 4));
    assert_eq!(
        refused(&s, ServiceRequest::Refit { upgrade: FINS }),
        Refusal::Fit(FitRefusal::OverPower {
            need: 5,
            cap: 4,
            hull: "Wren".into()
        })
    );
    let p = hullwright::preview(s.world(), s.history(), &FINS).unwrap();
    assert!(p.budget_after.over_power());
    assert_eq!(p.request, ServiceRequest::Refit { upgrade: FINS });
    assert!(matches!(
        p.refusal,
        Some(Refusal::Fit(FitRefusal::OverPower { .. }))
    ));
    // Taking the lamp off makes room.
    service(&mut s, ServiceRequest::Remove { upgrade: LAMP });
    service(&mut s, ServiceRequest::Refit { upgrade: FINS });
    let b = hullwright::budget(s.world()).unwrap();
    assert_eq!((b.power_used, b.power_cap), (3, 4));
}

/// Two refits asked for in one tick are each planned on the same starting fit, so each
/// alone fits the cap; the second to commit must be undone, not leave the Wren over it.
#[test]
fn two_refits_in_one_tick_respect_the_combined_power_cap() {
    let mut s = at_the_wharf();
    service(&mut s, ServiceRequest::Fit { upgrade: LENS });
    act(
        &mut s,
        [LAMP, FINS]
            .into_iter()
            .map(|p| Command::SetLocker {
                ship: PLAYER,
                part: p,
                stored: true,
            })
            .collect(),
    );
    let both = PlayerInput {
        services: vec![
            ServiceRequest::Refit { upgrade: LAMP },
            ServiceRequest::Refit { upgrade: FINS },
        ],
        ..PlayerInput::default()
    };
    let r = tick(&mut s, &both);
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    run(&mut s, 1);
    let b = hullwright::budget(s.world()).unwrap();
    assert!(b.power_used <= b.power_cap, "{b:?}");
    assert_eq!(locker(&s).len(), 1, "the refused part stays in the locker");
}

#[test]
fn the_stat_preview_shows_before_and_after() {
    let s = at_the_wharf();
    let p = hullwright::preview(s.world(), s.history(), &PODS).unwrap();
    assert_eq!(p.socket, HOLD);
    assert_eq!(p.request, ServiceRequest::Fit { upgrade: PODS });
    assert!(p.refusal.is_none());
    assert_eq!((p.before.hold, p.after.hold), (20, 30));
    // Four mass on the Wren's three free: a little slower, wider turns.
    assert!((p.after.top_speed - p.before.top_speed * 0.96).abs() < 1e-3);
    assert!((p.after.turn_rate - p.before.turn_rate * 0.94).abs() < 1e-3);
    assert_eq!(p.budget_after.excess_mass(), 1);
    let lens = hullwright::preview(s.world(), s.history(), &LENS).unwrap();
    assert!((lens.after.mining_rate - lens.before.mining_rate * 1.5).abs() < 1e-5);
    assert_eq!(lens.after.top_speed, lens.before.top_speed);
    // Control modules aren't parts: no preview.
    assert!(hullwright::preview(s.world(), s.history(), &NAV).is_none());

    // The sockets light up for the part held over the cradle.
    let sockets = hullwright::sockets(s.world(), s.history(), &WHARF, Some(&LENS));
    let lit: Vec<_> = sockets
        .iter()
        .filter(|x| x.takes)
        .map(|x| &x.socket)
        .collect();
    assert_eq!(lit, vec![&DRILL]);
    assert!(sockets.iter().any(|x| x.socket == RACK && x.rack));
    // At Cinder Hook only the drill mount is worked on.
    let mut s = s;
    dock(&mut s, &HOOK);
    let here: Vec<_> = hullwright::sockets(s.world(), s.history(), &HOOK, None)
        .into_iter()
        .filter(|x| x.works_here && !x.rack)
        .map(|x| x.socket)
        .collect();
    assert_eq!(here, vec![DRILL]);
}

#[test]
fn making_a_part_needs_its_blueprint_and_takes_the_materials() {
    let mut s = at_the_wharf();
    let make = ServiceRequest::Make {
        upgrade: LONG_DRILL,
        fit: false,
    };
    assert_eq!(
        refused(&s, make.clone()),
        Refusal::Fit(FitRefusal::NoBlueprint("long-throw drill blueprint".into()))
    );
    let room = hullwright::wharf(s.world(), s.history(), &WHARF).unwrap();
    assert!(room.make.is_empty(), "nothing to make before a blueprint");

    // Mining 20 in the Cinder Drift turns the blueprint up.
    act(
        &mut s,
        vec![Command::Emit {
            event: Event::Mine {
                ship: PLAYER,
                rock: RockId::from_static("rock.cinder.iron.01"),
                ware: IRON,
                amount: 20,
            },
        }],
    );
    run(&mut s, 1);
    assert!(s.world().player.flags.contains("blueprint.long_drill"));
    assert_eq!(
        last_note(&s, "blueprint_found"),
        "Found the long-throw drill blueprint in a seam in the Cinder Drift. Orrin can make it at Lantern Wharf."
    );
    assert_eq!(
        refused(&s, make.clone()),
        Refusal::Fit(FitRefusal::Materials(
            "8 iron ore and 4 refined plate".into()
        ))
    );
    let room = hullwright::wharf(s.world(), s.history(), &WHARF).unwrap();
    let offer = &room.make[0];
    assert_eq!(offer.tab, "Make");
    assert_eq!(
        offer
            .materials
            .iter()
            .map(|m| (m.name.as_str(), m.need, m.have))
            .collect::<Vec<_>>(),
        vec![("iron ore", 8, 0), ("refined plate", 4, 0)]
    );

    cargo(&mut s, &IRON, 10);
    cargo(&mut s, &PLATE, 4);
    service(&mut s, make);
    let ship = fit::flagship(s.world()).unwrap();
    assert_eq!(ship.hold.cargo.get(&IRON), Some(&2));
    assert_eq!(ship.hold.cargo.get(&PLATE), None);
    assert_eq!(locker(&s), vec![LONG_DRILL]);
    assert_eq!(
        last_note(&s, "part_made"),
        "Made long-throw drill · −8 iron ore, −4 refined plate; it's in the Wren's locker"
    );
    // Made once: it's the player's now.
    assert_eq!(
        refused(
            &s,
            ServiceRequest::Make {
                upgrade: LONG_DRILL,
                fit: true
            }
        ),
        Refusal::Fit(FitRefusal::InLocker)
    );
    // Swift fins: the blueprint is in a crate at Brassmouth; make and fit at once.
    dock(&mut s, &BRASSMOUTH);
    run(&mut s, 1);
    dock(&mut s, &WHARF);
    cargo(&mut s, &PLATE, 4);
    service(
        &mut s,
        ServiceRequest::Make {
            upgrade: FINS,
            fit: true,
        },
    );
    assert!(s.world().player.fittings.contains(&FINS));
    assert_eq!(
        last_note(&s, "part_made"),
        "Made and fitted swift fins · −4 refined plate"
    );
}

#[test]
fn a_friend_sends_a_part_after_the_next_docking_at_their_station() {
    let mut s = at_the_wharf();
    let order = ServiceRequest::Order { upgrade: LAMP };
    assert_eq!(refused(&s, order.clone()), Refusal::Locked);
    // Offered once the player has met Hesper at the Observatory.
    dock(&mut s, &OBSERVATORY);
    dock(&mut s, &WHARF);
    let before = s.world().player.credits;
    service(&mut s, order.clone());
    assert_eq!(s.world().player.credits, before - 90_000);
    assert_eq!(
        last_note(&s, "part_ordered"),
        "Ordered seam lamp from Hesper at Glass Drift Observatory · −900 cr; it comes after your next docking there"
    );
    let transit = hullwright::in_transit(s.world());
    assert_eq!(transit.len(), 1);
    assert_eq!(transit[0].from_name, "Hesper Lune");
    assert_eq!(refused(&s, order), Refusal::Fit(FitRefusal::OnOrder));

    // A docking elsewhere doesn't bring it; the next one at the Observatory does.
    dock(&mut s, &KETTLE);
    run(&mut s, 2);
    assert!(locker(&s).is_empty());
    dock(&mut s, &OBSERVATORY);
    run(&mut s, 1);
    assert_eq!(locker(&s), vec![LAMP]);
    assert!(hullwright::in_transit(s.world()).is_empty());
    assert_eq!(
        last_note(&s, "part_arrived"),
        "Hesper sent the seam lamp; it's in the Wren's locker"
    );
    // Fitted at the Wharf, it reads what's left in nearby rocks.
    dock(&mut s, &WHARF);
    service(&mut s, ServiceRequest::Refit { upgrade: LAMP });
    assert_eq!(
        galaxy_contract::modifiers::for_ship(s.world(), &PLAYER).seam_lamp,
        Some(300.0)
    );
    let looks = fit::looks(s.world(), &PLAYER);
    assert_eq!(looks.len(), 1);
    assert_eq!(
        (looks[0].look.as_str(), looks[0].place.as_str()),
        ("seam_lamp", "utility")
    );
}

#[test]
fn the_locker_and_fitted_parts_move_with_the_hull_at_prestige() {
    let mut s = at_the_wharf();
    service(&mut s, ServiceRequest::Fit { upgrade: LENS });
    service(&mut s, ServiceRequest::Fit { upgrade: PODS });
    service(&mut s, ServiceRequest::Remove { upgrade: LENS });
    set_beat(
        &mut s,
        "beat.b16_hullwrights_question",
        BeatStatus::Active,
        0,
    );
    credits(&mut s, 900_000);
    assert!(fit::left_at_prestige(s.world(), &TANDEM).is_empty());
    service(&mut s, ServiceRequest::Prestige { to: TANDEM });
    let w = s.world();
    assert_eq!(fit::flagship(w).unwrap().hull, TANDEM);
    assert!(w.player.fittings.contains(&PODS));
    assert_eq!(locker(&s), vec![LENS]);
    // The berthed Wren has no locker of its own.
    assert_eq!(s.world().progress.fit.lockers.len(), 1);
    // The Tandem carries six free: the pods weigh nothing extra on it.
    assert_eq!(hullwright::budget(s.world()).unwrap().excess_mass(), 0);
}

#[test]
fn a_save_from_before_the_fit_model_loads_with_nothing_lost() {
    // The world of a save the Phase 2 build wrote at beat 14 (`galaxy beat jump 14`).
    let text = include_str!("saves/b14_world.json");
    let world: WorldState = serde_json::from_str(text).expect("the old world loads");
    assert!(world.progress.rules.fit.is_empty());
    let fittings = world.player.fittings.clone();
    assert!(fittings.contains(&LENS) && fittings.contains(&EXTENDER));
    let ship = world.ships.get(&PLAYER).unwrap().clone();
    let hold = galaxy_contract::modifiers::hold_capacity(&world, &ship);
    let mods = galaxy_contract::modifiers::for_ship(&world, &PLAYER);
    let credits = world.player.credits;

    let mut s = galaxy_core::Sim::new(galaxy_core::SimState::new(world));
    s.add_system(Box::new(galaxy_progress::ProgressSystem::default()));
    // The fit model's backfill, with the station parts', the authored happenings' and
    // the pacing tunables added since that build.
    assert_eq!(
        galaxy_core::queue_backfill(&mut s, &core_data()).unwrap(),
        4
    );
    // A second open before it applied doesn't queue it again.
    assert_eq!(
        galaxy_core::queue_backfill(&mut s, &core_data()).unwrap(),
        0
    );
    run(&mut s, 2);

    let w = s.world();
    assert!(!w.progress.rules.fit.is_empty());
    assert_eq!(w.player.fittings, fittings, "nothing fitted is lost");
    assert_eq!(w.player.credits, credits);
    let ship = w.ships.get(&PLAYER).unwrap();
    assert_eq!(galaxy_contract::modifiers::hold_capacity(w, ship), hold);
    let after = galaxy_contract::modifiers::for_ship(w, &PLAYER);
    assert_eq!(
        (after.mining_rate, after.speed, after.turn_rate),
        (mods.mining_rate, mods.speed, mods.turn_rate),
        "the lens and extender weigh 2 of the Wren's 3 free"
    );
    // Each part in its socket, every control module in the rack.
    let station = ship.docked_at.clone().unwrap_or(WHARF);
    let sockets = hullwright::sockets(w, s.history(), &station, None);
    let at = |id: &SocketId| {
        sockets
            .iter()
            .find(|x| &x.socket == id)
            .map(|x| x.fitted.clone())
            .unwrap()
    };
    assert_eq!(at(&DRILL), vec![LENS]);
    assert_eq!(at(&HOLD), vec![EXTENDER]);
    let rack = at(&RACK);
    assert_eq!(rack.len(), fittings.len() - 2);
    assert!(rack.contains(&NAV));
    // Up to date now: the next open queues nothing.
    assert_eq!(
        galaxy_core::queue_backfill(&mut s, &core_data()).unwrap(),
        0
    );
}
