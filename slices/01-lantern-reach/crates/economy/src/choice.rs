//! NPC trade choice: native utility AI.
//!
//! [`rank_trades`] scores every plan a trader is allowed (which of its cargo to sell
//! here, what to buy here, where to go next) from what the world allows, and returns
//! them best first. Each plan is complete: the top one is always a sensible default.
//! Later the fast-model layer asks Jev to pick among the top few; if that fails or is
//! slow, the craft takes the first.
//!
//! Plans are priced unit by unit with the live market (saturation included), so a
//! trader avoids dumping into a flooded buyer. Scoring is a pure function of the
//! world and the profile, so it's deterministic.

use std::collections::BTreeSet;

use galaxy_contract::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::{self, spread_at};
use crate::trade::TradeError;

/// What a trader may do. The mind builds this from saved `craft.toml` policies.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TraderProfile {
    pub ship: ShipId,
    /// Stations it may visit.
    pub stations: BTreeSet<StationId>,
    /// Wares it may trade.
    pub wares: BTreeSet<WareId>,
    /// Most units of one ware per docking.
    pub max_units_per_ware: u32,
    /// Most different wares traded per docking.
    pub max_wares_per_docking: u32,
    /// Centicredits a plan is marked down per world unit of travel, so near trades win ties.
    #[serde(default)]
    pub cost_per_distance: f64,
}

/// One complete choice: trades here, now, and where to go next.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct TradePlan {
    /// Where the trades happen (the station the ship is docked at).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub here: Option<StationId>,
    /// Sells first, then buys. Each is ready for [`crate::trade`].
    pub trades: Vec<TradeRequest>,
    /// Where to fly next.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub destination: Option<StationId>,
    /// Cargo carried to the destination and what it should fetch there.
    pub carry: Vec<Carry>,
    /// Sales here, minus purchases here, plus what the carried cargo fetches there, in
    /// centicredits.
    pub expected_credits: i64,
    pub travel: f32,
    pub score: f64,
    /// One line a person (or a fast model) can read.
    pub summary: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Carry {
    pub ware: WareId,
    pub units: u32,
    /// In centicredits.
    pub expected_credits: i64,
}

fn sell_value(world: &WorldState, station: &StationId, ware: &WareId, units: u32) -> Option<i64> {
    let line = world.markets.get(station)?.lines.get(ware)?;
    if line.role != MarketRole::Buys || units == 0 {
        return None;
    }
    let (prices, _) = pricing::walk(
        line,
        TradeSide::Sell,
        units,
        &world.economy.rules.pricing,
        spread_at(world, station),
    );
    Some(pricing::total(&prices))
}

fn name<'a>(world: &'a WorldState, ware: &'a WareId) -> &'a str {
    world
        .wares
        .get(ware)
        .map_or(ware.as_str(), |w| w.name.as_str())
}

fn station_name<'a>(world: &'a WorldState, station: &'a StationId) -> &'a str {
    world
        .stations
        .get(station)
        .map_or(station.as_str(), |s| s.name.as_str())
}

/// The best units to buy here and sell at `dest`: `(units, cost, revenue)`.
fn best_haul(
    world: &WorldState,
    here: &StationId,
    dest: &StationId,
    ware: &WareId,
    max_units: u32,
) -> Option<(u32, i64, i64)> {
    let rules = &world.economy.rules.pricing;
    let buy_line = world.markets.get(here)?.lines.get(ware)?;
    let sell_line = world.markets.get(dest)?.lines.get(ware)?;
    if buy_line.role != MarketRole::Sells || sell_line.role != MarketRole::Buys {
        return None;
    }
    let (buys, _) = pricing::walk(
        buy_line,
        TradeSide::Buy,
        max_units,
        rules,
        spread_at(world, here),
    );
    let (sells, _) = pricing::walk(
        sell_line,
        TradeSide::Sell,
        buys.len() as u32,
        rules,
        spread_at(world, dest),
    );
    let mut best: Option<(u32, i64, i64)> = None;
    let (mut c, mut r) = (0_i64, 0_i64);
    for (i, (b, s)) in buys.iter().zip(&sells).enumerate() {
        c += i64::from(*b);
        r += i64::from(*s);
        if r - c > best.map_or(0, |(_, bc, br)| br - bc) {
            best = Some((i as u32 + 1, c, r));
        }
    }
    best
}

