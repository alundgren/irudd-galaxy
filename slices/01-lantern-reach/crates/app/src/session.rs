//! One play session over a `Sim`, with no window: the window ([`crate::play`]), the
//! headless shot ([`crate::shot`]) and the tests all drive it the same way.
//!
//! - **Real pause.** [`Session::pause`] stops the sim and submits a `paused` fact every
//!   time the card opens (content spec b05). The sim doesn't tick while paused, so the
//!   fact commits on the first tick after the card closes; if the game quits paused,
//!   the batch is saved with the kernel's pending batches and commits next time.
//! - **Dock trades** go through [`DockTrades`]: submitted ahead of each tick so they
//!   win their market line, resubmitted until they apply or are refused, and settled
//!   only by their own committed events.
//! - **Poses** from before and after the last tick, so drawing can interpolate between
//!   fixed ticks.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_core::bus::Subscriber;
use galaxy_core::{Sim, SimState};

use crate::trades::{DockTrades, Settled};

/// The app's cause name for what it submits itself.
pub const APP: &str = "app";

/// Where a ship is and which way it faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: Vec2,
    pub heading: f32,
}

impl Pose {
    fn of(ship: &Ship) -> Self {
        Self {
            position: ship.position,
            heading: ship.heading,
        }
    }

    /// The pose `t` (0..=1) of the way from `self` to `to`, turning the short way.
    pub fn lerp(self, to: Pose, t: f32) -> Pose {
        let t = t.clamp(0.0, 1.0);
        let p = self.position + (to.position - self.position) * t;
        let tau = std::f32::consts::TAU;
        let d = (to.heading - self.heading + std::f32::consts::PI).rem_euclid(tau)
            - std::f32::consts::PI;
        Pose {
            position: p,
            heading: self.heading + d * t,
        }
    }
}

/// What a tick did that the app reacts to.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TickOutcome {
    /// False when the sim didn't step (paused).
    pub stepped: bool,
    /// The flagship docked this tick: a calm moment to autosave.
    pub docked: Option<StationId>,
    /// The flagship left a station this tick.
    pub undocked: bool,
    /// The dock-screen trade intent's fate.
    pub trade: Option<Settled>,
}

pub struct Session {
    sim: Sim,
    sub: Subscriber,
    /// The `sessions` counter for this play session (0 until [`Session::begin`]).
    session: u64,
    pub trades: DockTrades,
    /// Poses before the last tick, for interpolation.
    before: BTreeMap<ShipId, Pose>,
    /// Events committed by the last tick.
    last_events: Vec<EventEnvelope>,
}

impl Session {
    pub fn new(state: SimState) -> Self {
        let mut sim = crate::game::game(state);
        let sub = sim.subscribe("app");
        let before = poses(sim.world());
        Self {
            sim,
            sub,
            session: 0,
            trades: DockTrades::default(),
            before,
            last_events: Vec::new(),
        }
    }

    /// Starts a play session: submits `session_start` (it commits on the first tick).
    pub fn begin(&mut self) {
        let sessions = self
            .sim
            .world()
            .counters
            .get(CounterKind::Sessions, &CounterScope::Player);
        self.session = sessions + 1;
        self.sim.submit(
            CommandBatch::new(Cause::system(APP, "the game was opened")).with(Command::Emit {
                event: Event::SessionStart {},
            }),
        );
    }

    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn sim(&self) -> &Sim {
        &self.sim
    }

    /// The sim, for code that works between ticks (the game-master bridge).
    pub fn sim_mut(&mut self) -> &mut Sim {
        &mut self.sim
    }

    pub fn world(&self) -> &WorldState {
        self.sim.world()
    }

    pub fn state(&self) -> &SimState {
        self.sim.state()
    }

    pub fn flagship(&self) -> Option<&Ship> {
        let w = self.sim.world();
        w.player.ship.as_ref().and_then(|s| w.ships.get(s))
    }

    pub fn is_paused(&self) -> bool {
        self.sim.is_paused()
    }

