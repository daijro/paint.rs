//! Ported from PhotoCraft crates/algo/src/video.rs @ ff47388 (MIT OR Apache-2.0, used under MIT). Changes: interleaved straight RGBA f32 buffers (w×h, alpha last) and `extra::geom::Rect` bounds instead of `Image`/`Ctx`; the CMYK and non-RGB colour-model branches dropped; tests rewritten on RGBA buffers (the depth and no-alpha cases drop out with 8-bit RGBA only).

use crate::extra::geom::Rect;

/// De-Interlace: removes one field (odd = lines 1, 3, 5... counted from the bounds' top, i.e.
/// even offsets) and rebuilds it from the neighbouring kept lines by duplication or
/// (premultiplied) interpolation. Pixels outside `b` are untouched.
pub fn deinterlace(
    src: &[f32],
    w: usize,
    h: usize,
    b: Rect,
    eliminate_even: bool,
    interpolate: bool,
) -> Vec<f32> {
    let mut res = src.to_vec();
    let at = |x: i32, y: i32, c: usize| src[(y as usize * w + x as usize) * 4 + c];
    for y in b.y0.max(0)..b.y1.min(h as i32) {
        // 1-based line number parity: offset 0 is line 1 (odd).
        let odd_line = (y - b.y0) % 2 == 0;
        if odd_line == eliminate_even {
            continue;
        }
        let above = (y > b.y0).then_some(y - 1);
        let below = (y + 1 < b.y1).then_some(y + 1);
        let (rows, cnt) = match (interpolate, above, below) {
            (true, Some(a), Some(c)) => ([a, c], 2),
            (_, Some(a), _) => ([a, a], 1),
            (_, None, Some(c)) => ([c, c], 1),
            _ => continue,
        };
        let k = 1.0 / cnt as f32;
        for x in b.x0.max(0)..b.x1.min(w as i32) {
            let mut acc = [0.0f32; 4];
            for &yy in &rows[..cnt] {
                let a = at(x, yy, 3);
                for (c, v) in acc.iter_mut().enumerate().take(3) {
                    *v += at(x, yy, c) * a * k;
                }
                acc[3] += a * k;
            }
            let a = acc[3];
            for v in &mut acc[..3] {
                *v = if a > 1e-7 { *v / a } else { 0.0 };
            }
            let i = (y as usize * w + x as usize) * 4;
            res[i..i + 4].copy_from_slice(&acc);
        }
    }
    res
}

/// Composite-signal limits (fractions of the 7.5-100 IRE luma range): broadcast-safe is
/// -20...120 IRE, i.e. Y +- C within these bounds.
const NTSC_MAX: f32 = (120.0 - 7.5) / 92.5;
const NTSC_MIN: f32 = (-20.0 - 7.5) / 92.5;

