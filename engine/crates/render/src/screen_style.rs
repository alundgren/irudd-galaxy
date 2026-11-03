//! Module-owned screen paint, validated before it reaches layout or drawing.
use crate::{Color, Panel, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenStyle {
    pub paper: Color,
    pub panel: Color,
    pub button: Color,
    pub text: Color,
    pub gain: Color,
    pub loss: Color,
    pub border: Color,
    pub bubble: Color,
    pub sign: Color,
    pub held: Color,
    pub socket: Color,
    pub accept: Color,
    pub refuse: Color,
    pub track: Color,
    pub handle: Color,
    pub handle_active: Color,
    /// Outline of a hovered `ui.target`'s model.
    pub highlight: Color,
    pub radius: f32,
    pub border_px: f32,
    pub frame_padding: f32,
    pub target_min: f32,
    pub handle_width: f32,
    pub handle_min: f32,
    pub handle_radius: f32,
    pub socket_dash: f32,
    pub hint_border_px: f32,
    /// Width of that outline, logical pixels.
    pub highlight_px: f32,
    /// Talk bubble text sizes in logical pixels: the speaker's heading, the quieter
    /// subtitle, the line (and its answer buttons), and the Continue/Done marker.
    pub bubble_title: f32,
    pub bubble_subtitle: f32,
    pub bubble_text: f32,
    pub bubble_marker: f32,
    /// The heading's font weight, 400 to 700.
    pub bubble_title_weight: f32,
}

impl Default for ScreenStyle {
    fn default() -> Self {
        Self {
            paper: Color::hex(0xf5f5f5),
            panel: Color::hex(0xeeeeee),
            button: Color::hex(0xdddddd),
            text: Color::hex(0x202020),
            gain: Color::hex(0x28552f),
            loss: Color::hex(0x872525),
            border: Color::hex(0x555555),
            bubble: Color::hex(0xf5f5f5),
            sign: Color::hex(0xf5f5f5),
            held: Color::hex(0xdddddd),
            socket: Color::hex(0xe4e4e4),
            accept: Color::hex(0xe2eee3),
            refuse: Color::hex(0xf2e4e4),
            track: Color::hex(0xdddddd),
            handle: Color::hex(0x666666),
            handle_active: Color::hex(0x444444),
            highlight: Color::hex(0x2f6fdb),
            radius: 8.0,
            border_px: 1.0,
            frame_padding: 16.0,
            target_min: 44.0,
            handle_width: 32.0,
            handle_min: 44.0,
            handle_radius: 8.0,
            socket_dash: 6.0,
            hint_border_px: 2.0,
            highlight_px: 6.0,
            bubble_title: 24.0,
            bubble_subtitle: 16.0,
            bubble_text: 20.0,
            bubble_marker: 16.0,
            bubble_title_weight: 650.0,
        }
    }
}

impl ScreenStyle {
    pub fn check(&self) -> Result<(), String> {
        for (field, value, lo, hi) in [
            ("radius", self.radius, 0.0, 24.0),
            ("border_px", self.border_px, 0.0, 4.0),
            ("frame_padding", self.frame_padding, 12.0, 32.0),
            ("target_min", self.target_min, 44.0, 64.0),
            ("handle_width", self.handle_width, 24.0, 48.0),
            ("handle_min", self.handle_min, 44.0, 80.0),
            ("handle_radius", self.handle_radius, 0.0, 24.0),
            ("socket_dash", self.socket_dash, 0.0, 16.0),
            ("hint_border_px", self.hint_border_px, 0.0, 4.0),
            ("highlight_px", self.highlight_px, 2.0, 12.0),
            ("bubble_title", self.bubble_title, 16.0, 40.0),
            ("bubble_subtitle", self.bubble_subtitle, 16.0, 32.0),
            ("bubble_text", self.bubble_text, 16.0, 32.0),
            ("bubble_marker", self.bubble_marker, 16.0, 32.0),
        ] {
            if !value.is_finite() || !(lo..=hi).contains(&value) {
                return Err(format!(
                    "[screen] {field} must be {lo} to {hi} logical pixels"
                ));
            }
        }
        if !self.bubble_title_weight.is_finite()
            || !(400.0..=700.0).contains(&self.bubble_title_weight)
        {
            return Err("[screen] bubble_title_weight must be 400 to 700".into());
        }
        let surfaces = [
            ("paper", self.paper),
            ("panel", self.panel),
            ("button", self.button),
            ("bubble", self.bubble),
            ("sign", self.sign),
            ("held", self.held),
            ("socket", self.socket),
            ("accept", self.accept),
            ("refuse", self.refuse),
        ];
        // Descendant text (including deltas) can sit on any containing surface.
        // Opaque colours make the guarantee independent of the world behind them.
        for (field, color) in surfaces.into_iter().chain([
            ("text", self.text),
            ("gain", self.gain),
            ("loss", self.loss),
            ("border", self.border),
            ("track", self.track),
            ("handle", self.handle),
            ("handle_active", self.handle_active),
            ("highlight", self.highlight),
        ]) {
            if color.a != 1.0 {
                return Err(format!("[screen] {field} must be opaque"));
            }
        }
        for (name, ink) in [
            ("text", self.text),
            ("gain", self.gain),
            ("loss", self.loss),
        ] {
            for (surface, color) in surfaces {
                if contrast(ink, color) < 4.5 {
                    return Err(format!(
                        "[screen] {name} on {surface} needs contrast at least 4.5:1"
                    ));
                }
            }
        }
        for (name, ink) in [
            ("handle", self.handle),
            ("handle_active", self.handle_active),
        ] {
            if contrast(ink, self.track) < 3.0 {
                return Err(format!(
                    "[screen] {name} on track needs contrast at least 3:1"
                ));
            }
        }
        Ok(())
    }

    pub fn surface(&self, rect: Rect, fill: Color) -> Panel {
        Panel {
            radius: self.radius,
            fill,
            border: self.border,
            border_px: self.border_px,
            // Screen surfaces have no renderer-owned tinted shadow.
            shadow: false,
            ..Panel::new(rect)
        }
    }
}

fn contrast(a: Color, b: Color) -> f32 {
    let luminance = |c: Color| 0.2126 * c.r + 0.7152 * c.g + 0.0722 * c.b;
    let (a, b) = (luminance(a), luminance(b));
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_and_all_text_surface_pairs_are_checked() {
        ScreenStyle::default().check().unwrap();
        let base = toml::Value::try_from(ScreenStyle::default()).unwrap();
        for surface in [
            "paper", "panel", "button", "bubble", "sign", "held", "socket", "accept", "refuse",
        ] {
            for ink in ["text", "gain", "loss"] {
                let mut value = base.clone();
                value[ink] = value[surface].clone();
                let style: ScreenStyle = value.try_into().unwrap();
                assert!(style.check().unwrap_err().contains(ink));
            }
        }
    }
}
