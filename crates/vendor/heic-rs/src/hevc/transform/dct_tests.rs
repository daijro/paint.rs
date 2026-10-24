//! Tests for clause 8.6.4.2 / 8.6.2, comparing the monomorphised fast path
//! against the slow reference in `dct_ref.rs`.

use super::reference::*;
use super::*;
use core::f64::consts::PI;

#[test]
fn fast_path_matches_reference_on_random_blocks() {
    let mut rng = Rng::new(0x1234_5678);
    for &n in &[4usize, 8, 16, 32] {
        for &tr_type in &[0u8, 1] {
            if tr_type == 1 && n != 4 {
                continue;
            }
            for &bit_depth in &[8u8, 10] {
                for _ in 0..200 {
                    let d: Vec<i32> = (0..n * n).map(|_| rng.coeff()).collect();
                    check(&d, n, tr_type, bit_depth, "random");
                }
            }
        }
    }
}

#[test]
fn fast_path_matches_reference_on_edge_blocks() {
    for &n in &[4usize, 8, 16, 32] {
        for &tr_type in &[0u8, 1] {
            if tr_type == 1 && n != 4 {
                continue;
            }
            for &bit_depth in &[8u8, 10] {
                check(&vec![0i32; n * n], n, tr_type, bit_depth, "zero");
                check(&vec![-32768i32; n * n], n, tr_type, bit_depth, "coeff_min");
                check(&vec![32767i32; n * n], n, tr_type, bit_depth, "coeff_max");
                let mut dc = vec![0i32; n * n];
                dc[0] = 32767;
                check(&dc, n, tr_type, bit_depth, "dc_only");
                let mut hi = vec![0i32; n * n];
                hi[n * n - 1] = -32768;
                check(&hi, n, tr_type, bit_depth, "highest_freq");
            }
        }
    }
}

#[test]
fn first_row_only_and_first_column_only_blocks_match_the_reference() {
    // The collapsed paths of `two_stage` rest on the DCT's flat first basis
    // row; this pins them to the literal transcription for every size.
    let mut rng = Rng::new(0x0BAD_CAFE);
    for &n in &[8usize, 16, 32] {
        for &bit_depth in &[8u8, 10] {
            for _ in 0..40 {
                let cols = 1 + (rng.next_u32() as usize % n);
                let mut row_only = vec![0i32; n * n];
                for v in row_only.iter_mut().take(cols) {
                    *v = rng.coeff();
                }
                check(&row_only, n, 0, bit_depth, "first_row_only");
                let rows = 1 + (rng.next_u32() as usize % n);
                let mut col_only = vec![0i32; n * n];
                for y in 0..rows {
                    col_only[y * n] = rng.coeff();
                }
                check(&col_only, n, 0, bit_depth, "first_column_only");
            }
        }
    }
}

#[test]
fn dct_matrix_matches_float_basis() {
    // 64 * sqrt(2), the gain of the HEVC integer basis.
    const SCALE: f64 = 90.50966799187808;
    for (n, &v) in DCT_MATRIX[0].iter().enumerate() {
        assert_eq!(v, 64, "row 0 must be flat at n={n}");
    }
    for (k, row) in DCT_MATRIX.iter().enumerate().skip(1) {
        for (n, &entry) in row.iter().enumerate() {
            let ideal = SCALE * (PI * k as f64 * (2 * n + 1) as f64 / 64.0).cos();
            let err = (entry as f64 - ideal).abs();
            // The standard's table is not simply the ideal basis rounded: the
            // two magnitudes 36 and 25 sit ~1.36 and ~1.27 away from it (the
            // ideal values are 34.64 and 26.27), a deliberate adjustment that
            // keeps the integer matrix close to orthogonal. Every other entry
            // is within one of the ideal value.
            let tol = if entry.abs() == 36 || entry.abs() == 25 {
                1.4
            } else {
                1.0
            };
            assert!(
                err <= tol,
                "k={k} n={n} entry={entry} ideal={ideal} err={err}"
            );
        }
    }
}

#[test]
fn row_subsampling_gives_the_textbook_small_matrices() {
    let m4: Vec<Vec<i16>> = (0..4)
        .map(|k| (0..4).map(|n| DCT_MATRIX[k * 8][n]).collect())
        .collect();
    assert_eq!(m4[0], vec![64, 64, 64, 64]);
    assert_eq!(m4[1], vec![83, 36, -36, -83]);
    assert_eq!(m4[2], vec![64, -64, -64, 64]);
    assert_eq!(m4[3], vec![36, -83, 83, -36]);

    let m8row = |k: usize| -> Vec<i16> { (0..8).map(|n| DCT_MATRIX[k * 4][n]).collect() };
    assert_eq!(m8row(1), vec![89, 75, 50, 18, -18, -50, -75, -89]);
    assert_eq!(m8row(3), vec![75, -18, -89, -50, 50, 89, 18, -75]);
}

#[test]
fn dc_only_coefficient_gives_a_flat_residual() {
    for &n in &[4usize, 8, 16, 32] {
        for &v in &[1i32, -1, 100, -4096, 32767, -32768] {
            let mut b = vec![0i32; n * n];
            b[0] = v;
            inverse_transform(&mut b, n, 0, 8);
            let g = ((64 * v + 64) >> 7).clamp(-32768, 32767);
            let expected = (64 * g + (1 << 11)) >> 12;
            for (i, &s) in b.iter().enumerate() {
                assert_eq!(s, expected, "n={n} v={v} i={i}");
            }
        }
    }
}

#[test]
fn transform_skip_matches_reference() {
    let mut rng = Rng::new(0x9E37_79B9);
    for &n in &[4usize, 8, 16, 32] {
        for &bit_depth in &[8u8, 10] {
            for &rotate in &[false, true] {
                for _ in 0..50 {
                    let d: Vec<i32> = (0..n * n).map(|_| rng.coeff()).collect();
                    let expected = ref_skip(&d, n, bit_depth, rotate);
                    let mut got = d.clone();
                    transform_skip(&mut got, n, bit_depth, rotate);
                    assert_eq!(got, expected, "n={n} bd={bit_depth} rot={rotate}");
                }
            }
        }
    }
}

#[test]
fn invalid_arguments_leave_the_block_untouched() {
    let original = vec![7i32; 64];
    for &(n, tr, bd, len) in &[
        (5usize, 0u8, 8u8, 64usize),
        (8, 0, 7, 64),
        (8, 0, 17, 64),
        (8, 1, 8, 64),
        (8, 0, 8, 63),
    ] {
        let mut b = original[..len].to_vec();
        inverse_transform(&mut b, n, tr, bd);
        assert_eq!(
            b,
            &original[..len],
            "inverse n={n} tr={tr} bd={bd} len={len}"
        );
    }
    for &(n, bd, len) in &[(5usize, 8u8, 64usize), (8, 7, 64), (8, 17, 64), (8, 8, 63)] {
        let mut c = original[..len].to_vec();
        transform_skip(&mut c, n, bd, false);
        assert_eq!(c, &original[..len], "skip n={n} bd={bd} len={len}");
    }
}
