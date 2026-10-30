//! One tick of one ship: input or orders in, one command batch out.
//!
//! The ship's state is copied in, changed in memory, and written back as a single
//! batch: motion, activity, order progress (a compare-and-set on the orders), every
//! verb's effects and events, and the facts they write. The kernel commits it whole or
//! rejects it whole, so a mine or trade that loses to another writer never advances
//! the order or writes its facts; the ship simply tries again next tick. The player's
//! clicks and presses are written ahead of the batch and only cleared by it, so they
//! are replayed rather than lost when it is rejected. The cause is the tick's first
//! verb (dock, undock, mine, trade, lane jump, order done), or the ship system's when
//! the ship only flew.

use galaxy_contract::geom;
use galaxy_contract::history::FactDraft;
use galaxy_contract::modifiers::Modifiers;
use galaxy_contract::prelude::*;

use crate::conditions;
use crate::controls::{self, Controls};
use crate::flight::{self, Feel, Hands, Motion, Steer};
use crate::regrowth::region_pulses;
use crate::space::{self, LaneWay, Press};
use crate::system::Claims;
use crate::trade::{Side, TradeDesk, TradeError, TradeRequest};

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

/// What asking the desk did.
enum Tried {
    /// Another ship traded on this line this tick: try again next tick.
    Busy,
    Moved(u32),
    Failed(TradeError),
}

pub(crate) struct ShipStep<'a, 'c> {
    w: &'a WorldState,
    desk: &'a dyn TradeDesk,
    claims: &'c mut Claims,
    ship: &'a Ship,
    hull: &'a Hull,
    /// The pilot's traits, fittings and hull ranks, added up (the flagship's), or a
    /// tender's hold bonus. Read through `galaxy_contract::modifiers`.
    mods: Modifiers,
    feel: Feel,
    actor: Actor,
    // The ship as it ends the tick.
    motion: Motion,
    /// `None`: motion was set directly (lane, docking); don't fly.
    steer: Option<Steer>,
    docked: Option<StationId>,
    region: RegionId,
    activity: ShipActivity,
    /// The activity as written ahead of this tick's batch, holding the player's new
    /// input (see [`ShipActivity::held_clicks`]); `None`: nothing written ahead.
    held: Option<ShipActivity>,
    queue: OrderQueue,
    /// Hold units added (+) or removed (-) by this tick's verbs.
    hold_delta: i64,
    /// The rock mined this tick, so a run that wasn't continued ends.
    mined: Option<RockId>,
    /// The verbs done this tick (the first names the cause) and what each did.
    verbs: Vec<VerbKind>,
    said: Vec<String>,
    /// Their effects, events and facts.
    commands: Vec<Command>,
}

impl<'a, 'c> ShipStep<'a, 'c> {
    pub(crate) fn new(
        w: &'a WorldState,
        desk: &'a dyn TradeDesk,
        claims: &'c mut Claims,
        ship: &'a Ship,
        hull: &'a Hull,
    ) -> Self {
        let flagship = w.player.ship.as_ref() == Some(&ship.id);
        let mods = galaxy_contract::modifiers::for_ship(w, &ship.id);
        let feel = Feel::of(&w.ship_rules.flight, hull).modified(&mods);
        Self {
            w,
            desk,
            claims,
            ship,
            hull,
            mods,
            feel,
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
            held: None,
            queue: ship.orders.clone(),
            hold_delta: 0,
            mined: None,
            verbs: Vec::new(),
            said: Vec::new(),
            commands: Vec::new(),
        }
    }

