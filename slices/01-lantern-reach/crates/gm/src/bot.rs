//! The sandbox's activity bot: flies the flagship round the authored stations of its
//! region by `PlayerInput`, the way a player would, so a sandbox run is measured in
//! activity (dockings), never in time.
//!
//! The loop: docked, it sells what the station buys and buys a little of what the next
//! station buys; it flies off (undocks); if the hold is empty and a rock of a ware the next station
//! buys is in the region, it mines it until the hold is full; then it flies into the
//! next station's ring and docks. Stations are visited in ID order. Everything it
//! decides comes from the world, so with and without a pack the runs differ only by
//! what the pack changed.

use galaxy_contract::prelude::*;
use galaxy_ship::space;

/// Ticks a mining stop may take before the bot moves on.
const MINE_TICKS: u32 = 900;
/// Ticks the bot waits docked for its trades to settle.
const SETTLE_TICKS: u32 = 4;

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// Just docked: trade, then wait for the trades to settle.
    Docked { waited: u32, traded: bool },
    /// Flying to a rock and mining it.
    Mine { rock: RockId, ticks: u32 },
    /// Flying to the next station.
    Fly,
}

#[derive(Clone, Debug)]
pub struct ActivityBot {
    route: Vec<StationId>,
    next: usize,
    phase: Phase,
    /// Space was down last tick (a press needs it up first).
    action_was: bool,
    mined_this_leg: bool,
    /// Flagship dockings seen.
    pub dockings: u32,
}

impl ActivityBot {
    /// A bot for the world's flagship. `None` if there is no flagship or no station in
    /// its region.
    pub fn new(world: &WorldState) -> Option<Self> {
        let ship = flagship(world)?;
        let route: Vec<StationId> = world
            .stations
            .values()
            .filter(|s| s.region == ship.region)
            .map(|s| s.id.clone())
            .collect();
        if route.is_empty() {
            return None;
        }
        let (next, phase) = match &ship.docked_at {
            Some(at) => (
                route
                    .iter()
                    .position(|s| s == at)
                    .map_or(0, |i| (i + 1) % route.len()),
                Phase::Docked {
                    waited: 0,
                    traded: false,
                },
            ),
            None => (0, Phase::Fly),
        };
        Some(Self {
            route,
            next,
            phase,
            action_was: false,
            mined_this_leg: false,
            dockings: 0,
        })
    }

    /// The station the bot is heading for.
    pub fn target(&self) -> &StationId {
        &self.route[self.next]
    }

    /// Counts the flagship's dockings in a tick's events.
    pub fn observe(&mut self, world: &WorldState, events: &[EventEnvelope]) {
        let me = world.player.ship.as_ref();
        for e in events {
            if let Event::Dock { ship, station } = &e.event
                && Some(ship) == me
            {
                self.dockings += 1;
                if let Some(i) = self.route.iter().position(|s| s == station) {
                    self.next = (i + 1) % self.route.len();
                }
                self.phase = Phase::Docked {
                    waited: 0,
                    traded: false,
                };
                self.mined_this_leg = false;
            }
        }
    }

    fn press(&mut self) -> bool {
        // A press is Space going down: alternate so every other tick is one.
        let down = !self.action_was;
        self.action_was = down;
        down
    }

    fn hold(&mut self, down: bool) -> bool {
        self.action_was = down;
        down
    }

