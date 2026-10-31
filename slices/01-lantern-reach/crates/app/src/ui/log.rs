//! The ship log: one small icon bottom left, and a panel that never grows with history.
//!
//! Everything in it is read from what the save already holds: the flagship's docking
//! and undocking facts mark the visits, the world's talk log holds what was said, and
//! history holds the trades and confirmations. Nothing is stored for it but the "new"
//! count ([`super::talk::TalkBook`]).
//!
//! - **Pinned** on top: the active jobs (each beat's goal, under who gave it and where),
//!   cargo carried for someone, and offers left open (a line's answers nobody gave,
//!   from the latest visit to that dock).
//! - **Entries**, newest first, ordered by visits: one line per visit that had anything
//!   in it ("Kettle Dock · Odo, Tamsin" over "Delivered 20 ice to Odo · +120 cr; sold 5
//!   ice"), which replays that visit's bubbles when clicked; lines said in flight
//!   ("Said"); and notices about places ("Notice": world changes, map notes).
//! - Each kind has its own word and shape: a visit is an outlined card with "Replay", a
//!   line said is marked "Said", a notice is a dark ticket marked "Notice", a pin has a
//!   bar and its word ("Job", "Cargo", "Offer").
//! - The entries page with Newer and Older; the panel's height is fixed by the screen.

use std::collections::BTreeMap;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, SaidLine, TalkLine};
use galaxy_core::History;
use render::glam::Vec2 as G2;
use render::{Align, Overlay, Panel, Rect, Text, palette};

use super::talk::{self, BLUE, Bubble, ORANGE, SKY, Voice};
use super::{BLACK, Button, GAP, MARGIN, UiAction, UiState, WHITE, button, clip, text_width};
use crate::hooks;

/// One stay at a dock: from a docking to the undocking after it.
#[derive(Clone, Debug, PartialEq)]
pub struct Visit {
    pub station: StationId,
    /// The docking's tick: it names the visit.
    pub from: Tick,
    /// The undocking's tick; `None` while still docked.
    pub until: Option<Tick>,
    /// What was said at the dock during the visit, oldest first.
    pub lines: Vec<SaidLine>,
    /// What happened, in short plain words: confirmations first, then trades, then
    /// other changes.
    pub summary: Vec<String>,
}

impl Visit {
    fn holds(&self, tick: Tick) -> bool {
        tick >= self.from && self.until.is_none_or(|u| tick < u)
    }

    /// The people who spoke, by first name, in the order they first spoke.
    pub fn people(&self, world: &WorldState) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for l in &self.lines {
            if let Some(name) = hooks::speaker(world, &l.line) {
                let first = name.split_whitespace().next().unwrap_or(&name).to_owned();
                if !out.contains(&first) {
                    out.push(first);
                }
            }
        }
        out
    }

    /// The bubbles a replay shows: the dock talk, oldest first.
    pub fn bubbles(&self) -> Vec<&SaidLine> {
        self.lines
            .iter()
            .filter(|l| l.line.channel == Channel::DockTalk)
            .collect()
    }
}

