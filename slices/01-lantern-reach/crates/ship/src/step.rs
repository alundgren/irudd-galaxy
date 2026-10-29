//! One tick of one ship: input or orders in, command batches out.
//!
//! The ship's state is copied in, changed in memory, and written back with at most
//! one motion/activity/orders batch (a system cause) plus one batch per verb that
//! happened (a verb cause: dock, undock, mine, trade, lane jump, order done), so
//! history says who did what.

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;

use crate::conditions;
use crate::controls::{self, Controls};
use crate::flight::{self, Feel, Motion, Steer};
use crate::space::{self, LaneWay};
use crate::trade::{Side, TradeDesk, TradeRequest};

/// How an order ended. It goes into the `order_done` cause summary.
#[derive(Clone, Debug, PartialEq)]
enum Outcome {
    Done,
    /// It couldn't act (no stock, nothing to sell, a spent rock ...). Calm: the queue
    /// moves on.
    Skipped(String),
    /// A crewed ship can't wait docked: it clears the ring.
    Cleared,
}

impl Outcome {
    fn describe(&self) -> String {
        match self {
            Outcome::Done => "done".into(),
            Outcome::Skipped(why) => format!("skipped: {why}"),
            Outcome::Cleared => "cleared the ring".into(),
        }
    }
}

/// Where an order is headed: a region, and a point in it (none: just get there).
struct Dest {
    region: RegionId,
    point: Option<Vec2>,
    within: f32,
    /// Arrive only once slow enough to stop this tick (a point to park on), rather
    /// than as soon as inside `within` (a ring to dock in).
    stop: bool,
}

enum Travel {
    Arrived,
    Going,
    NoRoute,
}

pub(crate) struct ShipStep<'a> {
    w: &'a WorldState,
    desk: &'a dyn TradeDesk,
    ship: &'a Ship,
    hull: &'a Hull,
    feel: Feel,
    actor: Actor,
    // The ship as it ends the tick.
    motion: Motion,
    /// `None`: motion was set directly (lane, docking); don't fly.
    steer: Option<Steer>,
    docked: Option<StationId>,
    region: RegionId,
    activity: ShipActivity,
    queue: OrderQueue,
    /// Hold units added (+) or removed (-) by this tick's batches.
    hold_delta: i64,
    /// The rock mined this tick, so a run that wasn't continued ends.
    mined: Option<RockId>,
    out: Vec<CommandBatch>,
}

impl<'a> ShipStep<'a> {
    pub(crate) fn new(
        w: &'a WorldState,
        desk: &'a dyn TradeDesk,
        ship: &'a Ship,
        hull: &'a Hull,
    ) -> Self {
        let flagship = w.player.ship.as_ref() == Some(&ship.id);
        Self {
            w,
            desk,
            ship,
            hull,
            feel: Feel::of(&w.ship_rules.flight, hull),
            actor: if flagship {
                Actor::Player
            } else {
                Actor::Ship {
                    ship: ship.id.clone(),
                }
            },
            motion: Motion::of(ship),
            steer: Some(Steer::Brake),
            docked: ship.docked_at.clone(),
            region: ship.region.clone(),
            activity: ship.activity.clone(),
            queue: ship.orders.clone(),
            hold_delta: 0,
            mined: None,
            out: Vec::new(),
        }
    }

    /// Runs the tick. `input` is the player's, for the flagship only.
    pub(crate) fn run(mut self, input: Option<&PlayerInput>) -> Vec<CommandBatch> {
        let pressed = input.is_some_and(|i| i.action && !self.activity.action_held);
        if let Some(i) = input {
            self.activity.action_held = i.action;
        }
        if let Some(t) = self.activity.transit.clone() {
            self.ride(t);
            return self.finish();
        }
        if let Some(input) = input {
            if let Some(click) = &input.click {
                self.click(click);
            }
            let thrust = input.thrust.clamp_length(1.0);
            if thrust != Vec2::ZERO && !self.queue.orders.is_empty() {
                // Any WASD input takes the ship back by hand.
                self.queue = OrderQueue::default();
            }
            if self.idle() {
                self.by_hand(thrust, pressed, input.action);
                return self.finish();
            }
        }
        if !self.idle() {
            self.run_orders();
        }
        self.finish()
    }

