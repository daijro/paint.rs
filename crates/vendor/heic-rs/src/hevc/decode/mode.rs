//! Intra prediction mode derivation (clauses 8.4.2 and 8.4.3).

use super::state::Dec;
use crate::hevc::cabac::off;
use crate::hevc::error::Result;
use crate::hevc::picture::{Picture, flags};

/// `INTRA_PLANAR`.
const PLANAR: u8 = 0;
/// `INTRA_DC`.
const DC: u8 = 1;
/// `INTRA_ANGULAR26`, the vertical mode.
const VER: u8 = 26;

/// Table 8-3, mapping a chroma mode to its 4:2:2 equivalent.
const MAP_422: [u8; 35] = [
    0, 1, 2, 2, 2, 2, 3, 5, 7, 8, 10, 12, 13, 15, 16, 18, 20, 22, 23, 25, 27, 29, 31, 33, 34, 34,
    34, 34, 34, 34, 34, 34, 34, 34, 34,
];

/// Parses the luma intra prediction modes of a coding unit and derives them.
pub fn luma_mode(d: &mut Dec<'_>, x0: usize, y0: usize, log2_size: usize) -> Result<()> {
    let size = 1usize << log2_size;
    let n = if d.intra_split { 2usize } else { 1 };
    let off_pb = size / n;
    let mut prev = [false; 4];
    for p in prev.iter_mut().take(n * n) {
        *p = d.cab.decision(off::PREV_INTRA) != 0;
    }
    let mut sel = [0u32; 4];
    for k in 0..n * n {
        let v = if prev[k] {
            let mut idx = 0u32;
            if d.cab.bypass() == 1 {
                idx = 1 + d.cab.bypass();
            }
            idx
        } else {
            d.cab.bypass_bits(5)
        };
        sel[k] = v;
    }
    for k in 0..n * n {
        let x = x0 + (k % n) * off_pb;
        let y = y0 + (k / n) * off_pb;
        let cands = candidates(d, x, y);
        let mode = if prev[k] {
            cands[sel[k].min(2) as usize]
        } else {
            let mut c = cands;
            c.sort_unstable();
            let mut m = sel[k] as u8;
            for &cand in c.iter() {
                if m >= cand {
                    m += 1;
                }
            }
            m.min(34)
        };
        d.mode_y[k] = mode;
        Picture::fill4(&mut d.pic.intra_mode, d.pic.min4_w, x, y, off_pb, mode);
    }
    Ok(())
}

/// Derives `candModeList` for the prediction block at `(x, y)` (clause 8.4.2).
fn candidates(d: &Dec<'_>, x: usize, y: usize) -> [u8; 3] {
    let ctb_mask = (1usize << d.sps.log2_ctb) - 1;
    let a = neighbour_mode(d, x, y, x as isize - 1, y as isize, false);
    let b = neighbour_mode(d, x, y, x as isize, y as isize - 1, y & ctb_mask == 0);
    if a == b {
        if a < 2 {
            [PLANAR, DC, VER]
        } else {
            [a, 2 + ((a + 29) % 32), 2 + ((a - 2 + 1) % 32)]
        }
    } else {
        let third = if a != PLANAR && b != PLANAR {
            PLANAR
        } else if a != DC && b != DC {
            DC
        } else {
            VER
        };
        [a, b, third]
    }
}

/// `candIntraPredModeX` for one neighbour.
fn neighbour_mode(d: &Dec<'_>, x: usize, y: usize, nx: isize, ny: isize, above_ctb: bool) -> u8 {
    if above_ctb || !d.available(x, y, nx, ny) {
        return DC;
    }
    let (nx, ny) = (nx as usize, ny as usize);
    if d.pic.cu_flags[d.pic.idx4(nx, ny)] & flags::PCM != 0 {
        return DC;
    }
    d.mode_at(nx, ny)
}

/// Parses `intra_chroma_pred_mode` and derives `IntraPredModeC` (clause 8.4.3).
pub fn chroma_mode(d: &mut Dec<'_>, _log2_size: usize) -> Result<()> {
    if d.cat == 0 {
        return Ok(());
    }
    let n = if d.cat == 3 && d.intra_split { 4 } else { 1 };
    for k in 0..n {
        let raw = if d.cab.decision(off::INTRA_CHROMA) == 0 {
            4
        } else {
            d.cab.bypass_bits(2)
        };
        let luma = d.mode_y[if n == 4 { k } else { 0 }];
        let mut m = match raw {
            0 => PLANAR,
            1 => VER,
            2 => 10,
            3 => DC,
            _ => luma,
        };
        if raw != 4 && m == luma {
            m = 34;
        }
        if d.cat == 2 {
            m = MAP_422[m as usize];
        }
        d.mode_c[k] = m;
    }
    if n == 1 {
        d.mode_c[1] = d.mode_c[0];
        d.mode_c[2] = d.mode_c[0];
        d.mode_c[3] = d.mode_c[0];
    }
    Ok(())
}
