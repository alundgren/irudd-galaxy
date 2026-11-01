//! Rule breaks found while reading, laying out and placing a screen.
//!
//! The readability rules are enforced by construction where they can be (a screen can't
//! choose a text colour, a resting button can't be greyed), and checked where they can't
//! (nothing cut off, nothing overlapping, a room panel at most 45% of the screen). A
//! break is listed on screen and printed; the shot run fails on any.

use render::Rect;

#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    /// The widget's path in the tree.
    pub path: String,
    pub what: String,
    /// Where it is on screen, when known, so it can be outlined.
    pub rect: Option<Rect>,
}

#[derive(Clone, Debug, Default)]
pub struct Problems(pub Vec<Problem>);

impl Problems {
    pub fn add(&mut self, path: &str, what: impl Into<String>) {
        self.push(path, what, None);
    }

    pub fn at(&mut self, path: &str, rect: Rect, what: impl Into<String>) {
        self.push(path, what, Some(rect));
    }

    fn push(&mut self, path: &str, what: impl Into<String>, rect: Option<Rect>) {
        let what = what.into();
        if !self.0.iter().any(|p| p.path == path && p.what == what) {
            self.0.push(Problem {
                path: path.to_string(),
                what,
                rect,
            });
        }
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn extend(&mut self, other: &Problems) {
        for p in &other.0 {
            self.push(&p.path, p.what.clone(), p.rect);
        }
    }
}

pub fn overlaps(a: Rect, b: Rect) -> bool {
    // A pixel of touching is fine.
    a.x + 1.0 < b.x + b.w && b.x + 1.0 < a.x + a.w && a.y + 1.0 < b.y + b.h && b.y + 1.0 < a.y + a.h
}

pub fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x - 0.5
        && inner.y >= outer.y - 0.5
        && inner.x + inner.w <= outer.x + outer.w + 0.5
        && inner.y + inner.h <= outer.y + outer.h + 0.5
}

/// The largest share of the screen's width a room panel may take.
pub const ROOM_SHARE: f32 = 0.45;
