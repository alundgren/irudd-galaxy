//! The hook API surface: the only things the script host exposes to packs.
//!
//! Each hook is one namespace on the Luau `ctx`, gated by capabilities ("uses") a
//! pack manifest declares, such as `stations.read` or `talk.write`. The host builds
//! `ctx` from that list, so undeclared namespaces and functions are absent at runtime.
//! Every write takes a cause (`ctx:cause { ... }`) and becomes a `Command` in the
//! handler's buffer.
//!
//! `cargo xtask contract` writes `crates/contract/api/galaxy.d.luau` (the typed module
//! scripts `require("@galaxy")`, used by the strict typecheck) and `HOOKS.md` from
//! here. Record and event types in the Luau module are generated from the same JSON
//! Schemas as `schemas/`, so they can't drift from the Rust types.
//!
//! **Adding a hook function**: add a [`FnDef`] to [`FUNCTIONS`] (and its capability to
//! [`USES`] if new), implement it in `galaxy-script`'s `ctx.rs`, run
//! `cargo xtask contract`. Reserved capabilities (`available: false`) are the
//! extension points other workstreams fill in: market levers, orders, beats, the
//! fast-model budget and the asset kit.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::events::{CounterKind, Event, EventKind};
use crate::history::{Fact, FactDraft, HistoryQuery};
use crate::progress::{BeatState, RegardTier, TraitDef, TunableValue};
use crate::verbs::{Order, OrderQueue};
use crate::world::{Faction, MarketLine, Npc, Region, Rumor, Station};

/// The 14 base engine hooks from the slice design.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Hook {
    /// `on.<event>`: subscribe to events; payloads carry stable IDs.
    Events,
    /// `ctx.counters`, `G.after`: read activity counters (pace by activity, never time).
    Counters,
    /// `ctx.stations`, `ctx.factions`, `ctx.npcs`, `ctx.rumors`, `ctx.territory`.
    Stores,
    /// `ctx.history`: `count`, `find`, `last` with filters.
    History,
    /// `ctx.canon`: the small canon graph.
    Canon,
    /// `ctx.market`: stock lines; levers to add or scale producers and consumers.
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

/// A capability a manifest can list in `uses`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UseDef {
    pub id: &'static str,
    pub hook: Hook,
    /// False for reserved extension points: declaring one is a validation error until
    /// the owning workstream implements it.
    pub available: bool,
    pub summary: &'static str,
}

const fn cap(id: &'static str, hook: Hook, summary: &'static str) -> UseDef {
    UseDef {
        id,
        hook,
        available: true,
        summary,
    }
}

const fn reserved(id: &'static str, hook: Hook, summary: &'static str) -> UseDef {
    UseDef {
        id,
        hook,
        available: false,
        summary,
    }
}

/// Every capability except the per-event ones (`events.<event>`, see [`uses`]).
pub const USES: &[UseDef] = &[
    cap(
        "counters.read",
        Hook::Counters,
        "Read activity counters per player, station and region.",
    ),
    cap(
        "history.read",
        Hook::History,
        "Count, sum, find and read history facts. Results are capped.",
    ),
    cap(
        "history.write",
        Hook::History,
        "Record facts of the pack's own kinds.",
    ),
    cap(
        "stations.read",
        Hook::Stores,
        "Read stations, stages and parts.",
    ),
    cap(
        "stations.write",
        Hook::Stores,
        "Set a station's stage and owner; attach and detach kit parts.",
    ),
    cap(
        "factions.read",
        Hook::Stores,
        "Read factions and the player's standing.",
    ),
    cap(
        "factions.write",
        Hook::Stores,
        "Adjust the player's standing.",
    ),
    cap("npcs.read", Hook::Stores, "Read named people."),
    cap(
        "npcs.write",
        Hook::Stores,
        "Create, replace and move named people.",
    ),
    cap(
        "rumors.read",
        Hook::Stores,
        "Read rumors and their truth value.",
    ),
    cap(
        "rumors.write",
        Hook::Stores,
        "Plant rumors, set their truth, spread them.",
    ),
    cap(
        "territory.read",
        Hook::Stores,
        "Read region and station owners.",
    ),
    cap(
        "territory.write",
        Hook::Stores,
        "Move region borders and station ownership.",
    ),
    cap("market.read", Hook::Market, "Read market stock lines."),
    cap(
        "market.lines",
        Hook::Market,
        "Add or remove a ware's stock line at a market.",
    ),
    cap(
        "talk.write",
        Hook::Talk,
        "Offer dock talk. Names a person or place and cites a fact.",
    ),
    cap(
        "map.write",
        Hook::Talk,
        "Pin a map note to a place. Cites a fact.",
    ),
    cap(
        "chronicle.write",
        Hook::Talk,
        "Write a chronicle entry. Cites a fact; can't be rolled back.",
    ),
    cap(
        "market.levers",
        Hook::Market,
        "Add or scale producers and consumers, set stock (galaxy-economy's levers).",
    ),
    cap("orders.read", Hook::Orders, "Read a craft's order queue."),
    cap(
        "orders.write",
        Hook::Orders,
        "Issue verbs to scripted craft and the player's tenders, never the player ship.",
    ),
    cap(
        "beats.read",
        Hook::Beats,
        "Read beat state, goal lines and the act.",
    ),
    cap(
        "beats.write",
        Hook::Beats,
        "Register beats with `G.beat` in `G.script { beats = { ... } }`.",
    ),
    cap(
        "mechanics.read",
        Hook::Mechanics,
        "Read mechanic flags and reserved slot bindings.",
    ),
    cap(
        "mechanics.write",
        Hook::Mechanics,
        "Teach a new verb: set `mech.*` flags, bind reserved input and UI slots.",
    ),
    cap(
        "standing.read",
        Hook::Stores,
        "Read the player's regard tier with a faction (the number stays hidden).",
    ),
    cap(
        "standing.write",
        Hook::Stores,
        "Record a deed that moves standing (writes standing_changed, regard_changed).",
    ),
    cap("traits.read", Hook::Stores, "Read pilot traits."),
    cap(
        "pacing.read",
        Hook::Counters,
        "Read pacing tunables (`pacing.toml`).",
    ),
    cap(
        "pacing.write",
        Hook::Counters,
        "Vary a pacing tunable with a cause; a rollback restores it.",
    ),
    reserved(
        "kit.write",
        Hook::Kit,
        "Assemble stations from kit recipes, register sprites (W6).",
    ),
    reserved("canon.read", Hook::Canon, "Read the canon graph."),
    reserved("canon.write", Hook::Canon, "Open canon nodes."),
    reserved(
        "mind.write",
        Hook::Mind,
        "Fast-model menus, briefs and budgets (W5, next wave).",
    ),
    reserved(
        "telemetry.read",
        Hook::Telemetry,
        "Economy and activity metrics.",
    ),
];

