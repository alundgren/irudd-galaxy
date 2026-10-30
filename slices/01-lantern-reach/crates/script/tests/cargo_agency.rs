//! Issue #47, Phase 1: cargo agency and the exploits.
//!
//! - #21: docking at Kettle with ice hands nothing over; Odo asks, and only Deliver
//!   moves the ice, with a line saying what went and what came back.
//! - #30, #32: Ilse's plate is an offer the player accepts, rides in the hold as hers,
//!   can't be sold, and b07 completes only when it reaches Rook.
//! - #33: the lane chart is offered on Rook's list, never fitted for the player, and
//!   whoever fits a part says so in the first person with its literal effect.
//! - #31: tapping out of a ring and straight back in is a docking, but not a counted
//!   one: no beat advances and price-snapshot ages don't move.
//! - The principle: a script changes the player's hold, credits or fitted parts only
//!   in a batch that cites the player's reply (`galaxy_script::confirm`), checked both
//!   by the host's guard and by sweeping the journal after play.

mod common;

use std::path::Path;

use common::pilot::*;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::Sim;
use galaxy_script::host::HostEvent;
use galaxy_script::{CheckOptions, ScriptHost, check_dir};

const LENS: &str = "upgrade.mining_lens";
const NAV_CHIP: &str = "upgrade.nav_chip";
const LANE_CHART: &str = "upgrade.lane_chart";
const CONSIGNMENT: LedgerId = LedgerId::from_static("ledger.hook_consignment");
const B03: &str = "beat.b03_first_delivery";
const B07: &str = "beat.b07_cinder_drift";
const B08: &str = "beat.b08_chasing_a_gap";
const B09: &str = "beat.b09_hook_dispute";

const ACT_ONE: [&str; 5] = [
    "beat.b01_undock",
    "beat.b02_mine_by_hand",
    "beat.b03_first_delivery",
    "beat.b04_first_sale",
    "beat.b05_pause",
];

/// Mines `field` by hand until the hold is full.
fn fill_by_hand(p: &mut Pilot, field: &FieldId) {
    while p.hold_free() > 0 {
        let rock = p.richest(field).expect("an unspent rock");
        p.mine_by_hand(&rock);
    }
}

/// Whether someone at the dock offered `choice`.
fn offered(p: &Pilot, npc: &str, choice: &str) -> bool {
    p.talk().iter().any(|l| {
        l.line.speaker.as_ref().is_some_and(|s| s.as_str() == npc)
            && l.line.choices.iter().any(|c| c.id.as_str() == choice)
    })
}

/// The confirmation notices said at `place` (dock talk with a place and no speaker).
fn notices(p: &Pilot, place: &StationId) -> Vec<String> {
    p.talk()
        .iter()
        .filter(|l| l.line.speaker.is_none() && l.line.place.as_ref() == Some(place))
        .map(|l| l.line.text.clone())
        .collect()
}

/// Every committed script batch that changed the player's hold, credits or fittings
/// without citing the player's reply (or, for cargo found, their own mining): what
/// the principle forbids, swept from the journal after the fact.
fn unconfirmed(sim: &Sim) -> Vec<String> {
    let h = sim.history();
    let mut out = Vec::new();
    for entry in sim.journal() {
        let Some(rc) = h.causes().get(&entry.cause) else {
            continue;
        };
        if !matches!(rc.cause.kind, CauseKind::Script { .. }) {
            continue;
        }
        let touches = entry.undo.iter().any(|u| {
            matches!(u, UndoRecord::Invert { command } if matches!(
                command,
                Command::AdjustCargo { .. }
                    | Command::AdjustCredits { .. }
                    | Command::SetConsignment { .. }
                    | Command::SetFitting { .. }
            ))
        });
        if !touches {
            continue;
        }
        let confirmed = rc.cause.cites.iter().any(|f| {
            h.fact(*f).is_some_and(|f| {
                matches!(
                    f.payload,
                    FactPayload::Event(
                        Event::Reply { .. } | Event::Mine { .. } | Event::RockDepleted { .. }
                    )
                )
            })
        });
        if !confirmed {
            out.push(format!("{}: {}", rc.tick.0, rc.cause.summary));
        }
    }
    out
}