    /// This tick's input.
    pub fn input(&mut self, world: &WorldState) -> PlayerInput {
        let Some(ship) = flagship(world) else {
            return PlayerInput::default();
        };
        let mut input = PlayerInput::default();
        if let Some(at) = ship.docked_at.clone() {
            match &mut self.phase {
                Phase::Docked { waited, traded } => {
                    if !*traded {
                        *traded = true;
                        input.trades = trades(world, ship, &at, self.target());
                        self.action_was = false;
                        return input;
                    }
                    if *waited < SETTLE_TICKS {
                        *waited += 1;
                        self.action_was = false;
                        return input;
                    }
                    // Flies off to undock.
                    input.thrust = Vec2::new(0.0, 1.0);
                    self.action_was = false;
                }
                // Docked without having seen the docking (a fork taken docked): trade first.
                _ => {
                    self.phase = Phase::Docked {
                        waited: 0,
                        traded: false,
                    };
                }
            }
            return input;
        }
        if matches!(self.phase, Phase::Docked { .. }) {
            self.phase = Phase::Fly;
        }
        if self.phase == Phase::Fly
            && !self.mined_this_leg
            && ship.hold.total() == 0
            && let Some(rock) = rock_for(world, ship, self.target())
        {
            self.phase = Phase::Mine { rock, ticks: 0 };
        }
        if let Phase::Mine { rock, ticks } = &mut self.phase {
            let cap = world
                .hulls
                .get(&ship.hull)
                .map_or(0, |h| u64::from(h.hold_capacity));
            let Some(r) = world.rocks.get(rock).filter(|r| r.remaining > 0) else {
                self.mined_this_leg = true;
                self.phase = Phase::Fly;
                return input;
            };
            *ticks += 1;
            if ship.hold.total() >= cap || *ticks > MINE_TICKS {
                self.mined_this_leg = true;
                self.phase = Phase::Fly;
                input.action = self.hold(false);
                return input;
            }
            if space::in_reach(world, r, ship.position) {
                input.action = self.hold(true);
                return input;
            }
            let radius = space::rock_radius(world, r);
            let spot =
                r.position + (ship.position - r.position).normalize_or_zero() * (radius + 15.0);
            input.thrust = stick(ship.position, spot);
            input.action = self.hold(false);
            return input;
        }
        // Fly into the next ring and press Space.
        let Some(st) = world.stations.get(self.target()) else {
            return input;
        };
        let off = st.position - ship.position;
        if off.length() < st.dock_radius - 10.0 {
            input.action = self.press();
        } else {
            input.thrust = stick(ship.position, st.position);
            self.action_was = false;
        }
        input
    }
}

fn flagship(world: &WorldState) -> Option<&Ship> {
    world.player.ship.as_ref().and_then(|s| world.ships.get(s))
}

/// The stick toward `to`, easing off near it.
fn stick(from: Vec2, to: Vec2) -> Vec2 {
    let off = to - from;
    off.normalize_or_zero() * (off.length() / 60.0).min(1.0)
}

/// What the bot trades docked at `at` before heading for `next`: sell everything the
/// station buys, then buy a little of what it sells that `next` buys.
fn trades(world: &WorldState, ship: &Ship, at: &StationId, next: &StationId) -> Vec<TradeRequest> {
    let Some(market) = world.markets.get(at) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (ware, n) in &ship.hold.cargo {
        if *n > 0
            && market
                .lines
                .get(ware)
                .is_some_and(|l| l.role == MarketRole::Buys)
        {
            out.push(TradeRequest {
                ship: ship.id.clone(),
                station: at.clone(),
                ware: ware.clone(),
                side: TradeSide::Sell,
                quantity: *n,
                limit: Some(0),
            });
        }
    }
    let cap = world.hulls.get(&ship.hull).map_or(0, |h| h.hold_capacity);
    let free = cap.saturating_sub(ship.hold.total() as u32);
    let wanted = world
        .markets
        .get(next)
        .map(|m| {
            m.lines
                .iter()
                .filter(|(_, l)| l.role == MarketRole::Buys)
                .map(|(w, _)| w.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Sales settle first, so spend only what's in the wallet now: half of it.
    let budget = world.player.credits / 2;
    for ware in wanted {
        let Some(line) = market
            .lines
            .get(&ware)
            .filter(|l| l.role == MarketRole::Sells)
        else {
            continue;
        };
        if line.stock == 0 || free == 0 {
            continue;
        }
        let Ok(q) = galaxy_economy::quote(world, at, &ware, TradeSide::Buy, free.min(line.stock))
        else {
            continue;
        };
        let mut spent = 0i64;
        let mut units = 0u32;
        for p in &q.unit_prices {
            if spent + i64::from(*p) > budget {
                break;
            }
            spent += i64::from(*p);
            units += 1;
        }
        if units > 0 {
            out.push(TradeRequest {
                ship: ship.id.clone(),
                station: at.clone(),
                ware,
                side: TradeSide::Buy,
                quantity: units,
                limit: None,
            });
        }
        break;
    }
    out
}

/// A rock in the ship's region whose ware `next` buys, nearest first.
fn rock_for(world: &WorldState, ship: &Ship, next: &StationId) -> Option<RockId> {
    let market = world.markets.get(next)?;
    world
        .rocks
        .values()
        .filter(|r| r.remaining > 0)
        .filter(|r| space::rock_region(world, r) == Some(&ship.region))
        .filter(|r| {
            world.rock_types.get(&r.rock_type).is_some_and(|t| {
                market
                    .lines
                    .get(&t.ware)
                    .is_some_and(|l| l.role == MarketRole::Buys)
            })
        })
        .min_by(|a, b| {
            let da = a.position.distance(ship.position);
            let db = b.position.distance(ship.position);
            da.total_cmp(&db).then_with(|| a.id.cmp(&b.id))
        })
        .map(|r| r.id.clone())
}
