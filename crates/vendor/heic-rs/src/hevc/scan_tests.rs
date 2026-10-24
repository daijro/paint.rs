//! Tests for the compile-time scan order tables (clause 6.5.3).

use super::{SUB_RASTER, scan_order, scan_pos, sub_scan};

#[test]
fn scan_pos_inverts_every_scan() {
    for log2 in 0..4usize {
        let n = 1usize << log2;
        for idx in 0..3usize {
            let s = scan_order(log2, idx);
            for (i, p) in s[..n * n].iter().enumerate() {
                let (x, y) = (p[0] as usize, p[1] as usize);
                assert_eq!(
                    scan_pos(log2, idx, x, y),
                    i,
                    "scan {idx} size {n} at ({x}, {y})"
                );
            }
        }
    }
}

#[test]
fn sub_raster_is_the_sub_block_scan_in_raster_form() {
    for (idx, raster) in SUB_RASTER.iter().enumerate() {
        for (m, p) in sub_scan(idx)[..16].iter().enumerate() {
            assert_eq!(raster[m], (p[1] << 2) | p[0], "scan {idx} position {m}");
        }
    }
}

#[test]
fn every_scan_is_a_permutation_of_the_block() {
    for log2 in 0..4usize {
        let n = 1usize << log2;
        for idx in 0..3usize {
            let s = scan_order(log2, idx);
            let mut seen = vec![false; n * n];
            for p in &s[..n * n] {
                let (x, y) = (p[0] as usize, p[1] as usize);
                assert!(x < n && y < n, "scan {idx} size {n} leaves the block");
                assert!(!seen[y * n + x], "scan {idx} size {n} repeats a position");
                seen[y * n + x] = true;
            }
            assert!(seen.iter().all(|&v| v));
        }
    }
}

#[test]
fn diagonal_scan_matches_the_generated_order() {
    // Clause 6.5.3 walks up-right diagonals starting from the top left.
    for log2 in 1..4usize {
        let n = 1usize << log2;
        let mut want = Vec::new();
        for d in 0..2 * n - 1 {
            for y in (0..=d).rev() {
                let x = d - y;
                if x < n && y < n {
                    want.push([x as u8, y as u8]);
                }
            }
        }
        assert_eq!(&scan_order(log2, 0)[..n * n], &want[..]);
    }
}

#[test]
fn horizontal_and_vertical_scans_are_transposes() {
    for log2 in 0..4usize {
        let n = 1usize << log2;
        let h = scan_order(log2, 1);
        let v = scan_order(log2, 2);
        for y in 0..n {
            for x in 0..n {
                assert_eq!(h[y * n + x], [x as u8, y as u8]);
                assert_eq!(v[x * n + y], [x as u8, y as u8]);
            }
        }
    }
}

#[test]
fn the_sub_block_scan_is_the_four_by_four_scan() {
    for idx in 0..3usize {
        assert_eq!(sub_scan(idx)[..16], scan_order(2, idx)[..16]);
    }
    // The 4x4 diagonal scan opens with the documented order.
    assert_eq!(
        &sub_scan(0)[..6],
        &[[0, 0], [0, 1], [1, 0], [0, 2], [1, 1], [2, 0]]
    );
}
