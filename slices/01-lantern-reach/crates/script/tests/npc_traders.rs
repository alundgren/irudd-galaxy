//! Real orders, markets and core journey hooks; FakeMind, no GPU or network.
mod common;

use common::{core_data, slice};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use galaxy_mind::{FakeMind, Inline, MindConfig, MindHost};
use galaxy_script::{ScriptHost, host::HostEvent};

const KEEL: &str = "craft.even_keel";
const KETTLE: &str = "station.kettle_dock";
const BRASS: &str = "station.brassmouth_exchange";

fn state(seed: u64) -> SimState {
    let mut s = galaxy_core::new_game(&core_data(), seed).unwrap();
    // Compress travel in this headless fixture, never quantities or market rules.
    for id in ["hull.coop_skiff", "hull.compact_barge", "hull.lamp_launch"] {
        let mut hull = s.world.hulls.get(id).unwrap().clone();
        hull.max_speed *= 20.0;
        s.world.hulls.put(hull);
    }
    s.world.ship_rules.lanes.speed *= 20.0;
    s
}

struct Fleet {
    sim: Sim,
    mind: MindHost,
    scripts: ScriptHost,
}

impl Fleet {
    fn open(s: SimState, default_only: bool) -> Self {
        let seed = s.world.meta.seed;
        let mut sim = galaxy_core::game(s)
            .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)));
        sim.add_system(Box::new(galaxy_economy::EconomySystem));
        let scripts = ScriptHost::new(None);
        let report = scripts.load(&slice().join("packs/core"), sim.world(), sim.history());
        assert!(report.accepted(), "{report:#?}");
        scripts.attach(&mut sim);
        sim.add_system(Box::new(galaxy_progress::ProgressSystem));
        let mut config = MindConfig::load_dir(&core_data()).unwrap();
        if default_only {
            config.budget.default.calls = 0;
            config.budget.region.clear();
        }
        let mind = MindHost::new(config, Inline::new(FakeMind::new(seed)), sim.history());
        Self { sim, mind, scripts }
    }

    fn step(&mut self) {
        let report = self.sim.step(&PlayerInput::default()).unwrap();
        assert!(report.rejected.is_empty(), "{:?}", report.rejected);
        self.mind.pump(&mut self.sim);
        for event in self.scripts.take_events() {
            assert!(
                !matches!(
                    event,
                    HostEvent::Aborted { .. } | HostEvent::Quarantined { .. }
                ),
                "{event:?}"
            );
        }
    }

    fn run(&mut self, n: u32) {
        for _ in 0..n {
            self.step();
        }
    }

    fn until(&mut self, done: impl Fn(&Self) -> bool) {
        for _ in 0..4000 {
            if done(self) {
                return;
            }
            self.step();
        }
        panic!(
            "fleet stalled at {}: {:?}",
            self.sim.tick(),
            self.sim.world().ships
        );
    }

    fn has(&self, kind: &str) -> bool {
        self.sim
            .history()
            .facts()
            .iter()
            .any(|f| f.kind.as_str() == kind)
    }
}

#[test]
fn traders_fly_trade_and_move_prices_with_or_without_model_budget() {
    for default_only in [false, true] {
        let mut f = Fleet::open(state(97), default_only);
        let mut moved = false;
        let mut sold = std::collections::BTreeSet::new();
        let mut per_dock = std::collections::BTreeMap::<(ShipId, StationId, WareId), u32>::new();
        for _ in 0..2500 {
            let before = f.sim.world().markets.clone();
            let cursor = f.sim.history().facts().len();
            f.step();
            for fact in &f.sim.history().facts()[cursor..] {
                match &fact.payload {
                    FactPayload::Event(Event::Dock { ship, .. }) => {
                        per_dock.retain(|(s, _, _), _| s != ship)
                    }
                    FactPayload::Event(Event::Sell {
                        ship,
                        station,
                        ware,
                        quantity,
                        ..
                    }) => {
                        if !f.sim.world().traders.contains_key(ship) {
                            continue;
                        }
                        sold.insert(ship.clone());
                        let n = per_dock
                            .entry((ship.clone(), station.clone(), ware.clone()))
                            .or_default();
                        *n += quantity;
                        assert!(*n <= f.sim.world().traders[ship].max_units_per_ware);
                        let a = &before.get(station).unwrap().lines[ware];
                        let b = &f.sim.world().markets.get(station).unwrap().lines[ware];
                        moved |= b.stock > a.stock && b.price < a.price;
                    }
                    _ => {}
                }
            }
            if sold.len() == 4 {
                break;
            }
        }
        assert_eq!(sold.len(), 4, "every named trader sells: {sold:?}");
        assert!(moved, "a real sale raised stock and lowered the price");
        assert!(f.has("buy") && f.has("jump"));
        assert_eq!(
            f.sim.world().player.credits,
            4000,
            "NPCs never spend the player's wallet"
        );
        assert_eq!(
            f.sim
                .world()
                .counters
                .get(CounterKind::Dockings, &CounterScope::Player),
            0
        );
    }
}