/// Every capability, including `events.<event>` for each event kind.
pub fn uses() -> Vec<UseDef> {
    let mut all: Vec<UseDef> = EventKind::ALL
        .iter()
        .map(|k| UseDef {
            id: event_use(*k),
            hook: Hook::Events,
            available: true,
            summary: "Subscribe with `on.<event>`.",
        })
        .collect();
    all.extend_from_slice(USES);
    all
}

/// The capability that lets a script handle `on.<kind>`.
pub const fn event_use(kind: EventKind) -> &'static str {
    match kind {
        EventKind::Dock => "events.dock",
        EventKind::Undock => "events.undock",
        EventKind::Mine => "events.mine",
        EventKind::RockDepleted => "events.rock_depleted",
        EventKind::Buy => "events.buy",
        EventKind::Sell => "events.sell",
        EventKind::Deliver => "events.deliver",
        EventKind::Jump => "events.jump",
        EventKind::Scout => "events.scout",
        EventKind::OrderDone => "events.order_done",
        EventKind::QuestStep => "events.quest_step",
        EventKind::TraitEarned => "events.trait_earned",
        EventKind::Prestige => "events.prestige",
        EventKind::SessionStart => "events.session_start",
        EventKind::BeatStarted => "events.beat_started",
        EventKind::BeatCompleted => "events.beat_completed",
        EventKind::UpgradeFitted => "events.upgrade_fitted",
        EventKind::CapUnlocked => "events.cap_unlocked",
        EventKind::HullRank => "events.hull_rank",
        EventKind::StandingChanged => "events.standing_changed",
        EventKind::RegardChanged => "events.regard_changed",
        EventKind::TenderAcquired => "events.tender_acquired",
    }
}

pub fn find_use(id: &str) -> Option<UseDef> {
    uses().into_iter().find(|u| u.id == id)
}

/// One function on a `ctx` namespace. Called with `:`, e.g. `ctx.stations:get(id)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FnDef {
    /// `ctx.<ns>`; `""` for methods on `ctx` itself (`ctx:cause`).
    pub ns: &'static str,
    pub name: &'static str,
    /// The capability it needs; `""` if always available.
    pub needs: &'static str,
    /// Luau parameters and result, without `self`.
    pub sig: &'static str,
    pub doc: &'static str,
}

const fn f(
    ns: &'static str,
    name: &'static str,
    needs: &'static str,
    sig: &'static str,
    doc: &'static str,
) -> FnDef {
    FnDef {
        ns,
        name,
        needs,
        sig,
        doc,
    }
}

