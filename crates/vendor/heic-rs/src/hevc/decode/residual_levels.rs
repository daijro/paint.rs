//! Level, sign and remainder decoding for one 4x4 residual sub-block.

use super::residual_ctx::read_remaining;
use super::state::Dec;
use crate::hevc::cabac::off;
use crate::hevc::error::Result;
use crate::hevc::scan::sub_scan;

/// Decodes the greater1, greater2, sign and remainder bins of one sub-block.
///
/// `sig` lists the significant scan positions of the sub-block, highest first,
/// which is the order every one of these syntax elements is coded in.
#[allow(clippy::too_many_arguments)]
pub fn decode_levels(
    d: &mut Dec<'_>,
    sig: &[u8],
    i: usize,
    c_idx: usize,
    c1: &mut i32,
    sb_type: usize,
    log2_size: usize,
    scan_idx: usize,
    xs: usize,
    ys: usize,
) -> Result<()> {
    let mut ctx_set = if i > 0 && c_idx == 0 { 2 } else { 0 };
    if *c1 == 0 {
        ctx_set += 1;
    }
    *c1 = 1;
    let num = sig.len();
    if num == 0 {
        return Ok(());
    }
    let base = off::GT1 + ctx_set * 4 + if c_idx > 0 { 16 } else { 0 };
    // Bit `k` of `gt1` is `coeff_abs_level_greater1_flag` of `sig[k]`.
    let mut gt1 = 0u16;
    let mut last_gt1 = usize::MAX;
    for k in 0..num.min(8) {
        let bin = d.cab.decision(base + (*c1).min(3) as usize) != 0;
        if bin {
            gt1 |= 1 << k;
            *c1 = 0;
            if last_gt1 == usize::MAX {
                last_gt1 = k;
            }
        } else if *c1 > 0 && *c1 < 3 {
            *c1 += 1;
        }
    }
    let sign_hidden = i32::from(sig[0]) - i32::from(sig[num - 1]) > 3 && !d.tq_bypass;
    let mut gt2 = false;
    if last_gt1 != usize::MAX {
        let ctx = off::GT2 + ctx_set + if c_idx > 0 { 4 } else { 0 };
        gt2 = d.cab.decision(ctx) != 0;
    }
    let hide = d.pps.sign_data_hiding && sign_hidden;
    // The sign flags come as one run of bypass bins, `sig[0]`'s first, with
    // the hidden one (the lowest position) simply absent from the end.
    let coded_signs = num - usize::from(hide);
    let signs = d.cab.bypass_bits(coded_signs as u32);
    let mut rice = if d.sps.persistent_rice {
        (d.stat_coeff[sb_type] / 4) as u32
    } else {
        0
    };
    let mut first_rem = true;
    let mut sum_abs = 0i64;
    let pos_scan = sub_scan(scan_idx);
    let n = 1usize << log2_size;
    for (k, &m) in sig.iter().enumerate() {
        let m = m as usize & 15;
        let base_level = 1 + i32::from((gt1 >> k) & 1 != 0) + i32::from(k == last_gt1 && gt2);
        let threshold = if k < 8 {
            if k == last_gt1 { 3 } else { 2 }
        } else {
            1
        };
        let mut level = base_level as i64;
        if base_level == threshold {
            let rem = read_remaining(d, rice)?;
            if first_rem && d.sps.persistent_rice {
                update_stat(d, sb_type, rem);
            }
            first_rem = false;
            level += rem as i64;
            if level > 3i64 << rice {
                rice = (rice + 1).min(4);
            }
        } else if level > 3i64 << rice {
            rice = (rice + 1).min(4);
        }
        sum_abs += level;
        let mut v = level;
        if k < coded_signs && (signs >> (coded_signs - 1 - k)) & 1 != 0 {
            v = -v;
        }
        if hide && k == num - 1 && sum_abs % 2 == 1 {
            v = -v;
        }
        let xc = xs * 4 + pos_scan[m][0] as usize;
        let yc = ys * 4 + pos_scan[m][1] as usize;
        d.coeffs[yc * n + xc] = v.clamp(-32768, 32767) as i32;
    }
    Ok(())
}

/// Applies the `StatCoeff` update of clause 9.3.3.11.
fn update_stat(d: &mut Dec<'_>, sb_type: usize, rem: u32) {
    let s = d.stat_coeff[sb_type];
    if rem as i64 >= 3i64 << (s / 4) {
        d.stat_coeff[sb_type] = s + 1;
    } else if (2 * rem as i64) < (1i64 << (s / 4)) && s > 0 {
        d.stat_coeff[sb_type] = s - 1;
    }
}
