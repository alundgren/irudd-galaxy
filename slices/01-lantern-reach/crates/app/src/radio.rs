//! The radio (#47, 5.1): channels per station and beacon, each with a range.
//!
//! - **Data**: `data/radio.toml` ([`RadioData`]), parsed strictly and checked against the
//!   world. A station channel's mood line for the music library is written from the
//!   station's theme unless the file gives one ([`RadioData::mood`]).
//! - **Discovery** ([`RadioSystem`]): the one piece of sim state. The first time the
//!   flagship flies into a channel's range, a system batch records a `radio_found` fact
//!   (once per save), and on the next tick a map note says "New signal: Kettle Dock
//!   radio" in the ship log, citing it.
//! - **Tuning and the mix** ([`Radio`]): view and audio only. While flying, every
//!   channel in range has a signal by distance; the strongest plays (or the one tuned
//!   by hand), overlapping channels crossfade by signal, static creeps in at the edges,
//!   and out of range of everything it is quiet. Docked, it fades out. The HUD widget
//!   ([`crate::ui::radio`]) shows the channel, its signal and its text bits: the
//!   channel's authored bits and the live layer's (`galaxy_mind::radio`).
//! - **Music** ([`music`]): what a channel plays, from the music library: a generated
//!   track if one is cached, else a named placeholder loop.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Mutex;

use galaxy_audio::radio::{self as sound, Falloff};
use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::{History, System, TickCtx};
use galaxy_mind::radio::{self as live, FOUND_FACT};
use render::kit::life::Motif;
use serde::Deserialize;

use crate::view::themes::Themes;

/// The data file, in the core pack's data folder.
pub const FILE: &str = "radio.toml";

/// The system's name: it keys its causes (and an RNG stream it never uses).
pub const SYSTEM: &str = "radio";

