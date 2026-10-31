//! The live world layer: small, local flourishes at a station through OpenRouter's
//! auto router (the low cost tier), each docking at most one:
//!
//! - **banter**: one of the station's keepers says a line at the dock (a talk bubble);
//! - **traffic**: a named craft hails the station on its way somewhere (a notice);
//! - **brag**: a trader brags about a haul of one ware (a notice);
//! - **discount**: the market keeper takes a few percent off one ware here for a few
//!   dockings (a market lever on a ware the station sells, a notice, a "Changed" ribbon
//!   on the market, and the keeper's line).
//!
//! The model only picks from a **closed world**: the prompt lists the station, its
//! keepers, the named craft, the wares and the stations by ID with their names, and the
//! answer is a JSON object that names only those IDs. [`validate`] checks every ID
//! against the list offered and every line (one short line, first person, no digits,
//! no links, no names from outside the world); a discount's percent and length are
//! clamped into `live.discount`. Anything else is rejected and the flourish skipped.
//!
//! Accepted results go into **station history only**: a `mind.banter`, `mind.traffic`,
//! `mind.brag` or `mind.discount` fact with `Cause::Mind`, cited by the line said about
//! it on the next pump ([`say`]). A discount's lever applies at a calm moment (the
//! player docked) and ends after its dockings (`mind.discount_ended`), even if the live
//! layer is off by then. Nothing here changes the player's cargo or credits.
//!
//! [`crate::MindHost`] drives it: dockings trigger it (counted, never timed), with a
//! per-station cooldown in dockings, a per-run call budget and the region's budget.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::parts::{PartNews, Ribbon, RoomKind};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_economy::levers::{Lever, lever_commands};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::config::{LiveRules, MindConfig};
use crate::mind::{LiveDraft, Purpose};
use crate::packet::Names;
use crate::validate::{Invalid, english, tokens};

pub const BANTER_FACT: &str = "mind.banter";
pub const TRAFFIC_FACT: &str = "mind.traffic";
pub const BRAG_FACT: &str = "mind.brag";
pub const DISCOUNT_FACT: &str = "mind.discount";
pub const DISCOUNT_ENDED_FACT: &str = "mind.discount_ended";

/// Every fact kind the live layer records, all with `Cause::Mind`, all about one station.
pub const LIVE_FACTS: [&str; 5] = [
    BANTER_FACT,
    TRAFFIC_FACT,
    BRAG_FACT,
    DISCOUNT_FACT,
    DISCOUNT_ENDED_FACT,
];

/// The fact kind an accepted flourish records.
pub fn fact_kind(purpose: Purpose) -> Option<&'static str> {
    match purpose {
        Purpose::Banter => Some(BANTER_FACT),
        Purpose::Traffic => Some(TRAFFIC_FACT),
        Purpose::Brag => Some(BRAG_FACT),
        Purpose::Discount => Some(DISCOUNT_FACT),
        _ => None,
    }
}

/// One thing offered to the model: its ID, its name, and a few words about it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Offer {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub about: String,
}

/// The closed world for one flourish at one station.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Closed {
    pub station: Offer,
    pub keepers: Vec<Offer>,
    /// The market's keeper, who announces a discount.
    pub market_keeper: Option<Offer>,
    pub craft: Vec<Offer>,
    pub wares: Vec<Offer>,
    /// Wares the station's market sells (a discount's choices).
    pub market_wares: Vec<Offer>,
    pub stations: Vec<Offer>,
}

impl Closed {
    /// Can `purpose` be asked for here?
    pub fn offers(&self, purpose: Purpose) -> bool {
        match purpose {
            Purpose::Banter => !self.keepers.is_empty(),
            Purpose::Traffic => !self.craft.is_empty() && self.stations.len() > 1,
            Purpose::Brag => !self.craft.is_empty() && !self.wares.is_empty(),
            Purpose::Discount => self.market_keeper.is_some() && !self.market_wares.is_empty(),
            _ => false,
        }
    }

    fn ids(list: &[Offer]) -> Vec<&str> {
        list.iter().map(|o| o.id.as_str()).collect()
    }

    /// Every name offered, for the "no names from outside" check.
    fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(&self.station)
            .chain(&self.keepers)
            .chain(&self.craft)
            .chain(&self.wares)
            .chain(&self.stations)
            .map(|o| o.name.as_str())
    }
}