/// The flagship's visits, oldest first, each with what was said and done.
pub fn visits(world: &WorldState, history: &History) -> Vec<Visit> {
    let Some(ship) = world.player.ship.as_ref() else {
        return Vec::new();
    };
    let mut visits: Vec<Visit> = Vec::new();
    for f in history.facts() {
        match &f.payload {
            FactPayload::Event(Event::Dock { ship: s, station }) if s == ship => {
                visits.push(Visit {
                    station: station.clone(),
                    from: f.tick,
                    until: None,
                    lines: Vec::new(),
                    summary: Vec::new(),
                });
            }
            FactPayload::Event(Event::Undock { ship: s, station }) if s == ship => {
                match visits.last_mut() {
                    Some(v) if v.until.is_none() => v.until = Some(f.tick),
                    Some(_) => {}
                    // A new game starts docked, with no docking fact: that first stay
                    // is a visit too.
                    None => visits.push(Visit {
                        station: station.clone(),
                        from: Tick(0),
                        until: Some(f.tick),
                        lines: Vec::new(),
                        summary: Vec::new(),
                    }),
                }
            }
            _ => {}
        }
    }
    // ... and is still going.
    let docked = world.ships.get(ship).and_then(|s| s.docked_at.clone());
    if let Some(st) = docked.as_ref().filter(|_| visits.is_empty()) {
        visits.push(Visit {
            station: st.clone(),
            from: Tick(0),
            until: None,
            lines: Vec::new(),
            summary: Vec::new(),
        });
    }
    if visits.is_empty() {
        return visits;
    }
    let find = |visits: &[Visit], tick: Tick| -> Option<usize> {
        let i = visits.partition_point(|v| v.from <= tick).checked_sub(1)?;
        visits[i].holds(tick).then_some(i)
    };
    for said in world.talk.on(Channel::DockTalk) {
        if let Some(i) = find(&visits, said.tick) {
            visits[i].lines.push(said.clone());
        }
    }
    // Trades by ware and side, then the plain digest of other changes.
    let mut trades: Vec<BTreeMap<(bool, WareId), u32>> = vec![BTreeMap::new(); visits.len()];
    let mut changes: Vec<Vec<String>> = vec![Vec::new(); visits.len()];
    for f in history.facts() {
        let Some(i) = find(&visits, f.tick) else {
            continue;
        };
        match &f.payload {
            FactPayload::Event(Event::Sell {
                ship: s,
                ware,
                quantity,
                ..
            }) if s == ship => *trades[i].entry((true, ware.clone())).or_default() += quantity,
            FactPayload::Event(Event::Buy {
                ship: s,
                ware,
                quantity,
                ..
            }) if s == ship => *trades[i].entry((false, ware.clone())).or_default() += quantity,
            _ => {
                if let Some(text) = hooks::digest(world, f) {
                    changes[i].push(text.trim_end_matches('.').to_owned());
                }
            }
        }
    }
    for (i, v) in visits.iter_mut().enumerate() {
        let confirmed = v
            .lines
            .iter()
            .filter(|l| Voice::of(&l.line) == Voice::Confirmed)
            .map(|l| l.line.text.clone());
        let traded = trades[i].iter().map(|((sold, ware), n)| {
            let name = world
                .wares
                .get(ware)
                .map_or_else(|| ware.to_string(), |w| w.name.to_lowercase());
            format!("{} {n} {name}", if *sold { "sold" } else { "bought" })
        });
        v.summary = confirmed
            .chain(traded)
            .chain(std::mem::take(&mut changes[i]))
            .collect();
    }
    visits
}

/// One entry in the log's list.
#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A visit with anything in it; its index in [`visits`].
    Visit(usize),
    /// A person's line said while the flagship was flying.
    Said(SaidLine),
    /// A notice about a place: a world change or a note on the map.
    Notice(SaidLine),
}

/// The log's entries, newest first, ordered by visits: each visit that had anything in
/// it, then what was said or noticed after it, before the next.
pub fn entries(world: &WorldState, visits: &[Visit]) -> Vec<Entry> {
    // (tick, order within the tick, entry), oldest first.
    let mut out: Vec<(Tick, u64, Entry)> = visits
        .iter()
        .enumerate()
        .filter(|(_, v)| !v.lines.is_empty() || !v.summary.is_empty())
        .map(|(i, v)| (v.from, 0, Entry::Visit(i)))
        .collect();
    let docked_then = |tick: Tick| visits.iter().any(|v| v.holds(tick));
    for said in world.talk.lines.values() {
        let entry = match said.line.channel {
            Channel::Chronicle | Channel::MapNote => Entry::Notice(said.clone()),
            // Said at a dock: it's in that visit.
            Channel::DockTalk if docked_then(said.tick) => continue,
            Channel::DockTalk => match Voice::of(&said.line) {
                Voice::Person => Entry::Said(said.clone()),
                _ => Entry::Notice(said.clone()),
            },
        };
        out.push((said.tick, said.id.0, entry));
    }
    out.sort_by_key(|(tick, order, _)| (*tick, *order));
    out.into_iter().rev().map(|(_, _, e)| e).collect()
}

