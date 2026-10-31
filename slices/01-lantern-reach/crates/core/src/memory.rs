//! World memory (issue #47, 4.4): short summaries of what happened, per station,
//! person and faction, kept in the save as a derived index with a full-text search.
//!
//! [`summarize`] reads a state's history, its talk log and its journal, and writes
//! one [`Entry`] for each thing worth remembering:
//!
//! - the player's trades, rolled up per visit per station and ware ("Sold 20 ice at
//!   Kettle Dock for 240 cr"), once the visit ends (the flagship undocks);
//! - each delivery, beat started or completed, part fitted, tender built, standing or
//!   regard change, trait, rank and trade-up;
//! - each happening's start and end, each stage built, and every other fact a pack
//!   records in its own namespace (bookkeeping kinds such as `core.npc_line` aside);
//! - each line said: talk with a speaker, and notices (confirmations, map notes, the
//!   chronicle).
//!
//! An entry has a stable ID (`mem.fact.N` for the first fact it summarises,
//! `mem.talk.N` for a line), its subjects (stations, people, factions), when it
//! happened (tick, and the player's counted dockings so far), the facts it summarises
//! and its source, from the cause: the core pack's scripts and data, the player's
//! verbs and the engine are `authored`, a game-master layer's packs (and rollbacks)
//! `game_master`, and `Cause::Mind` the `live_layer`. A happening's facts take the
//! source of the pack that wrote it.
//!
//! The save keeps the entries in `memory_entry` with `memory_subject` linking each to
//! its subjects and an FTS5 table, `memory_fts`, over the text ([`search`],
//! [`list`]). [`Save::write`](crate::Save::write) syncs them: entries only ever
//! append (an open visit's trades wait for the undock), so a write inserts the new
//! ones, and rebuilds them all when the stored ones no longer match (a restored
//! snapshot, a rollback that took a line back, a new summary format). Opening a save
//! without the tables builds them. The tables are never read by the sim, so
//! determinism and replays don't depend on them.
//!
//! `memory_embedding` (entry ID, model, dims, vector) is there for a vector index
//! (sqlite-vec) later, keyed by the stable entry ID. Nothing fills it yet.

use std::collections::BTreeMap;

use galaxy_contract::happening::{ENDED_FACT, STARTED_FACT};
use galaxy_contract::history::{Fact, FactPayload};
use galaxy_contract::pack::PackLayer;
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, SaidLine};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::save::{Save, SaveError};
use crate::sim::SimState;

/// Bump when [`summarize`]'s output changes: saves rebuild their entries on the next
/// write.
pub const MEMORY_VERSION: u32 = 1;

pub(crate) const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS memory_meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS memory_entry (
    seq INTEGER PRIMARY KEY,
    id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL,
    tick INTEGER NOT NULL,
    docking INTEGER NOT NULL,
    source TEXT NOT NULL,
    text TEXT NOT NULL,
    facts TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS memory_entry_source ON memory_entry (source, tick);
CREATE TABLE IF NOT EXISTS memory_subject (
    kind TEXT NOT NULL,
    subject TEXT NOT NULL,
    entry INTEGER NOT NULL,
    PRIMARY KEY (kind, subject, entry)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS memory_subject_entry ON memory_subject (entry);
CREATE VIRTUAL TABLE IF NOT EXISTS memory_fts USING fts5(text, tokenize = 'porter unicode61');
-- For a vector index later (sqlite-vec): one embedding per entry, keyed by the
-- entry's stable ID so it survives a rebuild. Nothing fills it yet.
CREATE TABLE IF NOT EXISTS memory_embedding (
    entry_id TEXT PRIMARY KEY,
    model TEXT NOT NULL,
    dims INTEGER NOT NULL,
    vector BLOB NOT NULL
);
"#;

/// What an entry is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubjectKind {
    Station,
    Person,
    Faction,
}

impl SubjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SubjectKind::Station => "station",
            SubjectKind::Person => "person",
            SubjectKind::Faction => "faction",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "station" => Some(SubjectKind::Station),
            "person" => Some(SubjectKind::Person),
            "faction" => Some(SubjectKind::Faction),
            _ => None,
        }
    }
}