fn forbidden_name(name: &str, forbidden: &[String]) -> bool {
    let lower = name.to_lowercase();
    forbidden
        .iter()
        .any(|t| !t.is_empty() && lower.contains(&t.to_lowercase()))
}

/// The closed world at `station`, from the world as it is. `None` if the station
/// doesn't exist.
pub fn closed_world(
    world: &WorldState,
    config: &MindConfig,
    station: &StationId,
) -> Option<Closed> {
    let st = world.stations.get(station)?;
    let names = Names(world);
    let forbidden = &config.talk.forbidden;
    let person = |npc: &NpcId| -> Offer {
        let brief = config.brief(npc.as_str());
        Offer {
            id: npc.to_string(),
            name: names.of(npc.as_str()),
            about: brief.map_or_else(
                || {
                    world
                        .npcs
                        .get(npc)
                        .map_or_else(String::new, |n| n.role.clone())
                },
                |b| format!("{}. {}", b.role, b.voice),
            ),
        }
    };
    let parts: Vec<_> = world
        .station_parts
        .values()
        .filter(|p| &p.station == station)
        .filter(|p| world.npcs.contains(&p.keeper))
        .collect();
    let mut keepers: Vec<Offer> = Vec::new();
    for p in &parts {
        if !keepers.iter().any(|k| k.id == p.keeper.as_str()) {
            keepers.push(person(&p.keeper));
        }
    }
    let market_keeper = parts
        .iter()
        .find(|p| p.room == RoomKind::Market)
        .map(|p| person(&p.keeper));
    let ware = |id: &WareId| -> Option<Offer> {
        let w = world.wares.get(id)?;
        (!forbidden_name(&w.name, forbidden)).then(|| Offer {
            id: id.to_string(),
            name: w.name.clone(),
            about: String::new(),
        })
    };
    let wares: Vec<Offer> = world.wares.keys().filter_map(ware).collect();
    // A discount is on what the station sells: lowering what it pays for a ware it buys
    // would cut the player's takings.
    let market_wares: Vec<Offer> = world
        .markets
        .get(station)
        .map(|m| {
            m.lines
                .iter()
                .filter(|(_, l)| l.role == MarketRole::Sells)
                .filter_map(|(w, _)| ware(w))
                .collect()
        })
        .unwrap_or_default();
    // Craft at this station first, then the rest, in ID order.
    let mut ships: Vec<&Ship> = world
        .ships
        .values()
        .filter(|s| !matches!(s.owner, Owner::Player))
        .filter(|s| !forbidden_name(&s.name, forbidden))
        .collect();
    ships.sort_by_key(|s| (s.docked_at.as_ref() != Some(station), s.id.clone()));
    let craft = ships
        .into_iter()
        .take(config.live.max_craft)
        .map(|s| Offer {
            id: s.id.to_string(),
            name: s.name.clone(),
            about: config.brief(s.id.as_str()).map_or_else(
                || match &s.owner {
                    Owner::Faction { faction } => names.of(faction.as_str()),
                    _ => String::new(),
                },
                |b| format!("{}. {}", b.role, b.voice),
            ),
        })
        .collect();
    let stations = world
        .stations
        .values()
        .map(|s| Offer {
            id: s.id.to_string(),
            name: s.name.clone(),
            about: String::new(),
        })
        .collect();
    Some(Closed {
        station: Offer {
            id: station.to_string(),
            name: st.name.clone(),
            about: st
                .owner
                .as_ref()
                .map_or_else(String::new, |f| names.of(f.as_str())),
        },
        keepers,
        market_keeper,
        craft,
        wares,
        market_wares,
        stations,
    })
}

/// The system prompt: the rules every flourish keeps.
pub fn system_prompt(rules: &LiveRules, forbidden: &[String]) -> String {
    let mut s = format!(
        "You write one small flourish of everyday life at a station in the Lantern Reach, a \
         calm frontier of mining and trading stations. Answer with one JSON object that \
         matches answer_schema and nothing else. Use only the IDs offered in the message; never \
         invent people, ships, places or wares. \"text\" is one short line of at most {} \
         words, said by the one speaking in their own words and in the first person: it \
         must use I, me, my, we, us or our at least once. Plain English. No digits or \
         numbers, no quotation marks, no links, no emoji, no line breaks. Keep it small and \
         local: work, weather in the lanes, cargo, food, gossip. No battles, deaths, \
         disasters, mysteries or relics, and nothing from outside this world: no real \
         places, brands, films, books or games.",
        rules.max_words
    );
    if !forbidden.is_empty() {
        s.push_str(&format!(" Never mention: {}.", forbidden.join(", ")));
    }
    s
}

