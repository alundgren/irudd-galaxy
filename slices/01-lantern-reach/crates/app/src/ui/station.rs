//! The station frame at the dock (issue #47, 3.1: the concept's direction A, "The
//! station up close", with C's news ribbons in the station key).
//!
//! - **The plate**, top left: the station's name and who holds it (the owner, or
//!   "Contested" / "Unclaimed" where nobody owns it), over a band in the owner's paint.
//! - **Modules**: every part of the station is a module on its art ([`crate::view::modules`])
//!   with a hanging sign. Hover one and it lifts, gets an orange outline, and its sign
//!   shows the keeper and one line of what it offers. Click it and its room opens beside
//!   it ([`super::rooms`]).
//! - **The station key**, along the bottom right of the log icon: one icon per part, for
//!   one click to any of them. A part that changed lately carries a ribbon (New, Changed,
//!   Closed) for `station.ribbon_dockings` dockings, counted, never timed. Hovering an
//!   icon shows a one-line headline, cut to one line in code ([`headline_line`]).
//!
//! Layout is on the UI canvas like the rest of [`super`], laid out against the goal
//! banner, the hold, the log icon and the open room so nothing overlaps.

use galaxy_contract::parts::{PartState, RIBBON_DOCKINGS, Ribbon, RoomKind, StationPart};
use galaxy_contract::prelude::*;
use render::glam::Vec2 as G2;
use render::{Align, Camera, Color, Overlay, Panel, Rect, Text, palette};

use super::talk::{BLUE, ORANGE};
use super::{
    BLACK, Button, GAP, HOLD_PANEL_WIDTH, MARGIN, UiAction, UiState, WHITE, clip, contains,
    overlaps, text_width,
};

/// The concept's vermilion: closed, over budget, a loss. Always with a word or shape.
pub const VERMILION: Color = Color::linear(0.6654, 0.1119, 0.0, 1.0);

/// The key's icons: size and spacing. They sit on the log icon's baseline, low enough
/// that the log's "new" tag stays clear above them.
pub const KEY_W: f32 = 96.0;
pub const KEY_H: f32 = 52.0;
const KEY_GAP: f32 = 8.0;
/// Room above the key for its ribbons.
const RIBBON_H: f32 = 20.0;

/// The headline's text size and the widest it may be: always one line.
pub const HEADLINE_SIZE: f32 = 16.0;
pub const HEADLINE_W: f32 = 520.0;

/// A room never covers more than this share of the screen's width.
pub const ROOM_SHARE: f32 = 0.45;
const ROOM_MAX_W: f32 = 600.0;

/// The station's parts in the key's order: by quarter, the office first, then by room
/// kind and ID.
pub fn parts_here<'a>(world: &'a WorldState, station: &'a StationId) -> Vec<&'a StationPart> {
    let mut parts: Vec<&StationPart> = galaxy_core::parts::of_station(world, station).collect();
    parts.sort_by(|a, b| {
        (&a.quarter, a.room != RoomKind::Dockmaster, a.room, &a.id).cmp(&(
            &b.quarter,
            b.room != RoomKind::Dockmaster,
            b.room,
            &b.id,
        ))
    });
    parts
}

/// How many dockings a ribbon stays, from the pacing data (3 if the save lacks it).
pub fn ribbon_dockings(world: &WorldState) -> u64 {
    world
        .progress
        .tunables
        .get(RIBBON_DOCKINGS)
        .and_then(|t| t.value.as_number())
        .map_or(3, |n| n.max(0.0) as u64)
}

/// A part's ribbon now: its latest change, until `station.ribbon_dockings` more
/// counted dockings have passed.
pub fn ribbon(world: &WorldState, part: &StationPart) -> Option<Ribbon> {
    let news = part.news?;
    let now = world
        .counters
        .get(CounterKind::Dockings, &CounterScope::Player);
    (now.saturating_sub(news.docking) < ribbon_dockings(world)).then_some(news.ribbon)
}

