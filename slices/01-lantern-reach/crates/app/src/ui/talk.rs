//! Talk at the dock: short bubbles over whoever speaks, click (or Enter) for the next.
//!
//! - **The talk book** ([`TalkBook`]) sorts every line the world says, once, after the
//!   tick that said it. Dock talk said while the flagship is docked waits in a queue for
//!   the station it was said at, oldest first, until the player has seen it. Anything
//!   said in flight, and every notice, goes to the ship log instead ([`super::log`]) and
//!   counts toward the log icon's "new" marker. Undocking ends the bubbles; what wasn't
//!   seen is shown at the next docking there.
//! - **Bubbles** ([`Bubble`], [`bubble`]): the oldest queued line at this dock, rising
//!   from the speaker's part of the station ([`TalkAnchors`], from the core pack's
//!   `data/talk_anchors.toml`). A line that offers answers shows them as buttons (keys
//!   1 to 9) and waits for one; any other line moves on with a click on it or Enter.
//!   Confirmations of what was given and got have no speaker: they show as a dark ticket
//!   marked "Confirmed".
//!
//! The book belongs to the save file, not the world, like the hint counts: it lives in
//! the app's own table ([`crate::saves::read_talk`]). Nothing here writes world state;
//! answers go out through the session's replies.

use std::collections::BTreeMap;
use std::path::Path;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::History;
use render::glam::Vec2 as G2;
use render::{Align, Color, Overlay, Panel, Rect, Text, palette};
use serde::Deserialize;

use super::{BLACK, Button, GAP, UiAction, UiState, WHITE, button, text_width};
use crate::hooks::{self, OpenChoice};

/// The colourblind-safe accents from the station concept. Each always comes with a
/// shape or a word.
pub const ORANGE: Color = Color::linear(0.7913, 0.3467, 0.0, 1.0);
pub const SKY: Color = Color::linear(0.0931, 0.4564, 0.8148, 1.0);
pub const BLUE: Color = Color::linear(0.0, 0.1683, 0.4452, 1.0);

/// How a line is shown: a person speaking, the dock confirming a transaction, or a
/// notice about a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Voice {
    Person,
    Confirmed,
    Notice,
}

impl Voice {
    pub fn of(line: &TalkLine) -> Self {
        match (line.channel, &line.speaker) {
            (Channel::DockTalk, Some(_)) => Voice::Person,
            (Channel::DockTalk, None) => Voice::Confirmed,
            _ => Voice::Notice,
        }
    }
}

/// What the talk book keeps in the save: the last line it sorted, the lines waiting to
/// be seen (talk ID, the station they wait at), and how many log entries are new.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TalkRecord {
    pub through: u64,
    pub log_new: u32,
    pub unseen: BTreeMap<u64, String>,
}

/// Which lines wait to be seen at which dock, and what's new in the log.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TalkBook {
    /// Every line up to this ID is sorted: queued, seen or sent to the log.
    through: u64,
    /// Dock talk said while docked and not seen yet, by ID (oldest first), with the
    /// station it waits at.
    unseen: BTreeMap<TalkId, StationId>,
    /// Log entries since the log was last opened: lines said in flight and notices.
    log_new: u32,
    /// Changed since [`TalkBook::take_changed`].
    changed: bool,
}

impl TalkBook {
    /// The book for a save that has no record yet (a new game, a beat jump, a save from
    /// before the log): everything said so far is sorted, except the current visit's
    /// dock talk, which waits to be seen.
    pub fn fresh(world: &WorldState, history: &History) -> Self {
        let mut book = Self {
            through: world.talk.last,
            ..Self::default()
        };
        if let Some(station) = docked_at(world) {
            let since = hooks::last_docking(world, history);
            for said in world.talk.on(Channel::DockTalk).filter(|l| l.tick >= since) {
                if said.line.validate().is_ok() {
                    book.unseen.insert(said.id, station.clone());
                }
            }
        }
        book
    }

    /// The book from the save's record. Lines it names that the world no longer has are
    /// dropped when they come up.
    pub fn from_record(record: &TalkRecord) -> Self {
        Self {
            through: record.through,
            unseen: record
                .unseen
                .iter()
                .filter_map(|(id, st)| Some((TalkId(*id), st.parse().ok()?)))
                .collect(),
            log_new: record.log_new,
            changed: false,
        }
    }

