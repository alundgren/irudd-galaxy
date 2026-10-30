//! The screen's panels, minimal and warm: one goal line, the hold and credits, a hint
//! for what Space does when something is in reach, the dock screen, the talk panel and
//! the PAUSED card. No timers, no progress bars; text is important or absent.
//!
//! Layout is logical pixels from the viewport, so it needs no renderer and can be
//! tested headless. Each function adds to an [`Overlay`] and returns the buttons it
//! drew, for the window's mouse hit test.
//!
//! The panels are laid out on a canvas at least [`DESIGN`] big ([`canvas`]) and scaled
//! down to the window ([`ui_scale`], [`scale_overlay`]), so a small window shows all of
//! them smaller rather than clipping any. Inside the canvas they keep clear of each
//! other, and a market with more lines than fit scrolls with the selection. The window
//! won't shrink below [`MIN_WINDOW`], where the text is still readable.

pub mod dock;

use galaxy_contract::prelude::*;
use galaxy_contract::talk::{Channel, TalkLine};
use galaxy_core::History;
use render::glam::Vec2 as G2;
use render::{Align, Camera, Color, Overlay, Panel, PausedCard, Rect, Text, palette};

use crate::hooks;
use crate::trades::credits;
use crate::view::g;
pub use dock::{Amount, DockLine, dock_lines, trade_request};

/// The dock screen's width on the canvas.
pub const DOCK_PANEL_WIDTH: f32 = 600.0;

/// The hold panel's width on the canvas.
pub const HOLD_PANEL_WIDTH: f32 = 200.0;

/// The smallest canvas the panels are laid out on, in logical pixels. Smaller windows
/// scale it down; bigger ones get a bigger canvas at 1:1.
pub const DESIGN: G2 = G2::new(1280.0, 720.0);

/// The smallest window `galaxy play` allows, in logical pixels (the panels at 3/4 size).
pub const MIN_WINDOW: G2 = G2::new(960.0, 540.0);

/// How much the panels are scaled for `viewport`: 1 at [`DESIGN`] and above, less on
/// smaller windows so the whole canvas fits.
pub fn ui_scale(viewport: G2) -> f32 {
    (viewport.x / DESIGN.x)
        .min(viewport.y / DESIGN.y)
        .clamp(0.05, 1.0)
}

/// The canvas the panels are laid out on for `viewport`: at least [`DESIGN`] in the
/// tighter direction, the window's shape.
pub fn canvas(viewport: G2) -> G2 {
    viewport / ui_scale(viewport)
}

/// Scales panels laid out on the canvas onto the window.
pub fn scale_overlay(o: &mut Overlay, scale: f32) {
    for p in &mut o.panels {
        p.rect = scale_rect(p.rect, scale);
        p.radius *= scale;
        p.border_px *= scale;
    }
    for t in &mut o.texts {
        t.pos *= scale;
        t.size *= scale;
        t.max_width = t.max_width.map(|w| w * scale);
    }
}

/// Scales buttons laid out on the canvas onto the window, for the hit test.
pub fn scale_buttons(buttons: &mut [Button], scale: f32) {
    for b in buttons {
        b.rect = scale_rect(b.rect, scale);
    }
}

fn scale_rect(r: Rect, s: f32) -> Rect {
    Rect::new(r.x * s, r.y * s, r.w * s, r.h * s)
}

/// What the screen is showing besides the world.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiState {
    pub map_open: bool,
    /// The selected dock line.
    pub selected: usize,
    /// The mouse, in logical pixels.
    pub cursor: Option<G2>,
    /// The prestige confirm card, open for this hull.
    pub confirm: Option<HullId>,
}

/// What a button does.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    Trade {
        line: usize,
        amount: Amount,
    },
    Select(usize),
    Undock,
    CloseMap,
    /// Buy a station service (an upgrade or a tender) now.
    Service(ServiceRequest),
    /// Open the prestige confirm card for this hull.
    Prestige(HullId),
    /// Confirm the prestige on the card.
    ConfirmPrestige(HullId),
    /// Close the card without trading up.
    CancelPrestige,
    /// Ask the dockmaster what changed.
    AskWhatChanged,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub action: UiAction,
}

