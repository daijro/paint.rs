//! Photocraft phase spec: S§4.2 (vendored algorithm code)
//!
//! Ported kernels work on interleaved straight RGBA f32 in 0–1; paint.rs layers are straight
//! RGBA8 (`ImageData`). These convert both ways and give the selection as flags.

use crate::canvas::ImageData;
use crate::effect::EffectEnv;

/// The layer as interleaved straight RGBA in 0–1.
pub fn to_f32(img: &ImageData) -> Vec<f32> {
    img.data.iter().map(|&b| f32::from(b) / 255.0).collect()
}

/// A w×h layer from interleaved straight RGBA: ×255, rounded half up, clamped (NaN → 0).
pub fn from_f32(buf: &[f32], w: u32, h: u32) -> ImageData {
    ImageData {
        width: w,
        height: h,
        // `as` saturates: negatives and NaN give 0, overshoot 255.
        data: buf.iter().map(|&v| (v * 255.0 + 0.5) as u8).collect(),
    }
}

/// The selection as one flag per pixel, row-major (`EffectEnv::selected`).
pub fn mask_bool(env: &EffectEnv) -> Vec<bool> {
    (0..env.height)
        .flat_map(|y| (0..env.width).map(move |x| env.selected(x, y)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_f32` → `from_f32` is the identity for all 256 levels; the way back rounds half up and
    /// clamps.
    #[test]
    fn shim_round_trips_rgba8() {
        let img = ImageData {
            width: 64,
            height: 1,
            data: (0..=255).collect(),
        };
        let f = to_f32(&img);
        assert!(f.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(from_f32(&f, 64, 1).data, img.data);
        assert_eq!(
            from_f32(&[0.5, 0.25, -1.0, 2.0, f32::NAN, 1.0, 0.0, 1.0], 2, 1).data,
            [128, 64, 0, 255, 0, 255, 0, 255]
        );
    }
}
