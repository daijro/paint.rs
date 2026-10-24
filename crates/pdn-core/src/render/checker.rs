//! Paint.NET's transparency checkerboard, shared by the canvas, the Colors form and thumbnails.

/// (light, dark) = (⌊255·b⌋, ⌊191·b⌋) in f32, truncated; b = `canvas.checkerboardBrightness`/100.
pub fn checker_colors(brightness: f32) -> (u8, u8) {
    ((255.0 * brightness) as u8, (191.0 * brightness) as u8)
}

/// SF = 2^⌊log₂ max(1, ⌊u⌋)⌋.
// calibrate: B-C7 — Paint.NET computes SF once at startup; callers pass the current scale.
pub fn checker_sf(u: f64) -> u32 {
    let n = u.floor().clamp(1.0, u32::MAX as f64) as u32;
    1 << n.ilog2()
}

/// Square side T = 4·m·SF px: Medium (m = 2) on the canvas, Small (m = 1) for swatches,
/// thumbnails and slider strips. Appendix B B-C2 calibrates the size and anchoring.
pub fn checker_square(u: f64, medium: bool) -> u32 {
    checker_sf(u).saturating_mul(if medium { 8 } else { 4 })
}

/// Device pixel (x, y) is light iff (⌊x/t⌋ ⊕ ⌊y/t⌋) is odd, with floor division; (0, 0) is dark.
pub fn checker_is_light(x: i32, y: i32, t: u32) -> bool {
    let t = t.clamp(1, i32::MAX as u32) as i32;
    (x.div_euclid(t) ^ y.div_euclid(t)) & 1 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    /// canvas-rendering.md §5 "Checker"; ui-drawn-graphics.md §3 "Checker vs brightness".
    #[test]
    fn colours_truncate() {
        assert_eq!(checker_colors(0.75), (191, 143));
        assert_eq!(checker_colors(0.6), (153, 114));
        assert_eq!(checker_colors(0.25), (63, 47));
        assert_eq!(checker_colors(0.5), (127, 95));
    }

    #[test]
    fn squares_and_phase() {
        assert_eq!(
            (
                checker_sf(1.0),
                checker_sf(1.5),
                checker_sf(2.0),
                checker_sf(4.5)
            ),
            (1, 1, 2, 4)
        );
        assert_eq!(
            (
                checker_square(1.0, true),
                checker_square(2.0, true),
                checker_square(1.0, false)
            ),
            (8, 16, 4)
        );
        // (0,0) dark; (8,0) light; (8,8) dark; (16,0) dark; (15,15) dark.
        let light: Vec<bool> = [(0, 0), (8, 0), (8, 8), (16, 0), (15, 15)]
            .iter()
            .map(|&(x, y)| checker_is_light(x, y, 8))
            .collect();
        assert_eq!(light, [false, true, false, false, false]);
    }
}