fn enum_of(ids: Vec<&str>) -> Value {
    json!({ "type": "string", "enum": ids })
}

/// The task, the closed world and the answer schema for `purpose`.
pub fn task(purpose: Purpose, closed: &Closed, rules: &LiveRules) -> (Value, Value) {
    let text = json!({ "type": "string" });
    let (task, props, required): (String, Value, Vec<&str>) = match purpose {
        Purpose::Banter => (
            "One of the keepers says a line of everyday banter to a pilot who has just \
             docked. Pick the speaker and write the line in their voice."
                .into(),
            json!({ "speaker": enum_of(Closed::ids(&closed.keepers)), "text": text }),
            vec!["speaker", "text"],
        ),
        Purpose::Traffic => (
            "One of the craft hails the station as it passes, saying where it is bound and \
             with what. Pick the craft, a station it is bound for (not this one) and a ware, \
             and write its hail in its captain's voice."
                .into(),
            json!({
                "craft": enum_of(Closed::ids(&closed.craft)),
                "bound_for": enum_of(
                    Closed::ids(&closed.stations)
                        .into_iter()
                        .filter(|s| *s != closed.station.id)
                        .collect(),
                ),
                "ware": enum_of(Closed::ids(&closed.wares)),
                "text": text,
            }),
            vec!["craft", "bound_for", "ware", "text"],
        ),
        Purpose::Brag => (
            "A trader at the station brags about a recent haul of one ware. Pick the craft \
             and the ware, and write the brag in its captain's voice. Name no amounts or \
             prices."
                .into(),
            json!({
                "craft": enum_of(Closed::ids(&closed.craft)),
                "ware": enum_of(Closed::ids(&closed.wares)),
                "text": text,
            }),
            vec!["craft", "ware", "text"],
        ),
        _ => {
            let d = &rules.discount;
            (
                format!(
                    "The market keeper takes a little off the price of one ware sold here for a \
                     few dockings, and says why in one line in their own voice (the game shows \
                     the numbers, so the line names none). Pick the ware, a percent from {} to \
                     {} and a number of dockings from {} to {}.",
                    d.min_percent, d.max_percent, d.min_dockings, d.max_dockings
                ),
                json!({
                    "ware": enum_of(Closed::ids(&closed.market_wares)),
                    "percent": { "type": "integer" },
                    "dockings": { "type": "integer" },
                    "text": text,
                }),
                vec!["ware", "percent", "dockings", "text"],
            )
        }
    };
    let mut user = json!({
        "task": task,
        "station": closed.station,
        "keepers": closed.keepers,
        "craft": closed.craft,
        "wares": closed.wares,
        "stations": closed.stations,
    });
    if purpose == Purpose::Discount {
        user["keepers"] = json!([closed.market_keeper]);
        user["wares"] = json!(closed.market_wares);
        if let Some(o) = user.as_object_mut() {
            o.remove("craft");
        }
    }
    if let Some(o) = user.as_object_mut() {
        match purpose {
            Purpose::Banter => {
                o.remove("craft");
            }
            Purpose::Traffic | Purpose::Brag => {
                o.remove("keepers");
            }
            _ => {}
        }
    }
    let schema = json!({
        "type": "object",
        "properties": props,
        "required": required,
        "additionalProperties": false,
    });
    // In the message too: the router's model may not be sent `response_format`.
    user["answer_schema"] = schema.clone();
    (user, schema)
}

/// A flourish that passed validation.
#[derive(Clone, Debug, PartialEq)]
pub enum Flourish {
    Banter {
        speaker: NpcId,
        text: String,
    },
    Traffic {
        craft: ShipId,
        bound_for: StationId,
        ware: WareId,
        text: String,
    },
    Brag {
        craft: ShipId,
        ware: WareId,
        text: String,
    },
    Discount {
        ware: WareId,
        keeper: NpcId,
        percent: u32,
        dockings: u32,
        /// The model's numbers were outside the bound and were clamped into it.
        clamped: bool,
        text: String,
    },
}

