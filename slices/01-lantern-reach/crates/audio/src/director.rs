//! Which cue plays, and how: the anti-fatigue rules in code.
//!
//! [`Director::decide`] takes a cue, a seed from the event that asked for it and the
//! presentation clock, and returns a [`Play`] or nothing. It enforces each cue's rate
//! limit, never picks the same take twice in a row, varies pitch and level, thins long
//! runs and staggers repeats off the grid. It knows nothing about kira, so it's tested
//! headless.
//!
//! Variation is seeded per event (the app passes the fact ID), so the same events give
//! the same choices. The clock is real time: audio is presentation, outside the sim.

use crate::cue::{Bus, Cue, CueSpec};

/// One cue to start.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Play {
    pub cue: Cue,
    pub bus: Bus,
    pub take: usize,
    /// Level offset in dB (thinning plus variation).
    pub gain_db: f32,
    /// Playback rate: 1.0 as rendered, 2.0 an octave up.
    pub rate: f64,
    /// Seconds to wait before starting.
    pub delay: f32,
}

#[derive(Clone, Debug, Default)]
struct State {
    /// When it last sounded.
    last: Option<f64>,
    last_take: Option<usize>,
    /// When it was last asked for, sounded or not, and how many asks the run has had.
    asked: Option<f64>,
    run: u32,
}

pub struct Director {
    specs: Vec<CueSpec>,
    state: Vec<State>,
}

impl Default for Director {
    fn default() -> Self {
        Self::new()
    }
}

impl Director {
    pub fn new() -> Self {
        Self {
            specs: Cue::ALL.iter().map(|c| c.spec()).collect(),
            state: vec![State::default(); Cue::ALL.len()],
        }
    }

    /// Whether `cue` should sound now (`now` in seconds), and how. `seed` comes from
    /// the event that asked for it.
    pub fn decide(&mut self, cue: Cue, seed: u64, now: f64) -> Option<Play> {
        let spec = &self.specs[cue.index()];
        let st = &mut self.state[cue.index()];
        let mut rng = Rng::new(seed, cue);
        let mut gain_db = 0.0;
        // Thinning: count this ask in the run first, so a dropped play still keeps the
        // run going.
        if let Some(thin) = spec.play.thin {
            let in_run = st.asked.is_some_and(|t| now - t < f64::from(thin.gap));
            st.run = if in_run { st.run + 1 } else { 0 };
            st.asked = Some(now);
            let keep = rng.unit();
            if st.run >= thin.always && keep > thin.keep {
                return None;
            }
            gain_db = (-thin.step_db * st.run as f32).max(thin.floor_db);
        }
        if st
            .last
            .is_some_and(|t| now - t < f64::from(spec.play.min_interval))
        {
            return None;
        }
        let takes = spec.patch.takes.max(1) as usize;
        let take = match st.last_take {
            Some(last) if takes > 1 => {
                let r = rng.below(takes - 1);
                if r >= last { r + 1 } else { r }
            }
            _ => rng.below(takes),
        };
        let cents = rng.signed() * spec.play.cents;
        gain_db += rng.signed() * spec.play.db;
        let delay = rng.unit() * spec.play.jitter;
        st.last = Some(now);
        st.last_take = Some(take);
        Some(Play {
            cue,
            bus: spec.bus,
            take,
            gain_db,
            rate: 2f64.powf(f64::from(cents) / 1200.0),
            delay,
        })
    }
}

/// A tiny splitmix64 stream, keyed by the event's seed and the cue.
struct Rng(u64);

impl Rng {
    fn new(seed: u64, cue: Cue) -> Self {
        Self(seed ^ (cue.index() as u64 + 1).wrapping_mul(0xA076_1D64_78BD_642F))
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in 0..1.
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in -1..1.
    fn signed(&mut self) -> f32 {
        self.unit() * 2.0 - 1.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_events_make_the_same_choices() {
        let run = || {
            let mut d = Director::new();
            (0..40)
                .map(|i| d.decide(Cue::OreTick, 100 + i, f64::from(i as u32) * 0.9))
                .collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn a_take_never_repeats_back_to_back() {
        let mut d = Director::new();
        let mut last = None;
        for i in 0..200u64 {
            // Far apart, so neither the rate limit nor thinning gets in the way.
            if let Some(p) = d.decide(Cue::Confirm, i, i as f64 * 10.0) {
                assert_ne!(Some(p.take), last, "take {} twice at {i}", p.take);
                last = Some(p.take);
            }
        }
    }

    #[test]
    fn the_rate_limit_merges_rapid_repeats() {
        let mut d = Director::new();
        assert!(d.decide(Cue::Dock, 1, 0.0).is_some());
        assert!(d.decide(Cue::Dock, 2, 0.5).is_none());
        assert!(d.decide(Cue::Dock, 3, 1.5).is_some());
        // Ore ticks a frame apart: only the first sounds.
        let mut d = Director::new();
        let n = (0..10)
            .filter_map(|i| d.decide(Cue::OreTick, i, f64::from(i as u32) / 60.0))
            .count();
        assert_eq!(n, 1);
    }

    #[test]
    fn a_long_mining_run_thins_out() {
        let mut d = Director::new();
        // One unit a second for a minute, as with Space held by an ice rock.
        let plays: Vec<_> = (0..60u64)
            .map(|i| d.decide(Cue::OreTick, 1000 + i, i as f64))
            .collect();
        let thin = Cue::OreTick.spec().play.thin.unwrap();
        // The onset always sounds.
        assert!(plays[..thin.always as usize].iter().all(Option::is_some));
        // Later, some ticks are skipped, and the ones that sound are quieter.
        let late: Vec<_> = plays[20..].iter().flatten().collect();
        assert!(late.len() < 40, "{} of 40 late ticks sounded", late.len());
        assert!(
            late.len() > 10,
            "only {} of 40 late ticks sounded",
            late.len()
        );
        let db = Cue::OreTick.spec().play.db;
        assert!(late.iter().all(|p| p.gain_db <= thin.floor_db + db + 1e-4));
        // A pause in mining starts a fresh run at full level.
        let p = d.decide(Cue::OreTick, 7, 70.0).unwrap();
        assert!(p.gain_db >= -db - 1e-4, "{}", p.gain_db);
    }

    #[test]
    fn variation_stays_in_its_ranges() {
        let mut d = Director::new();
        for i in 0..100u64 {
            let Some(p) = d.decide(Cue::OreTick, i, i as f64 * 10.0) else {
                continue;
            };
            let spec = Cue::OreTick.spec();
            let cents = 1200.0 * p.rate.log2();
            assert!(cents.abs() <= f64::from(spec.play.cents) + 1e-3);
            assert!(p.delay >= 0.0 && p.delay <= spec.play.jitter);
            assert!(p.take < spec.patch.takes as usize);
        }
    }
}
