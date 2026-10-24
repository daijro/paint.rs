//! The chroma half of the deblocking filter, clause 8.7.2.5.5.
//!
//! Chroma is only ever filtered with the short two-tap form — there is no
//! strong filter and no `dE` derivation — so it shares nothing with the luma
//! path but the tables and the segment addressing, both of which stay in the
//! parent module.

use super::super::{FilterInfo, chroma_qp_from_luma};
use super::{BS, Seg, TC_TABLE, clip3, locked};
use crate::hevc::picture::{Picture, Plane, edge};

/// Filters one four-line chroma segment of both chroma planes.
fn segment(pic: &mut Picture, info: &FilterInfo, ver: bool, e: usize, s: usize) {
    let (sw, sh) = (info.sub_w, info.sub_h);
    let (lqx, lqy) = if ver {
        (e * sw, s * sh)
    } else {
        (s * sw, e * sh)
    };
    let (lpx, lpy) = if ver { (lqx - 1, lqy) } else { (lqx, lqy - 1) };
    let qc = info.ctb_of(lqx, lqy);
    let cf = match info.ctb.get(qc) {
        Some(c) => *c,
        None => return,
    };
    if cf.deblock_disabled || !info.may_cross(info.ctb_of(lpx, lpy), qc) {
        return;
    }
    let pl = locked(pic.cu_flags[pic.idx4(lpx, lpy)], info);
    let ql = locked(pic.cu_flags[pic.idx4(lqx, lqy)], info);
    if pl && ql {
        return;
    }
    let qpl = (pic.qp_y[pic.idx4(lqx, lqy)] as i32 + pic.qp_y[pic.idx4(lpx, lpy)] as i32 + 1) >> 1;
    let scale = 1i32 << info.bit_depth_c.saturating_sub(8);
    let max = (1i32 << info.bit_depth_c) - 1;
    for comp in 0..2 {
        let off = i32::from(if comp == 0 {
            cf.cb_qp_offset
        } else {
            cf.cr_qp_offset
        });
        let qpc = chroma_qp_from_luma(qpl + off, info.chroma_array_type);
        let qtc = clip3(
            0,
            53,
            qpc + 2 * (BS - 1) + ((cf.tc_offset_div2 as i32) << 1),
        );
        let tc = TC_TABLE[qtc as usize] * scale;
        let plane: &mut Plane = if comp == 0 { &mut pic.cb } else { &mut pic.cr };
        let seg = Seg::new(plane.stride, ver, e, s, 2);
        let data = &mut plane.data;
        for i in 0..4 {
            let (p1, p0) = (data[seg.at(i, 0)] as i32, data[seg.at(i, 1)] as i32);
            let (q0, q1) = (data[seg.at(i, 2)] as i32, data[seg.at(i, 3)] as i32);
            let d = clip3(-tc, tc, (((q0 - p0) << 2) + p1 - q1 + 4) >> 3);
            if !pl {
                data[seg.at(i, 1)] = clip3(0, max, p0 + d) as u16;
            }
            if !ql {
                data[seg.at(i, 2)] = clip3(0, max, q0 - d) as u16;
            }
        }
    }
}

/// Filters every marked chroma edge of one direction over the whole picture.
pub(super) fn dir(pic: &mut Picture, info: &FilterInfo, ver: bool) {
    if info.chroma_array_type == 0 || info.sub_w == 0 || info.sub_h == 0 {
        return;
    }
    let kind = if ver { edge::VER } else { edge::HOR };
    let (sw, sh) = (info.sub_w, info.sub_h);
    let (cw, ch) = (pic.cb.width, pic.cb.height);
    let (elen, slen) = if ver { (cw, ch) } else { (ch, cw) };
    let mut e = 8;
    while e + 1 < elen {
        let mut s = 0;
        while s + 3 < slen {
            let (lx, ly) = if ver {
                (e * sw, s * sh)
            } else {
                (s * sw, e * sh)
            };
            let cell = (ly >> 3) * pic.min8_w + (lx >> 3);
            if pic.edges.get(cell).map_or(0u8, |v| *v) & kind != 0 {
                segment(pic, info, ver, e, s);
            }
            s += 4;
        }
        e += 8;
    }
}
