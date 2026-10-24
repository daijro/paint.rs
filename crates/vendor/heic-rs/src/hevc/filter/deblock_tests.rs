//! Tests for the deblocking filter, checked against a slow reference filter.

use super::*;
use crate::hevc::filter::CtbFilter;

const W: usize = 16;
const H: usize = 16;

/// One 16x16 luma-only CTB, deblocking enabled unless `disabled`.
fn mk_info(disabled: bool) -> FilterInfo {
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
        sao_enabled: false,
        ctb: vec![CtbFilter {
            deblock_disabled: disabled,
            ..CtbFilter::default()
        }],
        tile_id: vec![0],
        slice_addr: vec![0],
    }
}

/// A vertical step from `left` to `right` at x == 8, at quantiser `qp`.
fn mk_pic(left: u16, right: u16, qp: i8, mark: bool) -> Picture {
    let mut p = Picture::new(W, H, 0, 0).expect("picture");
    for y in 0..H {
        for x in 0..W {
            p.y.put(x, y, if x < 8 { left } else { right });
        }
    }
    for v in p.qp_y.iter_mut() {
        *v = qp;
    }
    if mark {
        for y in (0..H).step_by(8) {
            p.mark_edge(8, y, 8, edge::VER);
        }
    }
    p
}

/// Slow transcription of clauses 8.7.2.5.3 and 8.7.2.5.7 for one segment.
fn ref_segment(sm: &[[i32; 8]; 4], qp: i32) -> [[i32; 8]; 4] {
    let beta = BETA_TABLE[qp.clamp(0, 51) as usize];
    let tc = TC_TABLE[(qp + 2).clamp(0, 53) as usize];
    let mut dp = [0i32; 4];
    let mut dq = [0i32; 4];
    for i in 0..4 {
        dp[i] = (sm[i][1] - 2 * sm[i][2] + sm[i][3]).abs();
        dq[i] = (sm[i][6] - 2 * sm[i][5] + sm[i][4]).abs();
    }
    let mut out = *sm;
    if dp[0] + dq[0] + dp[3] + dq[3] >= beta {
        return out;
    }
    let mut strong = true;
    for i in [0usize, 3] {
        let (p3, p0, q0, q3) = (sm[i][0], sm[i][3], sm[i][4], sm[i][7]);
        strong &= 2 * (dp[i] + dq[i]) < beta / 4
            && (p3 - p0).abs() + (q0 - q3).abs() < beta / 8
            && (p0 - q0).abs() < (5 * tc + 1) / 2;
    }
    let lim = (beta + beta / 2) / 8;
    let (dep, deq) = (dp[0] + dp[3] < lim, dq[0] + dq[3] < lim);
    for i in 0..4 {
        let (p3, p2, p1, p0) = (sm[i][0], sm[i][1], sm[i][2], sm[i][3]);
        let (q0, q1, q2, q3) = (sm[i][4], sm[i][5], sm[i][6], sm[i][7]);
        if strong {
            out[i][3] = (p2 + 2 * p1 + 2 * p0 + 2 * q0 + q1 + 4) / 8;
            out[i][2] = (p2 + p1 + p0 + q0 + 2) / 4;
            out[i][1] = (2 * p3 + 3 * p2 + p1 + p0 + q0 + 4) / 8;
            out[i][4] = (p1 + 2 * p0 + 2 * q0 + 2 * q1 + q2 + 4) / 8;
            out[i][5] = (p0 + q0 + q1 + q2 + 2) / 4;
            out[i][6] = (p0 + q0 + q1 + 3 * q2 + 2 * q3 + 4) / 8;
            for (j, v) in out[i].iter_mut().enumerate() {
                *v = (*v).clamp(sm[i][j] - 2 * tc, sm[i][j] + 2 * tc);
            }
        } else {
            let raw = (9 * (q0 - p0) - 3 * (q1 - p1) + 8) >> 4;
            if raw.abs() < tc * 10 {
                let d = raw.clamp(-tc, tc);
                out[i][3] = (p0 + d).clamp(0, 255);
                out[i][4] = (q0 - d).clamp(0, 255);
                if dep {
                    let e = ((((p2 + p0 + 1) / 2) - p1 + d) >> 1).clamp(-(tc / 2), tc / 2);
                    out[i][2] = (p1 + e).clamp(0, 255);
                }
                if deq {
                    let e = ((((q2 + q0 + 1) / 2) - q1 - d) >> 1).clamp(-(tc / 2), tc / 2);
                    out[i][5] = (q1 + e).clamp(0, 255);
                }
            }
        }
    }
    out
}

