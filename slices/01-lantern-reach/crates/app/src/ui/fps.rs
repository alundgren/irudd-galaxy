//! The frame counter: frames a second and sim ticks a second, top left, in white on a
//! dark panel. F2 toggles it in play, and `galaxy play --fps` starts with it on.
//!
//! Developer tooling, off by default. It reads the wall clock and the tick number only,
//! never changes the sim, and keeps its state here (per thread: the window's), so the
//! HUD needs one call ([`draw`]) and nothing else to carry it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

use galaxy_contract::world::WorldState;
use render::glam::Vec2 as G2;
use render::{Color, Overlay, Panel, Rect, Text, palette};

/// How far back the rates look.
const WINDOW: Duration = Duration::from_secs(1);

/// Where the counter sits on the canvas: top left, or under the goal line (top
/// centre) when a long goal reaches that far left. The hold is top right.
pub fn rect(goal: Option<Rect>) -> Rect {
    let r = Rect::new(18.0, 18.0, 220.0, 36.0);
    match goal {
        Some(g) if g.x < r.x + r.w + 8.0 && g.y < r.y + r.h + 8.0 => {
            Rect::new(r.x, g.y + g.h + 8.0, r.w, r.h)
        }
        _ => r,
    }
}

/// Frames and sim ticks over the last [`WINDOW`] of frames.
#[derive(Clone, Debug, Default)]
pub struct Meter {
    frames: VecDeque<(Instant, u64)>,
}

impl Meter {
    /// Records a frame drawn at `now` with the sim at `tick`.
    pub fn frame(&mut self, now: Instant, tick: u64) {
        self.frames.push_back((now, tick));
        while self.frames.len() > 2
            && self
                .frames
                .front()
                .is_some_and(|&(t, _)| now.saturating_duration_since(t) > WINDOW)
        {
            self.frames.pop_front();
        }
    }

    /// Frames a second and sim ticks a second, once two frames are apart in time.
    pub fn rates(&self) -> Option<(f32, f32)> {
        let (&(t0, k0), &(t1, k1)) = (self.frames.front()?, self.frames.back()?);
        let secs = t1.saturating_duration_since(t0).as_secs_f32();
        if secs <= 0.0 {
            return None;
        }
        let frames = (self.frames.len() - 1) as f32;
        Some((frames / secs, k1.saturating_sub(k0) as f32 / secs))
    }
}

/// The counter's text for `rates` (dashes until there is something to measure).
pub fn label(rates: Option<(f32, f32)>) -> String {
    match rates {
        Some((fps, tps)) => format!("{fps:.0} fps · {tps:.0} ticks/s"),
        None => "-- fps · -- ticks/s".into(),
    }
}

#[derive(Default)]
struct State {
    shown: bool,
    meter: Meter,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// Shows or hides the counter.
pub fn set_shown(on: bool) {
    STATE.with_borrow_mut(|s| {
        s.shown = on;
        s.meter = Meter::default();
    });
}

/// F2: shows the counter if hidden, hides it if shown.
pub fn toggle() {
    set_shown(!shown());
}

pub fn shown() -> bool {
    STATE.with_borrow(|s| s.shown)
}

/// Counts this frame and, while the counter is shown, draws it. Called once a frame
/// by the HUD, on the canvas.
pub fn draw(o: &mut Overlay, world: &WorldState, canvas: G2) {
    let rates = STATE.with_borrow_mut(|s| {
        if !s.shown {
            return None;
        }
        s.meter.frame(Instant::now(), world.meta.tick.0);
        Some(s.meter.rates())
    });
    if let Some(rates) = rates {
        panel(o, rates, rect(crate::ui::goal_rect(world, canvas)));
    }
}

fn panel(o: &mut Overlay, rates: Option<(f32, f32)>, r: Rect) {
    o.panels.push(Panel {
        fill: palette::INK.with_alpha(0.92),
        border: Color::WHITE,
        border_px: 1.0,
        radius: 8.0,
        ..Panel::new(r)
    });
    o.texts.push(
        Text::new(label(rates), G2::new(r.x + 12.0, r.y + 8.0), 16.0)
            .weight(600)
            .color(Color::WHITE),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rates_count_frames_and_ticks_over_the_last_second() {
        let start = Instant::now();
        let mut m = Meter::default();
        assert_eq!(m.rates(), None);
        // 60 frames a second for two seconds, the sim at 30 ticks a second.
        for i in 0..=120u64 {
            m.frame(start + Duration::from_millis(i * 1000 / 60), i / 2);
        }
        let (fps, tps) = m.rates().unwrap();
        assert!((fps - 60.0).abs() < 1.5, "{fps}");
        assert!((tps - 30.0).abs() < 1.5, "{tps}");
        // Paused: frames go on, ticks stop.
        for i in 121..=240u64 {
            m.frame(start + Duration::from_millis(i * 1000 / 60), 60);
        }
        let (fps, tps) = m.rates().unwrap();
        assert!((fps - 60.0).abs() < 1.5, "{fps}");
        assert_eq!(tps, 0.0);
    }

    #[test]
    fn hidden_draws_nothing_and_shown_draws_white_text() {
        let world = WorldState::default();
        set_shown(false);
        let mut o = Overlay::default();
        draw(&mut o, &world, crate::ui::DESIGN);
        assert!(o.panels.is_empty() && o.texts.is_empty());
        toggle();
        draw(&mut o, &world, crate::ui::DESIGN);
        assert_eq!(o.texts.len(), 1);
        assert_eq!(o.texts[0].text, "-- fps · -- ticks/s");
        assert_eq!(o.texts[0].color, Color::WHITE);
        set_shown(false);
        assert!(!shown());
    }

    #[test]
    fn the_counter_keeps_clear_of_the_goal_line() {
        let apart = |a: Rect, b: Rect| {
            a.x + a.w <= b.x || b.x + b.w <= a.x || a.y + a.h <= b.y || b.y + b.h <= a.y
        };
        let canvas = crate::ui::DESIGN;
        let alone = rect(None);
        assert!(alone.x >= 0.0 && alone.y >= 0.0 && alone.x + alone.w <= canvas.x * 0.25);
        // A short goal leaves the corner free; a long one (two lines, nearly the
        // canvas wide) pushes the counter under it.
        for goal in [
            Rect::new(400.0, 18.0, 480.0, 36.0),
            Rect::new(60.0, 18.0, 1160.0, 64.0),
        ] {
            let r = rect(Some(goal));
            assert!(apart(r, goal), "{r:?} overlaps {goal:?}");
            assert!(r.y + r.h <= canvas.y);
        }
    }
}