/// A pinned row: its word, who and where, and what.
#[derive(Clone, Debug, PartialEq)]
pub struct Pin {
    /// "Job", "Cargo" or "Offer".
    pub tag: &'static str,
    pub head: String,
    pub body: String,
}

/// The pinned rows: active jobs, cargo carried for someone, open offers.
pub fn pinned(world: &WorldState, history: &History, visits: &[Visit]) -> Vec<Pin> {
    let mut out: Vec<Pin> = hooks::goals(world)
        .into_iter()
        .map(|g| Pin {
            tag: "Job",
            head: g.source,
            body: g.goal,
        })
        .collect();
    let ship = world.player.ship.as_ref().and_then(|s| world.ships.get(s));
    let name = |n: &NpcId| {
        world
            .npcs
            .get(n)
            .map_or_else(|| n.to_string(), |n| n.name.clone())
    };
    let station = |s: &StationId| {
        world
            .stations
            .get(s)
            .map_or_else(|| s.to_string(), |s| s.name.clone())
    };
    for c in ship.iter().flat_map(|s| s.hold.consigned.values()) {
        let ware = world
            .wares
            .get(&c.ware)
            .map_or_else(|| c.ware.to_string(), |w| w.name.to_lowercase());
        let to = name(&c.to);
        let first = to.split_whitespace().next().unwrap_or(&to).to_owned();
        let owner = world.npcs.get(&c.owner);
        out.push(Pin {
            tag: "Cargo",
            head: match owner.and_then(|o| o.location.as_ref()) {
                Some(at) => format!("{}  ·  {}", name(&c.owner), station(at)),
                None => name(&c.owner),
            },
            body: format!(
                "{} {ware} in your hold for {first} at {}. It isn't yours to sell.",
                c.quantity,
                station(&c.station)
            ),
        });
    }
    // Offers: from the latest visit to each dock that had talk in it.
    let replies: Vec<&Fact> = history
        .facts()
        .iter()
        .filter(|f| f.kind.as_str() == "reply")
        .collect();
    let mut latest: BTreeMap<&StationId, &Visit> = BTreeMap::new();
    for v in visits.iter().filter(|v| !v.lines.is_empty()) {
        latest.insert(&v.station, v);
    }
    for v in latest.values() {
        for said in &v.lines {
            let line = &said.line;
            let Some(npc) = &line.speaker else { continue };
            if line.choices.is_empty() {
                continue;
            }
            let answered = replies.iter().any(|r| {
                r.tick >= said.tick
                    && r.subjects.npc.as_ref() == Some(npc)
                    && line
                        .choices
                        .iter()
                        .any(|c| r.subjects.other.as_ref() == Some(&c.id))
            });
            if answered {
                continue;
            }
            let answers: Vec<&str> = line.choices.iter().map(|c| c.text.as_str()).collect();
            out.push(Pin {
                tag: "Offer",
                head: format!("{}  ·  {}", name(npc), station(&v.station)),
                // The answers first, so a long question never hides them.
                body: format!(
                    "{}, at {}. {}",
                    answers.join(" / "),
                    station(&v.station),
                    line.text
                ),
            });
        }
    }
    out
}

/// The log icon's size and where it sits: bottom left.
pub const ICON: f32 = 64.0;

pub fn icon_rect(canvas: G2) -> Rect {
    Rect::new(MARGIN, canvas.y - MARGIN - ICON, ICON, ICON)
}

