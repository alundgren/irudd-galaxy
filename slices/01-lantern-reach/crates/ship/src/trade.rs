//! Buy and sell orders go through a [`TradeDesk`].
//!
//! Prices and saturation belong to the economy crate. The ship crate only knows that a
//! docked ship asks to trade and gets back the commands that do it (cargo, credits,
//! the market line, the `buy`/`sell` event) or a typed reason it can't. Implement the
//! trait (or pass a closure with the same signature) to plug a real market in.
//!
//! The order runner puts a trade's commands in the same batch as the order's progress
//! and facts, so a trade the kernel rejects (a compare-and-set market update that lost
//! to another writer in the same tick) leaves the order where it was, to retry next
//! tick. Within a tick the ship system lets one ship trade on a market line, so ships
//! don't quote the same line twice; the player's flagship goes first and the others
//! take turns, so nobody waits for ever.
//!
//! The economy's `plan_trade` fits behind a thin adapter: map the side and the errors
//! ([`TradeError`] notes which economy error each variant stands for), return its
//! `filled` count and `total`, and meet [`TradeRequest::quantity`]'s "up to" rule by
//! clamping a limitless buy to the line's stock and to what the credits afford.

use galaxy_contract::prelude::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

/// One trade a docked ship asks for.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeRequest {
    pub ship: ShipId,
    /// Where the ship is docked.
    pub station: StationId,
    pub ware: WareId,
    pub side: Side,
    /// Units wanted, at most. The runner never asks for more than the ship can take:
    /// a buy is clipped to the free hold, a sell to the units held. The desk moves as
    /// many as it can up to this (stock, credits, `limit`) and says how many in
    /// [`Trade::units`]; it is never all or nothing.
    pub quantity: u32,
    /// Selling: only while the unit price is at or above this (the floor).
    /// Buying: only while the unit price is at or below this.
    pub limit: Option<u32>,
}

/// What a trade does: the commands, applied atomically with the order's progress.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trade {
    /// Units actually moved (the economy's `filled`).
    pub units: u32,
    /// Credits that changed hands, positive either way (the economy's `total`).
    pub total: i64,
    /// Cargo, credits, market and event commands. Include `Emit` of `Event::Buy` or
    /// `Event::Sell` so history and counters see the trade.
    pub commands: Vec<Command>,
}

/// Why a trade can't happen. Only [`TradeError::PastLimit`] can change by waiting
/// (the price moves with activity); a patient sell waits on it and nothing else.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TradeError {
    /// The station has no market (economy: `UnknownStation`).
    #[error("{0} has no market")]
    NoMarket(StationId),
    /// The station doesn't trade the ware this way (economy: `NotListed`,
    /// `WrongSide`, `NotTradeable`, `UnknownWare`).
    #[error("{station} doesn't trade {ware} that way")]
    NotTraded { station: StationId, ware: WareId },
    /// The ship isn't docked there (economy: `NotDocked`).
    #[error("not docked at {0}")]
    NotDocked(StationId),
    /// The next unit is past the floor or the most the buyer will pay (economy:
    /// `BeyondLimit`, its price rounded to whole credits).
    #[error("the price {price} is past the limit {limit}")]
    PastLimit { price: u32, limit: u32 },
    /// Economy: `NotEnoughCargo`, or `ZeroQuantity` on a sell.
    #[error("nothing to sell")]
    NothingToSell,
    /// Economy: `NotEnoughStock`.
    #[error("no stock")]
    NoStock,
    /// Not even one unit is affordable (economy: `NotEnoughCredits`).
    #[error("not enough credits")]
    NoCredits,
    /// Economy: `HoldFull`, or `ZeroQuantity` on a buy.
    #[error("the hold is full")]
    HoldFull,
    #[error("{0}")]
    Refused(String),
}

impl TradeError {
    /// Whether the price, not the market, stands in the way: the one failure a
    /// patient sell waits out. Every other failure ends the order (`skipped`).
    pub fn is_price_limit(&self) -> bool {
        matches!(self, TradeError::PastLimit { .. })
    }
}

/// Turns a trade request into commands, or says why not. Reads the world as it was at
/// the start of the tick.
pub trait TradeDesk {
    fn trade(&self, world: &WorldState, request: &TradeRequest) -> Result<Trade, TradeError>;

    /// The most `station` could pay for one unit of `ware` right now (its ceiling); a
    /// floor above it can never sell. `None`: it doesn't buy the ware. The default is
    /// the line's `max_price`; the economy's desk gives its own ceiling.
    fn ceiling(&self, world: &WorldState, station: &StationId, ware: &WareId) -> Option<u32> {
        let line = world.markets.get(station)?.lines.get(ware)?;
        Some(line.max_price.max(line.price))
    }
}

