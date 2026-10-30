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
pub mod fps;

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
    pub debug_open: bool,
    pub map_open: bool,
    /// The selected dock line.
    pub selected: usize,
    /// The mouse, in logical pixels.
    pub cursor: Option<G2>,
    /// The prestige confirm card, open for this hull.
    pub confirm: Option<HullId>,
    /// The first services row the dock screen shows when they don't all fit.
    pub services_first: usize,
    /// Mark the goal's station at the screen edge, in gold, wherever it is: an
    /// accessibility aid, off by default (`play --goal-marker`). Discovery comes first.
    pub goal_marker: bool,
}

/// Compact developer diagnostics; no sim state changes and no model calls.
pub fn debug_overlay(o: &mut Overlay, session: &crate::session::Session) {
    let w = session.world();
    let beat = w
        .progress
        .beats
        .iter()
        .filter(|(_, b)| b.status == BeatStatus::Active)
        .map(|(id, b)| {
            format!(
                "{} step {}",
                id.as_str().split('_').next().unwrap_or(id.as_str()),
                b.step + 1
            )
        })
        .collect::<Vec<_>>()
        .join(" / ");
    let count = |k| w.counters.get(k, &CounterScope::Player);
    let mut lines = vec![
        format!("F3  tick {}  {}", w.meta.tick.0, beat),
        format!(
            "Dock {}  Mine {}  Sales {}  Orders {}",
            count(CounterKind::Dockings),
            count(CounterKind::UnitsMined),
            count(CounterKind::Sales),
            count(CounterKind::OrdersDone)
        ),
        session.game().mind_debug(),
    ];
    let health: Vec<_> = w.packs.values().flat_map(|p| p.health.values()).collect();
    lines.push(format!(
        "Scripts {}  aborts {}  quarantined {}",
        health.len(),
        health.iter().map(|h| h.aborts as u64).sum::<u64>(),
        health.iter().filter(|h| h.quarantined).count()
    ));
    for fact in session.sim().history().facts().iter().rev().take(4) {
        lines.push(format!("{}  {}", fact.id, fact.kind));
    }
    let rect = Rect::new(12.0, 95.0, 500.0, 24.0 + lines.len() as f32 * 19.0);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.94),
        shadow: false,
        ..Panel::new(rect)
    });
    for (i, line) in lines.into_iter().enumerate() {
        o.texts.push(
            Text::new(
                line,
                G2::new(rect.x + 10.0, rect.y + 10.0 + i as f32 * 19.0),
                14.0,
            )
            .color(palette::PARCHMENT),
        );
    }
}

/// What a button does.
#[derive(Clone, Debug, PartialEq)]
pub enum UiAction {
    Trade {
        line: usize,
        amount: Amount,
    },
    Select(usize),
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
    /// Answer someone at the dock with one of the choices their line offered.
    Reply {
        npc: NpcId,
        choice: AnyId,
    },
    /// Show the dock screen's services from this row (the Up and Down buttons).
    ServicesFrom(usize),
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

/// The goal line's largest and smallest font size: a small, calm line, not a banner.
const GOAL_SIZE: f32 = 17.0;
const GOAL_MIN_SIZE: f32 = 14.0;

/// The goal line, centred at the top: one small, calm line on a soft panel. Nothing
/// when there's no goal.
pub fn goal(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let Some(line) = hooks::goal_line(world) else {
        return;
    };
    let (rect, size, pad) = goal_layout(&line, viewport);
    let w = rect.w;
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.82),
        border_px: 1.5,
        radius: 10.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            line,
            G2::new(viewport.x * 0.5, rect.y + 9.0 + (GOAL_SIZE - size) * 0.5),
            size,
        )
        .weight(500)
        .color(palette::INK.with_alpha(0.85))
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

/// The goal line's panel, its font size and side padding: centred between the hold on
/// the right and the same margin on the left; a long line gets a smaller font, then
/// wraps.
fn goal_layout(line: &str, viewport: G2) -> (Rect, f32, f32) {
    let room = (viewport.x - 2.0 * (HOLD_PANEL_WIDTH + 36.0)).max(240.0);
    let pad = 36.0;
    let natural = text_width(line, GOAL_SIZE);
    let size = (GOAL_SIZE * (room - pad) / natural).clamp(GOAL_MIN_SIZE, GOAL_SIZE);
    let rows = (text_width(line, size) / (room - pad)).ceil().max(1.0);
    let w = (text_width(line, size) + pad).min(room);
    let h = 38.0 + (rows - 1.0) * size * 1.25;
    (Rect::new((viewport.x - w) * 0.5, 18.0, w, h), size, pad)
}