/// The log icon: a book and "Log", and when anything is new, a blue tag with the count
/// and the word ("2 new"). Click it (or L) to open the log.
pub fn icon(o: &mut Overlay, new: u32, open: bool, ui: &UiState, canvas: G2) -> Vec<Button> {
    let r = icon_rect(canvas);
    let hover = ui.cursor.is_some_and(|c| super::contains(&r, c));
    o.panels.push(Panel {
        fill: palette::PARCHMENT,
        border: if hover || open { ORANGE } else { palette::INK },
        border_px: if hover || open { 4.0 } else { 3.0 },
        radius: 14.0,
        ..Panel::new(r)
    });
    // A little open book: two pages and a spine.
    let (cx, cy) = (r.x + r.w * 0.5, r.y + 24.0);
    for x in [cx - 16.0, cx + 1.0] {
        o.panels.push(Panel {
            fill: palette::CREAM,
            border: palette::INK,
            border_px: 2.0,
            radius: 3.0,
            shadow: false,
            ..Panel::new(Rect::new(x, cy - 11.0, 15.0, 21.0))
        });
    }
    o.texts.push(
        Text::new("Log", G2::new(cx, r.y + r.h - 24.0), 14.0)
            .weight(700)
            .color(BLACK)
            .align(Align::Center),
    );
    if new > 0 {
        let label = format!("{new} new");
        let w = text_width(&label, 13.0) + 16.0;
        let tag = Rect::new(r.x + r.w - 14.0, r.y - 10.0, w, 22.0);
        o.panels.push(Panel {
            fill: BLUE,
            border: palette::INK,
            border_px: 2.0,
            radius: 11.0,
            shadow: false,
            ..Panel::new(tag)
        });
        o.texts.push(
            Text::new(label, G2::new(tag.center().x, tag.y + 3.0), 13.0)
                .weight(700)
                .color(WHITE)
                .align(Align::Center),
        );
    }
    vec![Button {
        rect: r,
        action: UiAction::ToggleLog,
    }]
}

/// The icon and its "new" tag together, for layout.
pub fn icon_area(new: u32, canvas: G2) -> Rect {
    let r = icon_rect(canvas);
    if new == 0 {
        return r;
    }
    let w = text_width(&format!("{new} new"), 13.0) + 16.0;
    Rect::new(r.x, r.y - 10.0, r.w - 14.0 + w, r.h + 10.0)
}

/// The log panel's width.
pub const WIDTH: f32 = super::BUBBLE_ROOM;

/// Where the log panel goes: the left column, from the top to the icon. The goal banner
/// steps aside while it's open: the log pins every job with who gave it.
pub fn panel_rect(canvas: G2) -> Rect {
    let icon = icon_rect(canvas);
    Rect::new(MARGIN, MARGIN, WIDTH, icon.y - GAP - MARGIN)
}

/// A replay of one visit in the log: the visit (by its docking tick) and which line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Replay {
    pub from: Tick,
    pub line: usize,
}

/// Row sizes in the panel.
const PIN_HEAD: f32 = 17.0;
const BODY: f32 = 15.0;
const BODY_ROW: f32 = 19.0;
const ENTRY_GAP: f32 = 8.0;
const PAGER_H: f32 = 34.0;
/// Most pinned rows shown; the rest are counted.
const MAX_PINS: usize = 5;

/// What one list entry shows: its word, a head and up to two rows of body.
struct Row {
    tag: &'static str,
    head: String,
    body: String,
    action: Option<UiAction>,
}

