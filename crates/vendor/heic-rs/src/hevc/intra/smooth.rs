//! Filtering of the neighbouring reference samples, clause 8.4.4.2.3.

use super::Refs;

/// Filters the neighbouring reference samples (clause 8.4.4.2.3).
///
/// `mode` is predModeIntra (0 planar, 1 DC, 2..=34 angular), `c_idx` 0 for luma,
/// `chroma_array_type` is 0..=3, `strong_smoothing` is
/// `strong_intra_smoothing_enabled_flag`, `disable_smoothing` is
/// `intra_smoothing_disabled_flag` from the SPS range extension.
pub fn filter_refs(
    r: &Refs,
    mode: u8,
    c_idx: usize,
    bit_depth: u8,
    chroma_array_type: u8,
    strong_smoothing: bool,
    disable_smoothing: bool,
) -> Refs {
    let n = r.n;
    let nn = 2 * n as isize;
    if disable_smoothing || (c_idx != 0 && chroma_array_type != 3) {
        return r.clone();
    }
    let m = mode as i32;
    let thresh = match n {
        8 => 7,
        16 => 1,
        _ => 0,
    };
    let dist = core::cmp::min((m - 26).abs(), (m - 10).abs());
    if mode == 1 || mode > 34 || n == 4 || dist <= thresh {
        return r.clone();
    }
    let c = r.top(-1) as i32;
    let (te, le) = (r.top(nn - 1) as i32, r.left(nn - 1) as i32);
    // `1 << (bit_depth - 5)`, written so a nonsensical small bit depth cannot
    // underflow the shift amount.
    let lim = (1i32 << bit_depth) >> 5;
    let bilin = strong_smoothing
        && c_idx == 0
        && n == 32
        && (c + te - 2 * r.top(nn / 2 - 1) as i32).abs() < lim
        && (c + le - 2 * r.left(nn / 2 - 1) as i32).abs() < lim;
    let mut o = Refs::new(n);
    if bilin {
        o.set_top(-1, c as u16);
        for i in 0..nn - 1 {
            let (w0, w1) = (nn - 1 - i, i + 1);
            o.set_left(i, (((w0 as i32) * c + (w1 as i32) * le + 32) >> 6) as u16);
            o.set_top(i, (((w0 as i32) * c + (w1 as i32) * te + 32) >> 6) as u16);
        }
    } else {
        o.set_top(
            -1,
            ((r.left(0) as i32 + 2 * c + r.top(0) as i32 + 2) >> 2) as u16,
        );
        for i in 0..nn - 1 {
            let l = r.left(i + 1) as i32 + 2 * r.left(i) as i32 + r.left(i - 1) as i32;
            let t = r.top(i + 1) as i32 + 2 * r.top(i) as i32 + r.top(i - 1) as i32;
            o.set_left(i, ((l + 2) >> 2) as u16);
            o.set_top(i, ((t + 2) >> 2) as u16);
        }
    }
    o.set_left(nn - 1, le as u16);
    o.set_top(nn - 1, te as u16);
    o
}