/// Every hook function, grouped by namespace.
pub const FUNCTIONS: &[FnDef] = &[
    f(
        "",
        "cause",
        "",
        "(spec: CauseSpec) -> Cause",
        "The cause for this handler's writes. `cites` are history fact IDs; they must exist. \
         One per call: everything a handler writes applies together or not at all.",
    ),
    f(
        "rng",
        "int",
        "",
        "(lo: number, hi: number) -> number",
        "A whole number in `lo..=hi` from the script's seeded stream.",
    ),
    f(
        "rng",
        "float",
        "",
        "() -> number",
        "A number in `0..1` from the script's seeded stream.",
    ),
    f(
        "store",
        "get",
        "",
        "(store: string, key: string) -> ScriptValue?",
        "A value from one of the script's declared stores.",
    ),
    f(
        "store",
        "set",
        "",
        "(store: string, key: string, value: ScriptValue?, why: Cause) -> ()",
        "Sets (or clears, with nil) a declared key. The type must match the manifest.",
    ),
    f(
        "counters",
        "get",
        "counters.read",
        "(kind: CounterKind, scope: CounterScopeArg?) -> number",
        "An activity counter for the player, or a station or region.",
    ),
    f(
        "history",
        "count",
        "history.read",
        "(q: HistoryQuery) -> number",
        "How many facts match.",
    ),
    f(
        "history",
        "sum",
        "history.read",
        "(q: HistoryQuery) -> number",
        "The sum of `amount` over matching facts.",
    ),
    f(
        "history",
        "find",
        "history.read",
        "(q: HistoryQuery) -> { Fact }",
        "Matching facts, oldest first. At most `q.limit`, and never more than 50.",
    ),
    f(
        "history",
        "last",
        "history.read",
        "(q: HistoryQuery) -> Fact?",
        "The most recent matching fact.",
    ),
    f(
        "history",
        "fact",
        "history.read",
        "(id: FactId) -> Fact?",
        "One fact by ID.",
    ),
    f(
        "history",
        "record",
        "history.write",
        "(fact: FactDraft, why: Cause) -> ()",
        "Records a fact when the batch commits. Its kind carries the pack prefix: `gm.stage_built`.",
    ),
    f(
        "stations",
        "get",
        "stations.read",
        "(id: StationId) -> Station?",
        "A station record.",
    ),
    f(
        "stations",
        "list",
        "stations.read",
        "() -> { StationId }",
        "Every station ID, sorted.",
    ),
    f(
        "stations",
        "stage",
        "stations.read",
        "(id: StationId) -> number",
        "A station's build stage.",
    ),
    f(
        "stations",
        "parts",
        "stations.read",
        "(id: StationId) -> { PartId }",
        "A station's attached kit parts, sorted.",
    ),
    f(
        "stations",
        "set_stage",
        "stations.write",
        "(id: StationId, stage: number, why: Cause) -> ()",
        "Sets a station's build stage.",
    ),
    f(
        "stations",
        "attach",
        "stations.write",
        "(id: StationId, part: PartId, why: Cause) -> ()",
        "Attaches a kit part to a station.",
    ),
    f(
        "stations",
        "detach",
        "stations.write",
        "(id: StationId, part: PartId, why: Cause) -> ()",
        "Detaches a kit part.",
    ),
    f(
        "stations",
        "set_owner",
        "stations.write",
        "(id: StationId, owner: FactionId?, why: Cause) -> ()",
        "Sets (or clears) a station's owner.",
    ),
    f(
        "factions",
        "get",
        "factions.read",
        "(id: FactionId) -> Faction?",
        "A faction record.",
    ),
    f(
        "factions",
        "list",
        "factions.read",
        "() -> { FactionId }",
        "Every faction ID, sorted.",
    ),
    f(
        "factions",
        "standing",
        "factions.read",
        "(id: FactionId) -> number",
        "The player's standing with a faction.",
    ),
    f(
        "factions",
        "adjust_standing",
        "factions.write",
        "(id: FactionId, delta: number, why: Cause) -> ()",
        "Raises or lowers the player's standing.",
    ),
    f(
        "npcs",
        "get",
        "npcs.read",
        "(id: NpcId) -> Npc?",
        "A named person.",
    ),
    f(
        "npcs",
        "list",
        "npcs.read",
        "() -> { NpcId }",
        "Every NPC ID, sorted.",
    ),
    f(
        "npcs",
        "at",
        "npcs.read",
        "(station: StationId) -> { Npc }",
        "People found at a station.",
    ),
    f(
        "npcs",
        "put",
        "npcs.write",
        "(npc: Npc, why: Cause) -> ()",
        "Creates or replaces a person. New IDs are namespaced by the pack.",
    ),
    f(
        "npcs",
        "move",
        "npcs.write",
        "(id: NpcId, station: StationId?, why: Cause) -> ()",
        "Moves a person to a station (or nowhere).",
    ),
    f(
        "rumors",
        "get",
        "rumors.read",
        "(id: RumorId) -> Rumor?",
        "A rumor.",
    ),
    f(
        "rumors",
        "list",
        "rumors.read",
        "() -> { RumorId }",
        "Every rumor ID, sorted.",
    ),
    f(
        "rumors",
        "put",
        "rumors.write",
        "(rumor: Rumor, why: Cause) -> ()",
        "Plants or replaces a rumor.",
    ),
    f(
        "rumors",
        "set_truth",
        "rumors.write",
        "(id: RumorId, is_true: boolean, why: Cause) -> ()",
        "Sets a rumor's truth value.",
    ),
    f(
        "rumors",
        "spread",
        "rumors.write",
        "(id: RumorId, station: StationId, why: Cause) -> ()",
        "A rumor starts being told at a station.",
    ),
    f(
        "territory",
        "owner",
        "territory.read",
        "(region: RegionId) -> FactionId?",
        "Who holds a region.",
    ),
    f(
        "territory",
        "station_owner",
        "territory.read",
        "(station: StationId) -> FactionId?",
        "Who holds a station.",
    ),
    f(
        "territory",
        "set_owner",
        "territory.write",
        "(region: RegionId, owner: FactionId?, why: Cause) -> ()",
        "Moves a region to a faction (or makes it unclaimed).",
    ),
    f(
        "territory",
        "set_station_owner",
        "territory.write",
        "(station: StationId, owner: FactionId?, why: Cause) -> ()",
        "A station changes hands.",
    ),
    f(
        "market",
        "line",
        "market.read",
        "(station: StationId, ware: WareId) -> MarketLine?",
        "One ware's stock line at a market.",
    ),
    f(
        "market",
        "lines",
        "market.read",
        "(station: StationId) -> { [WareId]: MarketLine }",
        "Every stock line at a market.",
    ),
    f(
        "market",
        "add_line",
        "market.lines",
        "(station: StationId, ware: WareId, line: MarketLine, why: Cause) -> ()",
        "Adds a ware the market didn't trade yet. Prices must sit inside the line's band.",
    ),
    f(
        "market",
        "remove_line",
        "market.lines",
        "(station: StationId, ware: WareId, why: Cause) -> ()",
        "Removes a ware's stock line.",
    ),
    // --- Market levers (wrapping galaxy-economy) ---
    f(
        "market",
        "scale_producers",
        "market.levers",
        "(station: StationId, ware: WareId, factor: number, why: Cause) -> ()",
        "Multiplies a line's production (1.5 for +50%). Prices follow from supply.",
    ),
    f(
        "market",
        "scale_consumers",
        "market.levers",
        "(station: StationId, ware: WareId, factor: number, why: Cause) -> ()",
        "Multiplies a line's consumption (2 for a shortage's drink).",
    ),
    f(
        "market",
        "add_producers",
        "market.levers",
        "(station: StationId, ware: WareId, per_pulse: number, why: Cause) -> ()",
        "Adds (or with a negative number removes) producer units per pulse.",
    ),
    f(
        "market",
        "add_consumers",
        "market.levers",
        "(station: StationId, ware: WareId, per_pulse: number, why: Cause) -> ()",
        "Adds (or removes) consumer units per pulse.",
    ),
    f(
        "market",
        "set_stock",
        "market.levers",
        "(station: StationId, ware: WareId, stock: number, why: Cause) -> ()",
        "Sets a line's stock outright (a delivery, a gift).",
    ),
    f(
        "market",
        "levers",
        "market.levers",
        "(levers: { Lever }, why: Cause) -> ()",
        "Several levers in order, each seeing the ones before it. Use this to compose levers on one line in one call.",
    ),
    // --- Orders ---
    f(
        "orders",
        "get",
        "orders.read",
        "(ship: ShipId) -> OrderQueue?",
        "A craft's order queue.",
    ),
    f(
        "orders",
        "can_order",
        "orders.read",
        "(ship: ShipId) -> boolean",
        "Whether scripts may give this ship orders (scripted craft and the player's tenders; never the player ship or a berthed hull).",
    ),
    f(
        "orders",
        "give",
        "orders.write",
        "(ship: ShipId, orders: { Order }, why: Cause) -> ()",
        "Replaces a craft's orders with these (the same verbs the player uses).",
    ),
    f(
        "orders",
        "append",
        "orders.write",
        "(ship: ShipId, order: Order, why: Cause) -> ()",
        "Adds one order to the end of a craft's queue.",
    ),
    f(
        "orders",
        "clear",
        "orders.write",
        "(ship: ShipId, why: Cause) -> ()",
        "Clears a craft's orders.",
    ),
    // --- Beats ---
    f(
        "beats",
        "state",
        "beats.read",
        "(id: BeatId) -> BeatState?",
        "A started beat's state (dormant beats have none).",
    ),
    f(
        "beats",
        "is_done",
        "beats.read",
        "(id: BeatId) -> boolean",
        "Whether a beat has completed.",
    ),
    f(
        "beats",
        "is_active",
        "beats.read",
        "(id: BeatId) -> boolean",
        "Whether a beat has started and not completed.",
    ),
    f(
        "beats",
        "active",
        "beats.read",
        "() -> { BeatId }",
        "Active beats, newest first.",
    ),
    f(
        "beats",
        "goal",
        "beats.read",
        "() -> GoalLine?",
        "The one goal line the HUD shows: the newest active beat's.",
    ),
    f(
        "beats",
        "goals",
        "beats.read",
        "() -> { GoalLine }",
        "Every active beat's goal line, newest first (the dockmaster's list).",
    ),
    f(
        "beats",
        "act",
        "beats.read",
        "() -> number",
        "The act the journey is in (the highest act of any started beat).",
    ),
    f(
        "beats",
        "pilot",
        "beats.read",
        "() -> PilotView",
        "The player as a beat sees them: ship, hull, where they're docked, cargo, hold room and credits (in credits).",
    ),
    f(
        "beats",
        "deliver",
        "beats.write",
        "(spec: DeliverSpec, why: Cause) -> ()",
        "A goal handover: takes cargo from the docked player ship and writes `deliver` (to a ledger, if given). Fails unless docked there with the cargo.",
    ),
    f(
        "beats",
        "load",
        "beats.write",
        "(spec: LoadSpec, why: Cause) -> ()",
        "Loads goal cargo (a consignment) into the docked player ship. Fails without the hold room.",
    ),
    // --- Mechanics ---
    f(
        "mechanics",
        "is_on",
        "mechanics.read",
        "(flag: FlagId) -> boolean",
        "Whether a per-save flag is set (`mech.*`, `cap.*`, anything).",
    ),
    f(
        "mechanics",
        "slots",
        "mechanics.read",
        "() -> { [SlotId]: FlagId }",
        "Reserved input and UI slots bound so far, and the mechanic each serves.",
    ),
    f(
        "mechanics",
        "free_slots",
        "mechanics.read",
        "() -> { SlotId }",
        "Reserved slots no mechanic has bound yet.",
    ),
    f(
        "mechanics",
        "set",
        "mechanics.write",
        "(flag: FlagId, on: boolean, why: Cause) -> ()",
        "Sets or clears a mechanic flag. Only `mech.*` flags: capabilities come from upgrades and beats.",
    ),
    f(
        "mechanics",
        "bind",
        "mechanics.write",
        "(slot: SlotId, mechanic: FlagId, why: Cause) -> ()",
        "Binds a free reserved slot to a mechanic.",
    ),
    f(
        "mechanics",
        "unbind",
        "mechanics.write",
        "(slot: SlotId, why: Cause) -> ()",
        "Frees a reserved slot.",
    ),
    f(
        "mechanics",
        "teach",
        "mechanics.write",
        "(spec: TeachSpec, why: Cause) -> ()",
        "A new verb with a teacher: sets the `mech.*` flag, binds its slots, and records `<pack>.mechanic_taught` naming the teacher.",
    ),
    // --- Standing, traits, pacing ---
    f(
        "standing",
        "tier",
        "standing.read",
        "(faction: FactionId) -> RegardTier?",
        "The player's regard tier with a faction: how its people treat them.",
    ),
    f(
        "standing",
        "station_tier",
        "standing.read",
        "(station: StationId) -> RegardTier?",
        "The regard tier with a station's owner (nil if it has none).",
    ),
    f(
        "standing",
        "tiers",
        "standing.read",
        "() -> { RegardTier }",
        "Every regard tier, lowest first.",
    ),
    f(
        "standing",
        "deed",
        "standing.write",
        "(spec: DeedSpec, why: Cause) -> ()",
        "Records a deed: moves standing (clamped, scaled by traits for `tags`) and writes `standing_changed` and, if the tier moves, `regard_changed`.",
    ),
    f(
        "traits",
        "has",
        "traits.read",
        "(id: TraitId) -> boolean",
        "Whether the pilot has earned a trait.",
    ),
    f(
        "traits",
        "earned",
        "traits.read",
        "() -> { TraitId }",
        "The pilot's traits, sorted.",
    ),
    f(
        "traits",
        "get",
        "traits.read",
        "(id: TraitId) -> TraitDef?",
        "A trait's definition: its name, deed and effect.",
    ),
    f(
        "pacing",
        "get",
        "pacing.read",
        "(id: TunableId) -> TunableValue?",
        "A tunable's current value: a number, a string or a list of strings.",
    ),
    f(
        "pacing",
        "tuned",
        "pacing.read",
        "(id: TunableId) -> boolean",
        "Whether a script changed a tunable from its authored value.",
    ),
    f(
        "pacing",
        "set",
        "pacing.write",
        "(id: TunableId, value: TunableValue, why: Cause) -> ()",
        "Varies a tunable. Set it while the beat that reads it is dormant; a rollback restores the authored value.",
    ),
    f(
        "talk",
        "offer",
        "talk.write",
        "(line: LineSpec, why: Cause) -> ()",
        "Offers a line of dock talk. Needs a `speaker` or `place` and `cites`.",
    ),
    f(
        "map",
        "note",
        "map.write",
        "(line: LineSpec, why: Cause) -> ()",
        "Pins a note to a place on the map. Needs `place` and `cites`.",
    ),
    f(
        "chronicle",
        "write",
        "chronicle.write",
        "(line: LineSpec, why: Cause) -> ()",
        "Writes a chronicle entry. Needs a `speaker` or `place` and `cites`.",
    ),
];

