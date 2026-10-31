//! Layout: real text measuring, rows and columns, and the screen's placed layers.
//!
//! Two passes per widget: `measure` finds the size it wants within a width, `arrange`
//! gives it a rectangle. Text wraps to the width it is given; a word that can't fit, or a
//! row whose fixed parts are wider than its room, is reported as cut off.

use render::Rect;
use render::glam::Vec2;

use super::rules::{self, Problems};
use super::tree::{Align, Kind, Node, Role, TextStyle};

/// Measures text in the bundled font.
pub trait Measure {
    fn text(&mut self, text: &str, size: f32, weight: u16, wrap: Option<f32>) -> Vec2;
    /// Characters the font has no glyph for.
    fn missing(&mut self, text: &str) -> Vec<char>;
}

pub struct Ctx<'a> {
    pub m: &'a mut dyn Measure,
    pub problems: &'a mut Problems,
}

pub const BUTTON_FONT: (f32, u16) = (17.0, 700);
pub const BUTTON_PAD: Vec2 = Vec2::new(16.0, 8.0);
pub const BUTTON_MIN_H: f32 = 38.0;
pub const TILE: Vec2 = Vec2::new(92.0, 74.0);
pub const STATE_SHAPE: f32 = 14.0;
pub const METER_BAR: f32 = 12.0;
pub const DEFAULT_ICON: f32 = 24.0;
pub const MARGIN: f32 = 16.0;

/// The pad a role's surface has when the screen gives none.
pub fn role_pad(role: Role) -> f32 {
    match role {
        Role::Room => 20.0,
        Role::Speech => 14.0,
        Role::Goal | Role::Radio => 12.0,
        Role::Status | Role::Receipt => 10.0,
        Role::Sign => 8.0,
        Role::Action => 8.0,
        Role::Plain => 0.0,
    }
}

fn pad_of(n: &Node) -> f32 {
    match n.kind {
        Kind::Panel if n.pad <= 0.0 => role_pad(n.role),
        Kind::Drop if n.pad <= 0.0 && n.anchor.is_none() => 10.0,
        _ => n.pad,
    }
}

/// A text run's size within `max_w`, the wrap width if it wraps, and whether a word is
/// wider than the room.
pub fn text_size(
    m: &mut dyn Measure,
    s: &str,
    (size, weight): (f32, u16),
    max_w: f32,
) -> (Vec2, Option<f32>, bool) {
    let one = m.text(s, size, weight, None);
    if one.x <= max_w + 0.5 {
        return (one, None, false);
    }
    let wrapped = m.text(s, size, weight, Some(max_w));
    let longest = s
        .split_whitespace()
        .map(|w| m.text(w, size, weight, None).x)
        .fold(0.0f32, f32::max);
    (
        Vec2::new(max_w, wrapped.y),
        Some(max_w),
        longest > max_w + 0.5,
    )
}

