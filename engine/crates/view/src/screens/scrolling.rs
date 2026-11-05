//! A separate padded lane keeps both the track and its grab area clear of rows.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScrollGrab {
    pub path: String,
    pub grab: f32,
}

pub(super) fn viewport(b: &Boxed) -> Rect {
    let p = b.widget.pad;
    Rect::new(
        b.rect.x + p,
        b.rect.y + p,
        (b.rect.w - 2.0 * p - b.style.handle_width - 8.0).max(0.0),
        (b.rect.h - 2.0 * p).max(0.0),
    )
}

pub(super) fn geometry(b: &Boxed, state: &Presentation) -> Option<(Rect, Rect)> {
    if b.widget.kind != "scroll" || b.content_height <= b.rect.h {
        return None;
    }
    let v = viewport(b);
    let track = Rect::new(v.x + v.w + 8.0, v.y, b.style.handle_width, v.h);
    let h = (v.h * v.h / (b.content_height - 2.0 * b.widget.pad))
        .max(b.style.handle_min)
        .min(v.h);
    let offset = state.scroll.get(&b.widget.path).copied().unwrap_or(0.0);
    let y = track.y + (track.h - h) * offset / (b.content_height - b.rect.h);
    Some((track, Rect::new(track.x, y, b.style.handle_width, h)))
}

pub(super) fn draw(b: &Boxed, overlay: &mut Overlay, state: &Presentation, alpha: f32, dy: f32) {
    let Some((track, thumb)) = geometry(b, state) else {
        return;
    };
    let track = Rect::new(track.x, track.y + dy, track.w, track.h);
    let thumb = Rect::new(thumb.x, thumb.y + dy, thumb.w, thumb.h);
    let style = &b.style;
    let mut rail = style.surface(track, style.track);
    rail.radius = style.handle_radius;
    rail.border_px = 0.0;
    rail.shadow = false;
    let mut handle = style.surface(thumb, style.handle);
    handle.radius = style.handle_radius;
    handle.fill = if state
        .scroll_grab
        .as_ref()
        .is_some_and(|g| g.path == b.widget.path)
    {
        style.handle_active
    } else {
        style.handle
    };
    handle.border_px = style.border_px;
    handle.shadow = false;
    rail.opacity = alpha;
    handle.opacity = alpha;
    overlay.widgets.push(UiLayer {
        panels: vec![rail, handle],
        clip: b.clip.map(|c| Rect::new(c.x, c.y + dy, c.w, c.h)),
        ..Default::default()
    });
}

impl Screens {
    pub(super) fn scroll_down(&mut self, point: Vec2, tick: u64) -> Option<Option<Input>> {
        let hit = self.hit(point)?;
        let b = self
            .all()
            .into_iter()
            .rev()
            .find(|b| b.widget.path == hit.path)?;
        let (track, thumb) = geometry(b, &self.presentation)?;
        if !point_in(track, point) {
            return None;
        }
        let path = b.widget.path.clone();
        let grab = if point_in(thumb, point) {
            point.y - thumb.y
        } else {
            thumb.h * 0.5
        };
        self.presentation.scroll_grab = Some(ScrollGrab { path, grab });
        Some(self.scroll_move(point, tick))
    }

    pub(super) fn scroll_move(&mut self, point: Vec2, tick: u64) -> Option<Input> {
        let grab = self.presentation.scroll_grab.as_ref()?;
        let b = self
            .all()
            .into_iter()
            .find(|b| b.widget.path == grab.path)?;
        let (track, thumb) = geometry(b, &self.presentation)?;
        let max = b.content_height - b.rect.h;
        let next = (point.y - track.y - grab.grab) / (track.h - thumb.h).max(1.0) * max;
        let path = grab.path.clone();
        self.set_scroll(&path, next.clamp(0.0, max), tick)
    }

    pub(super) fn set_scroll(&mut self, path: &str, next: f32, tick: u64) -> Option<Input> {
        let b = self.all().into_iter().find(|b| b.widget.path == path)?;
        let on = b.widget.on.clone();
        let mut payload = b.widget.payload.clone();
        let offset = self.presentation.scroll.entry(path.into()).or_default();
        if next == *offset {
            return None;
        }
        *offset = next;
        payload.insert("offset".into(), serde_json::json!(next));
        on.map(|name| Input {
            tick,
            source: "ui".into(),
            name,
            payload,
            via: None,
        })
    }
}
