//! Paint.NET's integer pixel kernels on straight RGBA8.
//! The composition kernel, Merge Down, Overwrite `Blend`, Move/Paste, the primitives and blend
//! functions, applying a mask and the effect selection blend.

use super::{IntRect, Px};
use crate::document::layer::BlendMode;
use crate::document::surface::Surface;

/// FS, the rounded 8-bit scale (§3.1): r = x·f + 128, ⌊(r + ⌊r/256⌋)/256⌋ = round(x·f/255).
pub fn fs(x: u8, f: u8) -> u8 {
    let r = x as u32 * f as u32 + 128;
    ((r + (r >> 8)) >> 8) as u8
}

/// The MAS-table division (§2.3 step 6): exact floor, 0 when `d` = 0.
pub fn div_floor(n: u32, d: u32) -> u32 {
    n.checked_div(d).unwrap_or(0)
}

/// f(d, s) per channel on straight bytes (§3.2 table).
pub fn blend_fn(mode: BlendMode, d: u8, s: u8) -> u8 {
    use BlendMode::*;
    let (di, si) = (d as u32, s as u32);
    match mode {
        Normal => s,
        Multiply => fs(d, s),
        Additive => (di + si).min(255) as u8,
        ColorBurn if s == 0 => 0,
        ColorBurn => 255u32.saturating_sub((255 - di) * 255 / si) as u8,
        ColorDodge if s == 255 => 255,
        ColorDodge => (di * 255 / (255 - si)).min(255) as u8,
        Reflect if s == 255 => 255,
        Reflect => (di * di / (255 - si)).min(255) as u8,
        Glow if d == 255 => 255,
        Glow => (si * si / (255 - di)).min(255) as u8,
        Overlay if d < 128 => fs(2 * d, s),
        Overlay => 255 - fs(2 * (255 - d), 255 - s),
        Difference => d.abs_diff(s),
        Negation => (255 - (255 - d as i32 - s as i32).abs()) as u8,
        Lighten => d.max(s),
        Darken => d.min(s),
        Screen => (si + di - fs(s, d) as u32) as u8,
        Xor => d ^ s,
    }
}

/// The composition kernel (§2.3): backdrop `d`, source `s` at layer opacity `opacity`. Appendix B
/// B-B2 calibrates it on real files.
pub fn compose(mode: BlendMode, d: Px, s: Px, opacity: u8) -> Px {
    if opacity == 0 {
        return d;
    }
    let sa = if opacity == 255 {
        s[3]
    } else {
        fs(s[3], opacity)
    };
    if sa == 0 {
        return d;
    }
    let y = fs(d[3], 255 - sa) as u32;
    let x = fs(d[3], sa) as u32;
    let z = sa as u32 - x;
    let t = y + sa as u32;
    let ch = |c: usize| {
        let f = blend_fn(mode, d[c], s[c]) as u32;
        div_floor(d[c] as u32 * y + s[c] as u32 * z + f * x, t) as u8
    };
    [ch(0), ch(1), ch(2), t as u8]
}

