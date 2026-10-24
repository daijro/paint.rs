//! Block reconstruction: intra prediction plus the inverse-transformed residual.

use super::refs::gather_refs;
use super::state::Dec;
use crate::hevc::error::Result;
use crate::hevc::intra::{filter_refs, predict};
use crate::hevc::transform::{inverse_transform, scale, transform_skip};

/// Predicts one transform block and adds its residual into the picture.
///
/// `x` and `y` are in the coordinates of component `c_idx`. When `has_resid`
/// is false the prediction is written out unchanged.
#[allow(clippy::too_many_arguments)]
pub fn reconstruct(
    d: &mut Dec<'_>,
    x: usize,
    y: usize,
    log2: usize,
    c_idx: usize,
    mode: u8,
    has_resid: bool,
    ts: bool,
) -> Result<()> {
    let n = 1usize << log2;
    let bd = if c_idx == 0 {
        d.sps.bit_depth_y
    } else {
        d.sps.bit_depth_c
    };
    let raw = gather_refs(d, x, y, n, c_idx);
    let filtered = filter_refs(
        &raw,
        mode,
        c_idx,
        bd,
        d.cat,
        d.sps.strong_intra_smoothing,
        d.sps.intra_smoothing_disabled,
    );
    predict(&filtered, mode, c_idx, bd, &mut d.pred, n);
    if has_resid {
        if d.tq_bypass {
            // Residual samples are the coefficient levels themselves.
        } else {
            let qp = if c_idx == 0 {
                d.qp_luma()
            } else {
                d.qp_chroma(c_idx)
            };
            let fac = d.factors(log2, c_idx, ts);
            scale(&mut d.coeffs, n, qp, bd, fac);
            if ts {
                transform_skip(&mut d.coeffs, n, bd, d.sps.transform_skip_rotation);
            } else {
                let tr_type = u8::from(c_idx == 0 && n == 4);
                inverse_transform(&mut d.coeffs, n, tr_type, bd);
            }
        }
    }
    let max = (1i32 << bd) - 1;
    let plane = match c_idx {
        0 => &mut d.pic.y,
        1 => &mut d.pic.cb,
        _ => &mut d.pic.cr,
    };
    let w = n.min(plane.width.saturating_sub(x));
    let h = n.min(plane.height.saturating_sub(y));
    if w == 0 || h == 0 {
        // The block starts right of or below the picture: nothing to write, and its row offsets
        // can point past the end of the plane (reported by s17sh70x2jfl031ag against 0.1.3).
        return Ok(());
    }
    for j in 0..h {
        let o = (y + j) * plane.stride + x;
        let dst = &mut plane.data[o..o + w];
        let pred = &d.pred[j * n..j * n + w];
        if has_resid {
            let res = &d.coeffs[j * n..j * n + w];
            for ((o, &p), &r) in dst.iter_mut().zip(pred.iter()).zip(res.iter()) {
                *o = (p as i32 + r).clamp(0, max) as u16;
            }
        } else {
            dst.copy_from_slice(pred);
        }
    }
    Ok(())
}
