//! A headless pilot for the journey: the game's real composition (ship systems with
//! the economy desk, the economy, progress), the script host running `packs/core`
//! (and nothing else), and the fast-model layer on its seeded fake, driven by the
//! player's input like the journey bot in `content/07-pacing.md`.
//!
//! Everything here goes through the one write path the app uses: `PlayerInput`
//! (WASD, Space, trades, services, replies) and, for control tiers the player has
//! unlocked, the flagship's order queue. The helpers are meant to move into the journey
//! bot (`galaxy sim run --bot journey`) and `beat jump` as they are: each is a policy
//! step ("dock at Kettle", "mine this field until the hold is full"), not a script of
//! positions.
//!
//! Shortcuts that set up a later act's state (beats done, flags, fittings, credits,
//! counters, a lower pacing tunable) go through [`Setup`] before the sim starts, so a
//! test can begin at Act 3 without replaying Acts 1 and 2. Pacing is data: lowering a
//! tunable is how a test keeps a long wait short.
#![allow(dead_code)]

use std::collections::BTreeSet;

use galaxy_contract::history::Fact;
use galaxy_contract::prelude::*;
use galaxy_contract::progress::BeatState;
use galaxy_contract::talk::{Channel, SaidLine};
use galaxy_contract::verbs::{FlyTarget, Order, OrderQueue, Quantity};
use galaxy_core::{Sim, SimState, System, new_game};
use galaxy_mind::{FakeMind, Inline, MindConfig, MindHost};
use galaxy_progress::ProgressSystem;
use galaxy_script::ScriptHost;
use galaxy_script::host::HostEvent;

use super::{core_data, slice};

pub const PLAYER: ShipId = ShipId::from_static("ship.player");
pub const WHARF: StationId = StationId::from_static("station.lantern_wharf");
pub const KETTLE: StationId = StationId::from_static("station.kettle_dock");
pub const HOOK: StationId = StationId::from_static("station.cinder_hook");
pub const GDO: StationId = StationId::from_static("station.glass_drift_observatory");
pub const BRASS: StationId = StationId::from_static("station.brassmouth_exchange");
pub const ICE: WareId = WareId::from_static("ware.ice");
pub const ORE: WareId = WareId::from_static("ware.iron_ore");
pub const PLATE: WareId = WareId::from_static("ware.refined_plate");
pub const GLASSITE: WareId = WareId::from_static("ware.glassite");
pub const FRAGMENT: WareId = WareId::from_static("ware.ancients_fragment");
pub const RIMEWATER: FieldId = FieldId::from_static("field.shallows.ice");
pub const TINROOT: FieldId = FieldId::from_static("field.shallows.iron");
pub const GLASS_DRIFT: FieldId = FieldId::from_static("field.cinder.glassite");
pub const SHALLOWS: RegionId = RegionId::from_static("region.lantern_shallows");
pub const CINDER: RegionId = RegionId::from_static("region.cinder_drift");
pub const NARROWS: RegionId = RegionId::from_static("region.brassmouth_narrows");

/// Hands-off ticks a wait gives up after (activity, not time: the sim counts ticks).
const PATIENCE: u32 = 20_000;

/// State a test sets up before the sim starts: a later act's starting point.
pub struct Setup<'a> {
    pub state: &'a mut SimState,
}