/// Longest channel or beacon name.
const MAX_NAME: usize = 32;
/// Longest text bit.
const MAX_BIT: usize = 110;
/// Longest range, in world units.
const MAX_RANGE: f32 = 3000.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDef {
    #[serde(default)]
    signal: Option<SignalDef>,
    #[serde(default)]
    live: Option<LiveDef>,
    #[serde(default)]
    beacon: Vec<BeaconDef>,
    #[serde(default)]
    channel: Vec<ChannelDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SignalDef {
    near: f32,
    static_below: f32,
    static_max: f32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LiveDef {
    cooldown_dockings: u32,
    run_calls: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BeaconDef {
    id: String,
    name: String,
    station: StationId,
    region: RegionId,
    position: Vec2,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChannelDef {
    id: String,
    source: String,
    name: String,
    range: f32,
    #[serde(default)]
    mood: Option<String>,
    #[serde(default)]
    bits: Vec<String>,
}

/// A beacon: a channel's source that isn't a station.
#[derive(Clone, Debug, PartialEq)]
pub struct Beacon {
    pub id: String,
    pub name: String,
    /// The station that tends it: the place its notice is about.
    pub station: StationId,
    pub region: RegionId,
    pub position: Vec2,
}

/// Where a channel comes from.
#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Station(StationId),
    Beacon(Beacon),
}

/// One channel, checked.
#[derive(Clone, Debug, PartialEq)]
pub struct ChannelSpec {
    pub id: String,
    pub name: String,
    pub source: Source,
    pub range: f32,
    /// The file's mood line, if it gave one ([`RadioData::mood`] fills it in).
    pub mood: Option<String>,
    pub bits: Vec<String>,
}

impl ChannelSpec {
    /// The station the channel is about: its own, or the one tending its beacon.
    pub fn station(&self) -> &StationId {
        match &self.source {
            Source::Station(s) => s,
            Source::Beacon(b) => &b.station,
        }
    }

    /// Where the source is now: its region and position. `None` for a station the
    /// world doesn't have.
    pub fn at(&self, world: &WorldState) -> Option<(RegionId, Vec2)> {
        match &self.source {
            Source::Station(s) => world
                .stations
                .get(s)
                .map(|st| (st.region.clone(), st.position)),
            Source::Beacon(b) => Some((b.region.clone(), b.position)),
        }
    }

    /// The flagship's distance from the source, if it is flying in the source's region
    /// (not docked, not in a lane).
    pub fn distance(&self, world: &WorldState) -> Option<f32> {
        let ship = world
            .player
            .ship
            .as_ref()
            .and_then(|s| world.ships.get(s))?;
        if ship.docked_at.is_some() || ship.activity.transit.is_some() {
            return None;
        }
        let (region, at) = self.at(world)?;
        (region == ship.region).then(|| at.distance(ship.position))
    }

    /// The flagship is inside the channel's range.
    pub fn in_range(&self, world: &WorldState) -> bool {
        self.distance(world).is_some_and(|d| d < self.range)
    }
}

/// `radio.toml`, parsed and checked.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioData {
    pub falloff: Falloff,
    pub live: live::Rules,
    pub channels: Vec<ChannelSpec>,
}

/// Is `text` in the first person (the copy rule for anything said on the air)?
fn first_person(text: &str) -> bool {
    text.split(|c: char| !(c.is_alphanumeric() || c == '\''))
        .map(str::to_lowercase)
        .any(|w| {
            matches!(
                w.as_str(),
                "i" | "i'm"
                    | "i've"
                    | "i'll"
                    | "me"
                    | "my"
                    | "we"
                    | "we're"
                    | "we'll"
                    | "we've"
                    | "us"
                    | "our"
            )
        })
}

impl RadioData {
    /// Parses and checks a `radio.toml`. Every problem is listed, one per line.
    pub fn parse(text: &str) -> Result<Self, String> {
        let def: FileDef = toml::from_str(text).map_err(|e| format!("{FILE}: {e}"))?;
        let mut errors = Vec::new();
        let mut bad = |m: String| errors.push(format!("{FILE}: {m}"));
        let falloff = match def.signal {
            Some(s) => {
                if !(0.0..=0.9).contains(&s.near) {
                    bad(format!("signal.near {} is outside 0 to 0.9", s.near));
                }
                if !(0.05..=1.0).contains(&s.static_below) {
                    bad(format!(
                        "signal.static_below {} is outside 0.05 to 1",
                        s.static_below
                    ));
                }
                if !(0.0..=0.6).contains(&s.static_max) {
                    bad(format!(
                        "signal.static_max {} is outside 0 to 0.6",
                        s.static_max
                    ));
                }
                Falloff {
                    near: s.near,
                    static_below: s.static_below,
                    static_max: s.static_max,
                }
            }
            None => Falloff::default(),
        };
        let live = def.live.map_or_else(live::Rules::default, |l| live::Rules {
            cooldown_dockings: l.cooldown_dockings,
            run_calls: l.run_calls,
        });
        let name_ok = |n: &str| !n.trim().is_empty() && n.chars().count() <= MAX_NAME;
        let mut beacons: BTreeMap<String, Beacon> = BTreeMap::new();
        for b in def.beacon {
            if !b.id.starts_with("beacon.") || AnyId::new(&b.id).is_err() {
                bad(format!("beacon `{}`: IDs look like beacon.<name>", b.id));
            }
            if !name_ok(&b.name) {
                bad(format!(
                    "beacon `{}`: a name of 1 to {MAX_NAME} characters",
                    b.id
                ));
            }
            if !(b.position.x.is_finite() && b.position.y.is_finite()) {
                bad(format!("beacon `{}`: its position must be finite", b.id));
            }
            if beacons.contains_key(&b.id) {
                bad(format!("duplicate beacon `{}`", b.id));
            }
            beacons.insert(
                b.id.clone(),
                Beacon {
                    id: b.id,
                    name: b.name,
                    station: b.station,
                    region: b.region,
                    position: b.position,
                },
            );
        }
        let mut channels: Vec<ChannelSpec> = Vec::new();
        let mut sources = BTreeSet::new();
        for c in def.channel {
            let at = format!("channel `{}`", c.id);
            if !c.id.starts_with("radio.") || AnyId::new(&c.id).is_err() {
                bad(format!("{at}: IDs look like radio.<name>"));
            }
            if channels.iter().any(|o| o.id == c.id) {
                bad(format!("duplicate {at}"));
            }
            if !name_ok(&c.name) {
                bad(format!("{at}: a name of 1 to {MAX_NAME} characters"));
            }
            if c.range.is_nan() || c.range <= 0.0 || c.range > MAX_RANGE {
                bad(format!(
                    "{at}: range {} is outside 0 to {MAX_RANGE}",
                    c.range
                ));
            }
            if !sources.insert(c.source.clone()) {
                bad(format!("{at}: `{}` already has a channel", c.source));
            }
            for bit in &c.bits {
                if bit.trim().is_empty() || bit.chars().count() > MAX_BIT {
                    bad(format!("{at}: a bit of 1 to {MAX_BIT} characters"));
                } else if bit.chars().any(|ch| ch.is_ascii_digit()) {
                    bad(format!("{at}: no digits in a bit: {bit}"));
                } else if !first_person(bit) {
                    bad(format!("{at}: a bit says I, we or our: {bit}"));
                }
            }
            if c.mood.as_ref().is_some_and(|m| m.trim().is_empty()) {
                bad(format!("{at}: an empty mood"));
            }
            let source = if let Some(b) = beacons.get(&c.source) {
                if c.mood.is_none() {
                    bad(format!("{at}: a beacon's channel needs a mood"));
                }
                Source::Beacon(b.clone())
            } else if c.source.starts_with("station.") {
                match StationId::new(&c.source) {
                    Ok(s) => Source::Station(s),
                    Err(_) => {
                        bad(format!("{at}: `{}` isn't a station ID", c.source));
                        continue;
                    }
                }
            } else {
                bad(format!(
                    "{at}: source `{}` is neither a station nor a beacon",
                    c.source
                ));
                continue;
            };
            channels.push(ChannelSpec {
                id: c.id,
                name: c.name,
                source,
                range: c.range,
                mood: c.mood,
                bits: c.bits,
            });
        }
        if errors.is_empty() {
            Ok(Self {
                falloff,
                live,
                channels,
            })
        } else {
            Err(errors.join("\n"))
        }
    }

    /// Reads and checks `radio.toml` in a pack's data folder. A folder without one has
    /// no channels.
    pub fn load(data: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(data.join(FILE)) {
            Ok(text) => Self::parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{FILE}: {e}")),
        }
    }

    /// Checks the references into the world: stations and regions.
    pub fn check(&self, world: &WorldState) -> Result<(), String> {
        let mut errors = Vec::new();
        for c in &self.channels {
            match &c.source {
                Source::Station(s) if !world.stations.contains(s) => {
                    errors.push(format!("{FILE}: channel `{}`: no station `{s}`", c.id));
                }
                Source::Beacon(b) => {
                    if !world.stations.contains(&b.station) {
                        errors.push(format!(
                            "{FILE}: beacon `{}`: no station `{}`",
                            b.id, b.station
                        ));
                    }
                    if !world.regions.contains(&b.region) {
                        errors.push(format!(
                            "{FILE}: beacon `{}`: no region `{}`",
                            b.id, b.region
                        ));
                    }
                }
                Source::Station(_) => {}
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("\n"))
        }
    }

    pub fn channel(&self, id: &str) -> Option<&ChannelSpec> {
        self.channels.iter().find(|c| c.id == id)
    }

    /// A channel's mood line for the music library: the file's, or for a station, one
    /// written from its theme: its motif and its sounds.
    pub fn mood(&self, c: &ChannelSpec, world: &WorldState, themes: &Themes) -> String {
        if let Some(m) = &c.mood {
            return m.clone();
        }
        let theme = themes.for_station(world, c.station());
        let feel = match theme.motif {
            Motif::Square => "steady and orderly",
            Motif::Pointed => "bright and airy",
            Motif::Round => "warm and rounded",
        };
        let sounds: Vec<String> = theme.sounds.iter().map(|s| s.replace('_', " ")).collect();
        let name = world
            .stations
            .get(c.station())
            .map_or_else(|| c.name.clone(), |s| s.name.clone());
        let mut mood =
            format!("Gentle instrumental radio music for {name}, a {feel} frontier station");
        if !sounds.is_empty() {
            mood.push_str(&format!(", with a hint of {}", sounds.join(", ")));
        }
        mood.push_str(". Slow and calm, no vocals.");
        mood
    }

    /// The station channels, for the live layer's text bits.
    pub fn live_setup(&self) -> live::Setup {
        live::Setup {
            channels: self
                .channels
                .iter()
                .filter_map(|c| match &c.source {
                    Source::Station(s) => Some(live::Channel {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        station: s.clone(),
                        range: c.range,
                    }),
                    Source::Beacon(_) => None,
                })
                .collect(),
            rules: self.live,
        }
    }
}

// --- Discovery: the sim hook ---

/// What the system knows from history: the facts it has read, and each channel's
/// discovery. Derived from history alone (a fresh system reads it from the start), so
/// it holds nothing a save needs.
#[derive(Default)]
struct Seen {
    cursor: Option<FactId>,
    found: BTreeMap<String, FactId>,
}

/// Records each channel's discovery once per save, and says it in the ship log.
///
/// Each tick, while the flagship flies (not docked, not in a lane), a channel it is
/// inside the range of for the first time gets a `radio_found` fact (the channel in
/// `subjects.other`, the station it is about, the region, the ship) in a system batch.
/// When that fact is in history, the next tick says "New signal: <channel>" as a map
/// note about the station, citing it. A fact recorded but not yet said when the game
/// was saved is said after the load.
pub struct RadioSystem {
    channels: Vec<ChannelSpec>,
    seen: Mutex<Seen>,
}

impl RadioSystem {
    pub fn new(data: &RadioData) -> Self {
        Self {
            channels: data.channels.clone(),
            seen: Mutex::new(Seen::default()),
        }
    }
}

/// The notice for a discovery: "New signal: Kettle Dock radio".
pub fn notice(name: &str) -> String {
    format!("New signal: {name}")
}

fn said(world: &WorldState, fact: FactId) -> bool {
    world
        .talk
        .lines
        .values()
        .any(|l| l.line.cites.contains(&fact))
}

impl System for RadioSystem {
    fn name(&self) -> &'static str {
        SYSTEM
    }

    fn step(&self, ctx: &mut TickCtx<'_>) {
        let Ok(mut seen) = self.seen.lock() else {
            return;
        };
        let facts = ctx.history.facts();
        // A history that no longer holds what was read (a restored snapshot) is read
        // again from the start.
        if seen
            .cursor
            .is_some_and(|c| ctx.history.fact(c).is_none() || facts.last().is_none_or(|f| f.id < c))
        {
            *seen = Seen::default();
        }
        let from = seen
            .cursor
            .map_or(0, |id| facts.partition_point(|f| f.id <= id));
        for f in &facts[from..] {
            if f.kind.as_str() != FOUND_FACT {
                continue;
            }
            let Some(channel) = f.subjects.other.as_ref().map(|o| o.to_string()) else {
                continue;
            };
            seen.found.entry(channel.clone()).or_insert(f.id);
            if said(ctx.world, f.id) {
                continue;
            }
            let Some(spec) = self.channels.iter().find(|c| c.id == channel) else {
                continue;
            };
            let line = TalkLine {
                channel: Channel::MapNote,
                speaker: None,
                place: f.subjects.station.clone(),
                text: notice(&spec.name),
                cites: vec![f.id],
                about: Vec::new(),
                choices: Vec::new(),
            };
            if line.validate().is_ok()
                && line
                    .place
                    .as_ref()
                    .is_some_and(|s| ctx.world.stations.contains(s))
            {
                let mut cause = Cause::system(SYSTEM, notice(&spec.name));
                cause.cites = vec![f.id];
                ctx.submit(CommandBatch::new(cause).with(Command::Say {
                    line: Box::new(line),
                }));
            }
        }
        seen.cursor = facts.last().map(|f| f.id).or(seen.cursor);
        let world = ctx.world;
        let Some(ship) = world.player.ship.clone() else {
            return;
        };
        for c in &self.channels {
            if seen.found.contains_key(&c.id) || !c.in_range(world) {
                continue;
            }
            let Some((region, _)) = c.at(world) else {
                continue;
            };
            if !world.stations.contains(c.station()) {
                continue;
            }
            let Ok(other) = AnyId::new(&c.id) else {
                continue;
            };
            ctx.submit(
                CommandBatch::new(Cause::system(
                    SYSTEM,
                    format!("the pilot picked up {}", c.name),
                ))
                .with(Command::RecordFact {
                    fact: Box::new(FactDraft {
                        kind: FactKind::from_static(FOUND_FACT),
                        subjects: Subjects {
                            ship: Some(ship.clone()),
                            station: Some(c.station().clone()),
                            region: Some(region),
                            other: Some(other),
                            ..Subjects::default()
                        },
                        amount: None,
                        note: notice(&c.name),
                    }),
                }),
            );
        }
    }
}

/// Channels found in this save so far, from history.
pub fn found(history: &History) -> BTreeSet<String> {
    history
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == FOUND_FACT)
        .filter_map(|f| f.subjects.other.as_ref().map(|o| o.to_string()))
        .collect()
}

