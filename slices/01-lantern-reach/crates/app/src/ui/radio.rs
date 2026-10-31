//! The radio on the flying HUD (#47, 5.1), bottom right: the channel playing, its
//! signal as bars and a word (never colour alone), its text bits as a ticker, and
//! Prev, Off and Next buttons. R does Next (past the last channel, Off). Mouse first:
//! everything is a button. Hidden while docked and on the map.

use render::glam::Vec2 as G2;
use render::{Align, Overlay, Panel, Rect, Text, palette};

use super::talk::SKY;
use super::{Button, MARGIN, UiAction, UiState, WHITE, button, clip};
use crate::radio::{RadioUi, Tune};

/// The widget's size on the canvas.
pub const WIDTH: f32 = 300.0;
pub const HEIGHT: f32 = 150.0;

/// Seconds each text bit shows before the next (view only).
pub const BIT_SECONDS: f32 = 8.0;

const PAD: f32 = 14.0;
const BIT_SIZE: f32 = 14.0;

/// Where the radio goes: the bottom-right corner.
pub fn rect(canvas: G2) -> Rect {
    Rect::new(
        canvas.x - WIDTH - MARGIN,
        canvas.y - HEIGHT - MARGIN,
        WIDTH,
        HEIGHT,
    )
}

/// The signal in words.
pub fn strength(signal: f32) -> &'static str {
    if signal >= 0.7 {
        "Strong"
    } else if signal >= 0.35 {
        "Fair"
    } else if signal > 0.0 {
        "Faint"
    } else {
        "None"
    }
}

/// How many of the four bars are lit.
pub fn bars(signal: f32) -> u32 {
    if signal <= 0.0 {
        0
    } else {
        ((signal * 4.0).ceil() as u32).clamp(1, 4)
    }
}

/// The bit showing at `time` (seconds), rotating through `bits`.
pub fn bit_at(bits: &[String], time: f32) -> Option<&str> {
    if bits.is_empty() {
        return None;
    }
    let i = (time.max(0.0) / BIT_SECONDS) as usize % bits.len();
    Some(bits[i].as_str())
}

/// A button on the radio's dark panel: the HUD's button when it can be pressed; when
/// it can't, a thin white outline with white text (never greyed out). A press on it
/// does nothing.
fn radio_button(
    o: &mut Overlay,
    ui: &UiState,
    rect: Rect,
    label: &str,
    enabled: bool,
    action: UiAction,
) -> Option<Button> {
    if enabled {
        return Some(button(o, ui, rect, label, true, action));
    }
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.0),
        border: WHITE,
        border_px: 1.0,
        radius: 8.0,
        shadow: false,
        ..Panel::new(rect)
    });
    o.texts.push(
        Text::new(
            label,
            G2::new(rect.center().x, rect.y + rect.h * 0.5 - 9.0),
            15.0,
        )
        .weight(500)
        .color(WHITE)
        .align(Align::Center),
    );
    None
}

/// Draws the radio and returns its buttons. Nothing unless `radio.show`.
pub fn draw(o: &mut Overlay, ui: &UiState, radio: &RadioUi, time: f32, canvas: G2) -> Vec<Button> {
    if !radio.show {
        return Vec::new();
    }
    let r = rect(canvas);
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.88),
        border: palette::MUSTARD,
        ..Panel::new(r)
    });
    let x = r.x + PAD;
    let inner = r.w - 2.0 * PAD;
    let y = r.y + 12.0;
    o.texts.push(
        Text::new("RADIO", G2::new(x, y + 2.0), 12.0)
            .weight(700)
            .color(WHITE),
    );
    let signal = if radio.on && radio.channel.is_some() {
        radio.signal
    } else {
        0.0
    };
    // Four bars rising, lit ones filled, the rest outlined; then the word.
    let word = if radio.on { strength(signal) } else { "Off" };
    let word_w = super::text_width(word, 14.0);
    let word_x = r.x + r.w - PAD - word_w;
    let lit = bars(signal);
    for i in 0..4u32 {
        let h = 6.0 + 3.0 * i as f32;
        let bx = word_x - 8.0 - (4 - i) as f32 * 9.0;
        let on = i < lit;
        o.panels.push(Panel {
            fill: if on { SKY } else { SKY.with_alpha(0.0) },
            border: SKY,
            border_px: 1.5,
            radius: 1.0,
            shadow: false,
            ..Panel::new(Rect::new(bx, y + 16.0 - h, 6.0, h))
        });
    }
    o.texts.push(
        Text::new(word, G2::new(word_x, y), 14.0)
            .weight(600)
            .color(WHITE),
    );
    let name = match (&radio.channel, radio.on) {
        (_, false) => "Radio off".to_owned(),
        (Some(n), true) => n.clone(),
        (None, true) => "No signal".to_owned(),
    };
    let name_y = y + 22.0;
    o.texts.push(
        Text::new(clip(&name, 17.0, inner), G2::new(x, name_y), 17.0)
            .weight(700)
            .color(WHITE),
    );
    let ticker = match (&radio.channel, radio.on) {
        (Some(_), true) => bit_at(&radio.bits, time).map(str::to_owned),
        (None, true) => Some("Fly near a station or a lane beacon to pick up its radio.".into()),
        (_, false) => Some("R or On: the radio comes back on.".into()),
    };
    if let Some(t) = ticker {
        // At most two rows: cut to fit.
        let fit = clip(&t, BIT_SIZE, inner * 2.0 - BIT_SIZE);
        o.texts.push(
            Text::new(fit, G2::new(x, name_y + 26.0), BIT_SIZE)
                .weight(500)
                .color(WHITE)
                .wrap(inner),
        );
    }
    let bw = (inner - 2.0 * 8.0) / 3.0;
    let by = r.y + r.h - 12.0 - 32.0;
    let many = radio.on && radio.others > 0;
    // The panel's background takes a click that misses the buttons.
    let mut out = vec![Button {
        rect: r,
        action: UiAction::Absorb,
    }];
    out.extend(radio_button(
        o,
        ui,
        Rect::new(x, by, bw, 32.0),
        "Prev",
        many,
        UiAction::Radio(Tune::Prev),
    ));
    out.extend(radio_button(
        o,
        ui,
        Rect::new(x + bw + 8.0, by, bw, 32.0),
        if radio.on { "Off" } else { "On" },
        true,
        UiAction::Radio(Tune::Toggle),
    ));
    out.extend(radio_button(
        o,
        ui,
        Rect::new(x + 2.0 * (bw + 8.0), by, bw, 32.0),
        "Next · R",
        many,
        UiAction::Radio(Tune::Next),
    ));
    out
}