    /// The record for the save.
    pub fn record(&self) -> TalkRecord {
        TalkRecord {
            through: self.through,
            log_new: self.log_new,
            unseen: self
                .unseen
                .iter()
                .map(|(id, st)| (id.0, st.to_string()))
                .collect(),
        }
    }

    /// Whether the book changed since the last call (time to write it).
    pub fn take_changed(&mut self) -> bool {
        std::mem::take(&mut self.changed)
    }

    /// After a tick: sorts the lines it said. Dock talk said while docked waits for the
    /// player at that dock; everything else goes to the log.
    pub fn observe(&mut self, world: &WorldState) {
        if world.talk.last <= self.through {
            return;
        }
        let docked = docked_at(world);
        for said in world
            .talk
            .lines
            .range(TalkId(self.through + 1)..)
            .map(|(_, l)| l)
        {
            match (&docked, said.line.channel) {
                (Some(st), Channel::DockTalk) => {
                    if said.line.validate().is_ok() {
                        self.unseen.insert(said.id, st.clone());
                    }
                }
                (_, Channel::DockTalk | Channel::Chronicle | Channel::MapNote) => {
                    self.log_new = self.log_new.saturating_add(1);
                }
            }
        }
        self.through = world.talk.last;
        self.changed = true;
    }

    /// The lines waiting at `station`, oldest first.
    pub fn queue<'a>(&'a self, station: &'a StationId) -> impl Iterator<Item = TalkId> + 'a {
        self.unseen
            .iter()
            .filter(move |(_, st)| *st == station)
            .map(|(id, _)| *id)
    }

    /// How many lines wait at `station`.
    pub fn waiting(&self, station: &StationId) -> usize {
        self.queue(station).count()
    }

    /// The player has seen `id`.
    pub fn seen(&mut self, id: TalkId) {
        if self.unseen.remove(&id).is_some() {
            self.changed = true;
        }
    }

    /// The player answered `npc` with `choice`: the lines that asked are done.
    pub fn answered(&mut self, world: &WorldState, npc: &NpcId, choice: &AnyId) {
        let before = self.unseen.len();
        self.unseen.retain(|id, _| {
            !world.talk.lines.get(id).is_some_and(|l| {
                l.line.speaker.as_ref() == Some(npc)
                    && l.line.choices.iter().any(|c| &c.id == choice)
            })
        });
        self.changed |= self.unseen.len() != before;
    }

    /// Marks every line waiting at `station` seen, except those still waiting for an
    /// answer (`open`).
    pub fn skip(&mut self, world: &WorldState, station: &StationId, open: &[OpenChoice]) {
        let before = self.unseen.len();
        self.unseen.retain(|id, st| {
            st != station
                || world
                    .talk
                    .lines
                    .get(id)
                    .is_some_and(|l| !choices_open(&l.line, open).is_empty())
        });
        self.changed |= self.unseen.len() != before;
    }

    /// Log entries since the log was last opened.
    pub fn log_new(&self) -> u32 {
        self.log_new
    }

    /// The log was opened: nothing in it is new any more.
    pub fn read_log(&mut self) {
        if self.log_new > 0 {
            self.log_new = 0;
            self.changed = true;
        }
    }
}

fn docked_at(world: &WorldState) -> Option<StationId> {
    world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .and_then(|s| s.docked_at.clone())
}

/// The open answers `line` offers, with their 0-based place in `open` (the number keys'
/// order).
pub fn choices_open(line: &TalkLine, open: &[OpenChoice]) -> Vec<(usize, OpenChoice)> {
    let Some(npc) = &line.speaker else {
        return Vec::new();
    };
    open.iter()
        .enumerate()
        .filter(|(_, o)| &o.npc == npc && line.choices.iter().any(|c| c.id == o.choice))
        .map(|(i, o)| (i, o.clone()))
        .collect()
}

/// One bubble on screen.
#[derive(Clone, Debug, PartialEq)]
pub struct Bubble {
    /// The line in the world's talk log, if it is one (a "What changed?" notice or the
    /// "nothing new" answer isn't).
    pub talk: Option<TalkId>,
    pub line: TalkLine,
    pub voice: Voice,
    /// Lines still to come here after this one.
    pub more: usize,
    /// The answers it offers that are still open, with their 0-based number.
    pub choices: Vec<(usize, OpenChoice)>,
}