/// Namespaces on `ctx`, in the order they appear in the `Ctx` type.
pub const NAMESPACES: &[(&str, &str)] = &[
    ("rng", "RngApi"),
    ("store", "StoreApi"),
    ("counters", "CountersApi"),
    ("history", "HistoryApi"),
    ("stations", "StationsApi"),
    ("factions", "FactionsApi"),
    ("npcs", "NpcsApi"),
    ("rumors", "RumorsApi"),
    ("territory", "TerritoryApi"),
    ("market", "MarketApi"),
    ("orders", "OrdersApi"),
    ("beats", "BeatsApi"),
    ("mechanics", "MechanicsApi"),
    ("standing", "StandingApi"),
    ("traits", "TraitsApi"),
    ("pacing", "PacingApi"),
    ("talk", "TalkApi"),
    ("map", "MapApi"),
    ("chronicle", "ChronicleApi"),
];

/// The function `ctx.<ns>:<name>`, if it exists.
pub fn function(ns: &str, name: &str) -> Option<&'static FnDef> {
    FUNCTIONS.iter().find(|d| d.ns == ns && d.name == name)
}

/// Every activity counter, for `G.after` and `ctx.counters:get`.
pub const COUNTERS: [CounterKind; 11] = [
    CounterKind::Dockings,
    CounterKind::Deliveries,
    CounterKind::Jumps,
    CounterKind::Sessions,
    CounterKind::UnitsMined,
    CounterKind::RocksDepleted,
    CounterKind::Sales,
    CounterKind::Purchases,
    CounterKind::Scouts,
    CounterKind::OrdersDone,
    CounterKind::LedgerDeliveries,
];