impl Flourish {
    pub fn purpose(&self) -> Purpose {
        match self {
            Flourish::Banter { .. } => Purpose::Banter,
            Flourish::Traffic { .. } => Purpose::Traffic,
            Flourish::Brag { .. } => Purpose::Brag,
            Flourish::Discount { .. } => Purpose::Discount,
        }
    }

    pub fn text(&self) -> &str {
        match self {
            Flourish::Banter { text, .. }
            | Flourish::Traffic { text, .. }
            | Flourish::Brag { text, .. }
            | Flourish::Discount { text, .. } => text,
        }
    }
}

/// What a line is checked against.
pub struct TextRules<'a> {
    pub max_chars: u32,
    pub forbidden: &'a [String],
    pub markers: &'a [String],
    /// Words a capitalised word mid-sentence may be part of: every name in the world
    /// and in the closed list.
    pub names: &'a BTreeSet<String>,
}

const FIRST_PERSON: &[&str] = &[
    "i",
    "i'm",
    "i’m",
    "i've",
    "i’ve",
    "i'll",
    "i’ll",
    "i'd",
    "i’d",
    "me",
    "my",
    "mine",
    "myself",
    "we",
    "we're",
    "we’re",
    "we've",
    "we’ve",
    "we'll",
    "we’ll",
    "we'd",
    "we’d",
    "us",
    "our",
    "ours",
    "ourselves",
];

/// One short line in the first person, or why not. Messages never quote the text.
pub fn check_text(raw: &str, rules: &TextRules<'_>) -> Result<String, Invalid> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(Invalid::Text("empty".into()));
    }
    if text
        .chars()
        .any(|c| c == '\n' || c == '\r' || c.is_control())
    {
        return Err(Invalid::Text("more than one line".into()));
    }
    if text.chars().count() > rules.max_chars as usize {
        return Err(Invalid::Limits(format!(
            "longer than {} characters",
            rules.max_chars
        )));
    }
    if let Some(c) = text.chars().find(|c| !english(*c)) {
        return Err(Invalid::Text(format!("unexpected character {c:?}")));
    }
    if text.chars().any(|c| c.is_ascii_digit()) {
        return Err(Invalid::Text(
            "digits (numbers are the game's to state)".into(),
        ));
    }
    if text.contains(['"', '“', '”', '{', '}', '<', '>', '#', '@', '*', '_', '`']) {
        return Err(Invalid::Text("quotes or markup".into()));
    }
    let lower = text.to_lowercase();
    if lower.contains("fact.") || lower.contains("npc.") || lower.contains("ware.") {
        return Err(Invalid::Text("IDs in the text".into()));
    }
    if let Some(t) = rules
        .forbidden
        .iter()
        .find(|t| !t.is_empty() && lower.contains(&t.to_lowercase()))
    {
        return Err(Invalid::Text(format!("forbidden term `{t}`")));
    }
    if rules
        .markers
        .iter()
        .any(|m| !m.is_empty() && lower.contains(&m.to_lowercase()))
    {
        return Err(Invalid::Text("out-of-world marker".into()));
    }
    let words = tokens(text);
    if !words.iter().any(|w| FIRST_PERSON.contains(&w.as_str())) {
        return Err(Invalid::Rules("not in the first person".into()));
    }
    // No names from outside the world: a capitalised word that doesn't start a sentence
    // must be part of a name the world knows.
    let mut sentence_start = true;
    for raw in text.split_whitespace() {
        let w: String = raw
            .trim_matches(|c: char| !(c.is_alphanumeric() || c == '\'' || c == '’'))
            .to_owned();
        let starts = sentence_start;
        sentence_start = raw.ends_with(['.', '!', '?', ':', ';', '…']);
        if starts || w.is_empty() || w == "I" || w.starts_with("I'") || w.starts_with("I’") {
            continue;
        }
        if !w.chars().next().is_some_and(char::is_uppercase) {
            continue;
        }
        if !tokens(&w).iter().all(|p| rules.names.contains(p)) {
            return Err(Invalid::Text("a name from outside the world".into()));
        }
    }
    Ok(text.to_owned())
}