/// Every plan the trader is allowed, best first. The ship should be docked; if it
/// isn't, plans only choose where to take its cargo.
pub fn rank_trades(
    world: &WorldState,
    profile: &TraderProfile,
) -> Result<Vec<TradePlan>, TradeError> {
    let ship = world
        .ships
        .get(&profile.ship)
        .ok_or_else(|| TradeError::UnknownShip {
            ship: profile.ship.clone(),
        })?;
    let here = ship.docked_at.clone();
    let capacity = galaxy_contract::modifiers::hold_capacity(world, ship);
    let tradeable =
        |w: &WareId| profile.wares.contains(w) && world.wares.get(w).is_some_and(|w| w.tradeable);
    let wallet = (ship.owner == Owner::Player).then_some(world.player.credits);

    let mut dests: Vec<Option<&StationId>> = profile
        .stations
        .iter()
        .filter(|s| world.stations.contains(*s) && here.as_ref() != Some(*s))
        .map(Some)
        .collect();
    if dests.is_empty() {
        dests.push(None);
    }

    let mut plans = Vec::new();
    for dest in dests {
        // Cargo: sell here when that pays at least as well per unit as carrying it.
        let mut sells: Vec<(WareId, u32, i64)> = Vec::new();
        let mut carry: Vec<Carry> = Vec::new();
        for (ware, &have) in &ship.hold.cargo {
            if !tradeable(ware) {
                continue;
            }
            let q = have.min(profile.max_units_per_ware);
            let here_value = here.as_ref().and_then(|h| sell_value(world, h, ware, q));
            let there_all = dest
                .and_then(|d| sell_value(world, d, ware, have))
                .unwrap_or(0);
            let sell_here = here_value
                .is_some_and(|v| v as f64 / f64::from(q) >= there_all as f64 / f64::from(have));
            if let (true, Some(value)) = (sell_here, here_value) {
                sells.push((ware.clone(), q, value));
            }
        }
        sells.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        sells.truncate(profile.max_wares_per_docking as usize);
        let sold: BTreeSet<&WareId> = sells.iter().map(|s| &s.0).collect();
        for (ware, &have) in &ship.hold.cargo {
            let left = have - sells.iter().find(|s| s.0 == *ware).map_or(0, |s| s.1);
            if left == 0 {
                continue;
            }
            let value = dest
                .filter(|_| tradeable(ware))
                .and_then(|d| sell_value(world, d, ware, left))
                .unwrap_or(0);
            carry.push(Carry {
                ware: ware.clone(),
                units: left,
                expected_credits: value,
            });
        }
        let held_after: u64 = ship.hold.total() - sells.iter().map(|s| u64::from(s.1)).sum::<u64>();
        let free = u32::try_from(capacity.saturating_sub(held_after)).unwrap_or(u32::MAX);
        let sells_value: i64 = sells.iter().map(|s| s.2).sum();
        let carry_value: i64 = carry.iter().map(|c| c.expected_credits).sum();
        let travel = match (&here, dest) {
            (Some(h), Some(d)) => match (world.stations.get(h), world.stations.get(d)) {
                (Some(a), Some(b)) => a.position.distance(b.position),
                _ => 0.0,
            },
            (None, Some(d)) => world
                .stations
                .get(d)
                .map_or(0.0, |s| s.position.distance(ship.position)),
            _ => 0.0,
        };
        let sell_requests: Vec<TradeRequest> = here
            .as_ref()
            .map(|h| {
                sells
                    .iter()
                    .map(|(ware, q, _)| TradeRequest {
                        ship: ship.id.clone(),
                        station: h.clone(),
                        ware: ware.clone(),
                        side: TradeSide::Sell,
                        quantity: *q,
                        limit: None,
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mut push = |buy: Option<(WareId, u32, i64, i64)>| {
            let mut trades = sell_requests.clone();
            let mut carry = carry.clone();
            let mut expected = sells_value + carry_value;
            if let (Some((ware, units, cost, revenue)), Some(h)) = (&buy, &here) {
                trades.push(TradeRequest {
                    ship: ship.id.clone(),
                    station: h.clone(),
                    ware: ware.clone(),
                    side: TradeSide::Buy,
                    quantity: *units,
                    limit: None,
                });
                carry.push(Carry {
                    ware: ware.clone(),
                    units: *units,
                    expected_credits: *revenue,
                });
                expected += revenue - cost;
            }
            let score = expected as f64 - f64::from(travel) * profile.cost_per_distance;
            let mut parts: Vec<String> = sells
                .iter()
                .map(|(w, q, v)| {
                    format!("sell {q} {} here for {}", name(world, w), show_credits(*v))
                })
                .collect();
            if let Some((w, q, cost, _)) = &buy {
                parts.push(format!(
                    "buy {q} {} for {}",
                    name(world, w),
                    show_credits(*cost)
                ));
            }
            match dest {
                Some(d) => parts.push(format!("go to {}", station_name(world, d))),
                None => parts.push("stay".to_owned()),
            }
            let sign = if expected >= 0 { "+" } else { "" };
            parts.push(format!("expect {sign}{} credits", show_credits(expected)));
            plans.push(TradePlan {
                here: here.clone(),
                trades,
                destination: dest.cloned(),
                carry,
                expected_credits: expected,
                travel,
                score,
                summary: parts.join("; "),
            });
        };

        push(None);
        let wares_left = (profile.max_wares_per_docking as usize).saturating_sub(sells.len());
        if let (Some(h), Some(d), true) = (&here, dest, wares_left > 0 && free > 0)
            && let Some(market) = world.markets.get(h)
        {
            for (ware, _) in market.lines.iter().filter(|(w, _)| tradeable(w)) {
                if sold.contains(ware) || carry.iter().any(|c| c.ware == *ware) {
                    continue;
                }
                let max = profile.max_units_per_ware.min(free);
                if let Some((units, cost, revenue)) = best_haul(world, h, d, ware, max)
                    && wallet.is_none_or(|c| c >= cost)
                {
                    push(Some((ware.clone(), units, cost, revenue)));
                }
            }
        }
    }

    plans.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then_with(|| a.summary.cmp(&b.summary))
    });
    Ok(plans)
}

/// The best `n` plans: the menu the fast-model layer chooses from. The first is the
/// default.
pub fn top_choices(
    world: &WorldState,
    profile: &TraderProfile,
    n: usize,
) -> Result<Vec<TradePlan>, TradeError> {
    let mut plans = rank_trades(world, profile)?;
    plans.truncate(n);
    Ok(plans)
}
