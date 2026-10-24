//! ImageData and the premultiply/unpremultiply conversions Chromium applies in put/getImageData.
//! The exact rounding is pinned by the `premul-roundtrip-table` canvas case.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    /// Unpremultiplied RGBA8, row-major.
    pub data: Vec<u8>,
}

impl ImageData {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            data: vec![0; (width * height * 4) as usize],
        }
    }
}

/// putImageData: unpremultiplied → premultiplied (Skia raster pipeline: round(c * a / 255)).
pub fn premultiply(c: u8, a: u8) -> u8 {
    ((c as u32 * a as u32 + 127) / 255) as u8
}

/// getImageData: premultiplied → unpremultiplied, as Skia's highp raster pipeline does it in f32:
/// load `c * (1/255)`, `unpremul` multiplies by `1 / (a * (1/255))`, store rounds half-to-even
/// (x86 `cvtps2dq`). The integer `round(c * 255 / a)` form misses 962 and the `SkUnPreMultiply`
/// table form 1068 of the 65536 `premul-roundtrip-table` entries; this form matches all of them.
pub fn unpremultiply(c: u8, a: u8) -> u8 {
    if a == 0 {
        return 0;
    }
    let inv255 = 1.0f32 / 255.0;
    let scale = 1.0f32 / (a as f32 * inv255);
    ((c as f32 * inv255 * scale).clamp(0.0, 1.0) * 255.0).round_ties_even() as u8
}