#[test]
fn ice_stays_aboard_until_the_player_picks_deliver_and_a_line_says_what_came_back() {
    let mut p = Pilot::new(11);
    p.idle(2);
    p.undock();
    fill_by_hand(&mut p, &RIMEWATER);
    p.idle(2);
    let credits = p.credits();

    // #21: docking hands nothing over. Odo asks: Deliver, Sell or Keep.
    p.dock_by_hand(&KETTLE);
    p.idle(4);
    assert_eq!(p.cargo(&ICE), 20, "docking took no ice");
    assert!(!p.is_done(B03));
    for choice in [
        "choice.deliver_ice",
        "choice.sell_ice",
        "choice.keep_ice",
    ] {
        assert!(offered(&p, "npc.odo_fenn", choice), "{choice} offered");
    }

    // Keep: nothing moves.
    p.reply("npc.odo_fenn", "choice.keep_ice");
    p.idle(2);
    assert_eq!(p.cargo(&ICE), 20);
    assert_eq!(p.credits(), credits);

    // Deliver: the ice goes, the fee comes back in the same transaction, and a line
    // says both.
    p.reply("npc.odo_fenn", "choice.deliver_ice");
    p.idle(3);
    assert_eq!(p.cargo(&ICE), 0);
    assert_eq!(p.credits(), credits + 120);
    assert!(p.is_done(B03));
    let said = notices(&p, &KETTLE);
    assert!(
        said.iter().any(|t| t == "Delivered 20 ice to Odo · +120 cr"),
        "{said:?}"
    );
    assert!(p.heard("npc.odo_fenn", "20 ice. The beans will hear about this."));
    assert_eq!(unconfirmed(&p.sim), Vec::<String>::new());
    p.assert_healthy();
}

#[test]
fn a_part_delivery_says_how_much_is_still_to_go_and_pays_only_when_full() {
    let mut p = Pilot::new(5);
    p.idle(2);
    p.undock();
    // Ten units in the hold, then to Kettle.
    while p.cargo(&ICE) < 10 {
        let rock = p.richest(&RIMEWATER).expect("ice");
        p.mine_by_hand(&rock);
    }
    let extra = p.cargo(&ICE) - 10;
    p.dock_by_hand(&KETTLE);
    p.sell(&ICE, extra);
    let credits = p.credits();
    p.reply("npc.odo_fenn", "choice.deliver_ice");
    p.idle(3);
    assert_eq!(p.cargo(&ICE), 0);
    assert!(!p.is_done(B03));
    let said = notices(&p, &KETTLE);
    assert!(
        said.iter().any(|t| t == "Delivered 10 ice to Odo · 10 more to go"),
        "{said:?}"
    );
    // No fee yet: it comes with the delivery that fills the greenhouse.
    assert_eq!(p.credits(), credits);
    assert_eq!(unconfirmed(&p.sim), Vec::<String>::new());
}

/// Act 2 from b06's end, docked at the Wharf.
fn at_b07(seed: u64) -> Pilot {
    let mut p = Pilot::with(seed, |s| {
        let mut done = ACT_ONE.to_vec();
        done.push("beat.b06_nav_chip");
        s.done(&done);
        s.fitted(&[LENS, NAV_CHIP]);
        s.counter(CounterKind::Dockings, 6);
        s.credits(4000);
    });
    p.idle(3);
    p
}

