//! Trade execution: validate a [`TradeRequest`], price it unit by unit, and return
//! the commands to submit as one batch.
//!
//! [`trade`] is a plain function over `&WorldState`, so the order runner (W2), the
//! economy system (the player's dock screen) and bots all trade the same way. The
//! batch is atomic and checked again when it applies: it starts with
//! [`Command::ExpectDocked`], so a ship that undocked earlier in the tick can't trade,
//! and the market line update is a compare-and-set ([`Command::UpdateMarketLine`]),
//! so if anything else changed that line in the same tick the whole trade is rejected
//! and nothing moves.
//!
//! Money is in centicredits: each unit is priced and rounded on its own, and the total
//! is their sum.
//!
//! Only player-owned ships (the flagship and the player's craft) pay or earn credits.
//! NPC craft trade cargo against the market without a wallet.

use galaxy_contract::history::FactDraft;
use galaxy_contract::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::{self, spread_at};

/// History fact kind recorded when a sale leaves a line flooded.
pub const MARKET_FLOODED: FactKind = FactKind::from_static("market_flooded");

/// Why a trade can't happen.
#[derive(Clone, Debug, PartialEq, thiserror::Error, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "error", rename_all = "snake_case")]
pub enum TradeError {
    #[error("unknown ship `{ship}`")]
    UnknownShip { ship: ShipId },
    #[error("unknown station `{station}`")]
    UnknownStation { station: StationId },
    #[error("unknown ware `{ware}`")]
    UnknownWare { ware: WareId },
    #[error("{ship} isn't docked at {station}")]
    NotDocked { ship: ShipId, station: StationId },
    #[error("{ware} isn't for sale anywhere")]
    NotTradeable { ware: WareId },
    #[error("{station} doesn't trade {ware}")]
    NotListed { station: StationId, ware: WareId },
    #[error("{station} doesn't {side:?} {ware} from ships")]
    WrongSide {
        station: StationId,
        ware: WareId,
        side: TradeSide,
    },
    #[error("a trade needs at least one unit")]
    ZeroQuantity,
    #[error("the hold has {have} {ware}, not {want}")]
    NotEnoughCargo { ware: WareId, have: u32, want: u32 },
    #[error("the station has {have} {ware}, not {want}")]
    NotEnoughStock { ware: WareId, have: u32, want: u32 },
    #[error("the hold has room for {free}, not {want}")]
    HoldFull { free: u32, want: u32 },
    /// Centicredits.
    #[error("{} credits needed, {} on hand", show_credits(*need), show_credits(*have))]
    NotEnoughCredits { have: i64, need: i64 },
    /// Centicredits.
    #[error(
        "the next unit is {}, beyond the limit of {}",
        show_credits(i64::from(*price)),
        show_credits(i64::from(*limit))
    )]
    BeyondLimit { price: u32, limit: u32 },
}

/// What the next units would fetch or cost, for UIs and bots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Quote {
    pub station: StationId,
    pub ware: WareId,
    pub side: TradeSide,
    /// Unit prices in centicredits, in order; shorter than asked when a buy runs out of
    /// stock.
    pub unit_prices: Vec<u32>,
    /// In centicredits: the sum of the unit prices.
    pub total: i64,
    pub stock_after: u32,
    pub saturation_after: f64,
}

impl Quote {
    pub fn units(&self) -> u32 {
        self.unit_prices.len() as u32
    }

    /// The average unit price in centicredits.
    pub fn average(&self) -> f64 {
        if self.unit_prices.is_empty() {
            0.0
        } else {
            self.total as f64 / self.unit_prices.len() as f64
        }
    }
}

/// A trade worked out in full.
#[derive(Clone, Debug, PartialEq)]
pub struct TradeOutcome {
    pub request: TradeRequest,
    /// Units that trade (the quantity, or fewer under a limit).
    pub filled: u32,
    /// Centicredits that change hands.
    pub total: i64,
    /// Centicredits per unit, in order.
    pub unit_prices: Vec<u32>,
    pub line_after: MarketLine,
    /// Submit these as one batch.
    pub commands: Vec<Command>,
}

fn line_of<'a>(
    world: &'a WorldState,
    station: &StationId,
    ware: &WareId,
) -> Result<&'a MarketLine, TradeError> {
    if !world.stations.contains(station) {
        return Err(TradeError::UnknownStation {
            station: station.clone(),
        });
    }
    match world.wares.get(ware) {
        None => {
            return Err(TradeError::UnknownWare { ware: ware.clone() });
        }
        Some(w) if !w.tradeable => {
            return Err(TradeError::NotTradeable { ware: ware.clone() });
        }
        Some(_) => {}
    }
    world
        .markets
        .get(station)
        .and_then(|m| m.lines.get(ware))
        .ok_or_else(|| TradeError::NotListed {
            station: station.clone(),
            ware: ware.clone(),
        })
}

