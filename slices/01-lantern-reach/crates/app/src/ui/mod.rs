//! The screen's panels, minimal and warm: the active goals with who gave them, the hold
//! and credits, a hint for what Space does when something is in reach (until it has
//! taught it, [`hints`]), the station frame at the dock ([`station`]: the plate, module
//! signs and the station key) with a room open beside its module ([`rooms`]), talk
//! bubbles over whoever speaks at the dock ([`talk`]), the ship log behind one small
//! icon ([`log`]), the radio in flight ([`radio`]) and the PAUSED card. No timers, no progress bars; text is important
//! or absent.
//!
//! Layout is logical pixels from the viewport, so it needs no renderer and can be
//! tested headless. Each function adds to an [`Overlay`] and returns the buttons it
//! drew, for the window's mouse hit test.
//!
//! The panels are laid out on a canvas at least [`DESIGN`] big ([`canvas`]) and scaled
//! down to the window ([`ui_scale`], [`scale_overlay`]), so a small window shows all of
//! them smaller rather than clipping any. Inside the canvas they're laid out against
//! each other so none covers another: the goal banner between the edge and the hold
//! (beside the title on the map), the log icon in the bottom-left corner with the
//! station key beside it, the log panel in the left column between the banner and the
//! icon, an open room on the right (at most 45% of the width) with the station and its
//! bubbles left of it, the hint pill clear of the log icon, and in flight the radio in
//! the bottom-right corner. A market with more lines than fit pages with the selection.
//! The window won't shrink below [`MIN_WINDOW`], where the text is still readable.

pub mod dock;
pub mod fps;
pub mod hints;
pub mod log;
pub mod radio;
pub mod rooms;
pub mod station;
pub mod talk;

use galaxy_contract::prelude::*;
use galaxy_core::History;
use render::glam::Vec2 as G2;
use render::{Align, Camera, Color, Overlay, Panel, PausedCard, Rect, Text, palette};

use crate::hooks;
use crate::trades::credits;
use crate::view::g;
pub use dock::{Amount, DockLine, dock_lines, trade_request};
pub use hints::{HintBook, HintKind, HintRules};

/// The hold panel's width on the canvas.
pub const HOLD_PANEL_WIDTH: f32 = 200.0;

/// The log panel's width: the left column.
pub const BUBBLE_ROOM: f32 = 460.0;

/// The space between a panel and the canvas edge, and between two panels.
pub(crate) const MARGIN: f32 = 18.0;
pub(crate) const GAP: f32 = 12.0;

/// Pure white and pure black: the only text colours, on dark and on light panels.
const WHITE: Color = Color::WHITE;
const BLACK: Color = Color::BLACK;

/// Whether two rects share any area.
pub fn overlaps(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.w && b.x < a.x + a.w && a.y < b.y + b.h && b.y < a.y + a.h
}

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
    /// The ship log is open.
    pub log_open: bool,
    /// The first log entry on the page.
    pub log_first: usize,
    /// A visit replayed in the log.
    pub replay: Option<log::Replay>,
    /// The station part whose room is open beside its module, while docked.
    pub room: Option<StationPartId>,
    /// A module or key icon shown hovered whatever the mouse does (`galaxy shot`'s
    /// `:hover:ID`); the window hovers by the mouse.
    pub hover: Option<StationPartId>,
    /// The hullwright's room: the tray tab, a part in hand, the Make confirm and the
    /// fitting animation ([`rooms::fit`]).
    pub wharf: rooms::fit::WharfUi,
    /// What the radio shows this frame ([`crate::radio::Radio::update`]).
    pub radio: crate::radio::RadioUi,
}

impl UiState {
    /// L or the log icon: opens the ship log on its newest page (nothing in it is new
    /// any more), or closes it.
    pub fn toggle_log(&mut self, talk: &mut talk::TalkBook) {
        self.log_open = !self.log_open;
        self.replay = None;
        self.log_first = 0;
        if self.log_open {
            talk.read_log();
        }
    }