impl Setup<'_> {
    pub fn world(&mut self) -> &mut WorldState {
        &mut self.state.world
    }

    /// Seeds a focused act fixture without completion facts. The app's `beat jump`
    /// instead plays earlier beats, preserving their real history and citations.
    pub fn done(&mut self, beats: &[&str]) {
        let w = &mut self.state.world;
        for (i, id) in beats.iter().enumerate() {
            let id: BeatId = id.parse().unwrap();
            w.progress.beats.insert(
                id,
                BeatState {
                    pack: "core".parse().unwrap(),
                    act: 1,
                    kind: BeatKind::Directed,
                    status: BeatStatus::Completed,
                    step: 0,
                    step_id: None,
                    goal: String::new(),
                    giver: None,
                    since: None,
                    step_since: None,
                    done_since: None,
                    started: Tick(i as u64),
                },
            );
        }
    }

    pub fn credits(&mut self, credits: i64) {
        self.state.world.player.credits = credits * CENTS_PER_CREDIT;
    }

    pub fn flags(&mut self, flags: &[&str]) {
        for f in flags {
            self.state.world.player.flags.insert(f.parse().unwrap());
        }
    }

    pub fn fitted(&mut self, upgrades: &[&str]) {
        for u in upgrades {
            self.state.world.player.fittings.insert(u.parse().unwrap());
        }
    }

    /// Sets a player activity counter (as if that much had happened).
    pub fn counter(&mut self, kind: CounterKind, n: u64) {
        let c = &mut self.state.world.counters;
        let have = c.get(kind, &CounterScope::Player);
        c.bump(kind, &CounterScope::Player, n.saturating_sub(have));
    }

    pub fn station_counter(&mut self, kind: CounterKind, station: &StationId, n: u64) {
        let c = &mut self.state.world.counters;
        let scope = CounterScope::Station {
            station: station.clone(),
        };
        let have = c.get(kind, &scope);
        c.bump(kind, &scope, n.saturating_sub(have));
    }

    /// Sets a pacing tunable's value (pacing is data).
    pub fn tunable(&mut self, id: &str, value: f64) {
        let t = self
            .state
            .world
            .progress
            .tunables
            .get_mut(id)
            .unwrap_or_else(|| panic!("no tunable {id}"));
        t.value = TunableValue::Number(value);
        t.authored = TunableValue::Number(value);
    }

    /// Docks the player's ship at `station` (a later act starts there).
    pub fn docked_at(&mut self, station: &StationId) {
        let w = &mut self.state.world;
        let s = w.stations.get(station).unwrap().clone();
        let ship = w.ships.get_mut(&PLAYER).unwrap();
        ship.region = s.region.clone();
        ship.position = s.position;
        ship.velocity = Vec2::ZERO;
        ship.docked_at = Some(s.id.clone());
    }
}

pub struct Pilot {
    pub sim: Sim,
    pub host: ScriptHost,
    pub mind: MindHost,
    /// Batches the kernel rejected (contention is normal; faults are not).
    pub rejected: usize,
    /// The latest rejections' causes and reasons, for failure messages.
    pub rejections: Vec<String>,
}

impl Pilot {
    /// A new game with only the core pack installed.
    pub fn new(seed: u64) -> Self {
        Self::with(seed, |_| {})
    }

