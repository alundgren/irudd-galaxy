//! UI text through glyphon, using only the bundled font so shots look the same everywhere.

use crate::scene::{Align, Text};
use glam::Vec2;
use glyphon::cosmic_text::{self, PlatformFallback};
use glyphon::{
    Attrs, Buffer, Cache, Family, FontSystem, Metrics, Resolution, Shaping, SwashCache, TextArea,
    TextAtlas, TextBounds, TextRenderer, Viewport, Weight, fontdb,
};
use std::collections::HashMap;

/// Fredoka (SIL Open Font License 1.1), a rounded, friendly typeface.
const FONT: &[u8] = include_bytes!("../../assets/fonts/Fredoka-Variable.ttf");
const FAMILY: &str = "Fredoka";

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct TextKey {
    text: String,
    size_bits: u32,
    weight: u16,
    wrap_bits: Option<u32>,
    align: u8,
}

struct Shaped {
    buffer: Buffer,
    /// Laid-out size in physical pixels.
    size: Vec2,
    used: bool,
}

/// Number of independently drawn text layers (the HUD, two upper layers, the pause card).
pub(crate) const LAYERS: usize = 4;

pub(crate) struct TextSystem {
    font_system: FontSystem,
    swash: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderers: [TextRenderer; LAYERS],
    shaped: HashMap<TextKey, Shaped>,
}

/// A font system holding only the bundled font.
fn font_system() -> FontSystem {
    let mut db = fontdb::Database::new();
    db.load_font_data(FONT.to_vec());
    db.set_sans_serif_family(FAMILY);
    FontSystem::new_with_locale_and_db_and_fallback("en-US".into(), db, PlatformFallback)
}

fn key(t: &Text, scale: f32) -> TextKey {
    TextKey {
        text: t.text.clone(),
        size_bits: (t.size * scale).to_bits(),
        weight: t.weight.clamp(300, 700),
        wrap_bits: t.max_width.map(|w| (w * scale).to_bits()),
        align: t.align as u8,
    }
}

fn shape(fs: &mut FontSystem, t: &Text, scale: f32) -> Shaped {
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
        size: Vec2::new(wrap.unwrap_or(w), h),
        used: true,
    }
}

impl TextSystem {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
        samples: u32,
    ) -> Self {
        let cache = Cache::new(device);
        let viewport = Viewport::new(device, &cache);
        let mut atlas = TextAtlas::new(device, queue, &cache, format);
        let ms = wgpu::MultisampleState {
            count: samples,
            ..Default::default()
        };
        let renderers = std::array::from_fn(|_| TextRenderer::new(&mut atlas, device, ms, None));
        Self {
            font_system: font_system(),
            swash: SwashCache::new(),
            atlas,
            viewport,
            renderers,
            shaped: HashMap::new(),
        }
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

    /// Shapes and uploads every layer's text for this frame.
    pub fn prepare(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        size: (u32, u32),
        scale: f32,
        layers: [&[Text]; LAYERS],
    ) -> Result<(), glyphon::PrepareError> {
        self.viewport.update(
            queue,
            Resolution {
                width: size.0,
                height: size.1,
            },
        );
        for s in self.shaped.values_mut() {
            s.used = false;
        }
        for texts in layers {
            for t in texts {
                let k = key(t, scale);
                match self.shaped.get_mut(&k) {
                    Some(s) => s.used = true,
                    None => {
                        let s = shape(&mut self.font_system, t, scale);
                        self.shaped.insert(k, s);
                    }
                }
            }
        }
        self.shaped.retain(|_, s| s.used);
        let bounds = TextBounds {
            left: 0,
            top: 0,
            right: size.0 as i32,
            bottom: size.1 as i32,
        };
        for (layer, texts) in layers.iter().enumerate() {
            let areas: Vec<TextArea> = texts
                .iter()
                .map(|t| {
                    let s = &self.shaped[&key(t, scale)];
                    let w = s.size.x;
                    let x = t.pos.x * scale
                        - match t.align {
                            Align::Left => 0.0,
                            Align::Center => w * 0.5,
                            Align::Right => w,
                        };
                    let [r, g, b, a] = t.color.to_srgba8();
                    TextArea {
                        buffer: &s.buffer,
                        left: x.round(),
                        top: (t.pos.y * scale).round(),
                        scale: 1.0,
                        bounds,
                        default_color: glyphon::Color::rgba(r, g, b, a),
                        custom_glyphs: &[],
                    }
                })
                .collect();
            self.renderers[layer].prepare(
                device,
                queue,
                &mut self.font_system,
                &mut self.atlas,
                &self.viewport,
                areas,
                &mut self.swash,
            )?;
        }
        Ok(())
    }

    pub fn render(
        &self,
        layer: usize,
        pass: &mut wgpu::RenderPass<'_>,
    ) -> Result<(), glyphon::RenderError> {
        self.renderers[layer].render(&self.atlas, &self.viewport, pass)
    }

    /// Frees atlas space for glyphs that weren't used this frame.
    pub fn trim(&mut self) {
        self.atlas.trim();
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
            "a calm solo miner and trader in the Lantern Reach",
            Vec2::ZERO,
            20.0,
        )
        .wrap(120.0);
        assert!(shape(&mut fs, &long, 1.0).size.y > 60.0);
    }
}
