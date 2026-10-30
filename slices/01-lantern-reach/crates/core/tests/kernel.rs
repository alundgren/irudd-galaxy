//! Kernel tests: determinism, save/load, pause, atomic batches, undo and rollback.
//! All headless.

use std::path::{Path, PathBuf};

use galaxy_contract::pack::{HandlerRetry, ScriptHealth, ScriptValue};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::{Save, Sim, SimState, System, TickCtx, World, default_systems, new_game};

fn data_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
}

const WHARF: StationId = StationId::from_static("station.lantern_wharf");
const KETTLE: StationId = StationId::from_static("station.kettle_dock");
const PLAYER: ShipId = ShipId::from_static("ship.player");
const ICE: WareId = WareId::from_static("ware.ice");
const COOP: FactionId = FactionId::from_static("faction.wharf_cooperative");

/// Uses its RNG stream every tick, so the hash covers RNG state too.
struct Noise;

impl System for Noise {
    fn name(&self) -> &'static str {
        "test_noise"
    }
    fn step(&self, ctx: &mut TickCtx<'_>) {
        if ctx.rng().chance(0.2) {
            let delta = i64::from(ctx.rng().range_u32(1, 5));
            ctx.submit(
                CommandBatch::new(Cause::system("test_noise", "noise"))
                    .with(Command::AdjustCredits { delta }),
            );
        }
    }
}

fn sim(seed: u64) -> Sim {
    let state = new_game(&data_dir(), seed).expect("core data loads");
    resume(state)
}

fn resume(state: SimState) -> Sim {
    let mut sim = Sim::new(state).with_systems(default_systems());
    sim.add_system(Box::new(galaxy_core::placeholder::PlaceholderFlight));
    sim.add_system(Box::new(Noise));
    sim
}

/// A scripted input stream: undock and fly toward Kettle Dock, then try to dock.
fn input_at(tick: u64) -> PlayerInput {
    let phase = tick % 240;
    PlayerInput {
        thrust: match phase {
            0..=89 => Vec2::new(0.8, -0.6),
            90..=119 => Vec2::new(0.0, 1.0),
            120..=179 => Vec2::new(-1.0, 0.2),
            _ => Vec2::ZERO,
        },
        action: phase >= 200,
        ..PlayerInput::default()
    }
}

fn run_to(sim: &mut Sim, until: u64) {
    while sim.tick().0 < until {
        let t = sim.tick().0;
        sim.step(&input_at(t)).expect("not paused");
    }
}

#[test]
fn core_data_builds_the_lantern_reach() {
    let state = new_game(&data_dir(), 1).unwrap();
    let w = &state.world;
    assert_eq!(w.regions.len(), 3);
    assert_eq!(w.stations.len(), 5);
    assert_eq!(w.wares.len(), 6);
    assert_eq!(w.rock_types.len(), 4);
    assert!(w.rocks.contains("rock.shallows.ice.03"));
    assert!(w.lanes.len() >= 2);
    assert_eq!(w.ships.get(&PLAYER).unwrap().docked_at, Some(WHARF));
    assert!(w.markets.contains(&KETTLE));
    // The authored layout doesn't depend on the seed.
    let other = new_game(&data_dir(), 2).unwrap();
    assert_eq!(w.rocks, other.world.rocks);
}

#[test]
fn core_manifest_parses() {
    let text = std::fs::read_to_string(data_dir().join("../manifest.toml")).unwrap();
    let m: galaxy_contract::pack::PackManifest = toml::from_str(&text).unwrap();
    assert_eq!(m.id.as_str(), "core");
}

#[test]
fn same_seed_and_inputs_give_the_same_hash() {
    let mut a = sim(42);
    let mut b = sim(42);
    for checkpoint in [1, 50, 200, 600] {
        run_to(&mut a, checkpoint);
        run_to(&mut b, checkpoint);
        assert_eq!(a.hash(), b.hash(), "diverged by tick {checkpoint}");
    }
    // The run actually did things: moved, docked, made noise.
    assert!(
        a.history()
            .count(&HistoryQuery::kind(EventKind::Undock.fact_kind()))
            > 0
    );
    assert!(a.world().player.credits != 40);

    let mut c = sim(43);
    run_to(&mut c, 600);
    assert_ne!(a.hash(), c.hash(), "seed must matter");
}

#[test]
fn save_load_continue_matches_uninterrupted() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("run.sqlite");

    let mut straight = sim(7);
    run_to(&mut straight, 500);

    let mut first = sim(7);
    run_to(&mut first, 230);
    let hash_at_save = first.hash();
    Save::create(&path, first.state()).unwrap();
    drop(first);

    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut second = resume(loaded);
    assert_eq!(
        second.hash(),
        hash_at_save,
        "load must restore the exact state"
    );
    run_to(&mut second, 500);
    assert_eq!(second.hash(), straight.hash());

    // Writing again appends only the new history rows.
    let mut save = Save::open(&path).unwrap();
    save.write(second.state()).unwrap();
    let again = save.load().unwrap();
    assert_eq!(galaxy_core::state_hash(&again), straight.hash());
    assert_eq!(
        save.info().unwrap().facts as usize,
        straight.history().len()
    );
}