fn check_side(
    line: &MarketLine,
    station: &StationId,
    ware: &WareId,
    side: TradeSide,
) -> Result<(), TradeError> {
    if pricing::side_for(line.role) == side {
        Ok(())
    } else {
        Err(TradeError::WrongSide {
            station: station.clone(),
            ware: ware.clone(),
            side,
        })
    }
}

/// The price of the next `units` on `side` of a station's line, with no ship involved.
pub fn quote(
    world: &WorldState,
    station: &StationId,
    ware: &WareId,
    side: TradeSide,
    units: u32,
) -> Result<Quote, TradeError> {
    let line = line_of(world, station, ware)?;
    check_side(line, station, ware, side)?;
    let rules = &world.economy.rules.pricing;
    let (unit_prices, after) = pricing::walk(line, side, units, rules, spread_at(world, station));
    Ok(Quote {
        station: station.clone(),
        ware: ware.clone(),
        side,
        total: pricing::total(&unit_prices),
        unit_prices,
        stock_after: after.stock,
        saturation_after: after.saturation,
    })
}

/// Whether `req` would go through right now.
pub fn check_trade(world: &WorldState, req: &TradeRequest) -> Result<(), TradeError> {
    plan_trade(world, req).map(drop)
}

/// The commands for `req`, to submit as one batch (see [`trade_batch`] for the cause).
pub fn trade(world: &WorldState, req: &TradeRequest) -> Result<Vec<Command>, TradeError> {
    plan_trade(world, req).map(|o| o.commands)
}

/// `req` as a ready batch, caused by the ship's verb.
pub fn trade_batch(world: &WorldState, req: &TradeRequest) -> Result<CommandBatch, TradeError> {
    let outcome = plan_trade(world, req)?;
    Ok(CommandBatch {
        cause: trade_cause(world, &outcome),
        commands: outcome.commands,
    })
}

/// The cause for a trade: the player's verb for the flagship, the craft's otherwise.
pub fn trade_cause(world: &WorldState, outcome: &TradeOutcome) -> Cause {
    let req = &outcome.request;
    let actor = if world.player.ship.as_ref() == Some(&req.ship) {
        Actor::Player
    } else {
        Actor::Ship {
            ship: req.ship.clone(),
        }
    };
    let (verb, did) = match req.side {
        TradeSide::Sell => (VerbKind::Sell, "sold"),
        TradeSide::Buy => (VerbKind::Buy, "bought"),
    };
    let ware = world
        .wares
        .get(&req.ware)
        .map_or(req.ware.as_str(), |w| w.name.as_str());
    let station = world
        .stations
        .get(&req.station)
        .map_or(req.station.as_str(), |s| s.name.as_str());
    Cause::verb(
        actor,
        verb,
        format!(
            "{did} {} {ware} at {station} for {} credits",
            outcome.filled,
            show_credits(outcome.total)
        ),
    )
}