/// A counter's name in Luau and JSON.
pub fn counter_name(kind: CounterKind) -> String {
    serde_json::to_value(kind)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Most facts `history:find` returns.
pub const HISTORY_FIND_CAP: u32 = 50;

/// `PascalCase` of a snake_case name.
fn pascal(s: &str) -> String {
    s.split('_')
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_ascii_uppercase().to_string() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

/// The Luau type name for one event kind's handler argument, e.g. `DockEvent`.
pub fn event_type_name(kind: EventKind) -> String {
    format!("{}Event", pascal(kind.as_str()))
}

// --- Luau generation ----------------------------------------------------------------

/// Converts JSON Schemas to Luau types.
struct Luau<'a> {
    defs: &'a BTreeMap<String, Value>,
}

impl Luau<'_> {
    fn ty(&self, s: &Value) -> String {
        if let Some(r) = s.get("$ref").and_then(Value::as_str) {
            return r.rsplit('/').next().unwrap_or("any").to_owned();
        }
        if let Some(c) = s.get("const") {
            return literal(c);
        }
        if let Some(e) = s.get("enum").and_then(Value::as_array) {
            return e.iter().map(literal).collect::<Vec<_>>().join(" | ");
        }
        for key in ["oneOf", "anyOf"] {
            if let Some(alts) = s.get(key).and_then(Value::as_array) {
                return self.union(alts);
            }
        }
        if let Some(all) = s.get("allOf").and_then(Value::as_array)
            && all.len() == 1
        {
            return self.ty(&all[0]);
        }
        match s.get("type") {
            Some(Value::Array(types)) => {
                let alts: Vec<Value> = types
                    .iter()
                    .map(|t| {
                        let mut one = s.clone();
                        one["type"] = t.clone();
                        one
                    })
                    .collect();
                self.union(&alts)
            }
            Some(Value::String(t)) => match t.as_str() {
                "string" => "string".into(),
                "integer" | "number" => "number".into(),
                "boolean" => "boolean".into(),
                "null" => "nil".into(),
                "array" => {
                    let item = s.get("items").map_or("any".into(), |i| self.ty(i));
                    format!("{{ {item} }}")
                }
                "object" => self.object(s),
                _ => "any".into(),
            },
            _ => "any".into(),
        }
    }

    fn union(&self, alts: &[Value]) -> String {
        let mut optional = false;
        let mut parts: Vec<String> = Vec::new();
        for a in alts {
            let t = self.ty(a);
            if t == "nil" {
                optional = true;
            } else if !parts.contains(&t) {
                parts.push(t);
            }
        }
        let joined = parts.join(" | ");
        match (optional, parts.len()) {
            (false, _) => joined,
            (true, 1) => format!("{joined}?"),
            (true, _) => format!("({joined})?"),
        }
    }

    fn object(&self, s: &Value) -> String {
        let required: BTreeSet<&str> = s
            .get("required")
            .and_then(Value::as_array)
            .map(|r| r.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let mut fields = Vec::new();
        if let Some(props) = s.get("properties").and_then(Value::as_object) {
            for (name, p) in props {
                let mut t = self.ty(p);
                if !required.contains(name.as_str()) && !t.ends_with('?') {
                    t = if t.contains(" | ") {
                        format!("({t})?")
                    } else {
                        format!("{t}?")
                    };
                }
                fields.push(format!("{}: {t}", field_name(name)));
            }
        }
        if let Some(extra) = s.get("additionalProperties")
            && extra.is_object()
        {
            fields.push(format!("[string]: {}", self.ty(extra)));
        }
        if fields.is_empty() {
            return "{ [string]: any }".into();
        }
        format!("{{ {} }}", fields.join(", "))
    }

    /// The object schema of one tagged-enum variant (`type: { const: name }`).
    fn variant(&self, root: &Value, tag: &str, name: &str) -> Option<Value> {
        let alts = root.get("oneOf").and_then(Value::as_array)?;
        alts.iter()
            .find(|v| {
                v.pointer(&format!("/properties/{tag}/const"))
                    .and_then(Value::as_str)
                    == Some(name)
            })
            .cloned()
    }
}

/// Luau keywords can't be bare field names; they use the `["until"]` form.
fn field_name(name: &str) -> String {
    const KEYWORDS: &[&str] = &[
        "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "if", "in",
        "local", "nil", "not", "or", "repeat", "return", "then", "true", "until", "while",
    ];
    if KEYWORDS.contains(&name) {
        format!("[\"{name}\"]")
    } else {
        name.to_owned()
    }
}

fn literal(v: &Value) -> String {
    match v {
        Value::String(s) => format!("\"{s}\""),
        Value::Bool(b) => b.to_string(),
        Value::Null => "nil".into(),
        other => other.to_string(),
    }
}

fn schema_value<T: JsonSchema>() -> Value {
    serde_json::to_value(schema_for!(T)).expect("schema serializes")
}

fn collect_defs(roots: &[(&str, Value)]) -> BTreeMap<String, Value> {
    let mut defs = BTreeMap::new();
    for (_, s) in roots {
        if let Some(d) = s.get("$defs").and_then(Value::as_object) {
            for (k, v) in d {
                defs.insert(k.clone(), v.clone());
            }
        }
    }
    defs
}

/// The functions a capability list allows (all of them for `None`).
fn allowed(uses: Option<&[String]>) -> impl Fn(&FnDef) -> bool + '_ {
    move |d| d.needs.is_empty() || uses.is_none_or(|u| u.iter().any(|x| x == d.needs))
}