/// Name words for [`TextRules::names`]: every display name in the world and the list.
pub fn name_words(world: &WorldState, closed: &Closed) -> BTreeSet<String> {
    Names(world)
        .all()
        .iter()
        .map(String::as_str)
        .chain(closed.names())
        .flat_map(tokens)
        .collect()
}

fn pick<'a>(v: &'a Value, field: &str, list: &[Offer]) -> Result<&'a str, Invalid> {
    let id = v
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| Invalid::Shape(format!("`{field}` is missing or not a string")))?;
    if list.iter().any(|o| o.id == id) {
        Ok(id)
    } else {
        Err(Invalid::Reference(format!(
            "`{field}` is not one of the IDs offered"
        )))
    }
}

fn bad_id(_: galaxy_contract::ids::IdError) -> Invalid {
    Invalid::Shape("not a well-formed ID".into())
}

/// A whole number, clamped into `lo..=hi`. Not a number at all: a shape error.
fn clamp_number(v: &Value, field: &str, lo: u32, hi: u32) -> Result<(u32, bool), Invalid> {
    let n = v
        .get(field)
        .and_then(Value::as_f64)
        .filter(|n| n.is_finite())
        .ok_or_else(|| Invalid::Shape(format!("`{field}` is missing or not a number")))?;
    let r = libm::round(n);
    let c = r.clamp(f64::from(lo), f64::from(hi));
    Ok((c as u32, c != n))
}

/// Validates and clamps a live answer against the closed world it was offered.
pub fn validate(
    purpose: Purpose,
    draft: &LiveDraft,
    closed: &Closed,
    rules: &LiveRules,
    text: &TextRules<'_>,
) -> Result<Flourish, Invalid> {
    let v = &draft.value;
    if !v.is_object() {
        return Err(Invalid::Shape("not a JSON object".into()));
    }
    let line = || {
        let raw = v
            .get("text")
            .and_then(Value::as_str)
            .ok_or_else(|| Invalid::Shape("`text` is missing or not a string".into()))?;
        check_text(raw, text)
    };
    match purpose {
        Purpose::Banter => {
            let speaker = pick(v, "speaker", &closed.keepers)?;
            Ok(Flourish::Banter {
                speaker: NpcId::new(speaker).map_err(bad_id)?,
                text: line()?,
            })
        }
        Purpose::Traffic => {
            let craft = pick(v, "craft", &closed.craft)?;
            let bound_for = pick(v, "bound_for", &closed.stations)?;
            if bound_for == closed.station.id {
                return Err(Invalid::Rules("bound for the station it is hailing".into()));
            }
            let ware = pick(v, "ware", &closed.wares)?;
            Ok(Flourish::Traffic {
                craft: ShipId::new(craft).map_err(bad_id)?,
                bound_for: StationId::new(bound_for).map_err(bad_id)?,
                ware: WareId::new(ware).map_err(bad_id)?,
                text: line()?,
            })
        }
        Purpose::Brag => {
            let craft = pick(v, "craft", &closed.craft)?;
            let ware = pick(v, "ware", &closed.wares)?;
            Ok(Flourish::Brag {
                craft: ShipId::new(craft).map_err(bad_id)?,
                ware: WareId::new(ware).map_err(bad_id)?,
                text: line()?,
            })
        }
        Purpose::Discount => {
            let keeper = closed
                .market_keeper
                .as_ref()
                .ok_or_else(|| Invalid::Reference("no market keeper here".into()))?;
            let ware = pick(v, "ware", &closed.market_wares)?;
            let d = &rules.discount;
            let (percent, p_clamped) = clamp_number(v, "percent", d.min_percent, d.max_percent)?;
            let (dockings, d_clamped) =
                clamp_number(v, "dockings", d.min_dockings, d.max_dockings)?;
            Ok(Flourish::Discount {
                ware: WareId::new(ware).map_err(bad_id)?,
                keeper: NpcId::new(&keeper.id).map_err(bad_id)?,
                percent,
                dockings,
                clamped: p_clamped || d_clamped,
                text: line()?,
            })
        }
        _ => Err(Invalid::Shape("not a live purpose".into())),
    }
}