fn first_name(world: &WorldState, npc: &NpcId) -> String {
    world.npcs.get(npc).map_or_else(
        || npc.to_string(),
        |n| {
            n.name
                .split_whitespace()
                .next()
                .unwrap_or(&n.name)
                .to_owned()
        },
    )
}

fn full_name(world: &WorldState, npc: &NpcId) -> String {
    world
        .npcs
        .get(npc)
        .map_or_else(|| npc.to_string(), |n| n.name.clone())
}

/// What the part says about itself now: its keeper's line when it has one (why it's
/// closed, what changed), else its sign.
pub fn news_line(part: &StationPart) -> &str {
    if part.line.is_empty() {
        &part.sign
    } else {
        &part.line
    }
}

/// The key's headline for a part, uncut: the room and one line (its keeper's news, or
/// its sign).
pub fn headline(part: &StationPart) -> String {
    format!("{}  ·  {}", part.title(), news_line(part))
}

/// The headline as shown: one line, cut with an ellipsis to [`HEADLINE_W`].
pub fn headline_line(part: &StationPart) -> String {
    clip(&headline(part), HEADLINE_SIZE, HEADLINE_W)
}

/// The plate's rect, top left.
pub fn plate_rect(world: &WorldState, station: &StationId) -> Rect {
    let name = world
        .stations
        .get(station)
        .map_or(station.as_str(), |s| s.name.as_str());
    let rows = (text_width(name, 22.0) / (HOLD_PANEL_WIDTH - 32.0))
        .ceil()
        .max(1.0);
    Rect::new(MARGIN, MARGIN, HOLD_PANEL_WIDTH, 30.0 + rows * 27.0 + 36.0)
}

/// The station's plate: its name and who holds it, over the owner's colour.
pub fn plate(o: &mut Overlay, world: &WorldState, station: &StationId) {
    let Some(st) = world.stations.get(station) else {
        return;
    };
    let r = plate_rect(world, station);
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.95),
        radius: 12.0,
        border_px: 2.5,
        shadow: false,
        ..Panel::new(r)
    });
    let x = r.x + 16.0;
    o.texts.push(
        Text::new(st.name.clone(), G2::new(x, r.y + 12.0), 22.0)
            .weight(700)
            .color(BLACK)
            .wrap(r.w - 32.0),
    );
    let band_y = r.y + r.h - 40.0;
    let owner = st
        .owner
        .as_ref()
        .map_or(palette::TERRACOTTA, crate::view::recipes::faction_color);
    o.panels.push(Panel {
        fill: owner,
        border_px: 0.0,
        radius: 2.0,
        shadow: false,
        ..Panel::new(Rect::new(x, band_y, r.w - 32.0, 5.0))
    });
    o.texts.push(
        Text::new(super::holder(world, st), G2::new(x, band_y + 11.0), 15.0)
            .weight(600)
            .color(BLACK),
    );
}

/// The key's icons, left to right after the log icon: each part and its rect.
pub fn key_rects(
    world: &WorldState,
    station: &StationId,
    canvas: G2,
) -> Vec<(StationPartId, Rect)> {
    let log = super::log::icon_rect(canvas);
    // Clear of the log's "new" tag at its widest, so the key never shifts.
    let tag = super::log::icon_area(999, canvas);
    let y = log.y + log.h - KEY_H;
    parts_here(world, station)
        .into_iter()
        .enumerate()
        .map(|(i, p)| {
            let x = tag.x + tag.w + GAP + i as f32 * (KEY_W + KEY_GAP);
            (p.id.clone(), Rect::new(x, y, KEY_W, KEY_H))
        })
        .collect()
}

/// The whole key with its ribbons, for layout; `None` for a station with no parts.
pub fn key_area(world: &WorldState, station: &StationId, canvas: G2) -> Option<Rect> {
    let rects = key_rects(world, station, canvas);
    let (first, last) = (rects.first()?.1, rects.last()?.1);
    Some(Rect::new(
        first.x,
        first.y - RIBBON_H * 0.5,
        last.x + last.w + 24.0 - first.x,
        first.h + RIBBON_H * 0.5,
    ))
}

