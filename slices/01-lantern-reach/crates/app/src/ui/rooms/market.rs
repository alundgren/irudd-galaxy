//! The market room: the station's market lines as a stall. Each ware is drawn as goods
//! beside its name, its price hangs on a tag, and the stock and your hold sit under it.
//! Every line goes one way (the station buys it, so you sell; or it sells, so you buy),
//! in ones or as much as makes sense. A button that can't be used is a dashed outline.
//! Rows that don't fit page with the selection, and the latest refusal shows under them.

use galaxy_contract::prelude::*;
use render::glam::Vec2 as G2;
use render::{Align, Color, Overlay, Panel, Rect, Text, palette};

use super::{Room, action, notice, notice_height};
use crate::trades::credits;
use crate::ui::station::VERMILION;
use crate::ui::{Amount, BLACK, Button, UiAction, dock_lines, visible_rows};

/// One market row's height.
const ROW: f32 = 58.0;
const PAGER_H: f32 = 34.0;

/// The goods a ware is drawn as, in a 44 x 36 box at `at`: blocks of ice, lumps of ore,
/// a stack of plate, glassite shards, a ration crate, or a plain crate.
fn goods(o: &mut Overlay, ware: &WareId, at: G2) {
    let mut piece = |x: f32, y: f32, w: f32, h: f32, radius: f32, fill: Color| {
        o.panels.push(Panel {
            fill,
            border: palette::INK,
            border_px: 2.0,
            radius,
            shadow: false,
            ..Panel::new(Rect::new(at.x + x, at.y + y, w, h))
        });
    };
    match ware.as_str() {
        "ware.ice" => {
            let ice = Color::hex(0xc9ecf5);
            piece(2.0, 16.0, 18.0, 18.0, 3.0, ice);
            piece(22.0, 16.0, 18.0, 18.0, 3.0, ice);
            piece(12.0, 0.0, 18.0, 18.0, 3.0, ice);
        }
        "ware.iron_ore" => {
            let ore = Color::hex(0x9a5a3a);
            piece(0.0, 14.0, 22.0, 20.0, 10.0, ore);
            piece(18.0, 8.0, 24.0, 26.0, 12.0, ore);
        }
        "ware.refined_plate" => {
            let plate = Color::hex(0xd9cfb8);
            for i in 0..3 {
                piece(i as f32 * 3.0, 26.0 - i as f32 * 9.0, 38.0, 8.0, 2.0, plate);
            }
        }
        "ware.glassite" => {
            piece(4.0, 12.0, 14.0, 24.0, 7.0, palette::TEAL);
            piece(16.0, 2.0, 14.0, 32.0, 7.0, palette::TEAL);
            piece(28.0, 14.0, 12.0, 20.0, 6.0, palette::TEAL);
        }
        "ware.rations" => {
            piece(2.0, 6.0, 40.0, 28.0, 4.0, palette::MUSTARD);
            piece(2.0, 17.0, 40.0, 6.0, 0.0, palette::TERRACOTTA);
        }
        _ => {
            piece(4.0, 6.0, 36.0, 28.0, 4.0, palette::CREAM);
            piece(4.0, 6.0, 36.0, 8.0, 2.0, palette::MUSTARD);
        }
    }
}

/// A price tag: the price in black on a mustard tag with a hole, at `at`.
fn tag(o: &mut Overlay, text: &str, at: G2, w: f32) {
    let r = Rect::new(at.x, at.y, w, 30.0);
    o.panels.push(Panel {
        fill: palette::MUSTARD,
        border: palette::INK,
        border_px: 2.0,
        radius: 6.0,
        shadow: false,
        ..Panel::new(r)
    });
    o.panels.push(Panel {
        fill: palette::PARCHMENT,
        border: palette::INK,
        border_px: 1.5,
        radius: 3.5,
        shadow: false,
        ..Panel::new(Rect::new(r.x + 6.0, r.y + 11.5, 7.0, 7.0))
    });
    o.texts.push(
        Text::new(
            text,
            G2::new(r.x + 18.0 + (w - 18.0) * 0.5, r.y + 6.0),
            15.0,
        )
        .weight(700)
        .color(BLACK)
        .align(Align::Center),
    );
}

