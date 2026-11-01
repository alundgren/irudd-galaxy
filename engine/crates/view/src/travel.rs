//! One presentation grammar, rebuilt from the world's trip on every frame.
use galaxy_api::engine::{ROOM, TICKS_PER_SECOND, TRAVEL};
use galaxy_render::{Align, Color, Panel, Rect, Scene, Text, glam::Vec2};
use galaxy_sim::{World, travel::player};

pub struct Cut<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub elapsed: f32,
    pub reverse: bool,
}

impl<'a> Cut<'a> {
    pub fn step(world: &'a World, alpha: f32) -> Option<Self> {
        let (id, _) = player(world)?;
        let t = &world.component(id, TRAVEL)?.fields;
        if t["active"] != true {
            return None;
        }
        let elapsed = ((world.tick as f64 - 1.0 + alpha as f64 - t["start_tick"].as_f64()?)
            .max(0.0)
            / TICKS_PER_SECOND as f64) as f32;
        Some(Self {
            from: t["from"].as_str()?,
            to: t["to"].as_str()?,
            elapsed,
            reverse: t["reverse"] == true,
        })
    }
    pub fn busy(&self) -> bool {
        self.elapsed < 1.0
    }
    fn out(&self) -> f32 {
        if self.reverse { 0.22 } else { 0.18 }
    }
    fn fade_in(&self) -> f32 {
        if self.reverse { 0.18 } else { 0.22 }
    }
    pub fn place(&self) -> &'a str {
        if self.elapsed < self.out() {
            self.from
        } else {
            self.to
        }
    }
    pub fn opacity(&self) -> f32 {
        (if self.elapsed < self.out() {
            self.elapsed / self.out()
        } else if self.elapsed < 1.0 - self.fade_in() {
            1.0
        } else {
            (1.0 - self.elapsed) / self.fade_in()
        })
        .clamp(0.0, 1.0)
    }
    pub fn overlay(&self, world: &World, size: Vec2, scene: &mut Scene) {
        if !self.busy() {
            return;
        }
        let alpha = self.opacity();
        let layer = &mut scene.overlay.upper[1];
        layer.panels.push(Panel {
            fill: Color::hex(0x1b1424).with_alpha(alpha),
            border: Color::TRANSPARENT,
            border_px: 0.0,
            radius: 0.0,
            shadow: false,
            ..Panel::new(Rect::new(0.0, 0.0, size.x, size.y))
        });
        let title = world
            .component(self.to, ROOM)
            .and_then(|c| c.fields["title"].as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(self.to);
        layer.texts.push(
            Text::new(title, Vec2::new(size.x * 0.5, size.y * 0.5 - 24.0), 40.0)
                .weight(700)
                .align(Align::Center)
                .color(Color::WHITE.with_alpha(((alpha - 0.5) * 2.0).clamp(0.0, 1.0))),
        );
    }
}

pub fn place(world: &World, alpha: f32) -> Option<&str> {
    Cut::step(world, alpha)
        .map(|c| c.place())
        .or_else(|| player(world).map(|(_, p)| p))
}
