//! A separate padded lane keeps both the track and its grab area clear of rows.
use super::*;
use serde::{Deserialize, Serialize};

pub(super) const LANE: f32 = 32.0;

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
        (b.rect.w - 2.0 * p - LANE - 8.0).max(0.0),
        (b.rect.h - 2.0 * p).max(0.0),
    )
}

pub(super) fn geometry(b: &Boxed, state: &Presentation) -> Option<(Rect, Rect)> {
    if b.widget.kind != "scroll" || b.content_height <= b.rect.h {
        return None;
    }
    let v = viewport(b);
    let track = Rect::new(v.x + v.w + 8.0, v.y, LANE, v.h);
    let h = (v.h * v.h / (b.content_height - 2.0 * b.widget.pad))
        .max(44.0)
        .min(v.h);
    let offset = state.scroll.get(&b.widget.path).copied().unwrap_or(0.0);
    let y = track.y + (track.h - h) * offset / (b.content_height - b.rect.h);
    Some((track, Rect::new(track.x, y, LANE, h)))
}

pub(super) fn draw(b: &Boxed, overlay: &mut Overlay, state: &Presentation) {
    let Some((track, thumb)) = geometry(b, state) else {
        return;
    };
    let mut rail = Panel::new(track);
    rail.fill = Color::hex(0xe4dccb);
    rail.border_px = 0.0;
    rail.shadow = false;
    let mut handle = Panel::new(thumb);
    handle.fill = Color::hex(
        if state
            .scroll_grab
            .as_ref()
            .is_some_and(|g| g.path == b.widget.path)
        {
            0x8b7052
        } else {
            0xb69b78
        },
    );
    handle.border_px = 1.0;
    handle.shadow = false;
    overlay.widgets.push(UiLayer {
        panels: vec![rail, handle],
        clip: b.clip,
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
        })
    }
}