// --- Music ---

// What a channel plays comes from the music library: cached generated tracks, else a
// named placeholder loop. It never blocks and never calls the network from here.
use galaxy_mind::music::{MusicLibrary, TrackSource};

// --- Tuning and the mix: view and audio only ---

/// A press on the radio: the HUD's buttons, or R.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tune {
    /// The previous channel in range (wraps).
    Prev,
    /// The next channel in range (wraps).
    Next,
    /// R: the next channel, and past the last one, off; off, on again.
    Cycle,
    /// Off, or on again.
    Toggle,
}

/// A channel heard now.
#[derive(Clone, Debug, PartialEq)]
pub struct Heard {
    /// Index into [`RadioData::channels`].
    pub index: usize,
    pub signal: f32,
}

/// What the HUD's radio shows ([`crate::ui::radio`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioUi {
    /// Flying: the widget shows.
    pub show: bool,
    pub on: bool,
    /// The channel playing, its name and signal, and whether it was tuned by hand.
    pub channel: Option<String>,
    pub signal: f32,
    pub tuned: bool,
    /// Its text bits: the live layer's newest first ("Ilse: ..."), then the authored.
    pub bits: Vec<String>,
    /// Channels in range, besides the one playing.
    pub others: usize,
}

/// One frame of the radio: what it sounds like and what the HUD shows.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RadioFrame {
    pub mix: sound::Mix,
    pub ui: RadioUi,
}