    fn idle(&self) -> bool {
        self.queue.current >= self.queue.orders.len()
    }

    // --- Writing back ---

    fn verb(&mut self, verb: VerbKind, summary: String) -> &mut CommandBatch {
        self.out.push(CommandBatch::new(Cause::verb(
            self.actor.clone(),
            verb,
            summary,
        )));
        self.out.last_mut().expect("just pushed")
    }

    fn emit(&mut self, verb: VerbKind, summary: String, event: Event) {
        self.verb(verb, summary).push(Command::Emit { event });
    }

    fn fact(&mut self, verb: VerbKind, kind: &'static str, amount: Option<i64>, note: String) {
        let fact = FactDraft {
            kind: FactKind::from_static(kind),
            subjects: Subjects {
                ship: Some(self.ship.id.clone()),
                station: self.docked.clone(),
                region: Some(self.region.clone()),
                ..Subjects::default()
            },
            amount,
            note: note.clone(),
        };
        self.verb(verb, note).push(Command::RecordFact {
            fact: Box::new(fact),
        });
    }

    fn finish(mut self) -> Vec<CommandBatch> {
        if let Some(run) = &self.activity.mining
            && self.mined.as_ref() != Some(&run.rock)
        {
            self.end_mining(None);
        }
        if self.docked.is_some() {
            self.motion.velocity = Vec2::ZERO;
        } else if let Some(steer) = self.steer {
            self.motion = flight::step(&self.feel, self.motion, steer, TICK_SECONDS);
        }
        let id = &self.ship.id;
        let mut base = CommandBatch::new(Cause::system("ship", format!("{id} flies")));
        if self.motion != Motion::of(self.ship) {
            base.push(Command::SetShipMotion {
                ship: id.clone(),
                position: self.motion.position,
                velocity: self.motion.velocity,
                heading: self.motion.heading,
            });
        }
        if self.activity != self.ship.activity {
            base.push(Command::SetShipActivity {
                ship: id.clone(),
                activity: Box::new(self.activity),
            });
        }
        if self.queue != self.ship.orders {
            base.push(Command::SetOrders {
                ship: id.clone(),
                orders: self.queue,
            });
        }
        let mut out = Vec::with_capacity(self.out.len() + 1);
        if !base.commands.is_empty() {
            out.push(base);
        }
        out.append(&mut self.out);
        out
    }

    // --- By hand (tier 0) ---

    fn click(&mut self, click: &Click) {
        let controls = Controls::of(self.w);
        if let Some((order, append)) = controls::click_order(self.w, &controls, self.ship, click) {
            controls::apply_click(self.w, &mut self.queue, order, append);
        }
    }

    /// WASD flies; the context action does the one thing in reach.
    fn by_hand(&mut self, thrust: Vec2, pressed: bool, held: bool) {
        if self.docked.is_some() {
            if pressed || thrust != Vec2::ZERO {
                self.undock();
            }
            return;
        }
        if pressed {
            if let Some(station) = space::ring_at(self.w, &self.region, self.motion.position) {
                self.dock(station.id.clone());
                return;
            }
            if let Some(way) = space::mouth_at(self.w, &self.region, self.motion.position) {
                self.enter_lane(way);
                return;
            }
        }
        // A press starts a run on the nearest rock; holding keeps it going. When the
        // run ends (rock spent, hold full, out of reach) it takes a new press.
        let rock = if pressed {
            space::rock_in_reach(self.w, &self.region, self.motion.position)
        } else if held {
            self.activity
                .mining
                .as_ref()
                .and_then(|m| self.w.rocks.get(&m.rock))
                .filter(|r| r.remaining > 0 && space::in_reach(self.w, r, self.motion.position))
        } else {
            None
        };
        if let Some(rock) = rock {
            self.mine(&rock.id);
        }
        self.steer = Some(Steer::Thrust(thrust));
    }