fn contains(r: &Rect, p: G2) -> bool {
    p.x >= r.x && p.x <= r.x + r.w && p.y >= r.y && p.y <= r.y + r.h
}

/// The button under `p`, topmost first.
pub fn hit(buttons: &[Button], p: G2) -> Option<&UiAction> {
    buttons
        .iter()
        .rev()
        .find(|b| contains(&b.rect, p))
        .map(|b| &b.action)
}

/// A rough text width for layout, so panels fit their text without a renderer.
pub fn text_width(text: &str, size: f32) -> f32 {
    text.chars().count() as f32 * size * 0.52
}

fn button(
    o: &mut Overlay,
    ui: &UiState,
    rect: Rect,
    label: &str,
    enabled: bool,
    action: UiAction,
) -> Button {
    let hover = ui.cursor.is_some_and(|c| contains(&rect, c));
    let fill = if !enabled {
        palette::PARCHMENT.lerp(palette::INK, 0.12)
    } else if hover {
        palette::LANTERN
    } else {
        palette::MUSTARD
    };
    o.panels.push(Panel {
        radius: 8.0,
        fill,
        border_px: 2.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            label,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(600)
        .color(if enabled {
            palette::INK
        } else {
            palette::INK.with_alpha(0.45)
        })
        .align(Align::Center),
    );
    Button { rect, action }
}

/// The goal line, centred at the top. Nothing when there's no goal.
pub fn goal(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(line) = hooks::goal_line(world) else {
        return;
    };
    // Centred between the hold on the right and the same margin on the left; a long
    // line gets a smaller font, then wraps.
    let room = (viewport.x - 2.0 * (HOLD_PANEL_WIDTH + 36.0)).max(240.0);
    let pad = 56.0;
    let natural = text_width(&line, 24.0);
    let size = (24.0 * (room - pad) / natural).clamp(17.0, 24.0);
    let rows = (text_width(&line, size) / (room - pad)).ceil().max(1.0);
    let w = (text_width(&line, size) + pad).min(room);
    let h = 54.0 + (rows - 1.0) * size * 1.25;
    let rect = Rect::new((viewport.x - w) * 0.5, 18.0, w, h);
    o.panels.push(Panel::new(rect));
    o.texts.push(
        Text::new(
            line,
            G2::new(viewport.x * 0.5, 31.0 + (24.0 - size) * 0.5),
            size,
        )
        .weight(600)
        .align(Align::Center)
        .wrap(w - pad + 8.0),
    );
}

/// The hold panel's lines: one per ware aboard.
fn hold_wares(world: &WorldState, ship: &Ship) -> Vec<String> {
    ship.hold
        .cargo
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(w, n)| {
            let name = world.wares.get(w).map_or(w.as_str(), |w| w.name.as_str());
            format!("{name} {n}")
        })
        .collect()
}

/// Where the hold panel goes, top right; other panels keep clear of it.
pub fn hold_rect(world: &WorldState, viewport: G2) -> Option<Rect> {
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    let h = 78.0 + hold_wares(world, ship).len() as f32 * 20.0;
    Some(Rect::new(
        viewport.x - HOLD_PANEL_WIDTH - 18.0,
        18.0,
        HOLD_PANEL_WIDTH,
        h,
    ))
}

/// Credits and the hold, top right.
pub fn hold(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return;
    };
    let Some(rect) = hold_rect(world, viewport) else {
        return;
    };
    let cap = galaxy_contract::modifiers::hold_capacity(world, ship);
    let wares = hold_wares(world, ship);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.88),
        border: palette::MUSTARD,
        ..Panel::new(rect)
    });
    let x = rect.x + 18.0;
    o.texts.push(
        Text::new(
            credits(world.player.credits),
            G2::new(x, rect.y + 10.0),
            24.0,
        )
        .weight(700)
        .color(palette::LANTERN),
    );
    o.texts.push(
        Text::new(
            format!("Hold {} / {cap}", ship.hold.total()),
            G2::new(x, rect.y + 44.0),
            18.0,
        )
        .weight(600)
        .color(palette::CREAM),
    );
    for (i, line) in wares.into_iter().enumerate() {
        o.texts.push(
            Text::new(
                line,
                G2::new(x + 6.0, rect.y + 68.0 + i as f32 * 20.0),
                16.0,
            )
            .weight(500)
            .color(palette::CREAM.with_alpha(0.85)),
        );
    }
}

