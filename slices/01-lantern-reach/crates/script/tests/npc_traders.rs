//! Real orders, markets and core journey hooks; FakeMind, no GPU or network.
mod common;

use common::{core_data, slice};
use galaxy_contract::prelude::*;
use galaxy_core::{Save, Sim, SimState};
use galaxy_mind::executor::{Done, Executor, Job};
use galaxy_mind::menu::{RUMOR_SPREAD, TRADER_ROUTE};
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

/// Takes every model job and never answers: the model is still thinking when the
/// autosave is written.
#[derive(Default)]
struct Thinking(usize);

impl Executor for Thinking {
    fn submit(&mut self, _ticket: u64, _job: Job) -> bool {
        self.0 += 1;
        true
    }
    fn poll(&mut self) -> Vec<Done> {
        Vec::new()
    }
    fn in_flight(&self) -> usize {
        self.0
    }
}

/// A world as a save made before `craft.toml` holds it: no named traders or their
/// ships. (Their hulls stay, with the fixture's compressed travel; the kernel's
/// backfill test covers a save without them.)
fn older(s: &mut SimState) {
    for id in std::mem::take(&mut s.world.traders).into_keys() {
        s.world.ships.remove(&id);
    }
}

fn save_and_load(s: &SimState) -> SimState {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("save.sqlite");
    Save::create(&path, s).unwrap();
    Save::open(&path).unwrap().load().unwrap()
}