    // --- Verbs ---

    fn undock(&mut self) {
        let Some(station) = self.docked.take() else {
            return;
        };
        let id = self.ship.id.clone();
        self.verb(VerbKind::Undock, format!("{id} undocked from {station}"))
            .push(Command::SetShipDocked {
                ship: id.clone(),
                station: None,
            });
        self.emit(
            VerbKind::Undock,
            format!("{id} undocked from {station}"),
            Event::Undock { ship: id, station },
        );
        self.steer = Some(Steer::Brake);
    }

    fn dock(&mut self, station: StationId) {
        let id = self.ship.id.clone();
        let batch = self.verb(VerbKind::Dock, format!("{id} docked at {station}"));
        batch.push(Command::SetShipDocked {
            ship: id.clone(),
            station: Some(station.clone()),
        });
        batch.push(Command::Emit {
            event: Event::Dock {
                ship: id,
                station: station.clone(),
            },
        });
        self.docked = Some(station);
        self.motion.velocity = Vec2::ZERO;
        self.steer = None;
    }

    fn enter_lane(&mut self, way: LaneWay) {
        self.activity.transit = Some(Transit {
            lane: way.lane,
            from: way.from,
            to: way.to,
            start: self.motion.position,
            end: way.end,
            travelled: 0.0,
        });
        self.steer = None;
    }

    /// Carried by the lane current: no steering, every hull at the lane speed.
    fn ride(&mut self, mut t: Transit) {
        self.steer = None;
        let path = t.end - t.start;
        let length = path.length();
        let dir = path.normalize_or_zero();
        t.travelled += self.w.ship_rules.lanes.speed * TICK_SECONDS;
        if t.travelled < length {
            self.motion.position = t.start + dir * t.travelled;
            self.motion.velocity = dir * self.w.ship_rules.lanes.speed;
            if dir != Vec2::ZERO {
                self.motion.heading = dir.y.atan2(dir.x);
            }
            self.activity.transit = Some(t);
            return;
        }
        // Out of the far mouth, stopped.
        self.motion.position = t.end;
        self.motion.velocity = Vec2::ZERO;
        self.activity.transit = None;
        self.region = t.to.clone();
        let id = self.ship.id.clone();
        let summary = format!("{id} rode {} into {}", t.lane, t.to);
        let batch = self.verb(VerbKind::Fly, summary);
        batch.push(Command::SetShipRegion {
            ship: id.clone(),
            region: t.to.clone(),
        });
        batch.push(Command::Emit {
            event: Event::Jump {
                ship: id,
                lane: t.lane,
                from: t.from,
                to: t.to,
            },
        });
    }

    fn free_hold(&self) -> u64 {
        let used = i64::try_from(self.ship.hold.total()).unwrap_or(i64::MAX) + self.hold_delta;
        u64::from(self.hull.hold_capacity).saturating_sub(used.max(0) as u64)
    }

