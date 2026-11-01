//! GPU-independent shaping with the same bundled font and metrics used for drawing.
use crate::scene::{Align, Text};
use glam::Vec2;
use glyphon::cosmic_text::{self, PlatformFallback};
use glyphon::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, Weight, fontdb};
use std::collections::HashMap;
/// Fredoka (SIL Open Font License 1.1), a rounded, friendly typeface.
const FONT: &[u8] = include_bytes!("../assets/fonts/Fredoka-Variable.ttf");
const FAMILY: &str = "Fredoka";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TextKey {
    text: String,
    size_bits: u32,
    weight: u16,
    wrap_bits: Option<u32>,
    align: u8,
}

pub(crate) struct Shaped {
    pub(crate) buffer: Buffer,
    /// Laid-out size in physical pixels.
    pub(crate) size: Vec2,
    pub(crate) used: bool,
}

/// A font system holding only the bundled font.
pub(crate) fn font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_font_data(FONT.to_vec());
    db.set_sans_serif_family(FAMILY);
    FontSystem::new_with_locale_and_db_and_fallback("en-US".into(), db, PlatformFallback)
}

pub(crate) fn key(t: &Text, scale: f32) -> TextKey {
    TextKey {
        text: t.text.clone(),
        size_bits: (t.size * scale).to_bits(),
        weight: t.weight.clamp(300, 700),
        wrap_bits: t.max_width.map(|w| (w * scale).to_bits()),
        align: t.align as u8,
    }
}

pub(crate) fn shape(fs: &mut FontSystem, t: &Text, scale: f32) -> Shaped {
    let size = (t.size * scale).max(1.0);
    let mut buffer = Buffer::new(fs, Metrics::new(size, size * 1.25));
    let wrap = t.max_width.map(|w| (w * scale).max(1.0));
    buffer.set_size(wrap, None);
    let attrs = Attrs::new()
        .family(Family::Name(FAMILY))
        .weight(Weight(t.weight.clamp(300, 700)));
    let align = wrap.map(|_| match t.align {
        Align::Left => cosmic_text::Align::Left,
        Align::Center => cosmic_text::Align::Center,
        Align::Right => cosmic_text::Align::Right,
    });
    buffer.set_text(&t.text, &attrs, Shaping::Advanced, align);
    buffer.shape_until_scroll(fs, false);
    let (mut w, mut h) = (0.0f32, 0.0f32);
    for run in buffer.layout_runs() {
        w = w.max(run.line_w);
        h = h.max(run.line_top + run.line_height);
    }
    Shaped {
        buffer,
        // A single glyph can be wider than its wrap width. Keep that overflow visible
        // to readability checks (and to alignment), rather than reporting only the box.
        size: Vec2::new(wrap.unwrap_or(w).max(w), h),
        used: true,
    }
}

/// Headless text measurements. A layout owns this cache and clears it between trees.
pub struct TextMeasure {
    font_system: FontSystem,
    shaped: HashMap<TextKey, Shaped>,
}
impl Default for TextMeasure {
    fn default() -> Self {
        Self {
            font_system: font_system(),
            shaped: HashMap::new(),
        }
    }
}
impl TextMeasure {
    pub fn clear(&mut self) {
        self.shaped.clear();
    }
    /// Laid-out size of `text` in logical pixels.
    pub fn measure(&mut self, text: &Text, scale: f32) -> Vec2 {
        let k = key(text, scale);
        if !self.shaped.contains_key(&k) {
            let s = shape(&mut self.font_system, text, scale);
            self.shaped.insert(k.clone(), s);
        }
        self.shaped[&k].size / scale
    }

    /// Characters of `text` the bundled font has no glyph for.
    pub fn missing(&mut self, text: &Text, scale: f32) -> Vec<char> {
        let k = key(text, scale);
        if !self.shaped.contains_key(&k) {
            let s = shape(&mut self.font_system, text, scale);
            self.shaped.insert(k.clone(), s);
        }
        let mut out = Vec::new();
        for run in self.shaped[&k].buffer.layout_runs() {
            for g in run.glyphs {
                if g.glyph_id == 0
                    && let Some(c) = run.text.get(g.start..g.end).and_then(|s| s.chars().next())
                    && !c.is_whitespace()
                    && !out.contains(&c)
                {
                    out.push(c);
                }
            }
        }
        out
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_font_shapes_text_without_a_gpu() {
        let mut fs = font_system();
        let t = Text::new("PAUSED", Vec2::ZERO, 40.0);
        let s = shape(&mut fs, &t, 1.0);
        assert!(s.size.x > 60.0 && s.size.x < 400.0, "{:?}", s.size);
        assert!(s.size.y >= 40.0, "{:?}", s.size);
        // Weight is applied when rasterising (variable font), so layout stays stable.
        let bold = shape(&mut fs, &t.clone().weight(700), 1.0);
        assert!((bold.size.x - s.size.x).abs() < s.size.x * 0.2);
        // Wrapping makes long text taller.
        let long = Text::new(
            "a long measured line of text in a generic screen",
            Vec2::ZERO,
            20.0,
        )
        .wrap(120.0);
        assert!(shape(&mut fs, &long, 1.0).size.y > 60.0);
    }
}
