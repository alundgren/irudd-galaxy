//! `galaxy gm digest`: what changed since a fact, grouped by place and person, citing
//! fact IDs. The game master's main read.
//!
//! Every fact after `since` lands in the group of its station (or, without one, its
//! region) and in the group of its person, as per-kind tallies with the newest fact
//! IDs to cite. Facts with neither go under `elsewhere`. On top: the changes made by
//! scripts, fast models and rollbacks in that span (by cause), and the lines said on
//! the legibility channels. `until` is the newest fact covered; pass it as `--since`
//! next time.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_core::SimState;
use serde::Serialize;

/// Newest fact IDs kept per kind in a group.
pub const CITES_PER_KIND: usize = 8;
/// Most changes and talk lines listed.
pub const MAX_ENTRIES: usize = 50;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Digest {
    pub since: Option<FactId>,
    /// The newest fact covered: the next digest's `--since`.
    pub until: Option<FactId>,
    pub tick: Tick,
    /// Facts after `since`.
    pub facts: usize,
    pub player: PlayerNow,
    /// By station, then regions with facts that name no station.
    pub places: Vec<Group>,
    /// By named person.
    pub people: Vec<Group>,
    /// Facts that name no place or person.
    pub elsewhere: Vec<KindTally>,
    /// Writes by scripts, fast models and rollbacks, newest last.
    pub changes: Vec<ChangeEntry>,
    /// Lines said on dock talk, map notes and the chronicle, newest last.
    pub talk: Vec<TalkEntry>,
}