#[test]
fn pause_stops_ticks() {
    let mut s = sim(1);
    run_to(&mut s, 10);
    let before = s.hash();
    s.set_paused(true);
    assert!(s.step(&input_at(10)).is_none());
    assert_eq!(s.run(100, &input_at(10)), 0);
    assert_eq!(s.tick(), Tick(10));
    assert_eq!(s.hash(), before, "nothing changes while paused");
    s.set_paused(false);
    assert!(s.step(&input_at(10)).is_some());
    assert_eq!(s.tick(), Tick(11));
}

#[test]
fn batches_apply_all_or_nothing() {
    let mut s = Sim::new(new_game(&data_dir(), 1).unwrap());
    let before = s.world().clone();

    // Second command fails: unknown ware. The credit change must not stick.
    s.submit(
        CommandBatch::new(Cause::system("test", "bad"))
            .with(Command::AdjustCredits { delta: 100 })
            .with(Command::AdjustCargo {
                ship: PLAYER,
                ware: WareId::new("ware.nothing").unwrap(),
                delta: 1,
            })
            .with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: WHARF,
                },
            }),
    );
    // An invariant failure: more cargo than the Wren holds.
    s.submit(
        CommandBatch::new(Cause::system("test", "overfull")).with(Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 1000,
        }),
    );
    // A good batch in the same tick still applies.
    s.submit(
        CommandBatch::new(Cause::system("test", "good")).with(Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 3,
        }),
    );
    let report = s.step(&PlayerInput::default()).unwrap();
    assert_eq!(report.applied, 1);
    assert_eq!(report.rejected.len(), 2);
    assert_eq!(report.rejected[0].command, Some(1));
    assert!(report.rejected[1].reason.contains("hold_within_capacity"));

    let w = s.world();
    assert_eq!(w.player.credits, before.player.credits);
    assert_eq!(w.ships.get(&PLAYER).unwrap().hold.count(&ICE), 3);
    assert!(s.history().is_empty(), "rejected events never go out");
    assert!(w.counters.player.is_empty());
}

