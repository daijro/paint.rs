//! The integer transform matrices of ITU-T H.265 clause 8.6.4.2.
//!
//! `transMatrix` is generated from the standard's tabulated quarter period
//! rather than written out, so there is one place a digit could be wrong and
//! `dct_tests.rs` checks that place against the floating-point basis.

/// One quarter period of the integer cosine used by `transMatrix`, indexed by
/// the reduced angle `j` in units of pi/64. `CTAB[j]` is the magnitude of
/// `64 * sqrt(2) * cos(pi * j / 64)` as tabulated by the standard.
const CTAB: [i16; 33] = [
    0, 90, 90, 90, 89, 88, 87, 85, 83, 82, 80, 78, 75, 73, 70, 67, 64, 61, 57, 54, 50, 46, 43, 38,
    36, 31, 25, 22, 18, 13, 9, 4, 0,
];

/// Builds the 32x32 `transMatrix` of clause 8.6.4.2 from [`CTAB`] by folding
/// the angle `k * (2n + 1) * pi / 64` into the tabulated quarter period.
const fn build_dct() -> [[i16; 32]; 32] {
    let mut m = [[0i16; 32]; 32];
    let mut n = 0;
    while n < 32 {
        m[0][n] = 64;
        n += 1;
    }
    let mut k = 1;
    while k < 32 {
        let mut n = 0;
        while n < 32 {
            let mut j = (k * (2 * n + 1)) % 128;
            if j > 64 {
                j = 128 - j;
            }
            m[k][n] = if j <= 32 { CTAB[j] } else { -CTAB[64 - j] };
            n += 1;
        }
        k += 1;
    }
    m
}

/// The 4x4 DST-VII matrix of clause 8.6.4.2, the single source for
/// [`DST_MATRIX`] and for the `i32` copy used by the transform itself.
const DST_DATA: [[i16; 4]; 4] = [
    [29, 55, 74, 84],
    [74, 74, 0, -74],
    [84, -29, -74, 55],
    [55, -84, 74, -29],
];

/// The 32-point HEVC DCT-II integer matrix `transMatrix` of clause 8.6.4.2; the
/// `nTbS`-point matrix is the row sub-sampling `DCT_MATRIX[m * (32 / nTbS)][n]`
/// for `m`, `n` < `nTbS`. Materialised only for the tests that validate it: the
/// transform itself uses the `i32` copies produced by [`sub`].
#[cfg(test)]
pub(crate) static DCT_MATRIX: [[i16; 32]; 32] = build_dct();

/// The 4x4 HEVC DST-VII integer matrix of clause 8.6.4.2, used for the
/// 4x4 luma intra residual. Materialised only for the tests; see
/// [`DCT_MATRIX`].
#[cfg(test)]
pub(crate) static DST_MATRIX: [[i16; 4]; 4] = DST_DATA;

/// Row sub-samples [`build_dct`] down to the `N`-point matrix and widens it to
/// `i32` so the hot loops never sign-extend.
const fn sub<const N: usize>() -> [[i32; N]; N] {
    let full = build_dct();
    let step = 32 / N;
    let mut m = [[0i32; N]; N];
    let mut k = 0;
    while k < N {
        let mut n = 0;
        while n < N {
            m[k][n] = full[k * step][n] as i32;
            n += 1;
        }
        k += 1;
    }
    m
}

/// Widens [`DST_DATA`] to `i32` for the hot loops.
const fn dst_i32() -> [[i32; 4]; 4] {
    let mut m = [[0i32; 4]; 4];
    let mut k = 0;
    while k < 4 {
        let mut n = 0;
        while n < 4 {
            m[k][n] = DST_DATA[k][n] as i32;
            n += 1;
        }
        k += 1;
    }
    m
}

/// 4-point DCT-II matrix.
pub(super) const M4: [[i32; 4]; 4] = sub::<4>();
/// 8-point DCT-II matrix.
pub(super) const M8: [[i32; 8]; 8] = sub::<8>();
/// 16-point DCT-II matrix.
pub(super) const M16: [[i32; 16]; 16] = sub::<16>();
/// 32-point DCT-II matrix.
pub(super) const M32: [[i32; 32]; 32] = sub::<32>();
/// 4-point DST-VII matrix.
pub(super) const MDST: [[i32; 4]; 4] = dst_i32();
