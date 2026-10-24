//! The bounding rectangle of a block's non-zero coefficients.
//!
//! Residual coding sends a last-significant-coefficient position and nothing
//! beyond it, so for anything but the flattest content most of a 16x16 or
//! 32x32 block is zero. The scaling of clause 8.6.3 maps a zero level to a
//! zero coefficient, and both 1-D stages of clause 8.6.4.2 sum over an index
//! that those zeros index directly, so bounding either by this rectangle
//! removes work rather than approximating it: the result is bit-identical.

/// Widest block any transform accepts.
const MAX_N: usize = 32;

/// The smallest `(rows, cols)` rectangle at the top-left corner of the
/// row-major `n`x`n` block `b` that holds every non-zero entry.
///
/// Only the first `n * n` entries are read; `(0, 0)` means the block is all
/// zero. An `n` outside `1..=32` reports the whole block, which is always
/// safe for a caller to sum over.
///
/// Each row is OR-folded into a per-column mask instead of being searched,
/// which keeps the pass a straight vectorisable sweep with no data-dependent
/// branch inside the inner loop.
pub(super) fn extent(b: &[i32], n: usize) -> (usize, usize) {
    if n == 0 || n > MAX_N {
        return (n, n);
    }
    let mut col_mask = [0i32; MAX_N];
    let cm = &mut col_mask[..n];
    let mut rows = 0usize;
    for (y, row) in b.chunks_exact(n).take(n).enumerate() {
        let mut any = 0i32;
        for (m, &v) in cm.iter_mut().zip(row) {
            *m |= v;
            any |= v;
        }
        if any != 0 {
            rows = y + 1;
        }
    }
    let cols = cm.iter().rposition(|&v| v != 0).map_or(0, |x| x + 1);
    (rows, cols)
}

#[cfg(test)]
mod tests {
    use super::extent;
    use alloc::vec;

    /// The literal definition: one past the last non-zero row and column.
    fn slow(b: &[i32], n: usize) -> (usize, usize) {
        let (mut rows, mut cols) = (0, 0);
        for y in 0..n {
            for x in 0..n {
                if b[y * n + x] != 0 {
                    rows = rows.max(y + 1);
                    cols = cols.max(x + 1);
                }
            }
        }
        (rows, cols)
    }

    #[test]
    fn matches_the_literal_definition_on_sparse_blocks() {
        let mut s = 0x9E37_79B9u32;
        for &n in &[4usize, 8, 16, 32] {
            for _ in 0..200 {
                let mut b = vec![0i32; n * n];
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                let count = (s % 6) as usize;
                for _ in 0..count {
                    s ^= s << 13;
                    s ^= s >> 17;
                    s ^= s << 5;
                    let i = (s as usize) % (n * n);
                    b[i] = (s as i32 >> 8).max(1);
                }
                assert_eq!(extent(&b, n), slow(&b, n), "n={n} count={count}");
            }
        }
    }

    #[test]
    fn all_zero_reports_an_empty_rectangle() {
        for &n in &[4usize, 8, 16, 32] {
            assert_eq!(extent(&vec![0i32; n * n], n), (0, 0));
        }
    }

    #[test]
    fn a_negative_entry_counts_as_non_zero() {
        let mut b = vec![0i32; 64];
        b[7 * 8 + 3] = -1;
        assert_eq!(extent(&b, 8), (8, 4));
    }

    #[test]
    fn entries_past_n_squared_are_ignored() {
        let mut b = vec![0i32; 20];
        b[16] = 5;
        assert_eq!(extent(&b, 4), (0, 0));
    }

    #[test]
    fn a_short_block_reports_only_the_rows_present() {
        let b = [0, 0, 0, 0, 0, 1, 0, 0, 9];
        assert_eq!(extent(&b, 4), (2, 2));
    }

    #[test]
    fn a_size_outside_the_transforms_reports_the_whole_block() {
        assert_eq!(extent(&[1, 2, 3], 0), (0, 0));
        assert_eq!(extent(&[1, 2, 3], 64), (64, 64));
    }
}
