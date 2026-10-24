//! BB spec: common.md §1, §5.1, §5.5–§5.9; effects/object-bevel-object.md steps 4–5;
//! effects/photo-vignette-plus.md "The Direct2D vignette"; effects/artistic-dream.md step 3;
//! effects/text-formations-text-fun-factory.md "3D bevel" step 5
//!
//! The Paint.NET and Direct2D building blocks the plugin pipelines share. Paint.NET's own blocks are
//! paint.rs's existing ports, re-exported; the Direct2D ones paint.rs lacked are here. Pixels are
//! `[r, g, b, a]` f32 in the effect's working space.

pub use crate::effect::adjustments::{bc_shift, hsl_adjust, rgb_to_hsl, temp_tint_linear};
pub use crate::effect::blur::{bokeh, bokeh_with_edge, motion_blur};
pub use crate::effect::object::{drop_shadow_with, shadow_offset};
pub use crate::effect::rotate_zoom::rotate_zoom_with;
pub use crate::effect::stylize::emboss_surface;

use super::conventions::VIGNETTE_ALPHA_MODE;
use crate::effect::RenderInfo;
use crate::effect::blur::{box_fold, gaussian};
use crate::effect::distort::{catmull_rom, filter4};
use crate::effect::mix::MixMode;
use crate::effect::surface::{Alpha, Edge, EffectSurface, Space, luma, premultiply, unpremultiply};
use crate::pixel::gamma::{SRGB8_TO_LINEAR, quantize};
use crate::pixel::{IntRect, Px};
use std::f32::consts::PI;

/// §5.1 Paint.NET's GPU Gaussian blur on premultiplied `src`, transparent outside ("soft").
pub fn blur_soft(src: &EffectSurface, sigma: f64) -> EffectSurface {
    gaussian(src, sigma, Edge::Transparent)
}

/// A colour converted properly into `info`'s working space (sRGB → linear when linear), then
/// premultiplied.
pub fn colour_exact(c: Px, info: RenderInfo) -> [f32; 4] {
    let k = |v: u8| match info.space {
        Space::Linear => SRGB8_TO_LINEAR[v as usize],
        Space::Srgb => v as f32 / 255.0,
    };
    premultiply([k(c[0]), k(c[1]), k(c[2]), c[3] as f32 / 255.0])
}

/// A colour passed raw: k/255 used as working-space values whatever the space, premultiplied.
pub fn colour_raw(c: Px) -> [f32; 4] {
    premultiply(c.map(|v| v as f32 / 255.0))
}

/// §5.8 the 8-bit quantise step: round to a 1/255 step (UNORM: clamped, NaN → 0), go on in float.
pub fn quantise8(v: f32) -> f32 {
    quantize(v) as f32 / 255.0
}

/// §5.5–§5.6: D2D1_BLEND_MODE `index` (0 Multiply … 25 Division) as the paint.rs mix mode.
pub fn d2d_blend(index: usize) -> MixMode {
    debug_assert!(index < 26, "D2D1_BLEND_MODE {index}");
    MixMode::ALL[3 + index]
}

/// §5.6 Porter–Duff SourceOver on premultiplied colour: src + dst·(1 − srcₐ).
pub fn source_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    std::array::from_fn(|c| src[c] + dst[c] * (1.0 - src[3]))
}

/// SourceOver on straight colour (§5.5 Mix, straight alpha mode); transparent black when both are
/// transparent.
pub fn straight_over(dst: [f32; 4], src: [f32; 4]) -> [f32; 4] {
    let a = src[3] + dst[3] * (1.0 - src[3]);
    if a <= 0.0 {
        return [0.0; 4];
    }
    let c = |i: usize| (src[i] * src[3] + dst[i] * dst[3] * (1.0 - src[3])) / a;
    [c(0), c(1), c(2), a]
}

/// Channel A of premultiplied `c` replaced by `alpha`, keeping the straight colour.
pub fn with_alpha(c: [f32; 4], alpha: f32) -> [f32; 4] {
    let u = unpremultiply(c);
    premultiply([u[0], u[1], u[2], alpha])
}

