//! Rooms: what a station part opens into, as a panel beside its module (issue #47,
//! 3.1). The panel covers at most [`super::station::ROOM_SHARE`] of the screen, so the
//! station stays in view on its left.
//!
//! **The extension point.** [`draw`] is the one function that draws a room and returns
//! its buttons. It frames every room the same way (its name and keeper, the sign, the
//! keeper's line when there's news or it's shut, Back, how to leave) and hands the body
//! to the room kind's own function in [`body`]. A new kind of room is one more arm
//! there; the later hullwright cradle view replaces [`wharf::draw`]. Room actions are
//! the screen's ordinary [`UiAction`]s (trades, services, asking), so the window, the
//! key scripts and the tests drive rooms the same way.

pub mod counter;
pub mod market;
pub mod office;
pub mod wharf;

use galaxy_contract::parts::{PartState, RoomKind, StationPart};
use galaxy_contract::prelude::*;
use galaxy_core::History;
use render::glam::Vec2 as G2;
use render::{Align, Color, Overlay, Panel, Rect, Text, palette};

use super::station::{VERMILION, dashed_outline, ribbon};
use super::talk::BLUE;
use super::{BLACK, Button, DOCK_FOOT, Notices, UiAction, UiState, button, text_width};
use crate::hooks::{self, OfferRow};

/// Everything a room draws from.
#[derive(Clone, Copy)]
pub struct Room<'a> {
    pub world: &'a WorldState,
    pub history: &'a History,
    pub station: &'a StationId,
    pub part: &'a StationPart,
    pub ui: &'a UiState,
    pub notices: Notices<'a>,
}

/// The room's side padding.
pub(crate) const PAD: f32 = 24.0;
/// The foot: how to leave.
const FOOT_H: f32 = 44.0;

/// Draws `room` in `rect` and returns its buttons, Back first.
pub fn draw(o: &mut Overlay, room: &Room<'_>, rect: Rect) -> Vec<Button> {
    let mut buttons = Vec::new();
    o.panels.push(Panel {
        radius: 16.0,
        border_px: 3.5,
        ..Panel::new(rect)
    });
    // Clicks on the panel stop there, never reaching a module under it.
    buttons.push(Button {
        rect,
        action: UiAction::Absorb,
    });
    let part = room.part;
    let x = rect.x + PAD;
    let inner = rect.w - 2.0 * PAD;
    o.texts.push(
        Text::new(part.title(), G2::new(x, rect.y + 16.0), 26.0)
            .weight(700)
            .color(BLACK),
    );
    // A stand-in room at a station where nobody lives names no keeper.
    let keeper = room
        .world
        .npcs
        .get(&part.keeper)
        .map_or_else(String::new, |n| n.name.clone());
    if !keeper.is_empty() {
        o.texts.push(
            Text::new(
                format!("Kept by {keeper}"),
                G2::new(x + text_width(part.title(), 26.0) + 14.0, rect.y + 25.0),
                15.0,
            )
            .weight(600)
            .color(BLACK),
        );
    }
    buttons.push(button(
        o,
        room.ui,
        Rect::new(rect.x + rect.w - PAD - 112.0, rect.y + 14.0, 112.0, 34.0),
        "Back  ·  Esc",
        true,
        UiAction::CloseRoom,
    ));
    let mut y = rect.y + 56.0;
    o.texts.push(
        Text::new(part.sign.clone(), G2::new(x, y), 15.0)
            .weight(500)
            .color(BLACK)
            .wrap(inner),
    );
    y += 26.0;
    // News or a shut room: the keeper's line, marked with its word.
    let mark = match part.state {
        PartState::Closed => Some(("CLOSED", VERMILION)),
        PartState::Building => Some(("BUILDING", palette::INK)),
        PartState::Open => ribbon(room.world, part).map(|r| {
            (
                if r == galaxy_contract::parts::Ribbon::New {
                    "NEW"
                } else {
                    "CHANGED"
                },
                BLUE,
            )
        }),
    };
    if let Some((word, color)) = mark.filter(|_| !part.line.is_empty()) {
        let first = keeper
            .split_whitespace()
            .next()
            .unwrap_or(&keeper)
            .to_owned();
        let text = format!("{first}: “{}”", part.line);
        let rows = (text_width(&text, 15.0) / (inner - 18.0)).ceil().max(1.0);
        let h = 24.0 + rows * 19.0;
        o.panels.push(Panel {
            fill: color,
            border_px: 0.0,
            radius: 2.0,
            shadow: false,
            ..Panel::new(Rect::new(x, y, 5.0, h))
        });
        o.texts.push(
            Text::new(word, G2::new(x + 14.0, y), 12.0)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(text, G2::new(x + 14.0, y + 17.0), 15.0)
                .weight(500)
                .color(BLACK)
                .wrap(inner - 18.0),
        );
        y += h + 12.0;
    }
    o.panels.push(Panel {
        fill: palette::INK,
        border_px: 0.0,
        radius: 1.0,
        shadow: false,
        ..Panel::new(Rect::new(x, y, inner, 2.0))
    });
    y += 12.0;
    let foot_y = rect.y + rect.h - FOOT_H;
    let body = Rect::new(x, y, inner, (foot_y - y - 6.0).max(0.0));
    buttons.extend(self::body(o, room, body));
    o.texts.push(
        Text::new(DOCK_FOOT, G2::new(x, foot_y + 12.0), 15.0)
            .weight(500)
            .color(BLACK),
    );
    buttons
}