    /// Mines `rock` for one tick. Returns true once the run is over (hold full or rock
    /// spent).
    fn mine(&mut self, rock_id: &RockId) -> bool {
        let Some(rock) = self.w.rocks.get(rock_id) else {
            return true;
        };
        if self.free_hold() == 0 || rock.remaining == 0 {
            self.end_mining(None);
            return true;
        }
        let hardness = self
            .w
            .rock_types
            .get(&rock.rock_type)
            .map_or(1.0, |t| t.hardness);
        let mut run = match self.activity.mining.take() {
            Some(run) if &run.rock == rock_id => run,
            other => {
                self.activity.mining = other;
                self.end_mining(None);
                MiningRun {
                    rock: rock_id.clone(),
                    progress: 0.0,
                    mined: 0,
                }
            }
        };
        run.progress += self.hull.mining_rate.max(0.0) * hardness.max(0.0) * TICK_SECONDS;
        let whole = run.progress.floor() as u64;
        let units = whole.min(self.free_hold()).min(u64::from(rock.remaining)) as u32;
        let ware = self
            .w
            .rock_types
            .get(&rock.rock_type)
            .map(|t| t.ware.clone());
        if units > 0
            && let Some(ware) = ware
        {
            run.progress -= units as f32;
            run.mined += units;
            self.hold_delta += i64::from(units);
            self.queue.progress.lap_moved = true;
            let id = self.ship.id.clone();
            let batch = self.verb(VerbKind::Mine, format!("{id} mines {rock_id}"));
            batch.push(Command::AdjustRock {
                rock: rock_id.clone(),
                delta: -i64::from(units),
            });
            batch.push(Command::AdjustCargo {
                ship: id,
                ware,
                delta: i64::from(units),
            });
        }
        self.activity.mining = Some(run);
        self.mined = Some(rock_id.clone());
        if units >= rock.remaining {
            self.end_mining(Some(rock_id.clone()));
            return true;
        }
        if self.free_hold() == 0 {
            self.end_mining(None);
            return true;
        }
        false
    }

    /// Ends the mining run: one `mine` event for the whole run, and `rock_depleted`
    /// if this ship spent the rock.
    fn end_mining(&mut self, depleted: Option<RockId>) {
        let Some(run) = self.activity.mining.take() else {
            return;
        };
        self.mined = None;
        let id = self.ship.id.clone();
        let ware = self
            .w
            .rocks
            .get(&run.rock)
            .and_then(|r| self.w.rock_types.get(&r.rock_type))
            .map(|t| t.ware.clone());
        let batch = self.verb(
            VerbKind::Mine,
            format!("{id} mined {} from {}", run.mined, run.rock),
        );
        if run.mined > 0
            && let Some(ware) = ware
        {
            batch.push(Command::Emit {
                event: Event::Mine {
                    ship: id.clone(),
                    rock: run.rock.clone(),
                    ware,
                    amount: run.mined,
                },
            });
        }
        if let Some(rock) = depleted {
            batch.push(Command::Emit {
                event: Event::RockDepleted {
                    rock,
                    ship: Some(id),
                },
            });
        }
        if batch.commands.is_empty() {
            self.out.pop();
        }
    }

    // --- The order runner: the same verbs for every ship ---

    fn run_orders(&mut self) {
        let order = self.queue.orders[self.queue.current].clone();
        match order {
            Order::Loop { orders, until } => self.run_loop(&orders, until.as_ref()),
            order => {
                if let Some(outcome) = self.exec(&order) {
                    self.order_done(order.verb(), &outcome);
                    self.next_order(order.verb());
                }
            }
        }
    }

    fn order_done(&mut self, verb: VerbKind, outcome: &Outcome) {
        let id = self.ship.id.clone();
        let summary = format!("{id}: {verb:?} {}", outcome.describe()).to_lowercase();
        self.emit(verb, summary, Event::OrderDone { ship: id, verb });
        let p = &mut self.queue.progress;
        p.traded = 0;
        p.waiting = false;
    }

    /// Moves to the next top-level order; an empty queue when the last is done.
    fn next_order(&mut self, verb: VerbKind) {
        self.queue.current += 1;
        self.queue.progress = OrderProgress::default();
        if self.idle() {
            let n = self.queue.orders.len();
            if n >= 3 {
                self.fact(
                    verb,
                    "queue_done",
                    Some(n as i64),
                    format!("{} finished a queue of {n} orders", self.ship.id),
                );
            }
            self.queue = OrderQueue::default();
        }
    }