fn row_of(world: &WorldState, visits: &[Visit], e: &Entry) -> Row {
    let station = |s: Option<&StationId>| {
        s.and_then(|s| world.stations.get(s))
            .map(|s| s.name.clone())
    };
    match e {
        Entry::Visit(i) => {
            let v = &visits[*i];
            let name = station(Some(&v.station)).unwrap_or_else(|| v.station.to_string());
            let people = v.people(world);
            let head = if people.is_empty() {
                name
            } else {
                format!("{name}  ·  {}", people.join(", "))
            };
            let body = if v.summary.is_empty() {
                // Nothing done: what was said first.
                v.bubbles()
                    .first()
                    .map(|l| format!("\u{201c}{}\u{201d}", l.line.text))
                    .unwrap_or_default()
            } else {
                v.summary.join("; ")
            };
            Row {
                tag: "Visit",
                head,
                body,
                action: (!v.bubbles().is_empty()).then_some(UiAction::Replay(Some(Replay {
                    from: v.from,
                    line: 0,
                }))),
            }
        }
        Entry::Said(said) => {
            let who = hooks::speaker(world, &said.line).unwrap_or_default();
            let head = match station(said.line.place.as_ref()) {
                Some(p) => format!("{who}  ·  {p}  ·  in flight"),
                None => format!("{who}  ·  in flight"),
            };
            Row {
                tag: "Said",
                head,
                body: said.line.text.clone(),
                action: None,
            }
        }
        Entry::Notice(said) => Row {
            tag: "Notice",
            head: station(said.line.place.as_ref()).unwrap_or_else(|| "The Reach".to_owned()),
            body: said.line.text.clone(),
            action: None,
        },
    }
}

/// The text width rows wrap at inside the panel.
fn inner(rect: &Rect) -> f32 {
    rect.w - 40.0
}

/// How tall a row is: the head, and its body in at most two rows.
fn row_height(body: &str, wrap: f32) -> f32 {
    let rows = (text_width(body, BODY) / wrap).ceil().clamp(1.0, 2.0);
    12.0 + PIN_HEAD + rows * BODY_ROW + 6.0
}