/// One subject of an entry.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Subject {
    pub kind: SubjectKind,
    pub id: String,
}

/// Where an entry's story came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// The core pack's data and scripts, the player's own verbs, the engine.
    Authored,
    /// A game-master layer's pack, or a rollback of one.
    GameMaster,
    /// The fast-model live layer (`Cause::Mind`).
    LiveLayer,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Source::Authored => "authored",
            Source::GameMaster => "game_master",
            Source::LiveLayer => "live_layer",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "authored" => Some(Source::Authored),
            "game_master" => Some(Source::GameMaster),
            "live_layer" => Some(Source::LiveLayer),
            _ => None,
        }
    }
}

/// One remembered thing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Stable: `mem.fact.N` (the first fact it summarises) or `mem.talk.N` (a line).
    pub id: String,
    /// `trade`, `delivery`, `beat`, `part`, `stage`, `happening`, `standing`,
    /// `pilot`, `talk`, `notice` or `event`.
    pub kind: String,
    pub subjects: Vec<Subject>,
    pub tick: u64,
    /// The player's counted dockings when it happened.
    pub docking: u64,
    pub text: String,
    pub source: Source,
    /// The facts it summarises (a line: the facts it cites).
    pub facts: Vec<FactId>,
}

/// What [`search`] and [`list`] keep. Every field set must match.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Filter {
    pub station: Option<String>,
    pub person: Option<String>,
    pub faction: Option<String>,
    pub source: Option<Source>,
    /// Most entries returned (default [`DEFAULT_LIMIT`]).
    pub limit: Option<u32>,
}

/// How many entries a search returns unless the filter says.
pub const DEFAULT_LIMIT: u32 = 20;

/// Pack fact kinds (by last segment) that are bookkeeping, not memory: the lines and
/// confirmations they record are remembered from the talk log instead, and running
/// tallies and the fast model's own accounting say nothing a person would recall.
const SKIP_NOTES: [&str; 6] = [
    "npc_line",
    "confirmed",
    "asked_what_changed",
    "dispute_tally",
    "decision",
    "spend",
];

/// The engine's own notes that are memory, and their entry kinds: the hullwright's
/// part changes and finds. The rest (queues done, saturation, recounts) aren't.
const ENGINE_NOTES: [(&str, &str); 8] = [
    ("part_fitted", "part"),
    ("part_refitted", "part"),
    ("part_removed", "part"),
    ("part_made", "part"),
    ("part_ordered", "part"),
    ("part_arrived", "part"),
    ("blueprint_found", "part"),
    ("survey_reward_claimed", "event"),
];

/// A note as words: a JSON note (a payload some packs record) reads as its `claim`,
/// `text` or `line`, or not at all.
fn note_words(note: &str) -> Option<String> {
    let note = note.trim();
    if !note.starts_with('{') {
        return (!note.is_empty()).then(|| note.to_owned());
    }
    let v: serde_json::Value = serde_json::from_str(note).ok()?;
    ["claim", "text", "line"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|s| s.as_str()))
        .map(str::to_owned)
}

/// Names for IDs, as the world has them now.
struct Names<'a>(&'a WorldState);

impl Names<'_> {
    fn station(&self, id: &StationId) -> String {
        self.0
            .stations
            .get(id)
            .map_or_else(|| id.to_string(), |s| s.name.clone())
    }
    fn npc(&self, id: &NpcId) -> String {
        self.0
            .npcs
            .get(id)
            .map_or_else(|| id.to_string(), |n| n.name.clone())
    }
    fn faction(&self, id: &FactionId) -> String {
        self.0
            .factions
            .get(id)
            .map_or_else(|| id.to_string(), |f| f.name.clone())
    }
    fn ware(&self, id: &WareId) -> String {
        self.0
            .wares
            .get(id)
            .map_or_else(|| id.to_string(), |w| w.name.to_lowercase())
    }
    fn hull(&self, id: &HullId) -> String {
        self.0
            .hulls
            .get(id)
            .map_or_else(|| id.to_string(), |h| h.name.clone())
    }
}