/// The note on a live fact: everything needed to say its line and, for a discount, to
/// end it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LiveNote {
    pub purpose: Purpose,
    /// The request key (empty for a discount's end).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub key: String,
    /// The player's docking count when it was asked for (or ended).
    pub docking: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<NpcId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub craft: Option<ShipId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bound_for: Option<StationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ware: Option<WareId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dockings: Option<u32>,
    /// The player's docking count at which a discount ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until: Option<u64>,
    /// A discount's base price (centicredits) before and after.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_before: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_after: Option<u32>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub clamped: bool,
    /// The discount a `mind.discount_ended` ends.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ends: Option<FactId>,
}

impl LiveNote {
    pub fn parse(fact: &Fact) -> Option<Self> {
        if !LIVE_FACTS.contains(&fact.kind.as_str()) {
            return None;
        }
        match &fact.payload {
            FactPayload::Note(n) => serde_json::from_str(n).ok(),
            FactPayload::Event(_) => None,
        }
    }
}

/// A discount that is running.
#[derive(Clone, Debug, PartialEq)]
pub struct Running {
    pub fact: FactId,
    pub station: StationId,
    pub note: LiveNote,
}

/// Discounts running, folded from history.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Discounts {
    pub running: BTreeMap<FactId, Running>,
}

impl Discounts {
    pub fn observe(&mut self, fact: &Fact) {
        let Some(note) = LiveNote::parse(fact) else {
            return;
        };
        match fact.kind.as_str() {
            DISCOUNT_FACT => {
                if let Some(station) = fact.subjects.station.clone() {
                    self.running.insert(
                        fact.id,
                        Running {
                            fact: fact.id,
                            station,
                            note,
                        },
                    );
                }
            }
            DISCOUNT_ENDED_FACT => {
                if let Some(of) = note.ends {
                    self.running.remove(&of);
                }
            }
            _ => {}
        }
    }

    pub fn at(&self, station: &StationId) -> bool {
        self.running.values().any(|r| &r.station == station)
    }
}

/// The player's counted dockings.
pub fn dockings(world: &WorldState) -> u64 {
    world
        .counters
        .get(CounterKind::Dockings, &CounterScope::Player)
}

/// A calm moment for a lever: the flagship docked.
pub fn calm(world: &WorldState) -> bool {
    world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .is_some_and(|s| s.docked_at.is_some())
}

fn market_part_news(world: &WorldState, station: &StationId) -> Option<Command> {
    let part = world
        .station_parts
        .values()
        .find(|p| &p.station == station && p.room == RoomKind::Market)?;
    let mut part = part.clone();
    part.news = Some(PartNews {
        ribbon: Ribbon::Changed,
        docking: dockings(world),
    });
    Some(Command::PutStationPart {
        id: part.id.clone(),
        value: Some(Box::new(part)),
    })
}

fn record(kind: &'static str, subjects: Subjects, note: &LiveNote) -> Command {
    Command::RecordFact {
        fact: Box::new(FactDraft {
            kind: FactKind::from_static(kind),
            subjects,
            amount: None,
            note: serde_json::to_string(note).expect("notes serialize"),
        }),
    }
}

