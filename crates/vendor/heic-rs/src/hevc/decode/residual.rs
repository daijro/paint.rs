//! Residual coding syntax (clause 7.3.8.11) and its context derivations (9.3.4.2).

use super::residual_ctx::{SigCtx, last_position, scan_index};
use super::residual_levels::decode_levels;
use super::state::Dec;
use crate::hevc::cabac::off;
use crate::hevc::error::{Error, Result};
use crate::hevc::scan::{SUB_RASTER, scan_order, scan_pos};

/// Parses one `residual_coding()` into `d.coeffs` and reports `transform_skip_flag`.
pub fn residual_coding(
    d: &mut Dec<'_>,
    log2_size: usize,
    c_idx: usize,
    pred_mode: u8,
) -> Result<bool> {
    let n = 1usize << log2_size;
    d.coeffs[..n * n].fill(0);
    let mut ts = false;
    if d.pps.transform_skip && !d.tq_bypass && log2_size <= d.pps.log2_max_transform_skip {
        let ctx = if c_idx == 0 {
            off::TS_LUMA
        } else {
            off::TS_CHROMA
        };
        ts = d.cab.decision(ctx) != 0;
    }
    let scan_idx = scan_index(d, log2_size, c_idx, pred_mode);
    let (mut last_x, mut last_y) = last_position(d, log2_size, c_idx);
    if scan_idx == 2 {
        core::mem::swap(&mut last_x, &mut last_y);
    }
    if last_x >= n || last_y >= n {
        return Err(Error::InvalidData(
            "last significant coefficient outside block",
        ));
    }
    let sb_log2 = log2_size - 2;
    let sb_n = 1usize << sb_log2;
    let sb_scan = scan_order(sb_log2, scan_idx);
    let last_sb = scan_pos(sb_log2, scan_idx, last_x >> 2, last_y >> 2);
    let last_pos = scan_pos(2, scan_idx, last_x & 3, last_y & 3);
    d.csbf[..sb_n * sb_n].fill(0);
    let ts_ctx = d.sps.transform_skip_context && (ts || d.tq_bypass);
    let sb_type = (if c_idx == 0 { 0 } else { 2 }) + usize::from(ts || d.tq_bypass);
    let raster = &SUB_RASTER[scan_idx];
    let mut c1 = 1i32;
    for i in (0..=last_sb).rev() {
        let xs = sb_scan[i][0] as usize;
        let ys = sb_scan[i][1] as usize;
        let mut infer_dc = false;
        let coded = if i < last_sb && i > 0 {
            let mut c = 0u32;
            if xs + 1 < sb_n {
                c += d.csbf[ys * sb_n + xs + 1] as u32;
            }
            if ys + 1 < sb_n {
                c += d.csbf[(ys + 1) * sb_n + xs] as u32;
            }
            let ctx = off::CSBF + c.min(1) as usize + if c_idx > 0 { 2 } else { 0 };
            infer_dc = true;
            d.cab.decision(ctx) != 0
        } else {
            true
        };
        d.csbf[ys * sb_n + xs] = u8::from(coded);
        if !coded {
            continue;
        }
        let sig = SigCtx::new(d, log2_size, c_idx, xs, ys, sb_n, scan_idx, ts_ctx);
        // Significant scan positions of this sub-block, highest first.
        let mut pos = [0u8; 16];
        let mut num = 0usize;
        let mut first = 16usize;
        if i == last_sb {
            pos[0] = last_pos as u8;
            num = 1;
            first = last_pos;
        }
        for m in (0..first).rev() {
            if m > 0 || !infer_dc {
                let ctx = sig.at(m, raster[m] as usize);
                if d.cab.decision(off::SIG + ctx) != 0 {
                    pos[num] = m as u8;
                    num += 1;
                    infer_dc = false;
                }
            } else {
                pos[num] = m as u8;
                num += 1;
            }
        }
        decode_levels(
            d,
            &pos[..num],
            i,
            c_idx,
            &mut c1,
            sb_type,
            log2_size,
            scan_idx,
            xs,
            ys,
        )?;
    }
    Ok(ts)
}
