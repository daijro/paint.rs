//! Ported from PhotoCraft crates/algo/src/color_to_alpha.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: the kernel works on a whole interleaved straight RGBA f32 layer with an RGB key and no colour-model plumbing (paint.rs is 8-bit RGBA, so the depth, grayscale and no-alpha tests are dropped); the thresholds stay in the API, the effect passes 0 and 1.
//!
//! Removes a known colour from the pixels, turning it into transparency while keeping what was
//! mixed with it.
//!
//! A pixel `P` that is a semi-transparent colour `Q` composited over a background colour `C`
//! obeys `P = a·Q + (1 − a)·C`. Knowing only `P` and `C`, the most transparent solution (the
//! smallest `a` that keeps `Q` inside the 0–1 range) is, per channel `i`,
//!
//! * `a_i = (P_i − C_i) / (1 − C_i)` where `P_i > C_i` (`Q_i` would otherwise overshoot 1),
//! * `a_i = (C_i − P_i) / C_i` where `P_i < C_i` (`Q_i` would otherwise drop below 0),
//! * `a_i = 0` where `P_i = C_i`,
//!
//! and `a = max_i a_i`. Then `Q = C + (P − C) / a`, and the layer's alpha is multiplied by `a`.
//!
//! Two thresholds shape `a`: distances at or below `transparency` become fully transparent, at or
//! above `opacity` fully opaque, linear in between (`0` and `1` leave `a` as computed).

/// The opacity `a` (0–1) a pixel needs to be a mix of some in-range colour and `key`.
/// Non-finite samples are ignored.
pub fn opacity(colour: &[f32], key: &[f32]) -> f32 {
    let mut a = 0.0f32;
    for (&p, &c) in colour.iter().zip(key) {
        let ai = if p > c {
            // Past a key at the top of the range (`1 − C = 0`) no in-range colour reaches `P`.
            if c < 1.0 { (p - c) / (1.0 - c) } else { 1.0 }
        } else if p < c {
            if c > 0.0 { (c - p) / c } else { 1.0 }
        } else {
            0.0
        };
        if ai.is_finite() {
            a = a.max(ai);
        }
    }
    a.clamp(0.0, 1.0)
}

/// Applies the thresholds: `a <= transparency` → 0, `a >= opacity` → 1, linear in between.
pub fn remap(a: f32, transparency: f32, opacity: f32) -> f32 {
    if a <= transparency {
        0.0
    } else if a >= opacity {
        1.0
    } else {
        // Here transparency < a < opacity, so the span is positive.
        ((a - transparency) / (opacity - transparency)).clamp(0.0, 1.0)
    }
}

/// Unmixes `key` from one pixel's colour channels in place and returns the factor its alpha is
/// multiplied by. Fully opaque results keep the colour as it was; fully transparent ones take the
/// key colour (any colour would do, this one keeps the result deterministic).
pub fn unmix(colour: &mut [f32], key: &[f32], transparency: f32, opacity_threshold: f32) -> f32 {
    let a = remap(opacity(colour, key), transparency, opacity_threshold);
    if a >= 1.0 {
        return 1.0;
    }
    for (p, &c) in colour.iter_mut().zip(key) {
        *p = if a <= 0.0 {
            c
        } else {
            (c + (*p - c) / a).clamp(0.0, 1.0)
        };
    }
    a.max(0.0)
}