/// NTSC Colors: scales chroma (in YIQ) so the composite signal Y +- C stays within broadcast
/// limits; hue and luma are kept.
pub fn ntsc(buf: &mut [f32]) {
    for px in buf.as_chunks_mut::<4>().0 {
        let c = *px;
        let yy = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
        let ii = 0.595_716 * c[0] - 0.274_453 * c[1] - 0.321_263 * c[2];
        let qq = 0.211_456 * c[0] - 0.522_591 * c[1] + 0.311_135 * c[2];
        let chroma = (ii * ii + qq * qq).sqrt();
        if chroma <= 1e-7 {
            continue;
        }
        let mut k: f32 = 1.0;
        if yy + chroma > NTSC_MAX {
            k = k.min(((NTSC_MAX - yy) / chroma).max(0.0));
        }
        if yy - chroma < NTSC_MIN {
            k = k.min(((yy - NTSC_MIN) / chroma).max(0.0));
        }
        if k >= 1.0 {
            continue;
        }
        let (i2, q2) = (ii * k, qq * k);
        px[0] = yy + 0.956_3 * i2 + 0.621_0 * q2;
        px[1] = yy - 0.272_1 * i2 - 0.647_4 * q2;
        px[2] = yy - 1.107_0 * i2 + 1.704_6 * q2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One column of RGBA rows.
    fn rows(pixels: &[[f32; 4]]) -> Vec<f32> {
        pixels.iter().flatten().copied().collect()
    }

    fn row(buf: &[f32], y: usize) -> [f32; 4] {
        buf[y * 4..y * 4 + 4].try_into().unwrap()
    }

    #[test]
    fn deinterlace_all_field_and_interpolation_choices_use_bounds_relative_rows() {
        // Odd document origin: parity is relative to the bounds, not absolute y.
        let pixels = [0.0, 0.2, 0.6, 0.8, 1.0].map(|v| [v, 1.0 - v, v / 2.0, 1.0]);
        let src = rows(&pixels);
        let b = Rect::new(0, 0, 1, 5);
        for (eliminate_even, interpolate, neighbours) in [
            (false, false, [(1, 1), (1, 1), (1, 1), (3, 3), (3, 3)]),
            (false, true, [(1, 1), (1, 1), (1, 3), (3, 3), (3, 3)]),
            (true, false, [(0, 0), (0, 0), (2, 2), (2, 2), (4, 4)]),
            (true, true, [(0, 0), (0, 2), (2, 2), (2, 4), (4, 4)]),
        ] {
            let out = deinterlace(&src, 1, 5, b, eliminate_even, interpolate);
            for (r, (a, c)) in neighbours.into_iter().enumerate() {
                let actual = row(&out, r);
                for ch in 0..4 {
                    let expected = (pixels[a][ch] + pixels[c][ch]) / 2.0;
                    assert!(
                        (actual[ch] - expected).abs() <= 1e-6,
                        "even={eliminate_even}, interp={interpolate}, row={r}: {actual:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn deinterlace_interpolation_weights_colour_by_alpha() {
        let b = Rect::new(0, 0, 1, 3);
        for [a, c] in [[0.25, 0.75], [1.0, 0.0], [0.0, 0.0]] {
            let src = rows(&[[1.0, 0.0, 0.0, a], [0.0, 1.0, 0.0, 1.0], [0.0, 0.0, 1.0, c]]);
            let out = deinterlace(&src, 1, 3, b, true, true);
            let total = a + c;
            let expected = if total > 0.0 {
                [a / total, 0.0, c / total, total / 2.0]
            } else {
                [0.0; 4]
            };
            let actual = row(&out, 1);
            for (v, want) in actual.iter().zip(expected) {
                assert!((v - want).abs() <= 1e-6, "{actual:?} != {expected:?}");
            }
            assert_eq!(row(&out, 0), row(&src, 0));
            assert_eq!(row(&out, 2), row(&src, 2));
        }
    }

    #[test]
    fn deinterlace_one_and_two_row_images_handle_missing_neighbours() {
        for height in [1usize, 2] {
            let pixels: Vec<[f32; 4]> = [[0.2; 4], [0.8; 4]][..height].to_vec();
            let src = rows(&pixels);
            let b = Rect::new(0, 0, 1, height as i32);
            for eliminate_even in [false, true] {
                for interpolate in [false, true] {
                    let out = deinterlace(&src, 1, height, b, eliminate_even, interpolate);
                    let kept = if height == 2 && !eliminate_even { 1 } else { 0 };
                    for y in 0..height {
                        let (a, b) = (row(&out, y), pixels[kept]);
                        assert!(
                            a.iter().zip(b).all(|(p, q)| (p - q).abs() < 1e-6),
                            "height={height}, even={eliminate_even}, interp={interpolate}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn deinterlace_leaves_pixels_outside_reference_bounds_unchanged() {
        let src = rows(&[[0.1; 4], [0.2; 4], [0.4; 4], [0.8; 4], [1.0; 4]]);
        let b = Rect::new(0, 1, 1, 4);
        for eliminate_even in [false, true] {
            for interpolate in [false, true] {
                let out = deinterlace(&src, 1, 5, b, eliminate_even, interpolate);
                assert_eq!(row(&out, 0), row(&src, 0));
                assert_eq!(row(&out, 4), row(&src, 4));
            }
        }
    }

    #[test]
    fn ntsc_safe_colours_and_alpha_are_bit_exact() {
        let src = rows(&[
            [0.0, 0.0, 0.0, 0.0],
            [0.5, 0.5, 0.5, 0.25],
            [1.0, 1.0, 1.0, 0.75],
            [0.2, 0.3, 0.4, 1.0],
            [1.0, 0.0, 1.0, 1.0],
        ]);
        let mut out = src.clone();
        ntsc(&mut out);
        assert_eq!(out, src);
    }

    fn luma_chroma(p: &[f32]) -> (f32, f32) {
        let y = 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
        let i = 0.595716 * p[0] - 0.274453 * p[1] - 0.321263 * p[2];
        let q = 0.211456 * p[0] - 0.522591 * p[1] + 0.311135 * p[2];
        (y, i.hypot(q))
    }

    #[test]
    fn ntsc_saturated_colours_keep_luma_alpha_and_meet_composite_limits() {
        // Independent signal checks: -20..120 IRE relative to the 7.5..100 IRE luma range.
        let (lo, hi) = ((-20.0 - 7.5) / 92.5, (120.0 - 7.5) / 92.5);
        let colours = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.25],
            [1.0, 1.0, 0.0, 0.5],
            [0.0, 1.0, 1.0, 0.75],
        ];
        let src = rows(&colours);
        let mut out = src.clone();
        ntsc(&mut out);
        // The rounded YIQ inverse contributes < 0.0003.
        let eps = 0.0003 + 2.0e-6;
        for y in 0..colours.len() {
            let (before, after) = (row(&src, y), row(&out, y));
            let (y0, c0) = luma_chroma(&before);
            let (y1, c1) = luma_chroma(&after);
            assert!(
                y0 - c0 < lo || y0 + c0 > hi,
                "fixture must require correction"
            );
            assert!(c1 < c0, "chroma not reduced: {after:?}");
            assert!(
                (y1 - y0).abs() <= eps,
                "luma changed: {before:?} -> {after:?}"
            );
            assert!(
                y1 - c1 >= lo - eps && y1 + c1 <= hi + eps,
                "signal out of gamut: {after:?}"
            );
            assert!(
                after[..3]
                    .iter()
                    .all(|v| v.is_finite() && *v >= -1e-3 && *v <= 1.0 + 1e-3)
            );
            assert_eq!(after[3], before[3], "alpha changed");
        }
    }
}
