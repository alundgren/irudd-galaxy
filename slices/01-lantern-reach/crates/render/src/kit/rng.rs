//! A tiny deterministic random generator for the kit.
//!
//! The kit must give the same mesh for the same seed on every platform and build, so it
//! uses its own SplitMix64 instead of a crate whose stream might change between versions.

/// SplitMix64. Fast, tiny and good enough for shapes.
#[derive(Clone, Debug)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    /// A child generator for one named purpose, so adding draws in one place doesn't
    /// shift every other shape made from the same seed.
    pub fn fork(&self, salt: u64) -> Self {
        let mut r = Self(self.0 ^ salt.wrapping_mul(0xd1b5_4a32_d192_ed03));
        r.next_u64();
        r
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.f32()
    }

    /// Uniform integer in `[0, n)`.
    pub fn below(&mut self, n: u32) -> u32 {
        ((self.next_u64() >> 32) * n as u64 >> 32) as u32
    }

    /// Uniform in `[-1, 1)`.
    pub fn signed(&mut self) -> f32 {
        self.f32() * 2.0 - 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_and_in_range() {
        let mut a = Rng::new(42);
        let mut b = Rng::new(42);
        for _ in 0..1000 {
            let x = a.f32();
            assert_eq!(x, b.f32());
            assert!((0.0..1.0).contains(&x));
            assert!(a.below(7) < 7);
            b.below(7);
        }
        assert_ne!(Rng::new(1).next_u64(), Rng::new(2).next_u64());
        assert_ne!(
            Rng::new(1).fork(1).next_u64(),
            Rng::new(1).fork(2).next_u64()
        );
    }
}