/// The size `n` wants within `max_w`.
pub fn measure(cx: &mut Ctx, n: &mut Node, max_w: f32) -> Vec2 {
    let max_w = n.width.map_or(max_w, |w| w.min(max_w)).max(0.0);
    let size = match n.kind {
        Kind::Text => {
            let text = n.text.clone().unwrap_or_default();
            let (s, wrap, cut) = text_size(cx.m, &text, n.style.font(), max_w);
            n.wrap = wrap;
            if cut {
                cx.problems.add(
                    &n.path,
                    format!("{text:?} is cut off: a word is wider than its room"),
                );
            }
            s
        }
        Kind::Button => {
            let label = n.text.clone().unwrap_or_default();
            let icon = if n.icon.is_some() {
                DEFAULT_ICON + 8.0
            } else {
                0.0
            };
            let room = (max_w - BUTTON_PAD.x * 2.0 - icon).max(0.0);
            let (s, wrap, cut) = text_size(cx.m, &label, BUTTON_FONT, room);
            n.wrap = wrap;
            if wrap.is_some() || cut {
                cx.problems
                    .add(&n.path, format!("button {label:?} doesn't fit on one line"));
            }
            Vec2::new(
                s.x + BUTTON_PAD.x * 2.0 + icon,
                (s.y + BUTTON_PAD.y * 2.0).max(BUTTON_MIN_H),
            )
        }
        Kind::Tile => {
            let label = n.text.clone().unwrap_or_default();
            let s = cx.m.text(&label, TextStyle::Small.font().0, 600, None);
            Vec2::new(TILE.x.max(s.x + 16.0), TILE.y)
        }
        Kind::State => {
            let word = n.text.clone().unwrap_or_default();
            if word.trim().is_empty() {
                cx.problems.add(&n.path, "a state colour needs its word");
            }
            let (s, wrap, _) = text_size(
                cx.m,
                &word,
                (15.0, 700),
                (max_w - STATE_SHAPE - 6.0).max(0.0),
            );
            n.wrap = wrap;
            Vec2::new(STATE_SHAPE + 6.0 + s.x, s.y.max(STATE_SHAPE))
        }
        Kind::Meter => {
            let label = n.text.clone().unwrap_or_default();
            if label.trim().is_empty() {
                cx.problems.add(&n.path, "a bar needs its label");
            }
            let (s, wrap, cut) = text_size(cx.m, &label, TextStyle::Small.font(), max_w);
            n.wrap = wrap;
            if cut {
                cx.problems.add(&n.path, format!("{label:?} is cut off"));
            }
            Vec2::new(s.x.max(120.0).min(max_w), s.y + 4.0 + METER_BAR)
        }
        Kind::Icon => Vec2::splat(if n.size > 0.0 { n.size } else { DEFAULT_ICON }),
        Kind::Spacer => Vec2::splat(n.size),
        Kind::Divider => Vec2::new(0.0, 10.0),
        Kind::Row => measure_row(cx, n, max_w),
        Kind::Column | Kind::Panel | Kind::Drag | Kind::Drop | Kind::Screen => {
            measure_column(cx, n, max_w)
        }
        // Placed against a scene anchor when drawn.
        Kind::Bubble | Kind::Hotspot => Vec2::ZERO,
    };
    let size = match n.width {
        Some(w) => Vec2::new(w.min(max_w), size.y),
        None => size,
    };
    n.rect.w = size.x;
    n.rect.h = size.y;
    size
}

fn flow(n: &Node) -> impl Iterator<Item = usize> + '_ {
    n.children
        .iter()
        .enumerate()
        .filter(|(_, c)| !anchored(c))
        .map(|(i, _)| i)
}

/// Placed against a scene anchor, not in the flow.
pub fn anchored(n: &Node) -> bool {
    matches!(n.kind, Kind::Bubble | Kind::Hotspot)
        || (matches!(n.kind, Kind::Drop | Kind::Drag) && n.anchor.is_some())
}

/// A spacer takes room only along its parent's direction.
fn along(kind: Kind, s: Vec2, column: bool) -> Vec2 {
    match (kind, column) {
        (Kind::Spacer, true) => Vec2::new(0.0, s.y),
        (Kind::Spacer, false) => Vec2::new(s.x, 0.0),
        _ => s,
    }
}

fn measure_column(cx: &mut Ctx, n: &mut Node, max_w: f32) -> Vec2 {
    let pad = pad_of(n);
    let inner = (max_w - pad * 2.0).max(0.0);
    let idx: Vec<usize> = flow(n).collect();
    let mut w: f32 = 0.0;
    let mut h = 0.0;
    for (k, &i) in idx.iter().enumerate() {
        let s = along(
            n.children[i].kind,
            measure(cx, &mut n.children[i], inner),
            true,
        );
        w = w.max(s.x);
        h += s.y + if k > 0 { n.gap } else { 0.0 };
    }
    if n.align == Align::Stretch {
        w = inner;
    }
    Vec2::new(w + pad * 2.0, h + pad * 2.0)
}

fn measure_row(cx: &mut Ctx, n: &mut Node, max_w: f32) -> Vec2 {
    let pad = pad_of(n);
    let inner = (max_w - pad * 2.0).max(0.0);
    let idx: Vec<usize> = flow(n).collect();
    let gaps = n.gap * idx.len().saturating_sub(1) as f32;
    let mut fixed = 0.0;
    let mut h: f32 = 0.0;
    let mut grows = Vec::new();
    for &i in &idx {
        if n.children[i].grow > 0.0 {
            grows.push(i);
            continue;
        }
        let s = along(
            n.children[i].kind,
            measure(cx, &mut n.children[i], inner),
            false,
        );
        fixed += s.x;
        h = h.max(s.y);
    }
    let room = inner - fixed - gaps;
    if room < -0.5 {
        cx.problems.add(
            &n.path,
            format!(
                "row needs {:.0}px but has {inner:.0}px: something is cut off",
                fixed + gaps
            ),
        );
    }
    let total: f32 = grows.iter().map(|&i| n.children[i].grow).sum();
    let mut grown = 0.0;
    for &i in &grows {
        let share = (room.max(0.0) * n.children[i].grow / total.max(f32::EPSILON)).max(0.0);
        let s = along(
            n.children[i].kind,
            measure(cx, &mut n.children[i], share),
            false,
        );
        grown += s.x;
        h = h.max(s.y);
    }
    let w = if grows.is_empty() {
        fixed + gaps
    } else {
        inner.max(fixed + gaps + grown)
    };
    Vec2::new(w + pad * 2.0, h + pad * 2.0)
}