/// Validates `req` and works it out unit by unit.
pub fn plan_trade(world: &WorldState, req: &TradeRequest) -> Result<TradeOutcome, TradeError> {
    let ship = world
        .ships
        .get(&req.ship)
        .ok_or_else(|| TradeError::UnknownShip {
            ship: req.ship.clone(),
        })?;
    if ship.docked_at.as_ref() != Some(&req.station) {
        return Err(TradeError::NotDocked {
            ship: req.ship.clone(),
            station: req.station.clone(),
        });
    }
    let line = line_of(world, &req.station, &req.ware)?;
    check_side(line, &req.station, &req.ware, req.side)?;
    if req.quantity == 0 {
        return Err(TradeError::ZeroQuantity);
    }
    match req.side {
        TradeSide::Sell => {
            let have = ship.hold.count(&req.ware);
            if have < req.quantity {
                return Err(TradeError::NotEnoughCargo {
                    ware: req.ware.clone(),
                    have,
                    want: req.quantity,
                });
            }
        }
        TradeSide::Buy => {
            let capacity = galaxy_contract::modifiers::hold_capacity(world, ship);
            let free =
                u32::try_from(capacity.saturating_sub(ship.hold.total())).unwrap_or(u32::MAX);
            if free < req.quantity {
                return Err(TradeError::HoldFull {
                    free,
                    want: req.quantity,
                });
            }
            if req.limit.is_none() && line.stock < req.quantity {
                return Err(TradeError::NotEnoughStock {
                    ware: req.ware.clone(),
                    have: line.stock,
                    want: req.quantity,
                });
            }
        }
    }

    let rules = &world.economy.rules.pricing;
    let spread = spread_at(world, &req.station);
    let mut after = line.clone();
    let mut unit_prices = Vec::new();
    for _ in 0..req.quantity {
        if req.side == TradeSide::Buy && after.stock == 0 {
            break;
        }
        let price = pricing::unit_cents(&after, rules, spread);
        if let Some(limit) = req.limit {
            let ok = match req.side {
                TradeSide::Sell => price >= limit,
                TradeSide::Buy => price <= limit,
            };
            if !ok {
                if unit_prices.is_empty() {
                    return Err(TradeError::BeyondLimit { price, limit });
                }
                break;
            }
        }
        unit_prices.push(price);
        match req.side {
            TradeSide::Sell => pricing::sell_one(&mut after, rules),
            TradeSide::Buy => pricing::buy_one(&mut after),
        }
    }
    let filled = unit_prices.len() as u32;
    if filled == 0 {
        return Err(TradeError::NotEnoughStock {
            ware: req.ware.clone(),
            have: line.stock,
            want: req.quantity,
        });
    }
    let total = pricing::total(&unit_prices);
    let pays_credits = ship.owner == Owner::Player;
    if pays_credits && req.side == TradeSide::Buy && world.player.credits < total {
        return Err(TradeError::NotEnoughCredits {
            have: world.player.credits,
            need: total,
        });
    }
    let after = pricing::priced(&after, rules, spread);

    let filled_i = i64::from(filled);
    let (cargo, credits, tally) = match req.side {
        TradeSide::Sell => (
            -filled_i,
            total,
            ActivityTally {
                trades: 1,
                units_sold: filled_i,
                credits_earned: total,
                ..ActivityTally::default()
            },
        ),
        TradeSide::Buy => (
            filled_i,
            -total,
            ActivityTally {
                trades: 1,
                units_bought: filled_i,
                credits_spent: total,
                ..ActivityTally::default()
            },
        ),
    };
    let unit_price = (total as f64 / f64::from(filled)).round().max(0.0) as u32;
    let event = match req.side {
        TradeSide::Sell => Event::Sell {
            ship: req.ship.clone(),
            station: req.station.clone(),
            ware: req.ware.clone(),
            quantity: filled,
            unit_price,
        },
        TradeSide::Buy => Event::Buy {
            ship: req.ship.clone(),
            station: req.station.clone(),
            ware: req.ware.clone(),
            quantity: filled,
            unit_price,
        },
    };

    let mut commands = vec![
        Command::ExpectDocked {
            ship: req.ship.clone(),
            station: req.station.clone(),
        },
        Command::UpdateMarketLine {
            station: req.station.clone(),
            ware: req.ware.clone(),
            from: Box::new(line.clone()),
            to: Box::new(after.clone()),
        },
        Command::AdjustCargo {
            ship: req.ship.clone(),
            ware: req.ware.clone(),
            delta: cargo,
        },
    ];
    if pays_credits && credits != 0 {
        commands.push(Command::AdjustCredits { delta: credits });
    }
    commands.push(Command::AdjustActivity {
        scope: ActivityScope::Station {
            station: req.station.clone(),
        },
        delta: tally.clone(),
    });
    if let Some(region) = world.stations.get(&req.station).map(|s| s.region.clone()) {
        commands.push(Command::AdjustActivity {
            scope: ActivityScope::Region { region },
            delta: tally,
        });
    }
    commands.push(Command::Emit { event });
    if req.side == TradeSide::Sell && after.saturation >= rules.flood_saturation {
        commands.push(Command::RecordFact {
            fact: Box::new(FactDraft {
                kind: MARKET_FLOODED,
                subjects: Subjects {
                    ship: Some(req.ship.clone()),
                    station: Some(req.station.clone()),
                    ware: Some(req.ware.clone()),
                    ..Subjects::default()
                },
                amount: Some(filled_i),
                note: format!("saturation {:.2} after the sale", after.saturation),
            }),
        });
    }

    Ok(TradeOutcome {
        request: req.clone(),
        filled,
        total,
        unit_prices,
        line_after: after,
        commands,
    })
}