    /// Enter: the next line of a replay in the open log, else the next talk bubble at
    /// the dock.
    pub fn enter(&mut self, session: &mut crate::session::Session) {
        if !self.log_open {
            session.next_line();
            return;
        }
        if let Some(r) = self.replay {
            let len = log::replay_len(session.world(), session.sim().history(), r.from);
            self.replay = (r.line + 1 < len).then_some(log::Replay {
                line: r.line + 1,
                ..r
            });
        }
    }
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
    /// The talk bubble was clicked: the next line (Enter too).
    NextLine,
    /// Skip the rest of what's waiting here (lines waiting for an answer stay).
    SkipTalk,
    /// Open or close the ship log (the icon, or L).
    ToggleLog,
    /// Show the log's entries from this one (Newer and Older).
    LogFrom(usize),
    /// Replay a visit's line in the log, or go back to the list.
    Replay(Option<log::Replay>),
    /// A panel's background: the click stops there.
    Absorb,
    /// Open a station part's room (its module, or its icon on the station key).
    OpenRoom(StationPartId),
    /// Back to the whole station (Back, Esc or a right-click).
    CloseRoom,
    /// Something at the hullwright's cradle: a tab, a part, a socket, the locker.
    Wharf(rooms::fit::WharfAction),
    /// The radio's Prev, Off (or On) and Next.
    Radio(crate::radio::Tune),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub rect: Rect,
    pub action: UiAction,
}

pub fn contains(r: &Rect, p: G2) -> bool {
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

/// `text` cut to about `width` of text at `size` (at a word, with an ellipsis), so a row
/// never grows past its room.
pub fn clip(text: &str, size: f32, width: f32) -> String {
    if text_width(text, size) <= width {
        return text.to_owned();
    }
    let keep = ((width / (size * 0.52)).floor() as usize).saturating_sub(1);
    let cut: String = text.chars().take(keep).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > keep / 2 => &cut[..i],
        _ => cut.as_str(),
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.', ' ']))
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
    // Disabled is a bare thin outline, never greyed-out text.
    let fill = if !enabled {
        palette::PARCHMENT.with_alpha(0.0)
    } else if hover {
        palette::LANTERN
    } else {
        palette::MUSTARD
    };
    o.panels.push(Panel {
        radius: 8.0,
        fill,
        border: BLACK,
        border_px: if enabled { 2.0 } else { 1.0 },
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            label,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(if enabled { 600 } else { 500 })
        .color(BLACK)
        .align(Align::Center),
    );
    Button { rect, action }
}

/// The goal banner's text sizes, side padding and the space between goals.
const GOAL_SIZE: f32 = 16.0;
const GOAL_HEAD: f32 = 13.0;
const GOAL_PAD: f32 = 16.0;
const GOAL_GAP: f32 = 8.0;
/// One wrapped row of a goal.
const GOAL_ROW: f32 = 20.0;
/// The goal's source line above it.
const GOAL_HEAD_H: f32 = 17.0;

/// The goal banner laid out: the panel, the top of each goal's source line and text,
/// and the width the text wraps at.
struct GoalLayout {
    rect: Rect,
    rows: Vec<(f32, f32)>,
    wrap: f32,
}

/// Lays out `goals` at the top of the canvas, centred between `left` and `right`, each
/// goal under its source and wrapped to fit.
fn goal_layout(goals: &[hooks::Goal], left: f32, right: f32) -> Option<GoalLayout> {
    if goals.is_empty() {
        return None;
    }
    let room = (right - left).max(240.0);
    let natural = goals
        .iter()
        .map(|g| text_width(&g.goal, GOAL_SIZE).max(text_width(&g.source, GOAL_HEAD)))
        .fold(0.0, f32::max);
    let wrap = natural.min(room - 2.0 * GOAL_PAD);
    let top = MARGIN;
    let mut y = top + 10.0;
    let mut rows = Vec::new();
    for g in goals {
        rows.push((y, y + GOAL_HEAD_H));
        let lines = (text_width(&g.goal, GOAL_SIZE) / wrap).ceil().max(1.0);
        y += GOAL_HEAD_H + lines * GOAL_ROW + GOAL_GAP;
    }
    let h = y - GOAL_GAP + 10.0 - top;
    let w = wrap + 2.0 * GOAL_PAD;
    Some(GoalLayout {
        rect: Rect::new(left + (room - w) * 0.5, top, w, h),
        rows,
        wrap,
    })
}

/// Where the goal banner may go on the world screen: centred, between the hold on the
/// right and as much room on the left.
fn goal_span(viewport: G2) -> (f32, f32) {
    let side = HOLD_PANEL_WIDTH + 2.0 * MARGIN;
    (side, viewport.x - side)
}

/// The map's title, top left.
const MAP_TITLE: &str = "The Lantern Reach";
const MAP_TITLE_SIZE: f32 = 30.0;

/// Where the map's title goes.
pub fn map_title_rect() -> Rect {
    Rect::new(
        28.0,
        22.0,
        text_width(MAP_TITLE, MAP_TITLE_SIZE),
        MAP_TITLE_SIZE * 1.25,
    )
}

/// Where the goal banner may go on the map: right of the title, left of the hold.
fn map_goal_span(viewport: G2) -> (f32, f32) {
    let title = map_title_rect();
    (
        title.x + title.w + 2.0 * GAP,
        viewport.x - HOLD_PANEL_WIDTH - 2.0 * MARGIN,
    )
}

fn draw_goals(o: &mut Overlay, goals: &[hooks::Goal], layout: &GoalLayout) {
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.92),
        border_px: 1.5,
        radius: 10.0,
        shadow: false,
        ..Panel::new(layout.rect)
    });
    let x = layout.rect.x + GOAL_PAD;
    for (g, (head_y, text_y)) in goals.iter().zip(&layout.rows) {
        o.texts.push(
            Text::new(g.source.clone(), G2::new(x, *head_y), GOAL_HEAD)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(g.goal.clone(), G2::new(x, *text_y), GOAL_SIZE)
                .weight(500)
                .color(BLACK)
                .wrap(layout.wrap),
        );
    }
}