/// The radio's state in play: the data, the tuning and what has been heard. Nothing
/// here touches the sim.
pub struct Radio {
    data: RadioData,
    themes: Themes,
    music: MusicLibrary,
    /// A channel picked by hand; `None` follows the strongest.
    tuned: Option<String>,
    on: bool,
    /// Channels in range at the last update: one new to it was entered.
    heard: BTreeSet<String>,
    /// Channels entered this session, and how often (view only).
    entries: BTreeMap<String, u32>,
    bits: live::Bits,
}

impl Radio {
    pub fn new(data: RadioData, themes: Themes, on: bool) -> Self {
        Self {
            data,
            themes,
            music: MusicLibrary::placeholders(),
            tuned: None,
            on,
            heard: BTreeSet::new(),
            entries: BTreeMap::new(),
            bits: live::Bits::default(),
        }
    }

    /// Plays from `music` (the game's library, [`crate::game::Game::music`]) instead of
    /// placeholders only.
    pub fn with_music(mut self, music: MusicLibrary) -> Self {
        self.music = music;
        self
    }

    pub fn data(&self) -> &RadioData {
        &self.data
    }

    pub fn is_on(&self) -> bool {
        self.on
    }

    /// How often `channel` has come into range this session.
    pub fn entries(&self, channel: &str) -> u32 {
        self.entries.get(channel).copied().unwrap_or(0)
    }

