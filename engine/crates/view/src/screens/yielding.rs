//! Anchored widgets that yield (`yield = true`): instead of being clamped into the
//! window, such a widget isn't drawn while its anchor is off screen, while it would
//! leave the window, or while it would overlap another painted widget. Decided each frame
//! on what is shown, over every screen, talk bubble and life sign at once, in paint
//! order: every widget that doesn't yield is in the way, and so is each yielding one
//! already shown, so two labels never cover each other either.

use galaxy_render::Rect;

use super::{Boxed, contains, intersection, overlaps, painted_kind};

/// How far inside the window a yielding widget must stay: the margin others are clamped to.
const MARGIN: f32 = 8.0;

/// Marks yielding widgets that can't show as hidden.
pub(super) fn resolve(boxes: &mut [Boxed], size: galaxy_render::glam::Vec2) {
    if !boxes.iter().any(yielding) {
        return;
    }
    let mut fixed = Vec::new();
    for b in boxes.iter() {
        in_the_way(b, &mut fixed);
    }
    let window = Rect::new(
        MARGIN,
        MARGIN,
        (size.x - 2.0 * MARGIN).max(0.0),
        (size.y - 2.0 * MARGIN).max(0.0),
    );
    let mut shown: Vec<(String, Rect)> = Vec::new();
    for b in boxes.iter_mut() {
        decide(b, window, &fixed, &mut shown);
    }
}

/// Copies whose yielding widgets are hidden from the shown tree to the placed one: the
/// same tree before scrolling.
pub(super) fn copy_hidden(shown: &Boxed, placed: &mut Boxed) {
    placed.hidden = shown.hidden;
    for (s, p) in shown.children.iter().zip(&mut placed.children) {
        copy_hidden(s, p);
    }
}

/// The painted widgets outside any yielding one, with their shown rectangles.
fn in_the_way(b: &Boxed, out: &mut Vec<(String, Rect)>) {
    if b.widget.yields {
        return;
    }
    if painted_kind(&b.widget.kind) {
        out.push((b.widget.path.clone(), visible(b)));
    }
    for c in &b.children {
        in_the_way(c, out);
    }
}

fn yielding(b: &Boxed) -> bool {
    b.widget.yields || b.children.iter().any(yielding)
}

fn visible(b: &Boxed) -> Rect {
    b.clip.map_or(b.rect, |r| intersection(r, b.rect))
}

/// Decides each yielding widget in `b`, in paint order.
fn decide(b: &mut Boxed, window: Rect, fixed: &[(String, Rect)], shown: &mut Vec<(String, Rect)>) {
    if !b.widget.yields {
        for c in &mut b.children {
            decide(c, window, fixed, shown);
        }
        return;
    }
    if b.hidden {
        return;
    }
    let mut own = Vec::new();
    painted_rects(b, &mut own);
    // What contains this widget isn't in its way.
    let ancestor = |path: &str| {
        b.widget
            .path
            .strip_prefix(path)
            .is_some_and(|rest| rest.starts_with('/'))
    };
    let blocked = own.iter().any(|r| {
        !contains(window, *r)
            || fixed
                .iter()
                .chain(shown.iter())
                .any(|(path, other)| !ancestor(path) && overlaps(*r, *other))
    });
    if blocked {
        b.hidden = true;
    } else {
        shown.extend(own.into_iter().map(|r| (b.widget.path.clone(), r)));
    }
}

/// The rectangles a widget and everything in it paint.
fn painted_rects(b: &Boxed, out: &mut Vec<Rect>) {
    if painted_kind(&b.widget.kind) {
        out.push(visible(b));
    }
    for c in &b.children {
        painted_rects(c, out);
    }
}
