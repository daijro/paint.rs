//! sRGB transfer curves and the 8-bit quantizer.
//! Paint.NET's piecewise sRGB curves in double, the 256-entry linear table, D3D's float→UNORM
//! quantize and f32 curves for the brush sprite.

use std::sync::LazyLock;

/// IEC 61966-2-1 decode: c ≤ 0.04045 → c/12.92, else ((c + 0.055)/1.055)^2.4.
pub fn srgb_to_linear(c: f64) -> f64 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// IEC 61966-2-1 encode: l ≤ 0.0031308 → 12.92·l, else 1.055·l^(1/2.4) − 0.055.
pub fn linear_to_srgb(l: f64) -> f64 {
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_to_linear_f32(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb_f32(l: f32) -> f32 {
    if l <= 0.0031308 {
        12.92 * l
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    }
}

/// `f32(srgb_to_linear(k/255))`.
pub static SRGB8_TO_LINEAR: LazyLock<[f32; 256]> =
    LazyLock::new(|| std::array::from_fn(|k| srgb_to_linear(k as f64 / 255.0) as f32));

/// ⌊clamp(x, 0, 1)·255 + 0.5⌋ with NaN → 0 (the cast truncates the positive value: the floor,
/// without a libm call).
pub fn quantize(x: f32) -> u8 {
    if x.is_nan() {
        0
    } else {
        (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
    }
}

pub fn linear_to_srgb8(l: f32) -> u8 {
    quantize(linear_to_srgb(l as f64) as f32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantizer_is_round_half_up_with_nan_zero() {
        // adjustments-and-effect-framework.md §5 Task 37.
        assert_eq!(quantize(0.5), 128);
        assert_eq!(quantize(f32::NAN), 0);
        assert_eq!(quantize(-1.0), 0);
        assert_eq!(quantize(2.0), 255);
    }

    #[test]
    fn linear_table_and_round_trip() {
        assert_eq!(SRGB8_TO_LINEAR[0], 0.0);
        assert_eq!(SRGB8_TO_LINEAR[255], 1.0);
        // 10/255 is below the 0.04045 knee: c / 12.92.
        assert!((SRGB8_TO_LINEAR[10] as f64 - 10.0 / 255.0 / 12.92).abs() < 1e-7);
        for k in 0..=255u8 {
            assert_eq!(linear_to_srgb8(SRGB8_TO_LINEAR[k as usize]), k);
        }
        // canvas-rendering.md §5: 50 % black over checker 191 blends to 139 in linear light.
        assert_eq!(
            linear_to_srgb8(SRGB8_TO_LINEAR[191] * (1.0 - 128.0 / 255.0)),
            139
        );
    }
}
