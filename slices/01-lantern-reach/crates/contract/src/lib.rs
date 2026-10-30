//! `galaxy-contract`: the source of truth for the slice's shared interfaces.
//!
//! Everything other crates and packs agree on lives here as plain serde + schemars
//! types: stable IDs, the world-state schema, events and activity counters, the order
//! verbs, per-tick player input, the command write path with causes and undo records,
//! history facts, the authored data files, and placeholders for the hook API, pack
//! manifests and validation reports, the asset manifest and the talk channel.
//!
//! `cargo xtask contract` regenerates the JSON Schemas in `crates/contract/schemas/`.
//! A contract change shows up as a diff there.

pub mod api;
pub mod assets;
pub mod command;
pub mod data;
pub mod economy;
pub mod events;
pub mod geom;
pub mod history;
pub mod ids;
pub mod input;
pub mod money;
pub mod pack;
pub mod progress;
pub mod schema;
pub mod ship;
pub mod store;
pub mod talk;
pub mod verbs;
pub mod world;

/// Nominal tick rate. The sim only counts ticks; the app maps wall time onto them.
pub const TICKS_PER_SECOND: u32 = 30;
/// Seconds per tick at the nominal rate, for per-second data (speeds, mining rates).
pub const TICK_SECONDS: f32 = 1.0 / TICKS_PER_SECOND as f32;

/// The commonly used types in one import.
pub mod prelude {
    pub use crate::command::{
        Actor, Cause, CauseKind, Command, CommandBatch, JournalEntry, RecordedCause, Rejection,
        Reversibility, UndoRecord,
    };
    pub use crate::economy::{
        ActivityScope, ActivityTally, DockTrack, EconomyRules, EconomyState, MarketBase,
        MarketRates, MarketRole, PriceReach, TradeRequest, TradeSide,
    };
    pub use crate::events::{
        ActivityCounters, CounterKind, CounterScope, Event, EventEnvelope, EventKind,
    };
    pub use crate::geom::Vec2;
    pub use crate::history::{Fact, FactDraft, FactPayload, HistoryQuery, Subjects};
    pub use crate::ids::*;
    pub use crate::input::{Click, ClickTarget, PlayerInput, Reply};
    pub use crate::money::{CENTS_PER_CREDIT, show_credits};
    pub use crate::progress::{
        BeatKind, BeatState, BeatStatus, HullRecord, Modifier, ProgressRules, ProgressState,
        Requirement, ServiceRequest, Tunable, TunableValue,
    };
    pub use crate::ship::{MiningRun, OrderProgress, RockGrowth, ShipActivity, ShipRules, Transit};
    pub use crate::store::{Record, Store};
    pub use crate::verbs::{Condition, FlyTarget, Order, OrderQueue, Quantity, VerbKind};
    pub use crate::world::*;
    pub use crate::{TICK_SECONDS, TICKS_PER_SECOND};
}
