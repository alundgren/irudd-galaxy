//! Act 1's first beats end to end, headless: the game's composition (ship systems with
//! the economy desk, the economy, progress) plus the script host running the core
//! pack, flown by input like a player would. b01 to b03 complete from what the ship
//! and economy systems record, and the mining lens they unlock is bought at the Wharf.

mod common;

use common::{core_data, slice};
use galaxy_contract::prelude::*;
use galaxy_core::{Sim, new_game};
use galaxy_progress::{ProgressSystem, beats, services};
use galaxy_script::ScriptHost;

const PLAYER: ShipId = ShipId::from_static("ship.player");
const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const ICE: WareId = WareId::from_static("ware.ice");
const ICE_ROCK: RockId = RockId::from_static("rock.shallows.ice.03");
const LENS: UpgradeId = UpgradeId::from_static("upgrade.mining_lens");

fn game(seed: u64) -> (Sim, ScriptHost) {
    let mut sim = galaxy_core::game(new_game(&core_data(), seed).unwrap())
        .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
        .with_systems([
            Box::new(galaxy_economy::EconomySystem) as Box<dyn galaxy_core::System>,
            Box::new(ProgressSystem::default()),
        ]);
    let host = ScriptHost::new(None);
    let report = host.load(&slice().join("packs/core"), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    (sim, host)
}

fn step(sim: &mut Sim, input: &PlayerInput) {
    let report = sim.step(input).expect("not paused");
    assert!(report.rejected.is_empty(), "{:#?}", report.rejected);
}

fn hands_off(sim: &mut Sim, ticks: u32) {
    for _ in 0..ticks {
        step(sim, &PlayerInput::default());
    }
}

fn space(sim: &mut Sim) {
    step(
        sim,
        &PlayerInput {
            action: true,
            ..PlayerInput::default()
        },
    );
}

fn player(sim: &Sim) -> &Ship {
    sim.world().ships.get(&PLAYER).unwrap()
}

fn status(sim: &Sim, beat: &str) -> Option<BeatStatus> {
    sim.world().progress.beats.get(beat).map(|b| b.status)
}

fn fly(sim: &mut Sim, to: Vec2, arrived: impl Fn(&Sim) -> bool) {
    if player(sim).docked_at.is_some() {
        // Fly off first: turning alone doesn't undock.
        step(
            sim,
            &PlayerInput {
                thrust: Vec2::new(0.0, 1.0),
                ..PlayerInput::default()
            },
        );
    }
    for _ in 0..3000 {
        if arrived(sim) {
            break;
        }
        let ship = player(sim);
        let feel = galaxy_ship::flight::Feel::of_ship(sim.world(), ship).unwrap();
        let keys =
            galaxy_ship::flight::steer_to(&feel, galaxy_ship::flight::Motion::of(ship), to, 4.0);
        step(
            sim,
            &PlayerInput {
                thrust: keys,
                ..PlayerInput::default()
            },
        );
    }
    assert!(arrived(sim), "didn't get there: {:?}", player(sim).position);
    for _ in 0..60 {
        if player(sim).velocity == Vec2::ZERO {
            return;
        }
        hands_off(sim, 1);
    }
}

fn mine_ice(sim: &mut Sim) {
    let rock = sim.world().rocks.get(&ICE_ROCK).unwrap().clone();
    let radius = galaxy_ship::space::rock_radius(sim.world(), &rock);
    let from = player(sim).position;
    let spot = rock.position + (from - rock.position).normalize_or_zero() * (radius + 15.0);
    fly(sim, spot, |s| {
        let r = s.world().rocks.get(&ICE_ROCK).unwrap();
        galaxy_ship::space::in_reach(s.world(), r, player(s).position)
            && player(s).position.distance(spot) < 10.0
    });
    let cap = sim
        .world()
        .hulls
        .get(&player(sim).hull)
        .unwrap()
        .hold_capacity;
    for _ in 0..3000 {
        if player(sim).hold.total() >= u64::from(cap) {
            break;
        }
        space(sim);
    }
    hands_off(sim, 2); // let go of Space: the run ends and writes its `mine` fact
}

fn dock_at(sim: &mut Sim, station: &StationId) {
    let s = sim.world().stations.get(station).unwrap().clone();
    fly(sim, s.position, |w| {
        player(w).position.distance(s.position) < s.dock_radius - 10.0
    });
    space(sim);
    assert_eq!(player(sim).docked_at.as_ref(), Some(station));
    hands_off(sim, 4);
}

#[test]
fn act_one_beats_complete_from_real_flight_and_the_lens_is_bought() {
    let (mut sim, _host) = game(11);
    hands_off(&mut sim, 2);
    assert_eq!(status(&sim, "beat.b01_undock"), Some(BeatStatus::Active));
    assert!(
        beats::hud_goal(sim.world())
            .unwrap()
            .goal
            .contains("Fly off")
    );

    // Fly off to undock.
    step(
        &mut sim,
        &PlayerInput {
            thrust: Vec2::new(0.0, 1.0),
            ..PlayerInput::default()
        },
    );
    hands_off(&mut sim, 2);
    assert_eq!(status(&sim, "beat.b01_undock"), Some(BeatStatus::Completed));
    assert_eq!(
        status(&sim, "beat.b02_mine_by_hand"),
        Some(BeatStatus::Active)
    );

    mine_ice(&mut sim);
    hands_off(&mut sim, 1);
    assert_eq!(
        status(&sim, "beat.b02_mine_by_hand"),
        Some(BeatStatus::Completed)
    );

    let credits = sim.world().player.credits;
    dock_at(&mut sim, &KETTLE);
    assert_eq!(
        status(&sim, "beat.b03_first_delivery"),
        Some(BeatStatus::Active),
        "docking alone hands nothing over"
    );
    // Odo asks; the player picks Deliver.
    step(
        &mut sim,
        &PlayerInput {
            replies: vec![Reply {
                npc: "npc.odo_fenn".parse().unwrap(),
                choice: "choice.deliver_ice".parse().unwrap(),
            }],
            ..PlayerInput::default()
        },
    );
    hands_off(&mut sim, 3);
    assert_eq!(
        status(&sim, "beat.b03_first_delivery"),
        Some(BeatStatus::Completed),
        "Deliver hands Odo the ice"
    );
    assert_eq!(player(&sim).hold.count(&ICE), 0);
    assert_eq!(sim.world().stations.get(&KETTLE).unwrap().build.stage, 1);
    assert_eq!(sim.world().player.credits, credits + 12_000);

    // The reward and the starting 40 cr don't make 600: stand in for a few ore runs'
    // sales, then buy the lens at the Wharf.
    sim.submit(
        CommandBatch::new(Cause::system("test", "a few sales later"))
            .with(Command::AdjustCredits { delta: 50_000 }),
    );
    dock_at(&mut sim, &WHARF);
    let lens = ServiceRequest::Fit { upgrade: LENS };
    assert!(
        services::offers(sim.world(), sim.history(), &WHARF)
            .iter()
            .any(|o| o.request == lens && o.affordable)
    );
    step(
        &mut sim,
        &PlayerInput {
            services: vec![lens],
            ..PlayerInput::default()
        },
    );
    hands_off(&mut sim, 1);
    assert!(sim.world().player.fittings.contains(&LENS));
    assert_eq!(
        galaxy_progress::modifiers::for_ship(sim.world(), &PLAYER).mining_rate,
        1.5
    );
}