    /// Runs the tick. `input` is the player's, for the flagship only. Returns the
    /// tick's batch (none when nothing changed), preceded by one holding the player's
    /// new clicks and presses when there are any: if the tick's batch is rejected, they
    /// are replayed next tick instead of lost.
    pub(crate) fn run(mut self, input: Option<&PlayerInput>) -> Vec<CommandBatch> {
        // Input from ticks whose batch was rejected comes first.
        let mut clicks = std::mem::take(&mut self.activity.held_clicks);
        let replayed = std::mem::take(&mut self.activity.held_press);
        let fresh_press = input.is_some_and(|i| i.action && !self.activity.action_held);
        let pressed = replayed || fresh_press;
        if let Some(i) = input {
            self.activity.action_held = i.action;
            clicks.extend(i.click.clone());
        }
        let mut batches = Vec::new();
        if fresh_press || input.is_some_and(|i| i.click.is_some()) {
            let mut held = self.ship.activity.clone();
            held.held_clicks = clicks.clone();
            held.held_press = pressed;
            let mut batch = CommandBatch::new(Cause::system(
                "ship",
                format!("{} holds the player's input for its tick", self.ship.id),
            ));
            batch.push(Command::SetShipActivity {
                ship: self.ship.id.clone(),
                activity: Box::new(held.clone()),
            });
            batches.push(batch);
            self.held = Some(held);
        }
        self.act(input, &clicks, pressed);
        batches.extend(self.finish());
        batches
    }

    fn act(&mut self, input: Option<&PlayerInput>, clicks: &[Click], pressed: bool) {
        if let Some(t) = self.activity.transit.clone() {
            self.ride(t);
            return;
        }
        if let Some(input) = input {
            for click in clicks {
                self.click(click);
            }
            if input.thrust != Vec2::ZERO && !self.queue.orders.is_empty() {
                // Any WASD input takes the ship back by hand.
                self.queue = OrderQueue::default();
            }
            if self.idle() {
                self.by_hand(input, pressed);
                return;
            }
        }
        if !self.idle() {
            self.run_orders();
        }
    }

    fn idle(&self) -> bool {
        self.queue.current >= self.queue.orders.len()
    }

    // --- Writing back ---

