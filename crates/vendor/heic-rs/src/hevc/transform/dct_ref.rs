//! The slow reference for the transform tests: literal transcriptions of
//! clauses 8.6.4.2 and 8.6.2, plus the deterministic generator that feeds
//! them. Test-only; `dct_tests.rs` compares the fast path against these.

use super::*;
/// xorshift32, so the tests need no `rand` dependency.
pub(super) struct Rng(u32);

impl Rng {
    pub(super) fn new(seed: u32) -> Self {
        Self(seed | 1)
    }

    pub(super) fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    /// A coefficient uniformly in `coeffMin..=coeffMax`.
    pub(super) fn coeff(&mut self) -> i32 {
        (self.next_u32() % 65536) as i32 - 32768
    }
}

/// The `nTbS`-point matrix, spelled out as the spec defines it.
pub(super) fn ref_matrix(n: usize, tr_type: u8) -> Vec<Vec<i64>> {
    let mut m = vec![vec![0i64; n]; n];
    for k in 0..n {
        for j in 0..n {
            m[k][j] = if tr_type == 1 {
                DST_MATRIX[k][j] as i64
            } else {
                DCT_MATRIX[k * (32 / n)][j] as i64
            };
        }
    }
    m
}

/// Slow, literal transcription of clauses 8.6.4.2 and 8.6.2.
pub(super) fn ref_inverse(d: &[i32], n: usize, tr_type: u8, bit_depth: u8) -> Vec<i32> {
    let m = ref_matrix(n, tr_type);
    let mut g = vec![0i64; n * n];
    for x in 0..n {
        for i in 0..n {
            let mut s = 0i64;
            for j in 0..n {
                s += m[j][i] * d[j * n + x] as i64;
            }
            let v = (s + 64) >> 7;
            g[i * n + x] = v.clamp(-32768, 32767);
        }
    }
    let bd = 20i64 - bit_depth as i64;
    let mut r = vec![0i32; n * n];
    for y in 0..n {
        for i in 0..n {
            let mut s = 0i64;
            for j in 0..n {
                s += m[j][i] * g[y * n + j];
            }
            r[y * n + i] = ((s + (1 << (bd - 1))) >> bd) as i32;
        }
    }
    r
}

/// Slow, literal transcription of the transform-skip path of clause 8.6.2.
pub(super) fn ref_skip(d: &[i32], n: usize, bit_depth: u8, rotate: bool) -> Vec<i32> {
    let ts = 5 + n.trailing_zeros() as i64;
    let bd = 20i64 - bit_depth as i64;
    let mut r = vec![0i32; n * n];
    for y in 0..n {
        for x in 0..n {
            let s: i64 = if rotate {
                d[(n - 1 - y) * n + (n - 1 - x)] as i64
            } else {
                d[y * n + x] as i64
            };
            r[y * n + x] = (((s << ts) + (1 << (bd - 1))) >> bd) as i32;
        }
    }
    r
}

pub(super) fn check(d: &[i32], n: usize, tr_type: u8, bit_depth: u8, what: &str) {
    let expected = ref_inverse(d, n, tr_type, bit_depth);
    let mut got = d.to_vec();
    inverse_transform(&mut got, n, tr_type, bit_depth);
    assert_eq!(got, expected, "{what}: n={n} tr={tr_type} bd={bit_depth}");
}