#[test]
fn rumor_crosses_stations_by_craft_without_changing_truth() {
    for truth in [false, true] {
        let mut s = state(98);
        let id: RumorId = "rumor.test_cargo".parse().unwrap();
        s.world.rumors.put(Rumor {
            id: id.clone(),
            claim: "A chart from Brassmouth".into(),
            is_true: truth,
            heard_at: [BRASS.parse().unwrap()].into(),
            source: None,
            planted_by: None,
            heard_by_player: false,
        });
        let mut f = Fleet::open(s, true);
        f.until(|f| f.sim.world().rumors.get(&id).unwrap().heard_at.len() == 5);
        assert_eq!(f.sim.world().rumors.get(&id).unwrap().is_true, truth);
        for fact in f
            .sim
            .history()
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == "mind.rumor_spread")
        {
            let FactPayload::Note(note) = &fact.payload else {
                panic!("spread note")
            };
            let note: serde_json::Value = serde_json::from_str(note).unwrap();
            assert_eq!(note["truth"], truth);
            assert!(fact.subjects.ship.is_some());
        }
    }
}

#[test]
fn npc_dockings_move_the_dispute_tallies() {
    let mut s = state(99);
    let mut setup = common::pilot::Setup { state: &mut s };
    setup.done(&["beat.b07_cinder_drift"]);
    setup.tunable("b09.hook_dockings", 0.0);
    let mut f = Fleet::open(s, true);
    f.until(|f| f.has("core.dispute_tally"));
    let tallies: Vec<_> = f
        .sim
        .history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == "core.dispute_tally")
        .collect();
    assert!(tallies.iter().any(|f| {
        f.subjects
            .ship
            .as_ref()
            .is_some_and(|s| s.as_str() == "craft.ninefold")
    }));
}

#[test]
fn ignored_shortage_waits_for_even_keels_actual_arrival() {
    let mut s = state(100);
    let mut setup = common::pilot::Setup { state: &mut s };
    setup.done(&["beat.b11_orders"]);
    setup.tunable("shortage.rimewater_ice.trigger_left", 1.0);
    setup.tunable("shortage.rimewater_ice.mined", 0.0);
    setup.tunable("shortage.rimewater_ice.ignored_pulses", 0.0);
    let mut f = Fleet::open(s, true);
    f.until(|f| f.has("core.shortage_started"));
    assert!(!f.has("core.shortage_resolved"));
    f.until(|f| f.has("core.shortage_resolved"));
    let resolved = f
        .sim
        .history()
        .facts()
        .iter()
        .find(|f| f.kind.as_str() == "core.shortage_resolved")
        .unwrap();
    assert_eq!(resolved.amount, Some(0));
    assert_eq!(
        resolved.subjects.faction.as_ref().unwrap().as_str(),
        "faction.tollway_compact"
    );
    let arrival = f
        .sim
        .history()
        .facts()
        .iter()
        .rev()
        .find(|f| {
            f.kind.as_str() == "dock"
                && f.subjects.ship.as_ref().is_some_and(|s| s.as_str() == KEEL)
                && f.subjects
                    .station
                    .as_ref()
                    .is_some_and(|s| s.as_str() == KETTLE)
        })
        .unwrap();
    assert!(resolved.tick.0 >= arrival.tick.0 && resolved.tick.0 <= arrival.tick.0 + 2);
    assert!(f.sim.world().player.flags.contains("mech.even_keel_relief"));
}

#[test]
fn same_seed_and_mid_route_save_load_are_exact() {
    let mut a = Fleet::open(state(101), false);
    let mut b = Fleet::open(state(101), false);
    a.run(140);
    b.run(140);
    assert_eq!(a.sim.hash(), b.sim.hash());
    assert!(
        b.sim
            .world()
            .ships
            .values()
            .any(|s| s.owner != Owner::Player
                && s.docked_at.is_none()
                && !s.orders.orders.is_empty())
    );
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fleet.sqlite");
    Save::create(&path, b.sim.state()).unwrap();
    let loaded = Save::open(&path).unwrap().load().unwrap();
    let mut resumed = Fleet::open(loaded, false);
    assert_eq!(resumed.sim.hash(), b.sim.hash());
    a.run(500);
    resumed.run(500);
    assert_eq!(a.sim.hash(), resumed.sim.hash());
}