/// A readable name from an ID's last segment: `happening.kettle.lamp_night` -> "Lamp
/// night".
fn last_words(id: &str) -> String {
    let last = id.rsplit('.').next().unwrap_or(id).replace('_', " ");
    let mut c = last.chars();
    match c.next() {
        Some(first) => first.to_uppercase().chain(c).collect(),
        None => last,
    }
}

fn subject(kind: SubjectKind, id: &impl ToString) -> Subject {
    Subject {
        kind,
        id: id.to_string(),
    }
}

/// The stations, people and factions a fact names.
fn fact_subjects(f: &Fact) -> Vec<Subject> {
    let s = &f.subjects;
    let mut out = Vec::new();
    if let Some(x) = &s.station {
        out.push(subject(SubjectKind::Station, x));
    }
    if let Some(x) = &s.npc {
        out.push(subject(SubjectKind::Person, x));
    }
    if let Some(x) = &s.faction {
        out.push(subject(SubjectKind::Faction, x));
    }
    out
}

/// A person and where they are.
fn person_at(world: &WorldState, npc: &NpcId) -> Vec<Subject> {
    let mut out = vec![subject(SubjectKind::Person, npc)];
    if let Some(at) = world.npcs.get(npc).and_then(|n| n.location.as_ref()) {
        out.push(subject(SubjectKind::Station, at));
    }
    out
}

/// The layer a pack runs as: game master, or the authored backbone.
fn pack_source(world: &WorldState, pack: &PackId) -> Source {
    match world.packs.get(pack).and_then(|p| p.layer) {
        Some(PackLayer::GameMaster) => Source::GameMaster,
        _ => Source::Authored,
    }
}

fn cause_source(state: &SimState, cause: CauseId) -> Source {
    match state.history.cause(cause).map(|c| &c.cause.kind) {
        Some(CauseKind::Mind { .. }) => Source::LiveLayer,
        Some(CauseKind::Script { pack, .. }) => pack_source(&state.world, pack),
        Some(CauseKind::Rollback { .. }) => Source::GameMaster,
        _ => Source::Authored,
    }
}

/// The source of a fact: its cause's, except a happening's, which is its author's.
fn fact_source(state: &SimState, f: &Fact) -> Source {
    let kind = f.kind.as_str();
    if (kind == STARTED_FACT || kind == ENDED_FACT)
        && let Some(h) = f
            .subjects
            .other
            .as_ref()
            .and_then(|o| state.world.happenings.get(o.as_str()))
    {
        return match &h.by {
            Some(pack) => pack_source(&state.world, pack),
            None => Source::Authored,
        };
    }
    cause_source(state, f.cause)
}

/// Which journaled cause said each line (lines from unjournaled causes, the engine's
/// and the player's, aren't listed: they are authored).
fn line_causes(state: &SimState) -> BTreeMap<TalkId, CauseId> {
    let mut out = BTreeMap::new();
    for e in &state.journal {
        for u in &e.undo {
            match u {
                UndoRecord::Invert {
                    command: Command::PutTalk { id, value: None },
                } => {
                    out.insert(*id, e.cause);
                }
                UndoRecord::Compensate { note } => {
                    if let Some(id) = note
                        .strip_prefix("chronicle entry ")
                        .and_then(|n| n.strip_suffix(" was written"))
                        .and_then(|n| n.parse::<TalkId>().ok())
                    {
                        out.insert(id, e.cause);
                    }
                }
                _ => {}
            }
        }
    }
    out
}

/// A visit's trades in one ware, one way.
struct Rollup {
    first: FactId,
    tick: u64,
    docking: u64,
    units: u64,
    cents: i64,
    facts: Vec<FactId>,
}