/// What Space would do right now, if anything.
pub fn context_hint(world: &WorldState) -> Option<String> {
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    if ship.docked_at.is_some() || ship.activity.transit.is_some() {
        return None;
    }
    if let Some(station) = galaxy_ship::space::ring_for(world, ship, ship.position) {
        return Some(format!("Space  ·  dock at {}", station.name));
    }
    if let Some(way) = galaxy_ship::space::mouth_for(world, ship, ship.position) {
        let to = world
            .regions
            .get(&way.to)
            .map_or_else(|| way.to.to_string(), |r| r.name.clone());
        return Some(format!("Space  ·  ride the lantern current to {to}"));
    }
    if ship.activity.mining.is_some() {
        return None;
    }
    let rock = galaxy_ship::space::rock_in_reach(world, &ship.region, ship.position)?;
    let cap = galaxy_contract::modifiers::hold_capacity(world, ship);
    if ship.hold.total() >= cap {
        return Some("Your hold is full".into());
    }
    let ware = world
        .rock_types
        .get(&rock.rock_type)
        .and_then(|t| world.wares.get(&t.ware))
        .map_or_else(|| "it".to_owned(), |w| w.name.to_lowercase());
    Some(format!("Hold Space  ·  mine {ware}"))
}

/// The Space hint, low in the middle of the screen.
pub fn hint(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(text) = context_hint(world) else {
        return;
    };
    let size = 19.0;
    let w = text_width(&text, size) + 40.0;
    let rect = Rect::new((viewport.x - w) * 0.5, viewport.y - 84.0, w, 44.0);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.85),
        border: palette::LANTERN,
        radius: 12.0,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(text, G2::new(viewport.x * 0.5, rect.y + 10.0), size)
            .weight(600)
            .color(palette::CREAM)
            .align(Align::Center),
    );
}

/// Each station's name under its ring, when it's on screen and you aren't docked there.
pub fn station_labels(o: &mut Overlay, world: &WorldState, camera: &Camera, viewport: G2) {
    let docked = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))
        .and_then(|s| s.docked_at.clone());
    for s in world.stations.values() {
        if docked.as_ref() == Some(&s.id) {
            continue;
        }
        let below = g(s.position) - G2::new(0.0, s.dock_radius * 1.15);
        let Some(p) = camera.world_to_screen(below.extend(0.0), viewport) else {
            continue;
        };
        if p.x < 0.0 || p.y < 0.0 || p.x > viewport.x || p.y > viewport.y + 10.0 {
            continue;
        }
        o.texts.push(
            Text::new(s.name.clone(), p, 17.0)
                .weight(600)
                .color(palette::CREAM.with_alpha(0.9))
                .align(Align::Center),
        );
    }
}

/// Small markers at the screen edge for stations out of view in the region you're in,
/// and for the goal's station wherever it is.
pub fn edge_markers(o: &mut Overlay, world: &WorldState, camera: &Camera, viewport: G2) {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return;
    };
    let goal = hooks::goal_place(world);
    let inset = 34.0;
    for s in world.stations.values() {
        let is_goal = goal.as_ref() == Some(&s.id);
        if s.region != ship.region && !is_goal {
            continue;
        }
        let Some(p) = camera.world_to_screen(g(s.position).extend(0.0), viewport) else {
            continue;
        };
        // Any part of the station on screen counts as seen.
        let r = s.dock_radius * 1.6 / camera.world_per_pixel(viewport).max(1e-3);
        let on_screen = p.x > -r && p.y > -r && p.x < viewport.x + r && p.y < viewport.y + r;
        if on_screen {
            continue;
        }
        // From the middle of the screen towards the station, stopping at a frame that
        // keeps clear of the goal line and the hold at the top.
        let mid = viewport * 0.5;
        let d = p - mid;
        // ... and above the Space hint at the bottom.
        let (lo, hi) = (
            G2::new(inset, 130.0),
            G2::new(viewport.x - inset, viewport.y - 104.0),
        );
        let kx = if d.x > 0.0 {
            (hi.x - mid.x) / d.x
        } else {
            (lo.x - mid.x) / d.x.min(-1e-3)
        };
        let ky = if d.y > 0.0 {
            (hi.y - mid.y) / d.y
        } else {
            (lo.y - mid.y) / d.y.min(-1e-3)
        };
        let at = mid + d * kx.min(ky).max(0.0);
        let color = if is_goal {
            palette::MUSTARD
        } else {
            palette::CREAM.with_alpha(0.75)
        };
        o.panels.push(Panel {
            fill: color,
            border_px: 2.0,
            radius: 7.0,
            shadow: false,
            ..Panel::new(Rect::new(at.x - 7.0, at.y - 7.0, 14.0, 14.0))
        });
        let size = if is_goal { 17.0 } else { 15.0 };
        let w = text_width(&s.name, size);
        let (align, tx) = if d.x > 0.0 {
            (Align::Right, at.x - 14.0)
        } else {
            (Align::Left, at.x + 14.0)
        };
        let ty = (at.y - size * 0.6).clamp(6.0, viewport.y - size - 6.0);
        let _ = w;
        o.texts.push(
            Text::new(s.name.clone(), G2::new(tx, ty), size)
                .weight(if is_goal { 700 } else { 500 })
                .color(color)
                .align(align),
        );
    }
}