/// `compose` per RGBA pixel of two equal-length rows.
pub fn compose_row(mode: BlendMode, dst: &mut [u8], src: &[u8], opacity: u8) {
    for (d, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(src.as_chunks::<4>().0)
    {
        *d = compose(mode, *d, *s, opacity);
    }
}

/// A ← FS(A, o) for every pixel when o < 255; colour untouched (§2.2 step 3).
pub fn scale_alpha_row(row: &mut [u8], opacity: u8) {
    if opacity == 255 {
        return;
    }
    for p in row.as_chunks_mut::<4>().0 {
        p[3] = fs(p[3], opacity);
    }
}

/// The Overwrite blend (§2.7, §3.1): a lerp in premultiplied space with exact floor
/// division. Overwrite tools, Fill/Erase Selection, Eraser and effect clipping
/// (`blend(original, effect, mask)`, adjustments-and-effect-framework.md §2.5).
pub fn blend(a: Px, b: Px, m: u8) -> Px {
    match m {
        0 => a,
        255 => b,
        _ => {
            let ca = fs(255 - m, a[3]) as u32;
            let cb = fs(m, b[3]) as u32;
            let t = ca + cb;
            if t == 0 {
                return [0; 4];
            }
            let ch = |c: usize| div_floor(a[c] as u32 * ca + b[c] as u32 * cb, t) as u8;
            [ch(0), ch(1), ch(2), t as u8]
        }
    }
}

/// The alpha mask (selection-and-move.md §2.4, document-ops-and-formats.md §2.4):
/// A′ = FS(A, m); m = 0 clears the pixel.
pub fn apply_alpha_mask(p: Px, m: u8) -> Px {
    if m == 0 {
        [0; 4]
    } else {
        [p[0], p[1], p[2], fs(p[3], m)]
    }
}

/// Move Selected Pixels / Paste per pixel (§2.8; selection-and-move.md §2.9 step 6): layer pixel
/// `l`, transformed source `x`, `tc` = min(source coverage, final mask), `bm` = base-hole mask
/// (0 for paste or Leave Copy Behind). Appendix B B-S2 calibrates it.
pub fn move_pixel(l: Px, x: Px, tc: u8, bm: u8) -> Px {
    match tc {
        255 => x,
        0 => match 255 - bm {
            0 => [0; 4],
            255 => l,
            h => [l[0], l[1], l[2], fs(l[3], h)],
        },
        c => {
            let w = (255 - c).min(255 - bm) as u32;
            let (la, xa) = (l[3] as u32 * w, x[3] as u32 * c as u32);
            let total = la + xa;
            if total == 0 {
                return [0; 4];
            }
            let ch = |k: usize| div_floor(la * l[k] as u32 + xa * x[k] as u32, total) as u8;
            // The exact-floor divide by 255: a truncating ⌊W/255⌋.
            [ch(0), ch(1), ch(2), (total / 255) as u8]
        }
    }
}

/// Extra (Move Selected Pixels' Transparent option): [`move_pixel`] with `x` at coverage `tc`
/// laid over the layer pixel after the base hole, so where `x` is transparent (the Transparent
/// colour knocked out) the pixel under it shows.
pub fn move_pixel_over(l: Px, x: Px, tc: u8, bm: u8) -> Px {
    let under = move_pixel(l, x, 0, bm);
    let xa = x[3] as u32 * tc as u32;
    let ua = under[3] as u32 * (255 * 255 - xa) / 255;
    let total = xa + ua;
    if total == 0 {
        return [0; 4];
    }
    let ch = |k: usize| ((xa * x[k] as u32 + ua * under[k] as u32) / total) as u8;
    [ch(0), ch(1), ch(2), (total / 255) as u8]
}

/// Merge Down (§2.5): `compose_row` of `src` into `dst` over `rect ∩` both bounds.
pub fn compose_surface(
    mode: BlendMode,
    dst: &mut Surface,
    src: &Surface,
    opacity: u8,
    rect: IntRect,
) {
    let r = rect
        .intersect(&dst.int_bounds())
        .intersect(&src.int_bounds());
    for y in r.y..r.bottom() {
        let (d, s) = (dst.span(y, r), src.span(y, r));
        compose_row(mode, &mut dst.data_mut()[d], &src.data()[s], opacity);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::layer::BlendMode::{self, *};

    #[test]
    fn fs_is_rounded_scale_for_every_byte_pair() {
        for x in 0..=255u32 {
            for f in 0..=255u32 {
                assert_eq!(fs(x as u8, f as u8) as u32, (x * f + 127) / 255, "{x} {f}");
            }
        }
    }

    /// compositing.md §3.2 worked examples (R, G, B, A).
    #[test]
    fn worked_examples_for_every_mode() {
        let (d1, s1) = ([200, 120, 40, 255], [90, 180, 250, 255]);
        let (d2, s2) = ([200, 120, 40, 160], [90, 180, 250, 220]);
        let table: [(BlendMode, Px, Px); 14] = [
            (Normal, [113, 167, 204, 255], [115, 166, 202, 224]),
            (Multiply, [98, 92, 39, 255], [105, 120, 99, 224]),
            (Additive, [243, 225, 208, 255], [195, 202, 204, 224]),
            (ColorBurn, [121, 76, 36, 255], [119, 109, 98, 224]),
            (ColorDodge, [243, 225, 208, 255], [195, 202, 204, 224]),
            (Reflect, [232, 176, 208, 255], [189, 172, 204, 224]),
            (Glow, [158, 214, 208, 255], [142, 195, 204, 224]),
            (Overlay, [187, 158, 69, 255], [160, 160, 118, 224]),
            (Difference, [129, 72, 173, 255], [124, 107, 182, 224]),
            (Negation, [215, 190, 181, 255], [178, 180, 187, 224]),
            (Lighten, [200, 167, 204, 255], [168, 166, 202, 224]),
            (Darken, [113, 120, 40, 255], [115, 137, 100, 224]),
            (Screen, [214, 194, 205, 255], [177, 183, 202, 224]),
            (Xor, [157, 185, 173, 255], [142, 178, 182, 224]),
        ];
        for (mode, ex1, ex2) in table {
            assert_eq!(compose(mode, d1, s1, 200), ex1, "{mode:?} Ex1");
            assert_eq!(compose(mode, d2, s2, 200), ex2, "{mode:?} Ex2");
        }
    }

    /// The kernel reduces to f(d, s) when both pixels are opaque at opacity 255.
    #[test]
    fn opaque_compose_is_the_blend_function() {
        for mode in BlendMode::ALL {
            for d in 0..=255u8 {
                for s in 0..=255u8 {
                    let f = blend_fn(mode, d, s);
                    assert_eq!(
                        compose(mode, [d, d, d, 255], [s, s, s, 255], 255),
                        [f, f, f, 255],
                        "{mode:?} {d} {s}"
                    );
                }
            }
        }
    }

    /// compositing.md §3.3.
    #[test]
    fn paint_net_vectors() {
        assert_eq!(
            compose(Normal, [1, 2, 3, 0], [9, 9, 9, 1], 100),
            [1, 2, 3, 0]
        );
        assert_eq!(
            compose(Multiply, [50, 60, 70, 0], [100, 150, 200, 128], 255),
            [100, 150, 200, 128]
        );
        assert_eq!(blend_fn(ColorBurn, 255, 0), 0);
        assert_eq!(blend_fn(ColorDodge, 0, 255), 255);
        assert_eq!(blend_fn(Overlay, 127, 200), 199);
        assert_eq!(blend_fn(Overlay, 128, 200), 200);
        assert_eq!(
            blend([255, 0, 0, 255], [0, 0, 255, 255], 128),
            [127, 0, 128, 255]
        );
        assert_eq!(
            blend([200, 100, 50, 180], [20, 240, 90, 60], 77),
            [177, 117, 55, 144]
        );
        // Erase / Eraser and effect clip: transparent content through m = 128.
        assert_eq!(blend([255, 0, 0, 255], [0, 0, 0, 0], 128), [255, 0, 0, 127]);
        assert_eq!(blend([9, 9, 9, 9], [1, 1, 1, 1], 0), [9, 9, 9, 9]);
        assert_eq!(blend([9, 9, 9, 9], [1, 1, 1, 1], 255), [1, 1, 1, 1]);
        // Move edge, move hole, paste over content (§2.8).
        assert_eq!(
            move_pixel([0, 0, 255, 255], [255, 0, 0, 128], 128, 0),
            [85, 0, 169, 191]
        );
        assert_eq!(
            move_pixel([0, 0, 255, 255], [7, 7, 7, 7], 0, 128),
            [0, 0, 255, 127]
        );
        assert_eq!(
            move_pixel([0, 0, 255, 255], [0, 0, 0, 0], 255, 0),
            [0, 0, 0, 0]
        );
        assert_eq!(
            move_pixel([0, 0, 255, 255], [7, 7, 7, 7], 0, 255),
            [0, 0, 0, 0]
        );
    }

    /// adjustments-and-effect-framework.md §5 Task 37: the selection blend of effect output.
    #[test]
    fn effect_selection_blend_vectors() {
        assert_eq!(
            blend([200, 100, 50, 255], [55, 155, 205, 255], 128),
            [127, 127, 127, 255]
        );
        assert_eq!(
            blend([200, 100, 50, 255], [10, 20, 30, 64], 128),
            [161, 83, 45, 159]
        );
        assert_eq!(
            blend([200, 100, 50, 0], [10, 20, 30, 255], 1),
            [10, 20, 30, 1]
        );
        assert_eq!(blend([0, 0, 0, 0], [9, 9, 9, 0], 77), [0, 0, 0, 0]);
        assert_eq!(blend([0, 0, 0, 255], [3, 3, 3, 255], 128), [1, 1, 1, 255]);
    }

    /// document-ops-and-formats.md §5 Task 43 (Crop) / selection-and-move.md §2.4 (copy).
    #[test]
    fn alpha_mask_vectors() {
        assert_eq!(apply_alpha_mask([1, 2, 3, 255], 128), [1, 2, 3, 128]);
        assert_eq!(apply_alpha_mask([1, 2, 3, 200], 100), [1, 2, 3, 78]);
        assert_eq!(apply_alpha_mask([1, 2, 3, 200], 0), [0, 0, 0, 0]);
    }
}