/// Entries in the order they're written: by tick, then facts before lines, then ID.
type Key = (u64, u8, u64);

struct Builder<'a> {
    state: &'a SimState,
    names: Names<'a>,
    out: Vec<(Key, Entry)>,
    dockings: u64,
    /// The open visit's trades: (station, sold, ware).
    open: BTreeMap<(StationId, bool, WareId), Rollup>,
    /// The cause of the last `upgrade_fitted` entry, for its `part_fitted` note.
    last_fit: Option<CauseId>,
}

impl Builder<'_> {
    fn push(&mut self, key: Key, entry: Entry) {
        self.out.push((key, entry));
    }

    fn fact_entry(&self, f: &Fact, kind: &str, subjects: Vec<Subject>, text: String) -> Entry {
        Entry {
            id: format!("mem.{}", f.id),
            kind: kind.to_owned(),
            subjects,
            tick: f.tick.0,
            docking: self.dockings,
            text,
            source: fact_source(self.state, f),
            facts: vec![f.id],
        }
    }

    /// Ends the visit at `station`: its trades become entries, at the undock `f`.
    fn flush(&mut self, station: &StationId, at: &Fact) {
        let keys: Vec<_> = self
            .open
            .keys()
            .filter(|(s, _, _)| s == station)
            .cloned()
            .collect();
        for key in keys {
            let Some(r) = self.open.remove(&key) else {
                continue;
            };
            let (station, sold, ware) = key;
            let text = format!(
                "{} {} {} at {} for {} cr",
                if sold { "Sold" } else { "Bought" },
                r.units,
                self.names.ware(&ware),
                self.names.station(&station),
                show_credits(r.cents)
            );
            let entry = Entry {
                id: format!("mem.{}", r.first),
                kind: "trade".into(),
                subjects: vec![subject(SubjectKind::Station, &station)],
                tick: r.tick,
                docking: r.docking,
                text,
                source: Source::Authored,
                facts: r.facts,
            };
            self.push((at.tick.0, 0, r.first.0), entry);
        }
    }

    fn event(&mut self, f: &Fact, event: &Event, players: &dyn Fn(&ShipId) -> bool) {
        let world = &self.state.world;
        let key = (f.tick.0, 0, f.id.0);
        let entry = match event {
            Event::Buy {
                ship,
                station,
                ware,
                quantity,
                unit_price,
            }
            | Event::Sell {
                ship,
                station,
                ware,
                quantity,
                unit_price,
            } if players(ship) => {
                let sold = matches!(event, Event::Sell { .. });
                let docking = self.dockings;
                let r = self
                    .open
                    .entry((station.clone(), sold, ware.clone()))
                    .or_insert(Rollup {
                        first: f.id,
                        tick: f.tick.0,
                        docking,
                        units: 0,
                        cents: 0,
                        facts: Vec::new(),
                    });
                r.units += u64::from(*quantity);
                r.cents += i64::from(*quantity) * i64::from(*unit_price);
                r.facts.push(f.id);
                return;
            }
            Event::Undock { ship, station } if Some(ship) == world.player.ship.as_ref() => {
                self.flush(station, f);
                return;
            }
            Event::Dock { ship, .. } if Some(ship) == world.player.ship.as_ref() => {
                if f.amount != Some(0) {
                    self.dockings += 1;
                }
                return;
            }
            Event::Deliver {
                ship,
                station,
                ware,
                quantity,
                ..
            } if players(ship) => self.fact_entry(
                f,
                "delivery",
                vec![subject(SubjectKind::Station, station)],
                format!(
                    "Delivered {quantity} {} at {}",
                    self.names.ware(ware),
                    self.names.station(station)
                ),
            ),
            Event::BeatStarted { beat, .. } | Event::BeatCompleted { beat } => {
                let b = world.progress.beats.get(beat);
                let goal = b.map_or_else(|| beat.to_string(), |b| b.goal.clone());
                let subjects = b
                    .and_then(|b| b.giver.as_ref())
                    .map(|g| person_at(world, g))
                    .unwrap_or_default();
                let started = matches!(event, Event::BeatStarted { .. });
                let text = format!("{}: {goal}", if started { "Started" } else { "Done" });
                self.fact_entry(f, "beat", subjects, text)
            }
            Event::UpgradeFitted { upgrade, hull } => {
                self.last_fit = Some(f.cause);
                let def = world.progress.rules.upgrades.get(upgrade);
                let name = def.map_or_else(|| upgrade.to_string(), |d| d.name.to_lowercase());
                let mut subjects = Vec::new();
                if let Some(seller) = def.and_then(|d| d.seller.as_ref()) {
                    subjects = person_at(world, seller);
                }
                let text = format!("Fitted the {name} to the {}", self.names.hull(hull));
                self.fact_entry(f, "part", subjects, text)
            }
            Event::TenderAcquired { hull, station, .. } => self.fact_entry(
                f,
                "part",
                vec![subject(SubjectKind::Station, station)],
                format!(
                    "{} built the player a {} tender",
                    self.names.station(station),
                    self.names.hull(hull)
                ),
            ),
            Event::StandingChanged { faction, delta, .. } => self.fact_entry(
                f,
                "standing",
                vec![subject(SubjectKind::Faction, faction)],
                format!(
                    "Standing with {} {} by {}",
                    self.names.faction(faction),
                    if *delta < 0 { "fell" } else { "rose" },
                    delta.unsigned_abs()
                ),
            ),
            Event::RegardChanged { faction, from, to } => self.fact_entry(
                f,
                "standing",
                vec![subject(SubjectKind::Faction, faction)],
                format!(
                    "Regard with {} went from {} to {}",
                    self.names.faction(faction),
                    last_words(from).to_lowercase(),
                    last_words(to).to_lowercase()
                ),
            ),
            Event::TraitEarned { trait_id, .. } => {
                let name = world
                    .progress
                    .rules
                    .traits
                    .get(trait_id)
                    .map_or_else(|| last_words(trait_id.as_str()), |t| t.name.clone());
                self.fact_entry(
                    f,
                    "pilot",
                    fact_subjects(f),
                    format!("The pilot picked up a habit: {name}"),
                )
            }
            Event::HullRank { hull, rank } => self.fact_entry(
                f,
                "pilot",
                fact_subjects(f),
                format!("The {} earned rank {rank}", self.names.hull(hull)),
            ),
            Event::Prestige {
                from_hull, to_hull, ..
            } => self.fact_entry(
                f,
                "pilot",
                fact_subjects(f),
                format!(
                    "Traded the {} up for a {}",
                    self.names.hull(from_hull),
                    self.names.hull(to_hull)
                ),
            ),
            _ => return,
        };
        self.push(key, entry);
    }

    fn note(&mut self, f: &Fact, note: &str) {
        let kind = f.kind.as_str();
        let last = kind.rsplit('.').next().unwrap_or(kind);
        let live = cause_source(self.state, f.cause) == Source::LiveLayer;
        let happening = kind == STARTED_FACT || kind == ENDED_FACT;
        let engine = ENGINE_NOTES
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, e)| *e);
        // The engine's other notes (quarantines, contention, applies) aren't memory;
        // a pack's are, in its namespace, and anything the live layer writes.
        let wanted = happening || live || engine.is_some() || kind.contains('.');
        if !wanted || SKIP_NOTES.contains(&last) {
            return;
        }
        let (entry_kind, text) = if happening {
            let id = f.subjects.other.as_ref().map(|o| o.as_str()).unwrap_or("");
            let name = last_words(id);
            let at = f
                .subjects
                .station
                .as_ref()
                .map(|s| self.names.station(s))
                .unwrap_or_default();
            let text =
                if kind == STARTED_FACT {
                    let line =
                        self.state.world.happenings.get(id).map(|h| {
                            format!(" {}: {}", self.names.npc(&h.line.speaker), h.line.text)
                        });
                    format!("{name} began at {at}.{}", line.unwrap_or_default())
                } else {
                    let n = f.amount.unwrap_or(0);
                    format!("{name} ended at {at} after {n} dockings")
                };
            ("happening", text)
        } else {
            let Some(words) = note_words(note) else {
                return;
            };
            let kind = match engine {
                Some(k) => k,
                None if last == "stage_built" => "stage",
                None => "event",
            };
            (kind, words)
        };
        // A fit is an `upgrade_fitted` event and the hullwright's `part_fitted` note
        // in one batch: one entry, in the note's words.
        if kind == "part_fitted"
            && let Some((_, e)) = self.out.last_mut()
            && e.kind == "part"
            && self.last_fit == Some(f.cause)
        {
            e.text = text;
            e.facts.push(f.id);
            return;
        }
        let entry = self.fact_entry(f, entry_kind, fact_subjects(f), text);
        self.push((f.tick.0, 0, f.id.0), entry);
    }

    fn line(&mut self, said: &SaidLine, causes: &BTreeMap<TalkId, CauseId>) {
        let l = &said.line;
        let mut subjects = Vec::new();
        if let Some(s) = &l.speaker {
            subjects.push(subject(SubjectKind::Person, s));
        }
        if let Some(p) = &l.place {
            subjects.push(subject(SubjectKind::Station, p));
        }
        let (kind, who) = match (&l.speaker, l.channel) {
            (Some(s), Channel::DockTalk) => ("talk", self.names.npc(s)),
            (Some(s), _) => ("notice", self.names.npc(s)),
            (None, _) => (
                "notice",
                l.place
                    .as_ref()
                    .map(|p| self.names.station(p))
                    .unwrap_or_default(),
            ),
        };
        let text = if who.is_empty() {
            l.text.clone()
        } else {
            format!("{who}: {}", l.text)
        };
        let source = causes
            .get(&said.id)
            .map_or(Source::Authored, |c| cause_source(self.state, *c));
        let entry = Entry {
            id: format!("mem.{}", said.id),
            kind: kind.into(),
            subjects,
            tick: said.tick.0,
            docking: self.dockings,
            text,
            source,
            facts: l.cites.clone(),
        };
        self.push((said.tick.0, 1, said.id.0), entry);
    }
}