/// One line in the talk panel: who (and where), then what was said.
fn talk_row(o: &mut Overlay, world: &WorldState, line: &TalkLine, x: f32, y: f32, w: f32) -> f32 {
    let who = hooks::speaker(world, line);
    let place = hooks::place(world, line);
    let head = match (who, place) {
        (Some(w), Some(p)) => format!("{w}  ·  {p}"),
        (Some(w), None) => w,
        (None, Some(p)) => p,
        (None, None) => String::new(),
    };
    let head = if line.channel == Channel::Chronicle {
        format!("Chronicle  ·  {head}")
    } else {
        head
    };
    o.texts.push(
        Text::new(head, G2::new(x, y), 14.0)
            .weight(700)
            .color(palette::TERRACOTTA),
    );
    let rows = (text_width(&line.text, 16.0) / w).ceil().max(1.0);
    o.texts.push(
        Text::new(line.text.clone(), G2::new(x, y + 17.0), 16.0)
            .weight(500)
            .wrap(w),
    );
    24.0 + rows * 20.0
}

/// The talk panel, bottom left: what was said since the flagship last docked (dock
/// talk and the chronicle), each naming its speaker and place. While docked after
/// asking "What changed?", the dockmaster's answer instead. Nothing when nobody has
/// spoken.
pub fn talk(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    answer: Option<&hooks::Answer>,
    viewport: G2,
) {
    let w = 460.0;
    let inner = w - 32.0;
    let (title, lines): (Option<String>, Vec<TalkLine>) = match answer {
        Some(a) => (
            Some(match a.speaker.as_ref().and_then(|n| world.npcs.get(n)) {
                Some(n) => format!("What changed?  ·  {}", n.name),
                None => "What changed?".to_owned(),
            }),
            a.lines.clone(),
        ),
        None => (
            None,
            hooks::talk_lines(world, history, 3)
                .into_iter()
                .filter(|l| hooks::is_talk(l) && l.validate().is_ok())
                .collect(),
        ),
    };
    if lines.is_empty() && title.is_none() {
        return;
    }
    // Measure first, then draw bottom up from the screen's edge.
    let title_h = if title.is_some() { 30.0 } else { 0.0 };
    let heights: Vec<f32> = lines
        .iter()
        .map(|l| 24.0 + (text_width(&l.text, 16.0) / inner).ceil().max(1.0) * 20.0)
        .collect();
    let empty_h = if lines.is_empty() { 26.0 } else { 0.0 };
    let h = 20.0 + title_h + empty_h + heights.iter().sum::<f32>();
    let rect = Rect::new(18.0, viewport.y - h - 18.0, w, h);
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.95),
        ..Panel::new(rect)
    });
    let x = rect.x + 16.0;
    let mut y = rect.y + 12.0;
    if let Some(t) = title {
        o.texts.push(Text::new(t, G2::new(x, y), 17.0).weight(700));
        y += title_h;
    }
    if lines.is_empty() {
        o.texts.push(
            Text::new("Nothing new since you were last here.", G2::new(x, y), 16.0)
                .weight(500)
                .color(palette::INK.with_alpha(0.7)),
        );
    }
    for line in &lines {
        y += talk_row(o, world, line, x, y, inner);
    }
}