    /// Notes a verb for the cause and returns the commands to add its effects to.
    fn verb(&mut self, verb: VerbKind, summary: String) -> &mut Vec<Command> {
        self.verbs.push(verb);
        self.said.push(summary);
        &mut self.commands
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

    fn finish(mut self) -> Option<CommandBatch> {
        if let Some(run) = &self.activity.mining
            && self.mined.as_ref() != Some(&run.rock)
        {
            self.end_mining(None);
        }
        if self.docked.is_some() {
            self.motion.velocity = Vec2::ZERO;
            self.activity.stopping = 0;
        } else if let Some(steer) = self.steer {
            if let Steer::Hands(hands) = steer {
                let (motion, stopping) = flight::fly(
                    &self.feel,
                    self.motion,
                    self.activity.stopping,
                    hands,
                    TICK_SECONDS,
                );
                self.motion = motion;
                self.activity.stopping = stopping;
            } else {
                self.motion = flight::step(&self.feel, self.motion, steer, TICK_SECONDS);
                self.activity.stopping = 0;
            }
            // Past the edge of its region, a current carries the ship back in.
            self.motion.position =
                space::edge_drift(self.w, &self.region, self.motion.position, TICK_SECONDS);
        } else {
            self.activity.stopping = 0;
        }
        let id = &self.ship.id;
        let mut commands = Vec::with_capacity(self.commands.len() + 3);
        if self.motion != Motion::of(self.ship) {
            commands.push(Command::SetShipMotion {
                ship: id.clone(),
                position: self.motion.position,
                velocity: self.motion.velocity,
                heading: self.motion.heading,
            });
        }
        // Clears any input held ahead of this batch.
        if &self.activity != self.held.as_ref().unwrap_or(&self.ship.activity) {
            commands.push(Command::SetShipActivity {
                ship: id.clone(),
                activity: Box::new(self.activity),
            });
        }
        commands.append(&mut self.commands);
        // Orders given since the tick began (a loop assigned, a script, a cancel) win:
        // this batch was worked out from the old ones, so it is rejected. That holds
        // even when the queue looks unchanged (a mine under way, a flight, a wait), so
        // every batch from a ship with orders carries the check.
        if self.queue != self.ship.orders
            || (!self.ship.orders.orders.is_empty() && !commands.is_empty())
        {
            commands.push(Command::UpdateOrders {
                ship: id.clone(),
                from: Box::new(self.ship.orders.clone()),
                to: Box::new(self.queue),
            });
        }
        if commands.is_empty() {
            return None;
        }
        let cause = match self.verbs.first() {
            Some(&verb) => Cause::verb(self.actor, verb, self.said.join("; ")),
            None => Cause::system("ship", format!("{id} flies")),
        };
        let mut batch = CommandBatch::new(cause);
        batch.commands = commands;
        Some(batch)
    }

    // --- By hand (tier 0) ---

    fn click(&mut self, click: &Click) {
        let controls = Controls::of(self.w);
        if let Some((order, append)) = controls::click_order(self.w, &controls, self.ship, click) {
            controls::apply_click(self.w, &mut self.queue, order, append);
        }
    }

    /// Tank controls fly the ship. A press of Space does the one thing in reach (dock,
    /// ride a lane current, start mining; [`space::press_at`]); held, it keeps mining
    /// and brakes. Docked, thrust (W or S) flies off and undocks; turning and Space do
    /// nothing there.
    fn by_hand(&mut self, input: &PlayerInput, pressed: bool) {
        let hands = Hands::of(input);
        let held = input.action;
        if self.docked.is_some() {
            if hands.thrust != 0.0 {
                self.undock();
            }
            return;
        }
        if pressed {
            match space::press_at(self.w, self.ship, self.motion.position) {
                Some(Press::Dock(station)) => {
                    self.dock(station.id.clone());
                    return;
                }
                Some(Press::Ride(way)) => {
                    self.enter_lane(way);
                    return;
                }
                Some(Press::Mine(_)) | None => {}
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
                .filter(|r| {
                    self.claims.left(r) > 0 && space::in_reach(self.w, r, self.motion.position)
                })
        } else {
            None
        };
        if let Some(rock) = rock {
            self.mine(&rock.id);
        }
        self.steer = Some(Steer::Hands(hands));
    }

    // --- Verbs ---

    fn undock(&mut self) {
        let Some(station) = self.docked.take() else {
            return;
        };
        let id = self.ship.id.clone();
        let commands = self.verb(VerbKind::Undock, format!("{id} undocked from {station}"));
        commands.push(Command::SetShipDocked {
            ship: id.clone(),
            station: None,
        });
        commands.push(Command::Emit {
            event: Event::Undock { ship: id, station },
        });
        self.steer = Some(Steer::Brake);
    }

    fn dock(&mut self, station: StationId) {
        let id = self.ship.id.clone();
        let commands = self.verb(VerbKind::Dock, format!("{id} docked at {station}"));
        commands.push(Command::SetShipDocked {
            ship: id.clone(),
            station: Some(station.clone()),
        });
        commands.push(Command::Emit {
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
                self.motion.heading = geom::atan2(dir.y, dir.x);
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
        let commands = self.verb(VerbKind::Fly, summary);
        commands.push(Command::SetShipRegion {
            ship: id.clone(),
            region: t.to.clone(),
        });
        commands.push(Command::Emit {
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
        galaxy_contract::modifiers::hold_capacity(self.w, self.ship)
            .saturating_sub(used.max(0) as u64)
    }

    /// Mines `rock` for one tick. Returns true once the run is over (hold full or rock
    /// spent).
    fn mine(&mut self, rock_id: &RockId) -> bool {
        let Some(rock) = self.w.rocks.get(rock_id) else {
            return true;
        };
        // What earlier ships haven't already taken this tick.
        let left = self.claims.left(rock);
        if self.free_hold() == 0 || left == 0 {
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
        let rate = self.hull.mining_rate.max(0.0) * self.mods.mining_rate.max(0.0) as f32;
        run.progress += rate * hardness.max(0.0) * TICK_SECONDS;
        let whole = run.progress.floor() as u64;
        let units = whole.min(self.free_hold()).min(u64::from(left)) as u32;
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
            *self.claims.mined.entry(rock_id.clone()).or_default() += units;
            self.queue.progress.lap_moved = true;
            let id = self.ship.id.clone();
            let commands = self.verb(VerbKind::Mine, format!("{id} mines {rock_id}"));
            commands.push(Command::AdjustRock {
                rock: rock_id.clone(),
                delta: -i64::from(units),
            });
            commands.push(Command::AdjustCargo {
                ship: id,
                ware,
                delta: i64::from(units),
            });
        }
        self.activity.mining = Some(run);
        self.mined = Some(rock_id.clone());
        if units >= left {
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
        let mut commands = Vec::new();
        if run.mined > 0
            && let Some(ware) = ware
        {
            commands.push(Command::Emit {
                event: Event::Mine {
                    ship: id.clone(),
                    rock: run.rock.clone(),
                    ware,
                    amount: run.mined,
                },
            });
        }
        if let Some(rock) = depleted {
            commands.push(Command::Emit {
                event: Event::RockDepleted {
                    rock,
                    ship: Some(id.clone()),
                },
            });
        }
        if !commands.is_empty() {
            let summary = format!("{id} mined {} from {}", run.mined, run.rock);
            self.verb(VerbKind::Mine, summary).append(&mut commands);
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
        p.credits = 0;
        p.waiting = None;
        p.blocked = false;
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
        // Between laps: the world now shows what the last lap's final order did.
        if std::mem::take(&mut self.queue.progress.lap_end)
            && until.is_some_and(|c| conditions::holds(self.w, Some(self.ship), c))
        {
            self.order_done(VerbKind::Loop, &Outcome::Done);
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
        // `until` is checked next tick, once this tick's batch has committed.
        self.queue.progress.lap_end = until.is_some();
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
            Order::Tag { rock } => {
                if !self.w.player.flags.contains(crate::live::ROCK_TAGGING)
                    && !matches!(self.ship.owner, Owner::Npc { .. })
                {
                    return Some(Outcome::Skipped("rock lamps are off".into()));
                }
                let Some(r) = self.w.rocks.get(rock) else {
                    return Some(Outcome::Skipped("no rock".into()));
                };
                let dest = Dest {
                    region: self.w.fields.get(&r.field)?.region.clone(),
                    point: Some(r.position),
                    within: self.w.ship_rules.mining.reach,
                    stop: true,
                };
                if !matches!(self.travel(&dest), Travel::Arrived) {
                    return None;
                }
                if self.w.rock_tags.len() >= 6 && !self.w.rock_tags.contains(rock) {
                    return Some(Outcome::Skipped("six lamps already burn".into()));
                }
                if !matches!(self.ship.owner, Owner::Npc { .. }) {
                    // The pilot's craft lose their lamps with the mechanic.
                    self.commands.push(Command::ExpectFlag {
                        flag: FlagId::new(crate::live::ROCK_TAGGING).unwrap(),
                        on: true,
                    });
                }
                self.commands.push(Command::SetRockTag {
                    rock: rock.clone(),
                    on: true,
                });
                self.fact(
                    VerbKind::Tag,
                    "tagged",
                    None,
                    format!("a lamp burns on {rock}"),
                );
                Some(Outcome::Done)
            }
            Order::MineField {
                field,
                prefer_tagged,
            } => {
                // Finish the rock we started before choosing the next richest one.
                // Otherwise each unit mined can make another rock richer and send
                // the ship back and forth without completing either run.
                if let Some(run) = &self.activity.mining {
                    let rock = run.rock.clone();
                    return self.exec_mine(&rock);
                }
                let tagged =
                    *prefer_tagged && self.w.player.flags.contains(crate::live::ROCK_TAGGING);
                let rock = self
                    .w
                    .rocks
                    .values()
                    .filter(|r| &r.field == field && r.remaining > 0)
                    .max_by_key(|r| (tagged && self.w.rock_tags.contains(&r.id), r.remaining))
                    .map(|r| r.id.clone());
                match rock {
                    Some(id) => self.exec_mine(&id),
                    None => Some(Outcome::Skipped("field is spent".into())),
                }
            }
            Order::Dock { station } => {
                if self.docked.as_ref() == Some(station) {
                    return Some(Outcome::Done);
                }
                let Some(s) = self.w.stations.get(station) else {
                    return Some(Outcome::Skipped(format!("no station {station}")));
                };
                if s.services.is_empty() {
                    return Some(Outcome::Skipped("the ring is closed".into()));
                }
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
            Order::Transfer { ware, quantity, to } => Some(self.exec_transfer(ware, quantity, to)),
        }
    }

    /// The Coupling: hands cargo to one of the player's tenders alongside, in one go.
    fn exec_transfer(&mut self, ware: &WareId, quantity: &Quantity, to: &ShipId) -> Outcome {
        if !self.hull.coupling {
            return Outcome::Skipped("only a coupled hull can hand cargo over".into());
        }
        let Some(tender) = self.w.ships.get(to).filter(|t| {
            t.id != self.ship.id
                && t.owner == Owner::Player
                && !self.w.player.is_berthed(&t.id)
                && self.w.hulls.get(&t.hull).is_some_and(|h| h.standing_berth)
        }) else {
            return Outcome::Skipped(format!("{to} isn't one of your tenders"));
        };
        let alongside = match (&self.docked, &tender.docked_at) {
            (Some(a), Some(b)) => a == b,
            (None, None) => {
                tender.region == self.region
                    && tender.position.distance(self.motion.position)
                        <= self.w.ship_rules.docking.transfer_reach
            }
            _ => false,
        };
        if !alongside {
            return Outcome::Skipped(format!("{to} isn't alongside"));
        }
        let have = self.ship.hold.count(ware);
        let free = galaxy_contract::modifiers::hold_capacity(self.w, tender)
            .saturating_sub(tender.hold.total());
        let want = match quantity {
            Quantity::Units { n } => *n,
            Quantity::All => u32::MAX,
        };
        let n = want.min(have).min(u32::try_from(free).unwrap_or(u32::MAX));
        if n == 0 {
            return Outcome::Skipped(if have == 0 {
                format!("no {ware} to hand over")
            } else {
                format!("{to}'s hold is full")
            });
        }
        self.hold_delta -= i64::from(n);
        self.queue.progress.lap_moved = true;
        let id = self.ship.id.clone();
        let fact = FactDraft {
            kind: FactKind::from_static("transfer"),
            subjects: Subjects {
                ship: Some(id.clone()),
                station: self.docked.clone(),
                ware: Some(ware.clone()),
                region: Some(self.region.clone()),
                other: AnyId::new(to.as_str()).ok(),
                ..Subjects::default()
            },
            amount: Some(i64::from(n)),
            note: format!("{id} handed {n} {ware} to {to}"),
        };
        let commands = self.verb(VerbKind::Transfer, format!("{id} hands {n} {ware} to {to}"));
        commands.push(Command::AdjustCargo {
            ship: id,
            ware: ware.clone(),
            delta: -i64::from(n),
        });
        commands.push(Command::AdjustCargo {
            ship: to.clone(),
            ware: ware.clone(),
            delta: i64::from(n),
        });
        commands.push(Command::RecordFact {
            fact: Box::new(fact),
        });
        Outcome::Done
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
                // Park on the near side, just off the rock's drawn edge.
                let r = self.w.rocks.get(rock)?;
                let out = (self.motion.position - r.position).normalize_or_zero();
                let out = if out == Vec2::ZERO {
                    Vec2::new(1.0, 0.0)
                } else {
                    out
                };
                let gap = self.park_gap(r);
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
        // Mining starts once the ship is parked beside the rock, so it never slides
        // on from wherever it first came into reach; a run under way carries on.
        let mining = self
            .activity
            .mining
            .as_ref()
            .is_some_and(|m| &m.rock == rock_id);
        let parked = self.motion.position.distance(rock.position)
            <= self.park_gap(rock) + self.w.ship_rules.flight.arrive_distance.max(1.0)
            && self.motion.velocity.length() <= self.feel.brake * TICK_SECONDS;
        let ready = self.docked.is_none()
            && &self.region == region
            && space::in_reach(self.w, rock, self.motion.position)
            && (mining || parked);
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
        if self.claims.left(rock) == 0 {
            // Another ship took the last of it this tick; see next tick whether it did.
            return None;
        }
        self.mine(rock_id).then_some(Outcome::Done)
    }

    /// How far from a rock's centre a mine order parks: its drawn edge plus the
    /// standoff, and never past mining reach.
    fn park_gap(&self, rock: &Rock) -> f32 {
        let m = &self.w.ship_rules.mining;
        space::rock_radius(self.w, rock) + m.standoff.clamp(0.0, m.reach.max(0.0))
    }

    /// Asks the desk for a trade at the station the ship is docked at, and adds it to
    /// this tick's batch.
    fn trade(&mut self, side: Side, ware: &WareId, quantity: u32, limit: Option<u32>) -> Tried {
        let Some(station) = self.docked.clone() else {
            return Tried::Failed(TradeError::Refused("not docked".into()));
        };
        if self.ship.docked_at.as_ref() != Some(&station) {
            // Docked this tick: the desk reads the world as the tick began.
            return Tried::Busy;
        }
        let line = (station.clone(), ware.clone());
        if self.claims.lines.contains(&line) {
            return Tried::Busy;
        }
        let request = TradeRequest {
            ship: self.ship.id.clone(),
            station: station.clone(),
            ware: ware.clone(),
            side,
            quantity,
            limit,
        };
        let trade = match self.desk.trade(self.w, &request) {
            Ok(trade) => trade,
            Err(e) => return Tried::Failed(e),
        };
        if trade.units > 0 {
            self.claims.lines.insert(line);
            let id = self.ship.id.clone();
            let (verb, what) = match side {
                Side::Buy => (VerbKind::Buy, "bought"),
                Side::Sell => (VerbKind::Sell, "sold"),
            };
            let summary = format!("{id} {what} {} {ware} at {station}", trade.units);
            self.verb(verb, summary).extend(trade.commands);
            let sign = if side == Side::Buy { 1 } else { -1 };
            self.hold_delta += sign * i64::from(trade.units);
            let p = &mut self.queue.progress;
            p.traded += trade.units;
            p.credits += trade.total;
            p.lap_moved = true;
        }
        Tried::Moved(trade.units)
    }

    /// Buys up to the quantity, clipped to the free hold. Stock, credits and the price
    /// are the desk's to judge: whatever it can't do ends the order calmly.
    fn exec_buy(&mut self, ware: &WareId, quantity: &Quantity) -> Option<Outcome> {
        if self.docked.is_none() {
            return Some(Outcome::Skipped("not docked".into()));
        }
        let free = u32::try_from(self.free_hold()).unwrap_or(u32::MAX);
        let want = match quantity {
            Quantity::Units { n } => n.saturating_sub(self.queue.progress.traded),
            Quantity::All => free,
        }
        .min(free);
        if want == 0 {
            return Some(Outcome::Skipped(if free == 0 {
                "the hold is full".into()
            } else {
                "nothing to buy".into()
            }));
        }
        Some(match self.trade(Side::Buy, ware, want, None) {
            Tried::Busy => return None,
            Tried::Moved(n) if n > 0 => Outcome::Done,
            Tried::Moved(_) => Outcome::Skipped("bought nothing".into()),
            Tried::Failed(e) => Outcome::Skipped(e.to_string()),
        })
    }

    /// Sells while the price is at or above `floor`. A crewed ship then keeps the rest;
    /// a hull with a standing berth waits docked until the price is back (the patient
    /// sell), or, if its floor is above what the station can pay at all, waits blocked.
    /// Re-checked every tick against the market; never a timer.
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
        let patient = floor.filter(|_| self.hull.standing_berth);
        if let Some(floor) = patient
            && self.blocked(&station, ware, floor)
        {
            return None;
        }
        match self.trade(Side::Sell, ware, want, floor) {
            Tried::Busy => None,
            Tried::Moved(sold) if sold >= want => {
                self.sold_out(&station, ware, floor);
                Some(Outcome::Done)
            }
            // Part sold; next tick sees the new price.
            Tried::Moved(sold) if sold > 0 => None,
            Tried::Moved(_) => Some(if traded > 0 {
                Outcome::Done
            } else {
                Outcome::Skipped("sold nothing".into())
            }),
            Tried::Failed(e) if e.is_price_limit() => {
                let Some(floor) = patient else {
                    // A crewed ship doesn't wait: it keeps the rest.
                    return Some(if traded > 0 {
                        Outcome::Done
                    } else {
                        Outcome::Skipped(e.to_string())
                    });
                };
                if self.queue.progress.waiting.is_none() {
                    self.queue.progress.waiting = Some(region_pulses(self.w, &self.region));
                    let price = match e {
                        TradeError::PastLimit { price, .. } => price,
                        _ => 0,
                    };
                    self.fact(
                        VerbKind::Sell,
                        "patient_wait",
                        Some(i64::from(price)),
                        format!(
                            "{} waits at {station} for {ware} to reach {} cr (now {} cr)",
                            self.ship.id,
                            show_credits(i64::from(floor)),
                            show_credits(i64::from(price))
                        ),
                    );
                }
                None
            }
            // Nothing waiting can fix (no market, not bought here ...): move on.
            Tried::Failed(e) => Some(if traded > 0 {
                Outcome::Done
            } else {
                Outcome::Skipped(e.to_string())
            }),
        }
    }

    /// Whether a patient sell's floor is above what the station can pay now. Writes
    /// `patient_blocked` when it becomes so and `patient_unblocked` when the ceiling is
    /// back; the floor and the cargo are never touched.
    fn blocked(&mut self, station: &StationId, ware: &WareId, floor: u32) -> bool {
        let ceiling = self.desk.ceiling(self.w, station, ware);
        let blocked = ceiling.is_some_and(|c| floor > c);
        if blocked != self.queue.progress.blocked {
            self.queue.progress.blocked = blocked;
            let id = self.ship.id.clone();
            let ceiling = ceiling.unwrap_or(0);
            let (most, least) = (
                show_credits(i64::from(ceiling)),
                show_credits(i64::from(floor)),
            );
            if blocked {
                self.fact(
                    VerbKind::Sell,
                    "patient_blocked",
                    Some(i64::from(ceiling)),
                    format!(
                        "{id} can't sell {ware} at {station}: its floor {least} cr is above \
                         the most it pays now ({most} cr)"
                    ),
                );
            } else {
                self.fact(
                    VerbKind::Sell,
                    "patient_unblocked",
                    Some(i64::from(ceiling)),
                    format!(
                        "{id} can sell {ware} at {station} again: it pays up to {most} cr, \
                         the floor is {least} cr"
                    ),
                );
            }
        }
        blocked
    }

    /// The stack is gone. A floored sale from a standing berth is a patient sell, with
    /// or without a wait on the way (b13 counts both).
    fn sold_out(&mut self, station: &StationId, ware: &WareId, floor: Option<u32>) {
        let Some(floor) = floor.filter(|_| self.hull.standing_berth) else {
            return;
        };
        let p = &self.queue.progress;
        let (sold, credits) = (p.traded, p.credits);
        let average = if sold > 0 {
            credits / i64::from(sold)
        } else {
            0
        };
        let waited = p.waiting.map_or(0, |since| {
            region_pulses(self.w, &self.region).saturating_sub(since)
        });
        self.fact(
            VerbKind::Sell,
            "patient_sold",
            Some(i64::from(sold)),
            format!(
                "{} patiently sold {sold} {ware} at {station}: {} cr a unit, floor {} cr, \
                 {waited} pulses waited",
                self.ship.id,
                show_credits(average),
                show_credits(i64::from(floor))
            ),
        );
    }
}