/// §5.8 swizzle in premultiplied mode: the premultiplied RGB kept, A replaced. The output
/// quantizer clamps any straight value above 1.
pub fn swap_alpha(c: [f32; 4], alpha: f32) -> [f32; 4] {
    [c[0], c[1], c[2], alpha]
}

/// Which colour a lighting-layer pixel takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shade {
    Light,
    Dark,
}

/// The lighting-layer shader (object-bevel-object.md step 5; text-formations-text-fun-factory.md
/// "3D bevel" step 5) on relief value `r`: nothing within `dead_band` of 0.5 (only exactly 0.5 for
/// band 0); else Light above 0.5, Dark below, opacity rising linearly from 0 at 0.5 to `strength`
/// at 1 (0), extended past 0…1, capped at 0.99.
pub fn lighting(r: f32, strength: f32, dead_band: f32) -> Option<(Shade, f32)> {
    let d = r - 0.5;
    if d.is_nan() || d.abs() <= dead_band {
        return None;
    }
    let shade = if d > 0.0 { Shade::Light } else { Shade::Dark };
    let o = (strength * d.abs() / 0.5).min(0.99);
    (o > 0.0).then_some((shade, o))
}

/// §5.7 the Direct2D Vignette over `rect` (premultiplied `src`, premultiplied `colour`): source-over
/// of `colour` at coverage strength·(1 − cos πk)/2, k = clamp((e − (1 − transition))/transition,
/// 0, 1), e the elliptical distance from the rect's centre (1 at its corners). Transparent outside
/// `rect` (the plugins crop to the selection bounds first). transition ≤ 0 or strength ≤ 0 returns
/// `src`. The source-over's alpha mode is `VIGNETTE_ALPHA_MODE`.
// approximation: D2D Vignette falloff (photo-vignette-plus.md "The Direct2D vignette")
// spec-question: Q18 Direct2D Vignette on semi-transparent pixels
pub fn d2d_vignette(
    src: &EffectSurface,
    rect: IntRect,
    colour: [f32; 4],
    transition: f32,
    strength: f32,
) -> EffectSurface {
    if transition <= 0.0 || strength <= 0.0 {
        return src.clone();
    }
    let (hw, hh) = (rect.w as f32 / 2.0, rect.h as f32 / 2.0);
    let (cx, cy) = (rect.x as f32 + hw, rect.y as f32 + hh);
    let inner = 1.0 - transition;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        if !rect.contains(x as i32, y as i32) {
            return [0.0; 4];
        }
        let (u, v) = ((x as f32 + 0.5 - cx) / hw, (y as f32 + 0.5 - cy) / hh);
        let e = u.hypot(v) / std::f32::consts::SQRT_2;
        let k = ((e - inner) / transition).clamp(0.0, 1.0);
        let cov = strength * (1.0 - (PI * k).cos()) / 2.0;
        vignette_over(src.at(x, y), colour.map(|c| c * cov), VIGNETTE_ALPHA_MODE)
    })
}

/// Premultiplied `c` over premultiplied `p`: on premultiplied pixels, or on the straight colour
/// with `p`'s alpha kept.
fn vignette_over(p: [f32; 4], c: [f32; 4], mode: Alpha) -> [f32; 4] {
    match mode {
        Alpha::Premultiplied => source_over(p, c),
        Alpha::Straight => with_alpha(source_over(with_alpha(p, 1.0), c), p[3]),
    }
}

/// §5.7 Direct2D Sepia at its default intensity 0.5: the classic sepia matrix, halfway from the
/// identity.
// approximation: D2D Sepia coefficients (adjustments-combined-adjustments.md Notes)
pub fn d2d_sepia(rgb: [f32; 3]) -> [f32; 3] {
    const M: [[f32; 3]; 3] = [
        [0.393, 0.769, 0.189],
        [0.349, 0.686, 0.168],
        [0.272, 0.534, 0.131],
    ];
    std::array::from_fn(|i| {
        let s = M[i][0] * rgb[0] + M[i][1] * rgb[1] + M[i][2] * rgb[2];
        rgb[i] + 0.5 * (s - rgb[i])
    })
}

