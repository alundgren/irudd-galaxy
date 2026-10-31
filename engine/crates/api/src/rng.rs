//! Seeded random numbers. One PCG32 stream per user (an engine service, a module's
//! scripts), derived from the world seed and the stream's name, so adding a user never
//! shifts another's numbers. The streams are part of the saved state. Carried over from
//! the slice.

use std::collections::BTreeMap;

use rand_core::Rng;
use rand_pcg::Pcg32;
use serde::{Deserialize, Serialize};

/// A deterministic random generator.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SimRng(Pcg32);

impl SimRng {
    /// A generator keyed by `(seed, key)`. Same inputs, same numbers, on every run.
    pub fn for_key(seed: u64, key: &str) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(&seed.to_le_bytes());
        h.update(key.as_bytes());
        let bytes = h.finalize();
        let b = bytes.as_bytes();
        let state = u64::from_le_bytes(b[0..8].try_into().expect("8 bytes"));
        let stream = u64::from_le_bytes(b[8..16].try_into().expect("8 bytes"));
        SimRng(Pcg32::new(state, stream))
    }

    pub fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }

    /// Uniform in `[0, 1)`.
    pub fn unit_f32(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 * (1.0 / (1u32 << 24) as f32)
    }

    /// Uniform in `[0, 1)`, with 53 bits (what a script gets).
    pub fn unit_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
    }

    /// Uniform in `[lo, hi)`.
    pub fn range_f32(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.unit_f32()
    }

    /// Uniform in `lo..=hi`.
    pub fn range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        let span = u64::from(hi - lo) + 1;
        lo + (self.next_u64() % span) as u32
    }

    /// True with probability `p`.
    pub fn chance(&mut self, p: f32) -> bool {
        self.unit_f32() < p
    }

    /// The underlying PCG32, for `rand_core` based code.
    pub fn pcg_mut(&mut self) -> &mut Pcg32 {
        &mut self.0
    }
}

/// All subsystem streams for one world.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RngBank {
    seed: u64,
    streams: BTreeMap<String, SimRng>,
}

impl RngBank {
    pub fn new(seed: u64) -> Self {
        Self {
            seed,
            streams: BTreeMap::new(),
        }
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    /// The stream for a subsystem, created on first use.
    pub fn stream(&mut self, name: &str) -> &mut SimRng {
        let seed = self.seed;
        self.streams
            .entry(name.to_owned())
            .or_insert_with(|| SimRng::for_key(seed, name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streams_are_independent_and_repeatable() {
        let mut a = RngBank::new(7);
        let mut b = RngBank::new(7);
        let x: Vec<u32> = (0..4).map(|_| a.stream("first").next_u32()).collect();
        // Using another stream in between doesn't shift "first".
        b.stream("other").next_u32();
        let y: Vec<u32> = (0..4).map(|_| b.stream("first").next_u32()).collect();
        assert_eq!(x, y);
        assert_ne!(
            RngBank::new(8).stream("first").next_u32(),
            RngBank::new(7).stream("first").next_u32()
        );
    }

    #[test]
    fn ranges_stay_in_bounds() {
        let mut r = SimRng::for_key(1, "t");
        for _ in 0..1000 {
            let f = r.range_f32(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&f));
            let n = r.range_u32(5, 9);
            assert!((5..=9).contains(&n));
        }
    }
}
