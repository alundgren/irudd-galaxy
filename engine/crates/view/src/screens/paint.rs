//! Which surface a widget paints and which ink its words use: from the widget's kind,
//! its look and what it sits on. Drawing and the readability check share this, so the
//! contrast check tests exactly the colours a player sees.
use galaxy_render::{Color, screen_style::contrast};

use super::*;

/// The colours text inside a surface uses, and that surface.
#[derive(Clone, Copy, Debug)]
pub(super) struct Ink {
    pub surface: Color,
    pub text: Color,
    pub gain: Color,
    pub loss: Color,
    pub flash: Color,
    /// The painted box this ink belongs to; text outside it gets its own backing.
    pub rect: Option<Rect>,
}

impl Ink {
    pub fn base(style: &ScreenStyle) -> Self {
        Ink {
            surface: style.paper,
            text: style.text,
            gain: style.gain,
            loss: style.loss,
            flash: style.flash,
            rect: None,
        }
    }

    /// The colour of a widget's own words.
    pub fn words(&self, w: &Widget) -> Color {
        match (w.kind.as_str(), w.delta < 0.0) {
            ("delta", true) => self.loss,
            ("delta", false) => self.gain,
            _ => self.text,
        }
    }
}

/// A surface a widget paints under itself.
#[derive(Clone, Copy, Debug)]
pub(super) struct Surface {
    pub fill: Color,
    pub border: Color,
    pub border_px: f32,
    pub radius: f32,
    pub shadow: f32,
    /// A flat backing for loose text (no frame, no corners).
    pub flat: bool,
    /// A tag: a text's look with a fill, hugging its words.
    pub tag: bool,
}

pub(super) fn frame_kind(kind: &str) -> bool {
    matches!(
        kind,
        "panel" | "button" | "bubble" | "scroll" | "drag_source" | "drop_target"
    )
}

/// The surface `b` paints, if any, and the ink for its words and children.
pub(super) fn resolve(b: &Boxed, parent: Ink, style: &ScreenStyle) -> (Option<Surface>, Ink) {
    let look = style.look(b.widget.look.as_deref());
    let kind = b.widget.kind.as_str();
    let worded = b.text.is_some() || b.icon.is_some();
    let tag = !frame_kind(kind) && worded && look.is_some_and(|l| l.fill.is_some());
    let own = |fill: Color, flat: bool| {
        let text = look.and_then(|l| l.text).unwrap_or(style.text);
        let gain = look.and_then(|l| l.gain.or(l.text)).unwrap_or(style.gain);
        let loss = look.and_then(|l| l.loss.or(l.text)).unwrap_or(style.loss);
        let surface = Surface {
            fill,
            border: look.and_then(|l| l.border).unwrap_or(style.border),
            border_px: if flat {
                0.0
            } else {
                look.and_then(|l| l.border_px).unwrap_or(style.border_px)
            },
            radius: if flat {
                0.0
            } else {
                look.and_then(|l| l.radius).unwrap_or(style.radius)
            },
            shadow: look.and_then(|l| l.shadow).unwrap_or(0.0),
            flat,
            tag,
        };
        let ink = Ink {
            surface: fill,
            text,
            gain,
            loss,
            flash: look.and_then(|l| l.flash).unwrap_or(style.flash),
            rect: Some(b.rect),
        };
        (Some(surface), ink)
    };
    let base_fill = match kind {
        "bubble" => style.bubble,
        "button" | "drag_source" => style.button,
        "sign" => style.sign,
        "panel" | "scroll" | "drop_target" => style.panel,
        _ => style.paper,
    };
    if frame_kind(kind) || tag {
        return own(look.and_then(|l| l.fill).unwrap_or(base_fill), false);
    }
    if worded && parent.rect.is_none_or(|r| !contains(r, b.rect)) {
        // Loose text gets a flat paper surface covering every glyph.
        return own(base_fill, true);
    }
    let mut ink = parent;
    if let Some(l) = look {
        ink.text = l.text.unwrap_or(ink.text);
        ink.gain = l.gain.or(l.text).unwrap_or(ink.gain);
        ink.loss = l.loss.or(l.text).unwrap_or(ink.loss);
        ink.flash = l.flash.unwrap_or(ink.flash);
    }
    (None, ink)
}

/// Each span with a look: its byte range in the label and that look's name.
pub(super) fn span_ranges(w: &Widget) -> Vec<(std::ops::Range<usize>, String)> {
    let mut at = 0;
    let mut out = Vec::new();
    for s in &w.spans {
        let range = at..at + s.text.len();
        at = range.end;
        if let Some(look) = &s.look {
            out.push((range, look.clone()));
        }
    }
    out
}

/// Readability of the colours `b` is drawn in: its words on its surface, each span
/// (and its tag or underline), a tinted icon and a flash. Unknown looks are named.
pub(super) fn check(b: &Boxed, ink: Ink, style: &ScreenStyle, problems: &mut Vec<String>) {
    let path = &b.widget.path;
    let mut problem = |m: String| problems.push(format!("{path}: {m}"));
    if let Some(name) = &b.widget.look
        && !style.looks.contains_key(name)
    {
        problem(format!("look `{name}` is not in [screen.looks]"));
    }
    let named = |name: &Option<String>| {
        name.as_ref()
            .map_or(String::new(), |n| format!(" (look `{n}`)"))
    };
    if let Some(t) = &b.text {
        let words = ink.words(&b.widget);
        if !t.text.trim().is_empty() && contrast(words, ink.surface) < 4.5 {
            problem(format!(
                "text on its surface needs contrast at least 4.5:1{}",
                named(&b.widget.look)
            ));
        }
        if b.widget.cue.as_deref() == Some("flash")
            && style.cue_time > 0.0
            && contrast(words, ink.flash) < 4.5
        {
            problem("text on its flash needs contrast at least 4.5:1".into());
        }
        let mut sub = 0;
        for (range, name) in span_ranges(&b.widget) {
            let words = &t.text[range.clone()];
            sub += 1;
            let Some(look) = style.looks.get(&name) else {
                problem(format!(
                    "span {sub}: look `{name}` is not in [screen.looks]"
                ));
                continue;
            };
            let on = look.fill.unwrap_or(ink.surface);
            let colour = look.text.unwrap_or(ink.text);
            let need = if look.mark { 3.0 } else { 4.5 };
            if contrast(colour, on) < need {
                problem(format!(
                    "span `{}` (look `{name}`) needs contrast at least {need}:1 on what it sits on",
                    words.trim()
                ));
            }
            if let Some(u) = look.underline
                && contrast(u, on) < 3.0
            {
                problem(format!(
                    "span `{}` (look `{name}`): its underline needs contrast at least 3:1",
                    words.trim()
                ));
            }
            if look.mark && words.chars().any(char::is_alphanumeric) {
                problem(format!(
                    "span `{}` (look `{name}`) is a mark: symbols only, words go in ordinary text",
                    words.trim()
                ));
            }
        }
        if let Some(look) = style.look(b.widget.look.as_deref())
            && let Some(u) = look.underline
            && contrast(u, ink.surface) < 3.0
        {
            problem(format!(
                "underline needs contrast at least 3:1{}",
                named(&b.widget.look)
            ));
        }
    }
    if let Some(icon) = &b.icon
        && !icon.painted
        && contrast(ink.text, ink.surface) < 3.0
    {
        problem("icon needs contrast at least 3:1 on its surface".into());
    }
}