/// The prestige confirm card: what stays, what resets, and the fee.
pub fn prestige_card(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    to: &HullId,
    ui: &UiState,
    viewport: G2,
) -> Vec<Button> {
    let Some((stays, resets)) = hooks::prestige_summary(world, history) else {
        return Vec::new();
    };
    let name = world.hulls.get(to).map_or(to.as_str(), |h| h.name.as_str());
    let w = 620.0;
    let row = 22.0;
    let h = 150.0 + (stays.len() + resets.len()) as f32 * row + 60.0;
    let rect = Rect::new((viewport.x - w) * 0.5, (viewport.y - h) * 0.5, w, h);
    o.panels.push(Panel {
        radius: 16.0,
        border_px: 3.5,
        ..Panel::new(rect)
    });
    let x = rect.x + 28.0;
    let mut y = rect.y + 20.0;
    o.texts
        .push(Text::new(format!("Trade up into a {name}?"), G2::new(x, y), 26.0).weight(700));
    y += 44.0;
    for (head, list) in [("STAYS", &stays), ("RESETS", &resets)] {
        o.texts.push(
            Text::new(head, G2::new(x, y), 13.0)
                .weight(600)
                .color(palette::TERRACOTTA),
        );
        y += 20.0;
        for l in list {
            o.texts.push(
                Text::new(l.clone(), G2::new(x + 8.0, y), 15.0)
                    .weight(500)
                    .wrap(w - 64.0),
            );
            y += row;
        }
        y += 8.0;
    }
    let by = rect.y + h - 56.0;
    vec![
        button(
            o,
            ui,
            Rect::new(rect.x + w - 28.0 - 150.0, by, 150.0, 38.0),
            "Trade up",
            true,
            UiAction::ConfirmPrestige(to.clone()),
        ),
        button(
            o,
            ui,
            Rect::new(rect.x + w - 28.0 - 150.0 - 12.0 - 120.0, by, 120.0, 38.0),
            "Not yet",
            true,
            UiAction::CancelPrestige,
        ),
    ]
}

/// The market rows the dock screen shows: `(first, count)` out of `lines`, keeping the
/// selected one in view when they don't all fit in `room`.
fn visible_rows(lines: usize, selected: usize, fits: usize) -> (usize, usize) {
    if lines <= fits {
        return (0, lines);
    }
    let count = fits.max(1);
    let selected = selected.min(lines - 1);
    let first = (selected + 1).saturating_sub(count).min(lines - count);
    (first, count)
}

/// What the dock screen says besides the market: the latest refusals, one calm line
/// each.
#[derive(Clone, Copy, Debug, Default)]
pub struct Notices<'a> {
    pub trade: Option<&'a str>,
    pub service: Option<&'a str>,
}

const OFFER_ROW: f32 = 50.0;
const SERVICE_HEAD: f32 = 26.0;

/// The height of the services block: a heading per service with something to offer
/// (or a dockmaster to ask), a row per offer.
fn services_height(services: &[hooks::ServiceEntry]) -> f32 {
    services
        .iter()
        .filter(|s| !s.offers.is_empty() || s.can_ask)
        .map(|s| {
            SERVICE_HEAD + s.offers.len() as f32 * OFFER_ROW + if s.can_ask { 42.0 } else { 0.0 }
        })
        .sum::<f32>()
}

