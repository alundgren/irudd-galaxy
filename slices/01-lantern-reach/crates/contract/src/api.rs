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
    reserved(
        "market.levers",
        Hook::Market,
        "Add or scale producers and consumers (W3).",
    ),
    reserved(
        "orders.write",
        Hook::Orders,
        "Issue verbs to scripted and automated craft (W2).",
    ),
    reserved(
        "kit.write",
        Hook::Kit,
        "Assemble stations from kit recipes, register sprites (W6).",
    ),
    reserved("beats.write", Hook::Beats, "Register beats (W4)."),
    reserved("canon.read", Hook::Canon, "Read the canon graph."),
    reserved("canon.write", Hook::Canon, "Open canon nodes."),
    reserved(
        "mechanics.write",
        Hook::Mechanics,
        "Set mechanic flags, bind reserved slots.",
    ),
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
        EventKind::Paused => "events.paused",
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
    ("talk", "TalkApi"),
    ("map", "MapApi"),
    ("chronicle", "ChronicleApi"),
];

/// The function `ctx.<ns>:<name>`, if it exists.
pub fn function(ns: &str, name: &str) -> Option<&'static FnDef> {
    FUNCTIONS.iter().find(|d| d.ns == ns && d.name == name)
}

/// Every activity counter, for `G.after` and `ctx.counters:get`.
pub const COUNTERS: [CounterKind; 10] = [
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
        let _ = writeln!(out, "export type {name} = {}", lu.ty(s));
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
        "export type ScriptDef = { id: string, on: Handlers?, after: { [string]: Trigger }? }\n\n\
         export type Galaxy = {\n\
         \t-- Declares a script. `id` matches the manifest's `[[script]] id`.\n\
         \tscript: (def: ScriptDef) -> ScriptDef,\n\
         \t-- An activity trigger: runs once when the counters reach `spec`\n\
         \t-- (or every time they grow by it again, with `every = true`).\n\
         \tafter: (spec: AfterSpec, run: (ctx: Ctx, fire: TriggerFire) -> ()) -> Trigger,\n\
         }\n\n\
         local G: Galaxy = nil :: any\n\
         return G\n",
    );
    out
}

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
         `@galaxy` and the pack's own modules (`./name`) under `scripts/`, plus `lib/` if\n\
         the manifest sets `lib = true` (never `tests/`). Sandbox and snapshots are CLI\n\
         only (`galaxy gm`, `galaxy sim`).\n\n\
         ## Sandbox rules\n\n\
         - **No memory between calls.** Each handler call runs the script and its modules\n\
           afresh in a fresh Luau state, so module-level locals start over every call. What\n\
           a script must remember goes in its declared `stores` (saved with the world).\n\
         - **One cause per call.** A second `ctx:cause` is an error. The call's writes are\n\
           one batch: if any would be rejected, none apply and the call counts as aborted.\n\
         - **Deterministic.** `tostring` and `string.format` print tables, functions and\n\
           buffers as their type name, never an address. `pairs` and `next` refuse tables\n\
           keyed by tables, functions or buffers (their order isn't stable). A generic\n\
           `for` names exactly one `pairs(...)`, `ipairs(...)`, `string.gmatch(...)` or\n\
           `utf8.codes(...)` call, or `next, t`, between `in` and `do` (or a local that\n\
           only aliases one of them): anything else, such as `for k, v in t` or\n\
           `pairs(a) and t`, is a check error. The garbage collector doesn't run during a\n\
           call, so weak tables don't change under a handler.\n\
         - **Limits.** Per call: `limits.steps_per_call` steps, `limits.memory_bytes` of\n\
           Luau memory (everything the call allocates, garbage included), and at most\n\
           256 writes holding 256 KiB. Library functions that do work in proportion to\n\
           their input (`pairs`, `next`, `table.sort`, `string.rep`, `string.find`, history\n\
           queries ...) charge steps for it: one per table slot or element, one per 64\n\
           bytes. Text that leaves the call (error messages, `print`) is cut to 1 KiB,\n\
           and a recorded fact's note is at most 1 KiB.\n\
           Arguments to `ctx` are size-checked before they are\n\
           copied. Numbers passed to `ctx` must be whole where a whole number is expected\n\
           and in range; nothing is truncated or wrapped.\n\
         - **Strict types.** Every file a script can load is typechecked in strict mode.\n\
           Its first line must be `--!strict`, and `!nocheck` or `!nonstrict` anywhere in\n\
           it, even in a comment or string, is a check error.\n",
    );
    out
}

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