/// Where the open room goes: on the right, under the hold and the goal banner, no wider
/// than [`ROOM_SHARE`] of the screen, and above the key if the key reaches under it.
pub fn room_rect(world: &WorldState, station: &StationId, canvas: G2) -> Rect {
    let w = (canvas.x * ROOM_SHARE).min(ROOM_MAX_W);
    let x = canvas.x - MARGIN - w;
    let hold = super::hold_rect(world, canvas).map_or(MARGIN, |r| r.y + r.h + GAP);
    let goal = super::goal_rect(world, canvas)
        .filter(|g| g.x + g.w > x)
        .map_or(MARGIN, |r| r.y + r.h + GAP);
    let top = hold.max(goal);
    let mut bottom = canvas.y - MARGIN;
    if let Some(k) = key_area(world, station, canvas)
        && k.x + k.w > x
    {
        bottom = k.y - GAP;
    }
    Rect::new(x, top, w, (bottom - top).max(120.0))
}

/// Where talk bubbles may go at the dock: between the plate and goal banner above and
/// the key and log icon below, left of the open room (or of the hold).
pub fn bubble_bounds(
    world: &WorldState,
    station: &StationId,
    room_open: bool,
    log_new: u32,
    canvas: G2,
) -> Rect {
    let goal = super::goal_rect(world, canvas).map_or(0.0, |g| g.y + g.h);
    let plate = plate_rect(world, station);
    let top = goal.max(plate.y + plate.h) + GAP;
    let right = if room_open {
        room_rect(world, station, canvas).x
    } else {
        super::hold_rect(world, canvas).map_or(canvas.x - MARGIN, |r| r.x)
    } - GAP;
    let log = super::log::icon_area(log_new, canvas);
    let bottom = key_area(world, station, canvas).map_or(log.y, |k| k.y.min(log.y)) - GAP;
    Rect::new(
        MARGIN,
        top,
        (right - MARGIN).max(0.0),
        (bottom - top).max(0.0),
    )
}

/// How far left the station's art and modules reach on the canvas, through `camera`.
pub fn art_left(
    world: &WorldState,
    station: &StationId,
    camera: &Camera,
    viewport: G2,
    scale: f32,
) -> Option<f32> {
    let st = world.stations.get(station)?;
    let recipe = galaxy_core::parts::recipe_of(world, st);
    let to_world = crate::view::station_transform(st, &recipe);
    let centre = to_world.w_axis.truncate();
    let reach = recipe.footprint() * to_world.x_axis.truncate().length();
    let mut left = camera
        .world_to_screen(centre - render::glam::Vec3::X * reach, viewport)?
        .x;
    for t in module_targets(world, station, camera, viewport, scale) {
        left = left.min((t.at.x - t.radius) * scale);
    }
    Some(left / scale)
}

/// A dashed outline: a disabled button, a part being built. Drawn as short strokes.
pub fn dashed_outline(o: &mut Overlay, r: Rect, color: Color, px: f32) {
    let (dash, gap): (f32, f32) = (8.0, 5.0);
    let mut stroke = |x: f32, y: f32, w: f32, h: f32| {
        o.panels.push(Panel {
            fill: color,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(x, y, w, h))
        });
    };
    let mut t = 0.0;
    while t < r.w {
        let len = dash.min(r.w - t);
        stroke(r.x + t, r.y, len, px);
        stroke(r.x + t, r.y + r.h - px, len, px);
        t += dash + gap;
    }
    let mut t = 0.0;
    while t < r.h {
        let len = dash.min(r.h - t);
        stroke(r.x, r.y + t, px, len);
        stroke(r.x + r.w - px, r.y + t, px, len);
        t += dash + gap;
    }
}

