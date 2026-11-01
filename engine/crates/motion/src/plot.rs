//! Curves over time, drawn into one small picture: stacked panels sharing a time axis,
//! with flagged moments as red marks.

use crate::image::{FLAG, GRID, Image, PAPER, Rgba, TEXT};

/// One curve.
pub struct Series {
    pub name: String,
    pub values: Vec<Option<f32>>,
    pub color: Rgba,
}

/// One panel: curves sharing a vertical scale.
pub struct Panel {
    pub title: String,
    pub series: Vec<Series>,
    /// Keep zero on the scale.
    pub zero: bool,
}

const PANEL_H: u32 = 110;
const LEFT: u32 = 64;
const TOP: u32 = 22;

/// A chart of `panels` over `times` (seconds), `width` pixels wide, with each time in
/// `flags` marked.
pub fn chart(title: &str, times: &[f32], panels: &[Panel], flags: &[f32], width: u32) -> Image {
    let height = TOP + panels.len() as u32 * (PANEL_H + 20) + 24;
    let mut img = Image::new(width, height, PAPER);
    img.text(6, 6, title, 1, TEXT);
    let (t0, t1) = match (times.first(), times.last()) {
        (Some(a), Some(b)) if b > a => (*a, *b),
        (Some(a), _) => (*a, *a + 1.0),
        _ => (0.0, 1.0),
    };
    let plot_w = width.saturating_sub(LEFT + 10).max(10);
    let x_of = |t: f32| LEFT as f32 + (t - t0) / (t1 - t0) * plot_w as f32;
    for (p, panel) in panels.iter().enumerate() {
        let top = TOP + p as u32 * (PANEL_H + 20) + 12;
        img.text(i64::from(LEFT), i64::from(top) - 11, &panel.title, 1, TEXT);
        let mut lo = f32::INFINITY;
        let mut hi = f32::NEG_INFINITY;
        for s in &panel.series {
            for v in s.values.iter().flatten() {
                lo = lo.min(*v);
                hi = hi.max(*v);
            }
        }
        if panel.zero {
            lo = lo.min(0.0);
            hi = hi.max(0.0);
        }
        if !lo.is_finite() {
            (lo, hi) = (0.0, 1.0);
        }
        if hi - lo < 1e-6 {
            hi = lo + 1.0;
        }
        let pad = (hi - lo) * 0.06;
        let (lo, hi) = (lo - pad, hi + pad);
        let y_of = |v: f32| top as f32 + (1.0 - (v - lo) / (hi - lo)) * PANEL_H as f32;
        img.fill_rect(
            i64::from(LEFT),
            i64::from(top),
            plot_w,
            PANEL_H,
            [32, 35, 40, 255],
        );
        if panel.zero && lo < 0.0 && hi > 0.0 {
            let y = y_of(0.0);
            img.line(LEFT as f32, y, (LEFT + plot_w) as f32, y, GRID);
        }
        img.text(4, i64::from(top), &short(hi), 1, TEXT);
        img.text(4, i64::from(top + PANEL_H) - 8, &short(lo), 1, TEXT);
        for &t in flags {
            let x = x_of(t);
            img.line(
                x,
                top as f32,
                x,
                (top + PANEL_H) as f32,
                [FLAG[0], FLAG[1], FLAG[2], 120],
            );
        }
        let mut legend_x = i64::from(LEFT + plot_w) - 4;
        for s in panel.series.iter().rev() {
            legend_x -= s.name.chars().count() as i64 * 6 + 14;
            img.fill_rect(legend_x, i64::from(top) - 10, 8, 7, s.color);
            img.text(legend_x + 10, i64::from(top) - 11, &s.name, 1, s.color);
        }
        for s in &panel.series {
            let mut last: Option<(f32, f32)> = None;
            for (t, v) in times.iter().zip(&s.values) {
                match v {
                    Some(v) => {
                        let (x, y) = (x_of(*t), y_of(*v));
                        if let Some((lx, ly)) = last {
                            img.line(lx, ly, x, y, s.color);
                        } else {
                            img.set(x as i64, y as i64, s.color);
                        }
                        last = Some((x, y));
                    }
                    None => last = None,
                }
            }
        }
    }
    // The time axis: a mark and label every second (or tenth for short films).
    let axis_y = i64::from(height) - 20;
    let span = t1 - t0;
    let step = if span > 8.0 {
        1.0
    } else if span > 2.0 {
        0.5
    } else {
        0.1
    };
    let mut t = (t0 / step).ceil() * step;
    while t <= t1 + 1e-4 {
        let x = x_of(t) as i64;
        img.line(x as f32, axis_y as f32, x as f32, axis_y as f32 + 4.0, TEXT);
        img.text(x - 8, axis_y + 6, &format!("{t:.1}"), 1, TEXT);
        t += step;
    }
    img.text(i64::from(LEFT) - 40, axis_y + 6, "t (s)", 1, TEXT);
    img
}

/// A short label for a scale value.
fn short(v: f32) -> String {
    if v.abs() >= 100.0 {
        format!("{v:.0}")
    } else if v.abs() >= 1.0 {
        format!("{v:.1}")
    } else {
        format!("{v:.2}")
    }
}