/// Gives `n` its rectangle and lays out its children inside it.
pub fn arrange(cx: &mut Ctx, n: &mut Node, rect: Rect) {
    n.rect = rect;
    match n.kind {
        Kind::Row => arrange_row(cx, n, rect),
        Kind::Column | Kind::Panel | Kind::Drag | Kind::Drop | Kind::Screen => {
            arrange_column(cx, n, rect)
        }
        _ => {}
    }
}

fn arrange_column(cx: &mut Ctx, n: &mut Node, rect: Rect) {
    let pad = pad_of(n);
    let inner = Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    );
    let idx: Vec<usize> = flow(n).collect();
    let mut sizes = Vec::with_capacity(idx.len());
    for &i in &idx {
        let c = &mut n.children[i];
        let stretch = n.align == Align::Stretch || matches!(c.kind, Kind::Divider | Kind::Meter);
        let s = along(c.kind, measure(cx, c, inner.w), true);
        let w = if stretch { inner.w } else { s.x.min(inner.w) };
        // Re-measure at the final width so text wraps for it.
        let s = if (w - s.x).abs() > 0.5 {
            along(c.kind, measure(cx, c, w), true)
        } else {
            s
        };
        sizes.push(Vec2::new(w, s.y));
    }
    let used: f32 =
        sizes.iter().map(|s| s.y).sum::<f32>() + n.gap * idx.len().saturating_sub(1) as f32;
    let extra = (inner.h - used).max(0.0);
    let grow_total: f32 = idx.iter().map(|&i| n.children[i].grow).sum();
    let mut y = inner.y;
    for (k, &i) in idx.iter().enumerate() {
        let c = &mut n.children[i];
        let mut s = sizes[k];
        if grow_total > 0.0 && c.grow > 0.0 {
            s.y += extra * c.grow / grow_total;
        }
        let x = match n.align {
            Align::Center => inner.x + (inner.w - s.x) * 0.5,
            Align::End => inner.x + inner.w - s.x,
            _ => inner.x,
        };
        arrange(cx, c, Rect::new(x, y, s.x, s.y));
        y += s.y + n.gap;
    }
}

fn arrange_row(cx: &mut Ctx, n: &mut Node, rect: Rect) {
    let pad = pad_of(n);
    let inner = Rect::new(
        rect.x + pad,
        rect.y + pad,
        (rect.w - pad * 2.0).max(0.0),
        (rect.h - pad * 2.0).max(0.0),
    );
    let idx: Vec<usize> = flow(n).collect();
    let gaps = n.gap * idx.len().saturating_sub(1) as f32;
    let mut widths = vec![0.0; idx.len()];
    let mut fixed = 0.0;
    for (k, &i) in idx.iter().enumerate() {
        if n.children[i].grow <= 0.0 {
            widths[k] = n.children[i].rect.w;
            fixed += widths[k];
        }
    }
    let room = (inner.w - fixed - gaps).max(0.0);
    let total: f32 = idx.iter().map(|&i| n.children[i].grow).sum();
    for (k, &i) in idx.iter().enumerate() {
        if n.children[i].grow > 0.0 {
            widths[k] = room * n.children[i].grow / total.max(f32::EPSILON);
        }
    }
    let mut x = inner.x;
    for (k, &i) in idx.iter().enumerate() {
        let c = &mut n.children[i];
        let s = along(c.kind, measure(cx, c, widths[k]), false);
        let h = if n.align == Align::Stretch {
            inner.h
        } else {
            s.y
        };
        let y = match n.align {
            Align::Start | Align::Stretch => inner.y,
            Align::Center => inner.y + (inner.h - h) * 0.5,
            Align::End => inner.y + inner.h - h,
        };
        arrange(cx, c, Rect::new(x, y, widths[k], h));
        x += widths[k] + n.gap;
    }
}

