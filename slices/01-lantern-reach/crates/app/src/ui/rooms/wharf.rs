//! The wharf room: the hullwright's cradle (#47, 3.2). The ship is lifted into the
//! cradle on the station, left of this panel ([`super::cradle`] and
//! [`crate::view::cradle`]); the panel holds the rest, top to bottom:
//!
//! - **Budgets**: POWER as lamp pips ("3 of 4") and MASS as weights ("1 → 5 of 3
//!   free", the arrow drawn), with what a held part would change. Over the power cap is vermilion with
//!   its word and a cross ("Over power: 5 of 4").
//! - **The preview** of the part in hand ("IF YOU FIT HOLD PODS": top speed, turn,
//!   hold, mining, reach, before → after with ▲ or ▼ and the number), or of taking a
//!   fitted part off. With nothing in hand: the latest change as a "Confirmed" ticket
//!   and the keeper's word on it, or how to fit.
//! - **The tray**: Ready (for credits), Make (materials and a blueprint, always behind a
//!   confirm that lists what it uses) and Friends (sent after your next docking at the
//!   friend's station). A part the player has shows where it is (Fitted, In the locker,
//!   On its way), so nothing ever vanishes (#44).
//! - **The locker**: the hull's spare parts, to drag back onto a socket; a fitted part
//!   dragged here comes off.
//!
//! A wharf the fit rules don't know (a stand-in) lists its keeper's offers as before.

use galaxy_contract::fit::is_module;
use galaxy_contract::prelude::*;
use galaxy_progress::hullwright::{self, Held as Where, PartOffer, Stats, WharfView};
use render::glam::Vec2 as G2;
use render::{Align, Overlay, Panel, Rect, Text, palette};

use super::fit::{self, HeldPreview, Source, Tab, WharfAction};
use super::{Room, action, notice, offer_list, offers_of};
use crate::hooks::OfferRow;
use crate::trades::credits;
use crate::ui::station::{VERMILION, dashed_outline};
use crate::ui::talk::{BLUE, ORANGE, SKY};
use crate::ui::{BLACK, Button, UiAction, WHITE, button, clip, owned_mark, text_width};

/// One tray row's height.
const ROW: f32 = 58.0;
const TAB_H: f32 = 32.0;
const LOCKER_H: f32 = 66.0;
const PAGER_H: f32 = 30.0;

fn wharf_action(a: WharfAction) -> UiAction {
    UiAction::Wharf(a)
}

/// The keeper's first name, for their words in the room.
fn keeper_first(room: &Room<'_>) -> String {
    let name = room
        .world
        .npcs
        .get(&room.part.keeper)
        .map_or_else(|| "The hullwright".to_owned(), |n| n.name.clone());
    name.split_whitespace().next().unwrap_or(&name).to_owned()
}

pub fn draw(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    let Some(view) = hullwright::wharf(room.world, room.history, room.station) else {
        // A stand-in wharf: its keeper's offers, as before.
        let offers = offers_of(room, &room.part.keeper);
        if offers.is_empty() {
            line(
                o,
                "Nothing on the rack today.",
                body.x,
                body.y + 4.0,
                17.0,
                600,
            );
            return Vec::new();
        }
        return offer_list(o, room, &offers, body).0;
    };
    let ui = &room.ui.wharf;
    let mut buttons = Vec::new();
    // Clicks on the panel's empty parts put down whatever is in hand.
    if ui.busy() {
        buttons.push(Button {
            rect: body,
            action: wharf_action(WharfAction::Cancel),
        });
    }
    let held = ui.held_preview(room.world, room.history);
    let chosen = ui.chosen.as_ref().and_then(|p| {
        let h = fit::Held {
            part: p.clone(),
            from: Source::Socket,
            dragging: false,
        };
        let state = fit::WharfUi {
            held: Some(h),
            ..fit::WharfUi::default()
        };
        state.held_preview(room.world, room.history)
    });
    let shown = held.as_ref().or(chosen.as_ref());
    let mut y = body.y;
    y += budgets(o, room, shown, body.x, y, body.w) + 10.0;
    if let Some((part, fit_too)) = &ui.make {
        buttons.extend(make_confirm(o, room, part, *fit_too, body, y));
        return buttons;
    }
    let locker_y = body.y + body.h - LOCKER_H;
    let slot_h = if let Some(p) = &held {
        preview(o, room.world, p, body.x, y, body.w)
    } else if let Some(p) = &chosen {
        let h = preview(o, room.world, p, body.x, y, body.w);
        let by = y + h + 2.0;
        let bw = (body.w - 12.0) * 0.5;
        buttons.push(action(
            o,
            room.ui,
            Rect::new(body.x, by, bw, 32.0),
            "Remove to the locker",
            p.refusal.is_none(),
            wharf_action(WharfAction::Remove(p.part.clone())),
        ));
        buttons.push(button(
            o,
            room.ui,
            Rect::new(body.x + bw + 12.0, by, bw, 32.0),
            "Keep it on",
            true,
            wharf_action(WharfAction::Cancel),
        ));
        h + 40.0
    } else {
        word(o, room, body.x, y, body.w)
    };
    y += slot_h + 8.0;
    // The tabs, each with how many parts it has.
    let tab_w = (body.w - 16.0) / 3.0;
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let rect = Rect::new(body.x + i as f32 * (tab_w + 8.0), y, tab_w, TAB_H);
        let label = format!("{} {}", tab.label(), rows_of(room, &view, tab).len());
        buttons.push(tab_button(o, room, rect, &label, ui.tab == tab, tab));
    }
    y += TAB_H + 8.0;
    let tray = Rect::new(body.x, y, body.w, (locker_y - 8.0 - y).max(ROW));
    buttons.extend(tray_rows(o, room, &view, tray));
    buttons.extend(locker(o, room, &view, body.x, locker_y, body.w));
    buttons
}