    /// A new game, set up by `setup` before the first tick.
    pub fn with(seed: u64, setup: impl FnOnce(&mut Setup<'_>)) -> Self {
        let mut state = new_game(&core_data(), seed).expect("core data loads");
        setup(&mut Setup { state: &mut state });
        let mut sim = galaxy_core::game(state)
            .with_systems(galaxy_ship::systems(Box::new(galaxy_ship::EconomyDesk)))
            .with_systems([
                Box::new(galaxy_economy::EconomySystem) as Box<dyn System>,
                Box::new(ProgressSystem),
            ]);
        let host = ScriptHost::new(None);
        let report = host.load(&slice().join("packs/core"), sim.world(), sim.history());
        assert!(report.accepted(), "{report:#?}");
        host.attach(&mut sim);
        let config = MindConfig::load_dir(&core_data()).expect("core mind.toml loads");
        let mind = MindHost::new(config, Inline::new(FakeMind::new(seed)), sim.history());
        Self {
            sim,
            host,
            mind,
            rejected: 0,
            rejections: Vec::new(),
        }
    }

    pub fn world(&self) -> &WorldState {
        self.sim.world()
    }

    pub fn ship(&self) -> &Ship {
        self.world().ships.get(&PLAYER).expect("the player's ship")
    }

    pub fn docked(&self) -> Option<&StationId> {
        self.ship().docked_at.as_ref()
    }

    pub fn credits(&self) -> i64 {
        self.world().player.credits / CENTS_PER_CREDIT
    }

    pub fn cargo(&self, ware: &WareId) -> u32 {
        self.ship().hold.count(ware)
    }

    // --- Ticking --------------------------------------------------------------------

    /// One tick with `input`, then the fast-model layer's pump. A script fault fails
    /// the test with the script's own message.
    pub fn tick(&mut self, input: &PlayerInput) {
        let report = self.sim.step(input).expect("not paused");
        self.rejected += report.rejected.len();
        for r in &report.rejected {
            if self.rejections.len() >= 20 {
                self.rejections.remove(0);
            }
            self.rejections
                .push(format!("{}: {}: {}", r.tick.0, r.cause.summary, r.reason));
        }
        self.mind.pump(&mut self.sim);
        for e in self.host.take_events() {
            match e {
                HostEvent::Aborted { .. }
                | HostEvent::Quarantined { .. }
                | HostEvent::InstallRefused { .. } => {
                    panic!("a core script failed: {e:#?}")
                }
                _ => {}
            }
        }
    }

    pub fn idle(&mut self, ticks: u32) {
        for _ in 0..ticks {
            self.tick(&PlayerInput::default());
        }
    }

    /// Hands off until `done` holds. Panics after [`PATIENCE`] ticks with `what`.
    pub fn wait(&mut self, what: &str, done: impl Fn(&Pilot) -> bool) {
        for _ in 0..PATIENCE {
            if done(self) {
                return;
            }
            self.idle(1);
        }
        panic!("waited too long for {what}: at {:?}", self.ship().position);
    }

    /// Space for one tick (dock, undock, enter a lane, start mining).
    pub fn press(&mut self) {
        self.tick(&PlayerInput {
            action: true,
            ..PlayerInput::default()
        });
        self.idle(1);
    }

    // --- Tier 0: by hand ------------------------------------------------------------

    /// Flies by WASD to `to` (in the current region) and stops there.
    pub fn fly_to(&mut self, to: Vec2, close: f32) {
        for _ in 0..PATIENCE {
            let off = to - self.ship().position;
            if off.length() <= close {
                break;
            }
            let stick = off.normalize_or_zero() * (off.length() / 60.0).min(1.0);
            self.tick(&PlayerInput {
                thrust: stick,
                ..PlayerInput::default()
            });
        }
        assert!(
            self.ship().position.distance(to) <= close + 1.0,
            "didn't get to {to:?}: at {:?}",
            self.ship().position
        );
        for _ in 0..30 {
            if self.ship().velocity == Vec2::ZERO {
                return;
            }
            self.idle(1);
        }
    }

    /// Undocks with Space.
    pub fn undock(&mut self) {
        if self.docked().is_some() {
            self.press();
        }
        assert!(self.docked().is_none(), "still docked");
    }

    /// Flies into `station`'s ring in this region by hand and presses Space.
    pub fn dock_by_hand(&mut self, station: &StationId) {
        self.undock();
        let s = self.world().stations.get(station).unwrap().clone();
        assert_eq!(
            self.ship().region,
            s.region,
            "{station} is in another region"
        );
        self.fly_to(s.position, (s.dock_radius * 0.5).max(5.0));
        self.press();
        assert_eq!(self.docked(), Some(station), "didn't dock at {station}");
        self.idle(4);
    }

    /// Rides `lane` by hand from this region's mouth: fly in, press Space, ride.
    pub fn ride_lane_by_hand(&mut self, lane: &LaneId) {
        self.undock();
        let l = self.world().lanes.get(lane).unwrap().clone();
        let here = self.ship().region.clone();
        let (gate, to) = if l.a == here {
            (l.a_gate, l.b.clone())
        } else {
            (l.b_gate, l.a.clone())
        };
        self.fly_to(gate, 10.0);
        self.press();
        self.wait("the lane to carry the ship", |p| {
            p.ship().region == to && p.ship().activity.transit.is_none()
        });
        self.idle(2);
    }

    /// Mines `rock` by hand (Space held) until the hold is full or the rock is spent.
    pub fn mine_by_hand(&mut self, rock: &RockId) {
        self.undock();
        let r = self.world().rocks.get(rock).unwrap().clone();
        let radius = galaxy_ship::space::rock_radius(self.world(), &r);
        let from = self.ship().position;
        let spot = r.position + (from - r.position).normalize_or_zero() * (radius + 15.0);
        self.fly_to(spot, 5.0);
        for _ in 0..PATIENCE {
            let r = self.world().rocks.get(rock).unwrap();
            if self.hold_free() == 0 || r.remaining == 0 {
                break;
            }
            self.tick(&PlayerInput {
                action: true,
                ..PlayerInput::default()
            });
        }
        self.idle(2); // let go of Space: the run ends and writes its `mine` fact
    }

    // --- Orders (click to fly, dock and mine; map travel; queues and loops) ---------

    /// Replaces the flagship's orders, as clicks and the queue do.
    pub fn orders(&mut self, orders: Vec<Order>) {
        self.sim.submit(
            CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Fly,
                "the player's orders",
            ))
            .with(Command::SetOrders {
                ship: PLAYER,
                orders: OrderQueue {
                    orders,
                    ..OrderQueue::default()
                },
            }),
        );
        self.idle(1);
    }

    fn idle_orders(&self) -> bool {
        let q = &self.ship().orders;
        q.current >= q.orders.len()
    }

    /// Runs the flagship's queue until it's empty.
    pub fn run(&mut self, orders: Vec<Order>) {
        self.orders(orders);
        self.wait("the orders to finish", |p| p.idle_orders());
        self.idle(4);
    }

    /// Docks at `station` by order (routing through lanes with map travel).
    pub fn go(&mut self, station: &StationId) {
        if self.docked() == Some(station) {
            return;
        }
        self.run(vec![Order::Dock {
            station: station.clone(),
        }]);
        assert_eq!(self.docked(), Some(station), "didn't dock at {station}");
    }

    /// Flies to a point by order.
    pub fn go_to(&mut self, at: Vec2) {
        self.run(vec![Order::Fly {
            to: FlyTarget::Point { at },
        }]);
    }

    /// Mines `field`'s richest rocks by order until the hold is full (or the field is
    /// spent).
    pub fn mine_field(&mut self, field: &FieldId) {
        for _ in 0..20 {
            if self.hold_free() == 0 {
                return;
            }
            let Some(rock) = self.richest(field) else {
                return;
            };
            self.run(vec![Order::Mine { rock }]);
        }
    }

    /// The unspent rock in `field` with the most left.
    pub fn richest(&self, field: &FieldId) -> Option<RockId> {
        self.world()
            .rocks
            .values()
            .filter(|r| &r.field == field && r.remaining > 0)
            .max_by_key(|r| (r.remaining, std::cmp::Reverse(r.id.clone())))
            .map(|r| r.id.clone())
    }

    pub fn hold_free(&self) -> u64 {
        galaxy_progress::eval::hold_free(self.world(), Some(self.ship()))
    }

    // --- The dock screen ------------------------------------------------------------

    fn trade(&mut self, ware: &WareId, side: TradeSide, quantity: u32) {
        let station = self.docked().expect("trades happen docked").clone();
        self.tick(&PlayerInput {
            trades: vec![TradeRequest {
                ship: PLAYER,
                station,
                ware: ware.clone(),
                side,
                quantity,
                limit: Some(match side {
                    TradeSide::Sell => 0,
                    TradeSide::Buy => u32::MAX,
                }),
            }],
            ..PlayerInput::default()
        });
        self.idle(2);
    }

    /// Sells up to `n` of a ware here (fills what it can).
    pub fn sell(&mut self, ware: &WareId, n: u32) {
        self.trade(ware, TradeSide::Sell, n);
    }

    pub fn sell_all(&mut self, ware: &WareId) {
        let n = self.cargo(ware);
        if n > 0 {
            self.sell(ware, n);
        }
    }

    /// Buys up to `n` of a ware here (as many as stock, hold and credits allow).
    pub fn buy(&mut self, ware: &WareId, n: u32) {
        self.trade(ware, TradeSide::Buy, n);
    }

    /// Asks for a service (fit, build, prestige) and checks it happened.
    pub fn service(&mut self, req: ServiceRequest) {
        if let Err(e) = galaxy_progress::services::check(self.world(), self.sim.history(), &req) {
            panic!("{req:?} isn't offered: {e:?}");
        }
        self.tick(&PlayerInput {
            services: vec![req],
            ..PlayerInput::default()
        });
        self.idle(2);
    }

    pub fn fit(&mut self, upgrade: &str) {
        self.service(ServiceRequest::Fit {
            upgrade: upgrade.parse().unwrap(),
        });
        assert!(
            self.world().player.fittings.contains(upgrade),
            "{upgrade} fitted"
        );
    }

    /// Answers someone at the dock: a line's choice, or a standing question.
    pub fn reply(&mut self, npc: &str, choice: &str) {
        self.tick(&PlayerInput {
            replies: vec![Reply {
                npc: npc.parse().unwrap(),
                choice: choice.parse().unwrap(),
            }],
            ..PlayerInput::default()
        });
        self.idle(3);
    }

    /// Opens and closes the PAUSED card, as the app does: the `paused` fact commits on
    /// the first tick after it closes.
    pub fn pause(&mut self) {
        self.sim.submit(
            CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Wait,
                "the player opened the PAUSED card",
            ))
            .with(Command::Emit {
                event: Event::Paused { session: 1 },
            }),
        );
        self.sim.set_paused(true);
        assert!(self.sim.step(&PlayerInput::default()).is_none(), "paused");
        self.sim.set_paused(false);
        self.idle(3);
    }

    // --- Reading the journey --------------------------------------------------------

    pub fn status(&self, beat: &str) -> Option<BeatStatus> {
        self.world().progress.beats.get(beat).map(|b| b.status)
    }

    pub fn is_done(&self, beat: &str) -> bool {
        self.status(beat) == Some(BeatStatus::Completed)
    }

    pub fn step_of(&self, beat: &str) -> Option<u32> {
        self.world()
            .progress
            .beats
            .get(beat)
            .filter(|b| b.status == BeatStatus::Active)
            .map(|b| b.step)
    }

    /// Facts of a kind, oldest first.
    pub fn facts(&self, kind: &str) -> Vec<&Fact> {
        self.sim
            .history()
            .facts()
            .iter()
            .filter(|f| f.kind.as_str() == kind)
            .collect()
    }

    pub fn has_fact(&self, kind: &str) -> bool {
        !self.facts(kind).is_empty()
    }

    /// Whether `beat` completed (its `beat_completed` fact exists).
    pub fn completed(&self, beat: &str) -> bool {
        self.facts("beat_completed").iter().any(|f| {
            f.subjects
                .other
                .as_ref()
                .is_some_and(|o| o.as_str() == beat)
        })
    }

    /// Dock talk said so far.
    pub fn talk(&self) -> Vec<&SaidLine> {
        self.world().talk.on(Channel::DockTalk).collect()
    }

    /// Whether someone said a line containing `needle` (every line cites a fact).
    pub fn heard(&self, npc: &str, needle: &str) -> bool {
        self.talk().iter().any(|l| {
            l.line.speaker.as_ref().is_some_and(|s| s.as_str() == npc)
                && l.line.text.contains(needle)
                && !l.line.cites.is_empty()
        })
    }

    /// The lines said by ID (`core.npc_line` facts), oldest first.
    pub fn lines_said(&self) -> Vec<String> {
        self.facts("core.npc_line")
            .iter()
            .filter_map(|f| f.subjects.other.as_ref().map(|o| o.to_string()))
            .collect()
    }

    /// Fact kinds written so far.
    pub fn kinds(&self) -> BTreeSet<String> {
        self.sim
            .history()
            .facts()
            .iter()
            .map(|f| f.kind.to_string())
            .collect()
    }

    /// No core script ever aborted or was quarantined.
    pub fn assert_healthy(&self) {
        for (pack, state) in &self.world().packs {
            for (script, h) in &state.health {
                assert!(
                    h.aborts == 0 && !h.quarantined,
                    "{pack}/{script}: {} aborts, quarantined {}",
                    h.aborts,
                    h.quarantined
                );
            }
        }
        assert!(!self.has_fact("script_quarantined"));
        assert!(self.world().player.credits >= 0, "credits never negative");
    }
}

/// `Quantity::All`, for orders.
pub const ALL: Quantity = Quantity::All;
