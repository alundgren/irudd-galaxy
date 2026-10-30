//! Where things are: docking rings, lane mouths, rocks in reach and lane routes.
//! Plain reads of the world; iteration is in ID order, so ties break the same way
//! every run.

use std::collections::{BTreeMap, VecDeque};

use galaxy_contract::prelude::*;

/// The station whose docking ring contains `pos`, nearest first.
pub fn ring_at<'w>(world: &'w WorldState, region: &RegionId, pos: Vec2) -> Option<&'w Station> {
    ring_within(world, region, pos, 1.0)
}

/// [`ring_at`] with every ring's reach multiplied by `reach` (the `dock_reach`
/// modifier: Ringwise catches from 25% further out).
pub fn ring_within<'w>(
    world: &'w WorldState,
    region: &RegionId,
    pos: Vec2,
    reach: f64,
) -> Option<&'w Station> {
    let reach = (reach as f32).max(0.0);
    nearest(
        world.stations.values().filter(|s| {
            !s.services.is_empty()
                && &s.region == region
                && s.position.distance(pos) <= s.dock_radius * reach
        }),
        |s| s.position.distance(pos),
    )
}

/// The ring and lane mouth in reach of `ship` at `pos`, with its modifiers.
pub fn ring_for<'w>(world: &'w WorldState, ship: &Ship, pos: Vec2) -> Option<&'w Station> {
    let m = galaxy_contract::modifiers::for_ship(world, &ship.id);
    ring_within(world, &ship.region, pos, m.dock_reach)
}

/// The lane mouth `ship` at `pos` would ride, with its modifiers.
pub fn mouth_for(world: &WorldState, ship: &Ship, pos: Vec2) -> Option<LaneWay> {
    let m = galaxy_contract::modifiers::for_ship(world, &ship.id);
    mouth_within(world, &ship.region, pos, m.mouth_radius)
}

/// One way through a lane: in at `start` (this side's mouth), out at `end`.
#[derive(Clone, Debug, PartialEq)]
pub struct LaneWay {
    pub lane: LaneId,
    pub from: RegionId,
    pub to: RegionId,
    pub start: Vec2,
    pub end: Vec2,
}

/// The lane leaving `region` through `lane`, if it touches that region.
pub fn lane_way(lane: &Lane, region: &RegionId) -> Option<LaneWay> {
    let (to, start, end) = if &lane.a == region {
        (&lane.b, lane.a_gate, lane.b_gate)
    } else if &lane.b == region {
        (&lane.a, lane.b_gate, lane.a_gate)
    } else {
        return None;
    };
    Some(LaneWay {
        lane: lane.id.clone(),
        from: region.clone(),
        to: to.clone(),
        start,
        end,
    })
}

/// The lane mouth `pos` is inside, nearest first.
pub fn mouth_at(world: &WorldState, region: &RegionId, pos: Vec2) -> Option<LaneWay> {
    mouth_within(world, region, pos, None)
}

/// [`mouth_at`] with the catch radius widened to `radius` if that's larger (the
/// `mouth_radius` modifier: Lanerunner).
pub fn mouth_within(
    world: &WorldState,
    region: &RegionId,
    pos: Vec2,
    radius: Option<f32>,
) -> Option<LaneWay> {
    let base = world.ship_rules.lanes.mouth_radius;
    let radius = radius.map_or(base, |r| r.max(base));
    nearest(
        world
            .lanes
            .values()
            .filter_map(|l| lane_way(l, region))
            .filter(|w| w.start.distance(pos) <= radius),
        |w| w.start.distance(pos),
    )
}

/// The first lane on the shortest route (fewest lanes) from `from` to `to`.
pub fn route(world: &WorldState, from: &RegionId, to: &RegionId) -> Option<LaneWay> {
    if from == to {
        return None;
    }
    let mut first: BTreeMap<RegionId, LaneWay> = BTreeMap::new();
    let mut queue = VecDeque::from([from.clone()]);
    while let Some(region) = queue.pop_front() {
        for way in world.lanes.values().filter_map(|l| lane_way(l, &region)) {
            if &way.to == from || first.contains_key(&way.to) {
                continue;
            }
            let hop = if &region == from {
                way.clone()
            } else {
                first[&region].clone()
            };
            if &way.to == to {
                return Some(hop);
            }
            queue.push_back(way.to.clone());
            first.insert(way.to, hop);
        }
    }
    None
}

/// The region whose disc contains `pos`, nearest centre first.
pub fn region_at(world: &WorldState, pos: Vec2) -> Option<&RegionId> {
    nearest(
        world
            .regions
            .values()
            .filter(|r| r.center.distance(pos) <= r.radius),
        |r| r.center.distance(pos),
    )
    .map(|r| &r.id)
}

/// The region a rock is in (its field's).
pub fn rock_region<'w>(world: &'w WorldState, rock: &Rock) -> Option<&'w RegionId> {
    world.fields.get(&rock.field).map(|f| &f.region)
}

/// A rock's radius: it shrinks visibly as it is mined.
pub fn rock_radius(world: &WorldState, rock: &Rock) -> f32 {
    let m = &world.ship_rules.mining;
    m.radius_base + m.radius_per_sqrt_unit * (rock.remaining as f32).sqrt()
}

/// Whether a ship at `pos` can mine `rock`.
pub fn in_reach(world: &WorldState, rock: &Rock, pos: Vec2) -> bool {
    rock.position.distance(pos) - rock_radius(world, rock) <= world.ship_rules.mining.reach
}

/// The nearest unspent rock in mining reach.
pub fn rock_in_reach<'w>(world: &'w WorldState, region: &RegionId, pos: Vec2) -> Option<&'w Rock> {
    nearest(
        world.rocks.values().filter(|r| {
            r.remaining > 0 && rock_region(world, r) == Some(region) && in_reach(world, r, pos)
        }),
        |r| r.position.distance(pos),
    )
}

fn nearest<T>(items: impl Iterator<Item = T>, dist: impl Fn(&T) -> f32) -> Option<T> {
    let mut best: Option<(f32, T)> = None;
    for item in items {
        let d = dist(&item);
        if best.as_ref().is_none_or(|(b, _)| d < *b) {
            best = Some((d, item));
        }
    }
    best.map(|(_, t)| t)
}