fn line(o: &mut Overlay, text: &str, x: f32, y: f32, size: f32, weight: u16) {
    o.texts.push(
        Text::new(text, G2::new(x, y), size)
            .weight(weight)
            .color(BLACK),
    );
}

fn caption(o: &mut Overlay, text: &str, x: f32, y: f32) {
    line(o, text, x, y, 12.0, 700);
}

/// A tab: the open one inked dark with white words, the others light with black.
fn tab_button(
    o: &mut Overlay,
    room: &Room<'_>,
    rect: Rect,
    label: &str,
    open: bool,
    tab: Tab,
) -> Button {
    if !open {
        return button(
            o,
            room.ui,
            rect,
            label,
            true,
            wharf_action(WharfAction::Tab(tab)),
        );
    }
    o.panels.push(Panel {
        fill: palette::INK,
        border: palette::INK,
        border_px: 2.0,
        radius: 9.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(label, G2::new(rect.center().x, rect.y + 7.0), 15.0)
            .weight(700)
            .color(WHITE)
            .align(Align::Center),
    );
    Button {
        rect,
        action: wharf_action(WharfAction::Tab(tab)),
    }
}

/// POWER and MASS. Returns the height used.
fn budgets(
    o: &mut Overlay,
    room: &Room<'_>,
    shown: Option<&HeldPreview>,
    x: f32,
    y: f32,
    w: f32,
) -> f32 {
    let Some(now) = hullwright::budget(room.world) else {
        return 0.0;
    };
    let after = shown.map_or(now, |p| p.budget_after);
    let px = x + 70.0;
    // Power: a lamp pip per unit of the cap, lit for what runs; past the cap, crossed
    // pips in vermilion.
    caption(o, "POWER", x, y + 4.0);
    let pips = now.power_cap.max(after.power_used);
    for i in 0..pips {
        let r = Rect::new(px + i as f32 * 24.0, y, 18.0, 18.0);
        let over = i >= now.power_cap;
        let lit = i < after.power_used;
        o.panels.push(Panel {
            fill: if over {
                VERMILION
            } else if lit {
                palette::LANTERN
            } else {
                palette::PARCHMENT
            },
            border: palette::INK,
            border_px: 2.0,
            radius: 9.0,
            shadow: false,
            ..Panel::new(r)
        });
        if over {
            o.texts.push(
                Text::new("×", G2::new(r.center().x, r.y), 14.0)
                    .weight(700)
                    .color(WHITE)
                    .align(Align::Center),
            );
        } else if lit && i >= now.power_used {
            // What the held part adds: an inked dot in the lamp.
            o.panels.push(Panel {
                fill: palette::INK,
                border_px: 0.0,
                radius: 3.0,
                shadow: false,
                ..Panel::new(Rect::new(r.x + 6.0, r.y + 6.0, 6.0, 6.0))
            });
        }
    }
    // The words sit in a column after the longest row of pips or weights.
    let col = x + w - 238.0;
    if after.over_power() {
        let power = format!("×  Over power: {} of {}", after.power_used, after.power_cap);
        let tw = text_width(&power, 14.0) + 24.0;
        let r = Rect::new(x + w - tw, y - 3.0, tw, 24.0);
        o.panels.push(Panel {
            fill: VERMILION,
            border: palette::INK,
            border_px: 1.5,
            radius: 12.0,
            shadow: false,
            ..Panel::new(r)
        });
        o.texts.push(
            Text::new(power, G2::new(r.center().x, r.y + 3.0), 14.0)
                .weight(700)
                .color(WHITE)
                .align(Align::Center),
        );
    } else if after.power_used != now.power_used {
        change(
            o,
            &now.power_used.to_string(),
            &format!("{} of {}", after.power_used, now.power_cap),
            col,
            y + 1.0,
            14.0,
        );
    } else {
        line(
            o,
            &format!("{} of {}", now.power_used, now.power_cap),
            col,
            y + 1.0,
            14.0,
            700,
        );
    }
    // Mass: a weight per unit carried; the free allowance is outlined, and weights
    // past it (which slow the ship) are vermilion, after a tick mark.
    let my = y + 26.0;
    caption(o, "MASS", x, my + 4.0);
    let slots = now.free_mass.max(now.mass).max(after.mass);
    for i in 0..slots {
        let r = Rect::new(px + i as f32 * 20.0, my, 14.0, 18.0);
        let carried = i < after.mass;
        let added = carried && i >= now.mass;
        let slowing = carried && i >= now.free_mass;
        if i < now.mass && !carried {
            // Taken off: a dashed outline where it was.
            dashed_outline(o, r, palette::INK, 1.5);
            continue;
        }
        o.panels.push(Panel {
            fill: if slowing {
                VERMILION
            } else if added {
                SKY
            } else if carried {
                palette::BRASS
            } else {
                palette::PARCHMENT
            },
            border: palette::INK,
            border_px: if carried { 2.0 } else { 1.0 },
            radius: 3.0,
            shadow: false,
            ..Panel::new(r)
        });
        if i + 1 == now.free_mass {
            o.panels.push(Panel {
                fill: palette::INK,
                border_px: 0.0,
                radius: 1.0,
                shadow: false,
                ..Panel::new(Rect::new(r.x + r.w + 2.0, my - 4.0, 2.0, 26.0))
            });
        }
    }
    let rest = format!("{} of {} free", after.mass, now.free_mass);
    let rest = if after.excess_mass() > 0 {
        format!("{rest}  ·  {} over, slower", after.excess_mass())
    } else {
        rest
    };
    if after.mass != now.mass {
        change(o, &now.mass.to_string(), &rest, col, my + 1.0, 14.0);
    } else {
        line(o, &rest, col, my + 1.0, 14.0, 700);
    }
    46.0
}

/// An arrow pointing right, `len` long, centred on `cy`: drawn, since the UI font has
/// no arrows.
fn arrow(o: &mut Overlay, x: f32, cy: f32, len: f32, color: render::Color) {
    let mut bar = |r: Rect| {
        o.panels.push(Panel {
            fill: color,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(r)
        });
    };
    bar(Rect::new(x, cy - 1.25, len - 4.0, 2.5));
    for i in 0..5 {
        let h = 10.0 - 2.0 * i as f32;
        bar(Rect::new(
            x + len - 6.0 + i as f32 * 1.4,
            cy - h * 0.5,
            1.6,
            h,
        ));
    }
}

/// A small triangle, pointing up (a gain) or down (a loss), centred on `(cx, cy)`.
fn triangle(o: &mut Overlay, cx: f32, cy: f32, up: bool, color: render::Color) {
    for i in 0..5 {
        let w = 2.0 + 2.0 * i as f32;
        let row = if up { i } else { 4 - i } as f32;
        o.panels.push(Panel {
            fill: color,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(cx - w * 0.5, cy - 5.0 + row * 2.0, w, 2.0))
        });
    }
}