impl Bubble {
    pub fn new(talk: Option<TalkId>, line: TalkLine, open: &[OpenChoice], more: usize) -> Self {
        Self {
            choices: choices_open(&line, open),
            voice: Voice::of(&line),
            talk,
            line,
            more,
        }
    }

    /// Whether a click (or Enter) moves past it: not while it waits for an answer.
    pub fn advances(&self) -> bool {
        self.choices.is_empty()
    }

    /// Whether a click on it does anything: it moves on, or, waiting for an answer with
    /// other lines behind it, it steps aside for them.
    pub fn clickable(&self) -> bool {
        self.advances() || self.more > 0
    }
}

/// Where a speaker's bubble rises from: a spot on the station's art. From the core
/// pack's `data/talk_anchors.toml`; without it, everyone speaks from the hub.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TalkAnchors {
    pub default: String,
    pub notices: String,
    pub spots: BTreeMap<String, Spot>,
    #[serde(default)]
    pub roles: BTreeMap<String, String>,
    #[serde(default)]
    pub people: BTreeMap<String, String>,
}

/// A point on a station: an angle in degrees (0 east, counterclockwise) and a reach as
/// a fraction of the docking ring's radius.
#[derive(Clone, Copy, Debug, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spot {
    pub angle: f32,
    pub reach: f32,
}

impl Default for TalkAnchors {
    fn default() -> Self {
        Self {
            default: "hub".into(),
            notices: "hub".into(),
            spots: BTreeMap::from([(
                "hub".to_owned(),
                Spot {
                    angle: 90.0,
                    reach: 0.0,
                },
            )]),
            roles: BTreeMap::new(),
            people: BTreeMap::new(),
        }
    }
}

impl TalkAnchors {
    pub const FILE: &'static str = "talk_anchors.toml";