    /// Every channel in range with its signal, strongest first (ties by file order).
    pub fn heard(&self, world: &WorldState) -> Vec<Heard> {
        let mut out: Vec<Heard> = self
            .data
            .channels
            .iter()
            .enumerate()
            .filter_map(|(index, c)| {
                let s = sound::signal(c.distance(world)?, c.range, &self.data.falloff);
                (s > 0.0).then_some(Heard { index, signal: s })
            })
            .collect();
        out.sort_by(|a, b| b.signal.total_cmp(&a.signal).then(a.index.cmp(&b.index)));
        out
    }

    /// The channel playing among `heard`: the one tuned by hand if it is still in
    /// range, else the strongest.
    fn current<'a>(&self, heard: &'a [Heard]) -> Option<&'a Heard> {
        self.tuned
            .as_ref()
            .and_then(|t| heard.iter().find(|h| &self.data.channels[h.index].id == t))
            .or_else(|| heard.first())
    }

    /// A press on the radio, against what is in range now. Returns whether the radio
    /// is on afterwards (the caller keeps that in the sound settings).
    pub fn tune(&mut self, world: &WorldState, t: Tune) -> bool {
        let heard = self.heard(world);
        // In file order, so Next and Prev walk a stable list as signals change.
        let mut order: Vec<usize> = heard.iter().map(|h| h.index).collect();
        order.sort_unstable();
        let at = self
            .current(&heard)
            .and_then(|c| order.iter().position(|i| *i == c.index));
        let pick = |i: usize| Some(self.data.channels[order[i]].id.clone());
        match t {
            Tune::Toggle => self.on = !self.on,
            _ if !self.on => {
                self.on = true;
            }
            _ if order.is_empty() => {
                if t == Tune::Cycle {
                    self.on = false;
                }
            }
            Tune::Next => {
                let i = at.map_or(0, |i| (i + 1) % order.len());
                self.tuned = pick(i);
            }
            Tune::Prev => {
                let i = at.map_or(0, |i| (i + order.len() - 1) % order.len());
                self.tuned = pick(i);
            }
            Tune::Cycle => match at {
                Some(i) if i + 1 >= order.len() => {
                    self.on = false;
                    self.tuned = None;
                }
                Some(i) => self.tuned = pick(i + 1),
                None => self.tuned = pick(0),
            },
        }
        self.on
    }

    /// The radio for this frame: the mix and the widget. A channel coming into range
    /// asks the music library for tracks (with its mood). Docked or with nothing in
    /// range, the mix is empty: the radio fades out.
    pub fn update(&mut self, world: &WorldState, history: &History) -> RadioFrame {
        self.bits.observe(history.facts());
        let heard = self.heard(world);
        let now: BTreeSet<String> = heard
            .iter()
            .map(|h| self.data.channels[h.index].id.clone())
            .collect();
        for id in now.difference(&self.heard) {
            *self.entries.entry(id.clone()).or_default() += 1;
            if let Some(c) = self.data.channel(id) {
                let mood = self.data.mood(c, world, &self.themes);
                self.music.want(id, &mood);
            }
        }
        self.heard = now;
        let flying = world
            .player
            .ship
            .as_ref()
            .and_then(|s| world.ships.get(s))
            .is_some_and(|s| s.docked_at.is_none());
        let current = self.current(&heard).cloned();
        // A hand-tuned channel that has gone out of range lets go: the strongest plays.
        if current
            .as_ref()
            .is_none_or(|c| self.tuned.as_ref() != Some(&self.data.channels[c.index].id))
        {
            self.tuned = None;
        }
        let mut frame = RadioFrame {
            ui: RadioUi {
                show: flying,
                on: self.on,
                tuned: self.tuned.is_some(),
                others: heard.len().saturating_sub(1),
                ..RadioUi::default()
            },
            ..RadioFrame::default()
        };
        let Some(current) = current else {
            return frame;
        };
        let spec = &self.data.channels[current.index];
        frame.ui.channel = Some(spec.name.clone());
        frame.ui.signal = current.signal;
        frame.ui.bits = self
            .bits
            .on(&spec.id)
            .iter()
            .map(|(npc, text)| {
                let who = world
                    .npcs
                    .get(npc)
                    .map_or(npc.as_str(), |n| n.name.as_str());
                let first = who.split_whitespace().next().unwrap_or(who);
                format!("{first}: {text}")
            })
            .chain(spec.bits.iter().cloned())
            .collect();
        if !self.on || !flying {
            return frame;
        }
        // Tuned by hand: that channel alone. Else every channel heard, crossfaded.
        let voices: Vec<&Heard> = if self.tuned.is_some() {
            vec![&current]
        } else {
            heard.iter().collect()
        };
        let gains = sound::crossfade(&voices.iter().map(|h| h.signal).collect::<Vec<_>>());
        frame.mix.voices = voices
            .iter()
            .zip(gains)
            .map(|(h, gain)| {
                let id = &self.data.channels[h.index].id;
                sound::Voice {
                    key: id.clone(),
                    source: match self.music.track(id) {
                        TrackSource::Generated(p) => sound::Source::File(p),
                        TrackSource::Placeholder(n) => sound::Source::Placeholder(n.to_owned()),
                    },
                    gain,
                }
            })
            .collect();
        frame.mix.static_gain = sound::static_level(current.signal, &self.data.falloff);
        frame
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data")
    }

    #[test]
    fn the_core_radio_file_loads_checks_and_covers_every_station() {
        let data = RadioData::load(&data_dir()).unwrap();
        let state = galaxy_core::new_game(&data_dir(), 1).unwrap();
        data.check(&state.world).unwrap();
        for s in state.world.stations.keys() {
            assert!(
                data.channels
                    .iter()
                    .any(|c| matches!(&c.source, Source::Station(x) if x == s)),
                "{s} has no channel"
            );
        }
        assert!(
            data.channels
                .iter()
                .any(|c| matches!(c.source, Source::Beacon(_))),
            "a beacon or two"
        );
        // Every station's mood comes from its theme.
        let themes = Themes::load(&data_dir()).unwrap();
        let kettle = data.channel("radio.kettle_dock").unwrap();
        let mood = data.mood(kettle, &state.world, &themes);
        assert!(
            mood.contains("Kettle Dock") && mood.contains("kettle whistle"),
            "{mood}"
        );
    }

    #[test]
    fn bad_radio_files_are_refused_with_every_problem() {
        let text = r#"
            [signal]
            near = 2.0
            static_below = 0.5
            static_max = 0.3
            [[beacon]]
            id = "lamp"
            name = "A lamp"
            station = "station.lantern_wharf"
            region = "region.lantern_shallows"
            position = { x = 0.0, y = 0.0 }
            [[channel]]
            id = "radio.a"
            source = "station.lantern_wharf"
            name = "A"
            range = -5.0
            bits = ["The kettle is on.", "We have 12 crates."]
            [[channel]]
            id = "radio.b"
            source = "station.lantern_wharf"
            name = "B"
            range = 100.0
            [[channel]]
            id = "radio.c"
            source = "lamp"
            name = "C"
            range = 100.0
        "#;
        let err = RadioData::parse(text).unwrap_err();
        for want in [
            "signal.near",
            "beacon `lamp`",
            "range -5",
            "says I, we or our",
            "no digits",
            "already has a channel",
            "needs a mood",
        ] {
            assert!(err.contains(want), "missing `{want}` in:\n{err}");
        }
        assert!(RadioData::parse("[[channel]]\nid = \"radio.x\"\nsurprise = 1\n").is_err());
        // References into the world.
        let bad = RadioData::parse(
            "[[channel]]\nid = \"radio.x\"\nsource = \"station.nowhere\"\nname = \"X\"\nrange = 10.0\n",
        )
        .unwrap();
        let state = galaxy_core::new_game(&data_dir(), 1).unwrap();
        assert!(
            bad.check(&state.world)
                .unwrap_err()
                .contains("station.nowhere")
        );
    }
}
