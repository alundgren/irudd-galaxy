//! Activity pulses: how markets move without a clock.
//!
//! A **pulse** is a unit of activity at a place. A counted docking makes a station
//! pulse there, a smaller one at the other stations in the region and a region pulse;
//! a lane arrival makes a pulse at every station in the region it arrives in. All the
//! weights are [`PulseRules`] data.
//!
//! On a station pulse of weight `w`, for each market line: producers add
//! `production * production_scale * w` to stock (while every ware they need is in
//! stock, and up to `target * produce_cap`); consumers remove
//! `consumption * consumption_scale * w` (down to zero); a `buys` line's saturation
//! falls by `sat_recover * w` (down to zero). Nothing else ever lowers saturation.

use std::collections::BTreeMap;

use galaxy_contract::economy::{PricingRules, PulseRules};
use galaxy_contract::prelude::*;

/// Pulse weight in thousandths, as tallies store it.
pub fn milli(weight: f64) -> i64 {
    (weight * 1000.0).round() as i64
}

/// Pulses gathered over one tick, in thousandths.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Pulses {
    pub stations: BTreeMap<StationId, i64>,
    pub regions: BTreeMap<RegionId, i64>,
}

impl Pulses {
    fn station(&mut self, station: &StationId, weight: f64) {
        let m = milli(weight);
        if m != 0 {
            *self.stations.entry(station.clone()).or_default() += m;
        }
    }

    fn region(&mut self, region: &RegionId, weight: f64) {
        let m = milli(weight);
        if m != 0 {
            *self.regions.entry(region.clone()).or_default() += m;
        }
    }

    /// A counted docking at `station`.
    pub fn docking(&mut self, world: &WorldState, station: &StationId, rules: &PulseRules) {
        let Some(region) = world.stations.get(station).map(|s| &s.region) else {
            return;
        };
        self.station(station, rules.dock_station);
        for other in stations_in(world, region) {
            if other != station {
                self.station(other, rules.dock_neighbours);
            }
        }
        self.region(region, rules.dock_region);
    }

    /// A ship arriving in `region` through a lane.
    pub fn arrival(&mut self, world: &WorldState, region: &RegionId, rules: &PulseRules) {
        if !world.regions.contains(region) {
            return;
        }
        for station in stations_in(world, region) {
            self.station(station, rules.arrival_stations);
        }
        self.region(region, rules.arrival_region);
    }

    pub fn is_empty(&self) -> bool {
        self.stations.is_empty() && self.regions.is_empty()
    }
}

/// Stations in a region, in ID order.
pub fn stations_in<'a>(
    world: &'a WorldState,
    region: &'a RegionId,
) -> impl Iterator<Item = &'a StationId> + 'a {
    world
        .stations
        .values()
        .filter(move |s| s.region == *region)
        .map(|s| &s.id)
}

/// Would `ship` docking at `station` now count? It does unless the ship last docked
/// there and hasn't since gone `recount_distance` away, mined, or docked elsewhere.
pub fn is_counted(track: Option<&DockTrack>, station: &StationId) -> bool {
    track.is_none_or(|t| t.station != *station || t.away)
}

/// One line after a station pulse of `weight`. `needs_met` says whether every ware
/// its producers need is in stock.
pub fn pulse_line(
    line: &MarketLine,
    weight: f64,
    needs_met: bool,
    rules: &PricingRules,
) -> MarketLine {
    let mut next = line.clone();
    let before = f64::from(line.stock) + line.stock_carry;
    let mut exact = before;
    if line.production > 0 && needs_met {
        let made = f64::from(line.production) * line.production_scale * weight;
        let cap = if rules.produce_cap > 0.0 && line.target > 0 {
            (f64::from(line.target) * rules.produce_cap).max(before)
        } else {
            f64::INFINITY
        };
        exact = (exact + made).min(cap);
    }
    if line.consumption > 0 {
        exact -= f64::from(line.consumption) * line.consumption_scale * weight;
    }
    let exact = exact.clamp(0.0, f64::from(u32::MAX));
    let whole = exact.floor();
    next.stock = whole as u32;
    next.stock_carry = exact - whole;
    if line.role == MarketRole::Buys && line.sat_recover > 0.0 {
        next.saturation = (line.saturation - line.sat_recover * weight).max(0.0);
    }
    next
}

/// Every line of a market after a station pulse of `weight`. Needs are checked
/// against stock before the pulse.
pub fn pulse_market(
    market: &Market,
    weight: f64,
    rules: &PricingRules,
) -> BTreeMap<WareId, MarketLine> {
    market
        .lines
        .iter()
        .map(|(ware, line)| {
            let needs_met = line
                .needs
                .iter()
                .all(|n| market.lines.get(n).is_some_and(|l| l.stock > 0));
            (ware.clone(), pulse_line(line, weight, needs_met, rules))
        })
        .collect()
}