/// The batch an accepted flourish becomes: its fact (and, for a discount, the market
/// lever and the ribbon), with `Cause::Mind`, citing the docking that asked for it.
/// `Err` if a discount can't apply to the world as it is.
#[allow(clippy::too_many_arguments)]
pub fn accepted(
    world: &WorldState,
    station: &StationId,
    flourish: &Flourish,
    key: &str,
    model: &str,
    asked_at: u64,
    trigger: FactId,
) -> Result<CommandBatch, String> {
    let names = Names(world);
    let region = world.stations.get(station).map(|s| s.region.clone());
    let mut note = LiveNote {
        purpose: flourish.purpose(),
        key: key.to_owned(),
        docking: asked_at,
        model: model.to_owned(),
        text: flourish.text().to_owned(),
        speaker: None,
        craft: None,
        bound_for: None,
        ware: None,
        percent: None,
        dockings: None,
        until: None,
        base_before: None,
        base_after: None,
        clamped: false,
        ends: None,
    };
    let mut subjects = Subjects {
        station: Some(station.clone()),
        region,
        ..Subjects::default()
    };
    let mut commands = Vec::new();
    let (npc, summary, subject) = match flourish {
        Flourish::Banter { speaker, text } => {
            note.speaker = Some(speaker.clone());
            subjects.npc = Some(speaker.clone());
            (
                Some(speaker.clone()),
                format!("{}: {text}", names.of(speaker.as_str())),
                speaker.to_string(),
            )
        }
        Flourish::Traffic {
            craft,
            bound_for,
            ware,
            text,
        } => {
            note.craft = Some(craft.clone());
            note.bound_for = Some(bound_for.clone());
            note.ware = Some(ware.clone());
            subjects.ship = Some(craft.clone());
            subjects.ware = Some(ware.clone());
            (
                None,
                format!("{}: {text}", names.of(craft.as_str())),
                craft.to_string(),
            )
        }
        Flourish::Brag { craft, ware, text } => {
            note.craft = Some(craft.clone());
            note.ware = Some(ware.clone());
            subjects.ship = Some(craft.clone());
            subjects.ware = Some(ware.clone());
            (
                None,
                format!("{}: {text}", names.of(craft.as_str())),
                craft.to_string(),
            )
        }
        Flourish::Discount {
            ware,
            keeper,
            percent,
            dockings: lasts,
            clamped,
            ..
        } => {
            let line = world
                .markets
                .get(station)
                .and_then(|m| m.lines.get(ware))
                .filter(|l| l.role == MarketRole::Sells)
                .ok_or("the market no longer sells that ware")?;
            let factor = 1.0 - f64::from(*percent) / 100.0;
            let lever = lever_commands(
                world,
                &Lever::ScaleBase {
                    station: station.clone(),
                    ware: ware.clone(),
                    factor,
                },
            )
            .map_err(|e| e.to_string())?;
            let after = lever.iter().find_map(|c| match c {
                Command::SetMarketBase { base, .. } => Some(base.base_price),
                _ => None,
            });
            note.speaker = Some(keeper.clone());
            note.ware = Some(ware.clone());
            note.percent = Some(*percent);
            note.dockings = Some(*lasts);
            note.until = Some(dockings(world) + u64::from(*lasts));
            note.base_before = Some(line.base_price);
            note.base_after = after;
            note.clamped = *clamped;
            subjects.npc = Some(keeper.clone());
            subjects.ware = Some(ware.clone());
            commands.extend(lever);
            commands.extend(market_part_news(world, station));
            (
                Some(keeper.clone()),
                discount_notice(world, station, ware, *percent, *lasts),
                ware.to_string(),
            )
        }
    };
    let kind = fact_kind(flourish.purpose()).expect("a live purpose");
    commands.push(record(kind, subjects, &note));
    Ok(CommandBatch {
        cause: Cause {
            kind: CauseKind::Mind {
                npc,
                decision: format!("{}:{subject}", flourish.purpose().menu()),
            },
            summary,
            cites: vec![trigger],
        },
        commands,
    })
}

/// "Brassmouth Exchange: 10% off glassite for 3 dockings".
pub fn discount_notice(
    world: &WorldState,
    station: &StationId,
    ware: &WareId,
    percent: u32,
    lasts: u32,
) -> String {
    let n = Names(world);
    format!(
        "{}: {percent}% off {} for {lasts} docking{}",
        n.of(station.as_str()),
        n.of(ware.as_str()).to_lowercase(),
        if lasts == 1 { "" } else { "s" }
    )
}

/// The batch that ends a running discount: the base price goes back (unless something
/// else has moved it since, which then stands), the market's ribbon says Changed, and
/// a `mind.discount_ended` fact records it.
pub fn end(world: &WorldState, running: &Running) -> CommandBatch {
    let station = &running.station;
    let ware = running.note.ware.clone();
    let mut commands = Vec::new();
    if let (Some(ware), Some(before), Some(after)) =
        (&ware, running.note.base_before, running.note.base_after)
        && world
            .markets
            .get(station)
            .and_then(|m| m.lines.get(ware))
            .is_some_and(|l| l.base_price == after)
        && let Ok(lever) = lever_commands(
            world,
            &Lever::SetBase {
                station: station.clone(),
                ware: ware.clone(),
                base_price: before,
            },
        )
    {
        commands.extend(lever);
        commands.extend(market_part_news(world, station));
    }
    let note = LiveNote {
        purpose: Purpose::Discount,
        key: String::new(),
        docking: dockings(world),
        model: String::new(),
        text: String::new(),
        ends: Some(running.fact),
        speaker: running.note.speaker.clone(),
        ware: ware.clone(),
        craft: None,
        bound_for: None,
        percent: running.note.percent,
        dockings: running.note.dockings,
        until: running.note.until,
        base_before: running.note.base_before,
        base_after: running.note.base_after,
        clamped: false,
    };
    commands.push(record(
        DISCOUNT_ENDED_FACT,
        Subjects {
            station: Some(station.clone()),
            ware: ware.clone(),
            npc: running.note.speaker.clone(),
            region: world.stations.get(station).map(|s| s.region.clone()),
            ..Subjects::default()
        },
        &note,
    ));
    let n = Names(world);
    CommandBatch {
        cause: Cause {
            kind: CauseKind::Mind {
                npc: running.note.speaker.clone(),
                decision: format!(
                    "live.discount_ended:{}",
                    ware.as_ref().map_or("", |w| w.as_str())
                ),
            },
            summary: format!(
                "{}: the {} discount has ended",
                n.of(station.as_str()),
                ware.as_ref()
                    .map_or_else(String::new, |w| n.of(w.as_str()).to_lowercase())
            ),
            cites: vec![running.fact],
        },
        commands,
    }
}