/// `galaxy.d.luau`: the typed module behind `require("@galaxy")`.
///
/// With `uses`, the `Ctx` type and the handler table only contain what those
/// capabilities allow, so the strict typecheck rejects anything undeclared.
pub fn luau_module(uses: Option<&[String]>) -> String {
    let roots: Vec<(&str, Value)> = vec![
        ("Station", schema_value::<Station>()),
        ("Faction", schema_value::<Faction>()),
        ("Npc", schema_value::<Npc>()),
        ("Rumor", schema_value::<Rumor>()),
        ("Region", schema_value::<Region>()),
        ("MarketLine", schema_value::<MarketLine>()),
        ("Fact", schema_value::<Fact>()),
        ("FactDraft", schema_value::<FactDraft>()),
        ("HistoryQuery", schema_value::<HistoryQuery>()),
        ("Event", schema_value::<Event>()),
        ("CounterKind", schema_value::<CounterKind>()),
        ("BeatState", schema_value::<BeatState>()),
        ("Order", schema_value::<Order>()),
        ("OrderQueue", schema_value::<OrderQueue>()),
        ("RegardTier", schema_value::<RegardTier>()),
        ("TraitDef", schema_value::<TraitDef>()),
        ("TunableValue", schema_value::<TunableValue>()),
        ("IdTypes", schema_value::<IdTypes>()),
    ];
    let defs = collect_defs(&roots);
    let lu = Luau { defs: &defs };
    let ok = allowed(uses);

    let mut out = String::new();
    out.push_str(
        "--!strict\n\
         -- GENERATED by `cargo xtask contract` from crates/contract/src/api.rs. Do not edit.\n\
         --\n\
         -- The typed module behind `require(\"@galaxy\")`. Scripts are checked against it\n\
         -- in strict mode; at runtime the script host provides the real values.\n\n",
    );

    out.push_str("-- Records and payloads (generated from the contract's JSON Schemas)\n\n");
    let root_names: BTreeSet<&str> = roots.iter().map(|(n, _)| *n).collect();
    for (name, def) in lu.defs.iter() {
        if root_names.contains(name.as_str()) {
            continue;
        }
        let _ = writeln!(out, "export type {name} = {}", lu.ty(def));
    }
    for (name, s) in &roots {
        // A root that refers to itself (an order's loop holds orders) is `$ref: "#"`.
        let ty = lu
            .ty(s)
            .replace("{ # }", &format!("{{ {name} }}"))
            .replace(": #", &format!(": {name}"));
        let _ = writeln!(out, "export type {name} = {ty}");
    }

    out.push_str("\n-- Event handler arguments: the event plus the fact it was recorded as.\n\n");
    let event_schema = &roots.iter().find(|(n, _)| *n == "Event").expect("event").1;
    let mut handler_fields = Vec::new();
    for kind in EventKind::ALL {
        let name = event_type_name(kind);
        let mut variant = lu
            .variant(event_schema, "type", kind.as_str())
            .unwrap_or_else(|| serde_json::json!({"type": "object"}));
        // Envelope fields every handler argument carries.
        if let Some(p) = variant.get_mut("properties").and_then(Value::as_object_mut) {
            p.insert("fact".into(), serde_json::json!({"$ref": "#/$defs/FactId"}));
            p.insert("tick".into(), serde_json::json!({"$ref": "#/$defs/Tick"}));
            p.insert(
                "cause".into(),
                serde_json::json!({"$ref": "#/$defs/CauseId"}),
            );
        }
        if let Some(r) = variant.get_mut("required").and_then(Value::as_array_mut) {
            for k in ["fact", "tick", "cause"] {
                r.push(k.into());
            }
        }
        let _ = writeln!(out, "export type {name} = {}", lu.ty(&variant));
        if uses.is_none_or(|u| u.iter().any(|x| x == event_use(kind))) {
            handler_fields.push(format!(
                "\t{}: ((ev: {name}, ctx: Ctx) -> ())?,",
                kind.as_str()
            ));
        }
    }

    out.push_str("\n-- Hook arguments\n\n");
    out.push_str(
        "export type ScriptValue = boolean | number | string\n\
         export type CauseSpec = { summary: string, cites: { FactId }? }\n\
         -- A cause handle from `ctx:cause`. Pass it to every write.\n\
         export type Cause = { summary: string, cites: { FactId }, handle: number }\n\
         export type CounterScopeArg = { station: StationId?, region: RegionId? }\n\
         -- A line on a legibility channel. Needs `speaker` or `place`, and `cites`.\n\
         export type LineSpec = { speaker: NpcId?, place: StationId?, text: string, cites: { FactId } }\n",
    );
    let counter_fields: Vec<String> = COUNTERS
        .iter()
        .map(|c| format!("{}: number?", counter_name(*c)))
        .collect();
    let _ = writeln!(
        out,
        "-- `G.after` fires when every listed counter reaches its value (activity, never time).\n\
         export type AfterSpec = {{ {}, station: StationId?, region: RegionId?, every: boolean? }}",
        counter_fields.join(", ")
    );
    out.push_str(
        "export type TriggerFire = { name: string, fired: number, fact: FactId? }\n\
         export type Trigger = { after: AfterSpec, run: (ctx: Ctx, fire: TriggerFire) -> () }\n",
    );
    out.push_str(BEAT_TYPES);

    out.push_str("\n-- The ctx namespaces\n\n");
    for (ns, ty) in NAMESPACES {
        let mut fields = Vec::new();
        for d in FUNCTIONS.iter().filter(|d| d.ns == *ns && ok(d)) {
            let sig = with_self(d.sig, ty);
            fields.push(format!("\t-- {}\n\t{}: {sig},", d.doc, d.name));
        }
        if fields.is_empty() {
            continue;
        }
        let _ = writeln!(out, "export type {ty} = {{\n{}\n}}", fields.join("\n"));
    }

    let mut ctx_fields = vec![
        "\t-- The pack and script running.".to_owned(),
        "\tpack: string,".to_owned(),
        "\tscript: string,".to_owned(),
    ];
    for d in FUNCTIONS.iter().filter(|d| d.ns.is_empty()) {
        ctx_fields.push(format!(
            "\t-- {}\n\t{}: {},",
            d.doc,
            d.name,
            with_self(d.sig, "Ctx")
        ));
    }
    for (ns, ty) in NAMESPACES {
        if FUNCTIONS.iter().any(|d| d.ns == *ns && ok(d)) {
            ctx_fields.push(format!("\t{ns}: {ty},"));
        }
    }
    let _ = writeln!(out, "export type Ctx = {{\n{}\n}}", ctx_fields.join("\n"));

    let _ = writeln!(
        out,
        "\n-- What a script returns\n\nexport type Handlers = {{\n{}\n}}",
        handler_fields.join("\n")
    );
    out.push_str(
        "export type ScriptDef = { id: string, on: Handlers?, after: { [string]: Trigger }?, beats: { BeatSpec }? }\n\n\
         export type Galaxy = {\n\
         \t-- Declares a script. `id` matches the manifest's `[[script]] id`.\n\
         \tscript: (def: ScriptDef) -> ScriptDef,\n\
         \t-- An activity trigger: runs once when the counters reach `spec`\n\
         \t-- (or every time they grow by it again, with `every = true`).\n\
         \tafter: (spec: AfterSpec, run: (ctx: Ctx, fire: TriggerFire) -> ()) -> Trigger,\n",
    );
    out.push_str(BEAT_BUILDERS);
    out.push_str(
        "}\n\n\
         local G: Galaxy = nil :: any\n\
         return G\n",
    );
    out
}

/// Pulls the ID types hook signatures name into the generated module.
#[derive(JsonSchema)]
#[allow(dead_code)]
struct IdTypes {
    beat: crate::ids::BeatId,
    flag: crate::ids::FlagId,
    ledger: crate::ids::LedgerId,
    slot: crate::ids::SlotId,
    trait_id: crate::ids::TraitId,
    tunable: crate::ids::TunableId,
    upgrade: crate::ids::UpgradeId,
    ship: crate::ids::ShipId,
    hull: crate::ids::HullId,
}

