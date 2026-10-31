//! Fixed-step pacing for runs in real time. Carried over from the slice.
//!
//! The sim never reads a clock. A real-time run measures elapsed time and asks
//! [`FixedStep`] how many ticks to run now. Headless runs skip this entirely and step as
//! fast as they like.

use galaxy_api::engine::TICKS_PER_SECOND;

#[derive(Clone, Debug)]
pub struct FixedStep {
    tick_seconds: f64,
    accumulator: f64,
    /// Most ticks run in one frame; the rest is dropped so a long stall doesn't
    /// fast-forward the world.
    pub max_ticks_per_frame: u32,
}

impl Default for FixedStep {
    fn default() -> Self {
        Self::new(TICKS_PER_SECOND)
    }
}

impl FixedStep {
    pub fn new(ticks_per_second: u32) -> Self {
        Self {
            tick_seconds: 1.0 / f64::from(ticks_per_second.max(1)),
            accumulator: 0.0,
            max_ticks_per_frame: 5,
        }
    }

    /// Adds `elapsed_seconds` of real time; returns how many ticks to run now.
    pub fn advance(&mut self, elapsed_seconds: f64) -> u32 {
        self.accumulator += elapsed_seconds.max(0.0);
        let mut ticks = 0;
        while self.accumulator >= self.tick_seconds && ticks < self.max_ticks_per_frame {
            self.accumulator -= self.tick_seconds;
            ticks += 1;
        }
        if ticks == self.max_ticks_per_frame {
            self.accumulator = self.accumulator.min(self.tick_seconds);
        }
        ticks
    }

    /// How far into the next tick we are, `0..1`, for render interpolation.
    pub fn alpha(&self) -> f32 {
        (self.accumulator / self.tick_seconds) as f32
    }

    /// Drops accumulated time (on pause, so resuming doesn't jump).
    pub fn reset(&mut self) {
        self.accumulator = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_at_the_nominal_rate() {
        let mut s = FixedStep::new(30);
        let total: u32 = (0..60).map(|_| s.advance(1.0 / 60.0)).sum();
        assert!((29..=30).contains(&total));
        assert_eq!(s.advance(10.0), 5);
    }
}
