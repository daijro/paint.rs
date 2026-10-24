//! Ported from PhotoCraft crates/algo/src/stylize.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: only `solarize` and `per_pixel`, in place on an interleaved RGBA f32 buffer (alpha last, `as_chunks_mut` for `chunks_exact_mut`) instead of `Image`/`Rect`/`Ctx`; the CMYK (subtractive) branch dropped; the solarize assertions of `tests.rs` follow.

/// Solarize: inverts the highlights.
pub fn solarize(buf: &mut [f32]) {
    per_pixel(buf, |v| if v > 0.5 { 1.0 - v } else { v });
}

/// Colour channels through `f` (alpha untouched).
pub fn per_pixel(buf: &mut [f32], f: impl Fn(f32) -> f32) {
    for px in buf.as_chunks_mut::<4>().0 {
        for v in &mut px[..3] {
            *v = f(*v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stylize_basics() {
        let mut so = [0.8, 0.3, 0.1, 1.0];
        solarize(&mut so);
        assert!((so[0] - 0.2).abs() < 1e-6 && (so[1] - 0.3).abs() < 1e-6);
    }

    /// The RGB half of upstream's test: channels above 50% invert, alpha is untouched.
    #[test]
    fn find_edges_and_solarize_treat_cmyk_as_ink() {
        let mut so = [0.1, 0.2, 0.8, 1.0];
        solarize(&mut so);
        assert!(
            so.iter()
                .zip([0.1, 0.2, 0.2, 1.0])
                .all(|(a, b)| (a - b).abs() < 0.01),
            "{so:?}"
        );
    }
}