#[test]
fn ilses_plate_is_offered_carried_as_hers_unsellable_and_counts_only_at_rook() {
    let mut p = at_b07(21);
    assert_eq!(p.step_of(B07), Some(0));
    assert!(p.ship().hold.consigned.is_empty(), "nothing loaded unasked");
    assert!(offered(&p, "npc.ilse_marrow", "choice.accept_consignment"));

    // Not now: nothing loads.
    p.reply("npc.ilse_marrow", "choice.decline_consignment");
    p.idle(2);
    assert!(p.ship().hold.consigned.is_empty());

    // Accept: the plate goes aboard as Ilse's, and a line says so.
    p.reply("npc.ilse_marrow", "choice.accept_consignment");
    p.idle(3);
    let c = p
        .ship()
        .hold
        .consigned
        .get(&CONSIGNMENT)
        .expect("Ilse's plate aboard")
        .clone();
    assert_eq!(c.ware, PLATE);
    assert_eq!(c.quantity, 20);
    assert_eq!(c.owner.as_str(), "npc.ilse_marrow");
    assert_eq!(c.to.as_str(), "npc.rook_adeyemi");
    assert_eq!(p.cargo(&PLATE), 0, "the pilot owns none of it");
    assert_eq!(p.ship().hold.total(), 20, "it fills the hold");
    assert_eq!(p.step_of(B07), Some(1));
    assert!(
        notices(&p, &WHARF)
            .iter()
            .any(|t| t.starts_with("Loaded Ilse's 20 plate for Rook"))
    );

    // #32: the market won't buy it, here or anywhere.
    let credits = p.credits();
    p.sell(&PLATE, 20);
    assert!(p.facts("sell").iter().all(|f| f.subjects.ware.as_ref() != Some(&PLATE)));
    assert_eq!(p.credits(), credits);
    assert!(p.ship().hold.consigned.contains_key(&CONSIGNMENT));

    // To the Hook. Rook asks; Not yet keeps it aboard and b07 open.
    let lane: LaneId = "lane.shallows_cinder".parse().unwrap();
    p.ride_lane_by_hand(&lane);
    p.go(&HOOK);
    p.idle(3);
    assert!(offered(&p, "npc.rook_adeyemi", "choice.deliver_plate"));
    p.reply("npc.rook_adeyemi", "choice.keep_plate");
    p.idle(3);
    assert!(!p.is_done(B07));
    assert!(p.ship().hold.consigned.contains_key(&CONSIGNMENT));

    // Deliver: the consignment reaches Rook, the fee comes back, b07 completes.
    p.reply("npc.rook_adeyemi", "choice.deliver_plate");
    p.idle(3);
    assert!(p.ship().hold.consigned.is_empty());
    assert_eq!(p.credits(), credits + 250);
    assert!(p.is_done(B07));
    assert!(
        notices(&p, &HOOK)
            .iter()
            .any(|t| t == "Delivered Ilse's 20 plate to Rook · +250 cr"),
        "{:?}",
        notices(&p, &HOOK)
    );

    // #33: the lane chart is offered, not fitted. Fitting it is the player's act, and
    // Rook says so in the first person with its literal effect.
    assert!(!p.world().player.fittings.contains(LANE_CHART));
    assert!(p.heard("npc.rook_adeyemi", "a lane chart"));
    p.fit(LANE_CHART);
    p.idle(3);
    assert!(p.world().player.flags.contains("cap.map_travel"));
    assert!(p.heard(
        "npc.rook_adeyemi",
        "There: I've fitted your lane chart. Click a place on the map and your ship goes there."
    ));
    assert_eq!(unconfirmed(&p.sim), Vec::<String>::new());
    p.assert_healthy();
}

#[test]
fn twenty_plate_of_the_players_own_never_completes_b07() {
    let mut p = at_b07(4);
    // The player buys their own plate at the Wharf and takes it to Rook unasked.
    p.buy(&PLATE, 20);
    let own = p.cargo(&PLATE);
    assert!(own > 0, "bought plate");
    let lane: LaneId = "lane.shallows_cinder".parse().unwrap();
    p.ride_lane_by_hand(&lane);
    p.go(&HOOK);
    p.idle(4);
    assert!(!offered(&p, "npc.rook_adeyemi", "choice.deliver_plate"));
    assert_eq!(p.cargo(&PLATE), own, "Rook takes nothing he wasn't sent");
    assert_eq!(p.step_of(B07), Some(0));
    assert!(!p.has_fact("core.consignment_delivered"));
}

