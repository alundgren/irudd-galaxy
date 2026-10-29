//! Ship-side shared types (W2): the flight, mining, lane and control tunables, and the
//! per-ship and per-rock runtime state that has to survive a save.
//!
//! The maths that uses them lives in `galaxy-ship`. Every tunable here is data: the
//! core pack's `ship_rules.toml` sets them, and the defaults only fill gaps so older
//! saves and partial files still load.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::CounterKind;
use crate::geom::Vec2;
use crate::ids::*;

/// Tunables for ships, mining, lanes and control tiers. Saved with the world, so a
/// save keeps the rules it was played with until something changes them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ShipRules {
    pub flight: FlightRules,
    pub mining: MiningRules,
    pub docking: DockingRules,
    pub lanes: LaneRules,
    pub regrowth: RegrowthRules,
    pub controls: ControlRules,
}

/// Arcade flight feel. Ships stop when nothing pushes them: there is no coasting.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct FlightRules {
    /// Seconds from rest to top speed, for hulls without their own `accel`.
    pub accel_time: f32,
    /// Braking is this many times stronger than accelerating, so release stops fast.
    pub brake: f32,
    /// Degrees per second, for hulls without their own `turn_rate`.
    pub turn_rate: f32,
    /// Share of top speed lost while moving directly away from the heading (0..=1).
    /// The ship swings round to face where it goes, and turn rate shapes the feel.
    pub turn_slowdown: f32,
    /// A fly order is done within this many world units of its point.
    pub arrive_distance: f32,
}

impl Default for FlightRules {
    fn default() -> Self {
        Self {
            accel_time: 0.1,
            brake: 1.5,
            turn_rate: 270.0,
            turn_slowdown: 0.5,
            arrive_distance: 2.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct MiningRules {
    /// A ship mines while within this distance of a rock's edge.
    pub reach: f32,
    /// A rock's radius is `radius_base + radius_per_sqrt_unit * sqrt(remaining)`.
    pub radius_base: f32,
    pub radius_per_sqrt_unit: f32,
}

impl Default for MiningRules {
    fn default() -> Self {
        Self {
            reach: 40.0,
            radius_base: 6.0,
            radius_per_sqrt_unit: 3.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct DockingRules {
    /// A dock order steers until the ship is this far into the ring (share of its
    /// radius from the centre), then docks.
    pub approach: f32,
}

impl Default for DockingRules {
    fn default() -> Self {
        Self { approach: 0.6 }
    }
}

/// Lanes are currents: every hull rides them at the same speed and can't steer.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct LaneRules {
    /// World units per second inside a lane.
    pub speed: f32,
    /// A ship within this distance of a lane mouth can enter the lane.
    pub mouth_radius: f32,
}

impl Default for LaneRules {
    fn default() -> Self {
        Self {
            speed: 500.0,
            mouth_radius: 70.0,
        }
    }
}

/// How rocks regrow. Driven by activity counters in the field's region, never by time.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct RegrowthRules {
    /// Region counters that make up a region's activity ("region pulses"), with weights.
    pub pulses: Vec<PulseWeight>,
    /// A spent rock reforms at this share of its full yield.
    pub reform_share: f32,
}

impl Default for RegrowthRules {
    fn default() -> Self {
        Self {
            pulses: vec![
                PulseWeight {
                    counter: CounterKind::Dockings,
                    weight: 1,
                },
                PulseWeight {
                    counter: CounterKind::Jumps,
                    weight: 1,
                },
            ],
            reform_share: 0.25,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct PulseWeight {
    pub counter: CounterKind,
    pub weight: u64,
}

/// Control tiers as progression. Each input mode is unlocked by a per-save flag
/// (`Player::flags`); the flag names are data.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct ControlRules {
    /// Click a point near the ship: fly there and stop.
    pub click_fly: FlagId,
    /// Click a rock: fly into reach and mine.
    pub click_mine: FlagId,
    /// Click a station: fly into the ring and dock.
    pub click_dock: FlagId,
    /// Click anything on the map, any distance, routing through lanes.
    pub map_travel: FlagId,
    /// Shift-click appends orders to a queue.
    pub queues: FlagId,
    /// Save a queue as a named loop and run it.
    pub loops: FlagId,
    /// The player's automated craft may run saved loops.
    pub automation: FlagId,
    /// How far from the ship a click-to-fly point may be (map travel has no limit).
    pub click_fly_range: f32,
    /// Most orders in a queue.
    pub queue_limit: u32,
    /// Most orders in a saved loop.
    pub loop_limit: u32,
    /// Most saved loops.
    pub saved_loop_limit: u32,
}

impl Default for ControlRules {
    fn default() -> Self {
        Self {
            click_fly: FlagId::from_static("cap.click_fly"),
            click_mine: FlagId::from_static("cap.click_mine"),
            click_dock: FlagId::from_static("cap.click_dock"),
            map_travel: FlagId::from_static("cap.map_travel"),
            queues: FlagId::from_static("cap.queues"),
            loops: FlagId::from_static("cap.loops"),
            automation: FlagId::from_static("cap.automation"),
            click_fly_range: 450.0,
            queue_limit: 6,
            loop_limit: 8,
            saved_loop_limit: 3,
        }
    }
}

/// What a ship is in the middle of, beyond its motion and orders.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct ShipActivity {
    /// The context action was held last tick. A press docks, undocks or enters a lane
    /// once; holding keeps mining.
    #[serde(default)]
    pub action_held: bool,
    /// Riding a lane current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transit: Option<Transit>,
    /// The mining run under way: one `mine` event when it ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mining: Option<MiningRun>,
}

/// A lane transit: straight from `start` to `end` at the lane speed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Transit {
    pub lane: LaneId,
    pub from: RegionId,
    pub to: RegionId,
    pub start: Vec2,
    pub end: Vec2,
    /// World units travelled so far.
    pub travelled: f32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct MiningRun {
    pub rock: RockId,
    /// Fractional units cut but not yet in the hold.
    pub progress: f32,
    /// Units moved into the hold so far in this run.
    pub mined: u32,
}

/// A rock's regrowth bookkeeping, in region activity (pulses), never time.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct RockGrowth {
    /// The region activity this rock's regrowth has accounted for.
    #[serde(default)]
    pub mark: u64,
    /// The region activity when it was spent; it reforms some pulses later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spent_at: Option<u64>,
}

/// Where a ship is inside its current order, beyond `OrderQueue::current`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct OrderProgress {
    /// The order being run inside a `loop` order.
    #[serde(default)]
    pub step: usize,
    /// Laps of the current loop completed.
    #[serde(default)]
    pub laps: u64,
    /// This lap moved cargo or credits. A lap that moves nothing stalls the loop.
    #[serde(default)]
    pub lap_moved: bool,
    /// Units the current buy or sell has moved so far.
    #[serde(default)]
    pub traded: u32,
    /// A patient sell is waiting in a standing berth for its floor.
    #[serde(default)]
    pub waiting: bool,
}