/// The goal banner, centred at the top: every active goal, newest first, each under
/// who gave it and where. Nothing when there's no goal.
pub fn goal(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let goals = hooks::goals(world);
    let (left, right) = goal_span(viewport);
    if let Some(layout) = goal_layout(&goals, left, right) {
        draw_goals(o, &goals, &layout);
    }
}

/// The goal banner on the map, beside the title so both stay readable.
pub fn map_goal(o: &mut Overlay, world: &WorldState, viewport: G2) {
    let goals = hooks::goals(world);
    let (left, right) = map_goal_span(viewport);
    if let Some(layout) = goal_layout(&goals, left, right) {
        draw_goals(o, &goals, &layout);
    }
}

/// The hold panel's lines: one per ware aboard, then each consignment carried for
/// someone, marked with whose it is ("Refined plate 20 · Ilse's").
fn hold_wares(world: &WorldState, ship: &Ship) -> Vec<String> {
    let ware = |w: &WareId| {
        world
            .wares
            .get(w)
            .map_or_else(|| w.to_string(), |w| w.name.clone())
    };
    let own = ship
        .hold
        .cargo
        .iter()
        .filter(|(_, n)| **n > 0)
        .map(|(w, n)| format!("{} {n}", ware(w)));
    let consigned = ship.hold.consigned.values().map(|c| {
        let owner = world.npcs.get(&c.owner).map_or(c.owner.as_str(), |n| {
            n.name.split_whitespace().next().unwrap_or(&n.name)
        });
        format!("{} {} · {owner}'s", ware(&c.ware), c.quantity)
    });
    own.chain(consigned).collect()
}

/// Where the goal banner goes on the world screen, if there is a goal.
pub fn goal_rect(world: &WorldState, viewport: G2) -> Option<Rect> {
    let (left, right) = goal_span(viewport);
    goal_layout(&hooks::goals(world), left, right).map(|l| l.rect)
}