/// #31: the issue's repro. Docked at Cinder Hook with b08 active, tap out of the ring
/// and back in three times (`w*1,wait*2,space,wait*3`): every tap is a docking, none
/// is counted. b09 doesn't open early, and the map's "seen N dockings ago" stays put.
#[test]
fn tapping_out_of_the_ring_and_back_counts_no_docking() {
    let mut p = Pilot::with(9, |s| {
        let mut done = ACT_ONE.to_vec();
        done.extend(["beat.b06_nav_chip", "beat.b07_cinder_drift"]);
        s.done(&done);
        s.fitted(&[LENS, NAV_CHIP, LANE_CHART]);
        s.flags(&["cap.click_fly", "cap.map_travel"]);
        s.counter(CounterKind::Dockings, 12);
        // Two Hook dockings so far: b09 opens on the third.
        s.station_counter(CounterKind::Dockings, &HOOK, 2);
        s.docked_at(&HOOK);
        let w = s.world();
        // Docked at the Hook through a real docking: the economy tracks it.
        w.economy.docks.insert(
            PLAYER,
            DockTrack {
                station: HOOK,
                away: false,
            },
        );
        for (station, visit) in [(WHARF, 11), (KETTLE, 10)] {
            w.player.price_snapshots.insert(
                station,
                PriceSnapshot {
                    tick: Tick(0),
                    visit,
                    prices: Default::default(),
                },
            );
        }
    });
    p.idle(3);
    assert_eq!(p.status(B08), Some(BeatStatus::Active));
    let ages = |p: &Pilot| {
        [WHARF, KETTLE].map(|s| {
            let snap = p.world().player.price_snapshots.get(&s).unwrap();
            galaxy_economy::scout::describe_age(galaxy_economy::scout::dockings_ago(
                p.world(),
                snap,
            ))
        })
    };
    let before = ages(&p);
    assert_eq!(before, ["seen 1 docking ago", "seen 2 dockings ago"]);
    let docks = p.facts("dock").len();
    let dockings = |p: &Pilot| {
        let c = &p.world().counters;
        (
            c.get(CounterKind::Dockings, &CounterScope::Player),
            c.get(
                CounterKind::Dockings,
                &CounterScope::Station { station: HOOK },
            ),
            c.get(
                CounterKind::Dockings,
                &CounterScope::Region {
                    region: CINDER.clone(),
                },
            ),
        )
    };
    let counted = dockings(&p);

    for _ in 0..3 {
        p.undock();
        p.idle(1);
        p.press();
        p.idle(3);
        assert_eq!(p.docked(), Some(&HOOK), "re-docked on the spot");
    }
    assert_eq!(p.facts("dock").len(), docks + 3, "every tap was a docking");
    assert!(
        p.facts("dock")
            .iter()
            .skip(docks)
            .all(|f| f.amount == Some(0)),
        "none of them counted"
    );
    assert_eq!(dockings(&p), counted, "no docking counted");
    assert_eq!(p.status(B09), None, "b09 didn't open early");
    assert_eq!(ages(&p), before, "staleness didn't move");

    // A real trip away and back counts again.
    p.undock();
    let hook = p.world().stations.get(&HOOK).unwrap().position;
    p.fly_to(hook + Vec2::new(400.0, 0.0), 20.0);
    p.idle(3);
    p.dock_by_hand(&HOOK);
    p.idle(3);
    assert_eq!(
        p.world().counters.get(
            CounterKind::Dockings,
            &CounterScope::Station { station: HOOK }
        ),
        3
    );
    assert_eq!(p.status(B09), Some(BeatStatus::Active));
    assert_eq!(ages(&p), ["seen 2 dockings ago", "seen 3 dockings ago"]);
}

// --- The host's guard ------------------------------------------------------------

fn write_probe(dir: &Path, script: &str, beat_reward: bool) {
    std::fs::write(
        dir.join("manifest.toml"),
        "id = \"probe\"\nname = \"Probe\"\nversion = \"0.1.0\"\n\
         uses = [\"beats.read\", \"beats.write\", \"events.dock\", \"events.reply\"]\n\
         [[script]]\nid = \"main\"\nfile = \"scripts/main.luau\"\n\
         handlers = [\"on.dock\", \"on.reply\"]\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    let beats = if beat_reward {
        "beats = { G.beat({ id = \"beat.probe_pay\", act = 1, goal = \"Dock.\", \
         done = G.docked(), rewards = { credits = 5 } }) },"
    } else {
        ""
    };
    std::fs::write(
        dir.join("scripts/main.luau"),
        format!(
            "--!strict\nlocal G = require(\"@galaxy\")\n\
             return G.script({{ id = \"main\", {beats} on = {{\n{script}\n}} }})\n"
        ),
    )
    .unwrap();
}

fn probe_sim(dir: &Path) -> (Sim, ScriptHost) {
    let mut sim = common::new_sim(1);
    let host = ScriptHost::new(None);
    host.load_unchecked(dir).unwrap();
    host.attach(&mut sim);
    (sim, host)
}

fn aborts(host: &ScriptHost) -> String {
    host.take_events()
        .into_iter()
        .filter(|e| matches!(e, HostEvent::Aborted { .. }))
        .map(|e| format!("{e:?}"))
        .collect::<Vec<_>>()
        .join("\n")
}

const PAY_ON_BOTH: &str = "\
    dock = function(ev: G.DockEvent, ctx: G.Ctx)\n\
        ctx.beats:pay(5, ctx:cause({ summary = \"paid for docking\", cites = { ev.fact } }))\n\
    end,\n\
    reply = function(ev: G.ReplyEvent, ctx: G.Ctx)\n\
        ctx.beats:pay(5, ctx:cause({ summary = \"paid on the player's say-so\", cites = { ev.fact } }))\n\
    end,";

