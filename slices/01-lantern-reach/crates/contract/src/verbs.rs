//! The order verbs, shared by the player ship, the automated craft and NPC craft.
//!
//! Control tiers only change how the player *produces* orders (WASD, click, queue,
//! loop); they never change what an order does. W2 (ship) executes them.
//!
//! There is no duration anywhere: `wait` waits for a world condition, never for time.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::events::{CounterKind, CounterScope};
use crate::geom::Vec2;
use crate::ids::*;

/// Verb names, used in causes and `order_done` events.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum VerbKind {
    Fly,
    Mine,
    Dock,
    Undock,
    Buy,
    Sell,
    Wait,
    Loop,
}

/// One order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "verb", rename_all = "snake_case")]
pub enum Order {
    Fly {
        to: FlyTarget,
    },
    /// Mine until the hold is full or the rock is depleted.
    Mine {
        rock: RockId,
    },
    Dock {
        station: StationId,
    },
    Undock {},
    Buy {
        ware: WareId,
        quantity: Quantity,
    },
    Sell {
        ware: WareId,
        quantity: Quantity,
    },
    /// Wait until a world condition holds (e.g. a price recovers above a floor).
    Wait {
        until: Condition,
    },
    /// Repeat `orders` until `until` holds, or forever.
    Loop {
        orders: Vec<Order>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        until: Option<Condition>,
    },
}

impl Order {
    pub fn verb(&self) -> VerbKind {
        match self {
            Order::Fly { .. } => VerbKind::Fly,
            Order::Mine { .. } => VerbKind::Mine,
            Order::Dock { .. } => VerbKind::Dock,
            Order::Undock {} => VerbKind::Undock,
            Order::Buy { .. } => VerbKind::Buy,
            Order::Sell { .. } => VerbKind::Sell,
            Order::Wait { .. } => VerbKind::Wait,
            Order::Loop { .. } => VerbKind::Loop,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FlyTarget {
    Point {
        at: Vec2,
    },
    Station {
        station: StationId,
    },
    Rock {
        rock: RockId,
    },
    /// Travel to another region along lanes.
    Region {
        region: RegionId,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Quantity {
    Units {
        n: u32,
    },
    /// Everything in the hold (sell) or everything affordable that fits (buy).
    All,
}

/// A world condition. Evaluated by the engine against world state only.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Condition {
    HoldFull,
    HoldEmpty,
    PriceAtLeast {
        station: StationId,
        ware: WareId,
        price: u32,
    },
    PriceAtMost {
        station: StationId,
        ware: WareId,
        price: u32,
    },
    StockAtLeast {
        station: StationId,
        ware: WareId,
        stock: u32,
    },
    CounterAtLeast {
        counter: CounterKind,
        scope: CounterScope,
        value: u64,
    },
    FlagSet {
        flag: FlagId,
    },
    All {
        of: Vec<Condition>,
    },
    Any {
        of: Vec<Condition>,
    },
    Not {
        cond: Box<Condition>,
    },
}

/// A ship's orders and where it is in them. W2 may add progress fields.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderQueue {
    pub orders: Vec<Order>,
    /// Index of the order being executed.
    #[serde(default)]
    pub current: usize,
}