    /// Reads `talk_anchors.toml` from a pack's data folder. No file: the hub for all.
    pub fn load(data: &Path) -> Result<Self, String> {
        let path = data.join(Self::FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(format!("{}: {e}", path.display())),
        };
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parses the file and checks every name points at a spot.
    pub fn parse(text: &str) -> Result<Self, String> {
        let a: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        let named = [("default", &a.default), ("notices", &a.notices)]
            .into_iter()
            .chain(a.roles.iter().map(|(k, v)| (k.as_str(), v)))
            .chain(a.people.iter().map(|(k, v)| (k.as_str(), v)));
        for (who, spot) in named {
            if !a.spots.contains_key(spot) {
                return Err(format!("`{who}` speaks from `{spot}`, which isn't a spot"));
            }
        }
        for (name, s) in &a.spots {
            if !(0.0..=1.5).contains(&s.reach) || !s.angle.is_finite() {
                return Err(format!("spot `{name}`: reach is 0 to 1.5, angle a number"));
            }
        }
        Ok(a)
    }

    /// The spot a line rises from at `station`: its speaker's (by person, then role)
    /// when they're at this station, else the default; a line with no speaker, the
    /// notices' spot.
    pub fn spot_name(&self, world: &WorldState, line: &TalkLine, station: &StationId) -> &str {
        let Some(npc) = &line.speaker else {
            return &self.notices;
        };
        let here = world
            .npcs
            .get(npc)
            .filter(|n| n.location.as_ref() == Some(station));
        let Some(n) = here else {
            return &self.default;
        };
        self.people
            .get(npc.as_str())
            .or_else(|| self.roles.get(&n.role))
            .unwrap_or(&self.default)
    }

    /// The world point a line's bubble rises from at `station`.
    pub fn anchor(&self, world: &WorldState, line: &TalkLine, station: &StationId) -> Option<Vec2> {
        let st = world.stations.get(station)?;
        let spot = self.spots.get(self.spot_name(world, line, station))?;
        let a = spot.angle.to_radians();
        let r = spot.reach * st.dock_radius;
        Some(st.position + Vec2::new(a.cos() * r, a.sin() * r))
    }
}

/// The bubble's text size, row height, padding and widest text.
const SIZE: f32 = 17.0;
const ROW: f32 = 22.0;
const PAD: f32 = 16.0;
const WIDE: f32 = 420.0;
const HEAD: f32 = 26.0;
const FOOT: f32 = 26.0;
const CHOICE_H: f32 = 34.0;
/// How far the bubble sits from the spot it rises from.
const LIFT: f32 = 44.0;

/// The bubble laid out: its rect, the spot its pointer reaches down (or up) to, the
/// text wrap width and the answer buttons' rows.
#[derive(Clone, Debug, PartialEq)]
pub struct BubbleLayout {
    pub rect: Rect,
    /// `None`: no pointer (the log's replay).
    pub pointer: Option<G2>,
    wrap: f32,
    /// Each answer button's rect, in `choices` order.
    choice_rects: Vec<Rect>,
}

fn choice_label(i: usize, c: &OpenChoice) -> String {
    format!("{}  {}", i + 1, c.text)
}

fn choice_w(i: usize, c: &OpenChoice) -> f32 {
    text_width(&choice_label(i, c), 15.0) + 28.0
}

/// The words over the text: who speaks and where, or what the line is.
pub fn head(world: &WorldState, b: &Bubble) -> String {
    let place = hooks::place(world, &b.line);
    match b.voice {
        Voice::Person => match (hooks::speaker(world, &b.line), place) {
            (Some(w), Some(p)) => format!("{w}  ·  {p}"),
            (Some(w), None) => w,
            (None, p) => p.unwrap_or_default(),
        },
        Voice::Confirmed => match place {
            Some(p) => format!("Confirmed  ·  {p}"),
            None => "Confirmed".to_owned(),
        },
        Voice::Notice => match place {
            Some(p) => format!("Notice  ·  {p}"),
            None => "Notice".to_owned(),
        },
    }
}

/// The literal hint at the bubble's foot.
fn foot(b: &Bubble) -> String {
    if !b.advances() && b.more > 0 {
        return format!(
            "Pick an answer, or click or Enter · later  ({} more)",
            b.more
        );
    }
    if !b.advances() {
        return "Pick an answer: click it or press its number".to_owned();
    }
    match b.more {
        0 => "Click or Enter · close".to_owned(),
        n => format!("Click or Enter · next  ({n} more)"),
    }
}

/// Lays a bubble out over `anchor` (a canvas point), inside `bounds`: above the spot if
/// it fits, else below it, never outside `bounds`. A bubble too tall for the bounds
/// gets wider before it gets taller.
pub fn layout(b: &Bubble, world: &WorldState, anchor: G2, bounds: Rect) -> BubbleLayout {
    let head_w = text_width(&head(world, b), 14.0);
    // The foot shares its row with Skip when more lines wait.
    let foot_w = text_width(&foot(b), 13.0) + if b.more > 0 { 80.0 } else { 0.0 };
    let measure = |wrap: f32| {
        let rows = (text_width(&b.line.text, SIZE) / wrap).ceil().max(1.0);
        // Answer buttons in rows that fit the wrap width.
        let mut rects = Vec::new();
        let (mut x, mut y) = (0.0, 0.0);
        for (i, c) in &b.choices {
            let w = choice_w(*i, c).min(wrap);
            if x > 0.0 && x + w > wrap {
                x = 0.0;
                y += CHOICE_H + 8.0;
            }
            rects.push(Rect::new(x, y, w, CHOICE_H));
            x += w + 8.0;
        }
        let choices_h = if rects.is_empty() {
            0.0
        } else {
            y + CHOICE_H + 10.0
        };
        let natural = text_width(&b.line.text, SIZE).min(wrap);
        let w = natural.max(head_w).max(foot_w).max(160.0).min(wrap) + 2.0 * PAD;
        let h = PAD * 0.5 + HEAD + rows * ROW + 8.0 + choices_h + FOOT + PAD * 0.25;
        (w, h, rects)
    };
    let max_wrap = (bounds.w - 2.0 * PAD).max(120.0);
    let mut wrap = WIDE.min(max_wrap);
    let (mut w, mut h, mut rects) = measure(wrap);
    while h > bounds.h && wrap < max_wrap {
        wrap = (wrap + 80.0).min(max_wrap);
        (w, h, rects) = measure(wrap);
    }
    let x = (anchor.x - w * 0.5).clamp(bounds.x, (bounds.x + bounds.w - w).max(bounds.x));
    let above = anchor.y - LIFT - h;
    let y = if above >= bounds.y {
        above
    } else if anchor.y + LIFT + h <= bounds.y + bounds.h {
        anchor.y + LIFT
    } else {
        // Neither fits whole: as high as the bounds allow.
        bounds.y.max(bounds.y + bounds.h - h)
    };
    let rect = Rect::new(x, y, w, h);
    let base = G2::new(
        rect.x + PAD,
        rect.y
            + PAD * 0.5
            + HEAD
            + (text_width(&b.line.text, SIZE) / wrap).ceil().max(1.0) * ROW
            + 8.0,
    );
    let choice_rects = rects
        .into_iter()
        .map(|r| Rect::new(base.x + r.x, base.y + r.y, r.w, r.h))
        .collect();
    BubbleLayout {
        rect,
        pointer: Some(anchor),
        wrap,
        choice_rects,
    }
}

/// Draws a bubble laid out by [`layout`], with its pointer. Returns its buttons: the
/// answers, or the bubble itself to move on (`advance`), and "Skip" when more lines
/// wait (`skip`).
pub fn draw(
    o: &mut Overlay,
    world: &WorldState,
    b: &Bubble,
    l: &BubbleLayout,
    ui: &UiState,
    (advance, skip): (UiAction, Option<UiAction>),
) -> Vec<Button> {
    let r = l.rect;
    let (fill, border, ink, radius) = match b.voice {
        Voice::Person => (palette::PARCHMENT, palette::INK, BLACK, 16.0),
        // A ticket: square corners, dark, with its word.
        Voice::Confirmed => (palette::INK, ORANGE, WHITE, 3.0),
        Voice::Notice => (palette::INK, SKY, WHITE, 3.0),
    };
    if let Some(a) = l.pointer {
        pointer(o, r, a, fill, border);
    }
    o.panels.push(Panel {
        fill,
        border,
        border_px: 3.0,
        radius,
        ..Panel::new(r)
    });
    let x = r.x + PAD;
    let mut y = r.y + PAD * 0.5;
    if b.voice == Voice::Person {
        // The speaker's name on a tag.
        let name = head(world, b);
        let tag = Rect::new(x - 6.0, y + 1.0, text_width(&name, 14.0) + 14.0, 22.0);
        o.panels.push(Panel {
            fill: ORANGE,
            border: palette::INK,
            border_px: 1.5,
            radius: 6.0,
            shadow: false,
            ..Panel::new(tag)
        });
        o.texts.push(
            Text::new(name, G2::new(tag.x + 7.0, tag.y + 3.0), 14.0)
                .weight(700)
                .color(BLACK),
        );
    } else {
        o.texts.push(
            Text::new(head(world, b), G2::new(x, y + 4.0), 14.0)
                .weight(700)
                .color(ink),
        );
    }
    y += HEAD;
    o.texts.push(
        Text::new(b.line.text.clone(), G2::new(x, y), SIZE)
            .weight(500)
            .color(ink)
            .wrap(l.wrap),
    );
    let mut buttons = Vec::new();
    if b.clickable() {
        // The whole bubble moves on (or aside); drawn first so the answers and Skip
        // sit on top.
        buttons.push(Button {
            rect: r,
            action: advance,
        });
    }
    for ((i, c), rect) in b.choices.iter().zip(&l.choice_rects) {
        buttons.push(button(
            o,
            ui,
            *rect,
            &choice_label(*i, c),
            true,
            UiAction::Reply {
                npc: c.npc.clone(),
                choice: c.choice.clone(),
            },
        ));
    }
    let foot_y = r.y + r.h - FOOT - PAD * 0.25 + 4.0;
    o.texts.push(
        Text::new(foot(b), G2::new(x, foot_y), 13.0)
            .weight(600)
            .color(ink),
    );
    if let Some(skip) = skip.filter(|_| b.more > 0) {
        let rect = Rect::new(r.x + r.w - 74.0, foot_y - 4.0, 62.0, 22.0);
        o.panels.push(Panel {
            fill: fill.with_alpha(0.0),
            border: ink,
            border_px: 1.5,
            radius: 6.0,
            shadow: false,
            ..Panel::new(rect)
        });
        o.texts.push(
            Text::new("Skip", G2::new(rect.center().x, rect.y + 2.0), 13.0)
                .weight(700)
                .color(ink)
                .align(Align::Center),
        );
        buttons.push(Button { rect, action: skip });
    }
    buttons
}

/// A pointer from the bubble to the spot it rises from: three dots stepping down to a
/// ring on the spot.
fn pointer(o: &mut Overlay, r: Rect, anchor: G2, fill: Color, border: Color) {
    let below = anchor.y > r.y + r.h * 0.5;
    let from = G2::new(
        anchor.x.clamp(r.x + 20.0, r.x + r.w - 20.0),
        if below { r.y + r.h } else { r.y },
    );
    for (t, size) in [(0.3, 13.0), (0.6, 9.0)] {
        let p = from + (anchor - from) * t;
        o.panels.push(Panel {
            fill,
            border,
            border_px: 2.0,
            radius: size * 0.5,
            shadow: false,
            ..Panel::new(Rect::new(p.x - size * 0.5, p.y - size * 0.5, size, size))
        });
    }
    // The spot itself: an orange ring, not a fill alone.
    o.panels.push(Panel {
        fill: ORANGE.with_alpha(0.0),
        border: ORANGE,
        border_px: 3.0,
        radius: 9.0,
        shadow: false,
        ..Panel::new(Rect::new(anchor.x - 9.0, anchor.y - 9.0, 18.0, 18.0))
    });
}

/// The bubble now showing at the dock, at `anchor`, inside `bounds`. Returns its buttons.
pub fn dock_bubble(
    o: &mut Overlay,
    world: &WorldState,
    b: &Bubble,
    anchor: G2,
    bounds: Rect,
    ui: &UiState,
) -> (Rect, Vec<Button>) {
    let l = layout(b, world, anchor, bounds);
    let buttons = draw(
        o,
        world,
        b,
        &l,
        ui,
        (UiAction::NextLine, Some(UiAction::SkipTalk)),
    );
    (l.rect, buttons)
}

/// The room bubbles have while docked: right of the margin, left of the dock screen
/// (`card`), under the goal banner and over the log icon.
pub fn bounds(goal: Option<Rect>, card: Option<Rect>, log_icon: Rect, canvas: G2) -> Rect {
    let left = super::MARGIN;
    let right = card.map_or(canvas.x - super::MARGIN, |c| c.x - GAP);
    let top = goal.map_or(super::MARGIN, |g| g.y + g.h + GAP);
    let bottom = log_icon.y - GAP;
    Rect::new(left, top, (right - left).max(0.0), (bottom - top).max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_core_pack_anchors_parse_and_the_dockmaster_speaks_from_the_office() {
        let data =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let a = TalkAnchors::load(&data).unwrap();
        assert_eq!(
            a.roles.get("dockmaster").map(String::as_str),
            Some("office")
        );
        let state = galaxy_core::new_game(&data, 1).unwrap();
        let w = &state.world;
        let wharf = StationId::from_static("station.lantern_wharf");
        let line = |speaker: Option<&'static str>| TalkLine {
            channel: Channel::DockTalk,
            speaker: speaker.map(NpcId::from_static),
            place: Some(wharf.clone()),
            text: "Hello.".into(),
            cites: vec![FactId(1)],
            about: Vec::new(),
            choices: Vec::new(),
        };
        assert_eq!(
            a.spot_name(w, &line(Some("npc.ilse_marrow")), &wharf),
            "office"
        );
        assert_eq!(
            a.spot_name(w, &line(Some("npc.orrin_vale")), &wharf),
            "cradle"
        );
        // Someone from elsewhere, and a confirmation, rise from the hub.
        assert_eq!(a.spot_name(w, &line(Some("npc.odo_fenn")), &wharf), "hub");
        assert_eq!(a.spot_name(w, &line(None), &wharf), "hub");
        let office = a.anchor(w, &line(Some("npc.ilse_marrow")), &wharf).unwrap();
        let hub = a.anchor(w, &line(None), &wharf).unwrap();
        assert!(
            office.distance(hub) > 1.0,
            "a part of the station, not its centre"
        );
    }

    #[test]
    fn anchors_refuse_a_missing_spot() {
        assert!(TalkAnchors::parse("default = \"hub\"\nnotices = \"hub\"\n[spots]\nhub = { angle = 0, reach = 0 }\n[roles]\ndockmaster = \"office\"\n").is_err());
        assert!(
            TalkAnchors::parse(
                "default = \"hub\"\nnotices = \"hub\"\n[spots]\nhub = { angle = 0, reach = 0 }\n"
            )
            .is_ok()
        );
    }
}