/// §5.7 Direct2D Grayscale: R = G = B = BT.601 luma.
pub fn d2d_grayscale(rgb: [f32; 3]) -> [f32; 3] {
    [luma(rgb[0], rgb[1], rgb[2]); 3]
}

/// §5.7 the 2-D affine transform, high-quality cubic: each destination pixel centre p maps to the
/// source position s = (m11·pₓ + m21·p_y + dx, m12·pₓ + m22·p_y + dy) with `inverse` =
/// [m11, m12, m21, m22, dx, dy] (Direct2D's Matrix3x2 layout, destination → source), sampled
/// with Catmull-Rom taps through `edge`.
// approximation: D2D high-quality cubic (common.md §5.7 "2-D Affine Transform")
pub fn affine_cubic(src: &EffectSurface, inverse: [f32; 6], edge: Edge) -> EffectSurface {
    let [m11, m12, m21, m22, dx, dy] = inverse;
    EffectSurface::from_fn(src.w, src.h, |x, y| {
        let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
        let s = [m11 * px + m21 * py + dx, m12 * px + m22 * py + dy];
        filter4(s, catmull_rom, |i, j| src.fetch(i, j, edge))
    })
}

/// §5.7 Morphology erode: the per-channel minimum over the (2r + 1)² window (r ≥ 0).
pub fn morph_min(src: &EffectSurface, r: i32, edge: Edge) -> EffectSurface {
    debug_assert!(r >= 0, "morph_min radius {r}");
    box_fold(src, [-r, -r], [r, r], edge, f32::min)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::bb::conventions::*;
    use crate::effect::surface::{Edge, EffectSurface};
    use crate::pixel::gamma::SRGB8_TO_LINEAR;

    fn grey(w: u32, h: u32, f: impl Fn(u32, u32) -> f32 + Sync) -> EffectSurface {
        EffectSurface::from_fn(w, h, |x, y| {
            let v = f(x, y);
            [v, v, v, 1.0]
        })
    }

    /// common.md §5.6 / adjustments-black-and-white-plus.md step 4: drop-down position − 1 is D2D1_BLEND_MODE.
    #[test]
    fn d2d_blend_order() {
        let names: Vec<_> = (0..26).map(|n| d2d_blend(n).value()).collect();
        assert_eq!(
            (names[0], names[4], names[11], names[25]),
            ("multiply", "dissolve", "overlay", "division")
        );
    }

    /// object-bevel-object.md step 5 and text-formations-text-fun-factory.md "3D bevel" step 5 tables.
    #[test]
    fn lighting_layer_tables() {
        let o = |r, s, band| {
            lighting(r, s, band)
                .map(|(sh, o)| (matches!(sh, Shade::Light), (o * 1e4).round() / 1e4))
        };
        assert_eq!(
            [1.0, 0.75, 0.6].map(|r| o(r, 1.0, 0.001)),
            [Some((true, 0.99)), Some((true, 0.5)), Some((true, 0.2))]
        );
        assert_eq!(
            (
                o(0.5005, 1.0, 0.001),
                o(0.25, 1.0, 0.001),
                o(0.0, 1.0, 0.001)
            ),
            (None, Some((false, 0.5)), Some((false, 0.99)))
        );
        assert_eq!(
            [1.0, 0.875, 0.75, 0.25, 0.0].map(|r| o(r, 0.75, 1e-7).unwrap().1),
            [0.75, 0.5625, 0.375, 0.375, 0.75]
        );
        assert_eq!(o(0.5, 0.75, 0.0), None);
        assert_eq!(o(1.0, 2.0, 0.001), Some((true, 0.99)));
    }

    /// object-bevel-object.md step 4: at 315° a dark object on a white surround is lit along its upper-left inner
    /// edges and shaded along its lower-right ones; flat areas are 0.5. If this fails, set EMBOSS_ANGLE_OFFSET_DEG
    /// to 180 and file a spec-question.
    #[test]
    fn emboss_315_lights_the_upper_left_inner_edges() {
        let m = grey(20, 20, |x, y| {
            if (5..15).contains(&x) && (5..15).contains(&y) {
                0.0
            } else {
                1.0
            }
        });
        let e = emboss_surface(&m, 315.0 + EMBOSS_ANGLE_OFFSET_DEG);
        assert!(e.at(5, 10)[0] > 0.5 && e.at(10, 5)[0] > 0.5);
        assert!(e.at(14, 10)[0] < 0.5 && e.at(10, 14)[0] < 0.5);
        assert!(
            (e.at(10, 10)[0] - 128.0 / 255.0).abs() < 1e-6,
            "the built-in Emboss is flat at 128/255; bevel::relief re-centres it (Q16)"
        );
    }

    /// object-inner-shadow.md step 3: at −45° the non-centred smear drags a shape toward the lower right.
    #[test]
    fn smear_at_minus_45_moves_down_right() {
        let mut s = EffectSurface::new(20, 20);
        s.px[5 * 20 + 5] = [1.0; 4];
        let m = motion_blur(
            &s,
            6.0,
            -45.0 + SMEAR_ANGLE_OFFSET_DEG,
            false,
            Edge::Transparent,
        );
        assert!(m.at(8, 8)[3] > 0.0 && m.at(2, 2)[3] == 0.0);
    }

    /// common.md §5.9 Drop Shadow: −45° puts the shadow down-right.
    #[test]
    fn drop_shadow_minus_45_falls_down_right() {
        let [ox, oy] = shadow_offset(4.0, -45.0 + SHADOW_ANGLE_OFFSET_DEG);
        assert!(ox > 2.8 && oy > 2.8);
    }

    /// common.md §5.8: round to 1/255 steps, then continue in float.
    #[test]
    fn quantise_and_colour_hand_over() {
        assert_eq!(quantise8(0.5), 128.0 / 255.0);
        assert_eq!(quantise8(1.3), 1.0);
        let c = [255, 140, 0, 255];
        assert!((colour_exact(c, GPU_CONTEXT)[1] - SRGB8_TO_LINEAR[140]).abs() < 1e-6);
        assert!((colour_raw(c)[1] - 140.0 / 255.0).abs() < 1e-6);
        assert_eq!(
            colour_exact([255, 255, 255, 128], ALT_GPU_CONTEXT)[0],
            128.0 / 255.0
        );
        assert_eq!(colour_exact(c, ALT_GPU_CONTEXT)[1], 140.0 / 255.0);
    }

    /// photo-vignette-plus.md Expected results 1, 2, 4 at block level (approximation; the falloff is uncertain).
    #[test]
    fn vignette_shape() {
        let src = EffectSurface::from_fn(41, 31, |_, _| [1.0; 4]);
        let rect = crate::pixel::IntRect::new(0, 0, 41, 31);
        let v = d2d_vignette(&src, rect, [0.0, 0.0, 0.0, 1.0], 0.5, 0.5);
        assert!((v.at(20, 15)[0] - 1.0).abs() < 1e-3);
        assert!(v.at(0, 0)[0] < v.at(20, 0)[0] && v.at(20, 0)[0] < v.at(20, 15)[0]);
        assert!((v.at(3, 4)[0] - v.at(37, 26)[0]).abs() < 1e-4);
        assert_eq!(
            d2d_vignette(&src, rect, [0.0, 0.0, 0.0, 1.0], 0.5, 0.0).px,
            src.px
        );
    }

    #[test]
    fn premultiplied_composites() {
        assert_eq!(
            source_over([0.0, 0.0, 1.0, 1.0], [0.5, 0.0, 0.0, 0.5]),
            [0.5, 0.0, 0.5, 1.0]
        );
        assert_eq!(with_alpha([0.25, 0.0, 0.0, 0.5], 1.0), [0.5, 0.0, 0.0, 1.0]);
        assert_eq!(
            straight_over([0.0, 0.0, 1.0, 1.0], [1.0, 0.0, 0.0, 0.5]),
            [0.5, 0.0, 0.5, 1.0]
        );
        assert_eq!(swap_alpha([0.5, 0.5, 0.5, 1.0], 0.5), [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(straight_over([0.3; 4], [0.0; 4]), [0.3; 4]);
        assert_eq!(straight_over([0.0; 4], [0.0; 4]), [0.0; 4]);
        assert_eq!(colour_raw([255, 140, 0, 128])[0], 128.0 / 255.0);
        assert_eq!(
            colour_exact([0, 0, 0, 51], GPU_CONTEXT),
            [0.0, 0.0, 0.0, 0.2]
        );
        assert_eq!([quantise8(-0.2), quantise8(f32::NAN)], [0.0, 0.0]);
    }

    /// object-bevel-object.md "Extent and extremes": strength 0 draws nothing; NaN draws nothing.
    #[test]
    fn lighting_strength_zero_and_nan() {
        assert_eq!(lighting(1.0, 0.0, 0.001), None);
        assert_eq!(lighting(f32::NAN, 1.0, 0.0), None);
        // The band is inclusive (object-bevel-object.md step 5: "0.499 ≤ r ≤ 0.501").
        assert_eq!(lighting(0.75, 1.0, 0.25), None);
        let o = |r| lighting(r, 0.5, 0.001).map(|(sh, o)| (sh, (o * 1e4).round() / 1e4));
        assert_eq!(o(1.2), Some((Shade::Light, 0.7)));
        assert_eq!(o(-0.5), Some((Shade::Dark, 0.99)));
    }

    /// photo-vignette-plus.md Expected results 2 (never brighter toward a corner), 3 (red at
    /// strength 1, radius 1) and 5 (fitted to the selection rectangle; transparent outside it until
    /// Paint.NET's clip restores the source) at block level, at the spec's sizes.
    #[test]
    fn vignette_colour_monotony_and_rect() {
        let white = |w, h| EffectSurface::from_fn(w, h, |_, _| [1.0; 4]);
        let src = white(401, 301);
        let full = crate::pixel::IntRect::new(0, 0, 401, 301);
        let v = d2d_vignette(&src, full, [0.0, 0.0, 0.0, 1.0], 0.5, 0.5);
        let diag: Vec<f32> = (0..=100)
            .map(|k| v.at(200 - 2 * k, 150 - 3 * k / 2)[0])
            .collect();
        assert!(diag.windows(2).all(|w| w[1] <= w[0]), "{diag:?}");
        assert_eq!(v.at(200, 150), [1.0; 4]);
        let red = d2d_vignette(&src, full, [1.0, 0.0, 0.0, 1.0], 1.0, 1.0);
        for (x, y) in [(0, 0), (400, 0), (0, 300), (400, 300)] {
            let p = red.at(x, y);
            assert!(p[0] == 1.0 && p[1] < 0.1 && p[2] < 0.1, "({x}, {y}) {p:?}");
        }
        let src = white(600, 400);
        let half = crate::pixel::IntRect::new(0, 0, 300, 400);
        let v = d2d_vignette(&src, half, [0.0, 0.0, 0.0, 1.0], 0.5, 0.5);
        assert_eq!(v.at(150, 200), [1.0; 4]);
        assert!((v.at(10, 20)[0] - v.at(289, 379)[0]).abs() < 1e-4 && v.at(10, 20)[0] < 0.9);
        assert_eq!((v.at(300, 200), v.at(599, 0)), ([0.0; 4], [0.0; 4]));
        let off = crate::pixel::IntRect::new(10, 20, 30, 40);
        let v = d2d_vignette(&white(60, 70), off, [0.0, 0.0, 0.0, 1.0], 0.5, 0.5);
        assert!((v.at(10, 20)[0] - v.at(39, 59)[0]).abs() < 1e-4 && v.at(10, 20)[0] < 0.9);
        assert_eq!((v.at(24, 39), v.at(9, 30)), ([1.0; 4], [0.0; 4]));
        let src = white(5, 5);
        let rect = crate::pixel::IntRect::new(0, 0, 5, 5);
        // No transition or no strength: the source untouched, even outside a partial rect.
        let part = crate::pixel::IntRect::new(0, 0, 3, 3);
        for (t, s) in [(0.0, 0.5), (0.5, 0.0)] {
            let v = d2d_vignette(&src, part, [0.0, 0.0, 0.0, 1.0], t, s);
            assert_eq!(v.px, src.px);
        }
        // Q18: a transparent corner (e = 0.8, transition 1) takes the colour at alpha cov ≈ 0.905.
        let clear = d2d_vignette(
            &EffectSurface::new(5, 5),
            rect,
            [0.0, 0.0, 0.0, 1.0],
            1.0,
            1.0,
        );
        assert!((clear.at(0, 0)[3] - 0.9045).abs() < 1e-3 && clear.at(2, 2)[3] == 0.0);
        // Q18's other reading (`VIGNETTE_ALPHA_MODE` Straight): the straight colour mixes, alpha stays.
        let (p, c) = ([0.25, 0.25, 0.25, 0.5], [0.0, 0.0, 0.0, 0.5]);
        assert_eq!(
            vignette_over(p, c, Alpha::Premultiplied),
            [0.125, 0.125, 0.125, 0.75]
        );
        assert_eq!(
            vignette_over(p, c, Alpha::Straight),
            [0.125, 0.125, 0.125, 0.5]
        );
    }

    /// adjustments-combined-adjustments.md "Sepia" / "Black and White" (approximations).
    #[test]
    fn sepia_and_grayscale() {
        let r = |c: [f32; 3]| c.map(|v| (v * 1e4).round() / 1e4);
        assert_eq!(r(d2d_sepia([1.0; 3])), [1.1755, 1.1015, 0.9685]);
        assert_eq!(r(d2d_sepia([0.2, 0.4, 0.6])), [0.3498, 0.4225, 0.4733]);
        assert_eq!(d2d_sepia([0.0; 3]), [0.0; 3]);
        assert_eq!(d2d_grayscale([1.0, 0.0, 0.0]), [0.299; 3]);
        assert_eq!(d2d_grayscale([0.0, 0.0, 1.0]), [0.114; 3]);
    }

    /// §5.7 2-D affine transform: the identity and a one-pixel shift reproduce texels exactly, a
    /// transpose swaps the axes (m12 and m21 in Direct2D's order), a half-pixel shift is
    /// Catmull-Rom (exact on a ramp) and taps past the image follow `edge`.
    #[test]
    fn affine_cubic_maps_destination_to_source() {
        let src = EffectSurface::from_fn(6, 6, |x, y| [x as f32 / 8.0, y as f32 / 8.0, 0.0, 1.0]);
        let id = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
        assert_eq!(affine_cubic(&src, id, Edge::Clamp).px, src.px);
        let shift = affine_cubic(&src, [1.0, 0.0, 0.0, 1.0, 1.0, 0.0], Edge::Transparent);
        assert_eq!((shift.at(2, 3), shift.at(5, 3)), (src.at(3, 3), [0.0; 4]));
        let t = affine_cubic(&src, [0.0, 1.0, 1.0, 0.0, 0.0, 0.0], Edge::Clamp);
        assert_eq!(t.at(1, 4), [4.0 / 8.0, 1.0 / 8.0, 0.0, 1.0]);
        // A quarter turn, s = (6 − p_y, p_x): out(x, y) = src(5 − y, x).
        let turn = affine_cubic(&src, [0.0, 1.0, -1.0, 0.0, 6.0, 0.0], Edge::Clamp);
        assert_eq!(turn.at(1, 4), src.at(1, 1));
        assert_eq!(turn.at(3, 0), src.at(5, 3));
        let half = affine_cubic(&src, [1.0, 0.0, 0.0, 1.0, 0.5, 0.0], Edge::Clamp);
        assert!((half.at(2, 2)[0] - 2.5 / 8.0).abs() < 1e-6);
    }

    /// §5.7 Morphology erode (Feather Object): the minimum over the (2r + 1)² window; r = 0 is the
    /// identity.
    #[test]
    fn morph_min_erodes_a_square_window() {
        let src = grey(7, 7, |x, y| if (x, y) == (3, 3) { 0.0 } else { 1.0 });
        let m = morph_min(&src, 1, Edge::Clamp);
        assert_eq!(
            (m.at(2, 4)[0], m.at(4, 2)[0], m.at(1, 3)[0]),
            (0.0, 0.0, 1.0)
        );
        assert_eq!(morph_min(&src, 0, Edge::Clamp).px, src.px);
        assert_eq!(morph_min(&src, 1, Edge::Transparent).at(0, 0)[3], 0.0);
    }

    /// §5.1: the soft border fades an opaque layer at the canvas edge; σ 0 is the identity.
    #[test]
    fn blur_soft_is_transparent_outside() {
        let src = grey(9, 9, |_, _| 1.0);
        let b = blur_soft(&src, 2.0);
        assert!(b.at(0, 0)[3] < 0.5 && (b.at(4, 4)[3] - 1.0).abs() < 0.1);
        assert_eq!(blur_soft(&src, 0.0).px, src.px);
    }

    /// The re-exported Paint.NET blocks take what the plugins pass: the Emboss any angle (Text Fun
    /// Factory's −45°, while the built-in's slider clamps to 0…360), Bokeh any quality (clamped to
    /// 1…10), the Drop Shadow a premultiplied working-space colour, Hue/Saturation and
    /// Temperature/Tint their neutral values.
    #[test]
    fn re_exported_blocks_take_plugin_arguments() {
        let m = grey(9, 9, |x, y| ((x * 3 + y * 5) % 7) as f32 / 7.0);
        assert_eq!(emboss_surface(&m, -45.0).px, emboss_surface(&m, 315.0).px);
        let img = crate::effect::test_util::from_fn(9, 9, |x, y| {
            let v = ((x * 3 + y * 5) % 7 * 36) as u8;
            [v, v, v, 255]
        });
        let run = |a: f64| {
            crate::effect::test_util::run("embossEffect", serde_json::json!({"angle": a}), &img)
        };
        assert_eq!(run(-45.0).data, run(0.0).data);
        assert_eq!(bokeh(&m, 2.0, 0, 0.0).px, bokeh(&m, 2.0, 1, 0.0).px);
        assert_eq!(bokeh(&m, 2.0, 99, 0.0).px, bokeh(&m, 2.0, 10, 0.0).px);
        assert_ne!(bokeh(&m, 2.0, 10, 0.0).px, bokeh(&m, 2.0, 9, 0.0).px);
        let quality = |q: u32| {
            let v = serde_json::json!({"radius": 2, "quality": q});
            crate::effect::test_util::run("bokehEffect", v, &img).data
        };
        assert_ne!(quality(1), quality(10));
        let mut dot = EffectSurface::new(7, 5);
        dot.px[2 * 7 + 1] = [1.0; 4];
        let s = drop_shadow_with(&dot, 0.0, [2.0, 0.0], [0.1, 0.0, 0.0, 0.5], true);
        assert_eq!((s.at(3, 2), s.at(1, 2)), ([0.1, 0.0, 0.0, 0.5], [0.0; 4]));
        let over = drop_shadow_with(&dot, 0.0, [2.0, 0.0], [0.1, 0.0, 0.0, 0.5], false);
        assert_eq!(over.at(1, 2), [1.0; 4]);
        let rgb = [0.8, 0.3, 0.1];
        let same = hsl_adjust(rgb, 0.0, 100.0, 0.0);
        assert!(same.iter().zip(rgb).all(|(a, b)| (a - b).abs() < 1e-6));
        assert_eq!(hsl_adjust(rgb, 0.0, 100.0, 100.0), [1.0; 3]);
        let g = hsl_adjust([1.0, 0.0, 0.0], 0.0, 0.0, 0.0);
        assert!(g[0] == g[1] && g[1] == g[2]);
        assert_eq!(temp_tint_linear(rgb, 0.0, 0.0), rgb);
        let warm = temp_tint_linear([0.5; 3], 1.0, 0.0);
        assert!(warm[0] > 0.5 && warm[2] < 0.5 && warm[1] == 0.5);
        assert_eq!(rgb_to_hsl(1.0, 0.0, 0.0), [0.0, 1.0, 0.5]);
        let rz: crate::effect::Values =
            serde_json::from_value(serde_json::json!({"zoom": 1.0})).unwrap();
        assert_eq!(rotate_zoom_with(&m, &rz).px, m.px);
        assert_eq!(bc_shift([0.25; 3], 0.0, 0.0), [0.25; 3]);
    }
}
