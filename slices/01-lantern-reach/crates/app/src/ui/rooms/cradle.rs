//! What the wharf's room draws beside the cradle, on the station left of the panel
//! (#47, 3.2): a label for each socket the wharf works on, inked to its ring (what's in
//! it, "Free", or "Drop here" for a part in hand), the control rack's modules as small
//! labelled chips, the sockets' click and drop targets, and the tag that follows a
//! dragged part ("Let go to fit hold pods", or the keeper's reason it won't go).
//!
//! The ship, its rings and the crane are drawn in the world ([`crate::view::cradle`]);
//! this is the UI over them, laid out on the canvas like every panel.

use galaxy_contract::prelude::*;
use render::glam::Vec2 as G2;
use render::{Align, Camera, Color, Overlay, Panel, Rect, Text, palette};

use super::Room;
use super::fit::{self, Over, Source, WharfAction};
use crate::ui::station::VERMILION;
use crate::ui::talk::{ORANGE, SKY};
use crate::ui::{BLACK, Button, UiAction, WHITE, contains, text_width};
use crate::view::cradle::{self as vc, ScreenSocket};

const LABEL_W: f32 = 176.0;
const LABEL_H: f32 = 46.0;
const LABEL_GAP: f32 = 8.0;
const CHIP_H: f32 = 22.0;

/// What a pointer over `action` is over, for a part in hand.
pub fn over_of(action: Option<&UiAction>) -> Option<Over> {
    match action? {
        UiAction::Wharf(WharfAction::Socket(s)) => Some(Over::Socket(s.clone())),
        UiAction::Wharf(WharfAction::Locker)
        | UiAction::Wharf(WharfAction::Part {
            from: Source::Locker,
            ..
        }) => Some(Over::Locker),
        _ => None,
    }
}

/// Draws the cradle's labels, rack and drag tag for the wharf `room`, and returns its
/// buttons. `free`: the room the panel leaves on the left, on the canvas.
pub fn furniture(
    o: &mut Overlay,
    room: &Room<'_>,
    camera: &Camera,
    viewport: G2,
    scale: f32,
    free: Rect,
) -> Vec<Button> {
    let (world, history) = (room.world, room.history);
    let ui = &room.ui.wharf;
    let mut buttons = Vec::new();
    if galaxy_progress::hullwright::wharf(world, history, room.station).is_none() {
        return buttons;
    }
    // Clicks on the cradle stop there (never opening a module under it); with a part
    // in hand they put it down.
    if let Some(r) = vc::canvas_rect(world, room.part, camera, viewport, scale) {
        buttons.push(Button {
            rect: r,
            action: if ui.busy() {
                UiAction::Wharf(WharfAction::Cancel)
            } else {
                UiAction::Absorb
            },
        });
    }
    let sockets = vc::screen_sockets(world, room.station, room.part, camera, viewport, scale);
    let holding = ui
        .held
        .as_ref()
        .filter(|h| h.from != Source::Socket)
        .map(|h| &h.part);
    let views = galaxy_progress::hullwright::sockets(world, history, room.station, holding);
    let takes = |s: &SocketId| views.iter().any(|v| &v.socket == s && v.takes);
    let rack = views.iter().find(|v| v.rack).filter(|_| {
        sockets
            .iter()
            .any(|s| s.kit == render::kit::Socket::Utility)
    });
    // The labels, top to bottom in the order of their sockets on screen, never
    // overlapping; the rack's box goes under the utility label.
    let mut order: Vec<&ScreenSocket> = sockets.iter().collect();
    order.sort_by(|a, b| a.at.y.total_cmp(&b.at.y));
    let x = free.x + 6.0;
    let rack_h = rack.map_or(0.0, |r| rack_height(world, &r.fitted));
    // Each label level with its socket, stacked down where they'd overlap, then the
    // whole column moved up if it runs past the bottom.
    let mut labels: Vec<(&ScreenSocket, Rect)> = Vec::new();
    let mut rack_at = None;
    let mut next_y = free.y;
    for s in order {
        let y = (s.at.y - LABEL_H * 0.5).max(next_y);
        labels.push((s, Rect::new(x, y, LABEL_W, LABEL_H)));
        next_y = y + LABEL_H + LABEL_GAP;
        if s.kit == render::kit::Socket::Utility && rack.is_some() {
            rack_at = Some(next_y - LABEL_GAP + 4.0);
            next_y += rack_h;
        }
    }
    let over = (next_y - LABEL_GAP - (free.y + free.h)).max(0.0);
    for (_, r) in &mut labels {
        r.y = (r.y - over).max(free.y);
    }
    if let (Some(r), Some(y)) = (rack, rack_at) {
        let rr = Rect::new(x, (y - over).max(free.y), LABEL_W, rack_h - 4.0);
        buttons.extend(rack_box(o, world, &r.fitted, rr));
    }
    for (s, rect) in &labels {
        let (s, rect) = (*s, rect);
        let fitted = fit::occupant(world, &s.socket);
        let target = takes(&s.socket);
        let hovered = room.ui.cursor.is_some_and(|c| contains(rect, c))
            || ui.chosen.is_some() && ui.chosen == fitted;
        let over_here = ui.over == Some(Over::Socket(s.socket.clone()));
        let (border, px) = if target {
            (SKY, if over_here { 5.0 } else { 4.0 })
        } else if hovered {
            (ORANGE, 3.5)
        } else {
            (palette::INK, 2.5)
        };
        leader(o, *rect, s, border);
        o.panels.push(Panel {
            fill: palette::PARCHMENT,
            border,
            border_px: px,
            radius: 10.0,
            shadow: false,
            ..Panel::new(*rect)
        });
        let name = fit::socket_name(world, &s.socket);
        o.texts.push(
            Text::new(name, G2::new(rect.x + 12.0, rect.y + 5.0), 15.0)
                .weight(700)
                .color(BLACK),
        );
        let fitting_here = ui.fitting.as_ref().is_some_and(|f| f.socket == s.socket);
        let what = if target {
            "Drop here".to_owned()
        } else if fitting_here {
            "Fitting…".to_owned()
        } else if let Some(p) = &fitted {
            format!("{}  ·  Fitted", fit::part_name(world, p))
        } else {
            "Free".to_owned()
        };
        o.texts.push(
            Text::new(
                crate::ui::clip(&what, 13.0, LABEL_W - 24.0),
                G2::new(rect.x + 12.0, rect.y + 25.0),
                13.0,
            )
            .weight(if target { 700 } else { 500 })
            .color(BLACK),
        );
        buttons.push(Button {
            rect: *rect,
            action: UiAction::Wharf(WharfAction::Socket(s.socket.clone())),
        });
        let r = s.radius;
        buttons.push(Button {
            rect: Rect::new(s.at.x - r, s.at.y - r, r * 2.0, r * 2.0),
            action: UiAction::Wharf(WharfAction::Socket(s.socket.clone())),
        });
    }
    drag_tag(o, room, &sockets, free);
    buttons
}