/// Where the goal banner goes on the map, if there is a goal.
pub fn map_goal_rect(world: &WorldState, viewport: G2) -> Option<Rect> {
    let (left, right) = map_goal_span(viewport);
    goal_layout(&hooks::goals(world), left, right).map(|l| l.rect)
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
            .color(WHITE),
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

/// What Space does right now, if anything, read from the rule the sim uses
/// (`galaxy_ship::space::press_at`); while mining, how fast the ore comes aboard (so a
/// fitted lens shows). Past the region's edge, what the current there does.
pub fn context_hint(world: &WorldState) -> Option<String> {
    use galaxy_ship::space::{self, Press};
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    if ship.docked_at.is_some() || ship.activity.transit.is_some() {
        return None;
    }
    match space::press_at(world, ship, ship.position) {
        Some(Press::Dock(station)) => {
            return Some(format!("Space  ·  dock at {}", station.name));
        }
        Some(Press::Ride(way)) => {
            let to = world
                .regions
                .get(&way.to)
                .map_or_else(|| way.to.to_string(), |r| r.name.clone());
            return Some(format!("Space  ·  ride the lantern current to {to}"));
        }
        Some(Press::Mine(_)) | None => {}
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
    if let Some(rock) = space::rock_in_reach_of(world, &ship.id, &ship.region, ship.position) {
        let cap = galaxy_contract::modifiers::hold_capacity(world, ship);
        if ship.hold.total() >= cap {
            return Some("Your hold is full".into());
        }
        let ware = world
            .rock_types
            .get(&rock.rock_type)
            .and_then(|t| world.wares.get(&t.ware))
            .map_or_else(|| "it".to_owned(), |w| w.name.to_lowercase());
        return Some(format!("Hold Space  ·  mine {ware}"));
    }
    if space::past_edge(world, &ship.region, ship.position).is_some() {
        let name = world
            .regions
            .get(&ship.region)
            .map_or_else(|| ship.region.to_string(), |r| r.name.clone());
        return Some(format!("Edge of {name}  ·  a current carries you back in"));
    }
    // Flying by hand, held Space brakes.
    let by_hand = ship.orders.current >= ship.orders.orders.len();
    (by_hand && ship.velocity.length() > 1.0).then(|| "Space  ·  brake".to_owned())
}

/// Which hint [`context_hint`] shows now, so it can retire once it has taught
/// ([`hints`]). Checks the same things in the same order.
pub fn hint_kind(world: &WorldState) -> Option<HintKind> {
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    if ship.docked_at.is_some() || ship.activity.transit.is_some() {
        return None;
    }
    if galaxy_ship::space::ring_for(world, ship, ship.position).is_some() {
        return Some(HintKind::Dock);
    }
    if galaxy_ship::space::mouth_for(world, ship, ship.position).is_some() {
        return Some(HintKind::Ride);
    }
    if ship.activity.mining.is_some() {
        return Some(HintKind::MiningRate);
    }
    galaxy_ship::space::rock_in_reach_of(world, &ship.id, &ship.region, ship.position)?;
    if ship.hold.total() >= galaxy_contract::modifiers::hold_capacity(world, ship) {
        return Some(HintKind::HoldFull);
    }
    Some(HintKind::Mine)
}

/// While the flagship mines, how fast the ore comes aboard, as the readout shows it
/// ("1.5").
pub fn mining_rate_text(world: &WorldState) -> Option<String> {
    let ship = world
        .player
        .ship
        .as_ref()
        .and_then(|s| world.ships.get(s))?;
    let rock = world.rocks.get(&ship.activity.mining.as_ref()?.rock)?;
    let rate = format!("{:.2}", mining_rate(world, ship, rock));
    Some(rate.trim_end_matches('0').trim_end_matches('.').to_owned())
}

/// The hint pill's text size and height.
const HINT_SIZE: f32 = 19.0;
const HINT_H: f32 = 44.0;

/// Where the hint pill goes: low and centred, unless that would cover `avoid` (the
/// log icon in the bottom-left corner); then centred in the room right of it, or above
/// it if that room is too narrow.
pub fn hint_rect(text: &str, viewport: G2, avoid: Option<Rect>) -> Rect {
    let w = text_width(text, HINT_SIZE) + 40.0;
    let centred = Rect::new((viewport.x - w) * 0.5, viewport.y - 84.0, w, HINT_H);
    let Some(a) = avoid.filter(|a| overlaps(&centred, &grow(a, GAP))) else {
        return centred;
    };
    let (left, right) = (a.x + a.w + GAP, viewport.x - MARGIN);
    if right - left >= w {
        let x = ((left + right - w) * 0.5).max(centred.x);
        return Rect { x, ..centred };
    }
    Rect {
        y: (a.y - GAP - HINT_H).max(MARGIN),
        ..centred
    }
}

fn grow(r: &Rect, by: f32) -> Rect {
    Rect::new(r.x - by, r.y - by, r.w + 2.0 * by, r.h + 2.0 * by)
}

/// The Space hint, low in the middle of the screen and clear of the log icon (`avoid`).
/// A hint that has taught enough ([`HintBook`]) stays away.
pub fn hint(
    o: &mut Overlay,
    world: &WorldState,
    book: &HintBook,
    avoid: Option<Rect>,
    viewport: G2,
) {
    let Some(text) = context_hint(world) else {
        return;
    };
    if hint_kind(world).is_some_and(|k| book.retired(k, world)) {
        return;
    }
    let rect = hint_rect(&text, viewport, avoid);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.85),
        border: palette::LANTERN,
        radius: 12.0,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(text, G2::new(rect.center().x, rect.y + 10.0), HINT_SIZE)
            .weight(600)
            .color(WHITE)
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

/// The market rows a market room shows: `(first, count)` out of `lines`, keeping the
/// selected one in view when they don't all fit in `room`.
pub(crate) fn visible_rows(lines: usize, selected: usize, fits: usize) -> (usize, usize) {
    if lines <= fits {
        return (0, lines);
    }
    let count = fits.max(1);
    let selected = selected.min(lines - 1);
    let first = (selected + 1).saturating_sub(count).min(lines - count);
    (first, count)
}

/// What a room says besides its rows: the latest refusals, one calm line
/// each, and what the latest purchase changed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Notices<'a> {
    pub trade: Option<&'a str>,
    pub service: Option<&'a str>,
    /// The purchase that just went through, in plain words.
    pub fitted: Option<&'a str>,
}

/// "Fitted" (an upgrade) or "Yours" (a tender) in an outline, for something the player
/// already has: a state, not a button.
pub(crate) fn owned_mark(o: &mut Overlay, rect: Rect, request: &ServiceRequest) {
    let word = match request {
        ServiceRequest::Build { .. } => "Yours",
        _ => "Fitted",
    };
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.0),
        border: BLACK,
        border_px: 2.0,
        radius: 8.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            word,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(700)
        .color(BLACK)
        .align(Align::Center),
    );
}

