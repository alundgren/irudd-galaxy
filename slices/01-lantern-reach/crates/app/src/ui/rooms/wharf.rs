//! The wharf room: the hullwright's offers today, each with its price, what it does and
//! what stands in the way, and a "Fitted" state once it's yours, so the section never
//! vanishes after a purchase (#44).
//!
//! **Replace me.** This is the slice's simple wharf panel. The hullwright cradle view
//! (the Wren lifted into the cradle, drag to fit) takes over this room kind by
//! replacing [`draw`]: it gets the same [`Room`] and the room's body rect, and returns
//! its buttons. Everything around it (the frame, Back, the key, the camera glide to the
//! cradle arm) stays as it is.

use render::{Overlay, Rect};

use super::{Room, offer_list, offers_of};
use crate::ui::Button;

pub fn draw(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    let offers = offers_of(room, &room.part.keeper);
    if offers.is_empty() {
        o.texts.push(
            render::Text::new(
                "Nothing on the rack today.",
                render::glam::Vec2::new(body.x, body.y + 4.0),
                17.0,
            )
            .weight(600)
            .color(crate::ui::BLACK),
        );
        return Vec::new();
    }
    offer_list(o, room, &offers, body).0
}
