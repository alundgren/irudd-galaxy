//! Economy telemetry: per-region and per-station metrics the game master and the
//! sandbox validator read to spot runaway prices, dead markets and floods.

use galaxy_contract::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::pricing::{self, spread_at};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EconomyTelemetry {
    pub tick: Tick,
    pub regions: Vec<RegionTelemetry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RegionTelemetry {
    pub region: RegionId,
    pub name: String,
    pub activity: ActivityMetrics,
    /// Unit price over base, across every line in the region (1.0 = at base).
    pub price_ratio_mean: f64,
    pub price_ratio_min: f64,
    pub price_ratio_max: f64,
    /// Over the `buys` lines.
    pub saturation_mean: f64,
    pub saturation_max: f64,
    pub stock_total: u64,
    /// Lines pinned at their floor or ceiling.
    pub pinned_lines: u32,
    pub stations: Vec<StationTelemetry>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ActivityMetrics {
    pub pulses: f64,
    pub trades: i64,
    pub units_bought: i64,
    pub units_sold: i64,
    pub credits_spent: i64,
    pub credits_earned: i64,
}

impl From<&ActivityTally> for ActivityMetrics {
    fn from(t: &ActivityTally) -> Self {
        Self {
            pulses: t.pulses(),
            trades: t.trades,
            units_bought: t.units_bought,
            units_sold: t.units_sold,
            credits_spent: t.credits_spent,
            credits_earned: t.credits_earned,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct StationTelemetry {
    pub station: StationId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FactionId>,
    pub activity: ActivityMetrics,
    pub lines: Vec<LineTelemetry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct LineTelemetry {
    pub ware: WareId,
    pub role: MarketRole,
    pub stock: u32,
    pub target: u32,
    pub saturation: f64,
    /// The next unit's price.
    pub unit_price: f64,
    pub base_price: u32,
    pub price_ratio: f64,
    /// Within half a credit of the lowest price the formula allows.
    pub at_floor: bool,
    /// Within half a credit of the highest.
    pub at_ceiling: bool,
    /// Effective units per pulse.
    pub production: f64,
    pub consumption: f64,
}

impl EconomyTelemetry {
    pub fn build(world: &WorldState) -> Self {
        let rules = &world.economy.rules.pricing;
        let regions = world
            .regions
            .values()
            .map(|r| {
                let stations: Vec<StationTelemetry> = world
                    .stations
                    .values()
                    .filter(|s| s.region == r.id)
                    .map(|s| {
                        let spread = spread_at(world, &s.id);
                        let lines = world
                            .markets
                            .get(&s.id)
                            .map(|m| {
                                m.lines
                                    .iter()
                                    .map(|(ware, l)| line_telemetry(ware, l, rules, spread))
                                    .collect()
                            })
                            .unwrap_or_default();
                        StationTelemetry {
                            station: s.id.clone(),
                            name: s.name.clone(),
                            owner: s.owner.clone(),
                            activity: world
                                .economy
                                .stations
                                .get(&s.id)
                                .map(ActivityMetrics::from)
                                .unwrap_or_default(),
                            lines,
                        }
                    })
                    .collect();
                region_telemetry(world, r, stations)
            })
            .collect();
        Self {
            tick: world.meta.tick,
            regions,
        }
    }

    /// Lines pinned at a floor or ceiling, as `(station, ware)`.
    pub fn pinned(&self) -> Vec<(&StationId, &WareId)> {
        self.regions
            .iter()
            .flat_map(|r| &r.stations)
            .flat_map(|s| {
                s.lines
                    .iter()
                    .filter(|l| l.at_floor || l.at_ceiling)
                    .map(move |l| (&s.station, &l.ware))
            })
            .collect()
    }

    pub fn region(&self, id: &RegionId) -> Option<&RegionTelemetry> {
        self.regions.iter().find(|r| r.region == *id)
    }
}

fn line_telemetry(
    ware: &WareId,
    l: &MarketLine,
    rules: &galaxy_contract::economy::PricingRules,
    spread: f64,
) -> LineTelemetry {
    let unit_price = pricing::unit_price(l, rules, spread);
    let (lo, hi) = pricing::formula_range(l, rules);
    LineTelemetry {
        ware: ware.clone(),
        role: l.role,
        stock: l.stock,
        target: l.target,
        saturation: l.saturation,
        unit_price,
        base_price: l.base_price,
        price_ratio: if l.base_price == 0 {
            1.0
        } else {
            unit_price / f64::from(l.base_price)
        },
        at_floor: unit_price <= lo + 0.5,
        at_ceiling: unit_price >= hi - 0.5,
        production: f64::from(l.production) * l.production_scale,
        consumption: f64::from(l.consumption) * l.consumption_scale,
    }
}

fn region_telemetry(
    world: &WorldState,
    r: &Region,
    stations: Vec<StationTelemetry>,
) -> RegionTelemetry {
    let lines: Vec<&LineTelemetry> = stations.iter().flat_map(|s| &s.lines).collect();
    let ratios: Vec<f64> = lines.iter().map(|l| l.price_ratio).collect();
    let sats: Vec<f64> = lines
        .iter()
        .filter(|l| l.role == MarketRole::Buys)
        .map(|l| l.saturation)
        .collect();
    let mean = |v: &[f64]| {
        if v.is_empty() {
            0.0
        } else {
            v.iter().sum::<f64>() / v.len() as f64
        }
    };
    RegionTelemetry {
        region: r.id.clone(),
        name: r.name.clone(),
        activity: world
            .economy
            .regions
            .get(&r.id)
            .map(ActivityMetrics::from)
            .unwrap_or_default(),
        price_ratio_mean: mean(&ratios),
        price_ratio_min: ratios.iter().copied().reduce(f64::min).unwrap_or(1.0),
        price_ratio_max: ratios.iter().copied().reduce(f64::max).unwrap_or(1.0),
        saturation_mean: mean(&sats),
        saturation_max: sats.iter().copied().reduce(f64::max).unwrap_or(0.0),
        stock_total: lines.iter().map(|l| u64::from(l.stock)).sum(),
        pinned_lines: lines.iter().filter(|l| l.at_floor || l.at_ceiling).count() as u32,
        stations,
    }
}
