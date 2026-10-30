//! The dockmaster's office: ask what changed here and nearby (the answer comes as the
//! dockmaster's bubbles, rising from this office), the dockmaster's own offers, and any
//! offer at the station whose seller keeps no part of it, so nothing for sale goes
//! missing.

use render::glam::Vec2 as G2;
use render::{Overlay, Rect, Text};

use super::{Room, action, offer_list, offers_of, unhoused_offers};
use crate::ui::{BLACK, Button, UiAction};

pub fn draw(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    let mut buttons = Vec::new();
    let first = room
        .world
        .npcs
        .get(&room.part.keeper)
        .map_or("The dockmaster", |n| {
            n.name.split_whitespace().next().unwrap_or(&n.name)
        });
    buttons.push(action(
        o,
        room.ui,
        Rect::new(body.x, body.y + 2.0, 200.0, 36.0),
        "Ask what changed",
        true,
        UiAction::AskWhatChanged,
    ));
    o.texts.push(
        Text::new(
            format!("{first} tells you what changed here and nearby."),
            G2::new(body.x + 214.0, body.y + 11.0),
            14.0,
        )
        .weight(500)
        .color(BLACK)
        .wrap(body.w - 214.0),
    );
    let mut offers = offers_of(room, &room.part.keeper);
    for extra in unhoused_offers(room) {
        if !offers.contains(&extra) {
            offers.push(extra);
        }
    }
    let r = Rect::new(body.x, body.y + 56.0, body.w, (body.h - 56.0).max(0.0));
    buttons.extend(offer_list(o, room, &offers, r).0);
    buttons
}
