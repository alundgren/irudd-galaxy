//! Module-owned screen paint, validated before it reaches layout or drawing.
use std::collections::BTreeMap;

use crate::{Color, Panel, Rect};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
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
    /// Fill of a resting button (one that can't act now). Unset, it is `panel`.
    pub resting: Option<Color>,
    pub radius: f32,
    pub border_px: f32,
    pub frame_padding: f32,
    pub target_min: f32,
    pub handle_width: f32,
    pub handle_min: f32,
    pub handle_radius: f32,
    pub socket_dash: f32,
    /// Dash period of a resting button's frame; zero is solid.
    pub resting_dash: f32,
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
    /// A button's fill under the pointer. Unset, it keeps its fill.
    pub button_hover: Option<Color>,
    /// Looks for the talk bubble, its heading (the speaker's name) and life signs, by
    /// name from `looks`. Empty, they use `bubble`, `text` and `sign`.
    pub bubble_look: String,
    pub bubble_title_look: String,
    pub sign_look: String,
    /// UI motion, presentation only. Seconds a panel or bubble takes to come in and to
    /// go; how far it slides while it does (logical pixels; 0 fades only).
    pub motion_in: f32,
    pub motion_out: f32,
    pub motion_slide: f32,
    /// How far a button rises under the pointer and sinks while pressed.
    pub hover_lift: f32,
    pub press_sink: f32,
    /// Seconds a value cue (`cue = "count"` or `"flash"`) runs, and the flash's fill.
    pub cue_time: f32,
    pub flash: Color,
    /// Named looks a widget or text span picks with `look = "<name>"`.
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub looks: BTreeMap<String, ScreenLook>,
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
            resting: None,
            radius: 8.0,
            border_px: 1.0,
            frame_padding: 16.0,
            target_min: 44.0,
            handle_width: 32.0,
            handle_min: 44.0,
            handle_radius: 8.0,
            socket_dash: 6.0,
            resting_dash: 6.0,
            hint_border_px: 2.0,
            highlight_px: 6.0,
            bubble_title: 24.0,
            bubble_subtitle: 16.0,
            bubble_text: 20.0,
            bubble_marker: 16.0,
            bubble_title_weight: 650.0,
            button_hover: None,
            bubble_look: String::new(),
            bubble_title_look: String::new(),
            sign_look: String::new(),
            motion_in: 0.0,
            motion_out: 0.0,
            motion_slide: 0.0,
            hover_lift: 0.0,
            press_sink: 0.0,
            cue_time: 0.0,
            flash: Color::hex(0xfff0b0),
            looks: BTreeMap::new(),
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
            ("resting_dash", self.resting_dash, 0.0, 16.0),
            ("hint_border_px", self.hint_border_px, 0.0, 4.0),
            ("highlight_px", self.highlight_px, 2.0, 12.0),
            ("bubble_title", self.bubble_title, 16.0, 40.0),
            ("bubble_subtitle", self.bubble_subtitle, 16.0, 32.0),
            ("bubble_text", self.bubble_text, 16.0, 32.0),
            ("bubble_marker", self.bubble_marker, 16.0, 32.0),
            ("motion_slide", self.motion_slide, 0.0, 48.0),
            ("hover_lift", self.hover_lift, 0.0, 6.0),
            ("press_sink", self.press_sink, 0.0, 6.0),
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
        for (field, value, hi) in [
            ("motion_in", self.motion_in, 1.0),
            ("motion_out", self.motion_out, 1.0),
            ("cue_time", self.cue_time, 2.0),
        ] {
            if !value.is_finite() || !(0.0..=hi).contains(&value) {
                return Err(format!("[screen] {field} must be 0 to {hi} seconds"));
            }
        }
        for (field, name) in [
            ("bubble_look", &self.bubble_look),
            ("bubble_title_look", &self.bubble_title_look),
            ("sign_look", &self.sign_look),
        ] {
            if !name.is_empty() && !self.looks.contains_key(name) {
                return Err(format!(
                    "[screen] {field} names `{name}`, which [screen.looks] doesn't have"
                ));
            }
        }
        for (name, look) in &self.looks {
            if name.trim().is_empty() {
                return Err("[screen.looks] names a look with an empty name".into());
            }
            look.check(name, self)?;
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
            ("resting", self.resting_fill()),
            ("button_hover", self.button_hover.unwrap_or(self.button)),
            // Only cues draw a flash: with them off, the flash colour is never seen.
            (
                "flash",
                if self.cue_time > 0.0 {
                    self.flash
                } else {
                    self.paper
                },
            ),
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
            ("flash", self.flash),
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
        // A resting button must look unlike a ready one: by its dashed frame, which then
        // has to show against its fill, or by its fill alone.
        if self.resting_dash > 0.0 {
            if contrast(self.border, self.resting_fill()) < 3.0 {
                return Err(
                    "[screen] border on resting needs contrast at least 3:1 to show its dashes"
                        .into(),
                );
            }
        } else if self.resting_fill() == self.button {
            return Err(
                "[screen] resting must differ from button, or set resting_dash above 0".into(),
            );
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

    /// The fill a resting button is drawn with.
    pub fn resting_fill(&self) -> Color {
        self.resting.unwrap_or(self.panel)
    }

    /// The look a widget or span names, if the style has it.
    pub fn look(&self, name: Option<&str>) -> Option<&ScreenLook> {
        name.filter(|n| !n.is_empty())
            .and_then(|n| self.looks.get(n))
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

/// A named look in `[screen.looks.<name>]`. A widget wearing it takes its surface and
/// text; a text span wearing it takes its text, weight, underline and (as a tag behind
/// the words) its fill. Unset keys keep what the widget or span would have had.
#[derive(Clone, Debug, Default, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScreenLook {
    pub fill: Option<Color>,
    pub text: Option<Color>,
    pub border: Option<Color>,
    pub border_px: Option<f32>,
    pub radius: Option<f32>,
    /// Default padding of a widget wearing it; explicit widget `pad` wins.
    pub pad: Option<f32>,
    /// Drop shadow under its surface, logical pixels; 0 or unset has none.
    pub shadow: Option<f32>,
    /// Fill of a button wearing it under the pointer.
    pub hover: Option<Color>,
    /// Font weight, 300 to 700.
    pub weight: Option<f32>,
    /// A line under its text.
    pub underline: Option<Color>,
    /// Signed values (`ui.delta`) wearing it. Unset, they use its text colour.
    pub gain: Option<Color>,
    pub loss: Option<Color>,
    /// Fill behind a flashing value wearing it. Unset, it is `[screen] flash`.
    pub flash: Option<Color>,
    /// A mark, not words: a span of symbols (an arrow or a dot) that needs 3:1 contrast
    /// like any graphic. Letters and digits in a mark span are a screen error.
    pub mark: bool,
}

impl ScreenLook {
    fn check(&self, name: &str, base: &ScreenStyle) -> Result<(), String> {
        let at = |key: &str| format!("[screen.looks.{name}] {key}");
        for (key, value, lo, hi) in [
            ("border_px", self.border_px, 0.0, 4.0),
            ("radius", self.radius, 0.0, 24.0),
            ("pad", self.pad, 0.0, 32.0),
            ("shadow", self.shadow, 0.0, 16.0),
            ("weight", self.weight, 300.0, 700.0),
        ] {
            if let Some(v) = value
                && (!v.is_finite() || !(lo..=hi).contains(&v))
            {
                return Err(format!("{} must be {lo} to {hi}", at(key)));
            }
        }
        for (key, c) in [
            ("fill", self.fill),
            ("text", self.text),
            ("border", self.border),
            ("hover", self.hover),
            ("underline", self.underline),
            ("gain", self.gain),
            ("loss", self.loss),
            ("flash", self.flash),
        ] {
            if c.is_some_and(|c| c.a != 1.0) {
                return Err(format!("{} must be opaque", at(key)));
            }
        }
        let need = if self.mark { 3.0 } else { 4.5 };
        let text = self.text.unwrap_or(base.text);
        let inks = [
            ("text", text),
            ("gain", self.gain.unwrap_or(text)),
            ("loss", self.loss.unwrap_or(text)),
        ];
        // Without a fill the look sits on whatever surface is behind it: play and check
        // test that pair on the drawn screen.
        let surfaces = [
            ("fill", self.fill),
            ("hover", self.hover),
            ("flash", self.flash.filter(|_| base.cue_time > 0.0)),
        ];
        for (surface, fill) in surfaces {
            let Some(fill) = fill else { continue };
            for (ink, colour) in inks {
                if contrast(colour, fill) < need {
                    return Err(format!(
                        "{} on {surface} needs contrast at least {need}:1",
                        at(ink)
                    ));
                }
            }
            if let Some(u) = self.underline
                && surface == "fill"
                && contrast(u, fill) < 3.0
            {
                return Err(format!(
                    "{} on fill needs contrast at least 3:1",
                    at("underline")
                ));
            }
        }
        Ok(())
    }

    pub fn weight(&self) -> Option<u16> {
        self.weight.map(|w| w.round() as u16)
    }
}

/// Contrast between two colours by relative luminance (linear sRGB), 1 to 21.
pub fn contrast(a: Color, b: Color) -> f32 {
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
            "resting",
        ] {
            for ink in ["text", "gain", "loss"] {
                let mut value = base.clone();
                let table = value.as_table_mut().unwrap();
                // Resting is unset by default: give it the ink's colour instead, which
                // the text ink (checked first) may fail on too.
                let expected = if surface == "resting" {
                    let ink_colour = table[ink].clone();
                    table.insert(surface.into(), ink_colour);
                    "on resting"
                } else {
                    let fill = table[surface].clone();
                    table.insert(ink.into(), fill);
                    ink
                };
                let style: ScreenStyle = value.try_into().unwrap();
                assert!(style.check().unwrap_err().contains(expected));
            }
        }
    }

    #[test]
    fn a_resting_button_must_look_unlike_a_ready_one() {
        let ok = ScreenStyle {
            resting: Some(Color::hex(0xffffff)),
            ..ScreenStyle::default()
        };
        ok.check().unwrap();
        let faint = ScreenStyle {
            border: Color::hex(0xdddddd),
            ..ok.clone()
        };
        assert!(faint.check().unwrap_err().contains("border on resting"));
        let same = ScreenStyle {
            resting: Some(ok.button),
            resting_dash: 0.0,
            ..ok.clone()
        };
        assert!(same.check().unwrap_err().contains("resting must differ"));
        // Solid, but its own fill: still resting.
        ScreenStyle {
            resting_dash: 0.0,
            ..ok
        }
        .check()
        .unwrap();
    }
}

