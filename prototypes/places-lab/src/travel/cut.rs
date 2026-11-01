//! A cut with a title card: the view dips to a card naming where you are going, and
//! comes up on the new place.

use render::glam::Vec2;
use render::{Align, Color, Panel, Rect, Text, UiLayer, palette};

use super::Travel;
use crate::stage::{Shown, Stage};

/// Fading out of the old place, holding the card, fading into the new one (seconds).
const OUT: f32 = 0.18;
const HOLD: f32 = 0.6;
const IN: f32 = 0.22;

#[derive(Default)]
pub struct Cut {
    from: String,
    to: String,
    t: f32,
    busy: bool,
}

impl Travel for Cut {
    fn name(&self) -> &'static str {
        "cut"
    }

    fn start(&mut self, from: &str, to: &str) {
        self.from = from.to_string();
        self.to = to.to_string();
        self.t = 0.0;
        self.busy = true;
    }

    fn step(&mut self, dt: f32) {
        if self.busy {
            self.t += dt;
            if self.t >= OUT + HOLD + IN {
                self.busy = false;
            }
        }
    }

    fn busy(&self) -> bool {
        self.busy
    }

    fn finish(&mut self) {
        self.busy = false;
    }

    fn compose(&self, stage: &Stage) -> Shown {
        if self.busy && self.t < OUT {
            stage.view(&self.from)
        } else {
            stage.view(&stage.model.here)
        }
    }

    fn overlay(&self, stage: &Stage, layer: &mut UiLayer) {
        if !self.busy {
            return;
        }
        let a = if self.t < OUT {
            self.t / OUT
        } else if self.t < OUT + HOLD {
            1.0
        } else {
            1.0 - (self.t - OUT - HOLD) / IN
        }
        .clamp(0.0, 1.0);
        let v = stage.viewport;
        layer.panels.push(Panel {
            fill: Color::hex(0x1b1424).with_alpha(a),
            border: Color::TRANSPARENT,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(-4.0, -4.0, v.x + 8.0, v.y + 8.0))
        });
        // The card: the place's name over where it is.
        let (name, trail) = stage.title(&self.to);
        let text_a = ((a - 0.5) * 2.0).clamp(0.0, 1.0);
        let mid = v * 0.5;
        layer.panels.push(Panel {
            fill: palette::MUSTARD.with_alpha(text_a),
            border: Color::TRANSPARENT,
            border_px: 0.0,
            radius: 2.0,
            shadow: false,
            ..Panel::new(Rect::new(mid.x - 60.0, mid.y + 6.0, 120.0, 4.0))
        });
        layer.texts.push(
            Text::new(name, Vec2::new(mid.x, mid.y - 44.0), 40.0)
                .weight(700)
                .align(Align::Center)
                .color(Color::WHITE.with_alpha(text_a)),
        );
        layer.texts.push(
            Text::new(trail, Vec2::new(mid.x, mid.y + 20.0), 18.0)
                .weight(500)
                .align(Align::Center)
                .color(palette::CREAM.with_alpha(text_a)),
        );
    }
}