/// Every entry `state` holds, in the order they are written. Deterministic: the same
/// state always gives the same entries. The open visit's trades are left out until
/// the flagship undocks.
pub fn summarize(state: &SimState) -> Vec<Entry> {
    let world = &state.world;
    let players = |ship: &ShipId| {
        world.player.ship.as_ref() == Some(ship)
            || world
                .ships
                .get(ship)
                .is_some_and(|s| s.owner == Owner::Player)
    };
    let causes = line_causes(state);
    let mut lines = world.talk.lines.values().peekable();
    let mut b = Builder {
        state,
        names: Names(world),
        out: Vec::new(),
        dockings: 0,
        open: BTreeMap::new(),
        last_fit: None,
    };
    for f in state.history.facts() {
        // Lines said in earlier ticks come first: they saw the dockings before them.
        while let Some(said) = lines.next_if(|l| l.tick < f.tick) {
            b.line(said, &causes);
        }
        match &f.payload {
            FactPayload::Event(e) => b.event(f, e, &players),
            FactPayload::Note(n) => b.note(f, n),
        }
    }
    for said in lines {
        b.line(said, &causes);
    }
    let mut out = b.out;
    out.sort_by_key(|(k, _)| *k);
    out.into_iter().map(|(_, e)| e).collect()
}