/// Lays out a free-standing tree (a preview, a ghost) at `at` within `max_w`.
pub fn lay_out_at(cx: &mut Ctx, n: &mut Node, at: Vec2, max_w: f32) -> Rect {
    let s = measure(cx, n, max_w);
    let r = Rect::new(at.x, at.y, s.x, s.y);
    arrange(cx, n, r);
    r
}

/// The default widest a placed layer may be.
fn place_width(place: &str, screen: Vec2) -> f32 {
    match place {
        "right" | "left" => screen.x * rules::ROOM_SHARE,
        "top" | "bottom" | "center" => screen.x * 0.6,
        _ => screen.x * 0.34,
    }
}

/// Places the screen's top-level layers and checks them against each other and the
/// screen. Returns the rectangles of the placed layers (bubbles keep clear of them).
pub fn place_screen(cx: &mut Ctx, root: &mut Node, screen: Vec2) -> Vec<Rect> {
    root.rect = Rect::new(0.0, 0.0, screen.x, screen.y);
    let mut placed: Vec<(String, Rect)> = Vec::new();
    // Bottom layers first, so side panels can stop above them.
    let order: Vec<usize> = (0..root.children.len()).collect();
    let mut bottom_top = screen.y - MARGIN;
    for &i in &order {
        if anchored(&root.children[i]) {
            continue;
        }
        if let Some(p) = root.children[i].place.clone()
            && p.starts_with("bottom")
        {
            let c = &mut root.children[i];
            let max_w = c.width.unwrap_or(place_width(&p, screen));
            let s = measure(cx, c, max_w);
            bottom_top = bottom_top.min(screen.y - MARGIN - s.y);
        }
    }
    for i in order {
        if anchored(&root.children[i]) {
            continue;
        }
        let c = &mut root.children[i];
        let Some(place) = c.place.clone() else {
            cx.problems.add(
                &c.path,
                "a top-level widget needs a place (top, right, bottom-left, ...)",
            );
            continue;
        };
        let max_w = c.width.unwrap_or(place_width(&place, screen));
        let s = measure(cx, c, max_w);
        let (w, h) = (s.x, s.y);
        let side_h = (bottom_top - 12.0 - MARGIN).max(0.0);
        let r = match place.as_str() {
            "top" => Rect::new((screen.x - w) * 0.5, MARGIN, w, h),
            "top-left" => Rect::new(MARGIN, MARGIN, w, h),
            "top-right" => Rect::new(screen.x - MARGIN - w, MARGIN, w, h),
            "bottom" => Rect::new((screen.x - w) * 0.5, screen.y - MARGIN - h, w, h),
            "bottom-left" => Rect::new(MARGIN, screen.y - MARGIN - h, w, h),
            "bottom-right" => Rect::new(screen.x - MARGIN - w, screen.y - MARGIN - h, w, h),
            "center" => Rect::new((screen.x - w) * 0.5, (screen.y - h) * 0.5, w, h),
            // Side panels hang from the top margin and must end above the bottom layers.
            "right" => Rect::new(screen.x - MARGIN - w, MARGIN, w, h),
            "left" => Rect::new(MARGIN, MARGIN, w, h),
            other => {
                cx.problems.add(&c.path, format!("unknown place {other:?}"));
                continue;
            }
        };
        if matches!(place.as_str(), "right" | "left") && h > side_h + 0.5 {
            cx.problems.at(
                &c.path,
                r,
                format!("too tall for the side: needs {h:.0}px, has {side_h:.0}px"),
            );
        }
        if c.role == Role::Room && r.w > screen.x * rules::ROOM_SHARE + 0.5 {
            cx.problems.at(
                &c.path,
                r,
                format!(
                    "a room panel covers {:.0}% of the width (at most {:.0}%)",
                    r.w / screen.x * 100.0,
                    rules::ROOM_SHARE * 100.0
                ),
            );
        }
        if !rules::contains(Rect::new(0.0, 0.0, screen.x, screen.y), r) {
            cx.problems.at(&c.path, r, "runs off the screen");
        }
        arrange(cx, c, r);
        for (other, o) in &placed {
            if rules::overlaps(*o, r) {
                cx.problems.at(&c.path, r, format!("overlaps {other}"));
            }
        }
        placed.push((c.path.clone(), r));
    }
    placed.into_iter().map(|(_, r)| r).collect()
}