/// The filter kernel over a whole interleaved straight RGBA layer.
pub fn color_to_alpha(buf: &[f32], key: [f32; 3], transparency: f32, opacity: f32) -> Vec<f32> {
    let mut res = buf.to_vec();
    for px in res.as_chunks_mut::<4>().0 {
        let (colour, alpha) = px.split_at_mut(3);
        alpha[0] *= unmix(colour, &key, transparency, opacity);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, tol: f32) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn the_key_colour_becomes_fully_transparent() {
        let mut px = [1.0, 1.0, 1.0];
        assert_eq!(unmix(&mut px, &[1.0, 1.0, 1.0], 0.0, 1.0), 0.0);
        let mut px = [0.2, 0.4, 0.6];
        assert_eq!(unmix(&mut px, &[0.2, 0.4, 0.6], 0.0, 1.0), 0.0);
    }

    #[test]
    fn grey_on_white_is_black_with_partial_alpha() {
        // An antialiased black-on-white edge pixel at 30 % coverage.
        let mut px = [0.7, 0.7, 0.7];
        let a = unmix(&mut px, &[1.0, 1.0, 1.0], 0.0, 1.0);
        assert!(close(a, 0.3, 1e-6), "{a}");
        assert!(px.iter().all(|v| close(*v, 0.0, 1e-5)), "{px:?}");
        // Pure black stays opaque and black.
        let mut px = [0.0, 0.0, 0.0];
        assert_eq!(unmix(&mut px, &[1.0, 1.0, 1.0], 0.0, 1.0), 1.0);
        assert_eq!(px, [0.0, 0.0, 0.0]);
    }

    #[test]
    fn unmixed_colour_composited_over_the_key_gives_back_the_pixel() {
        let key = [0.2, 0.6, 0.9];
        for p in [
            [0.5, 0.5, 0.5],
            [0.1, 0.9, 0.95],
            [0.25, 0.6, 0.0],
            [1.0, 0.0, 0.3],
        ] {
            let mut q = p;
            let a = unmix(&mut q, &key, 0.0, 1.0);
            assert!(q.iter().all(|v| (0.0..=1.0).contains(v)), "{q:?}");
            for i in 0..3 {
                let back = a * q[i] + (1.0 - a) * key[i];
                assert!(close(back, p[i], 1e-5), "{p:?} → {q:?} a={a}");
            }
        }
    }

    #[test]
    fn thresholds_clip_and_stretch_the_opacity() {
        assert_eq!(remap(0.1, 0.2, 0.8), 0.0);
        assert_eq!(remap(0.2, 0.2, 0.8), 0.0);
        assert_eq!(remap(0.8, 0.2, 0.8), 1.0);
        assert!(close(remap(0.5, 0.2, 0.8), 0.5, 1e-6));
        // Crossed thresholds never divide by zero.
        assert_eq!(remap(0.5, 0.6, 0.4), 0.0);
        assert_eq!(remap(0.7, 0.6, 0.4), 1.0);
        assert_eq!(remap(0.5, 0.5, 0.5), 0.0);
        // A near-white pixel under the transparency threshold disappears completely…
        let mut px = [0.95, 0.95, 0.95];
        assert_eq!(unmix(&mut px, &[1.0; 3], 0.1, 1.0), 0.0);
        // …and a mid grey above the opacity threshold is left exactly as it was.
        let mut px = [0.4, 0.4, 0.4];
        assert_eq!(unmix(&mut px, &[1.0; 3], 0.0, 0.5), 1.0);
        assert_eq!(px, [0.4, 0.4, 0.4]);
    }

    #[test]
    fn hdr_and_non_finite_samples_stay_opaque_and_finite() {
        let mut px = [1.5, 1.0, 1.0];
        assert_eq!(unmix(&mut px, &[1.0; 3], 0.0, 1.0), 1.0);
        assert_eq!(px, [1.5, 1.0, 1.0]);
        let mut px = [f32::NAN, 0.5, 0.5];
        let a = unmix(&mut px, &[1.0; 3], 0.0, 1.0);
        assert!(a.is_finite() && close(a, 0.5, 1e-6));
        // A black key: everything brighter is a mix of black and a brighter colour.
        let mut px = [0.25, 0.5, 0.0];
        let a = unmix(&mut px, &[0.0; 3], 0.0, 1.0);
        assert!(close(a, 0.5, 1e-6));
        assert!(
            close(px[0], 0.5, 1e-6) && close(px[1], 1.0, 1e-6) && close(px[2], 0.0, 1e-6),
            "{px:?}"
        );
    }

    #[test]
    fn kernel_multiplies_existing_alpha() {
        let buf = [
            1.0, 1.0, 1.0, 1.0, // key: gone
            0.6, 0.6, 0.6, 1.0, // 40 % black
            0.0, 0.0, 0.0, 1.0, // black kept
            0.6, 0.6, 0.6, 0.5, // alpha multiplied
        ];
        let out = color_to_alpha(&buf, [1.0; 3], 0.0, 1.0);
        assert!(out[3] < 1e-6);
        assert!(close(out[7], 0.4, 1e-6) && close(out[4], 0.0, 1e-5));
        assert_eq!(out[8..12], buf[8..12]);
        assert!(close(out[15], 0.2, 1e-6));
    }
}
