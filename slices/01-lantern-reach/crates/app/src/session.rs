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
//! - **Talk at the dock** ([`TalkBook`]): after every tick the session sorts what was
//!   said. Dock talk waits as bubbles at its dock ([`Session::bubble`]) until the player
//!   clicks past it ([`Session::next_line`]) or answers it; the rest goes to the ship
//!   log. Like the hint counts, the book belongs to the save file, not the world.
//! - **Answers at the dock** go through [`Session::reply`]: a choice a line offered
//!   rides the next tick's `PlayerInput::replies`, and the scripts answer it.
//! - **"What changed?"** at the dockmaster ([`Session::ask_what_changed`]) is the same
//!   reply (`choice.what_changed`): the core pack records `core.asked_what_changed`
//!   and has the dockmaster say the answer's lines; the session adds digest lines for
//!   what's left and says them, so they come up as bubbles. The answer's notices (and
//!   "nothing new") follow them until the ship leaves.
//! - **Poses** from before and after the last tick, so drawing can interpolate between
//!   fixed ticks.
//! - **Hints** ([`HintBook`]): after every tick the session counts the hint actions the
//!   flagship did, so a hint retires once it has taught. The counts belong to the
//!   save file, not the world (the window writes them with each save).
//!
//! It runs the whole game ([`Game`]): native systems, the script host with the packs,
//! progress, and the mind pumped after every tick.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::bus::Subscriber;
use galaxy_core::{Sim, SimState};

use crate::game::{Game, GameOptions};
use crate::hooks::{self, Answer};
use crate::trades::{DockTrades, ServiceDesk, Settled};
use crate::ui::hints::{Acts, HintBook, HintRules};
use crate::ui::talk::{Bubble, TalkAnchors, TalkBook};

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

/// "What changed?", sent and waiting for the core pack's answer.
#[derive(Clone, Debug)]
struct Asking {
    station: StationId,
    npc: NpcId,
    /// The newest fact when it was asked: the answer's fact comes after it.
    after: FactId,
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
    /// How far each context hint has taught, in this save.
    pub hints: HintBook,
    /// Which lines wait to be seen at which dock, and what's new in the log.
    pub talk: TalkBook,
    /// Where each speaker's bubbles rise from.
    pub anchors: TalkAnchors,
    /// The dockmaster's answer to "What changed?", while docked where it was asked.
    pub answer: Option<Answer>,
    /// How many of the answer's own bubbles ([`Session::answer_extras`]) were seen.
    answer_seen: usize,
    /// Lines waiting for an answer that the player set aside for now, in the order they
    /// were; they show after the rest, until the ship leaves.
    later: Vec<TalkId>,
    /// The question, asked and not answered yet.
    asking: Option<Asking>,
    /// Answers for the next tick's `PlayerInput::replies`.
    replies: Vec<Reply>,
    /// Poses before the last tick, for interpolation.
    before: BTreeMap<ShipId, Pose>,
    /// Events committed by the last tick.
    last_events: Vec<EventEnvelope>,
}

impl Session {
    /// The whole game over `state`, headless: the packs folder found from the working
    /// directory, gm-examples as the save says, the seeded fake mind, the core pack's
    /// hint rules with nothing taught yet. For tests; panics if the core pack can't
    /// load.
    pub fn new(state: SimState) -> Self {
        let packs = crate::game::packs_dir(None).expect("the packs folder (packs/core)");
        let rules =
            HintRules::load(&crate::game::core_data(&packs)).unwrap_or_else(|e| panic!("{e}"));
        let seed = state.world.meta.seed;
        let game = Game::open(state, &GameOptions::headless(packs, seed))
            .unwrap_or_else(|e| panic!("{e}"));
        let mut s = Self::with_game(game);
        s.hints = HintBook::new(rules, Default::default());
        s.anchors = TalkAnchors::load(&crate::game::core_data(&packs_root()))
            .unwrap_or_else(|e| panic!("{e}"));
        s
    }