/// Luau types for registering beats (`beats.write`) and the shapes some hooks take.
const BEAT_TYPES: &str = "\
-- A requirement: world state, history and activity, never time. Build them with the\n\
-- G.* helpers below; a list of requirements means all of them.\n\
export type Requirement = string | { [any]: any }\n\
-- A number, or `G.tunable(id)`: the current value of a pacing tunable.\n\
export type Num = number | { tunable: TunableId }\n\
-- Which facts a history requirement counts: all of history, since the beat started,\n\
-- or since its current step started.\n\
export type Since = \"ever\" | \"start\" | \"step\"\n\
export type FactOpts = { at_least: Num?, since: Since? }\n\
export type StepSpec = { id: string, goal: string, done: Requirement }\n\
-- Credits are written as credits (like data files), not centicredits.\n\
export type RewardSpec = { credits: number?, standing: { [FactionId]: number }?, flags: { FlagId }?, fit: { UpgradeId }? }\n\
-- What a beat callback gets. Its writes made with `beat.why` apply atomically with the\n\
-- transition; if the callback errors, the beat waits and tries again next tick.\n\
export type BeatInfo = { id: BeatId, transition: \"start\" | \"step\" | \"complete\", step: number, step_id: string?, why: Cause }\n\
export type BeatHook = (ctx: Ctx, beat: BeatInfo) -> ()\n\
export type BeatSpec = { id: BeatId, act: number, kind: (\"directed\" | \"situation\")?, giver: NpcId?, goal: string, requires: Requirement?, steps: { StepSpec }?, done: Requirement?, rewards: RewardSpec?, on_start: BeatHook?, on_step: BeatHook?, on_complete: BeatHook? }\n\
-- The HUD's goal line for one active beat.\n\
export type GoalLine = { beat: BeatId, act: number, goal: string, giver: NpcId?, step: string? }\n\
export type TeachSpec = { mechanic: FlagId, teacher: NpcId, slots: { SlotId }? }\n\
export type DeliverSpec = { station: StationId, ware: WareId, quantity: number, ledger: LedgerId? }\n\
-- The player for a beat: `credits` are credits (40.5), not centicredits.\n\
export type PilotView = { ship: ShipId?, hull: HullId?, docked_at: StationId?, region: RegionId?, cargo: { [WareId]: number }, hold_free: number, credits: number }\n\
export type LoadSpec = { ware: WareId, quantity: number }\n\
export type DeedSpec = { faction: FactionId, deed: string, delta: number, tags: { string }? }\n\
-- A market lever (galaxy-economy). Prices follow from the new supply.\n\
export type Lever = { lever: \"add_producers\" | \"add_consumers\", station: StationId, ware: WareId, per_pulse: number }\n\
\t| { lever: \"scale_producers\" | \"scale_consumers\", station: StationId, ware: WareId, factor: number }\n\
\t| { lever: \"set_stock\", station: StationId, ware: WareId, stock: number }\n\
\t| { lever: \"remove_line\", station: StationId, ware: WareId }\n\
\t| { lever: \"add_line\", station: StationId, ware: WareId, line: { [string]: any } }\n";

/// The `G.*` beat and requirement builders, inside the `Galaxy` type.
const BEAT_BUILDERS: &str = "\
\t-- Registers a beat (needs `beats.write`). Put it in `G.script { beats = { ... } }`.\n\
\tbeat: (def: BeatSpec) -> BeatSpec,\n\
\t-- Requirements. Nothing here can name a time.\n\
\talways: Requirement,\n\
\tall: (list: { Requirement }) -> Requirement,\n\
\tany: (list: { Requirement }) -> Requirement,\n\
\tnever: (r: Requirement) -> Requirement,\n\
\tdone: (beat: BeatId) -> Requirement,\n\
\tstarted: (beat: BeatId) -> Requirement,\n\
\tactive: (beat: BeatId) -> Requirement,\n\
\t-- The beat has reached step `step` (0 = its first step is under way).\n\
\tat_step: (beat: BeatId, step: number) -> Requirement,\n\
\tcounter: (kind: CounterKind, at_least: Num, scope: CounterScopeArg?) -> Requirement,\n\
\t-- At least one (or `opts.at_least`) history fact matches.\n\
\tfact: (q: HistoryQuery, opts: FactOpts?) -> Requirement,\n\
\t-- Matching facts' amounts add up to `at_least`.\n\
\tsum: (q: HistoryQuery, at_least: Num, opts: FactOpts?) -> Requirement,\n\
\tflag: (flag: FlagId) -> Requirement,\n\
\ttrait: (id: TraitId) -> Requirement,\n\
\tfitted: (id: UpgradeId) -> Requirement,\n\
\thull: (id: HullId) -> Requirement,\n\
\towns: (ship: ShipId) -> Requirement,\n\
\t-- The player has at least this many credits.\n\
\tcredits: (credits: number) -> Requirement,\n\
\tcargo: (ware: WareId, at_least: Num) -> Requirement,\n\
\thold_free: (at_least: Num) -> Requirement,\n\
\tdocked_at: (station: StationId) -> Requirement,\n\
\tdocked: () -> Requirement,\n\
\tin_region: (region: RegionId) -> Requirement,\n\
\t-- Regard at least this tier (`stranger`, `known`, `trusted`, `kin`).\n\
\tregard: (faction: FactionId, tier: string) -> Requirement,\n\
\ttunable_is: (id: TunableId, value: TunableValue) -> Requirement,\n\
\t-- A Luau predicate over the world, for what the others can't say. Read only.\n\
\ttest: (f: (ctx: Ctx) -> boolean) -> Requirement,\n\
\t-- A pacing tunable's current value, wherever a number goes.\n\
\ttunable: (id: TunableId) -> Num,\n";

fn with_self(sig: &str, ty: &str) -> String {
    match sig.strip_prefix("()") {
        Some(rest) => format!("(self: {ty}){rest}"),
        None => format!("(self: {ty}, {}", &sig[1..]),
    }
}