pub fn draw(o: &mut Overlay, room: &Room<'_>, body: Rect) -> Vec<Button> {
    let mut buttons = Vec::new();
    let lines = dock_lines(room.world, room.station);
    if lines.is_empty() {
        o.texts.push(
            Text::new(
                "Nothing is traded here.",
                G2::new(body.x, body.y + 4.0),
                17.0,
            )
            .weight(600)
            .color(BLACK),
        );
        return buttons;
    }
    let notice_h = room
        .notices
        .trade
        .map_or(0.0, |n| notice_height(n, body.w) + 10.0);
    let all = ((body.h - notice_h) / ROW).floor() as usize;
    let fits = if lines.len() <= all {
        lines.len()
    } else {
        ((body.h - notice_h - PAGER_H) / ROW).floor().max(1.0) as usize
    };
    let (first, shown) = visible_rows(lines.len(), room.ui.selected, fits);
    let mut y = body.y;
    let (one_w, most_w) = (72.0, 86.0);
    let col_btn = body.x + body.w - most_w - 8.0 - one_w;
    let col_tag = body.x + 210.0;
    let tag_w = (col_btn - col_tag - 12.0).clamp(96.0, 150.0);
    for (i, line) in lines.iter().enumerate().skip(first).take(shown) {
        let selected = i == room.ui.selected.min(lines.len() - 1);
        let row = Rect::new(body.x - 10.0, y, body.w + 20.0, ROW - 6.0);
        if selected {
            o.panels.push(Panel {
                fill: palette::MUSTARD.with_alpha(0.28),
                border: palette::MUSTARD,
                border_px: 1.5,
                radius: 9.0,
                shadow: false,
                ..Panel::new(row)
            });
        }
        buttons.push(Button {
            rect: row,
            action: UiAction::Select(i),
        });
        goods(o, &line.ware, G2::new(body.x, y + 8.0));
        o.texts.push(
            Text::new(line.name.clone(), G2::new(body.x + 54.0, y + 6.0), 18.0)
                .weight(700)
                .color(BLACK),
        );
        o.texts.push(
            Text::new(
                format!("Stock {}  ·  Hold {}", line.stock, line.in_hold),
                G2::new(body.x + 54.0, y + 29.0),
                14.0,
            )
            .weight(500)
            .color(BLACK),
        );
        let verb = match line.side {
            TradeSide::Sell => "pays",
            TradeSide::Buy => "asks",
        };
        let price = line.unit.map_or_else(
            || "none left".to_owned(),
            |c| format!("{verb} {}", credits(i64::from(c))),
        );
        tag(o, &price, G2::new(col_tag, y + 10.0), tag_w);
        let can = match line.side {
            TradeSide::Sell => line.in_hold > 0 && line.unit.is_some(),
            TradeSide::Buy => line.stock > 0 && line.unit.is_some(),
        };
        let (one, most) = match line.side {
            TradeSide::Sell => ("Sell 1", "Sell all"),
            TradeSide::Buy => ("Buy 1", "Buy max"),
        };
        buttons.push(action(
            o,
            room.ui,
            Rect::new(col_btn, y + 9.0, one_w, 32.0),
            one,
            can,
            UiAction::Trade {
                line: i,
                amount: Amount::One,
            },
        ));
        buttons.push(action(
            o,
            room.ui,
            Rect::new(col_btn + one_w + 8.0, y + 9.0, most_w, 32.0),
            most,
            can,
            UiAction::Trade {
                line: i,
                amount: Amount::Most,
            },
        ));
        y += ROW;
    }
    if shown < lines.len() {
        let last = first + shown;
        o.texts.push(
            Text::new(
                format!("{}–{} of {}", first + 1, last, lines.len()),
                G2::new(body.x, y + 8.0),
                13.0,
            )
            .weight(600)
            .color(BLACK),
        );
        buttons.push(action(
            o,
            room.ui,
            Rect::new(body.x + body.w - 140.0, y + 2.0, 64.0, 26.0),
            "Up",
            first > 0,
            UiAction::Select(first.saturating_sub(1)),
        ));
        buttons.push(action(
            o,
            room.ui,
            Rect::new(body.x + body.w - 64.0, y + 2.0, 64.0, 26.0),
            "Down",
            last < lines.len(),
            UiAction::Select((last + shown - 1).min(lines.len() - 1)),
        ));
        y += PAGER_H;
    }
    if let Some(n) = room.notices.trade {
        notice(o, n, VERMILION, body.x, y + 6.0, body.w);
    }
    buttons
}