/// Where the player is now.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PlayerNow {
    pub credits: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ship: Option<ShipId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<RegionId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub docked_at: Option<StationId>,
    pub hold: BTreeMap<WareId, u32>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Group {
    /// A station, region or NPC ID.
    pub id: String,
    pub name: String,
    /// `station`, `region` or `person`.
    pub kind: &'static str,
    /// Where a person is now, or a station's region.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    pub facts: usize,
    /// One line per kind, e.g. `sell x3, 42 units (fact.12, fact.31)`.
    pub summary: Vec<String>,
    pub kinds: Vec<KindTally>,
    /// Lines said here or by this person.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub talk: Vec<TalkId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct KindTally {
    pub kind: FactKind,
    pub count: usize,
    /// Sum of the facts' amounts, if any had one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<i64>,
    /// Per ware, when the facts name one.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub wares: BTreeMap<WareId, i64>,
    pub first: FactId,
    pub last: FactId,
    /// The newest fact IDs, oldest first.
    pub cites: Vec<FactId>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct ChangeEntry {
    pub cause: CauseId,
    pub tick: Tick,
    /// `script`, `mind`, `rollback` or `system`.
    pub kind: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pack: Option<PackId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    pub summary: String,
    /// Facts the cause cited.
    pub cites: Vec<FactId>,
    /// Facts it recorded.
    pub recorded: Vec<FactId>,
    /// For rollbacks, the causes undone.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub undoes: Vec<CauseId>,
    pub rolled_back: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct TalkEntry {
    pub id: TalkId,
    pub tick: Tick,
    pub channel: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<NpcId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<StationId>,
    pub text: String,
    pub cites: Vec<FactId>,
}

#[derive(Default)]
struct Acc {
    facts: usize,
    kinds: BTreeMap<FactKind, KindTally>,
}

impl Acc {
    fn add(&mut self, f: &Fact) {
        self.facts += 1;
        let k = self
            .kinds
            .entry(f.kind.clone())
            .or_insert_with(|| KindTally {
                kind: f.kind.clone(),
                count: 0,
                amount: None,
                wares: BTreeMap::new(),
                first: f.id,
                last: f.id,
                cites: Vec::new(),
            });
        k.count += 1;
        k.last = f.id;
        if let Some(a) = f.amount {
            k.amount = Some(k.amount.unwrap_or(0).saturating_add(a));
            if let Some(w) = &f.subjects.ware {
                let e = k.wares.entry(w.clone()).or_insert(0);
                *e = e.saturating_add(a);
            }
        }
        k.cites.push(f.id);
        if k.cites.len() > CITES_PER_KIND {
            k.cites.remove(0);
        }
    }

    fn summary(&self) -> Vec<String> {
        self.kinds
            .values()
            .map(|k| {
                let mut s = format!("{} x{}", k.kind, k.count);
                if !k.wares.is_empty() {
                    let wares: Vec<String> =
                        k.wares.iter().map(|(w, n)| format!("{n} {w}")).collect();
                    s.push_str(&format!(", {}", wares.join(", ")));
                } else if let Some(a) = k.amount {
                    s.push_str(&format!(", amount {a}"));
                }
                let cites: Vec<String> = k.cites.iter().map(ToString::to_string).collect();
                s.push_str(&format!(" ({})", cites.join(", ")));
                s
            })
            .collect()
    }
}

/// The digest of everything after `since` (everything, without one).
pub fn digest(state: &SimState, since: Option<FactId>) -> Digest {
    let world = &state.world;
    let history = &state.history;
    let q = HistoryQuery {
        after: since,
        ..HistoryQuery::default()
    };
    let facts = history.find(&q);
    let since_tick = since.and_then(|f| history.fact(f)).map(|f| f.tick);
    let since_cause = since.and_then(|f| history.fact(f)).map(|f| f.cause);

    let mut stations: BTreeMap<StationId, Acc> = BTreeMap::new();
    let mut regions: BTreeMap<RegionId, Acc> = BTreeMap::new();
    let mut people: BTreeMap<NpcId, Acc> = BTreeMap::new();
    let mut elsewhere = Acc::default();
    for f in &facts {
        let s = &f.subjects;
        let mut placed = false;
        if let Some(st) = &s.station {
            stations.entry(st.clone()).or_default().add(f);
            placed = true;
        } else if let Some(r) = &s.region {
            regions.entry(r.clone()).or_default().add(f);
            placed = true;
        }
        if let Some(n) = &s.npc {
            people.entry(n.clone()).or_default().add(f);
            placed = true;
        }
        if !placed {
            elsewhere.add(f);
        }
    }

    // Lines said in the span.
    let mut talk: Vec<TalkEntry> = world
        .talk
        .lines
        .values()
        .filter(|l| since_tick.is_none_or(|t| l.tick > t))
        .map(|l| TalkEntry {
            id: l.id,
            tick: l.tick,
            channel: l.line.channel.as_str(),
            speaker: l.line.speaker.clone(),
            place: l.line.place.clone(),
            text: l.line.text.clone(),
            cites: l.line.cites.clone(),
        })
        .collect();
    if talk.len() > MAX_ENTRIES {
        talk.drain(..talk.len() - MAX_ENTRIES);
    }

    let mut groups_places = Vec::new();
    for (id, acc) in &stations {
        let st = world.stations.get(id);
        groups_places.push(Group {
            id: id.to_string(),
            name: st.map_or_else(|| id.to_string(), |s| s.name.clone()),
            kind: "station",
            at: st.map(|s| s.region.to_string()),
            facts: acc.facts,
            summary: acc.summary(),
            kinds: acc.kinds.values().cloned().collect(),
            talk: talk
                .iter()
                .filter(|t| t.place.as_ref() == Some(id))
                .map(|t| t.id)
                .collect(),
        });
    }
    for (id, acc) in &regions {
        groups_places.push(Group {
            id: id.to_string(),
            name: world
                .regions
                .get(id)
                .map_or_else(|| id.to_string(), |r| r.name.clone()),
            kind: "region",
            at: None,
            facts: acc.facts,
            summary: acc.summary(),
            kinds: acc.kinds.values().cloned().collect(),
            talk: Vec::new(),
        });
    }
    // People who spoke but have no facts of their own still get a group.
    for t in &talk {
        if let Some(s) = &t.speaker {
            people.entry(s.clone()).or_default();
        }
    }
    let groups_people = people
        .iter()
        .map(|(id, acc)| {
            let npc = world.npcs.get(id);
            Group {
                id: id.to_string(),
                name: npc.map_or_else(|| id.to_string(), |n| n.name.clone()),
                kind: "person",
                at: npc.and_then(|n| n.location.as_ref().map(ToString::to_string)),
                facts: acc.facts,
                summary: acc.summary(),
                kinds: acc.kinds.values().cloned().collect(),
                talk: talk
                    .iter()
                    .filter(|t| t.speaker.as_ref() == Some(id))
                    .map(|t| t.id)
                    .collect(),
            }
        })
        .collect();

    // Changes: script, fast-model and rollback causes in the span.
    let mut recorded: BTreeMap<CauseId, Vec<FactId>> = BTreeMap::new();
    for f in &facts {
        recorded.entry(f.cause).or_default().push(f.id);
    }
    let mut changes: Vec<ChangeEntry> = history
        .causes()
        .values()
        .filter(|c| since_cause.is_none_or(|s| c.id > s))
        .filter_map(|c| {
            let (kind, pack, revision, undoes) = match &c.cause.kind {
                CauseKind::Script { pack, revision } => (
                    "script",
                    Some(pack.clone()),
                    Some(revision.clone()),
                    Vec::new(),
                ),
                CauseKind::Mind { .. } => ("mind", None, None, Vec::new()),
                CauseKind::Rollback { of } => ("rollback", None, None, of.clone()),
                CauseKind::System { name }
                    if name == crate::bridge::SYSTEM_NAME
                        || name == galaxy_script::rollback::SYSTEM_NAME
                        || name == galaxy_script::host::SYSTEM_NAME =>
                {
                    ("system", None, None, Vec::new())
                }
                _ => return None,
            };
            Some(ChangeEntry {
                cause: c.id,
                tick: c.tick,
                kind,
                pack,
                revision,
                summary: c.cause.summary.clone(),
                cites: c.cause.cites.clone(),
                recorded: recorded.get(&c.id).cloned().unwrap_or_default(),
                undoes,
                rolled_back: state.kernel.rolled_back.contains(&c.id),
            })
        })
        .collect();
    if changes.len() > MAX_ENTRIES {
        changes.drain(..changes.len() - MAX_ENTRIES);
    }

    let ship = world.player.ship.as_ref().and_then(|s| world.ships.get(s));
    Digest {
        since,
        until: facts.last().map(|f| f.id).or(since),
        tick: world.meta.tick,
        facts: facts.len(),
        player: PlayerNow {
            credits: show_credits(world.player.credits),
            ship: ship.map(|s| s.id.clone()),
            region: ship.map(|s| s.region.clone()),
            docked_at: ship.and_then(|s| s.docked_at.clone()),
            hold: ship.map(|s| s.hold.cargo.clone()).unwrap_or_default(),
        },
        places: groups_places,
        people: groups_people,
        elsewhere: elsewhere.kinds.into_values().collect(),
        changes,
        talk,
    }
}
