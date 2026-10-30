//! The real market: [`EconomyDesk`] runs buy and sell orders through `galaxy_economy`.
//!
//! The economy prices, validates and builds every trade ([`galaxy_economy::plan_trade`]);
//! this desk only fits the order runner's "up to" requests to it and maps its errors:
//!
//! - A sell asks for at most the units held. With a floor the economy fills as many
//!   units as stay at or above it (each unit lowers the price) and refuses with
//!   `BeyondLimit` when the first one doesn't.
//! - A buy asks for at most the free hold. Without a limit the economy is all or
//!   nothing, so the desk walks the economy's own unit prices ([`galaxy_economy::quote`])
//!   and clamps the quantity to the stock, the free hold, the limit (if any) and what
//!   a player-owned ship's credits afford. NPC craft have no wallet.
//! - Errors map one for one onto [`TradeError`] ([`from_economy`]).
//!
//! The commands are the economy's, unchanged: they open with `ExpectDocked` (a ship
//! that undocked earlier in the tick can't trade) and change the line with a
//! compare-and-set `UpdateMarketLine`, then cargo, credits, activity tallies, the
//! `buy`/`sell` event and any `market_flooded` fact. The runner adds them to the
//! ship's one batch for the tick, so a lost race rejects the batch and the order
//! retries next tick. Money is centicredits throughout.

use galaxy_contract::prelude::*;
use galaxy_economy::{TradeError as EconomyError, plan_trade, pricing, quote};

use crate::trade::{Side, Trade, TradeDesk, TradeError, TradeRequest};

/// The economy crate's market behind the [`TradeDesk`] trait. What the game runs:
/// `galaxy_ship::systems(Box::new(EconomyDesk))`.
#[derive(Clone, Copy, Debug, Default)]
pub struct EconomyDesk;

impl TradeDesk for EconomyDesk {
    fn trade(&self, world: &WorldState, r: &TradeRequest) -> Result<Trade, TradeError> {
        let quantity = fit(world, r)?;
        let request = galaxy_contract::economy::TradeRequest {
            ship: r.ship.clone(),
            station: r.station.clone(),
            ware: r.ware.clone(),
            side: r.side,
            quantity,
            limit: r.limit,
        };
        let outcome = plan_trade(world, &request).map_err(|e| from_economy(e, r))?;
        Ok(Trade {
            units: outcome.filled,
            total: outcome.total,
            commands: outcome.commands,
        })
    }

    /// The top of the line's price band under the world's pricing rules: the most the
    /// formula can pay for one unit (empty stock, no saturation). `None` unless the
    /// station buys the ware from ships.
    fn ceiling(&self, world: &WorldState, station: &StationId, ware: &WareId) -> Option<u32> {
        let line = world.markets.get(station)?.lines.get(ware)?;
        let tradeable = world.wares.get(ware).is_some_and(|w| w.tradeable);
        (tradeable && pricing::side_for(line.role) == Side::Sell)
            .then(|| pricing::band(line, &world.economy.rules.pricing).1)
    }
}

/// The units to ask the economy for: as many of `r.quantity` as the ship, the market
/// and the wallet allow, or why not even one. Checks in the economy's order (ship,
/// docking, the line and side, then quantities) so errors read the same either way.
fn fit(world: &WorldState, r: &TradeRequest) -> Result<u32, TradeError> {
    let ship = world
        .ships
        .get(&r.ship)
        .ok_or_else(|| TradeError::UnknownShip(r.ship.clone()))?;
    if ship.docked_at.as_ref() != Some(&r.station) {
        return Err(TradeError::NotDocked(r.station.clone()));
    }
    // Validates the line and the side; with no units it prices nothing.
    let stock = quote(world, &r.station, &r.ware, r.side, 0)
        .map_err(|e| from_economy(e, r))?
        .stock_after;
    match r.side {
        Side::Sell => match r.quantity.min(ship.hold.count(&r.ware)) {
            0 => Err(TradeError::NothingToSell),
            n => Ok(n),
        },
        Side::Buy => {
            let capacity = world.hulls.get(&ship.hull).map_or(0, |h| h.hold_capacity);
            let free = u32::try_from(u64::from(capacity).saturating_sub(ship.hold.total()))
                .unwrap_or(u32::MAX);
            let want = r.quantity.min(free);
            if stock == 0 {
                return Err(TradeError::NoStock);
            }
            if want == 0 {
                return Err(TradeError::HoldFull);
            }
            // The economy's unit prices for the next `want` units (fewer if the
            // stock runs out), the same walk `plan_trade` does.
            let prices = quote(world, &r.station, &r.ware, Side::Buy, want)
                .map_err(|e| from_economy(e, r))?
                .unit_prices;
            let wallet = (ship.owner == Owner::Player).then_some(world.player.credits);
            let mut spent = 0_i64;
            let mut n = 0_u32;
            for &price in &prices {
                if let Some(limit) = r.limit
                    && price > limit
                {
                    if n == 0 {
                        return Err(TradeError::PastLimit { price, limit });
                    }
                    break;
                }
                let after = spent + i64::from(price);
                if wallet.is_some_and(|credits| after > credits) {
                    break;
                }
                spent = after;
                n += 1;
            }
            if n == 0 {
                return Err(TradeError::NoCredits);
            }
            Ok(n)
        }
    }
}

/// The ship crate's reason for an economy refusal of `r`. Exhaustive, so a new
/// economy error has to be mapped here before this compiles.
pub fn from_economy(e: EconomyError, r: &TradeRequest) -> TradeError {
    match e {
        EconomyError::UnknownShip { ship } => TradeError::UnknownShip(ship),
        EconomyError::UnknownStation { station } => TradeError::NoMarket(station),
        EconomyError::UnknownWare { ware } | EconomyError::NotTradeable { ware } => {
            TradeError::NotTraded {
                station: r.station.clone(),
                ware,
            }
        }
        EconomyError::NotListed { station, ware }
        | EconomyError::WrongSide { station, ware, .. } => TradeError::NotTraded { station, ware },
        EconomyError::NotDocked { station, .. } => TradeError::NotDocked(station),
        EconomyError::ZeroQuantity => match r.side {
            Side::Sell => TradeError::NothingToSell,
            Side::Buy => TradeError::HoldFull,
        },
        EconomyError::NotEnoughCargo { .. } => TradeError::NothingToSell,
        EconomyError::NotEnoughStock { .. } => TradeError::NoStock,
        EconomyError::HoldFull { .. } => TradeError::HoldFull,
        EconomyError::NotEnoughCredits { .. } => TradeError::NoCredits,
        EconomyError::BeyondLimit { price, limit } => TradeError::PastLimit { price, limit },
    }
}
