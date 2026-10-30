//! Market levers for scripts and the game master.
//!
//! A lever changes *supply and demand* at a station: how much its producers make and
//! its consumers use per pulse, which lines it lists, and its stock. It never touches
//! the price formula; prices follow from the new supply through the same maths as
//! everything else. Each call returns commands for one batch, which the caller wraps
//! with its own cause (a script pack revision, a fast-model decision), so the change
//! is journaled and can be rolled back.
//!
//! Base levers (`SetBase`, `ScaleBase`) that move a line's base price also record a
//! `market_base_changed` fact (station, ware, amount = the new base in centicredits),
//! so scripts and people can notice the change and cite it.
//!
//! Rates go through [`Command::SetMarketRates`], which leaves stock and saturation
//! alone, so rolling a lever back restores the rates without undoing trades made
//! since. Restocking goes through [`Command::SetMarketStock`] (stock and the prices
//! that follow); adding and removing lines use `SetMarketLine` / `PutMarket`, which a
//! script may cause only with `market.lines` (`galaxy_contract::needs`).

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use galaxy_core::apply::{Staged, apply};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::{self, spread_at};

/// One change to a station's market.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "lever", rename_all = "snake_case")]
pub enum Lever {
    /// Adds (or with a negative number removes) producer units per pulse.
    AddProducers {
        station: StationId,
        ware: WareId,
        per_pulse: i64,
    },
    /// Adds (or removes) consumer units per pulse.
    AddConsumers {
        station: StationId,
        ware: WareId,
        per_pulse: i64,
    },
    /// Multiplies the line's production (e.g. 1.5 for "+50% rations").
    ScaleProducers {
        station: StationId,
        ware: WareId,
        factor: f64,
    },
    /// Multiplies the line's consumption (e.g. 2.0 for a shortage).
    ScaleConsumers {
        station: StationId,
        ware: WareId,
        factor: f64,
    },
    /// Lists a new ware at a station (creating its market if it has none).
    AddLine {
        station: StationId,
        ware: WareId,
        line: LineSpec,
    },
    /// Stops trading a ware at a station.
    RemoveLine { station: StationId, ware: WareId },
    /// Sets a line's stock outright (a delivery, a theft, a gift).
    SetStock {
        station: StationId,
        ware: WareId,
        stock: u32,
    },
    /// Sets a line's base price, in centicredits (a change of owner, the end of a
    /// shortage). Its band and price follow from the formula; stock, saturation and
    /// rates stay. Standing tender floors are checked against the new ceiling.
    SetBase {
        station: StationId,
        ware: WareId,
        base_price: u32,
    },
    /// Multiplies a line's base price (1.4 for a shortage), rounded to a centicredit.
    ScaleBase {
        station: StationId,
        ware: WareId,
        factor: f64,
    },
}

/// A new market line. Its price comes from the formula.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineSpec {
    pub role: MarketRole,
    /// In centicredits.
    pub base_price: u32,
    #[serde(default)]
    pub target: u32,
    #[serde(default)]
    pub stock: u32,
    #[serde(default)]
    pub production: u32,
    #[serde(default)]
    pub consumption: u32,
    #[serde(default)]
    pub sat_per_unit: f64,
    #[serde(default)]
    pub sat_recover: f64,
    #[serde(default)]
    pub needs: BTreeSet<WareId>,
}

#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum LeverError {
    #[error("unknown station `{0}`")]
    UnknownStation(StationId),
    #[error("unknown ware `{0}`")]
    UnknownWare(WareId),
    #[error("{station} doesn't trade {ware}")]
    NoLine { station: StationId, ware: WareId },
    #[error("{station} already trades {ware}")]
    LineExists { station: StationId, ware: WareId },
    #[error("a rate can't go below zero ({0})")]
    NegativeRate(i64),
    #[error("a scale factor must be finite and not negative ({0})")]
    BadFactor(f64),
    #[error("a base price must be at least 0.01 cr ({0})")]
    BadBase(f64),
    #[error("lever {index} doesn't apply: {reason}")]
    Apply { index: usize, reason: String },
}