impl<F> TradeDesk for F
where
    F: Fn(&WorldState, &TradeRequest) -> Result<Trade, TradeError>,
{
    fn trade(&self, world: &WorldState, request: &TradeRequest) -> Result<Trade, TradeError> {
        self(world, request)
    }
}

/// Refuses every trade. For a sim without a market.
pub struct ClosedDesk;

impl TradeDesk for ClosedDesk {
    fn trade(&self, _: &WorldState, request: &TradeRequest) -> Result<Trade, TradeError> {
        Err(TradeError::NoMarket(request.station.clone()))
    }

    fn ceiling(&self, _: &WorldState, _: &StationId, _: &WareId) -> Option<u32> {
        None
    }
}

/// A stand-in market: trades at the line's current price, which never moves; stock
/// goes up and down. Credits move only for player-owned ships (NPC craft have no
/// wallet here). The economy crate's desk replaces it.
///
/// Its stock change is a compare-and-set on the line it quoted (`UpdateMarketLine`), so
/// a quote made stale by an earlier batch in the same tick (another trade, a script)
/// is rejected rather than selling stock that is gone.
pub struct FlatDesk;

impl TradeDesk for FlatDesk {
    fn trade(&self, world: &WorldState, r: &TradeRequest) -> Result<Trade, TradeError> {
        let market = world
            .markets
            .get(&r.station)
            .ok_or_else(|| TradeError::NoMarket(r.station.clone()))?;
        let line = market
            .lines
            .get(&r.ware)
            .ok_or_else(|| TradeError::NotTraded {
                station: r.station.clone(),
                ware: r.ware.clone(),
            })?;
        let ship = world
            .ships
            .get(&r.ship)
            .ok_or_else(|| TradeError::Refused(format!("unknown ship {}", r.ship)))?;
        if ship.docked_at.as_ref() != Some(&r.station) {
            return Err(TradeError::NotDocked(r.station.clone()));
        }
        let players = ship.owner == Owner::Player;
        let price = line.price;
        let past = match (r.side, r.limit) {
            (Side::Sell, Some(floor)) => price < floor,
            (Side::Buy, Some(max)) => price > max,
            (_, None) => false,
        };
        if let (true, Some(limit)) = (past, r.limit) {
            return Err(TradeError::PastLimit { price, limit });
        }

        let units = match r.side {
            Side::Sell => {
                let n = r.quantity.min(ship.hold.count(&r.ware));
                if n == 0 {
                    return Err(TradeError::NothingToSell);
                }
                n
            }
            Side::Buy => {
                let cap = world
                    .hulls
                    .get(&ship.hull)
                    .map_or(0, |h| u64::from(h.hold_capacity));
                let space = cap.saturating_sub(ship.hold.total());
                let afford = if players && price > 0 {
                    u64::try_from(world.player.credits).unwrap_or(0) / u64::from(price)
                } else {
                    u64::MAX
                };
                let n = u64::from(r.quantity.min(line.stock)).min(space).min(afford);
                if line.stock == 0 {
                    return Err(TradeError::NoStock);
                }
                if space == 0 || r.quantity == 0 {
                    return Err(TradeError::HoldFull);
                }
                if n == 0 {
                    return Err(TradeError::NoCredits);
                }
                n as u32
            }
        };

        let (cargo, stock, credits) = match r.side {
            Side::Sell => (-1, 1, 1),
            Side::Buy => (1, -1, -1),
        };
        let total = i64::from(units) * i64::from(price);
        let mut new_line = line.clone();
        new_line.stock = u32::try_from(i64::from(line.stock) + stock * i64::from(units))
            .map_err(|_| TradeError::Refused("the station can't hold that much".into()))?;
        let mut commands = vec![
            Command::AdjustCargo {
                ship: r.ship.clone(),
                ware: r.ware.clone(),
                delta: cargo * i64::from(units),
            },
            // Only if the line is still the one quoted: a trade or a script that
            // changed it earlier this tick rejects this batch, and the order retries.
            Command::UpdateMarketLine {
                station: r.station.clone(),
                ware: r.ware.clone(),
                from: Box::new(line.clone()),
                to: Box::new(new_line),
            },
        ];
        if players {
            commands.push(Command::AdjustCredits {
                delta: credits * total,
            });
        }
        let (ship_id, station, ware) = (r.ship.clone(), r.station.clone(), r.ware.clone());
        commands.push(Command::Emit {
            event: match r.side {
                Side::Sell => Event::Sell {
                    ship: ship_id,
                    station,
                    ware,
                    quantity: units,
                    unit_price: price,
                },
                Side::Buy => Event::Buy {
                    ship: ship_id,
                    station,
                    ware,
                    quantity: units,
                    unit_price: price,
                },
            },
        });
        Ok(Trade {
            units,
            total,
            commands,
        })
    }
}