/// A ribbon tag: its word on a blue (news) or vermilion (closed) tag at `at` (its
/// top-right corner).
fn ribbon_tag(o: &mut Overlay, ribbon: Ribbon, right: f32, top: f32) {
    let word = ribbon.word();
    let w = text_width(word, 12.0) + 16.0;
    let tag = Rect::new(right - w, top, w, RIBBON_H);
    o.panels.push(Panel {
        fill: if ribbon == Ribbon::Closed {
            VERMILION
        } else {
            BLUE
        },
        border: palette::INK,
        border_px: 2.0,
        radius: 9.0,
        shadow: false,
        ..Panel::new(tag)
    });
    o.texts.push(
        Text::new(word, G2::new(tag.center().x, tag.y + 3.0), 12.0)
            .weight(700)
            .color(WHITE)
            .align(Align::Center),
    );
}

/// The station key: an icon per part (its room and keeper), its ribbon, the open room
/// marked, the hovered one in orange with its one-line headline above the row. Returns
/// the icons' buttons.
pub fn key(
    o: &mut Overlay,
    world: &WorldState,
    station: &StationId,
    ui: &UiState,
    hovered: Option<&StationPartId>,
    canvas: G2,
) -> Vec<Button> {
    let mut buttons = Vec::new();
    for (id, r) in key_rects(world, station, canvas) {
        let Some(part) = world.station_parts.get(&id) else {
            continue;
        };
        let open = ui.room.as_ref() == Some(&id);
        let hover = hovered == Some(&id);
        let shut = part.state != PartState::Open;
        o.panels.push(Panel {
            fill: if open {
                palette::MUSTARD
            } else {
                palette::PARCHMENT
            },
            border: if hover { ORANGE } else { palette::INK },
            border_px: if shut && !hover {
                0.0
            } else if hover || open {
                4.0
            } else {
                2.5
            },
            radius: 10.0,
            ..Panel::new(r)
        });
        if shut && !hover {
            dashed_outline(o, r, palette::INK, 2.5);
        }
        o.texts.push(
            Text::new(part.title(), G2::new(r.center().x, r.y + 8.0), 15.0)
                .weight(700)
                .color(BLACK)
                .align(Align::Center),
        );
        let under = match part.state {
            PartState::Open => first_name(world, &part.keeper),
            PartState::Closed => "Closed".to_owned(),
            PartState::Building => "Building".to_owned(),
        };
        o.texts.push(
            Text::new(under, G2::new(r.center().x, r.y + 28.0), 13.0)
                .weight(500)
                .color(BLACK)
                .align(Align::Center),
        );
        if let Some(rb) = ribbon(world, part) {
            ribbon_tag(o, rb, r.x + r.w + 10.0, r.y - RIBBON_H * 0.5);
        }
        buttons.push(Button {
            rect: r,
            action: UiAction::OpenRoom(id.clone()),
        });
        if hover {
            let text = headline_line(part);
            let w = text_width(&text, HEADLINE_SIZE) + 32.0;
            let x = (r.center().x - w * 0.5).clamp(MARGIN, (canvas.x - MARGIN - w).max(MARGIN));
            let pill = Rect::new(x, r.y - RIBBON_H - GAP - 38.0, w, 38.0);
            o.panels.push(Panel {
                fill: palette::INK.with_alpha(0.94),
                border: ORANGE,
                border_px: 3.0,
                radius: 12.0,
                shadow: false,
                ..Panel::new(pill)
            });
            o.texts.push(
                Text::new(text, G2::new(pill.center().x, pill.y + 9.0), HEADLINE_SIZE)
                    .weight(600)
                    .color(WHITE)
                    .align(Align::Center),
            );
        }
    }
    buttons
}

/// A module on screen: its part, its centre on the canvas and how far it reaches there.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub part: StationPartId,
    pub at: G2,
    pub radius: f32,
}

