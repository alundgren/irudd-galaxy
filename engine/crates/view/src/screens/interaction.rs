//! Pointer interaction is presentation state, independent of screen function calls.
use super::*;
use galaxy_sim::scenario::Pointer;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    pub scroll: BTreeMap<String, f32>,
    pub drag: Option<Drag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drag {
    pub source: String,
    pub value: String,
    pub label: String,
    pub point: [f32; 2],
}

impl Screens {
    pub fn pointer_down(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        let w = self.hit(point)?;
        if w.kind == "drag_source" {
            self.presentation.drag = Some(Drag {
                source: w.path.clone(),
                value: w.value.clone(),
                label: w.text.clone(),
                point: point.to_array(),
            });
            None
        } else if w.kind == "button" {
            self.click(point, tick)
        } else {
            None
        }
    }

    pub fn pointer_move(&mut self, point: Vec2) {
        if let Some(drag) = &mut self.presentation.drag {
            drag.point = point.to_array();
        }
    }

    pub fn pointer_up(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        let drag = self.presentation.drag.take()?;
        // A hidden or changed source cannot complete an obsolete drag.
        if !self
            .all()
            .iter()
            .any(|b| b.widget.path == drag.source && b.widget.value == drag.value)
        {
            return None;
        }
        let target = self.hit(point)?;
        if target.kind != "drop_target"
            || !target
                .accepts
                .get(&drag.value)
                .is_some_and(String::is_empty)
        {
            return None;
        }
        let mut payload = target.payload.clone();
        payload.insert("value".into(), drag.value.into());
        Some(Input {
            tick,
            source: "ui".into(),
            name: target.on.clone()?,
            payload,
        })
    }

    /// None means the wheel belongs to the camera. Some consumes it, optionally emitting
    /// a world-changing event with the resulting offset (logical pixels).
    pub fn scroll(&mut self, point: Vec2, delta: f32, tick: u64) -> Option<Option<Input>> {
        let hit = self.hit(point)?;
        let hit_path = hit.path.clone();
        let b = self.all().into_iter().rev().find(|b| {
            b.widget.kind == "scroll"
                && (hit_path == b.widget.path
                    || hit_path.starts_with(&format!("{}/", b.widget.path)))
                && point_in(b.rect, point)
                && b.clip.is_none_or(|r| point_in(r, point))
        })?;
        let key = b.widget.path.clone();
        let max = (b.content_height - b.rect.h).max(0.0);
        let on = b.widget.on.clone();
        let mut payload = b.widget.payload.clone();
        let offset = self.presentation.scroll.entry(key).or_default();
        let next = (*offset + delta).clamp(0.0, max);
        if next == *offset {
            return Some(None);
        }
        *offset = next;
        payload.insert("offset".into(), serde_json::json!(next));
        Some(on.map(|name| Input {
            tick,
            source: "ui".into(),
            name,
            payload,
        }))
    }

    pub fn bot_pointer(&mut self, action: &Pointer) -> Result<Option<Input>, String> {
        let point = self.target(&action.widget)?;
        match action.action.as_str() {
            "pickup" => {
                if self.hit(point).is_none_or(|w| w.kind != "drag_source") {
                    return Err("pickup needs a drag source".into());
                }
                self.pointer_down(point, action.tick);
                Ok(None)
            }
            "move" => {
                if self.presentation.drag.is_none() {
                    return Err("move needs an active drag".into());
                }
                self.pointer_move(point);
                Ok(None)
            }
            "drop" => {
                if self.presentation.drag.is_none() {
                    return Err("drop needs an active drag".into());
                }
                Ok(self.pointer_up(point, action.tick))
            }
            "scroll" => self
                .scroll(point, action.delta, action.tick)
                .ok_or_else(|| "scroll needs a visible scrolling list".into()),
            _ => Err("pointer action must be pickup, move, drop or scroll".into()),
        }
    }

    fn all(&self) -> Vec<&Boxed> {
        fn walk<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
            out.push(b);
            for c in &b.children {
                walk(c, out);
            }
        }
        let mut out = Vec::new();
        for b in &self.boxes {
            walk(b, &mut out);
        }
        out
    }

    fn target(&self, name: &str) -> Result<Vec2, String> {
        let found: Vec<_> = self
            .all()
            .into_iter()
            .filter(|b| b.widget.path == name || b.widget.id.as_deref() == Some(name))
            .collect();
        let b = match found.as_slice() {
            [b] => *b,
            [] => return Err(format!("widget `{name}` is missing")),
            _ => return Err(format!("widget `{name}` is ambiguous; use its full path")),
        };
        let p = b.rect.center();
        let hit = self.hit(p);
        if b.clip.is_some_and(|r| !point_in(r, p))
            || hit.is_none_or(|w| {
                w.path != b.widget.path
                    && !(b.widget.kind == "scroll"
                        && w.path.starts_with(&format!("{}/", b.widget.path)))
            })
        {
            return Err(format!(
                "widget `{name}` is outside its scroll viewport or covered"
            ));
        }
        Ok(p)
    }

    pub(super) fn draw_drag(&mut self, overlay: &mut Overlay, size: Vec2) {
        let Some(drag) = &self.presentation.drag else {
            return;
        };
        let width = 260.0f32.min(size.x - 16.0).max(25.0);
        let mut text =
            Text::new(format!("Dragging: {}", drag.label), Vec2::ZERO, 20.0).wrap(width - 24.0);
        let height = self.measure.measure(&text, 1.0).y + 24.0;
        if height > size.y - 16.0 {
            self.problems
                .push(format!("{}: drag label exceeds the viewport", drag.source));
        }
        // A preview must not hide the verdict the player is trying to read. Prefer
        // beside the pointer, then an edge with space, using the same clipped boxes.
        let point = Vec2::from(drag.point);
        let limit = (size - Vec2::new(width + 8.0, height + 8.0)).max(Vec2::splat(8.0));
        let candidates = [
            point + Vec2::splat(18.0),
            point - Vec2::new(width + 18.0, height + 18.0),
            point + Vec2::new(18.0, -height - 18.0),
            point + Vec2::new(-width - 18.0, 18.0),
            Vec2::new(point.x + 18.0, limit.y),
            Vec2::new(point.x + 18.0, 8.0),
            Vec2::new(limit.x, limit.y),
            Vec2::new(8.0, limit.y),
        ];
        let boxes = self.all();
        let cost = |p: Vec2| -> f32 {
            let rect = Rect::new(p.x, p.y, width, height);
            boxes
                .iter()
                .filter(|b| leaf(&b.widget.kind))
                .map(|b| {
                    let visible = b.clip.map_or(b.rect, |clip| intersection(clip, b.rect));
                    let overlap = intersection(rect, visible);
                    overlap.w
                        * overlap.h
                        * if b.widget.kind == "drop_target" {
                            100.0
                        } else {
                            1.0
                        }
                })
                .sum()
        };
        let p = candidates
            .into_iter()
            .map(|p| p.clamp(Vec2::splat(8.0), limit))
            .min_by(|a, b| cost(*a).total_cmp(&cost(*b)))
            .expect("preview candidates");
        text.pos = p + Vec2::splat(12.0);
        overlay.widgets.push(UiLayer {
            panels: vec![Panel::new(Rect::new(p.x, p.y, width, height))],
            texts: vec![text],
            ..Default::default()
        });
    }
}
