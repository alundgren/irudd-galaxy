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
//! - **Station services** go through [`ServiceDesk`]: a purchase rides the next tick's
//!   `PlayerInput::services` and is settled by its own committed event.
//! - **"What changed?"** at the dockmaster: [`Session::ask_what_changed`] records
//!   `asked_what_changed`, has the dockmaster say the answer's lines and keeps the
//!   answer up until the ship leaves.
//! - **Poses** from before and after the last tick, so drawing can interpolate between
//!   fixed ticks.
//!
//! It runs the whole game ([`Game`]): native systems, the script host with the packs,
//! progress, and the mind pumped after every tick.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_core::bus::Subscriber;
use galaxy_core::{Sim, SimState};

use crate::game::{Game, GameOptions};
use crate::hooks::{self, Answer};
use crate::trades::{DockTrades, ServiceDesk, Settled};

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
    game: Game,
    sub: Subscriber,
    /// The `sessions` counter for this play session (0 until [`Session::begin`]).
    session: u64,
    pub trades: DockTrades,
    pub services: ServiceDesk,
    /// The dockmaster's answer to "What changed?", while docked where it was asked.
    pub answer: Option<Answer>,
    /// Poses before the last tick, for interpolation.
    before: BTreeMap<ShipId, Pose>,
    /// Events committed by the last tick.
    last_events: Vec<EventEnvelope>,
}

impl Session {
    /// The whole game over `state`, headless: the packs folder found from the working
    /// directory, gm-examples as the save says, the seeded fake mind. For `galaxy shot`
    /// and tests; panics if the core pack can't load.
    pub fn new(state: SimState) -> Self {
        let packs = crate::game::packs_dir(None).expect("the packs folder (packs/core)");
        let seed = state.world.meta.seed;
        let game = Game::open(state, &GameOptions::headless(packs, seed))
            .unwrap_or_else(|e| panic!("{e}"));
        Self::with_game(game)
    }

    /// A session over a composed game.
    pub fn with_game(mut game: Game) -> Self {
        let sub = game.sim_mut().subscribe("app");
        let before = poses(game.sim().world());
        Self {
            game,
            sub,
            session: 0,
            trades: DockTrades::default(),
            services: ServiceDesk::default(),
            answer: None,
            before,
            last_events: Vec::new(),
        }
    }

    pub fn game(&self) -> &Game {
        &self.game
    }

    pub fn game_mut(&mut self) -> &mut Game {
        &mut self.game
    }

    fn sim_mut(&mut self) -> &mut Sim {
        self.game.sim_mut()
    }

    /// Starts a play session: submits `session_start` (it commits on the first tick).
    pub fn begin(&mut self) {
        let sessions = self
            .world()
            .counters
            .get(CounterKind::Sessions, &CounterScope::Player);
        self.session = sessions + 1;
        self.sim_mut().submit(
            CommandBatch::new(Cause::system(APP, "the game was opened")).with(Command::Emit {
                event: Event::SessionStart {},
            }),
        );
    }

    pub fn session(&self) -> u64 {
        self.session
    }

    pub fn sim(&self) -> &Sim {
        self.game.sim()
    }

    pub fn world(&self) -> &WorldState {
        self.sim().world()
    }

    pub fn state(&self) -> &SimState {
        self.sim().state()
    }

    pub fn flagship(&self) -> Option<&Ship> {
        let w = self.world();
        w.player.ship.as_ref().and_then(|s| w.ships.get(s))
    }

    pub fn is_paused(&self) -> bool {
        self.sim().is_paused()
    }

    /// Opens the PAUSED card: the sim stops, and a `paused` fact is submitted for the
    /// first tick after it closes. Returns false if it was already open.
    pub fn pause(&mut self) -> bool {
        if self.is_paused() {
            return false;
        }
        let session = self.session;
        self.sim_mut().submit(
            CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Wait,
                "the player opened the PAUSED card",
            ))
            .with(Command::Emit {
                event: Event::Paused { session },
            }),
        );
        self.sim_mut().set_paused(true);
        true
    }

    /// Closes the PAUSED card.
    pub fn resume(&mut self) {
        self.sim_mut().set_paused(false);
    }

    /// P or Escape.
    pub fn toggle_pause(&mut self) -> bool {
        if self.is_paused() {
            self.resume();
            false
        } else {
            self.pause();
            true
        }
    }

    /// Runs one tick with `input`: the oldest dock trade intent is submitted ahead of
    /// it, and a waiting service purchase rides along in `input.services`. Then the
    /// mind is pumped. Does nothing while paused.
    pub fn tick(&mut self, mut input: PlayerInput) -> TickOutcome {
        if self.is_paused() {
            return TickOutcome::default();
        }
        if let Some(batch) = self.trades.batch_for_tick(self.game.sim().world()) {
            self.sim_mut().submit(batch);
        }
        if let Some(req) = self.services.for_tick() {
            input.services.push(req);
        }
        let before = poses(self.world());
        if self.game.step(&input).is_none() {
            return TickOutcome::default();
        }
        self.before = before;
        let sub = self.sub.clone();
        self.last_events = self.sim_mut().drain_events(&sub);
        let flagship = self.world().player.ship.clone();
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
            self.services.clear();
            self.answer = None;
        } else {
            let (world, history) = (self.game.sim().world(), self.game.sim().history());
            let settled = self.trades.settle(world, history, &self.last_events);
            if settled != Settled::Idle {
                out.trade = Some(settled);
            }
            self.services.settle(world, history, &self.last_events);
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
        self.world()
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
        let world = self.game.sim().world();
        self.trades.request(world, request);
    }

    /// Asks for a station service (an upgrade, a tender, prestige) on the next tick.
    pub fn request_service(&mut self, request: ServiceRequest) {
        let sim = self.game.sim();
        self.services.request(sim.world(), sim.history(), request);
    }

    /// Asks the dockmaster where the flagship is docked what changed. Records
    /// `asked_what_changed`; the dockmaster says the answer's lines (so ambient talk
    /// won't repeat them), and the answer stays up until the ship leaves.
    pub fn ask_what_changed(&mut self) -> Option<&Answer> {
        let station = self.flagship()?.docked_at.clone()?;
        let sim = self.game.sim();
        let answer = hooks::what_changed(sim.world(), sim.history(), self.game.lines(), &station);
        let mut batch = CommandBatch::new(Cause::verb(
            Actor::Player,
            VerbKind::Wait,
            "the player asked the dockmaster what changed",
        ))
        .with(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: hooks::ASKED,
                subjects: Subjects {
                    npc: answer.speaker.clone(),
                    station: Some(station.clone()),
                    ship: self.world().player.ship.clone(),
                    ..Subjects::default()
                },
                amount: Some(answer.lines.len() as i64),
                note: format!("asked what changed at {station}"),
            }),
        });
        for line in answer.lines.iter().filter(|l| l.channel != galaxy_contract::talk::Channel::Chronicle) {
            batch.push(Command::Say {
                line: Box::new(line.clone()),
            });
        }
        self.sim_mut().submit(batch);
        self.answer = Some(answer);
        self.answer.as_ref()
    }

    /// Submits a batch between ticks (tests and dev tools).
    pub fn submit(&mut self, batch: CommandBatch) {
        self.sim_mut().submit(batch);
    }

    /// Ends the session before the final save ([`Game::finish`]).
    pub fn finish(&mut self) {
        self.game.finish();
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