/// Draws the room of `part` at `station` where the frame puts it, with `notices`: its
/// rect and buttons. `None` if there's no such part or stand-in room at that station.
/// For tests and tools; play draws rooms through [`crate::frame::compose`].
#[allow(clippy::too_many_arguments)]
pub fn panel(
    o: &mut Overlay,
    world: &WorldState,
    history: &History,
    station: &StationId,
    part: &StationPartId,
    ui: &UiState,
    notices: Notices<'_>,
    canvas: G2,
) -> Option<(Rect, Vec<Button>)> {
    let part = super::station::room_here(world, station, part)?;
    let rect = super::station::room_rect(world, station, canvas);
    let room = Room {
        world,
        history,
        station,
        part: &part,
        ui,
        notices,
    };
    Some((rect, draw(o, &room, rect)))
}

/// The first room at `station` (a part, or a stand-in) of `kind`.
pub fn of_kind(world: &WorldState, station: &StationId, kind: RoomKind) -> Option<StationPartId> {
    super::station::rooms_here(world, station)
        .into_iter()
        .find(|p| p.room == kind)
        .map(|p| p.id.clone())
}

/// The room kind's own body. One arm per kind the code knows; every other kind is a
/// counter (its keeper and their offers). A part that isn't open shows why instead.
pub fn body(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    if room.part.state != PartState::Open {
        let text = match room.part.state {
            PartState::Building => "Not open yet. Come back once it's built.",
            _ => "Closed for now.",
        };
        o.texts.push(
            Text::new(text, G2::new(body.x, body.y + 4.0), 17.0)
                .weight(600)
                .color(BLACK)
                .wrap(body.w),
        );
        return Vec::new();
    }
    match room.part.room {
        RoomKind::Market => market::draw(o, room, body),
        RoomKind::Wharf => wharf::draw(o, room, body),
        RoomKind::Dockmaster => office::draw(o, room, body),
        _ => counter::draw(o, room, body),
    }
}

