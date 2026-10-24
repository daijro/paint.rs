//! Tests for sample adaptive offset.

use super::*;
use crate::hevc::filter::CtbFilter;

const W: usize = 16;
const H: usize = 16;

/// One 16x16 luma-only CTB carrying `s` for every component.
fn mk_info(s: SaoCtb, enabled: bool) -> FilterInfo {
    FilterInfo {
        bit_depth_y: 8,
        bit_depth_c: 8,
        chroma_array_type: 0,
        sub_w: 0,
        sub_h: 0,
        ctb_log2: 4,
        pic_w_ctbs: 1,
        pic_h_ctbs: 1,
        pcm_loop_filter_disabled: false,
        across_tiles: true,
        sao_enabled: enabled,
        ctb: vec![CtbFilter {
            sao: [s; 3],
            ..CtbFilter::default()
        }],
        tile_id: vec![0],
        slice_addr: vec![0],
    }
}

/// A 16x16 luma-only picture filled with `v`.
fn mk_pic(v: u16) -> Picture {
    let mut p = Picture::new(W, H, 0, 0).expect("picture");
    for s in p.y.data.iter_mut() {
        *s = v;
    }
    p
}

#[test]
fn band_offset_shifts_only_the_selected_bands() {
    let mut p = mk_pic(100);
    p.y.put(3, 3, 130);
    p.y.put(4, 4, 111);
    let s = SaoCtb {
        type_idx: 1,
        eo_class: 0,
        band_position: 12,
        offsets: [3, -2, 0, 0],
    };
    sao(&mut p, &mk_info(s, true));
    assert_eq!(p.y.at(0, 0), 103);
    assert_eq!(p.y.at(4, 4), 109);
    assert_eq!(p.y.at(3, 3), 130);
}

#[test]
fn disabled_sao_changes_nothing() {
    let mut p = mk_pic(100);
    let s = SaoCtb {
        type_idx: 1,
        eo_class: 0,
        band_position: 12,
        offsets: [3, 0, 0, 0],
    };
    sao(&mut p, &mk_info(s, false));
    assert!(p.y.data.iter().all(|v| *v == 100));
}

#[test]
fn edge_offset_on_flat_plane_is_a_noop() {
    let mut p = mk_pic(100);
    let s = SaoCtb {
        type_idx: 2,
        eo_class: 0,
        band_position: 0,
        offsets: [5, 5, 5, 5],
    };
    sao(&mut p, &mk_info(s, true));
    assert!(p.y.data.iter().all(|v| *v == 100));
}

#[test]
fn edge_offset_classifies_a_spike() {
    let mut p = mk_pic(100);
    p.y.put(8, 8, 200);
    let s = SaoCtb {
        type_idx: 2,
        eo_class: 0,
        band_position: 0,
        offsets: [1, 2, 3, 4],
    };
    sao(&mut p, &mk_info(s, true));
    assert_eq!(p.y.at(8, 8), 204);
    assert_eq!(p.y.at(7, 8), 102);
    assert_eq!(p.y.at(9, 8), 102);
    assert_eq!(p.y.at(6, 8), 100);
    assert_eq!(p.y.at(8, 7), 100);
}

#[test]
fn edge_offset_leaves_picture_border_alone() {
    let mut p = mk_pic(100);
    for y in 0..H {
        p.y.put(0, y, 50);
    }
    let s = SaoCtb {
        type_idx: 2,
        eo_class: 0,
        band_position: 0,
        offsets: [1, 2, 3, 4],
    };
    sao(&mut p, &mk_info(s, true));
    for y in 0..H {
        assert_eq!(p.y.at(0, y), 50, "column 0 row {y}");
        assert_eq!(p.y.at(1, y), 103, "column 1 row {y}");
        assert_eq!(p.y.at(15, y), 100, "column 15 row {y}");
    }
}

#[test]
fn bypassed_coding_unit_keeps_its_samples() {
    let mut p = mk_pic(100);
    p.cu_flags[0] = flags::TQ_BYPASS;
    let s = SaoCtb {
        type_idx: 1,
        eo_class: 0,
        band_position: 12,
        offsets: [3, 0, 0, 0],
    };
    sao(&mut p, &mk_info(s, true));
    assert_eq!(p.y.at(0, 0), 100);
    assert_eq!(p.y.at(3, 3), 100);
    assert_eq!(p.y.at(4, 0), 103);
}