    fn run_loop(&mut self, orders: &[Order], until: Option<&Condition>) {
        if orders.is_empty() {
            self.order_done(VerbKind::Loop, &Outcome::Skipped("an empty loop".into()));
            self.next_order(VerbKind::Loop);
            return;
        }
        let step = self.queue.progress.step.min(orders.len() - 1);
        let inner = &orders[step];
        let outcome = match inner {
            Order::Loop { .. } => Some(Outcome::Skipped("a loop inside a loop".into())),
            order => self.exec(order),
        };
        let Some(outcome) = outcome else {
            return;
        };
        self.order_done(inner.verb(), &outcome);
        self.queue.progress.step = step + 1;
        if step + 1 < orders.len() {
            return;
        }
        // A lap is done.
        let p = &mut self.queue.progress;
        p.laps += 1;
        p.step = 0;
        let (laps, moved) = (p.laps, p.lap_moved);
        p.lap_moved = false;
        let id = self.ship.id.clone();
        if !moved {
            // Nothing moved all lap: stop calmly and say so.
            self.fact(
                VerbKind::Loop,
                "loop_stalled",
                Some(laps as i64),
                format!("{id}'s loop stalled: a whole lap moved nothing"),
            );
            self.order_done(VerbKind::Loop, &Outcome::Skipped("stalled".into()));
            self.next_order(VerbKind::Loop);
            return;
        }
        self.fact(
            VerbKind::Loop,
            "loop_lap",
            Some(laps as i64),
            format!("{id} finished lap {laps}"),
        );
        if until.is_some_and(|c| conditions::holds(self.w, Some(self.ship), c)) {
            self.order_done(VerbKind::Loop, &Outcome::Done);
            self.next_order(VerbKind::Loop);
        }
    }

    /// Runs one order for a tick. `Some` when it has finished.
    fn exec(&mut self, order: &Order) -> Option<Outcome> {
        match order {
            Order::Fly { to } => {
                let Some(dest) = self.dest(to) else {
                    return Some(Outcome::Skipped("nowhere to fly".into()));
                };
                match self.travel(&dest) {
                    Travel::Arrived => Some(Outcome::Done),
                    Travel::Going => None,
                    Travel::NoRoute => Some(Outcome::Skipped("no lane goes there".into())),
                }
            }
            Order::Mine { rock } => self.exec_mine(rock),
            Order::Dock { station } => {
                if self.docked.as_ref() == Some(station) {
                    return Some(Outcome::Done);
                }
                let Some(s) = self.w.stations.get(station) else {
                    return Some(Outcome::Skipped(format!("no station {station}")));
                };
                let dest = Dest {
                    region: s.region.clone(),
                    point: Some(s.position),
                    within: s.dock_radius * self.w.ship_rules.docking.approach,
                    stop: false,
                };
                match self.travel(&dest) {
                    Travel::Arrived => {
                        self.dock(station.clone());
                        Some(Outcome::Done)
                    }
                    Travel::Going => None,
                    Travel::NoRoute => Some(Outcome::Skipped("no lane goes there".into())),
                }
            }
            Order::Undock {} => {
                self.undock();
                Some(Outcome::Done)
            }
            Order::Buy { ware, quantity } => self.exec_buy(ware, quantity),
            Order::Sell {
                ware,
                quantity,
                floor,
            } => self.exec_sell(ware, quantity, *floor),
            Order::Wait { until } => {
                if self.docked.is_some() && !self.hull.standing_berth {
                    return Some(Outcome::Cleared);
                }
                conditions::holds(self.w, Some(self.ship), until).then_some(Outcome::Done)
            }
            Order::Loop { .. } => Some(Outcome::Skipped("a loop inside a loop".into())),
        }
    }

