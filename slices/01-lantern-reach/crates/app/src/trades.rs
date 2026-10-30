//! Dock-screen trades that are never silently lost or falsely acknowledged.
//!
//! A market line's write is a compare-and-set, and the economy never retries a
//! `PlayerInput::trades` request that loses it. So the app keeps each request as an
//! *intent* and plans the oldest one itself every tick (`trade::trade_batch`, the batch
//! the economy would build), submitting it between ticks. Batches submitted between
//! ticks apply first, before any system's, so the player's explicit dock action wins
//! over everything worked out from the start-of-tick world: the flagship's own queued
//! orders, tenders and NPC craft. They lose the line instead and retry next tick, as
//! their order runners already do.
//!
//! An intent is acknowledged only from committed state: a Buy or Sell event for it in
//! the tick's events whose recorded cause is exactly the one submitted. Another trade
//! on the same line (the flagship's own order, say) has a different cause, so it can't
//! settle the intent. If something submitted earlier (a script) moved the line first,
//! the intent goes again next tick, until it applies or `check_trade` says it can't
//! happen any more; then the dock screen shows one calm line saying why.

use std::collections::VecDeque;

use galaxy_contract::prelude::*;
use galaxy_core::History;
use galaxy_economy::trade::trade_batch;
use galaxy_economy::{TradeError, check_trade};

/// How many ticks an intent is retried while the market keeps losing it to others. At
/// 30 ticks a second it gives up after about three seconds; it never happens in play
/// unless a script moves the same line every single tick.
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
    /// The cause of the batch submitted for it this tick, if one was.
    sent: Option<Cause>,
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
            Ok(()) => self.queue.push_back(Intent {
                request,
                tries: 0,
                sent: None,
            }),
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

    /// The batch to submit before this tick: the oldest intent planned against the
    /// world as it is now, one at a time so its result reads unambiguously. `None` when
    /// nothing is waiting or it can't be planned ([`DockTrades::settle`] then says why).
    pub fn batch_for_tick(&mut self, world: &WorldState) -> Option<CommandBatch> {
        let front = self.queue.front_mut()?;
        front.sent = None;
        let batch = trade_batch(world, &front.request).ok()?;
        front.sent = Some(batch.cause.clone());
        Some(batch)
    }

    /// Settles the oldest intent from the tick's committed events, the causes they
    /// were recorded with, and the world after the tick.
    pub fn settle(
        &mut self,
        world: &WorldState,
        history: &History,
        events: &[EventEnvelope],
    ) -> Settled {
        let Some(front) = self.queue.front_mut() else {
            return Settled::Idle;
        };
        let sent = front.sent.take();
        let ours = |e: &EventEnvelope| {
            sent.as_ref().is_some_and(|cause| {
                went_through(&front.request, &e.event)
                    && history.cause(e.cause).is_some_and(|c| c.cause == *cause)
            })
        };
        if events.iter().any(ours) {
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
    if t.is_multiple_of(10) {
        format!("{sign}{} cr", t / 10)
    } else {
        format!("{sign}{}.{} cr", t / 10, t % 10)
    }
}

/// A station-service purchase (an upgrade, a tender, prestige) asked for on the dock
/// screen. It goes out with the next tick's `PlayerInput::services`, after
/// `services::check` said it can happen, and is settled by its own committed event:
/// `upgrade_fitted`, `tender_acquired` or `prestige`. A refusal is one calm line.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServiceDesk {
    pending: Option<ServiceRequest>,
    sent: Option<ServiceRequest>,
    /// The latest refusal. Cleared by the next purchase that goes through.
    pub notice: Option<String>,
}

impl ServiceDesk {
    /// Asks for a purchase; one that can't happen now is refused at once.
    pub fn request(&mut self, world: &WorldState, history: &History, req: ServiceRequest) {
        match galaxy_progress::services::check(world, history, &req) {
            Ok(()) => {
                self.pending = Some(req);
                self.notice = None;
            }
            Err(r) => self.notice = Some(service_refusal(world, &req, &r)),
        }
    }

    /// The purchase for this tick's input, if one is waiting.
    pub fn for_tick(&mut self) -> Option<ServiceRequest> {
        self.sent = self.pending.take();
        self.sent.clone()
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some() || self.sent.is_some()
    }

    /// After the tick: did the purchase sent with it go through?
    pub fn settle(&mut self, world: &WorldState, history: &History, events: &[EventEnvelope]) {
        let Some(req) = self.sent.take() else { return };
        let bought = events.iter().any(|e| match (&req, &e.event) {
            (ServiceRequest::Fit { upgrade }, Event::UpgradeFitted { upgrade: u, .. }) => {
                u == upgrade
            }
            (ServiceRequest::Build { craft }, Event::TenderAcquired { craft: c, .. }) => c == craft,
            (ServiceRequest::Prestige { to }, Event::Prestige { to_hull, .. }) => to_hull == to,
            _ => false,
        });
        self.notice = if bought {
            None
        } else {
            Some(
                match galaxy_progress::services::check(world, history, &req) {
                    Err(r) => service_refusal(world, &req, &r),
                    Ok(()) => "That didn't go through, and nothing was charged.".into(),
                },
            )
        };
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

/// What a purchase is called on screen.
pub fn service_name(world: &WorldState, req: &ServiceRequest) -> String {
    let rules = &world.progress.rules;
    match req {
        ServiceRequest::Fit { upgrade } => rules
            .upgrades
            .get(upgrade)
            .map_or_else(|| upgrade.to_string(), |u| u.name.clone()),
        ServiceRequest::Build { craft } => rules
            .crafts
            .get(craft)
            .map_or_else(|| craft.to_string(), |c| c.name.clone()),
        ServiceRequest::Prestige { to } => world
            .hulls
            .get(to)
            .map_or_else(|| to.to_string(), |h| format!("A {}", h.name)),
    }
}

/// One calm line for why a purchase can't happen.
pub fn service_refusal(
    world: &WorldState,
    req: &ServiceRequest,
    r: &galaxy_progress::services::Refusal,
) -> String {
    use galaxy_progress::services::Refusal as R;
    let name = service_name(world, req);
    match r {
        R::Credits { need, have } => format!("{name} costs {need} cr. You have {have} cr."),
        R::Locked => format!("{name} isn't on offer yet."),
        R::AlreadyFitted => format!("Your ship already has the {}.", name.to_lowercase()),
        R::AlreadyOwned => format!("{name} is already yours."),
        R::NotDocked(station) => format!("Dock at {} for that.", station_name(world, station)),
        R::NoBranch => "The hullwright has no hull for that here.".into(),
        R::NotSold => format!("{name} can't be bought; it's given."),
        R::HoldTooFull { holds, room } => {
            format!("{name} holds {room}. You carry {holds}: sell or deliver some first.")
        }
        R::NoShip | R::Unknown(_) => "That can't be bought here.".into(),
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