#[test]
fn the_host_refuses_a_script_paying_without_the_players_reply() {
    let dir = tempfile::tempdir().unwrap();
    write_probe(dir.path(), PAY_ON_BOTH, false);
    let (mut sim, host) = probe_sim(dir.path());
    let credits = sim.world().player.credits;

    // A docking is not the player agreeing to anything: the write is a fault.
    sim.submit(
        CommandBatch::new(Cause::system("test", "dock"))
            .with(Command::SetShipDocked {
                ship: PLAYER,
                station: Some(KETTLE),
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: KETTLE,
                },
            }),
    );
    common::step(&mut sim, 3);
    assert_eq!(sim.world().player.credits, credits, "nothing paid");
    let why = aborts(&host);
    assert!(
        why.contains("unconfirmed_change") && why.contains("`adjust_credits`"),
        "{why}"
    );

    // The player's reply is: the same write applies.
    sim.submit(
        CommandBatch::new(Cause::system("test", "the player answers")).with(Command::Emit {
            event: Event::Reply {
                ship: PLAYER,
                npc: "npc.odo_fenn".parse().unwrap(),
                station: KETTLE,
                choice: "choice.probe".parse().unwrap(),
            },
        }),
    );
    common::step(&mut sim, 3);
    assert_eq!(sim.world().player.credits, credits + 500);
    assert_eq!(aborts(&host), "");
    assert_eq!(unconfirmed(&sim), Vec::<String>::new());
}

#[test]
fn pack_check_refuses_a_beat_that_pays_credits_as_a_reward() {
    let dir = tempfile::tempdir().unwrap();
    write_probe(dir.path(), PAY_ON_BOTH, true);
    let sim = common::new_sim(1);
    let r = check_dir(
        dir.path(),
        CheckOptions {
            world: Some(sim.world()),
            history: Some(sim.history()),
            ..Default::default()
        },
    )
    .report;
    let errors = format!("{:?}", r.errors().collect::<Vec<_>>());
    assert!(
        !r.accepted() && errors.contains("unconfirmed_change"),
        "{errors}"
    );
}

/// Every line said at a dock in the core journey's first two acts is in the first
/// person: nobody names themselves ("Odo's greenhouse needs ..." from Odo) or prefixes
/// their own name ("Juno: ...").
#[test]
fn people_never_speak_of_themselves_in_the_third_person() {
    let mut p = at_b07(21);
    p.reply("npc.ilse_marrow", "choice.accept_consignment");
    p.idle(3);
    let lane: LaneId = "lane.shallows_cinder".parse().unwrap();
    p.ride_lane_by_hand(&lane);
    p.go(&HOOK);
    p.idle(3);
    p.reply("npc.rook_adeyemi", "choice.deliver_plate");
    p.idle(3);
    p.fit(LANE_CHART);
    p.idle(3);
    let bad = third_person(p.world());
    assert!(bad.is_empty(), "{bad:#?}");
}

/// Lines whose speaker names themselves: their first name, other than "I'm <name>".
fn third_person(world: &WorldState) -> Vec<String> {
    world
        .talk
        .lines
        .values()
        .filter(|l| l.line.channel == Channel::DockTalk)
        .filter_map(|l| {
            let npc = world.npcs.get(l.line.speaker.as_ref()?)?;
            let first = npc.name.split_whitespace().next()?;
            let text = l.line.text.replace(&format!("I'm {first}"), "");
            text.contains(first).then(|| format!("{}: {}", npc.name, l.line.text))
        })
        .collect()
}

/// Goal lines state the real amounts from `pacing.toml` (#46: b08's "twice"), and b05's
/// is a literal hint.
#[test]
fn goal_lines_state_the_amounts_pacing_asks_for() {
    let world = galaxy_core::new_game(&common::core_data(), 1)
        .expect("core data loads")
        .world;
    let n = |id: &str| {
        world.progress.tunables[id]
            .value
            .as_number()
            .unwrap_or_else(|| panic!("{id}"))
    };
    let act1 = std::fs::read_to_string(common::slice().join("packs/core/scripts/act1.luau")).unwrap();
    let act2 = std::fs::read_to_string(common::slice().join("packs/core/scripts/act2.luau")).unwrap();
    assert!(act1.contains("goal = \"P or Esc · pause\""));
    assert!(act2.contains(&format!(
        "Sell {} there, over {} separate visits.",
        n("b08.glassite"),
        n("b08.dockings")
    )));
    assert!(act2.contains(&format!("carry {} plate to Rook", n("b07.plate"))));
    assert!(act2.contains(&format!("Carry Ilse's {} plate to Rook", n("b07.plate"))));
}
