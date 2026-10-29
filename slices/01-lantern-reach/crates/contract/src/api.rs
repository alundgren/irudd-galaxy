//! The hook API surface: the only things the script host may expose to packs.
//!
//! Placeholder owned by W1, filled in by W5 (script host). Each hook is one namespace
//! on the Luau `ctx`, gated by a pack manifest's `uses` list. The Luau definitions
//! (`galaxy.d.luau`) will be generated from here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The 14 base engine hooks from the slice design.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    /// `on.<event>`: subscribe to events; payloads carry stable IDs.
    Events,
    /// `ctx.counters`: read activity counters (pace by activity, never time).
    Counters,
    /// `ctx.stations`, `ctx.factions`, `ctx.npcs`, `ctx.rumors`: typed reads, writes with a cause.
    Stores,
    /// `ctx.history`: `count`, `find`, `last` with filters.
    History,
    /// `ctx.canon`: the small canon graph.
    Canon,
    /// `ctx.market`: add or scale producers and consumers.
    Market,
    /// `ctx.mechanics`: per-save flags, reserved input and UI slots.
    Mechanics,
    /// `ctx.orders`: issue verbs to scripted and automated craft (never the player ship).
    Orders,
    /// `ctx.kit`: station parts, stages and variants by ID.
    Kit,
    /// `G.beat`: register beats with preconditions and an end state.
    Beats,
    /// `ctx.talk`, `ctx.map`, `ctx.chronicle`: every line cites a fact.
    Talk,
    /// `ctx.mind`: fast-model menus, briefs and budgets.
    Mind,
    /// CLI only (`galaxy gm`, `galaxy sim`): fork, fast-forward, restore.
    Sandbox,
    /// `ctx.telemetry`: economy and activity metrics.
    Telemetry,
}

impl Hook {
    pub const ALL: [Hook; 14] = [
        Hook::Events,
        Hook::Counters,
        Hook::Stores,
        Hook::History,
        Hook::Canon,
        Hook::Market,
        Hook::Mechanics,
        Hook::Orders,
        Hook::Kit,
        Hook::Beats,
        Hook::Talk,
        Hook::Mind,
        Hook::Sandbox,
        Hook::Telemetry,
    ];
}