/// Applies `ref_segment` to every four-line segment of the edge at x == 8.
fn ref_picture(src: &Picture, qp: i32) -> Vec<u16> {
    let mut out = src.y.data.clone();
    for s in (0..H).step_by(4) {
        let mut sm = [[0i32; 8]; 4];
        for (i, line) in sm.iter_mut().enumerate() {
            for (j, v) in line.iter_mut().enumerate() {
                *v = src.y.at(8 + j - 4, s + i) as i32;
            }
        }
        let f = ref_segment(&sm, qp);
        for (i, line) in f.iter().enumerate() {
            for (j, v) in line.iter().enumerate() {
                out[(s + i) * W + 8 + j - 4] = *v as u16;
            }
        }
    }
    out
}

#[test]
fn flat_picture_is_unchanged() {
    let mut p = mk_pic(100, 100, 37, true);
    deblock(&mut p, &mk_info(false));
    assert!(p.y.data.iter().all(|v| *v == 100));
}

#[test]
fn weak_filter_matches_reference() {
    let mut p = mk_pic(100, 160, 37, true);
    let want = ref_picture(&p, 37);
    deblock(&mut p, &mk_info(false));
    assert_eq!(p.y.data, want);
    assert_eq!(p.y.at(6, 0), 102);
    assert_eq!(p.y.at(7, 0), 105);
    assert_eq!(p.y.at(8, 0), 155);
    assert_eq!(p.y.at(9, 0), 158);
    assert_eq!(p.y.at(4, 0), 100);
}

#[test]
fn strong_filter_matches_reference() {
    let mut p = mk_pic(100, 108, 37, true);
    let want = ref_picture(&p, 37);
    deblock(&mut p, &mk_info(false));
    assert_eq!(p.y.data, want);
    assert_ne!(p.y.at(6, 3), 100);
    assert_ne!(p.y.at(9, 3), 108);
}

#[test]
fn disabled_slice_is_untouched() {
    let mut p = mk_pic(100, 160, 37, true);
    let before = p.y.data.clone();
    deblock(&mut p, &mk_info(true));
    assert_eq!(p.y.data, before);
}

#[test]
fn unmarked_edge_is_untouched() {
    let mut p = mk_pic(100, 160, 37, false);
    let before = p.y.data.clone();
    deblock(&mut p, &mk_info(false));
    assert_eq!(p.y.data, before);
}

#[test]
fn picture_boundary_is_untouched() {
    let mut p = mk_pic(100, 160, 37, true);
    for y in (0..H).step_by(8) {
        p.mark_edge(0, y, 8, edge::VER);
    }
    deblock(&mut p, &mk_info(false));
    assert_eq!(p.y.at(0, 0), 100);
    assert_eq!(p.y.at(1, 0), 100);
}

#[test]
fn bypass_on_p_side_keeps_p_samples() {
    let mut p = mk_pic(100, 160, 37, true);
    for y in 0..p.min4_h {
        let i = y * p.min4_w + 1;
        p.cu_flags[i] = flags::TQ_BYPASS;
    }
    deblock(&mut p, &mk_info(false));
    for y in 0..H {
        for x in 4..8 {
            assert_eq!(p.y.at(x, y), 100, "p side at {x},{y}");
        }
        assert_eq!(p.y.at(8, y), 155);
        assert_eq!(p.y.at(9, y), 158);
    }
}

/// `src` turned on its side, with its vertical edge marked as horizontal.
fn transposed(src: &Picture) -> Picture {
    let mut p = Picture::new(W, H, 0, 0).expect("picture");
    for y in 0..H {
        for x in 0..W {
            p.y.put(y, x, src.y.at(x, y));
        }
    }
    p.qp_y.copy_from_slice(&src.qp_y);
    for x in (0..W).step_by(8) {
        p.mark_edge(x, 8, 8, edge::HOR);
    }
    p
}

#[test]
fn horizontal_edge_is_the_transpose_of_the_vertical_one() {
    for &(left, right) in &[(100u16, 160u16), (100, 108)] {
        let mut v = mk_pic(left, right, 37, true);
        let mut h = transposed(&v);
        deblock(&mut v, &mk_info(false));
        deblock(&mut h, &mk_info(false));
        for y in 0..H {
            for x in 0..W {
                assert_eq!(h.y.at(y, x), v.y.at(x, y), "{left}->{right} at {x},{y}");
            }
        }
    }
}
