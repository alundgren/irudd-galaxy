//! The beat runtime and the W4 hooks through the script host, headless: the core
//! pack's b01-b03, a probe pack for orders, mechanics, standing, levers and pacing,
//! save/load, hot reload and determinism.

mod common;

use std::path::{Path, PathBuf};

use common::*;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::Channel;
use galaxy_core::{Save, Sim};
use galaxy_progress::{ProgressSystem, beats, services};
use galaxy_script::{HostEvent, ScriptHost};

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");
const COMPACT: FactionId = FactionId::from_static("faction.tollway_compact");
const B01: &str = "beat.b01_undock";
const B02: &str = "beat.b02_mine_by_hand";
const B03: &str = "beat.b03_first_delivery";
const PROBE: &str = "beat.t01_probe";
const CRAFT: ShipId = ShipId::from_static("craft.probe");

fn core_pack() -> PathBuf {
    slice().join("packs/core")
}

fn probe_pack() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/beat_probe")
}

/// A new game with the core pack (and `extra` packs) loaded, the script host and the
/// progress system attached.
fn journey(seed: u64, extra: &[&Path]) -> (Sim, ScriptHost) {
    let mut sim = new_sim(seed);
    let host = ScriptHost::new(None);
    for dir in std::iter::once(core_pack().as_path()).chain(extra.iter().copied()) {
        let report = host.load(dir, sim.world(), sim.history());
        assert!(report.accepted(), "{report:#?}");
    }
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
    act(
        sim,
        vec![
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

fn undock_from(sim: &mut Sim, station: &StationId) {
    act(
        sim,
        vec![
            Command::SetShipDocked {
                ship: PLAYER,
                station: None,
            },
            Command::Emit {
                event: Event::Undock {
                    ship: PLAYER,
                    station: station.clone(),
                },
            },
        ],
    );
}

fn mine_ice(sim: &mut Sim, n: u32) {
    let rock = sim.world().rocks.keys().next().unwrap().clone();
    act(
        sim,
        vec![
            Command::AdjustCargo {
                ship: PLAYER,
                ware: ICE,
                delta: i64::from(n),
            },
            Command::Emit {
                event: Event::Mine {
                    ship: PLAYER,
                    rock,
                    ware: ICE,
                    amount: n,
                },
            },
        ],
    );
}

fn beat(sim: &Sim, id: &str) -> Option<BeatState> {
    sim.world().progress.beats.get(id).cloned()
}

fn status(sim: &Sim, id: &str) -> Option<BeatStatus> {
    beat(sim, id).map(|b| b.status)
}

fn facts(sim: &Sim, kind: &str) -> Vec<Fact> {
    sim.history()
        .find(&HistoryQuery::kind(FactKind::new(kind).unwrap()))
        .into_iter()
        .cloned()
        .collect()
}

/// Plays b01 to b03 from a new game.
fn play_act1(sim: &mut Sim) {
    step(sim, 1);
    undock_from(sim, &WHARF);
    step(sim, 1);
    mine_ice(sim, 20);
    step(sim, 1);
    dock_at(sim, &KETTLE);
    step(sim, 3);
}

#[test]
fn core_beats_b01_to_b03_complete_from_history_and_world_state() {
    let (mut sim, _host) = journey(1, &[]);
    step(&mut sim, 1);
    // b01 starts on a new save: docked at the Wharf in the Wren.
    assert_eq!(status(&sim, B01), Some(BeatStatus::Active));
    let goal = beats::hud_goal(sim.world()).expect("a goal line");
    assert!(goal.goal.starts_with("Ilse needs ice"), "{goal:?}");
    assert_eq!(goal.giver, Some(NpcId::from_static("npc.ilse_marrow")));
    assert_eq!(facts(&sim, "beat_started").len(), 1);
    assert_eq!(status(&sim, B02), None, "b02 waits on b01");

    // b01's end state is a history fact: the undock from the Wharf.
    undock_from(&mut sim, &WHARF);
    step(&mut sim, 1);
    assert_eq!(status(&sim, B01), Some(BeatStatus::Completed));
    assert_eq!(status(&sim, B02), Some(BeatStatus::Active));
    assert!(
        beats::hud_goal(sim.world())
            .unwrap()
            .goal
            .starts_with("Hold Space")
    );

    // b02's end state is world state: 20 ice in the hold (a tunable).
    mine_ice(&mut sim, 12);
    step(&mut sim, 1);
    assert_eq!(status(&sim, B02), Some(BeatStatus::Active));
    mine_ice(&mut sim, 8);
    step(&mut sim, 1);
    assert_eq!(status(&sim, B02), Some(BeatStatus::Completed));
    assert_eq!(status(&sim, B03), Some(BeatStatus::Active));

    // b03: dock at Kettle; the core script hands the ice to the greenhouse ledger.
    let credits = sim.world().player.credits;
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 3);
    let deliveries = facts(&sim, "deliver");
    assert_eq!(deliveries.len(), 1);
    assert_eq!(deliveries[0].amount, Some(20));
    assert_eq!(
        deliveries[0].subjects.other.as_ref().map(|o| o.as_str()),
        Some("ledger.kettle_greenhouse")
    );
    assert_eq!(sim.world().ships.get(&PLAYER).unwrap().hold.count(&ICE), 0);
    assert_eq!(status(&sim, B03), Some(BeatStatus::Completed));
    // Rewards: 120 cr, Coop +5 (plus +2 from the ledger-delivery deed), stage 1.
    assert_eq!(sim.world().player.credits, credits + 12_000);
    assert_eq!(sim.world().factions.get(&COOP).unwrap().standing, 7);
    let kettle = sim.world().stations.get(&KETTLE).unwrap();
    assert_eq!(kettle.build.stage, 1);
    assert!(kettle.build.parts.contains("part.kettle.pane_dome_a"));
    assert_eq!(facts(&sim, "core.stage_built").len(), 1);
    assert_eq!(facts(&sim, "beat_completed").len(), 3);
    assert_eq!(
        sim.world()
            .counters
            .get(CounterKind::LedgerDeliveries, &CounterScope::Player),
        1
    );
    // Odo names what was brought, citing the delivery.
    let talk = lines(&sim, Channel::DockTalk);
    let odo = talk
        .iter()
        .find(|l| l.line.speaker == Some(ODO))
        .expect("Odo speaks");
    assert_eq!(odo.line.text, "20 ice. The beans will hear about this.");
    assert_eq!(odo.line.cites, vec![deliveries[0].id]);
    // b03 done: the mining lens is on Orrin's list.
    assert!(
        services::offers(sim.world(), sim.history(), &WHARF)
            .iter()
            .any(|o| o.name == "Mining lens")
    );
    // b04 follows: the HUD points at the lens.
    let next = beats::hud_goal(sim.world()).expect("b04 is the goal after b03");
    assert_eq!(next.beat.as_str(), "beat.b04_first_sale");
    assert!(next.goal.contains("mining lens"));
}

#[test]
fn a_situation_beat_runs_its_steps_from_counters_and_a_predicate() {
    let mut sim = new_sim(2);
    seed_probe(&mut sim);
    let (mut sim, host) = attach_probe(sim);
    // The first Kettle docking starts it (and the probe raises its tunable to 3).
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    let b = beat(&sim, PROBE).expect("started");
    assert_eq!((b.kind, b.step), (BeatKind::Situation, 0));
    assert_eq!(b.goal, "Dock at Kettle again.");
    assert_eq!(
        sim.world().progress.tunables["probe.dockings"].value,
        TunableValue::Number(3.0)
    );
    assert!(sim.world().progress.tunables["probe.dockings"].tuned());
    for _ in 0..2 {
        undock_from(&mut sim, &KETTLE);
        dock_at(&mut sim, &KETTLE);
    }
    step(&mut sim, 1);
    let b = beat(&sim, PROBE).unwrap();
    assert_eq!((b.step, b.step_id.as_deref()), (1, Some("ice")));
    let steps = facts(&sim, "quest_step");
    assert_eq!(steps.len(), 1);
    assert_eq!(
        facts(&sim, "probe.stepped").len(),
        1,
        "on_step ran with the step"
    );
    // The predicate holds once the hold has 5 ice; then the end state waits on one more
    // docking after the step began.
    mine_ice(&mut sim, 5);
    step(&mut sim, 2);
    assert_eq!(beat(&sim, PROBE).unwrap().status, BeatStatus::Active);
    assert_eq!(beat(&sim, PROBE).unwrap().step, 2, "awaiting the end state");
    let compact = sim.world().factions.get(&COMPACT).unwrap().standing;
    let credits = sim.world().player.credits;
    dock_at(&mut sim, &WHARF);
    step(&mut sim, 2);
    assert_eq!(status(&sim, PROBE), Some(BeatStatus::Completed));
    assert_eq!(sim.world().player.credits, credits + 150, "1.5 cr");
    assert!(sim.world().player.flags.contains("cap.click_fly"));
    assert_eq!(facts(&sim, "cap_unlocked").len(), 1);
    assert_eq!(
        sim.world().factions.get(&COMPACT).unwrap().standing,
        compact + 2
    );
    assert_eq!(facts(&sim, "probe.noticed").len(), 1);
    drop(host);
}

/// The probe's tunable and an NPC craft for it to order.
fn seed_probe(sim: &mut Sim) {
    let mut craft = sim.world().ships.get(&PLAYER).unwrap().clone();
    craft.id = CRAFT;
    craft.owner = Owner::Faction { faction: COMPACT };
    craft.docked_at = Some(KETTLE);
    let value = TunableValue::Number(2.0);
    act(
        sim,
        vec![
            Command::SetTunable {
                id: TunableId::from_static("probe.dockings"),
                value: Some(Box::new(Tunable {
                    value: value.clone(),
                    authored: value,
                    set_by: None,
                })),
            },
            Command::PutShip {
                id: CRAFT,
                value: Some(Box::new(craft)),
            },
        ],
    );
}

fn attach_probe(mut sim: Sim) -> (Sim, ScriptHost) {
    let host = ScriptHost::new(None);
    let report = host.load(&probe_pack(), sim.world(), sim.history());
    assert!(report.accepted(), "{report:#?}");
    host.attach(&mut sim);
    sim.add_system(Box::new(ProgressSystem));
    (sim, host)
}

#[test]
fn scripts_order_craft_but_never_the_player_ship() {
    let mut sim = new_sim(3);
    seed_probe(&mut sim);
    let (mut sim, host) = attach_probe(sim);
    let player_orders = sim.world().ships.get(&PLAYER).unwrap().orders.clone();
    // Docking at the Wharf makes the probe try to order the player's ship.
    undock_from(&mut sim, &WHARF);
    dock_at(&mut sim, &WHARF);
    step(&mut sim, 1);
    let aborted: Vec<_> = host
        .take_events()
        .into_iter()
        .filter_map(|e| match e {
            HostEvent::Aborted { diagnostic, .. } => Some(diagnostic.code),
            _ => None,
        })
        .collect();
    assert_eq!(aborted, vec!["player_ship".to_owned()]);
    assert_eq!(
        sim.world().ships.get(&PLAYER).unwrap().orders,
        player_orders
    );

    // At Kettle it orders its own craft instead, with the same verbs.
    dock_at(&mut sim, &KETTLE);
    step(&mut sim, 1);
    let q = &sim.world().ships.get(&CRAFT).unwrap().orders;
    assert_eq!(
        q.orders,
        vec![Order::Dock { station: WHARF }, Order::Undock {}]
    );
    // The same handler taught a mechanic, moved standing and pulled a market lever.
    let w = sim.world();
    assert!(w.player.flags.contains("mech.rock_tagging"));
    assert_eq!(
        w.progress.slots.get("slot.key_t").map(|f| f.as_str()),
        Some("mech.rock_tagging")
    );
    assert_eq!(facts(&sim, "probe.mechanic_taught").len(), 1);
    assert_eq!(w.factions.get(&COMPACT).unwrap().standing, 12);
    assert_eq!(
        galaxy_progress::standing::tier(w, &COMPACT).map(|t| t.id.as_str()),
        Some("known")
    );
    assert_eq!(facts(&sim, "regard_changed").len(), 1);
    let ice = &w.markets.get(&KETTLE).unwrap().lines[&ICE];
    assert_eq!(ice.consumption_scale, 2.0);
}

#[test]
fn beats_survive_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("b.sqlite");
    let (mut sim, _host) = journey(4, &[]);
    step(&mut sim, 1);
    undock_from(&mut sim, &WHARF);
    step(&mut sim, 1);
    mine_ice(&mut sim, 20);
    assert_eq!(status(&sim, B02), Some(BeatStatus::Active));
    Save::create(&path, sim.state()).unwrap();

    // A fresh process: load the save, load the core pack again, carry on.
    let mut back = Sim::new(Save::open(&path).unwrap().load().unwrap());
    let host = ScriptHost::new(None);
    assert!(
        host.load(&core_pack(), back.world(), back.history())
            .accepted()
    );
    host.attach(&mut back);
    back.add_system(Box::new(ProgressSystem));
    step(&mut back, 1);
    assert_eq!(status(&back, B01), Some(BeatStatus::Completed));
    assert_eq!(status(&back, B02), Some(BeatStatus::Completed));
    assert_eq!(
        facts(&back, "beat_started").len(),
        3,
        "nothing started twice"
    );
    dock_at(&mut back, &KETTLE);
    step(&mut back, 3);
    assert_eq!(status(&back, B03), Some(BeatStatus::Completed));
}

#[test]
fn beats_survive_hot_reload() {
    let dir = tempfile::tempdir().unwrap();
    let pack = dir.path().join("core");
    copy_dir(&core_pack(), &pack);
    let mut sim = new_sim(5);
    let host = ScriptHost::new(None);
    assert!(host.load(&pack, sim.world(), sim.history()).accepted());
    host.attach(&mut sim);
    sim.add_system(Box::new(ProgressSystem));
    step(&mut sim, 1);
    undock_from(&mut sim, &WHARF);
    step(&mut sim, 1);
    assert_eq!(status(&sim, B02), Some(BeatStatus::Active));
    let old_rev = host.revision(&pack_id("core")).unwrap();

    // Edit b02's goal line on disk and reload between ticks.
    let file = pack.join("scripts/act1.luau");
    let src = std::fs::read_to_string(&file).unwrap();
    std::fs::write(
        &file,
        src.replace(
            "Hold Space beside an ice rock until your hold is full.",
            "Hold Space beside a pale ice rock until the hold is full.",
        ),
    )
    .unwrap();
    let report = host.reload(&pack_id("core"), &mut sim).unwrap();
    assert!(report.accepted(), "{report:#?}");
    assert_ne!(host.revision(&pack_id("core")).unwrap(), old_rev);
    step(&mut sim, 1);
    // Same beat, same step, new words; nothing restarted.
    let b = beat(&sim, B02).unwrap();
    assert_eq!(b.status, BeatStatus::Active);
    assert_eq!(
        b.goal,
        "Hold Space beside a pale ice rock until the hold is full."
    );
    assert_eq!(status(&sim, B01), Some(BeatStatus::Completed));
    assert_eq!(facts(&sim, "beat_started").len(), 2);
    mine_ice(&mut sim, 20);
    step(&mut sim, 1);
    assert_eq!(status(&sim, B02), Some(BeatStatus::Completed));
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            copy_dir(&p, &to.join(e.file_name()));
        } else {
            std::fs::copy(&p, to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn the_beat_journey_is_deterministic_and_save_load_matches_an_uninterrupted_run() {
    let (mut a, _ha) = journey(6, &[]);
    play_act1(&mut a);
    let (mut b, _hb) = journey(6, &[]);
    play_act1(&mut b);
    assert_eq!(a.hash(), b.hash());
    assert_eq!(status(&a, B03), Some(BeatStatus::Completed));

    // Save halfway, load, finish: the same hash as the uninterrupted run.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("d.sqlite");
    let (mut c, _hc) = journey(6, &[]);
    step(&mut c, 1);
    undock_from(&mut c, &WHARF);
    step(&mut c, 1);
    Save::create(&path, c.state()).unwrap();
    let mut d = Sim::new(Save::open(&path).unwrap().load().unwrap());
    let hd = ScriptHost::new(None);
    assert!(hd.load(&core_pack(), d.world(), d.history()).accepted());
    hd.attach(&mut d);
    d.add_system(Box::new(ProgressSystem));
    mine_ice(&mut d, 20);
    step(&mut d, 1);
    dock_at(&mut d, &KETTLE);
    step(&mut d, 3);
    assert_eq!(d.hash(), a.hash());
}
