//! Pointer interaction is presentation state, independent of screen function calls.
use super::*;
use galaxy_sim::scenario::Pointer;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Presentation {
    pub version: u32,
    #[serde(default)]
    pub pointer: Option<[f32; 2]>,
    pub scroll: BTreeMap<String, f32>,
    pub drag: Option<Drag>,
    #[serde(default)]
    pub settle: Option<Settle>,
    #[serde(default)]
    pub scroll_grab: Option<scrolling::ScrollGrab>,
    /// UI motion: what came in when, what is fading out, value cues, the held button.
    #[serde(default)]
    pub motion: super::Motion,
}

impl Default for Presentation {
    fn default() -> Self {
        Self {
            version: 1,
            pointer: None,
            scroll: BTreeMap::new(),
            drag: None,
            settle: None,
            scroll_grab: None,
            motion: super::Motion::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Drag {
    pub source: String,
    pub value: String,
    pub label: String,
    pub point: [f32; 2],
    pub id: String,
    pub home: [f32; 4],
    pub grab: [f32; 2],
    pub pad: f32,
    pub size: f32,
    /// Each drop target's verdict height at pickup, by path: layout keeps it while the
    /// value is held, so targets stay put whatever their notes change to.
    #[serde(default)]
    pub kept: BTreeMap<String, f32>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settle {
    pub drag: Drag,
    pub destination: [f32; 4],
    pub start: f32,
}
impl Drag {
    fn matches(&self, w: &Widget) -> bool {
        w.kind == "drag_source"
            && w.id.as_deref() == Some(&self.id)
            && w.value == self.value
            && w.path.split("/screen").next() == self.source.split("/screen").next()
    }
    fn held(&self) -> Rect {
        Rect::new(
            self.point[0] - self.grab[0],
            self.point[1] - self.grab[1],
            self.home[2],
            self.home[3],
        )
    }
}
impl Presentation {
    pub(super) fn lifted(&self, w: &Widget) -> bool {
        self.drag
            .as_ref()
            .or(self.settle.as_ref().map(|s| &s.drag))
            .is_some_and(|d| d.matches(w))
    }
}

impl Screens {
    /// Losing window focus releases capture without sending a drop event.
    pub fn cancel_pointer(&mut self, tick: u64) {
        self.presentation.pointer = None;
        self.presentation.motion.pressed = None;
        self.presentation.scroll_grab = None;
        if let Some(drag) = self.presentation.drag.take() {
            self.presentation.settle = Some(Settle {
                destination: drag.home,
                drag,
                start: tick.saturating_sub(1) as f32 / 30.0,
            });
        }
    }

    pub fn pointer_down(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        if let Some(input) = self.scroll_down(point, tick) {
            return input;
        }
        if self.presentation.drag.is_some() || self.presentation.settle.is_some() {
            return None;
        }
        let w = self.hit(point)?;
        if w.kind == "drag_source" {
            let b = self.all().into_iter().find(|b| b.widget.path == w.path)?;
            self.presentation.drag = Some(Drag {
                source: w.path.clone(),
                id: w.id.clone().unwrap_or_default(),
                value: w.value.clone(),
                label: w.text.clone(),
                point: point.to_array(),
                home: array(b.rect),
                grab: (point - Vec2::new(b.rect.x, b.rect.y)).to_array(),
                pad: w.pad,
                size: w.size,
                kept: self
                    .all()
                    .into_iter()
                    .filter(|b| b.widget.kind == "drop_target")
                    .map(|b| (b.widget.path.clone(), b.verdicts))
                    .collect(),
            });
            None
        } else if matches!(w.kind.as_str(), "button" | "bubble") {
            if w.kind == "button" && w.resting.is_none() {
                self.presentation.motion.pressed = Some(w.path.clone());
            }
            self.click(point, tick)
        } else {
            None
        }
    }

    pub fn pointer_move(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        self.presentation.pointer = Some(point.to_array());
        if let Some(drag) = &mut self.presentation.drag {
            drag.point = point.to_array();
        }
        self.scroll_move(point, tick)
    }

    pub fn pointer_up(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        self.presentation.motion.pressed = None;
        if self.presentation.scroll_grab.is_some() {
            let input = self.scroll_move(point, tick);
            self.presentation.scroll_grab = None;
            return input;
        }
        let mut drag = self.presentation.drag.take()?;
        drag.point = point.to_array();
        if !self
            .all()
            .iter()
            .any(|b| b.widget.path == drag.source && b.widget.value == drag.value)
        {
            return None;
        }
        let target = self.hit(point).and_then(|hit| {
            self.all().into_iter().rev().find(|b| {
                b.widget.kind == "drop_target"
                    && (hit.path == b.widget.path
                        || hit.path.starts_with(&format!("{}/", b.widget.path)))
            })
        });
        let accepted = target.filter(|b| {
            b.widget
                .accepts
                .get(&drag.value)
                .is_some_and(String::is_empty)
        });
        let destination = accepted.map(|b| array(b.rect)).unwrap_or(drag.home);
        let input = accepted.and_then(|b| {
            let mut payload = b.widget.payload.clone();
            payload.insert("value".into(), drag.value.clone().into());
            Some(Input {
                tick,
                source: "ui".into(),
                name: b.widget.on.clone()?,
                payload,
            })
        });
        self.presentation.settle = Some(Settle {
            drag,
            destination,
            start: tick.saturating_sub(1) as f32 / 30.0,
        });
        input
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
        let offset = self.presentation.scroll.get(&key).copied().unwrap_or(0.0);
        Some(self.set_scroll(&key, (offset + delta).clamp(0.0, max), tick))
    }

    pub fn bot_pointer(&mut self, action: &Pointer) -> Result<Option<Input>, String> {
        let point = if let Some(p) = action.widget.strip_prefix("point:") {
            let (x, y) = p.split_once(',').ok_or("point target needs x,y")?;
            let p = Vec2::new(
                x.parse().map_err(|_| "invalid point x")?,
                y.parse().map_err(|_| "invalid point y")?,
            );
            if !p.is_finite() {
                return Err("point must be finite".into());
            }
            p
        } else if let Some((name, part)) = action.widget.rsplit_once("::") {
            let _ = self.target(name)?;
            let b = self
                .all()
                .into_iter()
                .find(|b| b.widget.id.as_deref() == Some(name) || b.widget.path == name)
                .ok_or("missing scroll")?;
            let (track, thumb) =
                scrolling::geometry(b, &self.presentation).ok_or("list has no scrollbar")?;
            match part {
                "thumb" => thumb.center(),
                "track-end" => Vec2::new(track.center().x, track.y + track.h - thumb.h * 0.5),
                "track-start" => Vec2::new(track.center().x, track.y + thumb.h * 0.5),
                _ => return Err("scroll part must be thumb, track-start or track-end".into()),
            }
        } else {
            self.target(&action.widget)?
        };
        match action.action.as_str() {
            "pickup" => {
                if self
                    .hit(point)
                    .is_none_or(|w| w.kind != "drag_source" && w.kind != "scroll")
                {
                    return Err("pickup needs a drag source".into());
                }
                Ok(self.pointer_down(point, action.tick))
            }
            "move" => Ok(self.pointer_move(point, action.tick)),
            "drop" => {
                if self.presentation.drag.is_none() && self.presentation.scroll_grab.is_none() {
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

    pub(crate) fn all(&self) -> Vec<&Boxed> {
        fn walk<'a>(b: &'a Boxed, out: &mut Vec<&'a Boxed>) {
            if b.hidden {
                return;
            }
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

    pub(super) fn finish_motion(&mut self) {
        if self
            .presentation
            .settle
            .as_ref()
            .is_some_and(|s| self.clock >= s.start + 0.24)
        {
            self.presentation.settle = None;
        }
    }

    pub(super) fn draw_drag(&self, overlay: &mut Overlay) {
        let (drag, rect) = if let Some(drag) = &self.presentation.drag {
            (drag, drag.held())
        } else if let Some(settle) = &self.presentation.settle {
            let drag = &settle.drag;
            // The script may have moved the same id to another parent. Follow its new
            // box and suppress that copy until this one arrives.
            let destination = self
                .all()
                .into_iter()
                .find(|b| drag.matches(&b.widget))
                .map(|b| b.rect)
                .unwrap_or_else(|| rect(settle.destination));
            let from = drag.held();
            let t = ((self.clock - settle.start) / 0.24).clamp(0.0, 1.0);
            let t = t * t * (3.0 - 2.0 * t);
            (
                drag,
                Rect::new(
                    from.x + (destination.x - from.x) * t,
                    from.y + (destination.y - from.y) * t,
                    from.w + (destination.w - from.w) * t,
                    from.h + (destination.h - from.h) * t,
                ),
            )
        } else {
            return;
        };
        let mut panel = self.style.surface(rect, self.style.held);
        panel.radius = panel.radius.min(drag.pad * 2.0);
        panel.border_px = panel.border_px.min(drag.pad);
        let text = Text::new(
            &drag.label,
            Vec2::new(rect.x, rect.y) + Vec2::splat(drag.pad),
            drag.size,
        )
        .wrap((rect.w - 2.0 * drag.pad).max(1.0))
        .color(self.style.text);
        overlay.widgets.push(UiLayer {
            panels: vec![panel],
            texts: vec![text],
            ..Default::default()
        });
    }
}

fn rect(r: [f32; 4]) -> Rect {
    Rect::new(r[0], r[1], r[2], r[3])
}
fn array(r: Rect) -> [f32; 4] {
    [r.x, r.y, r.w, r.h]
}
