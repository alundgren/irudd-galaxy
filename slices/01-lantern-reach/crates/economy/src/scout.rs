//! Scouting: what the player knows about prices away from where they are.
//!
//! Remote prices are unknown until visited. Docking writes that station's snapshot,
//! dated by the tick and the player's docking count, so its age reads in activity
//! ("seen 3 dockings ago"), never in time, and nothing expires. Price reach widens
//! what is live: a hull's [`PriceReach`] (the Heron sees its whole region) and
//! reach flags from fitted upgrades (the price glass).

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::{self, spread_at};

/// The player's docking count: the clock snapshots are dated by.
pub fn player_dockings(world: &WorldState) -> u64 {
    world
        .counters
        .get(CounterKind::Dockings, &CounterScope::Player)
}

/// The current price of every ware at a station: what it pays or asks per unit,
/// rounded to whole credits.
pub fn current_prices(world: &WorldState, station: &StationId) -> BTreeMap<WareId, u32> {
    let Some(market) = world.markets.get(station) else {
        return BTreeMap::new();
    };
    let rules = &world.economy.rules.pricing;
    let spread = spread_at(world, station);
    market
        .lines
        .iter()
        .filter(|(w, _)| world.wares.get(*w).is_none_or(|w| w.tradeable))
        .map(|(w, l)| (w.clone(), pricing::priced(l, rules, spread).price))
        .collect()
}

/// A snapshot of a station's market as it is now. `None` if it has no market.
pub fn snapshot(world: &WorldState, station: &StationId) -> Option<PriceSnapshot> {
    world.markets.get(station)?;
    Some(PriceSnapshot {
        tick: world.meta.tick,
        visit: player_dockings(world),
        prices: current_prices(world, station),
    })
}

/// The commands that scout `station` for the player: store its snapshot and emit
/// `scout` (which bumps the scouting counter and becomes a history fact).
pub fn scout_commands(world: &WorldState, ship: &ShipId, station: &StationId) -> Vec<Command> {
    let Some(snapshot) = snapshot(world, station) else {
        return Vec::new();
    };
    vec![
        Command::SetPriceSnapshot {
            station: station.clone(),
            snapshot: Some(snapshot),
        },
        Command::Emit {
            event: Event::Scout {
                ship: ship.clone(),
                station: station.clone(),
            },
        },
    ]
}

/// How many player dockings ago a snapshot was taken.
pub fn dockings_ago(world: &WorldState, snapshot: &PriceSnapshot) -> u64 {
    player_dockings(world).saturating_sub(snapshot.visit)
}

/// A snapshot's age in words, by activity: "as of your last visit", "seen 1 docking
/// ago", "seen 6 dockings ago".
pub fn describe_age(dockings_ago: u64) -> String {
    match dockings_ago {
        0 => "as of your last visit".to_owned(),
        1 => "seen 1 docking ago".to_owned(),
        n => format!("seen {n} dockings ago"),
    }
}

/// How far the player's flagship sees live prices right now.
pub fn reach_of(world: &WorldState, ship: &Ship) -> PriceReach {
    let mut reach = world
        .hulls
        .get(&ship.hull)
        .map(|h| h.price_reach.clone())
        .unwrap_or_default();
    for (flag, radius) in &world.economy.rules.reach.flags {
        if world.player.flags.contains(flag) {
            reach.radius = reach.radius.max(*radius);
        }
    }
    reach
}

/// Stations whose prices the player sees live: where the flagship is docked, plus its
/// reach.
pub fn live_stations(world: &WorldState) -> BTreeSet<StationId> {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return BTreeSet::new();
    };
    let reach = reach_of(world, ship);
    world
        .stations
        .values()
        .filter(|s| {
            ship.docked_at.as_ref() == Some(&s.id)
                || (reach.region && s.region == ship.region)
                || (reach.radius > 0.0 && s.position.distance(ship.position) <= reach.radius)
        })
        .map(|s| s.id.clone())
        .collect()
}

/// One ware's price as the player knows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct KnownPrice {
    pub ware: WareId,
    /// `buys`: the station pays this; `sells`: it asks this.
    pub role: MarketRole,
    pub price: u32,
}

/// What the player knows about a station's prices.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PriceKnowledge {
    /// The station has no market.
    NoMarket,
    /// Never visited (or out of reach and never scouted): "?" for every ware.
    Unknown,
    /// Seen right now: docked here or within reach.
    Live { prices: Vec<KnownPrice> },
    /// The last snapshot, dated by activity.
    Seen {
        prices: Vec<KnownPrice>,
        tick: Tick,
        visit: u64,
        dockings_ago: u64,
        /// E.g. "seen 3 dockings ago".
        label: String,
    },
}

fn known(
    world: &WorldState,
    station: &StationId,
    prices: &BTreeMap<WareId, u32>,
) -> Vec<KnownPrice> {
    let lines = world.markets.get(station).map(|m| &m.lines);
    prices
        .iter()
        .map(|(ware, price)| KnownPrice {
            ware: ware.clone(),
            role: lines
                .and_then(|l| l.get(ware))
                .map_or(MarketRole::Buys, |l| l.role),
            price: *price,
        })
        .collect()
}

/// What the player knows about `station`, given the live set from [`live_stations`].
pub fn knowledge(
    world: &WorldState,
    station: &StationId,
    live: &BTreeSet<StationId>,
) -> PriceKnowledge {
    if !world.markets.contains(station) {
        return PriceKnowledge::NoMarket;
    }
    if live.contains(station) {
        return PriceKnowledge::Live {
            prices: known(world, station, &current_prices(world, station)),
        };
    }
    match world.player.price_snapshots.get(station) {
        None => PriceKnowledge::Unknown,
        Some(snap) => {
            let ago = dockings_ago(world, snap);
            PriceKnowledge::Seen {
                prices: known(world, station, &snap.prices),
                tick: snap.tick,
                visit: snap.visit,
                dockings_ago: ago,
                label: describe_age(ago),
            }
        }
    }
}
