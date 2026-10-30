//! Regressions for the PR #21 review, through the script host: predicates that read
//! position and field depletion (b10, b12), repeated standing deeds in one call, and a
//! goal delivery that an undock earlier in the tick must reject.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use galaxy_contract::prelude::*;
use galaxy_core::Sim;
use galaxy_progress::ProgressSystem;
use galaxy_script::ScriptHost;

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const BRASS: StationId = StationId::from_static("station.brassmouth_exchange");
const COMPACT: FactionId = FactionId::from_static("faction.tollway_compact");
const VIEWS: &str = "beat.t03_views";

fn review_pack() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/review_probe")
}

fn review_sim(seed: u64) -> (Sim, ScriptHost) {
    let mut sim = new_sim(seed);
    let host = ScriptHost::new(None);
    let report = host.load(&review_pack(), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    sim.add_system(Box::new(ProgressSystem));
    (sim, host)
}

fn act(sim: &mut Sim, commands: Vec<Command>) {
    sim.submit(CommandBatch {
        cause: Cause::system("test_player", "the player acts"),
        commands,
    });
    step(sim, 1);
}

fn dock_at(sim: &mut Sim, station: &StationId) {
    let region = sim.world().stations.get(station).unwrap().region.clone();
    act(
        sim,
        vec![
            Command::SetShipRegion {
                ship: PLAYER,
                region,
            },
            Command::SetShipDocked {
                ship: PLAYER,
                station: Some(station.clone()),
            },
            Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: station.clone(),
                },
            },
        ],
    );
}

fn facts(sim: &Sim, kind: &str) -> Vec<Fact> {
    sim.history()
        .find(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
        .into_iter()
        .cloned()
        .collect()
}

fn beat_status(sim: &Sim) -> Option<BeatStatus> {
    sim.world().progress.beats.get(VIEWS).map(|b| b.status)
}

#[test]
fn predicates_read_the_ships_position_and_a_fields_depletion() {
    let (mut sim, _host) = review_sim(11);
    step(&mut sim, 2);
    assert_eq!(beat_status(&sim), Some(BeatStatus::Active));

    // Thin the Shallows ice to 70% of full: not done, the ship is far away.
    let thin: Vec<Command> = sim
        .world()
        .rocks
        .values()
        .filter(|r| r.field.as_str() == "field.shallows.ice")
        .map(|r| Command::AdjustRock {
            rock: r.id.clone(),
            delta: i64::from(r.initial * 7 / 10) - i64::from(r.remaining),
        })
        .collect();
    assert!(!thin.is_empty());
    act(&mut sim, thin);
    step(&mut sim, 2);
    assert_eq!(beat_status(&sim), Some(BeatStatus::Active));

    // The right spot in the wrong region doesn't count.
    let near = Vec2::new(5900.0, 1450.0);
    act(
        &mut sim,
        vec![
            Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            },
            Command::SetShipMotion {
                ship: PLAYER,
                position: near,
                velocity: Vec2::ZERO,
                heading: 0.0,
            },
        ],
    );
    step(&mut sim, 2);
    assert_eq!(beat_status(&sim), Some(BeatStatus::Active));

    // Within 500 u of (5950, 1500) in Cinder Drift, with the field thin: done.
    act(
        &mut sim,
        vec![Command::SetShipRegion {
            ship: PLAYER,
            region: RegionId::from_static("region.cinder_drift"),
        }],
    );
    step(&mut sim, 2);
    assert_eq!(beat_status(&sim), Some(BeatStatus::Completed));
}

#[test]
fn the_pilot_rock_and_distance_views_read_the_world() {
    let (mut sim, _host) = review_sim(12);
    dock_at(&mut sim, &WHARF);
    step(&mut sim, 1);
    let views = facts(&sim, "review.views");
    assert_eq!(views.len(), 1);
    let FactPayload::Note(note) = &views[0].payload else {
        panic!("a note fact");
    };
    let w = sim.world();
    let ship = w.ships.get(&PLAYER).unwrap();
    let rock = w.rocks.get("rock.shallows.ice.03").unwrap();
    let kettle = w.stations.get(&KETTLE).unwrap();
    let (pos, rest) = note.split_once(' ').unwrap();
    let (x, y) = pos.split_once(',').unwrap();
    assert!(
        (x.parse::<f32>().unwrap() - ship.position.x).abs() <= 0.5,
        "{note}"
    );
    assert!(
        (y.parse::<f32>().unwrap() - ship.position.y).abs() <= 0.5,
        "{note}"
    );
    assert!(
        rest.starts_with(&format!(
            "rock={}/{} field={} ",
            rock.remaining, rock.initial, rock.field
        )),
        "{note}"
    );
    let d: f32 = rest.rsplit_once("distance=").unwrap().1.parse().unwrap();
    assert!(
        (d - ship.position.distance(kettle.position)).abs() <= 0.5,
        "{note}"
    );
}

#[test]
fn two_deeds_in_one_call_stop_at_the_top_of_the_range() {
    let (mut sim, _host) = review_sim(13);
    let start = sim.world().factions.get(&COMPACT).unwrap().standing;
    act(
        &mut sim,
        vec![Command::AdjustStanding {
            faction: COMPACT,
            delta: 90 - start,
        }],
    );
    dock_at(&mut sim, &BRASS);
    step(&mut sim, 1);
    assert_eq!(sim.world().factions.get(&COMPACT).unwrap().standing, 100);
    let deltas: Vec<Option<i64>> = facts(&sim, "standing_changed")
        .iter()
        .filter(|f| f.subjects.faction.as_ref() == Some(&COMPACT))
        .map(|f| f.amount)
        .collect();
    assert_eq!(
        deltas,
        vec![Some(8), Some(2)],
        "the second deed sees the first"
    );
}

#[test]
fn a_delivery_after_an_undock_in_the_same_tick_is_rejected() {
    let (mut sim, _host) = review_sim(14);
    act(
        &mut sim,
        vec![Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 5,
        }],
    );
    dock_at(&mut sim, &KETTLE);
    // The handler for that docking runs this tick, but the player undocks first.
    sim.submit(CommandBatch {
        cause: Cause::system("test_player", "undock"),
        commands: vec![Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        }],
    });
    let r = sim.step(&PlayerInput::default()).unwrap();
    assert_eq!(r.rejected.len(), 1, "{:#?}", r.rejected);
    assert!(matches!(r.rejected[0].cause.kind, CauseKind::Script { .. }));
    let ship = sim.world().ships.get(&PLAYER).unwrap();
    assert_eq!(ship.hold.count(&ICE), 5, "the cargo stays aboard");
    assert!(facts(&sim, "deliver").is_empty());

    // Docked for the whole tick, it goes through.
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    assert_eq!(sim.world().ships.get(&PLAYER).unwrap().hold.count(&ICE), 0);
    assert_eq!(facts(&sim, "deliver").len(), 1);
}
