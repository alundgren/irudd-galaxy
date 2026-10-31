//! A general counter: the room for every kind without its own (berths, an archive, a
//! canteen ...). Its keeper's offers, or a plain line when they have none today.

use render::glam::Vec2 as G2;
use render::{Overlay, Rect, Text};

use super::{Room, offer_list, offers_of};
use crate::ui::{BLACK, Button};

pub fn draw(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    let offers = offers_of(room, &room.part.keeper);
    if offers.is_empty() && room.notices.service.or(room.notices.fitted).is_none() {
        o.texts.push(
            Text::new(
                "Nothing for sale today.",
                G2::new(body.x, body.y + 4.0),
                17.0,
            )
            .weight(600)
            .color(BLACK),
        );
        return Vec::new();
    }
    offer_list(o, room, &offers, body).0
}