/// The log panel: pinned rows, then a page of entries, or one visit replayed. Returns
/// its buttons, a background one first so clicks on the panel stay on it.
pub fn panel(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    ui: &UiState,
    canvas: G2,
) -> Vec<Button> {
    let rect = panel_rect(canvas);
    let visits = visits(world, history);
    let mut buttons = vec![Button {
        rect,
        action: UiAction::Absorb,
    }];
    o.panels.push(Panel {
        fill: palette::PARCHMENT,
        border: palette::INK,
        border_px: 3.0,
        radius: 16.0,
        ..Panel::new(rect)
    });
    let x = rect.x + 20.0;
    let wrap = inner(&rect);
    let close = Rect::new(rect.x + rect.w - 20.0 - 110.0, rect.y + 14.0, 110.0, 30.0);
    if let Some(replay) = ui.replay
        && let Some(v) = visits.iter().find(|v| v.from == replay.from)
    {
        o.texts.push(
            Text::new(
                format!(
                    "Replay  ·  {}",
                    world
                        .stations
                        .get(&v.station)
                        .map_or(v.station.as_str(), |s| s.name.as_str())
                ),
                G2::new(x, rect.y + 16.0),
                24.0,
            )
            .weight(700)
            .color(BLACK),
        );
        buttons.push(button(
            o,
            ui,
            close,
            "Back to log",
            true,
            UiAction::Replay(None),
        ));
        buttons.extend(replay_bubble(o, world, v, replay, ui, rect));
        return buttons;
    }
    o.texts.push(
        Text::new("Ship log", G2::new(x, rect.y + 16.0), 24.0)
            .weight(700)
            .color(BLACK),
    );
    buttons.push(button(
        o,
        ui,
        close,
        "Close  ·  L",
        true,
        UiAction::ToggleLog,
    ));
    let mut y = rect.y + 58.0;
    // Pinned: a bar and a word on each.
    let pins = pinned(world, history, &visits);
    // Two rows each for a pin or two; one each when there are more, so the list keeps
    // its room.
    let pin_rows = if pins.len() <= 2 { 2.0 } else { 1.0 };
    for p in pins.iter().take(MAX_PINS) {
        let body = clip(&p.body, BODY, (wrap - 90.0) * pin_rows);
        let h = row_height(&body, wrap - 90.0);
        o.panels.push(Panel {
            fill: ORANGE,
            border: palette::INK,
            border_px: 0.0,
            radius: 2.0,
            shadow: false,
            ..Panel::new(Rect::new(x, y + 2.0, 5.0, h - 10.0))
        });
        o.texts.push(
            Text::new(p.tag.to_uppercase(), G2::new(x + 14.0, y + 5.0), 13.0)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(p.head.clone(), G2::new(x + 80.0, y + 4.0), 13.0)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(body, G2::new(x + 80.0, y + 4.0 + PIN_HEAD + 2.0), BODY)
                .weight(500)
                .color(BLACK)
                .wrap(wrap - 90.0),
        );
        y += h - 6.0;
    }
    if pins.len() > MAX_PINS {
        o.texts.push(
            Text::new(
                format!("and {} more pinned", pins.len() - MAX_PINS),
                G2::new(x + 80.0, y),
                13.0,
            )
            .weight(600)
            .color(BLACK),
        );
        y += 20.0;
    }
    if !pins.is_empty() {
        y += 6.0;
        o.panels.push(Panel {
            fill: palette::INK,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(x, y, wrap, 2.0))
        });
        y += 12.0;
    }
    let all = entries(world, &visits);
    if all.is_empty() {
        o.texts.push(
            Text::new(
                "Nothing yet. Each visit to a dock gets a line here.",
                G2::new(x, y),
                BODY,
            )
            .weight(500)
            .color(BLACK),
        );
        return buttons;
    }
    o.texts.push(
        Text::new(
            "Newest first. Click a visit to replay what was said.",
            G2::new(x, y),
            13.0,
        )
        .weight(600)
        .color(BLACK),
    );
    y += 24.0;
    // A page of entries: as many as fit above the pager.
    let bottom = rect.y + rect.h - 14.0 - PAGER_H;
    let first = ui.log_first.min(all.len() - 1);
    let mut shown = 0;
    for e in &all[first..] {
        let row = row_of(world, &visits, e);
        let body = clip(&row.body, BODY, (wrap - 90.0) * 2.0);
        let h = row_height(&body, wrap - 90.0);
        if y + h > bottom && shown > 0 {
            break;
        }
        let r = Rect::new(x, y, wrap, h);
        draw_row(o, ui, &row, &body, r);
        if let Some(a) = row.action {
            buttons.push(Button { rect: r, action: a });
        }
        y += h + ENTRY_GAP;
        shown += 1;
    }
    let last = first + shown;
    if first > 0 || last < all.len() {
        let py = rect.y + rect.h - 12.0 - PAGER_H + 4.0;
        o.texts.push(
            Text::new(
                format!("{}–{} of {}", first + 1, last, all.len()),
                G2::new(x, py + 6.0),
                13.0,
            )
            .weight(600)
            .color(BLACK),
        );
        let bw = 96.0;
        buttons.push(button(
            o,
            ui,
            Rect::new(rect.x + rect.w - 20.0 - 2.0 * bw - 8.0, py, bw, 28.0),
            "Newer",
            first > 0,
            UiAction::LogFrom(first.saturating_sub(shown.max(1))),
        ));
        buttons.push(button(
            o,
            ui,
            Rect::new(rect.x + rect.w - 20.0 - bw, py, bw, 28.0),
            "Older",
            last < all.len(),
            UiAction::LogFrom(last.min(all.len() - 1)),
        ));
    }
    buttons
}

