//! Ported from PhotoCraft crates/algo/src/photo_util.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: only `splitmix64` and `Rng`, made `pub`; the test keeps upstream's reference values.

/// Advances `state` one SplitMix64 step and returns the mixed output.
pub fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Deterministic 64-bit generator (SplitMix64).
pub struct Rng(pub u64);

impl Rng {
    /// The next value of the SplitMix64 sequence (upstream's name; it never ends, so it is not
    /// an `Iterator`).
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> u64 {
        splitmix64(&mut self.0)
    }
    pub fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splitmix64_output_is_unchanged() {
        let mut zero = 0;
        assert_eq!(
            [0; 3].map(|_| splitmix64(&mut zero)),
            [
                0xE220_A839_7B1D_CDAF,
                0x6E78_9E6A_A1B9_65F4,
                0x06C4_5D18_8009_454F
            ]
        );
        let mut rng = Rng(42);
        let mut state = 42;
        assert_eq!(rng.next(), splitmix64(&mut state));
        assert!(rng.below(3) < 3);
        assert_eq!(Rng(0).below(0), 0);
    }
}