/// Who holds a station, for its plate: the owning faction, or, for a station nobody
/// owns (Cinder Hook at the start), its region's state as the map shows it.
pub fn holder(world: &WorldState, st: &Station) -> String {
    match &st.owner {
        Some(f) => world
            .factions
            .get(f)
            .map_or_else(|| f.to_string(), |f| f.name.clone()),
        None => match galaxy_economy::map::control_of(world, &st.region) {
            galaxy_economy::map::Control::Contested { .. } => "Contested".into(),
            _ => "Unclaimed".into(),
        },
    }
}

/// A room's foot: how to leave. There's no undock button or key: flying off undocks.
pub const DOCK_FOOT: &str = "W or S · fly off and undock";

/// The map's own furniture: a title (the goal banner sits beside it, [`map_goal`]) and
/// how to close it.
pub fn map_furniture(o: &mut Overlay, ui: &UiState, viewport: G2) -> Vec<Button> {
    let title = map_title_rect();
    o.texts.push(
        Text::new(MAP_TITLE, G2::new(title.x, title.y), MAP_TITLE_SIZE)
            .weight(700)
            .color(WHITE),
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
        toggles: Vec::new(),
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
    fn the_space_hint_says_what_space_does_now() {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        let mut state = galaxy_core::new_game(&dir, 1).unwrap();
        let w = &mut state.world;
        let id = w.player.ship.clone().unwrap();
        let kettle = w.stations.get("station.kettle_dock").unwrap().clone();
        let region = w.regions.get("region.lantern_shallows").unwrap().clone();
        let put = |w: &mut WorldState, at: Vec2, v: Vec2| {
            let ship = w.ships.get_mut(&id).unwrap();
            ship.docked_at = None;
            ship.position = at;
            ship.velocity = v;
        };
        put(w, kettle.position, Vec2::ZERO);
        assert_eq!(
            context_hint(w).as_deref(),
            Some("Space  ·  dock at Kettle Dock")
        );
        // Open space: nothing while still, the brake while moving.
        let open = Vec2::new(-300.0, -1200.0);
        put(w, open, Vec2::ZERO);
        assert_eq!(context_hint(w), None);
        put(w, open, Vec2::new(80.0, 0.0));
        assert_eq!(context_hint(w).as_deref(), Some("Space  ·  brake"));
        // Past the edge, the current.
        let out = region.center + Vec2::new(-(region.radius + 100.0), 0.0);
        put(w, out, Vec2::new(-80.0, 0.0));
        assert_eq!(
            context_hint(w).as_deref(),
            Some("Edge of Lantern Shallows  ·  a current carries you back in")
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