/// The dock screen: the station's name, its market lines (price per unit, stock, your
/// hold, buy or sell), any refusal in one calm line, the station's services (what the
/// hullwright and tender keeper sell, with the price and what stands in the way in
/// plain words; the dockmaster's "What changed?"), and Undock. It sits under the hold
/// and fits the canvas height: market rows that don't fit scroll with the selection.
pub fn dock_panel(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    station: &StationId,
    ui: &UiState,
    notices: Notices<'_>,
    viewport: G2,
) -> Vec<Button> {
    let mut buttons = Vec::new();
    let Some(st) = world.stations.get(station) else {
        return buttons;
    };
    let notice = notices.trade;
    let lines = dock_lines(world, station);
    let services = hooks::station_services(world, history, station);
    let w = DOCK_PANEL_WIDTH.min(viewport.x - 36.0);
    let row = 44.0;
    let notice_h = if notice.is_some() { 34.0 } else { 0.0 };
    let service_notice_h = if notices.service.is_some() { 30.0 } else { 0.0 };
    let services_h = services_height(&services) + service_notice_h;
    // Title, notice, services and the foot with Undock.
    let fixed = 74.0 + notice_h + services_h + 70.0;
    // Under the goal line and the hold, which share the top right.
    let top = hold_rect(world, viewport)
        .map_or(90.0, |r| r.y + r.h + 12.0)
        .max(90.0);
    let room = viewport.y - top - 18.0;
    let (scroll_h, head_h) = (24.0, 26.0);
    let fits = if fixed + head_h + lines.len() as f32 * row <= room {
        lines.len()
    } else {
        ((room - fixed - head_h - scroll_h) / row).floor().max(1.0) as usize
    };
    let (first, shown) = visible_rows(lines.len(), ui.selected, fits);
    let scrolled = shown < lines.len();
    let table_h = if lines.is_empty() {
        30.0
    } else {
        head_h + shown as f32 * row + if scrolled { scroll_h } else { 0.0 }
    };
    let h = fixed + table_h;
    let x0 = viewport.x - w - 18.0;
    let y0 = top.max((viewport.y - h - 18.0).min(140.0));
    o.panels.push(Panel {
        radius: 16.0,
        border_px: 3.5,
        ..Panel::new(Rect::new(x0, y0, w, h))
    });
    // A coloured band under the title in the owner's colour.
    let owner_color = st
        .owner
        .as_ref()
        .map_or(palette::TERRACOTTA, crate::view::recipes::faction_color);
    o.panels.push(Panel {
        fill: owner_color,
        border_px: 0.0,
        radius: 3.0,
        shadow: false,
        ..Panel::new(Rect::new(x0 + 24.0, y0 + 58.0, w - 48.0, 4.0))
    });
    o.texts
        .push(Text::new(st.name.clone(), G2::new(x0 + 24.0, y0 + 16.0), 30.0).weight(700));
    if let Some(f) = st.owner.as_ref().and_then(|f| world.factions.get(f)) {
        o.texts.push(
            Text::new(f.name.clone(), G2::new(x0 + w - 24.0, y0 + 28.0), 15.0)
                .weight(600)
                .color(palette::TERRACOTTA)
                .align(Align::Right),
        );
    }

    let col_ware = x0 + 24.0;
    let col_price = x0 + 190.0;
    let col_stock = x0 + 330.0;
    let col_hold = x0 + 386.0;
    let col_btn = x0 + w - 24.0 - 64.0 - 8.0 - 78.0;
    let mut y = y0 + 74.0;
    if lines.is_empty() {
        o.texts.push(
            Text::new("No market here.", G2::new(col_ware, y), 17.0)
                .weight(500)
                .color(palette::INK.with_alpha(0.7)),
        );
        y += 30.0;
    } else {
        let head = |o: &mut Overlay, t: &str, x: f32, align: Align| {
            o.texts.push(
                Text::new(t, G2::new(x, y), 13.0)
                    .weight(600)
                    .color(palette::TERRACOTTA)
                    .align(align),
            );
        };
        head(o, "WARE", col_ware, Align::Left);
        head(o, "PRICE EACH", col_price, Align::Left);
        head(o, "STOCK", col_stock + 30.0, Align::Right);
        head(o, "HOLD", col_hold + 30.0, Align::Right);
        y += 24.0;
        for (i, line) in lines.iter().enumerate().skip(first).take(shown) {
            let selected = i == ui.selected.min(lines.len() - 1);
            let row_rect = Rect::new(x0 + 12.0, y - 2.0, w - 24.0, row - 4.0);
            if selected {
                o.panels.push(Panel {
                    fill: palette::MUSTARD.with_alpha(0.28),
                    border: palette::MUSTARD,
                    border_px: 1.5,
                    radius: 9.0,
                    shadow: false,
                    ..Panel::new(row_rect)
                });
            }
            buttons.push(Button {
                rect: row_rect,
                action: UiAction::Select(i),
            });
            let ty = y + 9.0;
            o.texts
                .push(Text::new(line.name.clone(), G2::new(col_ware, ty), 18.0).weight(600));
            let (verb, color) = match line.side {
                TradeSide::Sell => ("pays", Color::hex(0x2f7a4a)),
                TradeSide::Buy => ("asks", palette::INK),
            };
            let price = line.unit.map_or_else(
                || "—".to_owned(),
                |c| format!("{verb} {}", credits(i64::from(c))),
            );
            o.texts.push(
                Text::new(price, G2::new(col_price, ty), 17.0)
                    .weight(600)
                    .color(color),
            );
            o.texts.push(
                Text::new(line.stock.to_string(), G2::new(col_stock + 30.0, ty), 17.0)
                    .weight(500)
                    .align(Align::Right),
            );
            o.texts.push(
                Text::new(line.in_hold.to_string(), G2::new(col_hold + 30.0, ty), 17.0)
                    .weight(if line.in_hold > 0 { 700 } else { 400 })
                    .align(Align::Right),
            );
            let can = match line.side {
                TradeSide::Sell => line.in_hold > 0 && line.unit.is_some(),
                TradeSide::Buy => line.stock > 0 && line.unit.is_some(),
            };
            let (one, most) = match line.side {
                TradeSide::Sell => ("Sell 1", "Sell all"),
                TradeSide::Buy => ("Buy 1", "Buy max"),
            };
            let by = y + 3.0;
            buttons.push(button(
                o,
                ui,
                Rect::new(col_btn, by, 64.0, 32.0),
                one,
                can,
                UiAction::Trade {
                    line: i,
                    amount: Amount::One,
                },
            ));
            buttons.push(button(
                o,
                ui,
                Rect::new(col_btn + 72.0, by, 78.0, 32.0),
                most,
                can,
                UiAction::Trade {
                    line: i,
                    amount: Amount::Most,
                },
            ));
            y += row;
        }
        if scrolled {
            o.texts.push(
                Text::new(
                    format!(
                        "{}–{} of {}  ·  W/S for more",
                        first + 1,
                        first + shown,
                        lines.len()
                    ),
                    G2::new(col_ware, y + 2.0),
                    13.0,
                )
                .weight(500)
                .color(palette::INK.with_alpha(0.6)),
            );
            y += scroll_h;
        }
    }
    if let Some(n) = notice {
        o.texts.push(
            Text::new(n, G2::new(col_ware, y + 6.0), 16.0)
                .weight(600)
                .color(palette::TERRACOTTA)
                .wrap(w - 48.0),
        );
        y += notice_h;
    }
    for s in services
        .iter()
        .filter(|s| !s.offers.is_empty() || s.can_ask)
    {
        let head = match &s.keeper_name {
            Some(k) => format!("{}  ·  {k}", s.name.to_uppercase()),
            None => s.name.to_uppercase(),
        };
        o.texts.push(
            Text::new(head, G2::new(col_ware, y + 6.0), 13.0)
                .weight(600)
                .color(palette::TERRACOTTA),
        );
        y += SERVICE_HEAD;
        for offer in &s.offers {
            o.texts.push(
                Text::new(
                    format!("{}  ·  {}", offer.name, offer.price),
                    G2::new(col_ware, y + 2.0),
                    17.0,
                )
                .weight(600),
            );
            let detail = offer.blocked.as_deref().unwrap_or(&offer.effect);
            o.texts.push(
                Text::new(detail, G2::new(col_ware, y + 24.0), 14.0)
                    .weight(500)
                    .color(if offer.blocked.is_some() {
                        palette::TERRACOTTA
                    } else {
                        palette::INK.with_alpha(0.7)
                    })
                    .wrap(w - 48.0 - 140.0),
            );
            let (label, action) = match &offer.request {
                ServiceRequest::Prestige { to } => ("Trade up", UiAction::Prestige(to.clone())),
                ServiceRequest::Build { .. } => ("Build", UiAction::Service(offer.request.clone())),
                ServiceRequest::Fit { .. } if offer.price == "a gift" => {
                    ("Take it", UiAction::Service(offer.request.clone()))
                }
                ServiceRequest::Fit { .. } => ("Fit", UiAction::Service(offer.request.clone())),
            };
            buttons.push(button(
                o,
                ui,
                Rect::new(x0 + w - 24.0 - 120.0, y + 4.0, 120.0, 34.0),
                label,
                offer.blocked.is_none(),
                action,
            ));
            y += OFFER_ROW;
        }
        if s.can_ask {
            buttons.push(button(
                o,
                ui,
                Rect::new(col_ware, y + 2.0, 200.0, 32.0),
                "Ask what changed",
                true,
                UiAction::AskWhatChanged,
            ));
            y += 42.0;
        }
    }
    if let Some(n) = notices.service {
        o.texts.push(
            Text::new(n, G2::new(col_ware, y + 4.0), 16.0)
                .weight(600)
                .color(palette::TERRACOTTA)
                .wrap(w - 48.0),
        );
        y += service_notice_h;
    }
    let foot_y = y0 + h - 52.0;
    let _ = y;
    o.texts.push(
        Text::new(
            "W/S choose  ·  E trade one  ·  Shift+E trade all",
            G2::new(col_ware, foot_y + 12.0),
            14.0,
        )
        .weight(500)
        .color(palette::INK.with_alpha(0.6)),
    );
    buttons.push(button(
        o,
        ui,
        Rect::new(x0 + w - 24.0 - 150.0, foot_y, 150.0, 36.0),
        "Undock  ·  Space",
        true,
        UiAction::Undock,
    ));
    buttons
}