/// `HOOKS.md`: the human-readable hook reference.
pub fn hooks_md() -> String {
    let mut out = String::from(
        "# The Luau hook API\n\n\
         GENERATED by `cargo xtask contract` from `crates/contract/src/api.rs`. Do not edit.\n\n\
         A pack's scripts reach the world only through `ctx`, built from the manifest's\n\
         `uses` list: undeclared namespaces and functions are absent at runtime and are\n\
         type errors in `galaxy pack check`. Every write takes a cause from\n\
         `ctx:cause { summary = ..., cites = { fact IDs } }` and becomes a command in the\n\
         handler's buffer; if the handler errors or runs over budget, nothing applies.\n\
         Types are in [`galaxy.d.luau`](galaxy.d.luau).\n\n\
         ## Script shape\n\n\
         ```luau\n\
         --!strict\n\
         local G = require(\"@galaxy\")\n\
         return G.script {\n\
         \tid = \"kettle_growth\",\n\
         \ton = {\n\
         \t\tsell = function(ev: G.SellEvent, ctx: G.Ctx) end,\n\
         \t},\n\
         \tafter = {\n\
         \t\tregulars = G.after({ dockings = 3, station = \"station.kettle_dock\" }, function(ctx, fire) end),\n\
         \t},\n\
         }\n\
         ```\n\n\
         Always on `ctx`: `ctx.pack`, `ctx.script`, `ctx:cause`, `ctx.rng` (seeded per script)\n\
         and `ctx.store` (the script's declared stores).\n\n\
         ## Capabilities\n\n\
         | `uses` | Hook | Status | What |\n|---|---|---|---|\n",
    );
    for u in uses() {
        let status = if u.available { "available" } else { "reserved" };
        let _ = writeln!(
            out,
            "| `{}` | {:?} | {status} | {} |",
            u.id, u.hook, u.summary
        );
    }
    out.push_str("\n## Functions\n");
    let mut last = None;
    for d in FUNCTIONS {
        if last != Some(d.ns) {
            let title = if d.ns.is_empty() {
                "ctx".to_owned()
            } else {
                format!("ctx.{}", d.ns)
            };
            let _ = writeln!(out, "\n### `{title}`\n");
            last = Some(d.ns);
        }
        let call = if d.ns.is_empty() {
            format!("ctx:{}{}", d.name, d.sig)
        } else {
            format!("ctx.{}:{}{}", d.ns, d.name, d.sig)
        };
        let needs = if d.needs.is_empty() {
            "always".to_owned()
        } else {
            format!("`{}`", d.needs)
        };
        let _ = writeln!(out, "- `{call}` ({needs}): {}", d.doc);
    }
    out.push_str(BEATS_MD);
    out.push_str(
        "\n## Events\n\n\
         `on.<event>` handlers get the event's fields plus `fact` (the history fact it was\n\
         recorded as: cite it), `tick` and `cause`. They need `events.<event>` in `uses`.\n\n",
    );
    for k in EventKind::ALL {
        let _ = writeln!(out, "- `on.{}`: `{}`", k.as_str(), event_type_name(k));
    }
    out.push_str(
        "\n## Not available to scripts\n\n\
         No clock, timer or wall time (`os`, `wait`, `delay`, `tick`); no `math.random`\n\
         (use `ctx.rng`); no `loadstring`, `getfenv`, `setfenv`; `require` only sees\n\
         `@galaxy` and the pack's own modules (`./name`). Sandbox and snapshots are CLI\n\
         only (`galaxy gm`, `galaxy sim`).\n\n\
         ## Sandbox rules\n\n\
         - **No memory between calls.** Each handler call runs the script and its modules\n\
           afresh, so module-level locals start over every call. What a script must\n\
           remember goes in its declared `stores` (saved with the world).\n\
         - **One cause per call.** A second `ctx:cause` is an error. The call's writes are\n\
           one batch: if any would be rejected, none apply and the call counts as aborted.\n\
         - **Deterministic.** `tostring` and `string.format` print tables, functions and\n\
           buffers as their type name, never an address. `pairs` and `next` refuse tables\n\
           keyed by tables, functions or buffers (their order isn't stable), and loops must\n\
           iterate with `pairs(t)`, `ipairs(t)`, `next`, `string.gmatch` or `utf8.codes`:\n\
           a bare `for k, v in t do` is a check error. The garbage collector doesn't run\n\
           during a call, so weak tables don't change under a handler.\n\
         - **Limits.** Per call: `limits.steps_per_call` steps, `limits.memory_bytes` of\n\
           Luau memory (everything the call allocates, garbage included), and at most\n\
           256 writes holding 256 KiB. Arguments to `ctx` are size-checked before they are\n\
           copied. Numbers passed to `ctx` must be whole where a whole number is expected\n\
           and in range; nothing is truncated or wrapped.\n\
         - **Strict types.** Every checked file is typechecked in strict mode;\n\
           `--!nocheck` and `--!nonstrict` are check errors.\n",
    );
    out
}

const BEATS_MD: &str = "
## Beats (`beats.write`)

A script registers beats with `G.beat` in `G.script { beats = { ... } }`. The script
host checks each beat every tick: a dormant beat starts when `requires` holds
(`beat_started`), each step finishes when its `done` holds (the next step writes
`quest_step`, with `quest` = the beat ID), and the beat completes when every step is
done and its `done` holds (`beat_completed`, then `rewards`). Beat state lives in the
world by stable ID, so beats survive save, load and hot reload.

```luau
G.beat {
\tid = \"beat.b02_mine_by_hand\",
\tact = 1,
\tgiver = \"npc.ilse_marrow\",
\tgoal = \"Hold Space beside an ice rock until your hold is full.\",
\trequires = G.done(\"beat.b01_undock\"),
\tdone = G.cargo(\"ware.ice\", G.tunable(\"b02.ice\")),
\trewards = { credits = 0 },
\ton_complete = function(ctx, beat) end, -- optional; writes with beat.why are atomic with it
}
```

- Requirements never name a time. Build them with `G.all`, `G.any`, `G.never`,
  `G.done`, `G.started`, `G.active`, `G.at_step`, `G.counter`, `G.fact`, `G.sum`,
  `G.flag`, `G.trait`, `G.fitted`, `G.hull`, `G.owns`, `G.credits`, `G.cargo`,
  `G.hold_free`, `G.docked_at`, `G.docked`, `G.in_region`, `G.regard`,
  `G.tunable_is`, and `G.test(function(ctx) return ... end)` for anything else (read
  only). A list of requirements means all of them. `G.fact`/`G.sum` take
  `{ since = \"start\" }` or `\"step\"` to count only facts after the beat or step began.
- Numbers can be `G.tunable(\"id\")`: pacing thresholds live in `pacing.toml`.
- `kind = \"situation\"` marks a state-driven beat (shortage, dispute, rumor); it runs
  the same way.
- Callbacks `on_start`, `on_step` and `on_complete` get `(ctx, beat)`. To say a line
  that cites the beat's fact, handle `on.beat_started` / `on.quest_step` /
  `on.beat_completed` instead: their `ev.fact` exists.
- `ctx.beats:goal()` is the HUD's one goal line (the newest active beat's).
";

/// `(file name, contents)` of the generated API files in `crates/contract/api/`.
pub fn render() -> Vec<(String, String)> {
    vec![
        ("galaxy.d.luau".into(), luau_module(None)),
        ("HOOKS.md".into(), hooks_md()),
    ]
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn every_function_needs_a_known_capability() {
        for d in FUNCTIONS {
            if !d.needs.is_empty() {
                let u = find_use(d.needs).unwrap_or_else(|| panic!("{} unknown", d.needs));
                assert!(u.available, "{} is reserved", d.needs);
            }
            assert!(
                d.ns.is_empty() || NAMESPACES.iter().any(|(n, _)| *n == d.ns),
                "namespace {} not listed",
                d.ns
            );
        }
    }

    #[test]
    fn filtered_module_drops_undeclared_namespaces() {
        let uses = vec!["stations.read".to_owned(), "events.dock".to_owned()];
        let m = luau_module(Some(&uses));
        assert!(m.contains("stations: StationsApi"));
        assert!(!m.contains("talk: TalkApi"));
        assert!(!m.contains("set_stage"));
        assert!(m.contains("dock: ((ev: DockEvent"));
        assert!(!m.contains("sell: ((ev: SellEvent"));
    }

    #[test]
    fn committed_api_files_are_fresh() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("api");
        for (file, text) in render() {
            let on_disk = std::fs::read_to_string(dir.join(&file)).unwrap_or_default();
            assert!(
                on_disk.replace("\r\n", "\n") == text,
                "{file} is stale, run `cargo xtask contract`"
            );
        }
    }
}