#[test]
fn undo_records_invert_commands() {
    let mut world = new_game(&data_dir(), 1).unwrap().world;
    let original = world.clone();
    let mut kettle = world.stations.get(&KETTLE).unwrap().clone();
    kettle.id = StationId::new("gm.sixth_station").unwrap();
    kettle.name = "The sixth station".into();
    let kettle_ice = world.markets.get(&KETTLE).unwrap().lines[&ICE].clone();
    let coop = world.factions.get(&COOP).unwrap().standing;
    let mercy = ShipId::new("craft.little_mercy").unwrap();
    let mut mercy_route = world.traders[&mercy].clone();
    mercy_route.max_units_per_ware = 4;
    let commands = vec![
        Command::SetCanonStatus {
            id: "canon.second_fragment".into(),
            status: galaxy_contract::canon::CanonStatus::Open,
        },
        Command::PutMapOverlay {
            id: "gm.test.border".into(),
            value: Some(Box::new(galaxy_contract::live::MapOverlay {
                region: "region.lantern_shallows".parse().unwrap(),
                center: Vec2::ZERO,
                radius: 100.0,
                owner: None,
                contested: true,
                direction: None,
            })),
        },
        Command::PutSurveyBuoy {
            id: "gm.test.buoy".into(),
            value: Some(Box::new(galaxy_contract::live::SurveyBuoy {
                region: "region.lantern_shallows".parse().unwrap(),
                position: Vec2::ZERO,
                radius: 40.0,
                planted_by: COOP,
                reward: None,
                read: false,
            })),
        },
        Command::UpdateSurveyBuoy {
            id: "gm.test.buoy".into(),
            from: Box::new(galaxy_contract::live::SurveyBuoy {
                region: "region.lantern_shallows".parse().unwrap(),
                position: Vec2::ZERO,
                radius: 40.0,
                planted_by: COOP,
                reward: None,
                read: false,
            }),
            to: Box::new(galaxy_contract::live::SurveyBuoy {
                region: "region.lantern_shallows".parse().unwrap(),
                position: Vec2::ZERO,
                radius: 40.0,
                planted_by: COOP,
                reward: None,
                read: true,
            }),
        },
        Command::ExpectFlag {
            flag: FlagId::new("mech.rock_tagging").unwrap(),
            on: false,
        },
        // A consignment loaded, a second one loaded, the first handed over.
        Command::SetConsignment {
            ship: PLAYER,
            ledger: "ledger.hook_consignment".parse().unwrap(),
            consignment: Some(Box::new(galaxy_contract::world::Consignment {
                ware: ICE,
                quantity: 5,
                owner: "npc.ilse_marrow".parse().unwrap(),
                to: "npc.rook_adeyemi".parse().unwrap(),
                station: "station.cinder_hook".parse().unwrap(),
            })),
        },
        Command::SetConsignment {
            ship: PLAYER,
            ledger: "ledger.kettle_greenhouse".parse().unwrap(),
            consignment: Some(Box::new(galaxy_contract::world::Consignment {
                ware: ICE,
                quantity: 3,
                owner: "npc.ilse_marrow".parse().unwrap(),
                to: "npc.odo_fenn".parse().unwrap(),
                station: KETTLE,
            })),
        },
        Command::SetConsignment {
            ship: PLAYER,
            ledger: "ledger.hook_consignment".parse().unwrap(),
            consignment: None,
        },
        Command::SetRockTag {
            rock: world.rocks.keys().next().unwrap().clone(),
            on: true,
        },
        Command::PutStationRecipe {
            id: KETTLE,
            value: Some(Box::new(galaxy_kit::StationRecipe::kettle_dock(1))),
        },
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: 5,
        },
        Command::AdjustCargo {
            ship: PLAYER,
            ware: ICE,
            delta: -5,
        },
        Command::AdjustCredits { delta: 25 },
        Command::SetStationStage {
            station: KETTLE,
            stage: 2,
        },
        Command::SetStationPart {
            station: KETTLE,
            part: PartId::new("part.kettle.dome_b").unwrap(),
            attached: true,
        },
        Command::SetStationOwner {
            station: KETTLE,
            owner: None,
        },
        Command::AdjustStanding {
            faction: COOP,
            delta: 3,
        },
        Command::ExpectStanding {
            faction: COOP,
            min: coop + 3,
            max: coop + 3,
        },
        Command::SetFlag {
            flag: FlagId::new("flag.nav_chip").unwrap(),
            on: true,
        },
        Command::PutBeat {
            id: BeatId::new("beat.b01_undock").unwrap(),
            value: Some(Box::new(BeatState {
                pack: PackId::new("core").unwrap(),
                act: 1,
                kind: BeatKind::Directed,
                status: BeatStatus::Active,
                step: 0,
                step_id: None,
                goal: "Undock.".into(),
                giver: None,
                since: None,
                step_since: None,
                done_since: None,
                started: Tick(0),
            })),
        },
        Command::SetFitting {
            upgrade: UpgradeId::new("upgrade.mining_lens").unwrap(),
            fitted: true,
        },
        Command::ExpectFitting {
            upgrade: UpgradeId::new("upgrade.mining_lens").unwrap(),
            fitted: true,
        },
        Command::SetHullRecord {
            hull: HullId::new("hull.wren").unwrap(),
            record: Some(HullRecord {
                rank: 2,
                ..HullRecord::default()
            }),
        },
        Command::ExpectHullRecord {
            hull: HullId::new("hull.wren").unwrap(),
            record: Some(HullRecord {
                rank: 2,
                ..HullRecord::default()
            }),
        },
        Command::ExpectShip {
            ship: PLAYER,
            hull: Some(HullId::new("hull.wren").unwrap()),
        },
        Command::ExpectShip {
            ship: ShipId::new("ship.berth.wren").unwrap(),
            hull: None,
        },
        Command::SetShipHull {
            ship: PLAYER,
            hull: HullId::new("hull.tandem").unwrap(),
            name: "Tandem".into(),
        },
        Command::SetTunable {
            id: TunableId::new("b02.ice").unwrap(),
            value: Some(Box::new(Tunable {
                value: TunableValue::Number(30.0),
                authored: TunableValue::Number(20.0),
                set_by: Some(PackId::new("gm").unwrap()),
            })),
        },
        Command::BindSlot {
            slot: SlotId::new("slot.key_t").unwrap(),
            mechanic: Some(FlagId::new("mech.rock_tagging").unwrap()),
        },
        Command::ExpectDocked {
            ship: PLAYER,
            station: StationId::new("station.lantern_wharf").unwrap(),
        },
        Command::SetShipDocked {
            ship: PLAYER,
            station: None,
        },
        Command::SetShipMotion {
            ship: PLAYER,
            position: Vec2::new(10.0, 20.0),
            velocity: Vec2::new(1.0, 0.0),
            heading: 0.5,
        },
        Command::AdjustRock {
            rock: RockId::new("rock.shallows.ice.03").unwrap(),
            delta: -7,
        },
        Command::SetRockGrowth {
            rock: RockId::new("rock.shallows.ice.03").unwrap(),
            growth: RockGrowth {
                mark: 4,
                milli: 250,
            },
        },
        Command::UpdateOrders {
            ship: PLAYER,
            from: Box::default(),
            to: Box::new(OrderQueue {
                orders: vec![Order::Undock {}],
                ..OrderQueue::default()
            }),
        },
        Command::SetShipActivity {
            ship: PLAYER,
            activity: Box::new(ShipActivity {
                action_held: true,
                transit: None,
                mining: Some(MiningRun {
                    rock: RockId::new("rock.shallows.ice.03").unwrap(),
                    progress: 0.5,
                    mined: 2,
                }),
                held_clicks: vec![Click {
                    target: ClickTarget::Station { station: KETTLE },
                    queue: true,
                }],
                held_press: true,
            }),
        },
        Command::SetLoop {
            name: "ice run".into(),
            orders: Some(vec![Order::Undock {}]),
        },
        Command::UpdateMarketLine {
            station: KETTLE,
            ware: ICE,
            from: Box::new(kettle_ice.clone()),
            to: Box::new(MarketLine {
                stock: kettle_ice.stock + 4,
                saturation: 0.1,
                ..kettle_ice.clone()
            }),
        },
        Command::SetMarketRates {
            station: KETTLE,
            ware: ICE,
            rates: MarketRates {
                production: 0,
                consumption: 7,
                production_scale: 1.0,
                consumption_scale: 1.5,
            },
        },
        Command::SetMarketBase {
            station: KETTLE,
            ware: ICE,
            base: MarketBase {
                base_price: 1680,
                min_price: 353,
                max_price: 2184,
                price: 1900,
            },
        },
        Command::SetMarketStock {
            station: KETTLE,
            ware: ICE,
            stock: MarketStock {
                stock: 3,
                stock_carry: 0.0,
                min_price: 353,
                max_price: 2184,
                price: 2000,
            },
        },
        Command::SetMarketLine {
            station: KETTLE,
            ware: ICE,
            line: None,
        },
        Command::AdjustActivity {
            scope: ActivityScope::Station { station: KETTLE },
            delta: ActivityTally {
                pulses_milli: 1500,
                trades: 1,
                ..ActivityTally::default()
            },
        },
        Command::AdjustActivity {
            scope: ActivityScope::Region {
                region: RegionId::new("region.lantern_shallows").unwrap(),
            },
            delta: ActivityTally {
                units_sold: 20,
                ..ActivityTally::default()
            },
        },
        Command::SetDockTrack {
            ship: PLAYER,
            track: Some(DockTrack {
                station: KETTLE,
                away: true,
            }),
        },
        Command::SetSplitStack {
            ware: ICE,
            station: Some(KETTLE),
        },
        Command::PutStation {
            id: kettle.id.clone(),
            value: Some(Box::new(kettle)),
        },
        Command::PutRumor {
            id: RumorId::new("rumor.far_belt_glassite").unwrap(),
            value: Some(Box::new(Rumor {
                id: RumorId::new("rumor.far_belt_glassite").unwrap(),
                claim: "a rich glassite vein in the far belt".into(),
                is_true: false,
                planted_by: Some(FactionId::new("faction.tollway_compact").unwrap()),
                heard_at: [KETTLE].into(),
                heard_by_player: false,
                source: None,
            })),
        },
        Command::PutTrader {
            id: mercy.clone(),
            value: Some(Box::new(mercy_route)),
        },
        Command::PutTrader {
            id: mercy.clone(),
            value: None,
        },
        Command::SetScriptValue {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            script: "kettle_growth".into(),
            store: "progress".into(),
            key: "announced".into(),
            value: Some(ScriptValue::Integer(2)),
        },
        Command::SetTriggerFired {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            trigger: "kettle_growth.regulars".into(),
            fired: 1,
        },
        Command::SetScriptHealth {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            script: "kettle_growth".into(),
            health: ScriptHealth {
                revision: "r1".into(),
                aborts: 1,
                quarantined: false,
                quarantined_before: ["r0".to_owned()].into(),
                retries: vec![HandlerRetry {
                    event: EventEnvelope {
                        tick: Tick(3),
                        fact: FactId(7),
                        cause: CauseId(2),
                        event: Event::Dock {
                            ship: ShipId::new("ship.player").unwrap(),
                            station: StationId::new("station.kettle_dock").unwrap(),
                        },
                    },
                    lost: 1,
                }],
            },
        },
        Command::SetRevisionRetired {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            revision: "r1".into(),
            retired: true,
        },
        Command::SetPackOff {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            off: true,
        },
        Command::SetPackLayer {
            pack: PackId::new("gm.kettle_growth").unwrap(),
            layer: Some(galaxy_contract::pack::PackLayer::GameMaster),
        },
        Command::Say {
            line: Box::new(TalkLine {
                channel: Channel::DockTalk,
                speaker: Some(NpcId::new("npc.ilse_marrow").unwrap()),
                place: Some(WHARF),
                text: "Kettle lit another ring.".into(),
                cites: vec![FactId(1)],
                about: Vec::new(),
                choices: Vec::new(),
            }),
        },
        Command::SetRumorHeard {
            rumor: RumorId::new("rumor.far_belt_glassite").unwrap(),
            station: StationId::new("station.lantern_wharf").unwrap(),
            heard: true,
        },
    ];
    let mut staged = galaxy_core::apply::Staged::default();
    let mut undo = Vec::new();
    for c in &commands {
        assert_eq!(c.reversibility(), Reversibility::Reversible);
        // `op` names the command as serde does: the key of `needs::COMMAND_NEEDS`.
        assert_eq!(serde_json::to_value(c).unwrap()["op"], c.op());
        undo.push(galaxy_core::apply::apply(&mut world, c, &mut staged).unwrap());
    }
    assert_ne!(world, original);
    for u in undo.iter().rev() {
        let UndoRecord::Invert { command } = u else {
            panic!("reversible commands invert");
        };
        galaxy_core::apply::apply(&mut world, command, &mut staged).unwrap();
    }
    assert_eq!(world, original);

    let emit = Command::Emit {
        event: Event::SessionStart {},
    };
    assert_eq!(emit.reversibility(), Reversibility::CompensateOnly);
    assert!(matches!(
        galaxy_core::apply::apply(&mut world, &emit, &mut staged).unwrap(),
        UndoRecord::Compensate { .. }
    ));
}