/// One entry: a visit is an outlined card with "Replay"; a line said in flight has a
/// light "Said" tag; a notice is a dark ticket.
fn draw_row(o: &mut Overlay, ui: &UiState, row: &Row, body: &str, r: Rect) {
    let wrap = r.w - 90.0;
    let notice = row.tag == "Notice";
    let ink = if notice { WHITE } else { BLACK };
    let hover = ui.cursor.is_some_and(|c| super::contains(&r, c)) && row.action.is_some();
    match row.tag {
        "Visit" => o.panels.push(Panel {
            fill: palette::CREAM,
            border: if hover { ORANGE } else { palette::INK },
            border_px: if hover { 3.0 } else { 1.5 },
            radius: 10.0,
            shadow: false,
            ..Panel::new(r)
        }),
        "Notice" => o.panels.push(Panel {
            fill: palette::INK,
            border: SKY,
            border_px: 2.0,
            radius: 2.0,
            shadow: false,
            ..Panel::new(r)
        }),
        _ => {}
    }
    // The word, in a small box.
    let tag = Rect::new(r.x + 8.0, r.y + 8.0, 62.0, 20.0);
    o.panels.push(Panel {
        fill: if notice { SKY } else { palette::PARCHMENT },
        border: palette::INK,
        border_px: 1.5,
        radius: if row.tag == "Said" { 10.0 } else { 3.0 },
        shadow: false,
        ..Panel::new(tag)
    });
    o.texts.push(
        Text::new(
            row.tag.to_uppercase(),
            G2::new(tag.center().x, tag.y + 2.0),
            12.0,
        )
        .weight(700)
        .color(BLACK)
        .align(Align::Center),
    );
    o.texts.push(
        Text::new(row.head.clone(), G2::new(r.x + 80.0, r.y + 8.0), 13.0)
            .weight(700)
            .color(ink),
    );
    o.texts.push(
        Text::new(body, G2::new(r.x + 80.0, r.y + 8.0 + PIN_HEAD + 2.0), BODY)
            .weight(500)
            .color(ink)
            .wrap(wrap),
    );
    if row.action.is_some() {
        // "Replay" in an outline, like a small button.
        let b = Rect::new(r.x + r.w - 76.0, r.y + 6.0, 68.0, 22.0);
        o.panels.push(Panel {
            fill: if hover { ORANGE } else { palette::PARCHMENT },
            border: palette::INK,
            border_px: 1.5,
            radius: 6.0,
            shadow: false,
            ..Panel::new(b)
        });
        o.texts.push(
            Text::new("Replay", G2::new(b.center().x, b.y + 2.0), 13.0)
                .weight(700)
                .color(BLACK)
                .align(Align::Center),
        );
    }
}

/// One line of a replayed visit, as its bubble, inside the panel. A click on it shows
/// the next; after the last, the log again.
fn replay_bubble(
    o: &mut Overlay,
    world: &WorldState,
    v: &Visit,
    replay: Replay,
    ui: &UiState,
    rect: Rect,
) -> Vec<Button> {
    let lines = v.bubbles();
    let Some(said) = lines.get(replay.line.min(lines.len().saturating_sub(1))) else {
        return Vec::new();
    };
    let i = replay.line.min(lines.len() - 1);
    let more = lines.len() - 1 - i;
    let mut line: TalkLine = said.line.clone();
    // A replay only tells: its answers were given (or not) then.
    line.choices.clear();
    let b = Bubble::new(Some(said.id), line, &[], more);
    let bounds = Rect::new(rect.x + 20.0, rect.y + 64.0, rect.w - 40.0, rect.h - 110.0);
    let anchor = G2::new(rect.center().x, rect.y + rect.h * 0.7);
    // The replay only tells: no pointer to a station that isn't on screen.
    let mut l = talk::layout(&b, world, anchor, bounds);
    l.pointer = None;
    let next = (more > 0).then_some(Replay {
        from: replay.from,
        line: i + 1,
    });
    let buttons = talk::draw(o, world, &b, &l, ui, (UiAction::Replay(next), None));
    o.texts.push(
        Text::new(
            format!("Line {} of {}", i + 1, lines.len()),
            G2::new(rect.x + 20.0, rect.y + rect.h - 34.0),
            13.0,
        )
        .weight(600)
        .color(BLACK),
    );
    buttons
}

/// The visit a replay names, and how many bubbles it has.
pub fn replay_len(world: &WorldState, history: &History, from: Tick) -> usize {
    visits(world, history)
        .iter()
        .find(|v| v.from == from)
        .map_or(0, |v| v.bubbles().len())
}