/// Where `station`'s modules are on the canvas, through `camera` (window pixels over
/// `scale`), at least big enough to hit.
pub fn module_targets(
    world: &WorldState,
    station: &StationId,
    camera: &Camera,
    viewport: G2,
    scale: f32,
) -> Vec<Target> {
    let wpp = camera.world_per_pixel(viewport).max(1e-3);
    parts_here(world, station)
        .into_iter()
        .filter_map(|p| {
            let (at, r) = crate::view::modules::module_point(world, p)?;
            let s =
                camera.world_to_screen(at.extend(crate::view::modules::LIFT * 0.5), viewport)?;
            Some(Target {
                part: p.id.clone(),
                at: s / scale,
                radius: (r / wpp / scale).max(26.0),
            })
        })
        .collect()
}

fn target_rect(t: &Target) -> Rect {
    Rect::new(
        t.at.x - t.radius,
        t.at.y - t.radius,
        t.radius * 2.0,
        t.radius * 2.0,
    )
}

/// The module under `p`, if any (the nearest when two reach it).
pub fn module_at(targets: &[Target], p: G2) -> Option<&StationPartId> {
    targets
        .iter()
        .filter(|t| contains(&target_rect(t), p))
        .min_by(|a, b| a.at.distance(p).total_cmp(&b.at.distance(p)))
        .map(|t| &t.part)
}

/// Each module's click target on the canvas, under every panel's buttons.
pub fn module_buttons(targets: &[Target], canvas: G2) -> Vec<Button> {
    targets
        .iter()
        .filter_map(|t| {
            let r = target_rect(t);
            let (x0, y0) = (r.x.max(0.0), r.y.max(0.0));
            let (x1, y1) = ((r.x + r.w).min(canvas.x), (r.y + r.h).min(canvas.y));
            (x1 > x0 && y1 > y0).then(|| Button {
                rect: Rect::new(x0, y0, x1 - x0, y1 - y0),
                action: UiAction::OpenRoom(t.part.clone()),
            })
        })
        .collect()
}