/// A save made before `craft.toml`: no named traders, their ships or their hulls.
fn without_craft(world: &mut World) {
    for (id, def) in std::mem::take(&mut world.traders) {
        world.ships.remove(&id);
        world.hulls.remove(&def.hull);
    }
}

#[test]
fn craft_backfill_adds_named_traders_to_an_older_save_once() {
    let fresh = new_game(&data_dir(), 3).unwrap();
    assert!(
        galaxy_core::craft_backfill(&data_dir(), &fresh.world)
            .unwrap()
            .is_empty(),
        "a new game already has its craft"
    );
    let mut old = fresh.clone();
    without_craft(&mut old.world);
    assert!(old.world.traders.is_empty());
    let run = |state: SimState| {
        let mut sim = Sim::new(state).with_systems(default_systems());
        assert_eq!(
            galaxy_core::queue_craft_backfill(&mut sim, &data_dir()).unwrap(),
            fresh.world.traders.len()
        );
        // Opened again before the first step: nothing is queued twice.
        assert_eq!(
            galaxy_core::queue_craft_backfill(&mut sim, &data_dir()).unwrap(),
            0
        );
        let report = sim.step(&PlayerInput::default()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        sim
    };
    let (a, b) = (run(old.clone()), run(old));
    assert_eq!(a.hash(), b.hash(), "the backfill repeats exactly");
    let w = a.world();
    assert_eq!(w.traders, fresh.world.traders);
    for (id, def) in &w.traders {
        assert_eq!(w.ships.get(id), fresh.world.ships.get(id));
        assert_eq!(w.hulls.get(&def.hull), fresh.world.hulls.get(&def.hull));
    }
    assert!(
        galaxy_core::craft_backfill(&data_dir(), w)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn pacing_backfill_adds_missing_tunables_and_keeps_saved_ones() {
    let fresh = new_game(&data_dir(), 3).unwrap();
    assert!(
        galaxy_core::pacing_backfill(&data_dir(), &fresh.world)
            .unwrap()
            .is_none(),
        "a new game already has its tunables"
    );
    let mut old = fresh.clone();
    let tunables = &mut old.world.progress.tunables;
    let missing: Vec<TunableId> = tunables.keys().take(2).cloned().collect();
    for id in &missing {
        tunables.remove(id);
    }
    // A tunable the save holds keeps its saved value, tuned or not.
    let (kept, saved) = tunables.iter_mut().next().unwrap();
    let kept = kept.clone();
    saved.value = TunableValue::Number(12345.0);
    let saved = saved.clone();
    let run = |state: SimState| {
        let mut sim = Sim::new(state).with_systems(default_systems());
        assert_eq!(
            galaxy_core::queue_backfill(&mut sim, &data_dir()).unwrap(),
            1
        );
        // Opened again before the first step: nothing is queued twice.
        assert_eq!(
            galaxy_core::queue_backfill(&mut sim, &data_dir()).unwrap(),
            0
        );
        let report = sim.step(&PlayerInput::default()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        sim
    };
    let (a, b) = (run(old.clone()), run(old));
    assert_eq!(a.hash(), b.hash(), "the backfill repeats exactly");
    let w = &a.world().progress.tunables;
    for id in &missing {
        assert_eq!(w.get(id), fresh.world.progress.tunables.get(id), "{id}");
    }
    assert_eq!(w.get(&kept), Some(&saved));
    assert!(
        galaxy_core::pacing_backfill(&data_dir(), a.world())
            .unwrap()
            .is_none()
    );
}

#[test]
fn rumor_spreads_compose_and_never_repeat() {
    let mut world = new_game(&data_dir(), 1).unwrap().world;
    let rumor = RumorId::new("rumor.far_belt_glassite").unwrap();
    let mut staged = galaxy_core::apply::Staged::default();
    let put = Command::PutRumor {
        id: rumor.clone(),
        value: Some(Box::new(Rumor {
            id: rumor.clone(),
            claim: "a rich glassite vein in the far belt".into(),
            is_true: false,
            planted_by: None,
            heard_at: [KETTLE].into(),
            heard_by_player: false,
            source: None,
        })),
    };
    galaxy_core::apply::apply(&mut world, &put, &mut staged).unwrap();
    let spread = |station: &str| Command::SetRumorHeard {
        rumor: rumor.clone(),
        station: StationId::new(station).unwrap(),
        heard: true,
    };
    // Two spreads built against the same world both land.
    for s in ["station.lantern_wharf", "station.cinder_hook"] {
        galaxy_core::apply::apply(&mut world, &spread(s), &mut staged).unwrap();
    }
    assert_eq!(world.rumors.get(&rumor).unwrap().heard_at.len(), 3);
    // A second spread to the same station is refused, not merged.
    assert!(
        galaxy_core::apply::apply(&mut world, &spread("station.cinder_hook"), &mut staged).is_err()
    );
    assert!(
        galaxy_core::apply::apply(&mut world, &spread("station.nowhere"), &mut staged).is_err()
    );
}

#[test]
fn rollback_by_cause_restores_script_writes() {
    let mut s = Sim::new(new_game(&data_dir(), 1).unwrap());
    let before = s.world().clone();
    let pack = PackId::new("gm").unwrap();
    s.submit(
        CommandBatch::new(Cause::script(pack.clone(), "r1", "kettle grows"))
            .with(Command::SetStationStage {
                station: KETTLE,
                stage: 2,
            })
            .with(Command::SetStationPart {
                station: KETTLE,
                part: PartId::new("part.kettle.dome_b").unwrap(),
                attached: true,
            })
            .with(Command::RecordFact {
                fact: Box::new(galaxy_contract::history::FactDraft {
                    kind: FactKind::new("gm.kettle.stage_up").unwrap(),
                    subjects: Subjects {
                        station: Some(KETTLE),
                        ..Subjects::default()
                    },
                    amount: None,
                    note: "greenhouse stage 2".into(),
                }),
            }),
    );
    // A player write in the same tick is not journaled and survives the rollback.
    s.submit(
        CommandBatch::new(Cause::verb(Actor::Player, VerbKind::Sell, "sell"))
            .with(Command::AdjustCredits { delta: 9 }),
    );
    s.step(&PlayerInput::default()).unwrap();
    assert_eq!(s.world().stations.get(&KETTLE).unwrap().build.stage, 2);
    assert_eq!(s.journal().len(), 1);

    let causes = s.causes_where(
        |c| matches!(&c.cause.kind, CauseKind::Script { pack: p, revision } if *p == pack && revision == "r1"),
    );
    assert_eq!(causes.len(), 1);
    let plan = s.queue_rollback(&causes, "the dome order fell through");
    assert_eq!(plan.batches, 1);
    assert_eq!(plan.compensate.len(), 1, "the fact can't be unsaid");
    s.step(&PlayerInput::default()).unwrap();

    let w = s.world();
    assert_eq!(w.stations, before.stations);
    assert_eq!(w.player.credits, before.player.credits + 9);
    let fact = s.history().facts().last().unwrap();
    assert_eq!(fact.kind.as_str(), "gm.kettle.stage_up");
    assert_eq!(
        s.history().cause(fact.cause).unwrap().cause.summary,
        "kettle grows"
    );
}

#[test]
fn events_become_facts_and_counters() {
    let mut s = sim(3);
    // Undock, drift a little, come back, dock.
    s.step(&PlayerInput {
        thrust: Vec2::new(1.0, 0.0),
        ..PlayerInput::default()
    })
    .unwrap();
    for _ in 0..5 {
        s.step(&PlayerInput {
            thrust: Vec2::new(1.0, 0.0),
            ..PlayerInput::default()
        });
    }
    let dock = PlayerInput {
        action: true,
        ..PlayerInput::default()
    };
    s.step(&dock).unwrap();
    let sub = s.subscribe("test");
    s.step(&dock).unwrap(); // already docked: nothing
    assert!(s.drain_events(&sub).is_empty());

    let w = s.world();
    assert_eq!(w.ships.get(&PLAYER).unwrap().docked_at, Some(WHARF));
    let dockings = |scope| w.counters.get(CounterKind::Dockings, &scope);
    assert_eq!(dockings(CounterScope::Player), 1);
    assert_eq!(dockings(CounterScope::Station { station: WHARF }), 1);
    assert_eq!(
        dockings(CounterScope::Region {
            region: RegionId::new("region.lantern_shallows").unwrap()
        }),
        1
    );
    let q = HistoryQuery {
        kind: Some(EventKind::Dock.fact_kind()),
        station: Some(WHARF),
        ..HistoryQuery::default()
    };
    let fact = s.history().last(&q).unwrap();
    assert!(matches!(
        fact.payload,
        FactPayload::Event(Event::Dock { .. })
    ));
    // The envelope systems saw carries the fact id.
    let last = &s.state().kernel.last_events;
    assert!(last.is_empty(), "the second dock tick emitted nothing");
}

#[test]
fn history_sql_matches_memory() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("h.sqlite");
    let mut s = sim(11);
    run_to(&mut s, 720);
    let save = Save::create(&path, s.state()).unwrap();
    assert!(!s.history().is_empty());
    let queries = [
        HistoryQuery::default(),
        HistoryQuery::kind(EventKind::Dock.fact_kind()),
        HistoryQuery {
            station: Some(KETTLE),
            ..HistoryQuery::default()
        },
        HistoryQuery {
            ship: Some(PLAYER),
            since: Some(Tick(300)),
            limit: Some(2),
            ..HistoryQuery::default()
        },
    ];
    for q in &queries {
        let mem: Vec<Fact> = s.history().find(q).into_iter().cloned().collect();
        assert_eq!(save.query_history(q).unwrap(), mem, "{q:?}");
    }
}

#[test]
fn snapshots_restore_and_forks_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("s.sqlite");
    let mut s = sim(5);
    run_to(&mut s, 100);
    let mut save = Save::create(&path, s.state()).unwrap();
    let snap = save.snapshot(s.state(), "before change").unwrap();
    let hash_at_snap = s.hash();
    run_to(&mut s, 700);
    save.write(s.state()).unwrap();

    let fork = dir.path().join("fork.sqlite");
    galaxy_core::fork(&path, &fork).unwrap();
    assert!(galaxy_core::fork(&path, &fork).is_err(), "never overwrite");
    assert_eq!(Save::open(&fork).unwrap().info().unwrap().hash, s.hash());

    let restored = save.load_snapshot(snap).unwrap();
    assert_eq!(galaxy_core::state_hash(&restored), hash_at_snap);
    save.write(&restored).unwrap();
    let back = save.load().unwrap();
    assert_eq!(galaxy_core::state_hash(&back), hash_at_snap);
    assert_eq!(save.snapshots().unwrap().len(), 1);
}

#[test]
fn bad_data_is_rejected_with_reasons() {
    let dir = tempfile::tempdir().unwrap();
    for f in galaxy_contract::data::DATA_FILES {
        std::fs::copy(data_dir().join(f), dir.path().join(f)).unwrap();
    }
    let stations = std::fs::read_to_string(dir.path().join("stations.toml")).unwrap();
    std::fs::write(
        dir.path().join("stations.toml"),
        stations.replace("region.cinder_drift", "region.nowhere"),
    )
    .unwrap();
    let err = new_game(dir.path(), 1).unwrap_err().to_string();
    assert!(err.contains("region.nowhere"), "{err}");

    std::fs::write(
        dir.path().join("wares.toml"),
        "[[ware]]\nid = \"ware.x\"\nnmae = \"typo\"\n",
    )
    .unwrap();
    let err = new_game(dir.path(), 1).unwrap_err().to_string();
    assert!(err.contains("wares.toml"), "{err}");
}

#[test]
fn a_rejected_station_foundation_or_closure_restores_the_exact_records() {
    let mut sim = Sim::new(new_game(&data_dir(), 7).unwrap());
    let source =
        std::fs::read_to_string(data_dir().join("../../gm-examples/sixth_station/lib/recipe.luau"))
            .unwrap();
    let json = source
        .split("[=[")
        .nth(1)
        .unwrap()
        .split("]=]")
        .next()
        .unwrap();
    let recipe: galaxy_kit::StationRecipe = serde_json::from_str(json).unwrap();
    let mut station = sim.world().stations.get(&WHARF).unwrap().clone();
    station.id = "gm.test_station".parse().unwrap();
    let id = station.id.clone();
    let mut market = sim.world().markets.get(&WHARF).unwrap().clone();
    market.station = id.clone();
    let found = Command::FoundStation {
        station: Box::new(station),
        recipe: Box::new(recipe),
        market: Some(Box::new(market)),
    };
    let fail = Command::AdjustCredits { delta: i64::MIN };
    let mut before = sim.world().clone();
    sim.submit(
        CommandBatch::new(Cause::system("test", "reject the foundation"))
            .with(found.clone())
            .with(fail.clone()),
    );
    assert_eq!(sim.step(&PlayerInput::default()).unwrap().rejected.len(), 1);
    before.meta.tick = sim.world().meta.tick;
    assert_eq!(sim.world(), &before);
    sim.submit(CommandBatch::new(Cause::system("test", "found the station")).with(found));
    assert!(
        sim.step(&PlayerInput::default())
            .unwrap()
            .rejected
            .is_empty()
    );
    let mut before = sim.world().clone();
    sim.submit(
        CommandBatch::new(Cause::system("test", "reject the closure"))
            .with(Command::CloseFoundedStation { id })
            .with(fail),
    );
    assert_eq!(sim.step(&PlayerInput::default()).unwrap().rejected.len(), 1);
    before.meta.tick = sim.world().meta.tick;
    assert_eq!(sim.world(), &before);
}

#[test]
fn a_closed_scaffold_reopens_in_place_and_shuts_again_exactly() {
    let mut world = new_game(&data_dir(), 7).unwrap().world;
    let source =
        std::fs::read_to_string(data_dir().join("../../gm-examples/sixth_station/lib/recipe.luau"))
            .unwrap();
    let json = source
        .split("[=[")
        .nth(1)
        .unwrap()
        .split("]=]")
        .next()
        .unwrap();
    let recipe: galaxy_kit::StationRecipe = serde_json::from_str(json).unwrap();
    let mut station = world.stations.get(&WHARF).unwrap().clone();
    station.id = "gm.test_station".parse().unwrap();
    let id = station.id.clone();
    let mut market = world.markets.get(&WHARF).unwrap().clone();
    market.station = id.clone();
    let mut staged = galaxy_core::apply::Staged::default();
    let apply = |world: &mut WorldState, c: &Command, staged: &mut galaxy_core::apply::Staged| {
        galaxy_core::apply::apply(world, c, staged)
    };
    apply(
        &mut world,
        &Command::FoundStation {
            station: Box::new(station.clone()),
            recipe: Box::new(recipe.clone()),
            market: Some(Box::new(market.clone())),
        },
        &mut staged,
    )
    .unwrap();
    // A rollback's closure leaves a scaffold: no market, no services.
    apply(
        &mut world,
        &Command::CloseFoundedStation { id: id.clone() },
        &mut staged,
    )
    .unwrap();
    let closed = world.clone();
    assert!(!closed.markets.contains(&id));

    let reopen = Command::SetFoundedStation {
        station: Box::new(station.clone()),
        recipe: Box::new(recipe.clone()),
        market: Some(Box::new(market.clone())),
    };
    let undo = apply(&mut world, &reopen, &mut staged).unwrap();
    assert_eq!(world.stations.get(&id), Some(&station));
    assert_eq!(world.station_recipes.get(&id), Some(&recipe));
    assert_eq!(world.markets.get(&id), Some(&market));
    assert!(
        apply(&mut world, &reopen, &mut staged).is_err(),
        "an open ring can't be reopened"
    );
    let UndoRecord::Invert { command } = undo else {
        panic!("reopening inverts exactly")
    };
    apply(&mut world, &command, &mut staged).unwrap();
    assert_eq!(world, closed, "shutting it restores the exact scaffold");

    let mut moved = station.clone();
    moved.position.x += 1000.0;
    assert!(
        apply(
            &mut world,
            &Command::SetFoundedStation {
                station: Box::new(moved),
                recipe: Box::new(recipe),
                market: Some(Box::new(market)),
            },
            &mut staged,
        )
        .is_err(),
        "a reopened ring stays where its berths are"
    );
    assert_eq!(world, closed);
}

#[test]
fn a_survey_buoy_update_loses_to_a_removal_or_replacement() {
    let mut world = new_game(&data_dir(), 1).unwrap().world;
    let mut staged = galaxy_core::apply::Staged::default();
    let buoy = galaxy_contract::live::SurveyBuoy {
        region: "region.lantern_shallows".parse().unwrap(),
        position: Vec2::ZERO,
        radius: 40.0,
        planted_by: COOP,
        reward: None,
        read: false,
    };
    let read = galaxy_contract::live::SurveyBuoy {
        read: true,
        ..buoy.clone()
    };
    let update = Command::UpdateSurveyBuoy {
        id: "gm.test.buoy".into(),
        from: Box::new(buoy.clone()),
        to: Box::new(read),
    };
    assert!(galaxy_core::apply::apply(&mut world, &update, &mut staged).is_err());
    let mut moved = buoy.clone();
    moved.position.x = 50.0;
    galaxy_core::apply::apply(
        &mut world,
        &Command::PutSurveyBuoy {
            id: "gm.test.buoy".into(),
            value: Some(Box::new(moved.clone())),
        },
        &mut staged,
    )
    .unwrap();
    assert!(galaxy_core::apply::apply(&mut world, &update, &mut staged).is_err());
    assert_eq!(world.survey_buoys["gm.test.buoy"], moved);
}

/// #31: a docking where the ship last docked, before it has gone `recount_distance`
/// away, mined or docked elsewhere (the economy's docking track says so), is still a
/// `dock` fact, but with an amount of 0, and it moves no counter.
#[test]
fn a_re_dock_on_the_spot_is_a_docking_but_not_a_counted_one() {
    let mut s = sim(3);
    let dock = |s: &mut Sim, away: Option<bool>| {
        let mut b = CommandBatch::new(Cause::system("test", "docks"));
        if let Some(away) = away {
            b.push(Command::SetDockTrack {
                ship: PLAYER,
                track: Some(DockTrack {
                    station: WHARF,
                    away,
                }),
            });
        }
        s.submit(b);
        s.step(&PlayerInput::default()).unwrap();
        s.submit(
            CommandBatch::new(Cause::system("test", "dock")).with(Command::Emit {
                event: Event::Dock {
                    ship: PLAYER,
                    station: WHARF,
                },
            }),
        );
        s.step(&PlayerInput::default()).unwrap();
    };
    let dockings = |s: &Sim| {
        s.world().counters.get(
            CounterKind::Dockings,
            &CounterScope::Station { station: WHARF },
        )
    };
    let last = |s: &Sim| {
        s.history()
            .last(&HistoryQuery::kind(EventKind::Dock.fact_kind()))
            .unwrap()
            .amount
    };
    // Last docked here and never left: not counted.
    dock(&mut s, Some(false));
    assert_eq!(dockings(&s), 0);
    assert_eq!(last(&s), Some(0));
    // Gone far enough (or mined) since: counted, the fact as before.
    dock(&mut s, Some(true));
    assert_eq!(dockings(&s), 1);
    assert_eq!(last(&s), None);
    // No track at all (a ship's first docking): counted.
    s.submit(
        CommandBatch::new(Cause::system("test", "forget")).with(Command::SetDockTrack {
            ship: PLAYER,
            track: None,
        }),
    );
    dock(&mut s, None);
    assert_eq!(dockings(&s), 2);
}