fn line<'a>(
    world: &'a WorldState,
    station: &StationId,
    ware: &WareId,
) -> Result<&'a MarketLine, LeverError> {
    if !world.stations.contains(station) {
        return Err(LeverError::UnknownStation(station.clone()));
    }
    world
        .markets
        .get(station)
        .and_then(|m| m.lines.get(ware))
        .ok_or_else(|| LeverError::NoLine {
            station: station.clone(),
            ware: ware.clone(),
        })
}

fn rates_of(l: &MarketLine) -> MarketRates {
    MarketRates {
        production: l.production,
        consumption: l.consumption,
        production_scale: l.production_scale,
        consumption_scale: l.consumption_scale,
    }
}

fn add_rate(rate: u32, delta: i64) -> Result<u32, LeverError> {
    let next = i64::from(rate) + delta;
    u32::try_from(next).map_err(|_| LeverError::NegativeRate(next))
}

fn check_factor(factor: f64) -> Result<(), LeverError> {
    if factor.is_finite() && factor >= 0.0 {
        Ok(())
    } else {
        Err(LeverError::BadFactor(factor))
    }
}

/// `SetMarketBase` for a new base price (centicredits), with the band and price the
/// formula gives it on the line as it is.
fn base_commands(
    world: &WorldState,
    station: &StationId,
    ware: &WareId,
    base: f64,
) -> Result<Vec<Command>, LeverError> {
    if !(base.is_finite() && (1.0..=f64::from(u32::MAX)).contains(&base)) {
        return Err(LeverError::BadBase(base / 100.0));
    }
    let old = line(world, station, ware)?;
    let new = pricing::priced(
        &MarketLine {
            base_price: base as u32,
            ..old.clone()
        },
        &world.economy.rules.pricing,
        spread_at(world, station),
    );
    let mut out = vec![Command::SetMarketBase {
        station: station.clone(),
        ware: ware.clone(),
        base: MarketBase {
            base_price: new.base_price,
            min_price: new.min_price,
            max_price: new.max_price,
            price: new.price,
        },
    }];
    // A base that moves is something people notice (a standing floor may now sit
    // above the price band): one `market_base_changed` fact, amount the new base.
    if new.base_price != old.base_price {
        out.push(Command::RecordFact {
            fact: Box::new(galaxy_contract::history::FactDraft {
                kind: FactKind::from_static(BASE_CHANGED),
                subjects: Subjects {
                    station: Some(station.clone()),
                    ware: Some(ware.clone()),
                    ..Subjects::default()
                },
                amount: Some(i64::from(new.base_price)),
                note: format!(
                    "{ware} base at {station}: {} -> {} cr",
                    show_credits(i64::from(old.base_price)),
                    show_credits(i64::from(new.base_price))
                ),
            }),
        });
    }
    Ok(out)
}

/// The fact kind a base lever records when it moves a line's base price.
pub const BASE_CHANGED: &str = "market_base_changed";

