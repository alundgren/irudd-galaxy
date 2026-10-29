//! The price formula, unit by unit.
//!
//! ```text
//! stock_factor = clamp(1 + stock_slope * (1 - stock / target), stock_factor_min, stock_factor_max)
//! pays = base * stock_factor * (1 - saturation)      (a `buys` line)
//! asks = base * stock_factor * spread                (a `sells` line)
//! ```
//!
//! Every number comes from the line and the world's
//! [`PricingRules`](galaxy_contract::economy::PricingRules). Prices are in
//! centicredits (`content/08-contract.md`, "Numbers"). A trade of N units is N
//! single-unit trades: each unit sold adds one to stock and `sat_per_unit` to
//! saturation, each unit bought takes one from stock, and the next unit is priced
//! again. Each unit's price is rounded half away from zero to a centicredit and paid
//! exactly that; a trade's total is the sum. So splitting a trade into smaller ones
//! never changes what it pays.

use galaxy_contract::economy::PricingRules;
use galaxy_contract::prelude::*;

fn ordered(a: f64, b: f64) -> (f64, f64) {
    if a <= b { (a, b) } else { (b, a) }
}

/// The stock part of the price.
pub fn stock_factor(line: &MarketLine, rules: &PricingRules) -> f64 {
    let (lo, hi) = ordered(rules.stock_factor_min, rules.stock_factor_max);
    if line.target == 0 {
        return 1.0_f64.clamp(lo, hi);
    }
    let fill = f64::from(line.stock) / f64::from(line.target);
    (1.0 + rules.stock_slope * (1.0 - fill)).clamp(lo, hi)
}

/// The asking-price spread at `station`: by the player's regard with its owner, or
/// the rules' default for unowned stations.
pub fn spread_at(world: &WorldState, station: &StationId) -> f64 {
    let rules = &world.economy.rules.pricing;
    let standing = world
        .stations
        .get(station)
        .and_then(|s| s.owner.as_ref())
        .and_then(|f| world.factions.get(f))
        .map(|f| f.standing);
    match standing {
        None => rules.spread,
        Some(standing) => rules
            .regard
            .iter()
            .filter(|t| t.min_standing <= standing)
            .max_by_key(|t| t.min_standing)
            .map_or(rules.spread, |t| t.spread),
    }
}

/// The exact price of the next unit in centicredits, before rounding: what the
/// station pays (`buys`) or asks (`sells`).
pub fn unit_price(line: &MarketLine, rules: &PricingRules, spread: f64) -> f64 {
    let base = f64::from(line.base_price) * stock_factor(line, rules);
    match line.role {
        MarketRole::Buys => base * (1.0 - line.saturation.clamp(0.0, 1.0)),
        MarketRole::Sells => base * spread,
    }
}

/// The price the next unit trades at: [`unit_price`] rounded half away from zero to a
/// centicredit.
pub fn unit_cents(line: &MarketLine, rules: &PricingRules, spread: f64) -> u32 {
    unit_price(line, rules, spread)
        .round()
        .clamp(0.0, f64::from(u32::MAX)) as u32
}

/// The lowest and highest prices the formula can give this line under these rules.
pub fn formula_range(line: &MarketLine, rules: &PricingRules) -> (f64, f64) {
    let base = f64::from(line.base_price);
    let (sf_lo, sf_hi) = ordered(rules.stock_factor_min, rules.stock_factor_max);
    match line.role {
        MarketRole::Buys => {
            let sat_max = rules.saturation_max.clamp(0.0, 1.0);
            (base * sf_lo * (1.0 - sat_max), base * sf_hi)
        }
        MarketRole::Sells => {
            let spreads = rules.regard.iter().map(|t| t.spread);
            let lo = spreads.clone().fold(rules.spread, f64::min);
            let hi = spreads.fold(rules.spread, f64::max);
            (base * sf_lo * lo, base * sf_hi * hi)
        }
    }
}

/// The band a line's cached price must stay in: the formula's range, widened to whole
/// centicredits.
pub fn band(line: &MarketLine, rules: &PricingRules) -> (u32, u32) {
    let (lo, hi) = formula_range(line, rules);
    (lo.floor().max(0.0) as u32, hi.ceil().max(0.0) as u32)
}

/// The line with its cached `price`, `min_price` and `max_price` brought up to date.
pub fn priced(line: &MarketLine, rules: &PricingRules, spread: f64) -> MarketLine {
    let (min_price, max_price) = band(line, rules);
    let price = unit_cents(line, rules, spread).clamp(min_price, max_price);
    MarketLine {
        price,
        min_price,
        max_price,
        ..line.clone()
    }
}

/// One unit sold to the station: stock up, saturation up (to the cap).
pub fn sell_one(line: &mut MarketLine, rules: &PricingRules) {
    line.stock = line.stock.saturating_add(1);
    let cap = rules.saturation_max.clamp(0.0, 1.0);
    if line.saturation < cap {
        line.saturation = (line.saturation + line.sat_per_unit).min(cap);
    }
}

/// One unit bought from the station: stock down.
pub fn buy_one(line: &mut MarketLine) {
    line.stock = line.stock.saturating_sub(1);
}

/// The next `units` unit prices on `side` in centicredits, in order, and the line after
/// them. Buys stop when stock runs out.
pub fn walk(
    line: &MarketLine,
    side: TradeSide,
    units: u32,
    rules: &PricingRules,
    spread: f64,
) -> (Vec<u32>, MarketLine) {
    let mut line = line.clone();
    let mut prices = Vec::with_capacity(units as usize);
    for _ in 0..units {
        if side == TradeSide::Buy && line.stock == 0 {
            break;
        }
        prices.push(unit_cents(&line, rules, spread));
        match side {
            TradeSide::Sell => sell_one(&mut line, rules),
            TradeSide::Buy => buy_one(&mut line),
        }
    }
    (prices, line)
}

/// A trade's total in centicredits: the sum of its rounded unit prices.
pub fn total(prices: &[u32]) -> i64 {
    prices.iter().map(|&p| i64::from(p)).sum()
}

/// The side a ship trades on for a line: sell to a `buys` line, buy from a `sells` one.
pub fn side_for(role: MarketRole) -> TradeSide {
    match role {
        MarketRole::Buys => TradeSide::Sell,
        MarketRole::Sells => TradeSide::Buy,
    }
}
