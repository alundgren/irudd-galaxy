//! Buy and sell orders go through a [`TradeDesk`].
//!
//! Prices and saturation belong to the economy crate. The ship crate only knows that a
//! docked ship asks to trade and gets back the commands that do it (cargo, credits,
//! the market line, the `buy`/`sell` event) or a reason it can't. Implement the trait
//! (or pass a closure with the same signature) to plug a real market in.

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
    /// Units wanted. The desk may move fewer (stock, hold space, credits, `limit`).
    pub quantity: u32,
    /// Selling: only while the unit price is at or above this (the floor).
    /// Buying: only while the unit price is at or below this.
    pub limit: Option<u32>,
}

/// What a trade does: the commands, applied atomically with the order's cause.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Trade {
    /// Units actually moved.
    pub units: u32,
    /// Cargo, credits, market and event commands. Include `Emit` of `Event::Buy` or
    /// `Event::Sell` so history and counters see the trade.
    pub commands: Vec<Command>,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum TradeError {
    #[error("{0} has no market")]
    NoMarket(StationId),
    #[error("{station} doesn't trade {ware}")]
    NotTraded { station: StationId, ware: WareId },
    #[error("the price {price} is past the limit {limit}")]
    PastLimit { price: u32, limit: u32 },
    #[error("nothing to sell")]
    NothingToSell,
    #[error("no stock")]
    NoStock,
    #[error("not enough credits")]
    NoCredits,
    #[error("the hold is full")]
    HoldFull,
    #[error("{0}")]
    Refused(String),
}

/// Turns a trade request into commands, or says why not. Reads the world as it was at
/// the start of the tick.
pub trait TradeDesk {
    fn trade(&self, world: &WorldState, request: &TradeRequest) -> Result<Trade, TradeError>;
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
}

/// A stand-in market: trades at the line's current price, which never moves; stock
/// goes up and down. Credits move only for player-owned ships (NPC craft have no
/// wallet here). The economy crate's desk replaces it.
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
                if space == 0 {
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
        let mut new_line = line.clone();
        new_line.stock = (i64::from(line.stock) + stock * i64::from(units)) as u32;
        let mut commands = vec![
            Command::AdjustCargo {
                ship: r.ship.clone(),
                ware: r.ware.clone(),
                delta: cargo * i64::from(units),
            },
            Command::SetMarketLine {
                station: r.station.clone(),
                ware: r.ware.clone(),
                line: Some(new_line),
            },
        ];
        if players {
            commands.push(Command::AdjustCredits {
                delta: credits * i64::from(units) * i64::from(price),
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
        Ok(Trade { units, commands })
    }
}
