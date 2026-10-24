//! Context index derivations for residual coding (clauses 9.3.4.2.3 to .2.7).

use super::state::Dec;
use crate::hevc::cabac::off;
use crate::hevc::error::{Error, Result};

/// `ctxIdxMap` for `sig_coeff_flag` in a 4x4 transform block (Table 9-39),
/// by raster position `(yC << 2) | xC`.
const SIG_MAP_4X4: [u8; 16] = [0, 1, 4, 5, 2, 3, 4, 5, 6, 6, 8, 8, 7, 7, 8, 8];

/// `sigCtx` before the size and colour offsets, by `prevCsbf` and raster
/// position `(yP << 2) | xP` inside the sub-block (clause 9.3.4.2.5).
const SIG_PATTERN: [[u8; 16]; 4] = [
    [2, 1, 1, 0, 1, 1, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0],
    [2, 2, 2, 2, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0, 0],
    [2, 1, 0, 0, 2, 1, 0, 0, 2, 1, 0, 0, 2, 1, 0, 0],
    [2; 16],
];

/// Every position shares one context under `transform_skip_context_enabled_flag`.
const FLAT: [u8; 16] = [0; 16];

/// Derives `scanIdx` (clause 7.4.9.11): 0 diagonal, 1 horizontal, 2 vertical.
pub fn scan_index(d: &Dec<'_>, log2_size: usize, c_idx: usize, pred_mode: u8) -> usize {
    let mode_dependent =
        log2_size == 2 || (log2_size == 3 && c_idx == 0) || (log2_size == 3 && d.cat == 3);
    if !mode_dependent {
        return 0;
    }
    if (6..=14).contains(&pred_mode) {
        2
    } else if (22..=30).contains(&pred_mode) {
        1
    } else {
        0
    }
}

/// Decodes `last_sig_coeff_x/y_prefix` and their suffixes (clause 9.3.3.10).
pub fn last_position(d: &mut Dec<'_>, log2_size: usize, c_idx: usize) -> (usize, usize) {
    let (ctx_offset, ctx_shift) = if c_idx == 0 {
        (
            3 * (log2_size - 2) + ((log2_size - 1) >> 2),
            (log2_size + 1) >> 2,
        )
    } else {
        (15, log2_size - 2)
    };
    let c_max = (log2_size << 1) - 1;
    let mut prefix = [0usize; 2];
    for (k, base) in [off::LAST_X, off::LAST_Y].into_iter().enumerate() {
        let mut v = 0usize;
        while v < c_max {
            let ctx = base + ctx_offset + (v >> ctx_shift);
            if d.cab.decision(ctx) == 0 {
                break;
            }
            v += 1;
        }
        prefix[k] = v;
    }
    let mut out = [0usize; 2];
    for k in 0..2 {
        out[k] = if prefix[k] > 3 {
            let bits = (prefix[k] >> 1) - 1;
            let suffix = d.cab.bypass_bits(bits as u32) as usize;
            ((1usize << bits) * (2 + (prefix[k] & 1))) + suffix
        } else {
            prefix[k]
        };
    }
    (out[0], out[1])
}

/// The `sig_coeff_flag` context derivation for one sub-block (clause 9.3.4.2.5).
///
/// Everything that is fixed across the coefficients of a sub-block — the
/// `prevCsbf` pattern, the block size and colour offsets — is settled once
/// here, leaving one table lookup per coefficient.
pub struct SigCtx {
    table: &'static [u8; 16],
    add: usize,
    /// Context of the DC coefficient, where it does not follow the table.
    dc: Option<usize>,
}

impl SigCtx {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        d: &Dec<'_>,
        log2_size: usize,
        c_idx: usize,
        xs: usize,
        ys: usize,
        sb_n: usize,
        scan_idx: usize,
        ts_ctx: bool,
    ) -> SigCtx {
        let chroma = if c_idx > 0 { 27 } else { 0 };
        if ts_ctx {
            let add = if c_idx == 0 { 42 } else { 43 };
            return SigCtx {
                table: &FLAT,
                add,
                dc: None,
            };
        }
        if log2_size == 2 {
            return SigCtx {
                table: &SIG_MAP_4X4,
                add: chroma,
                dc: None,
            };
        }
        let mut prev = 0usize;
        if xs + 1 < sb_n {
            prev += d.csbf[ys * sb_n + xs + 1] as usize;
        }
        if ys + 1 < sb_n {
            prev += (d.csbf[(ys + 1) * sb_n + xs] as usize) << 1;
        }
        let add = if c_idx == 0 {
            let size = if log2_size == 3 {
                if scan_idx == 0 { 9 } else { 15 }
            } else {
                21
            };
            usize::from(xs + ys > 0) * 3 + size
        } else {
            chroma + if log2_size == 3 { 9 } else { 12 }
        };
        SigCtx {
            table: &SIG_PATTERN[prev & 3],
            add,
            dc: if xs + ys == 0 { Some(chroma) } else { None },
        }
    }

    /// `ctxInc` of scan position `m`, whose raster index in the sub-block is `raster`.
    #[inline]
    pub fn at(&self, m: usize, raster: usize) -> usize {
        match self.dc {
            Some(dc) if m == 0 => dc,
            _ => self.table[raster & 15] as usize + self.add,
        }
    }
}

/// Decodes `coeff_abs_level_remaining` (clause 9.3.3.11).
pub fn read_remaining(d: &mut Dec<'_>, rice: u32) -> Result<u32> {
    let mut prefix = 0u32;
    while d.cab.bypass() == 1 {
        prefix += 1;
        if prefix > 24 {
            return Err(Error::InvalidData(
                "coeff_abs_level_remaining prefix too long",
            ));
        }
    }
    if prefix < 3 {
        let suffix = d.cab.bypass_bits(rice);
        Ok((prefix << rice) + suffix)
    } else {
        let extra = prefix - 3;
        let suffix = d.cab.bypass_bits(extra + rice);
        Ok(suffix + (((1u32 << extra) + 3 - 1) << rice))
    }
}