#[cfg(test)]
mod look_tests {
    use super::*;
    use crate::Style;

    fn screen(text: &str) -> Result<ScreenStyle, String> {
        Style::parse(text).map(|s| s.screen.unwrap_or_default())
    }

    #[test]
    fn named_looks_are_read_and_each_is_checked_against_its_own_fill() {
        let s = screen(
            "[screen]\nbubble_title_look = 'name'\n\
             [screen.looks.chip]\nfill = '#1f1a24'\ntext = '#ffffff'\nshadow = 6.0\n\
             [screen.looks.name]\nweight = 700.0\nunderline = '#b07800'\n",
        )
        .unwrap();
        assert_eq!(s.looks.len(), 2);
        assert_eq!(
            s.look(Some("chip")).unwrap().text,
            Some(Color::hex(0xffffff))
        );
        // Dark text on the dark chip fails; so does a mark too faint even for 3:1.
        let e = screen("[screen.looks.chip]\nfill = '#1f1a24'\ntext = '#333333'\n").unwrap_err();
        assert!(e.contains("[screen.looks.chip] text on fill"), "{e}");
        let e = screen("[screen.looks.m]\nfill = '#ffffff'\ntext = '#dddddd'\nmark = true\n")
            .unwrap_err();
        assert!(e.contains("3:1"), "{e}");
        screen("[screen.looks.m]\nfill = '#f7e8c8'\ntext = '#0072b2'\nmark = true\n").unwrap();
        let e = screen("[screen.looks.c]\nfill = '#000000'\ntext = '#ffffff'\nhover = '#eeeeee'\n")
            .unwrap_err();
        assert!(e.contains("on hover"), "{e}");
        let e = screen("[screen]\nbubble_look = 'absent'\n").unwrap_err();
        assert!(e.contains("bubble_look names `absent`"), "{e}");
        let e = screen("[screen.looks.c]\nradius = 30.0\n").unwrap_err();
        assert!(e.contains("radius must be 0 to 24"), "{e}");
    }

    #[test]
    fn motion_keys_are_ranged_and_the_flash_is_checked_only_with_cues_on() {
        assert!(
            screen("[screen]\nmotion_in = 2.0")
                .unwrap_err()
                .contains("motion_in")
        );
        assert!(
            screen("[screen]\nhover_lift = 9.0")
                .unwrap_err()
                .contains("hover_lift")
        );
        // A dark flash under dark text is fine until cues run.
        screen("[screen]\nflash = '#202020'").unwrap();
        let e = screen("[screen]\nflash = '#202020'\ncue_time = 0.5").unwrap_err();
        assert!(e.contains("text on flash"), "{e}");
    }
}