/// `before`, an arrow, then `after`, left to right from `x`, in bold black.
fn change(o: &mut Overlay, before: &str, after: &str, x: f32, y: f32, size: f32) {
    line(o, before, x, y, size, 700);
    let ax = x + text_width(before, size) + 6.0;
    arrow(o, ax, y + size * 0.62, 16.0, palette::INK);
    line(o, after, ax + 22.0, y, size, 700);
}

/// A change as a pill: ▲ and the number in blue for a gain, ▼ in vermilion for a loss,
/// "same" in words. Right-aligned at `right`.
fn badge(o: &mut Overlay, delta: f32, decimals: usize, right: f32, y: f32) {
    let shown = (delta.abs() * 10f32.powi(decimals as i32)).round();
    if shown == 0.0 {
        o.texts.push(
            Text::new("same", G2::new(right - 31.0, y + 2.0), 13.0)
                .weight(600)
                .color(BLACK)
                .align(Align::Center),
        );
        return;
    }
    let up = delta > 0.0;
    let fill = if up { BLUE } else { VERMILION };
    let r = Rect::new(right - 62.0, y, 62.0, 20.0);
    o.panels.push(Panel {
        fill,
        border_px: 0.0,
        radius: 10.0,
        shadow: false,
        ..Panel::new(r)
    });
    triangle(o, r.x + 14.0, r.y + 10.0, up, WHITE);
    o.texts.push(
        Text::new(
            format!("{:.*}", decimals, delta.abs()),
            G2::new(r.x + 38.0, r.y + 2.0),
            13.0,
        )
        .weight(700)
        .color(WHITE)
        .align(Align::Center),
    );
}