    /// Opens the PAUSED card: the sim stops, and a `paused` fact is submitted for the
    /// first tick after it closes. Returns false if it was already open.
    pub fn pause(&mut self) -> bool {
        if self.sim.is_paused() {
            return false;
        }
        self.sim.submit(
            CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Wait,
                "the player opened the PAUSED card",
            ))
            .with(Command::Emit {
                event: Event::Paused {
                    session: self.session,
                },
            }),
        );
        self.sim.set_paused(true);
        true
    }

    /// Closes the PAUSED card.
    pub fn resume(&mut self) {
        self.sim.set_paused(false);
    }

    /// P or Escape.
    pub fn toggle_pause(&mut self) -> bool {
        if self.sim.is_paused() {
            self.resume();
            false
        } else {
            self.pause();
            true
        }
    }

    /// Runs one tick with `input`, the oldest dock trade intent submitted ahead of it.
    /// Does nothing while paused.
    pub fn tick(&mut self, input: PlayerInput) -> TickOutcome {
        if self.sim.is_paused() {
            return TickOutcome::default();
        }
        if let Some(batch) = self.trades.batch_for_tick(self.sim.world()) {
            self.sim.submit(batch);
        }
        let before = poses(self.sim.world());
        if self.sim.step(&input).is_none() {
            return TickOutcome::default();
        }
        self.before = before;
        self.last_events = self.sim.drain_events(&self.sub);
        let flagship = self.sim.world().player.ship.clone();
        let mut out = TickOutcome {
            stepped: true,
            ..TickOutcome::default()
        };
        for e in &self.last_events {
            match &e.event {
                Event::Dock { ship, station } if Some(ship) == flagship.as_ref() => {
                    out.docked = Some(station.clone());
                }
                Event::Undock { ship, .. } if Some(ship) == flagship.as_ref() => {
                    out.undocked = true;
                }
                _ => {}
            }
        }
        if out.undocked {
            self.trades.clear();
        } else {
            let (world, history) = (self.sim.world(), self.sim.history());
            let settled = self.trades.settle(world, history, &self.last_events);
            if settled != Settled::Idle {
                out.trade = Some(settled);
            }
        }
        out
    }

    /// Events committed by the last tick.
    pub fn last_events(&self) -> &[EventEnvelope] {
        &self.last_events
    }

    /// Every ship's pose `alpha` (0..1) of the way from the tick before the last to the
    /// last, for drawing between ticks.
    pub fn poses(&self, alpha: f32) -> BTreeMap<ShipId, Pose> {
        self.sim
            .world()
            .ships
            .values()
            .map(|s| {
                let now = Pose::of(s);
                let pose = match self.before.get(&s.id) {
                    // A jump (lane exit, docking snap) shouldn't smear across the screen.
                    Some(b) if b.position.distance(now.position) < 200.0 => b.lerp(now, alpha),
                    _ => now,
                };
                (s.id.clone(), pose)
            })
            .collect()
    }

    /// Asks the dock screen's desk for a trade.
    pub fn request_trade(&mut self, request: TradeRequest) {
        let world = self.sim.world();
        self.trades.request(world, request);
    }

    /// Submits a batch between ticks (tests and dev tools).
    pub fn submit(&mut self, batch: CommandBatch) {
        self.sim.submit(batch);
    }
}

fn poses(world: &WorldState) -> BTreeMap<ShipId, Pose> {
    world
        .ships
        .values()
        .map(|s| (s.id.clone(), Pose::of(s)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poses_turn_the_short_way() {
        let a = Pose {
            position: Vec2::new(0.0, 0.0),
            heading: 3.0,
        };
        let b = Pose {
            position: Vec2::new(10.0, 0.0),
            heading: -3.0,
        };
        let m = a.lerp(b, 0.5);
        assert!((m.position.x - 5.0).abs() < 1e-5);
        // Halfway the short way round is near pi, not near 0.
        assert!(m.heading.cos() < -0.99, "{}", m.heading);
    }
}