    fn dest(&self, to: &FlyTarget) -> Option<Dest> {
        let arrive = self.w.ship_rules.flight.arrive_distance;
        Some(match to {
            FlyTarget::Point { at } => Dest {
                region: space::region_at(self.w, *at)
                    .cloned()
                    .unwrap_or_else(|| self.region.clone()),
                point: Some(*at),
                within: arrive,
                stop: true,
            },
            FlyTarget::Station { station } => {
                let s = self.w.stations.get(station)?;
                Dest {
                    region: s.region.clone(),
                    point: Some(s.position),
                    within: s.dock_radius * self.w.ship_rules.docking.approach,
                    stop: false,
                }
            }
            FlyTarget::Rock { rock } => {
                // Park on the near side, well inside mining reach of the edge.
                let r = self.w.rocks.get(rock)?;
                let out = (self.motion.position - r.position).normalize_or_zero();
                let out = if out == Vec2::ZERO {
                    Vec2::new(1.0, 0.0)
                } else {
                    out
                };
                let gap = space::rock_radius(self.w, r) + self.w.ship_rules.mining.reach * 0.5;
                Dest {
                    region: space::rock_region(self.w, r)?.clone(),
                    point: Some(r.position + out * gap),
                    within: arrive,
                    stop: true,
                }
            }
            FlyTarget::Region { region } => Dest {
                region: self.w.regions.get(region)?.id.clone(),
                point: None,
                within: arrive,
                stop: true,
            },
        })
    }

    /// Steers toward `dest` for a tick: undocks, routes through lanes, arrives.
    fn travel(&mut self, dest: &Dest) -> Travel {
        if self.docked.is_some() {
            self.undock();
            return Travel::Going;
        }
        if self.region != dest.region {
            let Some(way) = space::route(self.w, &self.region, &dest.region) else {
                return Travel::NoRoute;
            };
            let mouth = self.w.ship_rules.lanes.mouth_radius;
            if self.motion.position.distance(way.start) <= mouth {
                self.enter_lane(way);
            } else {
                self.steer = Some(Steer::Arrive(way.start));
            }
            return Travel::Going;
        }
        match dest.point {
            None => Travel::Arrived,
            Some(p)
                if self.motion.position.distance(p) <= dest.within
                    && (!dest.stop
                        || self.motion.velocity.length() <= self.feel.brake * TICK_SECONDS) =>
            {
                self.steer = Some(Steer::Brake);
                Travel::Arrived
            }
            Some(p) => {
                self.steer = Some(Steer::Arrive(p));
                Travel::Going
            }
        }
    }

    fn exec_mine(&mut self, rock_id: &RockId) -> Option<Outcome> {
        let Some(rock) = self.w.rocks.get(rock_id) else {
            return Some(Outcome::Skipped(format!("no rock {rock_id}")));
        };
        if rock.remaining == 0 {
            return Some(Outcome::Skipped("the rock is spent".into()));
        }
        if self.free_hold() == 0 {
            return Some(Outcome::Done);
        }
        let region = space::rock_region(self.w, rock)?;
        let ready = self.docked.is_none()
            && &self.region == region
            && space::in_reach(self.w, rock, self.motion.position);
        if !ready {
            let dest = self.dest(&FlyTarget::Rock {
                rock: rock_id.clone(),
            })?;
            return match self.travel(&dest) {
                Travel::NoRoute => Some(Outcome::Skipped("no lane goes there".into())),
                _ => None,
            };
        }
        self.steer = Some(Steer::Brake);
        self.mine(rock_id).then_some(Outcome::Done)
    }

    fn market_price(&self, station: &StationId, ware: &WareId) -> Option<u32> {
        Some(self.w.markets.get(station)?.lines.get(ware)?.price)
    }