fn hash_entry(h: &mut blake3::Hasher, e: &Entry) {
    let json = serde_json::to_string(e).expect("entries serialize");
    h.update(&(json.len() as u64).to_le_bytes());
    h.update(json.as_bytes());
}

fn meta(conn: &Connection, key: &str) -> Result<Option<String>, SaveError> {
    Ok(conn
        .query_row("SELECT value FROM memory_meta WHERE key = ?1", [key], |r| {
            r.get(0)
        })
        .optional()?)
}

fn set_meta(conn: &Connection, key: &str, value: &str) -> Result<(), SaveError> {
    conn.execute(
        "INSERT INTO memory_meta (key, value) VALUES (?1, ?2)
         ON CONFLICT (key) DO UPDATE SET value = ?2",
        params![key, value],
    )?;
    Ok(())
}

/// Whether the save has the memory tables filled for this build's summary format.
pub(crate) fn is_current(conn: &Connection) -> Result<bool, SaveError> {
    let exists: Option<String> = conn
        .query_row(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'memory_meta'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if exists.is_none() {
        return Ok(false);
    }
    Ok(meta(conn, "version")?.as_deref() == Some(MEMORY_VERSION.to_string().as_str()))
}

/// Brings the save's memory tables in line with `state` (inside the caller's
/// transaction): inserts the entries written since the last sync, or rebuilds them all
/// if the stored ones aren't the first entries of `state`'s. Returns how many entries
/// it wrote.
pub(crate) fn sync(conn: &Connection, state: &SimState) -> Result<usize, SaveError> {
    conn.execute_batch(SCHEMA)?;
    let entries = summarize(state);
    let stored: usize = meta(conn, "count")?
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let digest = meta(conn, "digest")?;
    let current = meta(conn, "version")?.as_deref() == Some(MEMORY_VERSION.to_string().as_str());
    let mut h = blake3::Hasher::new();
    let keep = if current && stored <= entries.len() {
        for e in &entries[..stored] {
            hash_entry(&mut h, e);
        }
        if Some(h.finalize().to_hex().to_string()) == digest {
            stored
        } else {
            0
        }
    } else {
        0
    };
    if keep == 0 {
        h = blake3::Hasher::new();
        conn.execute_batch(
            "DELETE FROM memory_subject; DELETE FROM memory_entry; DELETE FROM memory_fts;",
        )?;
    }
    let mut entry = conn.prepare(
        "INSERT INTO memory_entry (seq, id, kind, tick, docking, source, text, facts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
    )?;
    let mut subject = conn.prepare(
        "INSERT OR IGNORE INTO memory_subject (kind, subject, entry) VALUES (?1, ?2, ?3)",
    )?;
    let mut fts = conn.prepare("INSERT INTO memory_fts (rowid, text) VALUES (?1, ?2)")?;
    for (i, e) in entries.iter().enumerate().skip(keep) {
        let seq = i64::try_from(i + 1).expect("entry counts fit in i64");
        entry.execute(params![
            seq,
            e.id,
            e.kind,
            i64::try_from(e.tick).unwrap_or(i64::MAX),
            i64::try_from(e.docking).unwrap_or(i64::MAX),
            e.source.as_str(),
            e.text,
            serde_json::to_string(&e.facts)?,
        ])?;
        for s in &e.subjects {
            subject.execute(params![s.kind.as_str(), s.id, seq])?;
        }
        fts.execute(params![seq, e.text])?;
        hash_entry(&mut h, e);
    }
    if keep == 0 {
        // Embeddings follow their entries' stable IDs; drop any whose entry is gone.
        conn.execute(
            "DELETE FROM memory_embedding WHERE entry_id NOT IN (SELECT id FROM memory_entry)",
            [],
        )?;
    }
    set_meta(conn, "version", &MEMORY_VERSION.to_string())?;
    set_meta(conn, "count", &entries.len().to_string())?;
    set_meta(conn, "digest", h.finalize().to_hex().as_ref())?;
    Ok(entries.len() - keep)
}

/// An FTS5 query from the player's words: each word quoted (so punctuation and
/// operators in it are plain text), all of them required.
fn fts_query(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{}\"", w.replace('"', "")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn read_entries(
    save: &Save,
    sql: &str,
    args: Vec<rusqlite::types::Value>,
) -> Result<Vec<Entry>, SaveError> {
    let conn = save.conn();
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(args), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, i64>(3)?,
            r.get::<_, i64>(4)?,
            r.get::<_, String>(5)?,
            r.get::<_, String>(6)?,
            r.get::<_, String>(7)?,
        ))
    })?;
    let mut subjects = conn.prepare(
        "SELECT kind, subject FROM memory_subject WHERE entry = ?1 ORDER BY kind, subject",
    )?;
    let mut out = Vec::new();
    for row in rows {
        let (seq, id, kind, tick, docking, source, text, facts) = row?;
        let subjects = subjects
            .query_map([seq], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })?
            .filter_map(|r| {
                let (k, id) = r.ok()?;
                Some(Subject {
                    kind: SubjectKind::parse(&k)?,
                    id,
                })
            })
            .collect();
        out.push(Entry {
            id,
            kind,
            subjects,
            tick: u64::try_from(tick).unwrap_or(0),
            docking: u64::try_from(docking).unwrap_or(0),
            text,
            source: Source::parse(&source)
                .ok_or_else(|| SaveError::Corrupt(format!("memory source `{source}`")))?,
            facts: serde_json::from_str(&facts)?,
        });
    }
    Ok(out)
}

