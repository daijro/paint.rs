//! Tests for clause 8.6.3, comparing the extent-bounded fast path against a
//! literal per-coefficient transcription of the scaling formula.

use super::*;
use alloc::vec;
use alloc::vec::Vec;

/// xorshift32, so the tests need no `rand` dependency.
struct Rng(u32);

impl Rng {
    fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A level uniformly in the 16-bit range residual coding can produce.
    fn level(&mut self) -> i32 {
        (self.next_u32() % 65536) as i32 - 32768
    }
}

/// Clause 8.6.3 applied to every coefficient, with no shortcut.
fn reference(levels: &[i32], n: usize, qp: i32, bit_depth: u8, factors: Option<&[u8]>) -> Vec<i32> {
    let bd_shift = bit_depth as i64 + n.trailing_zeros() as i64 - 5;
    let qp = qp.max(0);
    let ls = LEVEL_SCALE[(qp % 6) as usize];
    let mut out = levels.to_vec();
    for (i, v) in out.iter_mut().enumerate().take(n * n) {
        let m = factors.map_or(16, |f| f[i] as i64);
        let t = (((*v as i64) * m * ls) << (qp / 6)) + (1i64 << (bd_shift - 1));
        *v = (t >> bd_shift).clamp(COEFF_MIN, COEFF_MAX) as i32;
    }
    out
}

/// A block with a random handful of non-zero levels, so the extent is real.
fn sparse(rng: &mut Rng, n: usize) -> Vec<i32> {
    let mut b = vec![0i32; n * n];
    for _ in 0..(rng.next_u32() % 8) {
        let i = rng.next_u32() as usize % (n * n);
        b[i] = rng.level();
    }
    b
}

fn factors(rng: &mut Rng, n: usize) -> Vec<u8> {
    (0..n * n)
        .map(|_| (rng.next_u32() % 255 + 1) as u8)
        .collect()
}

#[test]
fn a_zero_level_scales_to_zero_at_every_qp_and_depth() {
    // The property the extent shortcut rests on.
    for bit_depth in 8u8..=16 {
        for qp in 0..=(51 + 6 * (bit_depth as i32 - 8)) {
            for &n in &[4usize, 8, 16, 32] {
                let mut b = vec![0i32; n * n];
                scale(&mut b, n, qp, bit_depth, None);
                assert!(b.iter().all(|&v| v == 0), "n={n} qp={qp} bd={bit_depth}");
            }
        }
    }
}

#[test]
fn sparse_blocks_match_the_reference_with_a_flat_factor() {
    let mut rng = Rng(0x1234_5678);
    for &n in &[4usize, 8, 16, 32] {
        for &bit_depth in &[8u8, 10] {
            for qp in [0, 4, 22, 37, 51] {
                for _ in 0..50 {
                    let d = sparse(&mut rng, n);
                    let expected = reference(&d, n, qp, bit_depth, None);
                    let mut got = d.clone();
                    scale(&mut got, n, qp, bit_depth, None);
                    assert_eq!(got, expected, "n={n} qp={qp} bd={bit_depth}");
                }
            }
        }
    }
}

#[test]
fn dense_blocks_match_the_reference_with_scaling_factors() {
    let mut rng = Rng(0x9E37_79B9);
    for &n in &[4usize, 8, 16, 32] {
        for qp in [0, 29, 51] {
            let d: Vec<i32> = (0..n * n).map(|_| rng.level()).collect();
            let f = factors(&mut rng, n);
            let expected = reference(&d, n, qp, 8, Some(&f));
            let mut got = d.clone();
            scale(&mut got, n, qp, 8, Some(&f));
            assert_eq!(got, expected, "n={n} qp={qp}");
            let d = sparse(&mut rng, n);
            let expected = reference(&d, n, qp, 8, Some(&f));
            let mut got = d.clone();
            scale(&mut got, n, qp, 8, Some(&f));
            assert_eq!(got, expected, "sparse n={n} qp={qp}");
        }
    }
}

#[test]
fn a_level_past_n_squared_is_neither_scaled_nor_read() {
    let mut b = vec![0i32; 20];
    b[3] = 5;
    b[16] = 5;
    let expected = reference(&b[..16], 4, 30, 8, None);
    scale(&mut b, 4, 30, 8, None);
    assert_eq!(&b[..16], &expected[..]);
    // 5 * levelScale[30 % 6] * 16 << (30 / 6), then >> bdShift of 5.
    assert_eq!(b[3], 5 * 40 * 16, "the level is scaled, not saturated");
    assert_eq!(b[16], 5);
}

#[test]
fn invalid_arguments_leave_the_block_untouched() {
    let original = vec![7i32; 64];
    for &(n, bd, len, flen) in &[
        (8usize, 8u8, 63usize, None),
        (8, 8, 64, Some(63usize)),
        (0, 8, 64, None),
        (8, 2, 64, None),
    ] {
        let mut b = original[..len].to_vec();
        let f = flen.map(|l| vec![16u8; l]);
        scale(&mut b, n, 30, bd, f.as_deref());
        assert_eq!(b, &original[..len], "n={n} bd={bd} len={len} flen={flen:?}");
    }
}