/// The lines a live fact is said with, citing it: banter as the keeper's dock talk;
/// traffic and brags as notices about the station; a discount as a notice and the
/// keeper's line; a discount's end as a notice.
pub fn say(world: &WorldState, fact: &Fact) -> Vec<CommandBatch> {
    let Some(note) = LiveNote::parse(fact) else {
        return Vec::new();
    };
    let Some(station) = fact.subjects.station.clone() else {
        return Vec::new();
    };
    let n = Names(world);
    let line = |channel, speaker: Option<NpcId>, text: String| TalkLine {
        channel,
        speaker,
        place: Some(station.clone()),
        text,
        cites: vec![fact.id],
        about: Vec::new(),
        choices: Vec::new(),
    };
    let mut lines = Vec::new();
    match fact.kind.as_str() {
        BANTER_FACT => lines.push(line(
            Channel::DockTalk,
            note.speaker.clone(),
            note.text.clone(),
        )),
        TRAFFIC_FACT => {
            let craft = note
                .craft
                .as_ref()
                .map_or_else(String::new, |c| n.of(c.as_str()));
            let to = note
                .bound_for
                .as_ref()
                .map_or_else(String::new, |s| n.of(s.as_str()));
            lines.push(line(
                Channel::MapNote,
                None,
                format!("{craft}, bound for {to}: \"{}\"", note.text),
            ));
        }
        BRAG_FACT => {
            let craft = note
                .craft
                .as_ref()
                .map_or_else(String::new, |c| n.of(c.as_str()));
            lines.push(line(
                Channel::MapNote,
                None,
                format!("{craft}: \"{}\"", note.text),
            ));
        }
        DISCOUNT_FACT => {
            if let (Some(ware), Some(p), Some(d)) = (&note.ware, note.percent, note.dockings) {
                lines.push(line(
                    Channel::MapNote,
                    None,
                    discount_notice(world, &station, ware, p, d),
                ));
            }
            if note.speaker.is_some() && !note.text.is_empty() {
                lines.push(line(
                    Channel::DockTalk,
                    note.speaker.clone(),
                    note.text.clone(),
                ));
            }
        }
        DISCOUNT_ENDED_FACT => {
            let ware = note
                .ware
                .as_ref()
                .map_or_else(String::new, |w| n.of(w.as_str()).to_lowercase());
            lines.push(line(
                Channel::MapNote,
                None,
                format!("{}: the {ware} discount has ended", n.of(station.as_str())),
            ));
        }
        _ => {}
    }
    lines
        .into_iter()
        .filter(|l| l.validate().is_ok())
        .map(|l| CommandBatch {
            cause: Cause {
                kind: CauseKind::Mind {
                    npc: l.speaker.clone(),
                    decision: format!("{}:say", note.purpose.menu()),
                },
                summary: format!("{} says a live line", fact.kind),
                cites: vec![fact.id],
            },
            commands: vec![Command::Say { line: Box::new(l) }],
        })
        .collect()
}

/// Has any talk line cited `fact` yet (so it was said)?
pub fn said(world: &WorldState, fact: FactId) -> bool {
    world
        .talk
        .lines
        .values()
        .any(|l| l.line.cites.contains(&fact))
}
