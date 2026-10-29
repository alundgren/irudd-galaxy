//! The map model: a read-only view of the world for the app to draw.
//!
//! Regions, lanes, stations with the price knowledge the player has, faction owners,
//! and territory. Territory follows station ownership, so when a station changes
//! hands (`SetStationOwner`) the region's control and the borders move with it.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::prelude::*;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::scout::{self, PriceKnowledge};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapView {
    pub tick: Tick,
    pub regions: Vec<MapRegion>,
    pub lanes: Vec<MapLane>,
    pub stations: Vec<MapStation>,
    pub factions: Vec<MapFaction>,
    /// Lanes whose two ends are held by different factions (or one end is contested).
    pub borders: Vec<MapBorder>,
    /// Where the player's flagship is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub player: Option<MapShip>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapRegion {
    pub id: RegionId,
    pub name: String,
    pub center: Vec2,
    pub radius: f32,
    pub control: Control,
    /// Activity so far, in pulses.
    pub pulses: f64,
}

/// Who holds a region.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Control {
    /// One faction owns more than half its stations (or the region itself, if it has
    /// no stations).
    Held {
        faction: FactionId,
    },
    /// Stations are split, or some are unowned.
    Contested {
        factions: Vec<FactionId>,
    },
    Unclaimed,
}

impl Control {
    pub fn holder(&self) -> Option<&FactionId> {
        match self {
            Control::Held { faction } => Some(faction),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapLane {
    pub id: LaneId,
    pub a: RegionId,
    pub b: RegionId,
    pub a_gate: Vec2,
    pub b_gate: Vec2,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapStation {
    pub id: StationId,
    pub name: String,
    pub region: RegionId,
    pub position: Vec2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<FactionId>,
    pub prices: PriceKnowledge,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapFaction {
    pub id: FactionId,
    pub name: String,
    /// Stations it owns.
    pub stations: Vec<StationId>,
    /// Regions it holds.
    pub regions: Vec<RegionId>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapBorder {
    pub lane: LaneId,
    pub a: RegionId,
    pub b: RegionId,
    pub a_control: Control,
    pub b_control: Control,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MapShip {
    pub ship: ShipId,
    pub region: RegionId,
    pub position: Vec2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docked_at: Option<StationId>,
}

/// Who holds `region`, from its stations' owners.
pub fn control_of(world: &WorldState, region: &RegionId) -> Control {
    let stations: Vec<&Station> = world
        .stations
        .values()
        .filter(|s| s.region == *region)
        .collect();
    if stations.is_empty() {
        return match world.regions.get(region).and_then(|r| r.owner.clone()) {
            Some(faction) => Control::Held { faction },
            None => Control::Unclaimed,
        };
    }
    let mut owned: BTreeMap<&FactionId, usize> = BTreeMap::new();
    for s in &stations {
        if let Some(f) = &s.owner {
            *owned.entry(f).or_default() += 1;
        }
    }
    if let Some((faction, _)) = owned.iter().find(|(_, n)| **n * 2 > stations.len()) {
        return Control::Held {
            faction: (*faction).clone(),
        };
    }
    if owned.is_empty() {
        Control::Unclaimed
    } else {
        Control::Contested {
            factions: owned.keys().map(|f| (*f).clone()).collect(),
        }
    }
}

impl MapView {
    /// The map as the player sees it now.
    pub fn build(world: &WorldState) -> Self {
        let live = scout::live_stations(world);
        let controls: BTreeMap<RegionId, Control> = world
            .regions
            .keys()
            .map(|r| (r.clone(), control_of(world, r)))
            .collect();

        let regions = world
            .regions
            .values()
            .map(|r| MapRegion {
                id: r.id.clone(),
                name: r.name.clone(),
                center: r.center,
                radius: r.radius,
                control: controls[&r.id].clone(),
                pulses: world
                    .economy
                    .regions
                    .get(&r.id)
                    .map_or(0.0, ActivityTally::pulses),
            })
            .collect();

        let lanes = world
            .lanes
            .values()
            .map(|l| MapLane {
                id: l.id.clone(),
                a: l.a.clone(),
                b: l.b.clone(),
                a_gate: l.a_gate,
                b_gate: l.b_gate,
            })
            .collect();

        let stations = world
            .stations
            .values()
            .map(|s| MapStation {
                id: s.id.clone(),
                name: s.name.clone(),
                region: s.region.clone(),
                position: s.position,
                owner: s.owner.clone(),
                prices: scout::knowledge(world, &s.id, &live),
            })
            .collect();

        let factions = world
            .factions
            .values()
            .map(|f| MapFaction {
                id: f.id.clone(),
                name: f.name.clone(),
                stations: world
                    .stations
                    .values()
                    .filter(|s| s.owner.as_ref() == Some(&f.id))
                    .map(|s| s.id.clone())
                    .collect(),
                regions: controls
                    .iter()
                    .filter(|(_, c)| c.holder() == Some(&f.id))
                    .map(|(r, _)| r.clone())
                    .collect(),
            })
            .collect();

        let borders = world
            .lanes
            .values()
            .filter_map(|l| {
                let a = controls.get(&l.a)?;
                let b = controls.get(&l.b)?;
                let differ = a.holder().is_none() || b.holder().is_none() || a != b;
                let both_unclaimed = *a == Control::Unclaimed && *b == Control::Unclaimed;
                (differ && !both_unclaimed).then(|| MapBorder {
                    lane: l.id.clone(),
                    a: l.a.clone(),
                    b: l.b.clone(),
                    a_control: a.clone(),
                    b_control: b.clone(),
                })
            })
            .collect();

        let player = world
            .player
            .ship
            .as_ref()
            .and_then(|id| world.ships.get(id))
            .map(|s| MapShip {
                ship: s.id.clone(),
                region: s.region.clone(),
                position: s.position,
                docked_at: s.docked_at.clone(),
            });

        MapView {
            tick: world.meta.tick,
            regions,
            lanes,
            stations,
            factions,
            borders,
            player,
        }
    }

    pub fn station(&self, id: &StationId) -> Option<&MapStation> {
        self.stations.iter().find(|s| s.id == *id)
    }

    pub fn region(&self, id: &RegionId) -> Option<&MapRegion> {
        self.regions.iter().find(|r| r.id == *id)
    }

    /// Stations whose prices the player has never seen.
    pub fn unknown_stations(&self) -> BTreeSet<StationId> {
        self.stations
            .iter()
            .filter(|s| s.prices == PriceKnowledge::Unknown)
            .map(|s| s.id.clone())
            .collect()
    }
}