/// The stat rows a preview compares: (label, before, after, decimals).
fn stat_rows(before: &Stats, after: &Stats) -> [(&'static str, f32, f32, usize); 5] {
    [
        ("Top speed", before.top_speed, after.top_speed, 0),
        ("Turn", before.turn_rate, after.turn_rate, 0),
        ("Hold", before.hold as f32, after.hold as f32, 0),
        ("Mining", before.mining_rate, after.mining_rate, 1),
        ("Reach", before.mining_reach, after.mining_reach, 0),
    ]
}

/// "IF YOU FIT HOLD PODS" (or "IF YOU TAKE OFF ..."): each stat before → after with
/// its change, what goes to the locker, and the keeper's refusal if there is one.
/// Returns the height used.
fn preview(o: &mut Overlay, world: &WorldState, p: &HeldPreview, x: f32, y: f32, w: f32) -> f32 {
    let title = if p.removing {
        format!("IF YOU TAKE OFF THE {}", p.name.to_uppercase())
    } else {
        format!("IF YOU FIT {}", p.name.to_uppercase())
    };
    caption(o, &title, x, y);
    let mut ry = y + 18.0;
    let num_x = x + w - 150.0;
    for (label, before, after, d) in stat_rows(&p.before, &p.after) {
        line(o, label, x, ry, 14.0, 500);
        o.texts.push(
            Text::new(format!("{before:.d$}"), G2::new(num_x, ry), 14.0)
                .weight(700)
                .color(BLACK)
                .align(Align::Right),
        );
        arrow(o, num_x + 6.0, ry + 14.0 * 0.62, 16.0, palette::INK);
        line(o, &format!("{after:.d$}"), num_x + 28.0, ry, 14.0, 700);
        badge(o, after - before, d, x + w, ry - 1.0);
        ry += 20.0;
    }
    if let Some(out) = &p.swaps_out {
        ry += 2.0;
        line(
            o,
            &format!(
                "Your {} goes to the locker.",
                fit::part_name(world, out).to_lowercase()
            ),
            x,
            ry,
            13.0,
            600,
        );
        ry += 18.0;
    }
    ry += 4.0;
    ry - y
}

/// With nothing in hand: the latest change as a ticket and the keeper's word, a
/// refusal in the keeper's words, or how to fit. Returns the height used.
fn word(o: &mut Overlay, room: &Room<'_>, x: f32, y: f32, w: f32) -> f32 {
    let ui = &room.ui.wharf;
    let who = keeper_first(room);
    let mut h = 0.0;
    if let Some(said) = &ui.said {
        let text = format!(
            "{}{who}: “{}”",
            if said.refusal { "×  " } else { "" },
            said.text
        );
        let color = if said.refusal { VERMILION } else { BLUE };
        return notice(o, &text, color, x, y, w) + 4.0;
    }
    if let Some(done) = room.notices.fitted {
        h += ticket(o, room, done, x, y, w) + 8.0;
        if let Some(said) = ui.sent.as_ref().and_then(|r| fit::done_line(room.world, r)) {
            let text = format!("{who}: “{said}”");
            h += notice(o, &text, BLUE, x, y + h, w) + 4.0;
        }
        return h;
    }
    if let Some(refused) = room.notices.service {
        let text = format!("×  {who}: “{refused}”");
        return notice(o, &text, VERMILION, x, y, w) + 4.0;
    }
    line(
        o,
        "Drag a part from the tray onto a socket, or click a part and then a socket.",
        x,
        y,
        14.0,
        500,
    );
    o.texts.last_mut().map(|t| t.max_width = Some(w));
    44.0
}

/// A "Confirmed" ticket, as the dock confirms what was given and what came back: a
/// dark ticket with an orange edge, its words in white.
fn ticket(o: &mut Overlay, room: &Room<'_>, text: &str, x: f32, y: f32, w: f32) -> f32 {
    let rows = (text_width(text, 15.0) / (w - 28.0)).ceil().max(1.0);
    let h = 30.0 + rows * 19.0;
    o.panels.push(Panel {
        fill: palette::INK,
        border: ORANGE,
        border_px: 3.0,
        radius: 10.0,
        shadow: false,
        ..Panel::new(Rect::new(x, y, w, h))
    });
    let place = room
        .world
        .stations
        .get(room.station)
        .map_or_else(String::new, |s| format!("  ·  {}", s.name));
    o.texts.push(
        Text::new(
            format!("Confirmed{place}"),
            G2::new(x + 14.0, y + 7.0),
            12.0,
        )
        .weight(700)
        .color(WHITE),
    );
    o.texts.push(
        Text::new(text, G2::new(x + 14.0, y + 24.0), 15.0)
            .weight(600)
            .color(WHITE)
            .wrap(w - 28.0),
    );
    h
}

/// The Make confirm: what making the part takes from the hold, before anything is
/// spent, and the choice to make it (and fit it, from a drop), or not.
fn make_confirm(
    o: &mut Overlay,
    room: &Room<'_>,
    part: &UpgradeId,
    fit_too: bool,
    body: Rect,
    y: f32,
) -> Vec<Button> {
    let mut buttons = Vec::new();
    let name = fit::part_name(room.world, part);
    let bill = hullwright::make_bill(room.world, part);
    let rect = Rect::new(body.x, y, body.w, (body.y + body.h - y).min(260.0));
    o.panels.push(Panel {
        fill: palette::CREAM,
        border: palette::INK,
        border_px: 3.0,
        radius: 12.0,
        shadow: false,
        ..Panel::new(rect)
    });
    let x = rect.x + 16.0;
    let mut ry = rect.y + 14.0;
    line(
        o,
        &format!("Make the {}?", name.to_lowercase()),
        x,
        ry,
        19.0,
        700,
    );
    ry += 30.0;
    caption(o, "IT USES, FROM YOUR HOLD", x, ry);
    ry += 18.0;
    let mut enough = true;
    for m in &bill {
        let short = m.have < m.need;
        enough &= !short;
        let text = if short {
            format!("×  {} {}  ·  you have {}", m.need, m.name, m.have)
        } else {
            format!("{} {}  ·  you have {}", m.need, m.name, m.have)
        };
        if short {
            o.panels.push(Panel {
                fill: VERMILION,
                border_px: 0.0,
                radius: 2.0,
                shadow: false,
                ..Panel::new(Rect::new(x, ry, 5.0, 18.0))
            });
        }
        line(o, &text, x + 12.0, ry, 15.0, 600);
        ry += 22.0;
    }
    if bill.is_empty() {
        line(o, "Nothing from your hold.", x, ry, 15.0, 600);
        ry += 22.0;
    }
    ry += 4.0;
    let after = if fit_too {
        format!(
            "Then I fit it on the {}.",
            fit::socket_of(room.world, part).map_or_else(
                || "ship".into(),
                |s| fit::socket_name(room.world, &s).to_lowercase()
            )
        )
    } else {
        "It goes into your locker, to fit when you like.".to_owned()
    };
    let who = keeper_first(room);
    line(o, &format!("{who}: “{after}”"), x, ry, 14.0, 500);
    ry += 30.0;
    let bw = (rect.w - 32.0 - 12.0) * 0.5;
    buttons.push(action(
        o,
        room.ui,
        Rect::new(x, ry, bw, 34.0),
        if fit_too { "Make and fit" } else { "Make it" },
        enough,
        wharf_action(WharfAction::ConfirmMake),
    ));
    buttons.push(button(
        o,
        room.ui,
        Rect::new(x + bw + 12.0, ry, bw, 34.0),
        "Not now",
        true,
        wharf_action(WharfAction::Cancel),
    ));
    buttons
}

/// The Ready tab's rows besides parts: control modules not yet fitted, tenders and
/// trading up, from the keeper's offers.
fn ready_extras(room: &Room<'_>) -> Vec<OfferRow> {
    offers_of(room, &room.part.keeper)
        .into_iter()
        .filter(|o| match &o.request {
            ServiceRequest::Fit { upgrade } => is_module(room.world, upgrade) && !o.owned,
            ServiceRequest::Build { .. } | ServiceRequest::Prestige { .. } => true,
            _ => false,
        })
        .collect()
}

enum Row<'a> {
    Part(&'a PartOffer),
    Offer(OfferRow),
}

/// A tab's rows: its parts (Ready also lists the keeper's control modules not yet
/// fitted, tenders and trading up; fitted modules show in the rack beside the ship).
fn rows_of<'a>(room: &Room<'_>, view: &'a WharfView, tab: Tab) -> Vec<Row<'a>> {
    match tab {
        Tab::Ready => view
            .ready
            .iter()
            .filter(|p| !is_module(room.world, &p.part))
            .map(Row::Part)
            .chain(ready_extras(room).into_iter().map(Row::Offer))
            .collect(),
        Tab::Make => view.make.iter().map(Row::Part).collect(),
        Tab::Friends => view.friends.iter().map(Row::Part).collect(),
    }
}

/// The open tab's rows, paged.
fn tray_rows(o: &mut Overlay, room: &Room<'_>, view: &WharfView, tray: Rect) -> Vec<Button> {
    let ui = &room.ui.wharf;
    let mut buttons = Vec::new();
    let rows = rows_of(room, view, ui.tab);
    if rows.is_empty() {
        let text = match ui.tab {
            Tab::Ready => "Nothing on the rack today.",
            Tab::Make => "No blueprints here yet. Blueprints turn up out in the Reach.",
            Tab::Friends => "Nobody sends parts through here yet.",
        };
        line(o, text, tray.x, tray.y + 4.0, 15.0, 600);
        o.texts.last_mut().map(|t| t.max_width = Some(tray.w));
        return buttons;
    }
    let fits = (tray.h / ROW).floor().max(1.0) as usize;
    let (first, shown) = if rows.len() <= fits {
        (0, rows.len())
    } else {
        let n = ((tray.h - PAGER_H) / ROW).floor().max(1.0) as usize;
        (ui.first.min(rows.len().saturating_sub(n)), n)
    };
    let mut y = tray.y;
    for row in rows.iter().skip(first).take(shown) {
        match row {
            Row::Part(p) => buttons.extend(part_row(o, room, p, tray.x, y, tray.w)),
            Row::Offer(r) => buttons.extend(offer_row(o, room, r, tray.x, y, tray.w)),
        }
        y += ROW;
    }
    if shown < rows.len() {
        let last = first + shown;
        line(
            o,
            &format!("{}–{} of {}", first + 1, last, rows.len()),
            tray.x,
            y + 7.0,
            13.0,
            600,
        );
        buttons.push(action(
            o,
            room.ui,
            Rect::new(tray.x + tray.w - 168.0, y + 2.0, 80.0, 26.0),
            "Up",
            first > 0,
            wharf_action(WharfAction::From(first.saturating_sub(shown))),
        ));
        buttons.push(action(
            o,
            room.ui,
            Rect::new(tray.x + tray.w - 80.0, y + 2.0, 80.0, 26.0),
            "Down",
            last < rows.len(),
            wharf_action(WharfAction::From(last)),
        ));
    }
    buttons
}

/// A grip of dots: this row can be picked up and dragged.
fn grip(o: &mut Overlay, x: f32, y: f32) {
    for r in 0..3 {
        for c in 0..2 {
            o.panels.push(Panel {
                fill: palette::INK,
                border_px: 0.0,
                radius: 1.5,
                shadow: false,
                ..Panel::new(Rect::new(x + c as f32 * 6.0, y + r as f32 * 6.0, 3.0, 3.0))
            });
        }
    }
}

/// One part in the tray: name and price, what it does, its socket and budgets, and
/// where it is if it's the player's already. A row that can be fitted is a handle to
/// drag (or click, then click a socket).
fn part_row(
    o: &mut Overlay,
    room: &Room<'_>,
    p: &PartOffer,
    x: f32,
    y: f32,
    w: f32,
) -> Vec<Button> {
    let ui = &room.ui.wharf;
    let mut buttons = Vec::new();
    let state_w = 116.0;
    let text_x = x + 18.0;
    let text_w = w - state_w - 30.0;
    let row = Rect::new(x - 8.0, y, w + 16.0, ROW - 6.0);
    let in_hand = ui.held.as_ref().is_some_and(|h| h.part == p.part);
    let draggable = p.held.is_none() && p.tab != "Friends";
    if in_hand {
        // On the hook: a dashed sky-blue outline where it was.
        dashed_outline(o, row, SKY, 2.5);
    } else if draggable && room.ui.cursor.is_some_and(|c| crate::ui::contains(&row, c)) {
        o.panels.push(Panel {
            fill: palette::MUSTARD.with_alpha(0.22),
            border: ORANGE,
            border_px: 2.0,
            radius: 9.0,
            shadow: false,
            ..Panel::new(row)
        });
    }
    if draggable {
        grip(o, x, y + 14.0);
        buttons.push(Button {
            rect: row,
            action: wharf_action(WharfAction::Part {
                part: p.part.clone(),
                from: Source::Tray,
            }),
        });
    }
    let price = match p.tab {
        "Make" => String::new(),
        _ if p.held.is_some() => String::new(),
        _ => format!("  ·  {}", credits(p.cost)),
    };
    let title = if in_hand {
        format!("{}  ·  on the crane hook", p.name)
    } else {
        format!("{}{price}", p.name)
    };
    line(o, &title, text_x, y + 2.0, 16.0, 700);
    line(
        o,
        &clip(&p.effect, 13.0, text_w),
        text_x,
        y + 22.0,
        13.0,
        500,
    );
    let socket = fit::socket_name(room.world, &p.socket);
    let mut facts = format!("{socket}  ·  power {}  ·  mass {}", p.power, p.mass);
    match p.tab {
        "Make" if p.held.is_none() => {
            let bill: Vec<String> = p
                .materials
                .iter()
                .map(|m| format!("{} {} ({} in hold)", m.need, m.name, m.have))
                .collect();
            facts = format!("Uses {}", bill.join(", "));
        }
        "Friends" => {
            if let Some((npc, station)) = &p.friend {
                let who = room
                    .world
                    .npcs
                    .get(npc)
                    .map_or_else(|| npc.to_string(), |n| n.name.clone());
                let at = room
                    .world
                    .stations
                    .get(station)
                    .map_or_else(|| station.to_string(), |s| s.name.clone());
                facts = if p.held == Some(Where::OnOrder) {
                    format!("Arrives after your next docking at {at}")
                } else {
                    format!("From {who} at {at}")
                };
            }
        }
        _ => {}
    }
    if p.held.is_none()
        && let Some(b) = &p.blocked
        && !matches!(
            b,
            galaxy_progress::services::Refusal::Fit(
                galaxy_progress::hullwright::FitRefusal::OverPower { .. }
            )
        )
    {
        facts = crate::trades::service_refusal(room.world, &p.request, b);
    }
    o.texts.push(
        Text::new(clip(&facts, 13.0, text_w), G2::new(text_x, y + 38.0), 13.0)
            .weight(700)
            .color(BLACK),
    );
    let state = Rect::new(x + w - state_w, y + 8.0, state_w, 32.0);
    match p.held {
        Some(Where::Fitted) => owned_mark(o, state, &p.request),
        Some(Where::Locker) => mark(o, state, "In the locker"),
        Some(Where::OnOrder) => mark(o, state, "On its way"),
        None => match p.tab {
            "Make" => buttons.push(button(
                o,
                room.ui,
                state,
                "Make…",
                true,
                wharf_action(WharfAction::Make {
                    part: p.part.clone(),
                    fit: false,
                }),
            )),
            "Friends" => buttons.push(action(
                o,
                room.ui,
                state,
                "Order",
                p.blocked.is_none(),
                UiAction::Service(p.request.clone()),
            )),
            _ => {
                o.texts.push(
                    Text::new(
                        "Drag to fit",
                        G2::new(state.center().x, state.y + 8.0),
                        13.0,
                    )
                    .weight(600)
                    .color(BLACK)
                    .align(Align::Center),
                );
            }
        },
    }
    buttons
}

/// A state in an outline (not a button).
fn mark(o: &mut Overlay, rect: Rect, word: &str) {
    o.panels.push(Panel {
        fill: palette::PARCHMENT.with_alpha(0.0),
        border: BLACK,
        border_px: 2.0,
        radius: 8.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(word, G2::new(rect.center().x, rect.y + 8.0), 14.0)
            .weight(700)
            .color(BLACK)
            .align(Align::Center),
    );
}

/// A keeper's offer that isn't a part: a control module, a tender, trading up.
fn offer_row(
    o: &mut Overlay,
    room: &Room<'_>,
    r: &OfferRow,
    x: f32,
    y: f32,
    w: f32,
) -> Vec<Button> {
    let btn_w = 116.0;
    let text_x = x + 18.0;
    let text_w = w - btn_w - 30.0;
    let title = if r.owned {
        r.name.clone()
    } else {
        format!("{}  ·  {}", r.name, r.price)
    };
    line(o, &title, text_x, y + 2.0, 16.0, 700);
    line(
        o,
        &clip(&r.effect, 13.0, text_w),
        text_x,
        y + 22.0,
        13.0,
        500,
    );
    let what = match &r.request {
        ServiceRequest::Fit { .. } => "Control rack  ·  no power, no mass".to_owned(),
        _ => String::new(),
    };
    let facts = match (&r.blocked, what.is_empty()) {
        (Some(b), true) => b.clone(),
        (Some(b), false) => format!("{what}  ·  {b}"),
        (None, _) => what,
    };
    if !facts.is_empty() {
        line(o, &clip(&facts, 13.0, text_w), text_x, y + 38.0, 13.0, 700);
    }
    let state = Rect::new(x + w - btn_w, y + 8.0, btn_w, 32.0);
    if r.owned {
        owned_mark(o, state, &r.request);
        return Vec::new();
    }
    let (label, act) = match &r.request {
        ServiceRequest::Prestige { to } => ("Trade up", UiAction::Prestige(to.clone())),
        ServiceRequest::Build { .. } => ("Build", UiAction::Service(r.request.clone())),
        _ if r.price == "a gift" => ("Take it", UiAction::Service(r.request.clone())),
        _ => ("Fit", UiAction::Service(r.request.clone())),
    };
    vec![action(o, room.ui, state, label, r.blocked.is_none(), act)]
}

/// The hull's locker: a drop zone for taking parts off, with its parts as chips to drag
/// back onto a socket.
fn locker(
    o: &mut Overlay,
    room: &Room<'_>,
    view: &WharfView,
    x: f32,
    y: f32,
    w: f32,
) -> Vec<Button> {
    let ui = &room.ui.wharf;
    let mut buttons = Vec::new();
    let zone = Rect::new(x, y, w, LOCKER_H);
    let ship = galaxy_contract::fit::flagship(room.world).map_or("ship", |s| s.name.as_str());
    let taking_off = ui.held.as_ref().is_some_and(|h| h.from == Source::Socket);
    let over = taking_off && ui.over == Some(fit::Over::Locker);
    if taking_off {
        // A drop target: dashed sky blue, white under it.
        o.panels.push(Panel {
            fill: if over {
                SKY.with_alpha(0.18)
            } else {
                palette::PARCHMENT.with_alpha(0.0)
            },
            border: WHITE,
            border_px: 5.0,
            radius: 10.0,
            shadow: false,
            ..Panel::new(zone)
        });
        dashed_outline(o, zone, SKY, 3.0);
    } else {
        o.panels.push(Panel {
            fill: palette::MUSTARD.with_alpha(0.12),
            border: palette::INK,
            border_px: 2.0,
            radius: 10.0,
            shadow: false,
            ..Panel::new(zone)
        });
    }
    buttons.push(Button {
        rect: zone,
        action: wharf_action(WharfAction::Locker),
    });
    caption(
        o,
        &format!("THE {}'S LOCKER", ship.to_uppercase()),
        x + 12.0,
        y + 8.0,
    );
    let hint = if taking_off {
        "Let go here to take it off"
    } else if view.locker.is_empty() {
        "Empty. Drag a fitted part here to take it off."
    } else {
        "Drag a part back onto a socket"
    };
    o.texts.push(
        Text::new(hint, G2::new(x + w - 12.0, y + 7.0), 12.0)
            .weight(600)
            .color(BLACK)
            .align(Align::Right),
    );
    let mut cx = x + 12.0;
    for item in &view.locker {
        let cw = text_width(&item.name, 14.0) + 28.0;
        if cx + cw > x + w - 8.0 {
            break;
        }
        let chip = Rect::new(cx, y + 28.0, cw, 28.0);
        let in_hand = ui.held.as_ref().is_some_and(|h| h.part == item.part);
        if in_hand {
            dashed_outline(o, chip, SKY, 2.5);
        } else {
            o.panels.push(Panel {
                fill: palette::CREAM,
                border: palette::INK,
                border_px: 2.0,
                radius: 8.0,
                shadow: false,
                ..Panel::new(chip)
            });
        }
        grip(o, chip.x + 6.0, chip.y + 7.0);
        line(o, &item.name, chip.x + 20.0, chip.y + 5.0, 14.0, 700);
        buttons.push(Button {
            rect: chip,
            action: wharf_action(WharfAction::Part {
                part: item.part.clone(),
                from: Source::Locker,
            }),
        });
        cx += cw + 8.0;
    }
    buttons
}