/// The filter's conditions on `memory_entry e`, with their arguments.
fn conditions(filter: &Filter, args: &mut Vec<rusqlite::types::Value>) -> String {
    let mut sql = String::new();
    let subjects = [
        (SubjectKind::Station, &filter.station),
        (SubjectKind::Person, &filter.person),
        (SubjectKind::Faction, &filter.faction),
    ];
    for (kind, id) in subjects {
        if let Some(id) = id {
            args.push(kind.as_str().to_owned().into());
            args.push(id.clone().into());
            sql.push_str(&format!(
                " AND EXISTS (SELECT 1 FROM memory_subject s WHERE s.entry = e.seq \
                 AND s.kind = ?{} AND s.subject = ?{})",
                args.len() - 1,
                args.len()
            ));
        }
    }
    if let Some(source) = filter.source {
        args.push(source.as_str().to_owned().into());
        sql.push_str(&format!(" AND e.source = ?{}", args.len()));
    }
    sql
}

const COLUMNS: &str = "e.seq, e.id, e.kind, e.tick, e.docking, e.source, e.text, e.facts";

/// Full-text search over the save's memory: entries whose text has every word of
/// `query` (stemmed, so "sold" finds "Sold"), best match first, then newest. An empty
/// query lists the filter's entries, newest first ([`list`]).
pub fn search(save: &Save, query: &str, filter: &Filter) -> Result<Vec<Entry>, SaveError> {
    let q = fts_query(query);
    if q.is_empty() {
        return list(save, filter);
    }
    let mut args: Vec<rusqlite::types::Value> = vec![q.into()];
    let cond = conditions(filter, &mut args);
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT);
    let sql = format!(
        "SELECT {COLUMNS} FROM memory_fts f JOIN memory_entry e ON e.seq = f.rowid \
         WHERE memory_fts MATCH ?1{cond} ORDER BY bm25(memory_fts), e.seq DESC LIMIT {limit}"
    );
    read_entries(save, &sql, args)
}

/// The filter's entries (a station's, a person's or a faction's history), newest
/// first.
pub fn list(save: &Save, filter: &Filter) -> Result<Vec<Entry>, SaveError> {
    let mut args = Vec::new();
    let cond = conditions(filter, &mut args);
    let limit = filter.limit.unwrap_or(DEFAULT_LIMIT);
    let sql = format!(
        "SELECT {COLUMNS} FROM memory_entry e WHERE 1 = 1{cond} ORDER BY e.seq DESC LIMIT {limit}"
    );
    read_entries(save, &sql, args)
}

/// How many entries the save's memory holds.
pub fn count(save: &Save) -> Result<u64, SaveError> {
    let n: i64 = save
        .conn()
        .query_row("SELECT COUNT(*) FROM memory_entry", [], |r| r.get(0))?;
    Ok(u64::try_from(n).unwrap_or(0))
}
