//! Inverse DCT-II / DST-VII and transform-skip residual derivation,
//! ITU-T H.265 clause 8.6.4.2 ("Transformation process for scaled transform
//! coefficients") and the final bit-depth shift of clause 8.6.2.
//!
//! Written from the specification text alone. No code, table layout or
//! butterfly schedule was taken from any existing decoder. The matrices
//! themselves are built in `matrix`, which is the only place the standard's
//! tabulated cosine appears.

#[path = "dct_matrix.rs"]
mod matrix;

#[cfg(test)]
pub(crate) use matrix::{DCT_MATRIX, DST_MATRIX};
use matrix::{M4, M8, M16, M32, MDST};

use super::extent::extent;

/// Lower bound `coeffMin` on an intermediate transform coefficient (16 bit).
const COEFF_MIN: i32 = -32768;
/// Upper bound `coeffMax` on an intermediate transform coefficient (16 bit).
const COEFF_MAX: i32 = 32767;
/// Right shift applied between the two one-dimensional stages (clause 8.6.4.2).
const STAGE1_SHIFT: u32 = 7;
/// Lowest bit depth this module accepts; below it `bdShift` would be undefined.
const MIN_BIT_DEPTH: u8 = 8;
/// Highest bit depth this module accepts (H.265 range extensions cap).
const MAX_BIT_DEPTH: u8 = 16;

/// `Clip3(coeffMin, coeffMax, v)` of clause 5, written without `clamp` so that
/// the transform keeps no panicking path at all.
#[allow(clippy::manual_clamp)]
fn clip_coeff(v: i32) -> i32 {
    if v < COEFF_MIN {
        COEFF_MIN
    } else if v > COEFF_MAX {
        COEFF_MAX
    } else {
        v
    }
}

/// `bdShift` of clause 8.6.2, or `None` when the bit depth is out of range.
const fn bd_shift_of(bit_depth: u8) -> Option<u32> {
    if bit_depth >= MIN_BIT_DEPTH && bit_depth <= MAX_BIT_DEPTH {
        Some(20 - bit_depth as u32)
    } else {
        None
    }
}

/// One 1-D inverse transform: `out[i] = sum_j m[j][i] * src[j]`.
///
/// The sum index is the *row* of `m`, i.e. this multiplies by the transpose,
/// exactly as clause 8.6.4.2 writes it. Accumulating over `j` on the outside
/// keeps the inner loop a contiguous multiply-accumulate over two slices.
///
/// `terms` is how many of `src`'s entries may be non-zero; the rest add
/// nothing and are not read. Dropping them is exact, not an approximation.
/// With `SKIP` set, a zero term inside that range is skipped too, which pays
/// on the sparse large blocks and costs a mispredicted branch on dense 4x4s.
///
/// The largest attainable magnitude is `32 * 32768 * 90 = 94_371_840`, well
/// inside `i32`, because both stages take 16-bit-clipped inputs.
fn mul_1d<const N: usize, const SKIP: bool>(
    m: &[[i32; N]; N],
    src: &[i32; N],
    terms: usize,
) -> [i32; N] {
    let mut acc = [0i32; N];
    for j in 0..core::cmp::min(terms, N) {
        let c = src[j];
        if SKIP && c == 0 {
            continue;
        }
        let row = &m[j];
        for i in 0..N {
            acc[i] += row[i] * c;
        }
    }
    acc
}