/// The shortage ignored from the start: it lands at once and needs no Kettle activity.
fn ignored_shortage(seed: u64) -> SimState {
    let mut s = state(seed);
    let mut setup = common::pilot::Setup { state: &mut s };
    setup.done(&["beat.b11_orders"]);
    setup.tunable("shortage.rimewater_ice.trigger_left", 1.0);
    setup.tunable("shortage.rimewater_ice.mined", 0.0);
    setup.tunable("shortage.rimewater_ice.ignored_pulses", 0.0);
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
        Self::build(s, default_only, Inline::new(FakeMind::new(seed)), true)
    }

    /// `backfill`: queue the tunables and named traders an older save lacks, as
    /// `Game::open` does.
    fn build(
        s: SimState,
        default_only: bool,
        exec: impl Executor + 'static,
        backfill: bool,
    ) -> Self {
        let mut sim = galaxy_core::game(s)
            .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)));
        if backfill {
            galaxy_core::queue_backfill(&mut sim, &core_data()).unwrap();
        }
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
        let mind = MindHost::new(config, exec, sim.history());
        Self { sim, mind, scripts }
    }

    fn step(&mut self) {
        let report = self.sim.step(&PlayerInput::default()).unwrap();
        // A market pulse that loses its compare-and-set to a trade in the same tick is
        // ordinary contention (it pulses again later); anything else is a bug.
        let rejected: Vec<_> = report
            .rejected
            .iter()
            .filter(|r| {
                !(matches!(&r.cause.kind, CauseKind::System { name } if name == "economy")
                    && r.cause.summary == "market pulse")
            })
            .collect();
        assert!(rejected.is_empty(), "{rejected:?}");
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

    fn fact(&self, kind: &str) -> Option<&Fact> {
        self.sim
            .history()
            .facts()
            .iter()
            .find(|f| f.kind.as_str() == kind)
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
    setup.tunable("dispute.hook.win", 4.0);
    setup.tunable("dispute.hook.lead", 0.0);
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
    f.until(|f| f.has("core.territory_changed"));
    assert_eq!(
        f.sim
            .world()
            .stations
            .get("station.cinder_hook")
            .unwrap()
            .owner
            .as_ref()
            .unwrap()
            .as_str(),
        "faction.tollway_compact",
        "NPC traffic can settle the dispute while the player stays docked"
    );
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

#[test]
fn older_save_gets_its_named_traders_and_the_ignored_shortage_resolves() {
    let mut s = ignored_shortage(103);
    older(&mut s);
    let mut f = Fleet::build(
        save_and_load(&s),
        true,
        Inline::new(FakeMind::new(103)),
        true,
    );
    f.until(|f| f.has("core.shortage_started"));
    f.until(|f| f.has("core.shortage_resolved"));
    assert_eq!(f.fact("core.shortage_resolved").unwrap().amount, Some(0));
    let w = f.sim.world();
    assert_eq!(w.traders.len(), 4, "every named trader joined the save");
    assert!(w.ships.contains(KEEL) && w.traders.contains_key(KEEL));
    assert!(w.player.flags.contains("mech.even_keel_relief"));
}

#[test]
fn older_save_without_a_newer_tunable_plays_past_the_ignored_shortage() {
    // A save made before named traders and before `relief_fallback_pulses`: the
    // shortage's dock handler reads the tunable once relief is flagged.
    const FALLBACK: &str = "shortage.rimewater_ice.relief_fallback_pulses";
    let mut s = ignored_shortage(107);
    older(&mut s);
    let authored = s.world.progress.tunables.remove(FALLBACK).unwrap();
    // `Fleet::step` fails on any script abort.
    let mut f = Fleet::build(
        save_and_load(&s),
        true,
        Inline::new(FakeMind::new(107)),
        true,
    );
    f.until(|f| f.has("core.shortage_started"));
    f.until(|f| f.has("core.shortage_resolved"));
    assert_eq!(f.fact("core.shortage_resolved").unwrap().amount, Some(0));
    let w = f.sim.world();
    assert!(w.player.flags.contains("mech.even_keel_relief"));
    assert_eq!(w.progress.tunables.get(FALLBACK), Some(&authored));
    // The save's own values stay.
    assert_eq!(
        w.progress.tunables["shortage.rimewater_ice.ignored_pulses"].value,
        TunableValue::Number(0.0)
    );
}

#[test]
fn ignored_shortage_resolves_without_even_keel() {
    // An older save the backfill couldn't reach: no Even Keel at all.
    let mut s = ignored_shortage(104);
    older(&mut s);
    // With no craft flying, the player's own dockings are the only activity.
    let player = s.world.player.ship.clone().unwrap();
    let wharf: StationId = "station.lantern_wharf".parse().unwrap();
    let kettle: StationId = KETTLE.parse().unwrap();
    s.world.ships.get_mut(&player).unwrap().orders = OrderQueue {
        orders: vec![
            Order::Undock {},
            Order::Dock {
                station: kettle.clone(),
            },
            Order::Undock {},
            Order::Dock { station: wharf },
            Order::Undock {},
            Order::Dock { station: kettle },
        ],
        ..OrderQueue::default()
    };
    let mut f = Fleet::build(
        save_and_load(&s),
        true,
        Inline::new(FakeMind::new(104)),
        false,
    );
    f.until(|f| f.has("core.shortage_started"));
    f.until(|f| f.has("core.shortage_resolved"));
    assert!(!f.sim.world().ships.contains(KEEL));
    let resolved = f.fact("core.shortage_resolved").unwrap();
    assert_eq!(resolved.amount, Some(0));
    assert_eq!(
        resolved.subjects.faction.as_ref().unwrap().as_str(),
        "faction.tollway_compact"
    );
}

#[test]
fn ignored_shortage_resolves_when_even_keel_never_reaches_kettle() {
    let mut s = ignored_shortage(105);
    let mut setup = common::pilot::Setup { state: &mut s };
    setup.tunable("shortage.rimewater_ice.relief_fallback_pulses", 6.0);
    // Even Keel is in the save but can't get the ice there: Kettle is off its route.
    let keel = s.world.traders.get_mut(KEEL).unwrap();
    keel.stations.remove(KETTLE);
    keel.preferences.clear();
    let mut f = Fleet::open(s, true);
    f.until(|f| f.has("core.shortage_resolved"));
    assert_eq!(f.fact("core.shortage_resolved").unwrap().amount, Some(0));
    assert!(!f.sim.history().facts().iter().any(|f| {
        f.kind.as_str() == "dock"
            && f.subjects.ship.as_ref().is_some_and(|s| s.as_str() == KEEL)
            && f.subjects
                .station
                .as_ref()
                .is_some_and(|s| s.as_str() == KETTLE)
    }));
}

#[test]
fn route_choices_pending_at_save_are_asked_again_on_restore() {
    // Every route choice goes to a model that is still thinking when the autosave is
    // written: the craft sit docked with no orders and the requests live only in the
    // host.
    let mut live = Fleet::build(state(106), false, Thinking::default(), true);
    live.until(|f| f.mind.in_flight() >= 2);
    let parked: Vec<ShipId> = live
        .sim
        .world()
        .ships
        .values()
        .filter(|s| {
            live.sim.world().traders.contains_key(&s.id)
                && s.docked_at.is_some()
                && s.orders.orders.is_empty()
        })
        .map(|s| s.id.clone())
        .collect();
    assert!(parked.len() >= 2, "{parked:?}");
    let saved = live.sim.history().last_fact_id().unwrap();
    let loaded = save_and_load(live.sim.state());

    let resume = || {
        let mut f = Fleet::build(loaded.clone(), false, Inline::new(FakeMind::new(106)), true);
        f.run(600);
        f
    };
    let (a, b) = (resume(), resume());
    assert_eq!(a.sim.hash(), b.sim.hash(), "the restore repeats exactly");
    for ship in &parked {
        let after: Vec<&Fact> = a
            .sim
            .history()
            .facts()
            .iter()
            .filter(|f| f.id > saved && f.subjects.ship.as_ref() == Some(ship))
            .collect();
        assert!(
            after.iter().any(|f| f.kind.as_str() == "undock"),
            "{ship} left its saved docking"
        );
        assert!(
            after.iter().any(|f| f.kind.as_str() == "dock"),
            "{ship} reached its next stop"
        );
    }
    // Each saved docking got exactly one answer: none were asked twice.
    let mut answered = std::collections::BTreeMap::<FactId, u32>::new();
    for f in a.sim.history().facts() {
        if f.kind.as_str() == galaxy_mind::DECISION_FACT
            && let Some(c) = a.sim.history().cause(f.cause)
        {
            for cite in &c.cause.cites {
                *answered.entry(*cite).or_default() += 1;
            }
        }
    }
    assert!(answered.values().all(|&n| n == 1), "{answered:?}");
}

/// Answers every model job at once except route choices, which it keeps thinking about.
struct RoutesThinking {
    inner: Inline,
    routes: usize,
}

impl Executor for RoutesThinking {
    fn submit(&mut self, ticket: u64, job: Job) -> bool {
        if matches!(&job, Job::Decide(r) if r.instructions.contains("trade plan")) {
            self.routes += 1;
            return true;
        }
        self.inner.submit(ticket, job)
    }
    fn poll(&mut self) -> Vec<Done> {
        self.inner.poll()
    }
    fn in_flight(&self) -> usize {
        self.routes + self.inner.in_flight()
    }
}

/// The menu kinds of the committed `mind.decision`s citing `trigger`.
fn decisions(sim: &Sim, trigger: FactId) -> Vec<String> {
    sim.history()
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == galaxy_mind::DECISION_FACT)
        .filter_map(|f| sim.history().cause(f.cause))
        .filter(|c| c.cause.cites.contains(&trigger))
        .filter_map(|c| match &c.cause.kind {
            CauseKind::Mind { decision, .. } => decision.split_once(':').map(|(k, _)| k.into()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_rumor_answered_at_the_docking_does_not_settle_its_pending_route_on_restore() {
    let mut s = state(109);
    let rumor: RumorId = "rumor.test_cargo".parse().unwrap();
    s.world.rumors.put(Rumor {
        id: rumor.clone(),
        claim: "A chart from Brassmouth".into(),
        is_true: false,
        heard_at: [BRASS.parse().unwrap()].into(),
        source: None,
        planted_by: None,
        heard_by_player: false,
    });
    // Fly until a craft leaves Brassmouth carrying the rumor.
    let mut a = Fleet::open(s, false);
    let last_stop = |sim: &Sim, ship: &ShipId| {
        sim.history()
            .facts()
            .iter()
            .rev()
            .find_map(|f| match &f.payload {
                FactPayload::Event(Event::Dock { station, .. })
                    if f.subjects.ship.as_ref() == Some(ship) =>
                {
                    Some(station.clone())
                }
                _ => None,
            })
    };
    let leaving = |f: &Fleet| {
        let w = f.sim.world();
        w.traders
            .keys()
            .find(|id| {
                let ship = w.ships.get(*id).unwrap();
                ship.docked_at.is_none()
                    && !ship.orders.orders.is_empty()
                    && last_stop(&f.sim, id).is_some_and(|s| s.as_str() == BRASS)
                    && w.rumors.get(&rumor).unwrap().heard_at.len() == 1
            })
            .cloned()
    };
    a.until(|f| leaving(f).is_some());
    let craft = leaving(&a).unwrap();
    let left = a.sim.history().last_fact_id().unwrap();

    // It docks with the rumor: that choice finishes while its route choice is still out.
    let mut b = Fleet::build(
        save_and_load(a.sim.state()),
        false,
        RoutesThinking {
            inner: Inline::new(FakeMind::new(109)),
            routes: 0,
        },
        true,
    );
    let docking = |sim: &Sim| {
        sim.history()
            .facts()
            .iter()
            .find(|f| {
                f.id > left
                    && f.subjects.ship.as_ref() == Some(&craft)
                    && matches!(f.payload, FactPayload::Event(Event::Dock { .. }))
            })
            .map(|f| f.id)
    };
    b.until(|f| docking(&f.sim).is_some_and(|d| !decisions(&f.sim, d).is_empty()));
    let docked = docking(&b.sim).unwrap();
    b.run(20);
    assert_eq!(
        decisions(&b.sim, docked),
        [RUMOR_SPREAD],
        "only the rumor answered"
    );
    let ship = b.sim.world().ships.get(&craft).unwrap();
    assert!(
        ship.docked_at.is_some() && ship.orders.orders.is_empty(),
        "{craft} is parked"
    );
    let saved = b.sim.history().last_fact_id().unwrap();

    // Restored, the route is asked again once and the craft flies on.
    let mut c = Fleet::open(save_and_load(b.sim.state()), false);
    c.run(650);
    let mut kinds = decisions(&c.sim, docked);
    kinds.sort();
    assert_eq!(kinds, [RUMOR_SPREAD, TRADER_ROUTE]);
    assert!(
        c.sim.history().facts().iter().any(|f| f.id > saved
            && f.subjects.ship.as_ref() == Some(&craft)
            && f.kind.as_str() == "undock"),
        "{craft} left the docking it was parked at"
    );
}