    /// A session over a composed game.
    pub fn with_game(mut game: Game) -> Self {
        let sub = game.sim_mut().subscribe("app");
        let before = poses(game.sim().world());
        let talk = TalkBook::fresh(game.sim().world(), game.sim().history());
        Self {
            game,
            sub,
            session: 0,
            trades: DockTrades::default(),
            services: ServiceDesk::default(),
            hints: HintBook::default(),
            talk,
            anchors: TalkAnchors::default(),
            answer: None,
            answer_seen: 0,
            later: Vec::new(),
            asking: None,
            replies: Vec::new(),
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

    /// The sim, for code that works between ticks (the game-master bridge).
    pub fn sim_mut(&mut self) -> &mut Sim {
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
        input.replies.append(&mut self.replies);
        let before = poses(self.world());
        let acts = Acts::of(self.world());
        if self.game.step(&input).is_none() {
            return TickOutcome::default();
        }
        self.before = before;
        self.hints.observe(acts, self.game.sim().world());
        self.talk.observe(self.game.sim().world());
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
            self.answer_seen = 0;
            self.later.clear();
            self.asking = None;
            self.replies.clear();
        } else {
            self.settle_answer();
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

    /// Answers `npc` at the dock with `choice` (a line's choice, or a standing question
    /// such as `choice.what_changed`) on the next tick. "What changed?" also opens the
    /// answer panel once the pack answers ([`Session::ask_what_changed`]). False when
    /// not docked.
    pub fn reply(&mut self, npc: NpcId, choice: AnyId) -> bool {
        let Some(station) = self.flagship().and_then(|f| f.docked_at.clone()) else {
            return false;
        };
        if choice.as_str() == hooks::WHAT_CHANGED {
            self.asking = Some(Asking {
                station,
                npc: npc.clone(),
                after: self.sim().history().last_fact_id().unwrap_or(FactId(0)),
            });
            self.answer = None;
            self.answer_seen = 0;
        } else {
            // The line that asked is answered: its bubble is done.
            let world = self.game.sim().world();
            self.talk.answered(world, &npc, &choice);
        }
        self.replies.retain(|r| r.npc != npc || r.choice != choice);
        self.replies.push(Reply { npc, choice });
        true
    }

    /// Picks open choice `index` (0-based, in [`hooks::open_choices`] order) where the
    /// flagship is docked, as its button or number key does.
    pub fn reply_to_open(&mut self, index: usize) -> bool {
        let Some(station) = self.flagship().and_then(|f| f.docked_at.clone()) else {
            return false;
        };
        let open = hooks::open_choices(self.world(), self.sim().history(), &station);
        match open.into_iter().nth(index) {
            Some(c) => self.reply(c.npc, c.choice),
            None => false,
        }
    }

    /// Asks the dockmaster where the flagship is docked what changed: a
    /// `choice.what_changed` reply on the next tick. The answer ([`Session::answer`])
    /// comes up once the core pack has answered, and stays until the ship leaves.
    /// False when there's no dockmaster here.
    pub fn ask_what_changed(&mut self) -> bool {
        let Some(station) = self.flagship().and_then(|f| f.docked_at.clone()) else {
            return false;
        };
        let Some(npc) = hooks::keeper(self.world(), &station, "dockmaster") else {
            return false;
        };
        let choice = AnyId::new(hooks::WHAT_CHANGED).expect("a valid choice ID");
        self.reply(npc, choice)
    }

    /// Whether "What changed?" is asked and waiting for its answer.
    pub fn is_asking(&self) -> bool {
        self.asking.is_some()
    }

    /// Once the core pack has answered the question (its `core.asked_what_changed`
    /// fact), builds the answer and says its digest lines. Until then, or until the
    /// ship leaves, the question waits: no answer beats one the pack didn't give.
    fn settle_answer(&mut self) {
        let Some(asking) = self.asking.clone() else {
            return;
        };
        let sim = self.game.sim();
        let history = sim.history();
        let Some(asked) = history
            .facts()
            .iter()
            .rev()
            .take_while(|f| f.id > asking.after)
            .find(|f| f.kind == hooks::ASKED && f.subjects.npc.as_ref() == Some(&asking.npc))
            .cloned()
        else {
            return;
        };
        let answer = hooks::what_changed(
            sim.world(),
            history,
            &sim.state().kernel.pending,
            &asking.station,
            &asked,
        );
        if !answer.digest.is_empty() {
            let mut batch = CommandBatch::new(Cause::verb(
                Actor::Player,
                VerbKind::Wait,
                "the dockmaster tells what changed",
            ));
            for line in &answer.digest {
                batch.push(Command::Say {
                    line: Box::new(line.clone()),
                });
            }
            self.sim_mut().submit(batch);
        }
        self.asking = None;
        self.answer = Some(answer);
    }

    /// The answer's own bubbles, after the lines it said: its notices, or the
    /// dockmaster's "nothing new" when it found nothing.
    fn answer_extras(&self) -> Vec<TalkLine> {
        let Some(a) = &self.answer else {
            return Vec::new();
        };
        if a.lines.is_empty() {
            return vec![TalkLine {
                channel: Channel::DockTalk,
                speaker: a.speaker.clone(),
                place: Some(a.station.clone()),
                text: "Nothing new since you were last here.".to_owned(),
                cites: Vec::new(),
                about: Vec::new(),
                choices: Vec::new(),
            }];
        }
        a.lines
            .iter()
            .filter(|l| l.channel != Channel::DockTalk)
            .cloned()
            .collect()
    }

    /// The talk bubble showing at the dock now: the oldest line waiting here, then the
    /// "What changed?" answer's notices, then any line set aside while it waited for an
    /// answer ([`Session::next_line`]). While someone waits for an answer whose line
    /// was already seen, that line again, so the answer can always be clicked. `None`
    /// in flight or when nothing is waiting.
    pub fn bubble(&self) -> Option<Bubble> {
        let station = self.flagship()?.docked_at.clone()?;
        let (world, history) = (self.world(), self.sim().history());
        let open = hooks::open_choices(world, history, &station);
        // Everything still to show here, in order: (talk ID, line).
        let queued = self
            .talk
            .queue(&station)
            .filter_map(|id| Some((id, world.talk.lines.get(&id)?)));
        let (later, now): (Vec<_>, Vec<_>) = queued.partition(|(id, _)| self.later.contains(id));
        let lines: Vec<(Option<TalkId>, TalkLine)> = now
            .into_iter()
            .map(|(id, l)| (Some(id), l.line.clone()))
            .chain(
                self.answer_extras()
                    .into_iter()
                    .skip(self.answer_seen)
                    .map(|l| (None, l)),
            )
            .chain(later.into_iter().map(|(id, l)| (Some(id), l.line.clone())))
            .collect();
        if let Some((id, line)) = lines.first() {
            return Some(Bubble::new(*id, line.clone(), &open, lines.len() - 1));
        }
        let first = open.first()?;
        let since = hooks::last_docking(world, history);
        let asking = world
            .talk
            .on(Channel::DockTalk)
            .filter(|l| l.tick >= since && l.line.speaker.as_ref() == Some(&first.npc))
            .filter(|l| l.line.choices.iter().any(|c| c.id == first.choice))
            .last()?;
        Some(Bubble::new(Some(asking.id), asking.line.clone(), &open, 0))
    }

    /// Moves past the bubble showing now (a click on it, or Enter). One that waits for
    /// an answer is set aside behind the other lines here, and comes back after them;
    /// alone, it stays. False when nothing moved.
    pub fn next_line(&mut self) -> bool {
        let Some(b) = self.bubble() else {
            return false;
        };
        match (b.talk, b.advances()) {
            (Some(id), true) => self.talk.seen(id),
            (None, true) => self.answer_seen += 1,
            (Some(id), false) if b.more > 0 => {
                // Behind everything else: a second time round it goes last again.
                self.later.retain(|l| *l != id);
                self.later.push(id);
            }
            _ => return false,
        }
        true
    }

    /// Skips what's waiting at this dock (it stays in the log). Lines that wait for an
    /// answer stay up.
    pub fn skip_talk(&mut self) {
        let Some(station) = self.flagship().and_then(|f| f.docked_at.clone()) else {
            return;
        };
        let world = self.game.sim().world();
        let open = hooks::open_choices(world, self.game.sim().history(), &station);
        self.talk.skip(world, &station, &open);
        self.answer_seen = self.answer_extras().len();
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

/// The packs folder, for the headless constructor.
fn packs_root() -> std::path::PathBuf {
    crate::game::packs_dir(None).expect("the packs folder (packs/core)")
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
