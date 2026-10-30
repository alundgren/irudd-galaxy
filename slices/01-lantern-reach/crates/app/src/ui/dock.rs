//! The dock screen's model: the market lines as the player trades them.
//!
//! Each market line goes one way: a line the station *buys* is one the player sells
//! into, a line it *sells* the player buys from. So every row has one action, in ones
//! or as much as makes sense ("all" of the hold, or as many as fit and are affordable).

use galaxy_contract::prelude::*;
use galaxy_economy::{plan_trade, quote};

/// How much to trade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Amount {
    One,
    /// Sell everything of it in the hold; buy as many as fit and you can afford.
    Most,
}

/// One market line on the dock screen.
#[derive(Clone, Debug, PartialEq)]
pub struct DockLine {
    pub ware: WareId,
    pub name: String,
    /// The player's side: `Sell` where the station buys.
    pub side: TradeSide,
    /// The next unit's price in centicredits, from `quote`; `None` when nothing can
    /// trade (no stock left to buy).
    pub unit: Option<u32>,
    pub stock: u32,
    pub in_hold: u32,
}

/// The market lines at `station` for the flagship, in the market's order.
pub fn dock_lines(world: &WorldState, station: &StationId) -> Vec<DockLine> {
    let Some(market) = world.markets.get(station) else {
        return Vec::new();
    };
    let hold = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .map(|s| s.hold.clone())
        .unwrap_or_default();
    market
        .lines
        .iter()
        .filter(|(w, _)| world.wares.get(*w).is_none_or(|w| w.tradeable))
        .map(|(ware, line)| {
            let side = galaxy_economy::pricing::side_for(line.role);
            let unit = quote(world, station, ware, side, 1)
                .ok()
                .and_then(|q| q.unit_prices.first().copied());
            DockLine {
                ware: ware.clone(),
                name: world
                    .wares
                    .get(ware)
                    .map_or_else(|| ware.to_string(), |w| w.name.clone()),
                side,
                unit,
                stock: line.stock,
                in_hold: hold.count(ware),
            }
        })
        .collect()
}

/// The request for trading `amount` of `line` now, or `None` when there's nothing to
/// ask for (then the caller asks for one, so the player gets the reason).
pub fn trade_request(
    world: &WorldState,
    station: &StationId,
    line: &DockLine,
    amount: Amount,
) -> Option<TradeRequest> {
    let ship = world.player.ship.clone()?;
    let base = TradeRequest {
        ship,
        station: station.clone(),
        ware: line.ware.clone(),
        side: line.side,
        quantity: 1,
        limit: None,
    };
    match (amount, line.side) {
        (Amount::One, _) => Some(base),
        (Amount::Most, TradeSide::Sell) => Some(TradeRequest {
            quantity: line.in_hold.max(1),
            ..base
        }),
        (Amount::Most, TradeSide::Buy) => {
            // The most that fits, is in stock and can be paid for, walking the price.
            let room = world
                .ships
                .get(&base.ship)
                .map(|s| {
                    galaxy_contract::modifiers::hold_capacity(world, s)
                        .saturating_sub(s.hold.total())
                })
                .unwrap_or(0);
            let most = room.min(u64::from(line.stock)) as u32;
            let mut best = 1;
            for n in (1..=most).rev() {
                let req = TradeRequest {
                    quantity: n,
                    ..base.clone()
                };
                if plan_trade(world, &req).is_ok() {
                    best = n;
                    break;
                }
            }
            Some(TradeRequest {
                quantity: best,
                ..base
            })
        }
    }
}