/// The hanging signs: a small tag under each module with its room (and "Closed" or a
/// dashed edge while shut or building); the hovered module's sign opens up above it with
/// its keeper and one line of what it offers, in an orange outline. A sign that would
/// sit on a panel in `avoid` isn't drawn.
pub fn signs(
    o: &mut Overlay,
    world: &WorldState,
    targets: &[Target],
    hovered: Option<&StationPartId>,
    avoid: &[Rect],
    canvas: G2,
) {
    let clear = |r: &Rect| {
        r.x >= 0.0
            && r.y >= 0.0
            && r.x + r.w <= canvas.x
            && r.y + r.h <= canvas.y
            && !avoid.iter().any(|a| overlaps(a, r))
    };
    for t in targets {
        let Some(part) = world.station_parts.get(&t.part) else {
            continue;
        };
        if hovered == Some(&t.part) {
            let head = format!("{}  ·  {}", part.title(), full_name(world, &part.keeper));
            let line = clip(news_line(part), 15.0, 380.0);
            let w = text_width(&head, 16.0).max(text_width(&line, 15.0)) + 28.0;
            let h = 56.0;
            let x = (t.at.x - w * 0.5).clamp(MARGIN, (canvas.x - MARGIN - w).max(MARGIN));
            let y = t.at.y - t.radius - h - 6.0;
            let r = Rect::new(x, y.max(MARGIN), w, h);
            if !clear(&r) {
                continue;
            }
            o.panels.push(Panel {
                fill: palette::PARCHMENT,
                border: ORANGE,
                border_px: 3.5,
                radius: 10.0,
                ..Panel::new(r)
            });
            o.texts.push(
                Text::new(head, G2::new(r.x + 14.0, r.y + 8.0), 16.0)
                    .weight(700)
                    .color(BLACK),
            );
            o.texts.push(
                Text::new(line, G2::new(r.x + 14.0, r.y + 30.0), 15.0)
                    .weight(500)
                    .color(BLACK),
            );
            continue;
        }
        let word = match part.state {
            PartState::Open => part.title().to_owned(),
            PartState::Closed => format!("{}  ·  Closed", part.title()),
            PartState::Building => format!("{}  ·  Building", part.title()),
        };
        let w = text_width(&word, 14.0) + 20.0;
        let r = Rect::new(t.at.x - w * 0.5, t.at.y + t.radius * 0.9, w, 24.0);
        if !clear(&r) {
            continue;
        }
        let (border, px) = match part.state {
            PartState::Open => (palette::INK, 2.0),
            PartState::Closed => (VERMILION, 3.0),
            PartState::Building => (palette::INK, 0.0),
        };
        o.panels.push(Panel {
            fill: palette::PARCHMENT.with_alpha(0.95),
            border,
            border_px: px,
            radius: 6.0,
            shadow: false,
            ..Panel::new(r)
        });
        if part.state == PartState::Building {
            dashed_outline(o, r, palette::INK, 2.0);
        }
        o.texts.push(
            Text::new(word, G2::new(r.center().x, r.y + 4.0), 14.0)
                .weight(600)
                .color(BLACK)
                .align(Align::Center),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn world() -> WorldState {
        let dir =
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/data");
        galaxy_core::new_game(&dir, 1).unwrap().world
    }

    const WHARF: StationId = StationId::from_static("station.lantern_wharf");

    #[test]
    fn the_headline_is_always_one_line() {
        let mut w = world();
        let id: StationPartId = "part.wharf.wharf".parse().unwrap();
        let part = w.station_parts.get_mut(&id).unwrap();
        part.line = "Cradle's down for re-tarring, and while it's down I'm re-cutting \
                     every bolt, sanding the rails and having a long think about the crane, \
                     so I'm fitting at Kettle Dock till the pitch sets."
            .into();
        let part = w.station_parts.get(&id).unwrap();
        let full = headline(part);
        let shown = headline_line(part);
        assert!(
            text_width(&full, HEADLINE_SIZE) > HEADLINE_W,
            "the test line is long"
        );
        assert!(text_width(&shown, HEADLINE_SIZE) <= HEADLINE_W, "{shown}");
        assert!(shown.ends_with('…'), "{shown}");
        assert!(shown.starts_with("Wharf  ·  Cradle's down"), "{shown}");
        assert!(!shown.contains('\n'));
        // A short one is left whole.
        let mut w2 = world();
        w2.station_parts.get_mut(&id).unwrap().line.clear();
        let short = headline_line(w2.station_parts.get(&id).unwrap());
        assert_eq!(short, "Wharf  ·  Orrin fits parts in the cradle");
    }

    #[test]
    fn ribbons_clear_after_a_few_dockings_counted_never_timed() {
        use galaxy_contract::parts::PartNews;
        let mut w = world();
        let id: StationPartId = "part.wharf.market".parse().unwrap();
        let now = w.counters.get(CounterKind::Dockings, &CounterScope::Player);
        w.station_parts.get_mut(&id).unwrap().news = Some(PartNews {
            ribbon: Ribbon::New,
            docking: now,
        });
        assert_eq!(ribbon_dockings(&w), 3, "from pacing.toml");
        let ribbon_at = |w: &mut WorldState, dockings: u64| {
            w.counters
                .bump(CounterKind::Dockings, &CounterScope::Player, dockings);
            ribbon(w, w.station_parts.get(&id).unwrap())
        };
        assert_eq!(ribbon_at(&mut w, 0), Some(Ribbon::New));
        // Ticks alone change nothing.
        w.meta.tick = Tick(w.meta.tick.0 + 1_000_000);
        assert_eq!(ribbon_at(&mut w, 2), Some(Ribbon::New));
        assert_eq!(ribbon_at(&mut w, 1), None, "gone after three more dockings");
    }

    #[test]
    fn the_key_has_an_icon_per_part_and_the_wharf_three() {
        let w = world();
        let rects = key_rects(&w, &WHARF, super::super::DESIGN);
        let ids: Vec<&str> = rects.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(
            ids,
            ["part.wharf.office", "part.wharf.market", "part.wharf.wharf"],
            "the office first"
        );
        let log = super::super::log::icon_area(120, super::super::DESIGN);
        for (_, r) in &rects {
            assert!(!overlaps(r, &log), "{r:?} on the log icon");
        }
    }
}
