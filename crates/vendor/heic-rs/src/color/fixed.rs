//! The fixed-point form of the `colr` matrix.
//!
//! Every coefficient is derived from the same [`MatrixCoefficients`] weights
//! and [`Range`] the float version used, then scaled by a power of two and
//! rounded once, here, rather than being applied as floating point per sample.
//!
//! [`MatrixCoefficients`]: crate::props::colr::MatrixCoefficients

use crate::props::colr::{Nclx, Range};

/// Fractional bits the matrix carries, chosen so that the widest product a
/// 12-bit sample can make still fits in an `i32`.
pub(crate) const fn shift_for(wide: bool) -> u32 {
    if wide { 13 } else { 16 }
}

/// Fractional bits used when rescaling an alpha sample to the output depth.
const ALPHA_SHIFT: u32 = 14;

/// Round away from zero, without pulling in a libm `round`.
fn q(v: f32) -> i32 {
    if v < 0.0 {
        (v - 0.5) as i32
    } else {
        (v + 0.5) as i32
    }
}

/// The largest sample a given bit depth can hold.
fn sample_max(depth: u8) -> i32 {
    ((1u32 << depth) - 1) as i32
}

/// One matrix, one range and one output depth, reduced to integers.
///
/// Each `k*` field is its real coefficient times `1 << shift`, already folded
/// together with the sample scaling and the output maximum, so a channel is
/// one multiply-accumulate, one rounded shift and one clamp.
pub(crate) struct Coeffs {
    /// Fractional bits in `ky` and the four chroma gains.
    pub shift: u32,
    /// Half a least significant bit, added before the shift.
    pub round: i32,
    /// The largest value a channel may take: 255 or 65535.
    pub max_out: i32,
    /// Subtracted from a luma sample before scaling.
    pub y_offset: i32,
    /// Subtracted from a chroma sample before scaling.
    pub c_offset: i32,
    /// Luma gain, also the gain the identity matrix uses on all three planes.
    pub ky: i32,
    /// Cr's contribution to red.
    pub rv: i32,
    /// Cb's contribution to green.
    pub gu: i32,
    /// Cr's contribution to green.
    pub gv: i32,
    /// Cb's contribution to blue.
    pub bu: i32,
}

impl Coeffs {
    /// Derive the integer matrix. `wide` selects 16-bit output.
    pub fn new(nclx: Nclx, depth: u8, wide: bool) -> Coeffs {
        let shift = shift_for(wide);
        let max_out: i32 = if wide { 65_535 } else { 255 };
        let unit = (max_out as f32) * ((1u32 << shift) as f32);
        let max = sample_max(depth) as f32;
        let (y_offset, y_gain, c_offset, c_gain) = match nclx.range {
            Range::Full => (0.0, 1.0 / max, f32::from(1u16 << (depth - 1)), 1.0 / max),
            Range::Limited => {
                let step = (1u32 << (depth - 8)) as f32;
                (
                    16.0 * step,
                    1.0 / (219.0 * step),
                    128.0 * step,
                    1.0 / (224.0 * step),
                )
            }
        };
        let (kr, kb) = nclx.matrix.luma_weights();
        let kg = 1.0 - kr - kb;
        let (rv, bu) = (2.0 * (1.0 - kr), 2.0 * (1.0 - kb));
        let (gu, gv) = (-2.0 * kb * (1.0 - kb) / kg, -2.0 * kr * (1.0 - kr) / kg);
        Coeffs {
            shift,
            round: 1 << (shift - 1),
            max_out,
            y_offset: y_offset as i32,
            c_offset: c_offset as i32,
            ky: q(y_gain * unit),
            rv: q(rv * c_gain * unit),
            gu: q(gu * c_gain * unit),
            gv: q(gv * c_gain * unit),
            bu: q(bu * c_gain * unit),
        }
    }

    /// Round an accumulated channel and clamp it into the output range.
    #[inline(always)]
    pub fn finish(&self, acc: i32) -> i32 {
        ((acc + self.round) >> self.shift).clamp(0, self.max_out)
    }
}

/// How an auxiliary alpha plane's samples map onto the output depth.
pub(crate) struct AlphaScale {
    gain: i32,
    max_in: i32,
    max_out: i32,
}

impl AlphaScale {
    /// Derive the scaling for an alpha plane of the given depth.
    pub fn new(depth: u8, max_out: i32) -> AlphaScale {
        let max_in = sample_max(depth);
        let gain = q((max_out as f32) * ((1u32 << ALPHA_SHIFT) as f32) / (max_in as f32));
        AlphaScale {
            gain,
            max_in,
            max_out,
        }
    }

    /// Rescale one sample, clamping a plane that overruns its declared depth.
    #[inline(always)]
    pub fn apply(&self, v: u16) -> i32 {
        let v = core::cmp::min(i32::from(v), self.max_in);
        ((v * self.gain + (1 << (ALPHA_SHIFT - 1))) >> ALPHA_SHIFT).clamp(0, self.max_out)
    }
}