/// A room's action button: a filled button when it can be used, a dashed outline with
/// its label when it can't (never greyed text).
pub(crate) fn action(
    o: &mut Overlay,
    ui: &UiState,
    rect: Rect,
    label: &str,
    enabled: bool,
    action: UiAction,
) -> Button {
    if enabled {
        return button(o, ui, rect, label, true, action);
    }
    dashed_outline(o, rect, palette::INK, 2.0);
    o.texts.push(
        Text::new(
            label,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(500)
        .color(BLACK)
        .align(Align::Center),
    );
    Button { rect, action }
}

/// A notice in a room: its words in black beside a coloured bar (vermilion for a
/// refusal, blue for what a purchase gave). Returns its height.
pub(crate) fn notice(o: &mut Overlay, text: &str, color: Color, x: f32, y: f32, w: f32) -> f32 {
    let rows = (text_width(text, 15.0) / (w - 18.0)).ceil().max(1.0);
    let h = rows * 19.0 + 6.0;
    o.panels.push(Panel {
        fill: color,
        border_px: 0.0,
        radius: 2.0,
        shadow: false,
        ..Panel::new(Rect::new(x, y, 5.0, h))
    });
    o.texts.push(
        Text::new(text, G2::new(x + 14.0, y + 2.0), 15.0)
            .weight(600)
            .color(BLACK)
            .wrap(w - 18.0),
    );
    h
}

/// The height [`notice`] takes for `text`.
pub(crate) fn notice_height(text: &str, w: f32) -> f32 {
    (text_width(text, 15.0) / (w - 18.0)).ceil().max(1.0) * 19.0 + 6.0
}

/// What `keeper` offers at the room's station now, their services' rows in order.
pub fn offers_of(room: &Room<'_>, keeper: &NpcId) -> Vec<OfferRow> {
    hooks::station_services(room.world, room.history, room.station)
        .into_iter()
        .filter(|e| e.keeper.as_ref() == Some(keeper))
        .flat_map(|e| e.offers)
        .collect()
}

/// Offers at the station whose seller keeps no part here: the office shows them, so
/// nothing for sale goes missing.
pub fn unhoused_offers(room: &Room<'_>) -> Vec<OfferRow> {
    let keepers: Vec<&NpcId> = galaxy_core::parts::of_station(room.world, room.station)
        .map(|p| &p.keeper)
        .collect();
    hooks::station_services(room.world, room.history, room.station)
        .into_iter()
        .filter(|e| e.keeper.as_ref().is_none_or(|k| !keepers.contains(&k)))
        .flat_map(|e| e.offers)
        .collect()
}

/// One offer row's height.
pub(crate) const OFFER_ROW: f32 = 70.0;
const PAGER_H: f32 = 40.0;

/// A list of offers in `body` from `ui.services_first`: each with its name and price,
/// what it does, what stands in the way, and its button, or "Fitted" ("Yours" for a
/// tender) in an outline once it's the player's. Rows that don't fit page with Up and
/// Down. The latest refusal or purchase shows under them. Returns the buttons and the
/// height used.
pub(crate) fn offer_list(
    o: &mut Overlay,
    room: &Room<'_>,
    offers: &[OfferRow],
    body: Rect,
) -> (Vec<Button>, f32) {
    let mut buttons = Vec::new();
    let line = room.notices.service.or(room.notices.fitted);
    let line_h = line.map_or(0.0, |l| notice_height(l, body.w) + 10.0);
    let room_for = ((body.h - line_h) / OFFER_ROW).floor().max(1.0) as usize;
    let (first, shown) = if offers.len() <= room_for {
        (0, offers.len())
    } else {
        let n = ((body.h - line_h - PAGER_H) / OFFER_ROW).floor().max(1.0) as usize;
        let first = room.ui.services_first.min(offers.len().saturating_sub(n));
        (first, n)
    };
    let mut y = body.y;
    let btn_w = 120.0;
    let text_w = body.w - btn_w - 16.0;
    for offer in offers.iter().skip(first).take(shown) {
        let title = if offer.owned {
            offer.name.clone()
        } else {
            format!("{}  ·  {}", offer.name, offer.price)
        };
        o.texts.push(
            Text::new(title, G2::new(body.x, y + 2.0), 17.0)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(offer.effect.as_str(), G2::new(body.x, y + 24.0), 14.0)
                .weight(500)
                .color(BLACK)
                .wrap(text_w),
        );
        if let Some(blocked) = offer.blocked.as_deref() {
            o.texts.push(
                Text::new(blocked, G2::new(body.x, y + 44.0), 13.0)
                    .weight(700)
                    .color(BLACK)
                    .wrap(text_w),
            );
        }
        let state = Rect::new(body.x + body.w - btn_w, y + 4.0, btn_w, 34.0);
        if offer.owned {
            super::owned_mark(o, state, &offer.request);
        } else {
            let (label, act) = match &offer.request {
                ServiceRequest::Prestige { to } => ("Trade up", UiAction::Prestige(to.clone())),
                ServiceRequest::Build { .. } => ("Build", UiAction::Service(offer.request.clone())),
                ServiceRequest::Fit { .. } if offer.price == "a gift" => {
                    ("Take it", UiAction::Service(offer.request.clone()))
                }
                ServiceRequest::Fit { .. } => ("Fit", UiAction::Service(offer.request.clone())),
            };
            buttons.push(action(
                o,
                room.ui,
                state,
                label,
                offer.blocked.is_none(),
                act,
            ));
        }
        y += OFFER_ROW;
    }
    if shown < offers.len() {
        let last = first + shown;
        o.texts.push(
            Text::new(
                format!("{}–{} of {}", first + 1, last, offers.len()),
                G2::new(body.x, y + 12.0),
                13.0,
            )
            .weight(600)
            .color(BLACK),
        );
        buttons.push(action(
            o,
            room.ui,
            Rect::new(body.x + body.w - 168.0, y + 4.0, 80.0, 30.0),
            "Up",
            first > 0,
            UiAction::ServicesFrom(first.saturating_sub(shown)),
        ));
        buttons.push(action(
            o,
            room.ui,
            Rect::new(body.x + body.w - 80.0, y + 4.0, 80.0, 30.0),
            "Down",
            last < offers.len(),
            UiAction::ServicesFrom(last),
        ));
        y += PAGER_H;
    }
    if let Some(l) = line {
        let color = if room.notices.service.is_some() {
            VERMILION
        } else {
            BLUE
        };
        y += 6.0;
        y += notice(o, l, color, body.x, y, body.w) + 4.0;
    }
    (buttons, y - body.y)
}