/// Where the goal line's panel goes, if there is a goal.
pub fn goal_rect(world: &WorldState, viewport: G2) -> Option<Rect> {
    hooks::goal_line(world).map(|line| goal_layout(&line, viewport).0)
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
    fps::draw(o, world, viewport);
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

/// How fast `ship` cuts `rock` now, in units a second: the hull's rate, the fitted
/// boosts (the mining lens) and the rock's hardness, as the sim mines it.
pub fn mining_rate(world: &WorldState, ship: &Ship, rock: &Rock) -> f32 {
    let hull = world.hulls.get(&ship.hull).map_or(0.0, |h| h.mining_rate);
    let boost = galaxy_contract::modifiers::for_ship(world, &ship.id).mining_rate as f32;
    let hardness = world
        .rock_types
        .get(&rock.rock_type)
        .map_or(1.0, |t| t.hardness);
    hull.max(0.0) * boost.max(0.0) * hardness.max(0.0)
}

/// What Space would do right now, if anything; while mining, how fast the ore comes
/// aboard (so a fitted lens shows).
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
    if let Some(run) = &ship.activity.mining {
        let rock = world.rocks.get(&run.rock)?;
        let ware = world
            .rock_types
            .get(&rock.rock_type)
            .and_then(|t| world.wares.get(&t.ware))
            .map_or_else(|| "ore".to_owned(), |w| w.name.to_lowercase());
        let rate = format!("{:.2}", mining_rate(world, ship, rock));
        let rate = rate.trim_end_matches('0').trim_end_matches('.');
        return Some(format!("Mining {ware}  ·  {rate} a second"));
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

/// Small markers at the screen edge for stations out of view in the region you're in.
/// With `goal_marker` (off by default) the goal's station is marked too, in gold,
/// wherever it is.
pub fn edge_markers(
    o: &mut Overlay,
    world: &WorldState,
    camera: &Camera,
    viewport: G2,
    goal_marker: bool,
) {
    let Some(ship) = world.player.ship.as_ref().and_then(|s| world.ships.get(s)) else {
        return;
    };
    let goal = if goal_marker {
        hooks::goal_place(world)
    } else {
        None
    };
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

/// The height of one answer button in the talk panel.
const CHOICE_H: f32 = 34.0;

/// The talk panel, bottom left: what was said since the flagship last docked (dock
/// talk and the chronicle), each naming its speaker and place. While docked after
/// asking "What changed?", the dockmaster's answer instead. Below the lines, a button
/// for each answer someone here is waiting for (`choices`, keys 1 to 9). Nothing when
/// nobody has spoken. Returns the answer buttons.
pub fn talk(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    answer: Option<&hooks::Answer>,
    choices: &[hooks::OpenChoice],
    ui: &UiState,
    viewport: G2,
) -> Vec<Button> {
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
    if lines.is_empty() && title.is_none() && choices.is_empty() {
        return Vec::new();
    }
    // Measure first, then draw bottom up from the screen's edge.
    let title_h = if title.is_some() { 30.0 } else { 0.0 };
    let heights: Vec<f32> = lines
        .iter()
        .map(|l| 24.0 + (text_width(&l.text, 16.0) / inner).ceil().max(1.0) * 20.0)
        .collect();
    let empty_h = if lines.is_empty() && title.is_some() {
        26.0
    } else {
        0.0
    };
    let choices_h = if choices.is_empty() {
        0.0
    } else {
        24.0 + choices.len() as f32 * (CHOICE_H + 6.0)
    };
    let h = 20.0 + title_h + empty_h + heights.iter().sum::<f32>() + choices_h;
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
    if lines.is_empty() && empty_h > 0.0 {
        o.texts.push(
            Text::new("Nothing new since you were last here.", G2::new(x, y), 16.0)
                .weight(500)
                .color(palette::INK.with_alpha(0.7)),
        );
        y += empty_h;
    }
    for line in &lines {
        y += talk_row(o, world, line, x, y, inner);
    }
    let mut buttons = Vec::new();
    if !choices.is_empty() {
        // Who is waiting: one name, or "Your answer" when several people asked.
        let first = &choices[0].npc;
        let head = if choices.iter().all(|c| &c.npc == first) {
            let name = world
                .npcs
                .get(first)
                .map_or_else(|| first.to_string(), |n| n.name.clone());
            format!("Answer {name}")
        } else {
            "Your answer".to_owned()
        };
        o.texts.push(
            Text::new(head, G2::new(x, y + 2.0), 14.0)
                .weight(700)
                .color(palette::TERRACOTTA),
        );
        y += 24.0;
        for (i, c) in choices.iter().enumerate() {
            let rect = Rect::new(x, y, inner, CHOICE_H);
            buttons.push(button(
                o,
                ui,
                rect,
                &format!("{}   {}", i + 1, c.text),
                true,
                UiAction::Reply {
                    npc: c.npc.clone(),
                    choice: c.choice.clone(),
                },
            ));
            y += CHOICE_H + 6.0;
        }
    }
    buttons
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
/// each, and what the latest purchase changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Notices<'a> {
    pub trade: Option<&'a str>,
    pub service: Option<&'a str>,
    /// The purchase that just went through, in plain words.
    pub fitted: Option<&'a str>,
}

/// An offer: its name and price, what it does, and what stands in the way, if anything.
const OFFER_ROW: f32 = 68.0;
const SERVICE_HEAD: f32 = 26.0;
const ASK_ROW: f32 = 42.0;
/// The services pager: "Services 2–4 of 6" with Up and Down.
const PAGER_ROW: f32 = 40.0;

/// One row of the services block.
#[derive(Clone, Copy, Debug, PartialEq)]
enum ServiceRow {
    /// A service's heading (its keeper's name).
    Head(usize),
    /// An offer: service, offer.
    Offer(usize, usize),
    /// The dockmaster's "Ask what changed".
    Ask(usize),
}

impl ServiceRow {
    fn service(self) -> usize {
        match self {
            ServiceRow::Head(s) | ServiceRow::Offer(s, _) | ServiceRow::Ask(s) => s,
        }
    }

    fn height(self) -> f32 {
        match self {
            ServiceRow::Head(_) => SERVICE_HEAD,
            ServiceRow::Offer(..) => OFFER_ROW,
            ServiceRow::Ask(_) => ASK_ROW,
        }
    }
}

/// The services block, row by row: a heading per service with something to offer (or
/// a dockmaster to ask), a row per offer, the ask button.
fn service_rows(services: &[hooks::ServiceEntry]) -> Vec<ServiceRow> {
    let mut rows = Vec::new();
    for (i, s) in services.iter().enumerate() {
        if s.offers.is_empty() && !s.can_ask {
            continue;
        }
        rows.push(ServiceRow::Head(i));
        rows.extend((0..s.offers.len()).map(|o| ServiceRow::Offer(i, o)));
        if s.can_ask {
            rows.push(ServiceRow::Ask(i));
        }
    }
    rows
}

/// The height of `rows[first..first + n]`, with its service's heading repeated on top
/// when the window starts inside a service.
fn window_height(rows: &[ServiceRow], first: usize, n: usize) -> f32 {
    let head = match rows.get(first) {
        Some(ServiceRow::Head(_)) | None => 0.0,
        Some(_) => SERVICE_HEAD,
    };
    head + rows[first..first + n]
        .iter()
        .map(|r| r.height())
        .sum::<f32>()
}

/// How many rows from `first` fit in `room` (at least one).
fn rows_fitting(rows: &[ServiceRow], first: usize, room: f32) -> usize {
    let mut n = 1;
    while first + n < rows.len() && window_height(rows, first, n + 1) <= room {
        n += 1;
    }
    n.min(rows.len() - first)
}

/// The services rows shown: `(first, count)`, starting near `wanted` and filling
/// `room` (the last page is a full one).
fn service_window(rows: &[ServiceRow], wanted: usize, room: f32) -> (usize, usize) {
    if rows.is_empty() {
        return (0, 0);
    }
    let last_page = (0..rows.len())
        .find(|&f| window_height(rows, f, rows.len() - f) <= room)
        .unwrap_or(rows.len() - 1);
    let first = wanted.min(last_page);
    (first, rows_fitting(rows, first, room))
}

/// The dock screen: the station's name, its market lines (price per unit, stock, your
/// hold, buy or sell), any refusal in one calm line, the station's services (what the
/// hullwright and tender keeper sell, with the price and what stands in the way in
/// plain words; the dockmaster's "What changed?"), and how to leave. It sits under the hold,
/// or beside it when a full hold leaves too little room below, and always fits the
/// canvas height with its foot on screen: market rows that don't fit page with the
/// selection, and services that don't fit page with Up and Down.
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
    // A refusal, or else what the purchase that just went through changes (it can
    // take two rows).
    let service_line = notices.service.or(notices.fitted);
    let service_notice_h = match service_line {
        Some(n) if text_width(n, 16.0) > w - 48.0 => 50.0,
        Some(_) => 30.0,
        None => 0.0,
    };
    let (scroll_h, head_h) = (24.0, 26.0);
    let rows = service_rows(&services);
    let services_full = window_height(&rows, 0, rows.len());
    let market_full = if lines.is_empty() {
        30.0
    } else {
        head_h + lines.len() as f32 * row
    };
    // Title, the notices and the foot (how to leave): always shown.
    let fixed = 74.0 + notice_h + service_notice_h + 70.0;
    let natural = fixed + market_full + services_full;
    // Under the goal line and the hold, which share the top right; if it doesn't fit
    // there (a full hold makes the hold panel tall), beside the hold, under the goal.
    let goal_bottom = goal_rect(world, viewport).map_or(0.0, |r| r.y + r.h + 12.0);
    let hold = hold_rect(world, viewport);
    let under = hold
        .map_or(90.0, |r| r.y + r.h + 12.0)
        .max(goal_bottom)
        .max(90.0);
    let beside = hold.map(|r| r.x - 12.0 - w).filter(|x| *x >= 18.0);
    let (top, x0) = match beside {
        Some(x) if natural > viewport.y - under - 18.0 => (goal_bottom.max(90.0), x),
        _ => (under, viewport.x - w - 18.0),
    };
    // The body between the title and the foot shares what's left: everything if it
    // fits; otherwise the services get what they need (at least one row and the pager)
    // and page, and the market scrolls with the selection in the rest.
    let body = viewport.y - top - 18.0 - fixed;
    let market_min = if lines.len() > 1 {
        head_h + row + scroll_h
    } else {
        market_full
    };
    let (services_first, services_shown, services_h) = if services_full <= body - market_min {
        (0, rows.len(), services_full)
    } else {
        let room = (body - market_min).max(SERVICE_HEAD + OFFER_ROW + PAGER_ROW) - PAGER_ROW;
        let (first, n) = service_window(&rows, ui.services_first, room);
        (first, n, window_height(&rows, first, n) + PAGER_ROW)
    };
    let market_room = body - services_h;
    let fits = if market_full <= market_room {
        lines.len()
    } else {
        ((market_room - head_h - scroll_h) / row).floor().max(1.0) as usize
    };
    let (first, shown) = visible_rows(lines.len(), ui.selected, fits);
    let scrolled = shown < lines.len();
    let table_h = if lines.is_empty() {
        30.0
    } else {
        head_h + shown as f32 * row + if scrolled { scroll_h } else { 0.0 }
    };
    let h = fixed + table_h + services_h;
    // Never below the canvas: on a screen too small for even the minimum, the panel
    // rides up over the goal line rather than lose its foot.
    let y0 = top
        .max((viewport.y - h - 18.0).min(140.0))
        .min((viewport.y - h - 18.0).max(18.0));
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
            // The rest of the market pages with the mouse, like the services.
            let last = first + shown;
            o.texts.push(
                Text::new(
                    format!("{}–{} of {}", first + 1, last, lines.len()),
                    G2::new(col_ware, y + 2.0),
                    13.0,
                )
                .weight(500)
                .color(palette::INK.with_alpha(0.6)),
            );
            buttons.push(button(
                o,
                ui,
                Rect::new(x0 + w - 24.0 - 128.0, y - 2.0, 60.0, 24.0),
                "Up",
                first > 0,
                UiAction::Select(first.saturating_sub(1)),
            ));
            buttons.push(button(
                o,
                ui,
                Rect::new(x0 + w - 24.0 - 60.0, y - 2.0, 60.0, 24.0),
                "Down",
                last < lines.len(),
                UiAction::Select((last + shown - 1).min(lines.len() - 1)),
            ));
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
    let head = |o: &mut Overlay, s: &hooks::ServiceEntry, y: f32| {
        let head = match &s.keeper_name {
            Some(k) => format!("{}  ·  {k}", s.name.to_uppercase()),
            None => s.name.to_uppercase(),
        };
        o.texts.push(
            Text::new(head, G2::new(col_ware, y + 6.0), 13.0)
                .weight(600)
                .color(palette::TERRACOTTA),
        );
    };
    let window = &rows[services_first..services_first + services_shown];
    if let Some(r) = window.first().filter(|r| !matches!(r, ServiceRow::Head(_))) {
        // The window starts inside a service: say whose it is.
        head(o, &services[r.service()], y);
        y += SERVICE_HEAD;
    }
    for r in window {
        match *r {
            ServiceRow::Head(s) => head(o, &services[s], y),
            ServiceRow::Offer(s, i) => {
                let offer = &services[s].offers[i];
                o.texts.push(
                    Text::new(
                        format!("{}  ·  {}", offer.name, offer.price),
                        G2::new(col_ware, y + 2.0),
                        17.0,
                    )
                    .weight(600),
                );
                // What it does always shows, even before you can buy it.
                o.texts.push(
                    Text::new(offer.effect.as_str(), G2::new(col_ware, y + 23.0), 14.0)
                        .weight(500)
                        .color(palette::INK.with_alpha(0.75))
                        .wrap(w - 48.0 - 140.0),
                );
                if let Some(blocked) = offer.blocked.as_deref() {
                    o.texts.push(
                        Text::new(blocked, G2::new(col_ware, y + 43.0), 13.0)
                            .weight(600)
                            .color(palette::TERRACOTTA)
                            .wrap(w - 48.0 - 140.0),
                    );
                }
                let (label, action) = match &offer.request {
                    ServiceRequest::Prestige { to } => ("Trade up", UiAction::Prestige(to.clone())),
                    ServiceRequest::Build { .. } => {
                        ("Build", UiAction::Service(offer.request.clone()))
                    }
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
            }
            ServiceRow::Ask(_) => {
                buttons.push(button(
                    o,
                    ui,
                    Rect::new(col_ware, y + 2.0, 200.0, 32.0),
                    "Ask what changed",
                    true,
                    UiAction::AskWhatChanged,
                ));
            }
        }
        y += r.height();
    }
    if services_shown < rows.len() {
        // The services page: what's shown of how many, and Up / Down.
        let last = services_first + services_shown;
        o.texts.push(
            Text::new(
                format!("Services {}–{} of {}", services_first + 1, last, rows.len()),
                G2::new(col_ware, y + 12.0),
                13.0,
            )
            .weight(500)
            .color(palette::INK.with_alpha(0.6)),
        );
        buttons.push(button(
            o,
            ui,
            Rect::new(x0 + w - 24.0 - 168.0, y + 4.0, 80.0, 30.0),
            "Up",
            services_first > 0,
            UiAction::ServicesFrom(services_first.saturating_sub(services_shown)),
        ));
        buttons.push(button(
            o,
            ui,
            Rect::new(x0 + w - 24.0 - 80.0, y + 4.0, 80.0, 30.0),
            "Down",
            last < rows.len(),
            UiAction::ServicesFrom(last),
        ));
        y += PAGER_ROW;
    }
    if let Some(n) = service_line {
        let color = if notices.service.is_some() {
            palette::TERRACOTTA
        } else {
            Color::hex(0x2f7a4a)
        };
        o.texts.push(
            Text::new(n, G2::new(col_ware, y + 4.0), 16.0)
                .weight(600)
                .color(color)
                .wrap(w - 48.0),
        );
        y += service_notice_h;
    }
    let foot_y = y0 + h - 52.0;
    let _ = y;
    o.texts.push(
        Text::new(DOCK_FOOT, G2::new(col_ware, foot_y + 12.0), 15.0)
            .weight(500)
            .color(palette::INK.with_alpha(0.65)),
    );
    buttons
}

/// The dock screen's foot: how to leave. There's no undock button or key: flying off
/// undocks.
pub const DOCK_FOOT: &str = "Fly off with W A S D to undock";

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
                action: UiAction::CloseMap,
            },
        ];
        assert_eq!(hit(&b, G2::new(15.0, 15.0)), Some(&UiAction::CloseMap));
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
    fn the_mining_hint_shows_the_rate_and_the_lens() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let mut state = galaxy_core::new_game(&dir, 1).unwrap();
        let w = &mut state.world;
        let ship_id = w.player.ship.clone().unwrap();
        let rock = w
            .rocks
            .values()
            .find(|r| r.rock_type.as_str() == "rock_type.ice")
            .unwrap()
            .clone();
        let ship = w.ships.get_mut(&ship_id).unwrap();
        ship.docked_at = None;
        ship.position = rock.position + Vec2::new(10.0, 0.0);
        ship.activity.mining = Some(galaxy_contract::ship::MiningRun {
            rock: rock.id.clone(),
            progress: 0.0,
            mined: 0,
        });
        assert_eq!(
            context_hint(w).as_deref(),
            Some("Mining ice  ·  1 a second")
        );
        w.player
            .fittings
            .insert(UpgradeId::from_static("upgrade.mining_lens"));
        assert_eq!(
            context_hint(w).as_deref(),
            Some("Mining ice  ·  1.5 a second"),
            "the lens shows"
        );
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
