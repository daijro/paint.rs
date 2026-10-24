//! Sample adaptive offset, ITU-T H.265 clause 8.7.3.
//!
//! SAO reads the deblocked picture and writes a second picture, so the filter
//! works from a snapshot of every plane taken before the first sample changes.

use super::deblock::clip3;
use super::{FilterInfo, SaoCtb};
use crate::hevc::picture::{Picture, Plane, flags};
use alloc::vec::Vec;

/// Neighbour offsets of each `SaoEoClass`, clause 8.7.3.2.
const EO: [[(isize, isize); 2]; 4] = [
    [(-1, 0), (1, 0)],
    [(0, -1), (0, 1)],
    [(-1, -1), (1, 1)],
    [(1, -1), (-1, 1)],
];

/// `SaoTypeIdx` selecting band offset.
const BAND: u8 = 1;
/// `SaoTypeIdx` selecting edge offset.
const EDGE: u8 = 2;

/// Offset to add to the sample at `(px, py)`, or `None` to leave it alone.
///
/// `sw` and `sh` scale component coordinates back to luma coordinates so that
/// the CTB of a neighbour can be found; `addr` is the CTB being filtered.
#[allow(clippy::too_many_arguments)]
fn edge_offset(
    src: &Plane,
    info: &FilterInfo,
    s: &SaoCtb,
    addr: usize,
    px: usize,
    py: usize,
    sw: usize,
    sh: usize,
) -> Option<i32> {
    let rec = src.at(px, py) as i32;
    let mut idx = 2i32;
    for (dx, dy) in EO[(s.eo_class & 3) as usize] {
        let nx = px as isize + dx;
        let ny = py as isize + dy;
        if nx < 0 || ny < 0 || nx as usize >= src.width || ny as usize >= src.height {
            return None;
        }
        let (nx, ny) = (nx as usize, ny as usize);
        if !info.may_cross(info.ctb_of(nx * sw, ny * sh), addr) {
            return None;
        }
        idx += (rec - src.at(nx, ny) as i32).signum();
    }
    let idx = match idx {
        2 => 0,
        0 => 1,
        1 => 2,
        v => v,
    };
    if idx == 0 {
        return Some(0);
    }
    s.offsets.get(idx as usize - 1).map(|v| *v as i32)
}

/// Applies sample adaptive offset to one component of one coding tree block.
#[allow(clippy::too_many_arguments)]
fn sao_ctb(
    dst: &mut Plane,
    src: &Plane,
    cu_flags: &[u8],
    min4_w: usize,
    info: &FilterInfo,
    s: SaoCtb,
    addr: usize,
    sw: usize,
    sh: usize,
    bd: u8,
) {
    let (cx, cy) = (addr % info.pic_w_ctbs, addr / info.pic_w_ctbs);
    let x0 = (cx << info.ctb_log2) / sw;
    let y0 = (cy << info.ctb_log2) / sh;
    let x1 = (((cx + 1) << info.ctb_log2) / sw).min(dst.width);
    let y1 = (((cy + 1) << info.ctb_log2) / sh).min(dst.height);
    let max = (1i32 << bd) - 1;
    for py in y0..y1 {
        for px in x0..x1 {
            let (lx, ly) = (px * sw, py * sh);
            let f = cu_flags
                .get((ly >> 2) * min4_w + (lx >> 2))
                .map_or(0u8, |v| *v);
            if f & flags::TQ_BYPASS != 0 || (f & flags::PCM != 0 && info.pcm_loop_filter_disabled) {
                continue;
            }
            let rec = src.at(px, py) as i32;
            let delta = if s.type_idx == BAND {
                let k = (rec >> bd.saturating_sub(5)) - s.band_position as i32;
                match s.offsets.get(k as usize) {
                    Some(v) if (0..4).contains(&k) => *v as i32,
                    _ => continue,
                }
            } else {
                match edge_offset(src, info, &s, addr, px, py, sw, sh) {
                    Some(v) => v,
                    None => continue,
                }
            };
            dst.put(px, py, clip3(0, max, rec + delta) as u16);
        }
    }
}

/// Applies the sample adaptive offset filter of clause 8.7.3 to `pic` in place.
///
/// Does nothing unless `info.sao_enabled`; coding tree blocks whose
/// `SaoTypeIdx` is zero keep their deblocked samples.
pub fn sao(pic: &mut Picture, info: &FilterInfo) {
    if !info.sao_enabled || info.pic_w_ctbs == 0 {
        return;
    }
    let ncomp = if info.chroma_array_type == 0 { 1 } else { 3 };
    let n = info.ctb.len().min(info.pic_w_ctbs * info.pic_h_ctbs);
    // A component that no CTB applies SAO to keeps its deblocked samples, so
    // its snapshot would never be read. Not taking it is what makes a picture
    // that merely signals SAO in its parameter sets cost nothing here.
    let used = |c: usize| {
        info.ctb[..n]
            .iter()
            .any(|f| f.sao[c].type_idx == BAND || f.sao[c].type_idx == EDGE)
    };
    let snapshot = |c: usize, p: &Plane| {
        if c < ncomp && used(c) {
            p.clone()
        } else {
            Plane {
                data: Vec::new(),
                stride: 0,
                width: 0,
                height: 0,
            }
        }
    };
    let src = [
        snapshot(0, &pic.y),
        snapshot(1, &pic.cb),
        snapshot(2, &pic.cr),
    ];
    let Picture {
        y,
        cb,
        cr,
        cu_flags,
        min4_w,
        ..
    } = pic;
    for addr in 0..n {
        for (c, plane_src) in src.iter().enumerate().take(ncomp) {
            let s = info.ctb[addr].sao[c];
            if s.type_idx != BAND && s.type_idx != EDGE {
                continue;
            }
            let (sw, sh) = if c == 0 {
                (1, 1)
            } else {
                (info.sub_w.max(1), info.sub_h.max(1))
            };
            let bd = if c == 0 {
                info.bit_depth_y
            } else {
                info.bit_depth_c
            };
            let dst: &mut Plane = match c {
                0 => &mut *y,
                1 => &mut *cb,
                _ => &mut *cr,
            };
            sao_ctb(dst, plane_src, cu_flags, *min4_w, info, s, addr, sw, sh, bd);
        }
    }
}

#[cfg(test)]
#[path = "sao_tests.rs"]
mod tests;
