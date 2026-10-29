//! Grounding packets: what a model may know about one moment.
//!
//! A packet holds the speaker's brief, the state around them with stable IDs, the few
//! history facts that matter (by ID), and the options. A model may only refer to what
//! is in it; [`crate::validate`] checks that.

use std::collections::{BTreeMap, BTreeSet};

use galaxy_contract::money::show_credits;
use galaxy_contract::prelude::*;
use galaxy_core::History;
use serde::Serialize;

use crate::config::{Brief, Limits};
use crate::menu::Menu;
use crate::situation::OutcomeNote;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Packet {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub who: Option<BriefView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<StateItem>,
    pub state: Vec<StateItem>,
    pub facts: Vec<PacketFact>,
    pub options: Vec<PacketOption>,
}

/// The brief as the model sees it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BriefView {
    pub id: String,
    pub name: String,
    pub role: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub goals: Vec<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub temperament: BTreeMap<String, f32>,
    pub voice: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub off_limits: Vec<String>,
}

impl From<&Brief> for BriefView {
    fn from(b: &Brief) -> Self {
        Self {
            id: b.id.to_string(),
            name: b.name.clone(),
            role: b.role.clone(),
            goals: b.goals.clone(),
            temperament: b.temperament.clone(),
            voice: b.voice.clone(),
            off_limits: b.off_limits.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct StateItem {
    pub id: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PacketFact {
    pub id: FactId,
    pub kind: String,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PacketOption {
    pub id: String,
    pub text: String,
}

impl Packet {
    pub fn fact_ids(&self) -> Vec<FactId> {
        self.facts.iter().map(|f| f.id).collect()
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::to_value(self).expect("packets serialize")
    }

    /// What an answer depends on, for its situation key: the whole packet except the
    /// routine fact that opened the menu (a docking), whose ID changes every time.
    /// Place, state, remembered facts, brief and options all count. [`build`] leaves
    /// other routine facts and the layer's own outcomes out of the packet, so the same
    /// situation at a later docking has the same key.
    pub fn key_parts(&self, trigger: FactId) -> serde_json::Value {
        let mut p = self.clone();
        p.facts.retain(|f| f.id != trigger);
        p.to_value()
    }

    /// Every text in the packet a line may take names from: the speaker, the place,
    /// the state and the facts.
    pub fn texts(&self) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(w) = &self.who {
            out.push(w.name.clone());
            out.push(w.role.clone());
        }
        out.extend(self.place.iter().map(|p| p.text.clone()));
        out.extend(self.state.iter().map(|s| s.text.clone()));
        out.extend(self.facts.iter().map(|f| f.text.clone()));
        out
    }

    /// Each fact's text by ID.
    pub fn fact_texts(&self) -> BTreeMap<FactId, String> {
        self.facts.iter().map(|f| (f.id, f.text.clone())).collect()
    }
}

/// Display names for IDs, falling back to the ID.
pub struct Names<'a>(pub &'a WorldState);

impl Names<'_> {
    pub fn of(&self, id: &str) -> String {
        let w = self.0;
        let name = match id.split('.').next().unwrap_or_default() {
            "station" => w.stations.get(id).map(|x| x.name.clone()),
            "ware" => w.wares.get(id).map(|x| x.name.clone()),
            "npc" => w.npcs.get(id).map(|x| x.name.clone()),
            "faction" => w.factions.get(id).map(|x| x.name.clone()),
            "region" => w.regions.get(id).map(|x| x.name.clone()),
            "ship" | "craft" => w.ships.get(id).map(|x| x.name.clone()),
            "rumor" => w.rumors.get(id).map(|x| x.claim.clone()),
            _ => None,
        };
        name.unwrap_or_else(|| id.to_owned())
    }

    /// Every display name in the world, for the "no new names" text check.
    pub fn all(&self) -> BTreeSet<String> {
        let w = self.0;
        let mut out = BTreeSet::new();
        out.extend(w.stations.values().map(|x| x.name.clone()));
        out.extend(w.wares.values().map(|x| x.name.clone()));
        out.extend(w.npcs.values().map(|x| x.name.clone()));
        out.extend(w.factions.values().map(|x| x.name.clone()));
        out.extend(w.regions.values().map(|x| x.name.clone()));
        out.extend(w.ships.values().map(|x| x.name.clone()));
        out.extend(w.fields.values().map(|x| x.name.clone()));
        out.extend(w.hulls.values().map(|x| x.name.clone()));
        out.insert(w.player.name.clone());
        out
    }
}

/// One fact in words, with names.
pub fn describe(world: &WorldState, fact: &Fact) -> String {
    let n = Names(world);
    let text = match &fact.payload {
        FactPayload::Event(e) => match e {
            Event::Dock { ship, station } => {
                format!(
                    "{} docked at {}",
                    n.of(ship.as_str()),
                    n.of(station.as_str())
                )
            }
            Event::Undock { ship, station } => {
                format!(
                    "{} undocked from {}",
                    n.of(ship.as_str()),
                    n.of(station.as_str())
                )
            }
            Event::Mine {
                ship, ware, amount, ..
            } => format!(
                "{} mined {amount} {}",
                n.of(ship.as_str()),
                n.of(ware.as_str())
            ),
            Event::RockDepleted { rock, ship } => match ship {
                Some(s) => format!("{} mined out {rock}", n.of(s.as_str())),
                None => format!("{rock} was mined out"),
            },
            Event::Buy {
                ship,
                station,
                ware,
                quantity,
                unit_price,
            } => format!(
                "{} bought {quantity} {} at {} for {} cr each",
                n.of(ship.as_str()),
                n.of(ware.as_str()),
                n.of(station.as_str()),
                show_credits(i64::from(*unit_price))
            ),
            Event::Sell {
                ship,
                station,
                ware,
                quantity,
                unit_price,
            } => format!(
                "{} sold {quantity} {} at {} for {} cr each",
                n.of(ship.as_str()),
                n.of(ware.as_str()),
                n.of(station.as_str()),
                show_credits(i64::from(*unit_price))
            ),
            Event::Deliver {
                ship,
                station,
                ware,
                quantity,
            } => format!(
                "{} delivered {quantity} {} to {}",
                n.of(ship.as_str()),
                n.of(ware.as_str()),
                n.of(station.as_str())
            ),
            Event::Jump { ship, from, to, .. } => format!(
                "{} came through the lane from {} to {}",
                n.of(ship.as_str()),
                n.of(from.as_str()),
                n.of(to.as_str())
            ),
            other => format!("{}", serde_json::to_value(other).unwrap_or_default()),
        },
        FactPayload::Note(note) => match OutcomeNote::parse(fact) {
            Some(o) => match (&o.line, &o.choice) {
                (Some(l), _) => format!("{} said: \"{}\"", n.of(&o.actor), l.text),
                (None, Some(c)) => format!("{} chose {c} ({})", n.of(&o.actor), o.menu),
                (None, None) => format!("{} let the moment pass ({})", n.of(&o.actor), o.menu),
            },
            None => note.chars().take(200).collect(),
        },
    };
    text.chars().take(240).collect()
}

/// Builds the packet for `menu`. `speaker` is the brief that will decide or speak.
pub fn build(
    world: &WorldState,
    history: &History,
    menu: &Menu,
    speaker: Option<&Brief>,
    limits: &Limits,
) -> Packet {
    let n = Names(world);
    let place = menu.station.as_ref().and_then(|s| {
        let st = world.stations.get(s)?;
        let owner = st
            .owner
            .as_ref()
            .map_or(String::new(), |f| format!(", held by {}", n.of(f.as_str())));
        Some(StateItem {
            id: s.to_string(),
            text: format!("{} in {}{owner}", st.name, n.of(st.region.as_str())),
        })
    });

    let mut state = Vec::new();
    if let Some(ship) = menu.ship.as_ref().and_then(|s| world.ships.get(s)) {
        let hold: Vec<String> = ship
            .hold
            .cargo
            .iter()
            .filter(|(_, q)| **q > 0)
            .map(|(w, q)| format!("{q} {}", n.of(w.as_str())))
            .collect();
        let hold = if hold.is_empty() {
            "empty hold".to_owned()
        } else {
            hold.join(", ")
        };
        state.push(StateItem {
            id: ship.id.to_string(),
            text: format!("{} ({}): {hold}", ship.name, n.of(ship.hull.as_str())),
        });
    }
    if let Some(market) = menu.station.as_ref().and_then(|s| world.markets.get(s)) {
        for (ware, line) in &market.lines {
            state.push(StateItem {
                id: format!("{}/{ware}", market.station),
                text: format!(
                    "{}: price {} cr, stock {}",
                    n.of(ware.as_str()),
                    show_credits(i64::from(line.price)),
                    line.stock
                ),
            });
        }
    }

    // Facts: the cited ones always; then the newest about this ship, NPC or place that
    // say something happened. Routine comings and goings and the layer's own records
    // change at every docking and would make each one a new situation.
    let routine = |kind: &str| {
        kind.starts_with("mind.") && kind != crate::RUMOR_SPREAD_FACT
            || limits.routine_facts.iter().any(|r| r == kind)
    };
    let knows = speaker.map(|b| b.knows.as_slice()).unwrap_or_default();
    let known = |kind: &str| {
        knows.is_empty()
            || knows
                .iter()
                .any(|k| k == kind || kind.split_once('.').is_some_and(|(_, rest)| rest == k))
    };
    let about = |f: &Fact| {
        let s = &f.subjects;
        (menu.ship.is_some() && s.ship == menu.ship)
            || (menu.npc.is_some() && s.npc == menu.npc)
            || (menu.station.is_some() && s.station == menu.station)
    };
    let mut ids: BTreeSet<FactId> = menu.cites.iter().copied().collect();
    ids.insert(menu.trigger);
    let budget = limits.packet_facts.max(ids.len());
    for f in history.facts().iter().rev() {
        if ids.len() >= budget {
            break;
        }
        if f.id < menu.trigger && about(f) && known(f.kind.as_str()) && !routine(f.kind.as_str()) {
            ids.insert(f.id);
        }
    }
    let facts = ids
        .into_iter()
        .filter_map(|id| history.fact(id))
        .map(|f| PacketFact {
            id: f.id,
            kind: f.kind.to_string(),
            text: describe(world, f),
        })
        .collect();

    let options = menu
        .options
        .iter()
        .enumerate()
        .map(|(i, o)| PacketOption {
            id: crate::mind::wire_id(i),
            text: o.text.clone(),
        })
        .collect();

    let mut packet = Packet {
        who: speaker.map(BriefView::from),
        place,
        state,
        facts,
        options,
    };
    // Keep the packet small: drop the oldest uncited facts, then state, until it fits.
    let cited: BTreeSet<FactId> = menu.cites.iter().copied().chain([menu.trigger]).collect();
    while packet.to_value().to_string().len() > limits.packet_bytes {
        if let Some(i) = packet.facts.iter().position(|f| !cited.contains(&f.id)) {
            packet.facts.remove(i);
        } else if packet.state.pop().is_none() {
            break;
        }
    }
    packet
}