/// The map's own furniture: a title and how to close it.
pub fn map_furniture(o: &mut Overlay, ui: &UiState, viewport: G2) -> Vec<Button> {
    o.texts.push(
        Text::new("The Lantern Reach", G2::new(28.0, 22.0), 30.0)
            .weight(700)
            .color(palette::CREAM),
    );
    vec![button(
        o,
        ui,
        Rect::new(viewport.x - 168.0, viewport.y - 58.0, 150.0, 38.0),
        "Back  ·  M",
        true,
        UiAction::CloseMap,
    )]
}

/// The big PAUSED card.
pub fn paused_card() -> PausedCard {
    PausedCard {
        title: "PAUSED".into(),
        hint: "Nothing moves until you're back. P or Esc to carry on.".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_finds_the_topmost_button() {
        let b = vec![
            Button {
                rect: Rect::new(0.0, 0.0, 100.0, 100.0),
                action: UiAction::Select(0),
            },
            Button {
                rect: Rect::new(10.0, 10.0, 20.0, 20.0),
                action: UiAction::Undock,
            },
        ];
        assert_eq!(hit(&b, G2::new(15.0, 15.0)), Some(&UiAction::Undock));
        assert_eq!(hit(&b, G2::new(50.0, 50.0)), Some(&UiAction::Select(0)));
        assert_eq!(hit(&b, G2::new(150.0, 50.0)), None);
    }

    #[test]
    fn market_rows_scroll_with_the_selection() {
        assert_eq!(visible_rows(4, 3, 6), (0, 4), "all fit");
        assert_eq!(visible_rows(9, 0, 4), (0, 4));
        assert_eq!(visible_rows(9, 3, 4), (0, 4));
        assert_eq!(visible_rows(9, 5, 4), (2, 4), "the selection stays in view");
        assert_eq!(visible_rows(9, 8, 4), (5, 4));
        assert_eq!(visible_rows(9, 20, 4), (5, 4));
        assert_eq!(visible_rows(9, 2, 0), (2, 1), "at least one row");
    }

    #[test]
    fn small_windows_scale_the_canvas_down() {
        assert_eq!(ui_scale(G2::new(1920.0, 1080.0)), 1.0);
        assert_eq!(canvas(G2::new(1920.0, 1080.0)), G2::new(1920.0, 1080.0));
        assert_eq!(ui_scale(G2::new(640.0, 360.0)), 0.5);
        assert_eq!(canvas(G2::new(640.0, 360.0)), DESIGN);
        // A wide, short window keeps its shape.
        let c = canvas(G2::new(1200.0, 450.0));
        assert!((c.y - 720.0).abs() < 1e-3 && c.x > 1900.0, "{c:?}");
    }
}
