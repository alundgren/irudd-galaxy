//! UI text through glyphon, using only the bundled font so shots look the same everywhere.

use crate::scene::{Align, Text};
use glam::Vec2;
use glyphon::{
    Cache, FontSystem, Resolution, SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer,
    Viewport,
};
use std::collections::HashMap;

use crate::text::{Shaped, TextKey, font_system, key, shape};

pub(crate) struct TextSystem {
    font_system: FontSystem,
    swash: SwashCache,
    atlas: TextAtlas,
    viewport: Viewport,
    renderers: Vec<TextRenderer>,
    shaped: HashMap<TextKey, Shaped>,
    samples: u32,
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
        let atlas = TextAtlas::new(device, queue, &cache, format);
        let renderers = Vec::new();
        Self {
            font_system: font_system(),
            swash: SwashCache::new(),
            atlas,
            viewport,
            renderers,
            samples,
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
        layers: &[&[Text]],
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
            for t in *texts {
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
        while self.renderers.len() < layers.len() {
            self.renderers.push(TextRenderer::new(
                &mut self.atlas,
                device,
                wgpu::MultisampleState {
                    count: self.samples,
                    ..Default::default()
                },
                None,
            ));
        }
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