    fn trade(
        &mut self,
        side: Side,
        ware: &WareId,
        quantity: u32,
        limit: Option<u32>,
    ) -> Result<u32, String> {
        let Some(station) = self.docked.clone() else {
            return Err("not docked".into());
        };
        let request = TradeRequest {
            ship: self.ship.id.clone(),
            station: station.clone(),
            ware: ware.clone(),
            side,
            quantity,
            limit,
        };
        let trade = self
            .desk
            .trade(self.w, &request)
            .map_err(|e| e.to_string())?;
        if trade.units > 0 {
            let id = self.ship.id.clone();
            let (verb, what) = match side {
                Side::Buy => (VerbKind::Buy, "bought"),
                Side::Sell => (VerbKind::Sell, "sold"),
            };
            let summary = format!("{id} {what} {} {ware} at {station}", trade.units);
            self.verb(verb, summary).commands.extend(trade.commands);
            let sign = if side == Side::Buy { 1 } else { -1 };
            self.hold_delta += sign * i64::from(trade.units);
            self.queue.progress.traded += trade.units;
            self.queue.progress.lap_moved = true;
        }
        Ok(trade.units)
    }

    fn exec_buy(&mut self, ware: &WareId, quantity: &Quantity) -> Option<Outcome> {
        if self.docked.is_none() {
            return Some(Outcome::Skipped("not docked".into()));
        }
        let want = match quantity {
            Quantity::Units { n } => n.saturating_sub(self.queue.progress.traded),
            Quantity::All => u32::MAX,
        };
        Some(match self.trade(Side::Buy, ware, want, None) {
            Ok(n) if n > 0 => Outcome::Done,
            Ok(_) => Outcome::Skipped("bought nothing".into()),
            Err(e) => Outcome::Skipped(e),
        })
    }

    /// Sells while the price is at or above `floor`. A crewed ship then keeps the rest;
    /// a hull with a standing berth waits docked until the price is back (the patient
    /// sell). Re-checked every tick against the market; never a timer.
    fn exec_sell(
        &mut self,
        ware: &WareId,
        quantity: &Quantity,
        floor: Option<u32>,
    ) -> Option<Outcome> {
        let Some(station) = self.docked.clone() else {
            return Some(Outcome::Skipped("not docked".into()));
        };
        let traded = self.queue.progress.traded;
        let held = self.ship.hold.count(ware);
        let want = match quantity {
            Quantity::Units { n } => n.saturating_sub(traded),
            Quantity::All => held,
        }
        .min(held);
        if want == 0 {
            return Some(if traded > 0 {
                self.sold_out(&station, ware, floor);
                Outcome::Done
            } else {
                Outcome::Skipped("nothing to sell".into())
            });
        }
        let Some(price) = self.market_price(&station, ware) else {
            return Some(Outcome::Skipped(format!("{station} doesn't buy {ware}")));
        };
        let below = floor.is_some_and(|f| price < f);
        let sold = if below {
            0
        } else {
            match self.trade(Side::Sell, ware, want, floor) {
                Ok(n) => n,
                Err(e) if traded == 0 && floor.is_none() => return Some(Outcome::Skipped(e)),
                Err(_) => 0,
            }
        };
        if sold >= want {
            self.sold_out(&station, ware, floor);
            return Some(Outcome::Done);
        }
        if sold > 0 {
            // Part sold; next tick sees the new price.
            return None;
        }
        // Nothing sold at this price.
        let Some(floor) = floor.filter(|_| self.hull.standing_berth) else {
            return Some(if traded > 0 {
                Outcome::Done
            } else {
                Outcome::Skipped(format!("the price {price} is under the floor"))
            });
        };
        if !self.queue.progress.waiting {
            self.queue.progress.waiting = true;
            self.fact(
                VerbKind::Sell,
                "patient_wait",
                Some(i64::from(price)),
                format!(
                    "{} waits at {station} for {ware} to reach {floor} (now {price})",
                    self.ship.id
                ),
            );
        }
        None
    }

    fn sold_out(&mut self, station: &StationId, ware: &WareId, floor: Option<u32>) {
        if !self.queue.progress.waiting {
            return;
        }
        let traded = self.queue.progress.traded;
        let floor = floor.unwrap_or(0);
        self.fact(
            VerbKind::Sell,
            "patient_sold",
            Some(i64::from(traded)),
            format!(
                "{} patiently sold {traded} {ware} at {station} (floor {floor})",
                self.ship.id
            ),
        );
    }
}