/// The two-dimensional inverse transform for one `N`x`N` block.
///
/// `block` is row-major with `d[x][y]` at `block[y * N + x]`; it is overwritten
/// with the residual `r[x][y]` at the same position.
///
/// With `BOUNDED` set, both stages sum only over the rectangle that holds the
/// non-zero coefficients. A 4x4 block is too small for the scan to pay for
/// itself, so it runs the full, branch-free matrix product instead; the two
/// give identical results because the skipped terms are all zero.
fn two_stage<const N: usize, const BOUNDED: bool>(
    block: &mut [i32],
    m: &[[i32; N]; N],
    bd_shift: u32,
) {
    let b = &mut block[..N * N];
    let (rows, cols) = if BOUNDED { extent(b, N) } else { (N, N) };
    let rnd = 1i32 << (bd_shift - 1);
    if BOUNDED && rows == 1 {
        // Only the first coefficient row is non-zero, so the column stage
        // gives every row of g the same values, m[0][y] * d[x] with m[0][y]
        // equal to m[0][0] for every y of the DCT. The row stage therefore
        // produces one residual row, repeated down the block.
        let mut g0 = [0i32; N];
        for (g, &c) in g0.iter_mut().zip(b.iter()).take(cols) {
            *g = clip_coeff((m[0][0] * c + (1 << (STAGE1_SHIFT - 1))) >> STAGE1_SHIFT);
        }
        let mut row = mul_1d::<N, true>(m, &g0, cols);
        for v in row.iter_mut() {
            *v = (*v + rnd) >> bd_shift;
        }
        for out in b.chunks_exact_mut(N) {
            out.copy_from_slice(&row);
        }
        return;
    }
    // g[y][x], the clipped output of the column stage. Columns at or past
    // `cols` have an all-zero source, so they stay zero and are not computed;
    // the second stage is then told to stop summing there.
    let mut g = [[0i32; N]; N];
    let mut col = [0i32; N];
    for x in 0..cols {
        for j in 0..rows {
            col[j] = b[j * N + x];
        }
        let e = mul_1d::<N, BOUNDED>(m, &col, rows);
        for y in 0..N {
            g[y][x] = clip_coeff((e[y] + (1 << (STAGE1_SHIFT - 1))) >> STAGE1_SHIFT);
        }
    }
    for y in 0..N {
        if BOUNDED && cols == 1 {
            // One column of g means every sample of row y is m[0][i] * g[y][0]
            // with m[0][i] flat across the DCT's first basis row.
            b[y * N..y * N + N].fill((m[0][0] * g[y][0] + rnd) >> bd_shift);
            continue;
        }
        let r = mul_1d::<N, BOUNDED>(m, &g[y], cols);
        for i in 0..N {
            b[y * N + i] = (r[i] + rnd) >> bd_shift;
        }
    }
}

/// Applies the two-dimensional inverse transform in place (clause 8.6.4.2)
/// followed by the final `bdShift` of clause 8.6.2.
///
/// `block` holds the scaled transform coefficients d[x][y] at
/// `block[y * n + x]` and receives the residual samples r[x][y].
/// `tr_type` is 0 for DCT-II and 1 for the 4x4 DST-VII.
///
/// `block` is left untouched when `n` is not 4, 8, 16 or 32, when `tr_type` is
/// 1 and `n` is not 4, when `bit_depth` is outside 8..=16, or when `block` is
/// shorter than `n * n`.
pub fn inverse_transform(block: &mut [i32], n: usize, tr_type: u8, bit_depth: u8) {
    let bd_shift = match bd_shift_of(bit_depth) {
        Some(s) => s,
        None => return,
    };
    match (tr_type, n) {
        (1, 4) if block.len() >= 16 => two_stage::<4, false>(block, &MDST, bd_shift),
        (0, 4) if block.len() >= 16 => two_stage::<4, false>(block, &M4, bd_shift),
        (0, 8) if block.len() >= 64 => two_stage::<8, true>(block, &M8, bd_shift),
        (0, 16) if block.len() >= 256 => two_stage::<16, true>(block, &M16, bd_shift),
        (0, 32) if block.len() >= 1024 => two_stage::<32, true>(block, &M32, bd_shift),
        _ => {}
    }
}

/// Transform-skip residual derivation (clause 8.6.2) for one block.
///
/// `block` holds d[x][y] and receives r[x][y]. `rotate` is
/// `transform_skip_rotation_enabled_flag` (always false for Main profiles).
///
/// `block` is left untouched when `n` is not 4, 8, 16 or 32, when `bit_depth`
/// is outside 8..=16, or when `block` is shorter than `n * n`.
pub fn transform_skip(block: &mut [i32], n: usize, bit_depth: u8, rotate: bool) {
    let log2_n: u32 = match n {
        4 => 2,
        8 => 3,
        16 => 4,
        32 => 5,
        _ => return,
    };
    let bd_shift = match bd_shift_of(bit_depth) {
        Some(s) => s,
        None => return,
    };
    if block.len() < n * n {
        return;
    }
    let b = &mut block[..n * n];
    // Reading d[n-1-x][n-1-y] into r[x][y] is exactly a reversal of the
    // row-major block, since (n-1-y) * n + (n-1-x) == n * n - 1 - (y * n + x).
    if rotate {
        b.reverse();
    }
    let ts_shift = 5 + log2_n;
    let rnd = 1i32 << (bd_shift - 1);
    for v in b.iter_mut() {
        *v = ((*v << ts_shift) + rnd) >> bd_shift;
    }
}

#[cfg(test)]
#[path = "dct_ref.rs"]
mod reference;

#[cfg(test)]
#[path = "dct_tests.rs"]
mod tests;
