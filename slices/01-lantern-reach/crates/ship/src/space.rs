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

/// A rock's radius as the game draws it: it shrinks visibly as it is mined, from its
/// full size to `spent_scale` of it. Clicks, mining reach and mine orders all measure
/// from it.
pub fn rock_radius(world: &WorldState, rock: &Rock) -> f32 {
    let m = &world.ship_rules.mining;
    let full = m.radius_base + m.radius_per_sqrt_unit * (rock.initial as f32).sqrt();
    let share = (rock.remaining as f32 / rock.initial.max(1) as f32).clamp(0.0, 1.0);
    let spent = m.spent_scale.clamp(0.0, 1.0);
    full * (spent + (1.0 - spent) * share.sqrt())
}

/// How far from a rock's drawn edge `ship` mines: the rules' reach plus its
/// `mining_reach` modifiers (the long-throw drill).
pub fn mining_reach(world: &WorldState, ship: &ShipId) -> f32 {
    reach_with(world, &galaxy_contract::modifiers::for_ship(world, ship))
}

/// Mining reach with these modifiers.
pub fn reach_with(world: &WorldState, m: &galaxy_contract::modifiers::Modifiers) -> f32 {
    world.ship_rules.mining.reach + m.mining_reach.max(0.0)
}

/// Whether a ship at `pos` can mine `rock` with the rules' reach (no reach
/// modifiers: bots and tests). [`in_reach_of`] adds a ship's own.
pub fn in_reach(world: &WorldState, rock: &Rock, pos: Vec2) -> bool {
    within_reach(world, rock, pos, world.ship_rules.mining.reach)
}

/// Whether `ship` at `pos` can mine `rock`.
pub fn in_reach_of(world: &WorldState, ship: &ShipId, rock: &Rock, pos: Vec2) -> bool {
    within_reach(world, rock, pos, mining_reach(world, ship))
}

/// Whether `rock`'s drawn edge is within `reach` of `pos`.
pub fn within_reach(world: &WorldState, rock: &Rock, pos: Vec2, reach: f32) -> bool {
    rock.position.distance(pos) - rock_radius(world, rock) <= reach
}

/// The nearest unspent rock in the rules' mining reach, for a ship with no reach
/// modifiers (bots and tests). [`rock_in_reach_of`] adds a ship's own.
pub fn rock_in_reach<'w>(world: &'w WorldState, region: &RegionId, pos: Vec2) -> Option<&'w Rock> {
    nearest_within(world, region, pos, world.ship_rules.mining.reach)
}

/// The nearest unspent rock `ship` at `pos` can mine.
pub fn rock_in_reach_of<'w>(
    world: &'w WorldState,
    ship: &ShipId,
    region: &RegionId,
    pos: Vec2,
) -> Option<&'w Rock> {
    nearest_within(world, region, pos, mining_reach(world, ship))
}

/// The nearest unspent rock in `region` whose edge is within `reach` of `pos`.
pub fn nearest_within<'w>(
    world: &'w WorldState,
    region: &RegionId,
    pos: Vec2,
    reach: f32,
) -> Option<&'w Rock> {
    nearest(
        world.rocks.values().filter(|r| {
            r.remaining > 0
                && rock_region(world, r) == Some(region)
                && within_reach(world, r, pos, reach)
        }),
        |r| r.position.distance(pos),
    )
}

/// What a press of Space does for `ship` at `pos`, besides braking.
#[derive(Clone, Debug, PartialEq)]
pub enum Press<'w> {
    /// Dock: the ship is in a station's docking ring.
    Dock(&'w Station),
    /// Ride the lane current: the ship is in a lane mouth.
    Ride(LaneWay),
    /// Start mining: an unspent rock is in reach (holding Space keeps it going).
    Mine(&'w Rock),
}

/// What a press of Space does for `ship` at `pos`, in that order: dock, ride a lane,
/// start mining. `None`: held Space only brakes. The sim and the hint both read this,
/// so the hint always says what Space really does.
pub fn press_at<'w>(world: &'w WorldState, ship: &Ship, pos: Vec2) -> Option<Press<'w>> {
    let m = galaxy_contract::modifiers::for_ship(world, &ship.id);
    if let Some(station) = ring_within(world, &ship.region, pos, m.dock_reach) {
        return Some(Press::Dock(station));
    }
    if let Some(way) = mouth_within(world, &ship.region, pos, m.mouth_radius) {
        return Some(Press::Ride(way));
    }
    nearest_within(world, &ship.region, pos, reach_with(world, &m)).map(Press::Mine)
}

/// How far `pos` is past the edge of `region` (the drawn region circle), if it is.
pub fn past_edge(world: &WorldState, region: &RegionId, pos: Vec2) -> Option<f32> {
    let r = world.regions.get(region)?;
    let depth = r.center.distance(pos) - r.radius;
    (depth > 0.0).then_some(depth)
}

/// `pos`, or the nearest point on the edge of `region` if it is past it: where a fly
/// order can actually stop, since the edge current carries ships back in.
pub fn within_edge(world: &WorldState, region: &RegionId, pos: Vec2) -> Vec2 {
    match (world.regions.get(region), past_edge(world, region, pos)) {
        (Some(r), Some(_)) => r.center + (pos - r.center).normalize_or_zero() * r.radius,
        _ => pos,
    }
}

/// Where the edge current carries a ship at `pos` in `dt` seconds: straight back
/// toward its region's centre, faster the further out it is, never past the edge.
/// Inside the edge it does nothing.
pub fn edge_drift(world: &WorldState, region: &RegionId, pos: Vec2, dt: f32) -> Vec2 {
    let Some(r) = world.regions.get(region) else {
        return pos;
    };
    let Some(depth) = past_edge(world, region, pos) else {
        return pos;
    };
    let rules = &world.ship_rules.edge;
    let speed = (rules.pull.max(0.0) * depth).max(rules.min_speed.max(0.0));
    let step = (speed * dt).min(depth);
    pos - (pos - r.center).normalize_or_zero() * step
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
