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
use crate::ship::OrderProgress;

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
    /// Hand cargo to one of the player's tenders (the Tandem's Coupling).
    Transfer,
    Tag,
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
    Tag {
        rock: RockId,
    },
    MineField {
        field: FieldId,
        #[serde(default)]
        prefer_tagged: bool,
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
        /// Sell only while the unit price, in centicredits, is at or above this. A crewed
        /// ship then keeps the rest; a hull with a standing berth waits docked for the
        /// price (patient sell).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        floor: Option<u32>,
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
    /// Hand cargo to one of the player's tenders, both docked at the same station or
    /// within `docking.transfer_reach` in space. Only a hull with `coupling` (the
    /// Tandem) can. Writes a `transfer` fact.
    Transfer {
        ware: WareId,
        quantity: Quantity,
        to: ShipId,
    },
}

impl Order {
    pub fn verb(&self) -> VerbKind {
        match self {
            Order::Tag { .. } => VerbKind::Tag,
            Order::MineField { .. } => VerbKind::Mine,
            Order::Fly { .. } => VerbKind::Fly,
            Order::Mine { .. } => VerbKind::Mine,
            Order::Dock { .. } => VerbKind::Dock,
            Order::Undock {} => VerbKind::Undock,
            Order::Buy { .. } => VerbKind::Buy,
            Order::Sell { .. } => VerbKind::Sell,
            Order::Wait { .. } => VerbKind::Wait,
            Order::Loop { .. } => VerbKind::Loop,
            Order::Transfer { .. } => VerbKind::Transfer,
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
    /// The line's current price, in centicredits, is at least `price`.
    PriceAtLeast {
        station: StationId,
        ware: WareId,
        price: u32,
    },
    /// The line's current price, in centicredits, is at most `price`.
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

/// A ship's orders and where it is in them.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OrderQueue {
    pub orders: Vec<Order>,
    /// Index of the order being executed.
    #[serde(default)]
    pub current: usize,
    /// Progress inside the current order (loop step, laps, patient sell).
    #[serde(default)]
    pub progress: OrderProgress,
}