/// The commands for one lever against the world as it is.
pub fn lever_commands(world: &WorldState, lever: &Lever) -> Result<Vec<Command>, LeverError> {
    let rates = |station: &StationId,
                 ware: &WareId,
                 f: &dyn Fn(&mut MarketRates)|
     -> Result<Vec<Command>, LeverError> {
        let mut r = rates_of(line(world, station, ware)?);
        f(&mut r);
        Ok(vec![Command::SetMarketRates {
            station: station.clone(),
            ware: ware.clone(),
            rates: r,
        }])
    };
    match lever {
        Lever::AddProducers {
            station,
            ware,
            per_pulse,
        } => {
            let next = add_rate(line(world, station, ware)?.production, *per_pulse)?;
            rates(station, ware, &|r| r.production = next)
        }
        Lever::AddConsumers {
            station,
            ware,
            per_pulse,
        } => {
            let next = add_rate(line(world, station, ware)?.consumption, *per_pulse)?;
            rates(station, ware, &|r| r.consumption = next)
        }
        Lever::ScaleProducers {
            station,
            ware,
            factor,
        } => {
            check_factor(*factor)?;
            rates(station, ware, &|r| r.production_scale *= factor)
        }
        Lever::ScaleConsumers {
            station,
            ware,
            factor,
        } => {
            check_factor(*factor)?;
            rates(station, ware, &|r| r.consumption_scale *= factor)
        }
        Lever::AddLine {
            station,
            ware,
            line: spec,
        } => {
            if !world.stations.contains(station) {
                return Err(LeverError::UnknownStation(station.clone()));
            }
            if !world.wares.contains(ware) {
                return Err(LeverError::UnknownWare(ware.clone()));
            }
            if let Some(missing) = spec.needs.iter().find(|w| !world.wares.contains(*w)) {
                return Err(LeverError::UnknownWare(missing.clone()));
            }
            let market = world.markets.get(station);
            if market.is_some_and(|m| m.lines.contains_key(ware)) {
                return Err(LeverError::LineExists {
                    station: station.clone(),
                    ware: ware.clone(),
                });
            }
            let new = pricing::priced(
                &MarketLine {
                    stock: spec.stock,
                    base_price: spec.base_price,
                    production: spec.production,
                    consumption: spec.consumption,
                    role: spec.role,
                    target: spec.target,
                    sat_per_unit: spec.sat_per_unit,
                    sat_recover: spec.sat_recover,
                    needs: spec.needs.clone(),
                    ..MarketLine::default()
                },
                &world.economy.rules.pricing,
                spread_at(world, station),
            );
            Ok(vec![match market {
                Some(_) => Command::SetMarketLine {
                    station: station.clone(),
                    ware: ware.clone(),
                    line: Some(new),
                },
                None => Command::PutMarket {
                    id: station.clone(),
                    value: Some(Box::new(Market {
                        station: station.clone(),
                        lines: BTreeMap::from([(ware.clone(), new)]),
                    })),
                },
            }])
        }
        Lever::RemoveLine { station, ware } => {
            line(world, station, ware)?;
            Ok(vec![Command::SetMarketLine {
                station: station.clone(),
                ware: ware.clone(),
                line: None,
            }])
        }
        Lever::SetBase {
            station,
            ware,
            base_price,
        } => base_commands(world, station, ware, f64::from(*base_price)),
        Lever::ScaleBase {
            station,
            ware,
            factor,
        } => {
            check_factor(*factor)?;
            let base = f64::from(line(world, station, ware)?.base_price) * factor;
            base_commands(world, station, ware, base.round())
        }
        Lever::SetStock {
            station,
            ware,
            stock,
        } => {
            let old = line(world, station, ware)?;
            let new = pricing::priced(
                &MarketLine {
                    stock: *stock,
                    stock_carry: 0.0,
                    ..old.clone()
                },
                &world.economy.rules.pricing,
                spread_at(world, station),
            );
            Ok(vec![Command::SetMarketStock {
                station: station.clone(),
                ware: ware.clone(),
                stock: MarketStock {
                    stock: new.stock,
                    stock_carry: new.stock_carry,
                    min_price: new.min_price,
                    max_price: new.max_price,
                    price: new.price,
                },
            }])
        }
    }
}

/// Several levers as one batch with `cause`. Each lever sees the ones before it, so
/// "add 2 producers, then scale by 1.5" composes.
pub fn lever_batch(
    world: &WorldState,
    cause: Cause,
    levers: &[Lever],
) -> Result<CommandBatch, LeverError> {
    let mut scratch = world.clone();
    let mut batch = CommandBatch::new(cause);
    for (index, lever) in levers.iter().enumerate() {
        for cmd in lever_commands(&scratch, lever)? {
            apply(&mut scratch, &cmd, &mut Staged::default()).map_err(|e| LeverError::Apply {
                index,
                reason: e.to_string(),
            })?;
            batch.push(cmd);
        }
    }
    Ok(batch)
}