/// An inked line from a label to its socket's ring: straight across, or with an elbow
/// where the labels had to stack.
fn leader(o: &mut Overlay, label: Rect, s: &ScreenSocket, color: Color) {
    let w = 2.5;
    let mut bar = |r: Rect| {
        o.panels.push(Panel {
            fill: color,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(r)
        });
    };
    let from = G2::new(label.x + label.w, label.y + label.h * 0.5);
    let to_x = (s.at.x - s.radius).max(from.x + 4.0);
    if (from.y - s.at.y).abs() < 3.0 {
        bar(Rect::new(from.x, from.y - w * 0.5, to_x - from.x, w));
        return;
    }
    let mid = from.x + 14.0;
    bar(Rect::new(from.x, from.y - w * 0.5, mid - from.x, w));
    let (top, bottom) = (from.y.min(s.at.y), from.y.max(s.at.y));
    bar(Rect::new(mid - w * 0.5, top, w, bottom - top));
    bar(Rect::new(mid, s.at.y - w * 0.5, (to_x - mid).max(0.0), w));
}

/// The chips the rack's modules take, laid out in rows in the box's width.
fn chip_rows(world: &WorldState, modules: &[UpgradeId]) -> Vec<Vec<(UpgradeId, String, f32)>> {
    let mut rows: Vec<Vec<(UpgradeId, String, f32)>> = vec![Vec::new()];
    let mut used = 0.0;
    for m in modules {
        let name = fit::part_name(world, m);
        let w = text_width(&name, 12.0) + 14.0;
        if used + w > LABEL_W - 16.0 && !rows.last().is_some_and(Vec::is_empty) {
            rows.push(Vec::new());
            used = 0.0;
        }
        used += w + 4.0;
        rows.last_mut().expect("a row").push((m.clone(), name, w));
    }
    rows
}

fn rack_height(world: &WorldState, modules: &[UpgradeId]) -> f32 {
    let rows = if modules.is_empty() {
        1
    } else {
        chip_rows(world, modules).len()
    };
    30.0 + rows as f32 * (CHIP_H + 4.0) + 6.0
}

