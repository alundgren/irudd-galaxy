//! Hover labels use the ordinary text measurement, panel paint and readability rules.
use super::*;
use serde_json::{Value, json};

impl Screens {
    pub(crate) fn hover_label(&mut self, text: &str, origin: Vec2, size: Vec2) -> (Value, UiLayer) {
        let pad = self.style.frame_padding;
        let width = 320.0f32.min((size.x - 16.0).max(1.0));
        let mut label = Text::new(text, Vec2::ZERO, 20.0).wrap((width - pad * 2.0).max(1.0));
        label.color = self.style.text;
        let measured = self.measure.measure(&label, 1.0);
        let height = measured.y + pad * 2.0;
        // Empty panel paper is usable space; glyphs and controls are not. Clipped
        // scroll content contributes only the area the player can currently see.
        let obstacles: Vec<_> = self
            .all()
            .into_iter()
            .filter(|b| leaf(&b.widget.kind) || b.widget.kind == "bubble")
            .map(|b| {
                (
                    b.widget.path.clone(),
                    b.clip.map_or(b.rect, |c| intersection(c, b.rect)),
                )
            })
            .filter(|(_, r)| r.w > 0.0 && r.h > 0.0)
            .collect();
        let preferred = origin + Vec2::new(16.0, -height - 16.0);
        let mut candidates = vec![
            preferred,
            origin + Vec2::splat(16.0),
            origin - Vec2::new(width + 16.0, height + 16.0),
        ];
        for (_, r) in &obstacles {
            candidates.extend([
                Vec2::new(r.x + r.w + 8.0, preferred.y),
                Vec2::new(r.x - width - 8.0, preferred.y),
                Vec2::new(preferred.x, r.y - height - 8.0),
                Vec2::new(preferred.x, r.y + r.h + 8.0),
            ]);
        }
        let bounds = (size - Vec2::new(width, height) - Vec2::splat(8.0)).max(Vec2::splat(8.0));
        let rect_at = |p: Vec2| Rect::new(p.x, p.y, width, height);
        let area = |p: Vec2| {
            obstacles
                .iter()
                .map(|(_, r)| {
                    let overlap = intersection(*r, rect_at(p));
                    overlap.w * overlap.h
                })
                .sum::<f32>()
        };
        let pos = candidates
            .into_iter()
            .map(|p| p.clamp(Vec2::splat(8.0), bounds))
            .min_by(|a, b| {
                area(*a).total_cmp(&area(*b)).then_with(|| {
                    a.distance_squared(preferred)
                        .total_cmp(&b.distance_squared(preferred))
                })
            })
            .unwrap();
        label.pos = pos + Vec2::splat(pad);
        let rect = Rect::new(pos.x, pos.y, width, height);
        let mut problems = Vec::new();
        if text.trim().is_empty() {
            problems.push("empty hover label".to_owned());
        }
        if !self.measure.missing(&label, 1.0).is_empty() {
            problems.push("hover has missing glyphs".to_owned());
        }
        if height > size.y - 16.0 || measured.x > width - pad * 2.0 + 0.5 {
            problems.push("hover text is outside the viewport or clipped".to_owned());
        }
        for (path, r) in obstacles {
            if overlaps(rect, r) {
                problems.push(format!("hover covers {path}"));
            }
        }
        let report = json!({"text":text, "rect":[rect.x,rect.y,rect.w,rect.h], "readable":problems.is_empty(), "problems":problems});
        (
            report,
            UiLayer {
                panels: vec![self.style.surface(rect, self.style.paper)],
                texts: vec![label],
                ..Default::default()
            },
        )
    }
}
