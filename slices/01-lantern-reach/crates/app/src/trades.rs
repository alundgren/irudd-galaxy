//! Dock-screen trades that are never silently lost.
//!
//! The economy runs the player's `PlayerInput::trades` against the world at the start
//! of the tick, and its market-line write is a compare-and-set. If another ship (an
//! order-driven tender or NPC craft) or a script trades or moves the same line in the
//! same tick, the player's batch is rejected and the economy doesn't retry it. So the
//! app keeps each request as an *intent*: it sends the oldest one every tick until the
//! tick's events show it went through, or `check_trade` says it can't happen any more,
//! in which case the dock screen shows one calm line saying why.

use std::collections::VecDeque;

use galaxy_contract::prelude::*;
use galaxy_economy::{TradeError, check_trade};

/// How many ticks an intent is retried while the market keeps losing it to others. At
/// 30 ticks a second it gives up after about three seconds; it never happens in play
/// unless something trades the same line every single tick.
pub const MAX_TRIES: u32 = 90;

/// What became of the oldest intent after a tick.
#[derive(Clone, Debug, PartialEq)]
pub enum Settled {
    /// Nothing was waiting.
    Idle,
    /// The trade went through.
    Applied,
    /// It lost the line to another writer this tick; it goes again next tick.
    Retrying,
    /// It can't happen now. The reason is in [`DockTrades::notice`].
    Refused,
}

#[derive(Clone, Debug, PartialEq)]
struct Intent {
    request: TradeRequest,
    tries: u32,
}

/// The dock screen's trade intents, oldest first.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DockTrades {
    queue: VecDeque<Intent>,
    /// The latest refusal, in one calm line. Cleared by the next successful trade.
    pub notice: Option<String>,
}

impl DockTrades {
    /// Asks for a trade. A request that can't happen right now is refused at once.
    pub fn request(&mut self, world: &WorldState, request: TradeRequest) {
        match check_trade(world, &request) {
            Ok(()) => self.queue.push_back(Intent { request, tries: 0 }),
            Err(e) => self.notice = Some(refusal(world, &e)),
        }
    }

    pub fn pending(&self) -> usize {
        self.queue.len()
    }

    /// Drops every intent (the ship left the station).
    pub fn clear(&mut self) {
        self.queue.clear();
        self.notice = None;
    }

    /// The trades for this tick's input: the oldest intent, one at a time, so its
    /// result can be read unambiguously from the tick's events.
    pub fn for_tick(&self) -> Vec<TradeRequest> {
        self.queue
            .front()
            .map(|i| i.request.clone())
            .into_iter()
            .collect()
    }

    /// Settles the oldest intent from the tick's committed events and the world after
    /// the tick.
    pub fn settle(&mut self, world: &WorldState, events: &[EventEnvelope]) -> Settled {
        let Some(front) = self.queue.front_mut() else {
            return Settled::Idle;
        };
        if events
            .iter()
            .any(|e| went_through(&front.request, &e.event))
        {
            self.queue.pop_front();
            self.notice = None;
            return Settled::Applied;
        }
        front.tries += 1;
        match check_trade(world, &front.request) {
            Ok(()) if front.tries < MAX_TRIES => Settled::Retrying,
            Ok(()) => {
                self.queue.pop_front();
                self.notice = Some("The market is busy just now. Try again in a moment.".into());
                Settled::Refused
            }
            Err(e) => {
                let line = refusal(world, &e);
                self.queue.pop_front();
                self.notice = Some(line);
                Settled::Refused
            }
        }
    }
}

/// Whether `event` is `request` going through.
fn went_through(request: &TradeRequest, event: &Event) -> bool {
    let (ship, station, ware, quantity) = match (request.side, event) {
        (
            TradeSide::Sell,
            Event::Sell {
                ship,
                station,
                ware,
                quantity,
                ..
            },
        )
        | (
            TradeSide::Buy,
            Event::Buy {
                ship,
                station,
                ware,
                quantity,
                ..
            },
        ) => (ship, station, ware, *quantity),
        _ => return false,
    };
    *ship == request.ship
        && *station == request.station
        && *ware == request.ware
        // With a limit the trade may fill fewer units than asked.
        && (request.limit.is_some() || quantity == request.quantity)
}

fn ware_name(world: &WorldState, ware: &WareId) -> String {
    world
        .wares
        .get(ware)
        .map_or_else(|| ware.to_string(), |w| w.name.to_lowercase())
}

fn station_name(world: &WorldState, station: &StationId) -> String {
    world
        .stations
        .get(station)
        .map_or_else(|| station.to_string(), |s| s.name.clone())
}

/// Credits for the screen: whole, or with one decimal (content decision 16).
pub fn credits(cents: i64) -> String {
    let tenths = (cents as f64 / 10.0).round() as i64;
    let sign = if tenths < 0 { "-" } else { "" };
    let t = tenths.unsigned_abs();
    if t % 10 == 0 {
        format!("{sign}{} cr", t / 10)
    } else {
        format!("{sign}{}.{} cr", t / 10, t % 10)
    }
}

/// One calm line for why a trade can't happen.
pub fn refusal(world: &WorldState, e: &TradeError) -> String {
    match e {
        TradeError::NotEnoughCredits { have, need } => format!(
            "That costs {}; you have {}.",
            credits(*need),
            credits(*have)
        ),
        TradeError::HoldFull { free: 0, .. } => "Your hold is full.".into(),
        TradeError::HoldFull { free, .. } => format!("Your hold has room for {free} more."),
        TradeError::NotEnoughCargo { ware, have: 0, .. } => {
            format!("You have no {} to sell.", ware_name(world, ware))
        }
        TradeError::NotEnoughCargo { ware, have, .. } => {
            format!("You only have {have} {}.", ware_name(world, ware))
        }
        TradeError::NotEnoughStock { ware, have: 0, .. } => {
            format!("There's no {} left to buy here.", ware_name(world, ware))
        }
        TradeError::NotEnoughStock { ware, have, .. } => {
            format!("Only {have} {} left to buy here.", ware_name(world, ware))
        }
        TradeError::NotDocked { .. } => "You've left the ring.".into(),
        TradeError::WrongSide { station, ware, .. } | TradeError::NotListed { station, ware } => {
            format!(
                "{} doesn't trade {} that way.",
                station_name(world, station),
                ware_name(world, ware)
            )
        }
        TradeError::NotTradeable { ware } => {
            format!("Nobody trades {}.", ware_name(world, ware))
        }
        TradeError::BeyondLimit { .. } => "The price moved; nothing changed hands.".into(),
        TradeError::ZeroQuantity
        | TradeError::UnknownShip { .. }
        | TradeError::UnknownStation { .. }
        | TradeError::UnknownWare { .. } => "That trade can't happen here.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credits_show_at_most_one_decimal() {
        assert_eq!(credits(4000), "40 cr");
        assert_eq!(credits(1430), "14.3 cr");
        assert_eq!(credits(1436), "14.4 cr");
        assert_eq!(credits(-50), "-0.5 cr");
        assert_eq!(credits(0), "0 cr");
    }
}