/// The control rack: every fitted control module as a small labelled chip. They use no
/// budget and never come off; clicking one says so.
fn rack_box(o: &mut Overlay, world: &WorldState, modules: &[UpgradeId], r: Rect) -> Vec<Button> {
    let mut buttons = Vec::new();
    o.panels.push(Panel {
        fill: palette::CREAM,
        border: palette::INK,
        border_px: 2.0,
        radius: 8.0,
        shadow: false,
        ..Panel::new(r)
    });
    o.texts.push(
        Text::new("Control rack", G2::new(r.x + 10.0, r.y + 6.0), 13.0)
            .weight(700)
            .color(BLACK),
    );
    o.texts.push(
        Text::new("stays fitted", G2::new(r.x + r.w - 10.0, r.y + 7.0), 11.0)
            .weight(600)
            .color(BLACK)
            .align(Align::Right),
    );
    if modules.is_empty() {
        o.texts.push(
            Text::new("No controls yet", G2::new(r.x + 10.0, r.y + 28.0), 12.0)
                .weight(500)
                .color(BLACK),
        );
        return buttons;
    }
    let mut y = r.y + 26.0;
    for row in chip_rows(world, modules) {
        let mut x = r.x + 8.0;
        for (id, name, w) in row {
            let chip = Rect::new(x, y, w, CHIP_H);
            o.panels.push(Panel {
                fill: palette::TEAL,
                border: palette::INK,
                border_px: 1.5,
                radius: 4.0,
                shadow: false,
                ..Panel::new(chip)
            });
            o.texts.push(
                Text::new(name, G2::new(chip.center().x, chip.y + 3.0), 12.0)
                    .weight(700)
                    .color(WHITE)
                    .align(Align::Center),
            );
            buttons.push(Button {
                rect: chip,
                action: UiAction::Wharf(WharfAction::Module(id)),
            });
            x += w + 4.0;
        }
        y += CHIP_H + 4.0;
    }
    buttons
}

/// The tag beside a dragged part: what letting go there does, in words, and what it
/// changes. It follows the mouse; a scripted drag shows it at the socket it's over.
fn drag_tag(o: &mut Overlay, room: &Room<'_>, sockets: &[ScreenSocket], free: Rect) {
    let ui = &room.ui.wharf;
    let Some(held) = ui.held.as_ref() else {
        return;
    };
    let at = match (held.dragging, room.ui.cursor, &ui.over) {
        (true, Some(c), _) => c,
        (_, _, Some(Over::Socket(s))) => match sockets.iter().find(|x| &x.socket == s) {
            Some(x) => x.at + G2::new(x.radius * 0.6, x.radius * 0.6),
            None => return,
        },
        _ => return,
    };
    let Some(p) = ui.held_preview(room.world, room.history) else {
        return;
    };
    let name = fit::part_name(room.world, &held.part).to_lowercase();
    let right_place = match &ui.over {
        Some(Over::Socket(s)) => *s == p.socket && !p.removing,
        Some(Over::Locker) => p.removing,
        None => false,
    };
    let (head, color) = match (&ui.over, right_place, &p.refusal) {
        (None, ..) => (
            if p.removing {
                "Drag it to the locker to take it off".to_owned()
            } else {
                "Drag onto a lit socket".to_owned()
            },
            palette::INK,
        ),
        (Some(_), true, Some(r)) => (format!("×  {r}"), VERMILION),
        (Some(_), true, None) if p.removing => (format!("Let go to take off the {name}"), SKY),
        (Some(_), true, None) => (format!("Let go to fit {name}"), SKY),
        (Some(_), false, _) => (
            format!(
                "×  That goes on the {}",
                fit::socket_name(room.world, &p.socket).to_lowercase()
            ),
            VERMILION,
        ),
    };
    let mut changes = Vec::new();
    let hold = p.after.hold as i64 - p.before.hold as i64;
    if hold != 0 {
        changes.push(format!("Hold {hold:+}"));
    }
    let mass = i64::from(p.budget_after.mass) - i64::from(p.budget_before.mass);
    if mass != 0 {
        changes.push(format!("Mass {mass:+}"));
    }
    let power = i64::from(p.budget_after.power_used) - i64::from(p.budget_before.power_used);
    if power != 0 {
        changes.push(format!("Power {power:+}"));
    }
    let w = 250.0;
    let head_rows = (text_width(&head, 14.0) / (w - 24.0)).ceil().max(1.0);
    let h = 14.0 + head_rows * 18.0 + if changes.is_empty() { 0.0 } else { 20.0 };
    // Beside the pointer, kept left of the room's panel.
    let r = Rect::new((at.x + 18.0).min(free.x + free.w - w), at.y + 14.0, w, h);
    o.panels.push(Panel {
        fill: palette::PARCHMENT,
        border: color,
        border_px: 3.0,
        radius: 10.0,
        shadow: true,
        ..Panel::new(r)
    });
    o.texts.push(
        Text::new(head, G2::new(r.x + 12.0, r.y + 7.0), 14.0)
            .weight(700)
            .color(BLACK)
            .wrap(w - 24.0),
    );
    if !changes.is_empty() {
        o.texts.push(
            Text::new(
                changes.join("  ·  "),
                G2::new(r.x + 12.0, r.y + 7.0 + head_rows * 18.0),
                13.0,
            )
            .weight(500)
            .color(BLACK),
        );
    }
}
