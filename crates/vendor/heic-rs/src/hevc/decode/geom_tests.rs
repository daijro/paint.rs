//! Tests for tile geometry and the z-scan address table (clause 6.5).

use super::Geometry;
use crate::hevc::ps::{Pps, Sps};

fn sps(width: usize, height: usize, log2_ctb: usize) -> Sps {
    Sps {
        id: 0,
        separate_colour_plane: false,
        chroma_array_type: 1,
        sub_w: 2,
        sub_h: 2,
        width,
        height,
        crop: [0; 4],
        bit_depth_y: 8,
        bit_depth_c: 8,
        log2_max_poc_lsb: 8,
        log2_min_cb: 3,
        log2_ctb,
        log2_min_tb: 2,
        log2_max_tb: 5,
        max_tr_depth_intra: 0,
        scaling_list_enabled: false,
        scaling: None,
        sao_enabled: false,
        pcm_enabled: false,
        pcm_bit_depth_y: 8,
        pcm_bit_depth_c: 8,
        log2_min_pcm_cb: 3,
        log2_max_pcm_cb: 3,
        pcm_loop_filter_disabled: false,
        num_st_rps: 0,
        num_delta_pocs: Vec::new(),
        long_term_present: false,
        num_lt_sps: 0,
        temporal_mvp: false,
        strong_intra_smoothing: false,
        intra_smoothing_disabled: false,
        persistent_rice: false,
        transform_skip_context: false,
        transform_skip_rotation: false,
        implicit_rdpcm: false,
        video_signal: None,
    }
}

#[test]
fn without_tiles_the_scans_are_the_identity() {
    let s = sps(256, 128, 5);
    let p = Pps {
        num_tile_cols: 1,
        num_tile_rows: 1,
        uniform_spacing: true,
        ..Default::default()
    };
    let g = Geometry::new(&s, &p).expect("geometry");
    assert_eq!((g.w_ctbs, g.h_ctbs), (8, 4));
    for rs in 0..g.size_ctbs {
        assert_eq!(g.rs_to_ts[rs] as usize, rs);
        assert_eq!(g.ts_to_rs[rs] as usize, rs);
        assert_eq!(g.tile_id[rs], 0);
    }
}

#[test]
fn uniform_tiles_reorder_the_tile_scan() {
    let s = sps(256, 128, 5); // 8x4 coding tree blocks
    let p = Pps {
        tiles_enabled: true,
        num_tile_cols: 2,
        num_tile_rows: 2,
        uniform_spacing: true,
        ..Default::default()
    };
    let g = Geometry::new(&s, &p).expect("geometry");
    assert_eq!(g.col_width, vec![4, 4]);
    assert_eq!(g.row_height, vec![2, 2]);
    // The first tile holds the top left 4x2 block of CTBs in raster order.
    assert_eq!(g.rs_to_ts[0], 0);
    assert_eq!(g.rs_to_ts[3], 3);
    assert_eq!(g.rs_to_ts[8], 4); // second CTB row, still tile 0
    assert_eq!(g.rs_to_ts[4], 8); // first CTB of tile 1
    assert_eq!(g.tile_id[4], 1);
    assert_eq!(g.tile_id[16], 2);
    assert_eq!(g.tile_id[20], 3);
    for ts in 0..g.size_ctbs {
        assert_eq!(g.rs_to_ts[g.ts_to_rs[ts] as usize] as usize, ts);
    }
}

#[test]
fn explicit_tile_spacing_uses_the_signalled_widths() {
    let s = sps(256, 128, 5);
    let p = Pps {
        tiles_enabled: true,
        num_tile_cols: 3,
        num_tile_rows: 1,
        uniform_spacing: false,
        column_widths: vec![1, 5],
        ..Default::default()
    };
    let g = Geometry::new(&s, &p).expect("geometry");
    assert_eq!(g.col_width, vec![1, 5, 2]);
    assert_eq!(g.tile_id[0], 0);
    assert_eq!(g.tile_id[1], 1);
    assert_eq!(g.tile_id[6], 2);
}

#[test]
fn z_scan_addresses_increase_along_the_quad_tree_order() {
    let s = sps(64, 64, 6); // one 64x64 coding tree block
    let p = Pps {
        num_tile_cols: 1,
        num_tile_rows: 1,
        uniform_spacing: true,
        ..Default::default()
    };
    let g = Geometry::new(&s, &p).expect("geometry");
    // Within one CTB the z order visits quadrants top left, top right,
    // bottom left, bottom right.
    assert_eq!(g.z(0, 0), 0);
    assert_eq!(g.z(4, 0), 1);
    assert_eq!(g.z(0, 4), 2);
    assert_eq!(g.z(4, 4), 3);
    assert_eq!(g.z(32, 0), 64);
    assert_eq!(g.z(0, 32), 128);
    assert_eq!(g.z(32, 32), 192);
    let mut seen = vec![false; 256];
    for y in (0..64).step_by(4) {
        for x in (0..64).step_by(4) {
            let z = g.z(x, y) as usize;
            assert!(!seen[z]);
            seen[z] = true;
        }
    }
}

#[test]
fn degenerate_tile_configurations_are_rejected() {
    let s = sps(64, 64, 5); // 2x2 coding tree blocks
    let p = Pps {
        tiles_enabled: true,
        num_tile_cols: 4,
        num_tile_rows: 1,
        uniform_spacing: true,
        ..Default::default()
    };
    assert!(Geometry::new(&s, &p).is_err());
}
