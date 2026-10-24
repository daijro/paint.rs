//! Paint.NET's thumbnails: integer aspect fit, MagicScaler Catmull-Rom in linear light, the Small
//! checkerboard and the drop-shadow ring, blended in the display mode's space. The document strip
//! (Task 18), the Layers form (Task 19), the document list (Task 44) and the unsaved-changes
//! previews (Task 45) share it.
use crate::pixel::{IntRect, Px};
use crate::render::checker::{checker_colors, checker_is_light, checker_square};
use crate::render::display::{DisplayTarget, RenderMode};
use crate::resample::magic::{RowScaler, narrow};
use crate::resample::{Gamma, Kernel, accepts, widen};

/// §2.8 Size: integer aspect fit within `max_edge`, never upscaled. W > H gives
/// (min(W, M), max(1, H·w′ ÷ W)), H > W the mirror, a square min(W, M) both ways. An empty
/// source or a zero edge gives (0, 0).
pub fn thumbnail_size(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    if w == 0 || h == 0 || max_edge == 0 {
        return (0, 0);
    }
    let fit = |long: u32, short: u32| {
        let l = long.min(max_edge);
        (l, ((short as u64 * l as u64) / long as u64).max(1) as u32)
    };
    match w.cmp(&h) {
        std::cmp::Ordering::Greater => fit(w, h),
        std::cmp::Ordering::Less => {
            let (th, tw) = fit(h, w);
            (tw, th)
        }
        std::cmp::Ordering::Equal => (w.min(max_edge), w.min(max_edge)),
    }
}

/// §2.8 Scaling: straight RGBA8 `w × h` to its `thumbnail_size`, through MagicScaler's
/// Catmull-Rom in linear premultiplied f32 (`resample::resize` with `ResizeMode::Bicubic`,
/// `Gamma::Linear`), 8-bit sRGB out. A source that already fits comes back unchanged.
/// `(vec![], 0, 0)` when the source is empty or too large to resample (`resize`'s empty result).
/// This is `ThumbnailScaler` run to completion.
pub fn scale_thumbnail(src: &[u8], w: u32, h: u32, max_edge: u32) -> (Vec<u8>, u32, u32) {
    let mut job = ThumbnailScaler::new(src, w, h, max_edge);
    job.step(src, usize::MAX);
    job.finish(src)
}

/// `scale_thumbnail` as a resumable job, for a build without threads (the web): `step(src, rows)`
/// runs at most `rows` row passes (`resample::magic` streams rows) and is true once done;
/// `finish(src)` then returns exactly `scale_thumbnail`'s result. Every call gets the same `src`.
pub struct ThumbnailScaler {
    src: (u32, u32),
    size: (u32, u32),
    /// `None` when nothing is resampled (the source fits, or `resize` refuses it).
    rows: Option<RowScaler>,
    out: Vec<u8>,
}

impl ThumbnailScaler {
    pub fn new(src: &[u8], w: u32, h: u32, max_edge: u32) -> Self {
        let size = thumbnail_size(w, h, max_edge);
        let (tw, th) = size;
        let scaled = size != (w, h) && tw > 0 && accepts(src.len(), w, h, tw, th);
        ThumbnailScaler {
            src: (w, h),
            size,
            rows: scaled.then(|| {
                let (w, h, tw, th) = (w as usize, h as usize, tw as usize, th as usize);
                RowScaler::new((w, h), (tw, th), Kernel::CatmullRom)
            }),
            out: Vec::new(),
        }
    }

    /// `resize`'s MagicScaler path: source rows widened into the horizontal pass, output rows
    /// narrowed straight into the result.
    pub fn step(&mut self, src: &[u8], rows: usize) -> bool {
        let Some(job) = &mut self.rows else {
            return true;
        };
        let row_bytes = self.src.0 as usize * 4;
        let out = &mut self.out;
        job.step(
            rows,
            |y, line| {
                let row = src[y * row_bytes..][..row_bytes].as_chunks::<4>().0;
                for (p, &b) in line.iter_mut().zip(row) {
                    *p = widen(b, Gamma::Linear);
                }
            },
            |_, line| out.extend(line.iter().flat_map(|&p| narrow(p, Gamma::Linear))),
        )
    }

    /// The thumbnail (after `step` returned true): (RGBA8, width, height).
    pub fn finish(self, src: &[u8]) -> (Vec<u8>, u32, u32) {
        let ((w, h), (tw, th)) = (self.src, self.size);
        match self.rows {
            Some(_) => (self.out, tw, th),
            None if (tw, th) == (w, h) => (src.to_vec(), w, h),
            None => (Vec::new(), 0, 0),
        }
    }
}

/// P3 step 1: ring extent e = min(4, ⌈u⌉) (at least 1).
pub fn ring_extent(u: f64) -> u32 {
    u.ceil().clamp(1.0, 4.0) as u32
}

/// P3 step 2: per extent e = 1…4, each ring's alphas from the corner inward (ring 0 outermost).
pub const RING_ALPHAS: [&[&[u8]]; 4] = [
    &[&[15, 47, 63]],
    &[&[3, 9, 18, 25, 28], &[28, 56, 75, 85]],
    &[
        &[0, 2, 5, 9, 12, 14, 15],
        &[8, 17, 29, 38, 44, 47],
        &[35, 59, 77, 89, 95],
    ],
    &[
        &[0, 1, 2, 4, 6, 7, 8, 9, 10],
        &[3, 7, 12, 18, 23, 26, 29, 30],
        &[14, 24, 36, 46, 53, 58, 61],
        &[40, 61, 77, 89, 97, 102],
    ],
];

/// P3 steps 3–4, the outer shadow of the `w × h` thumbnail at (0, 0) with extent `e` (1…4):
/// the ring alpha at pixel (x, y). The outer shadow is the inner one of the rect inflated by e;
/// ring i is that rect inset by i (W_i × H_i) with table a of length n: corners a[0], the pixels δ =
/// 1 … min(n − 2, (W_i − 2) ÷ 2) from a corner along an edge a[δ] (H_i on the vertical edges),
/// the rest of the edge a[n − 1]. 0 inside the thumbnail and beyond the ring.
pub fn ring_alpha(w: u32, h: u32, e: u32, x: i32, y: i32) -> u8 {
    let e = e.clamp(1, 4) as i64;
    let (x, y) = (x as i64, y as i64);
    let (x0, y0, x1, y1) = (-e, -e, w as i64 + e - 1, h as i64 + e - 1);
    let i = (x - x0).min(y - y0).min(x1 - x).min(y1 - y);
    if i < 0 || i >= e {
        return 0;
    }
    let a = RING_ALPHAS[e as usize - 1][i as usize];
    let n = a.len() as i64;
    let (rx0, ry0, rx1, ry1) = (x0 + i, y0 + i, x1 - i, y1 - i);
    // Distance from the nearer corner along the pixel's edge, and that edge's length.
    let (d, len) = if y == ry0 || y == ry1 {
        ((x - rx0).min(rx1 - x), rx1 - rx0 + 1)
    } else {
        ((y - ry0).min(ry1 - y), ry1 - ry0 + 1)
    };
    let bound = (n - 2).min((len - 2) / 2);
    a[if d <= bound { d } else { n - 1 } as usize]
}

/// How a thumbnail is framed: `u` = device px per DIP (checker square, ring extent),
/// `checker_brightness` = `canvas.checkerboardBrightness` / 100, `dark` = Aero Dark (Dark and
/// Blue schemes), `checker` = draw the Small checkerboard under it (not in the document list),
/// `backdrop` = the opaque colour around it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ThumbFrame {
    pub u: f64,
    pub checker_brightness: f32,
    pub dark: bool,
    pub mode: RenderMode,
    pub checker: bool,
    pub backdrop: Px,
}

/// §2.8 Display steps 3–6: an opaque straight RGBA8 image of (tw + 2e) × (th + 2e) with the
/// thumbnail at (e, e): `backdrop`, the P3 ring outside the thumbnail (Aero Dark: white at
/// α (192/255)·(a/255); light: black at α a/255), then inside it the Small checkerboard (P2,
/// anchored at the thumbnail's top-left, (0, 0) dark) when `f.checker`, then the thumbnail
/// source-over; every blend in `f.mode`'s space (P1: linear light in Hardware, 8-bit sRGB in
/// Software; spec §0.10 Ruling (P1) 15). Returns (pixels, width, height).
pub fn render_framed(thumb: &[u8], tw: u32, th: u32, f: &ThumbFrame) -> (Vec<u8>, u32, u32) {
    let e = ring_extent(f.u);
    let (w, h) = (tw + 2 * e, th + 2 * e);
    let mut t = DisplayTarget::new(w, h, f.mode);
    t.fill(IntRect::new(0, 0, w, h), f.backdrop);
    let ring: Px = if f.dark {
        [255, 255, 255, 192]
    } else {
        [0, 0, 0, 255]
    };
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            let a = ring_alpha(tw, th, e, x - e as i32, y - e as i32);
            if a > 0 {
                t.over_srgb(x, y, ring, a as f32 / 255.0);
            }
        }
    }
    if f.checker {
        let side = checker_square(f.u, false);
        let (light, dark) = checker_colors(f.checker_brightness);
        for sy in (0..th).step_by(side as usize) {
            for sx in (0..tw).step_by(side as usize) {
                let v = if checker_is_light(sx as i32, sy as i32, side) {
                    light
                } else {
                    dark
                };
                let r = IntRect::new((e + sx) as i32, (e + sy) as i32, side, side);
                let inside = IntRect::new(e as i32, e as i32, tw, th);
                t.fill(r.intersect(&inside), [v, v, v, 255]);
            }
        }
    }
    for (i, p) in thumb
        .as_chunks::<4>()
        .0
        .iter()
        .take((tw * th) as usize)
        .enumerate()
    {
        let (x, y) = ((i as u32 % tw + e) as i32, (i as u32 / tw + e) as i32);
        t.over_srgb(x, y, [p[0], p[1], p[2], p[3]], 1.0);
    }
    (t.to_rgba8(), w, h)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::display::RenderMode;

    /// ui-drawn-graphics.md §3 "Thumb, M = 40" / §5 Task 18 (M = 60); the Task 19b document strip
    /// (ui-text-and-glyphs.md §8.5: item − 2·rnd(4s) − 2) is 44 at 100 % and 55 px at 125 %.
    #[test]
    fn sizes_fit_by_integer_division_and_never_upscale() {
        assert_eq!(thumbnail_size(800, 600, 60), (60, 45));
        assert_eq!(thumbnail_size(800, 600, 44), (44, 33));
        assert_eq!(thumbnail_size(800, 600, 55), (55, 41));
        assert_eq!(thumbnail_size(800, 600, 40), (40, 30));
        assert_eq!(thumbnail_size(16, 16, 60), (16, 16));
        assert_eq!(thumbnail_size(1000, 300, 40), (40, 12));
        assert_eq!(thumbnail_size(300, 1000, 40), (12, 40));
        assert_eq!(thumbnail_size(41, 1, 40), (40, 1));
        assert_eq!(thumbnail_size(1920, 1080, 40), (40, 22));
        assert_eq!(thumbnail_size(30, 20, 40), (30, 20));
        assert_eq!(thumbnail_size(800, 600, 50), (50, 37));
    }

    /// §2.8 Scaling: Catmull-Rom in linear premultiplied f32 (ui-drawn-graphics.md §5 Task 19).
    #[test]
    fn scaler_is_linear_light_catmull_rom() {
        let uniform: Vec<u8> = std::iter::repeat_n([40u8, 120, 200, 255], 16)
            .flatten()
            .collect();
        let (out, w, h) = scale_thumbnail(&uniform, 4, 4, 2);
        assert_eq!((w, h), (2, 2));
        assert!(out.chunks(4).all(|p| p == [40, 120, 200, 255]), "{out:?}");
        let pair = [0u8, 0, 0, 255, 255, 255, 255, 255];
        let (out, _, _) = scale_thumbnail(&pair, 2, 1, 1);
        assert!(out[0].abs_diff(188) <= 1, "{out:?}");
        let small: Vec<u8> = (0..16 * 16 * 4).map(|i| (i * 7) as u8).collect();
        assert_eq!(scale_thumbnail(&small, 16, 16, 60), (small.clone(), 16, 16));
    }

    /// P3 step 2–4 for a 10×6 thumbnail at e = 1: corners 15, next 47, runs 63.
    #[test]
    fn ring_alphas_outside_the_thumbnail() {
        assert_eq!(
            [0.5, 1.0, 1.25, 2.0, 3.5, 9.0].map(ring_extent),
            [1, 1, 2, 2, 4, 4]
        );
        for (x, y) in [(-1, -1), (10, -1), (-1, 6), (10, 6)] {
            assert_eq!(ring_alpha(10, 6, 1, x, y), 15, "corner ({x},{y})");
        }
        for (x, y) in [
            (0, -1),
            (-1, 0),
            (9, -1),
            (10, 0),
            (-1, 5),
            (0, 6),
            (10, 5),
            (9, 6),
        ] {
            assert_eq!(ring_alpha(10, 6, 1, x, y), 47, "next ({x},{y})");
        }
        for (x, y) in [(5, -1), (1, -1), (8, 6), (-1, 3), (10, 2)] {
            assert_eq!(ring_alpha(10, 6, 1, x, y), 63, "run ({x},{y})");
        }
        assert_eq!(
            (ring_alpha(10, 6, 1, 0, 0), ring_alpha(10, 6, 1, -2, 3)),
            (0, 0)
        );
    }

    /// P3 at e = 2 (u ∈ (1, 2]): ring 0 is the outer table, ring 1 the inner one.
    #[test]
    fn rings_at_extent_two() {
        let a = |x, y| ring_alpha(10, 6, 2, x, y);
        assert_eq!(
            [a(-2, -2), a(-1, -1), a(-2, -1), a(0, -1), a(5, -2)],
            [3, 28, 9, 56, 28]
        );
        assert_eq!([a(1, -1), a(11, 7), a(0, 0), a(-3, 0)], [75, 3, 0, 0]);
    }

    /// Task 18 fix round: however the work is sliced, the job gives `resize`'s pixels exactly.
    #[test]
    fn sliced_scaling_is_bit_identical() {
        let (w, h) = (301u32, 157u32);
        let src: Vec<u8> = (0..w * h * 4)
            .map(|i| (i.wrapping_mul(2654435761) >> 13) as u8)
            .collect();
        let whole = crate::resample::resize(
            &src,
            w,
            h,
            60,
            31,
            crate::resample::ResizeMode::Bicubic,
            Gamma::Linear,
        );
        assert_eq!(scale_thumbnail(&src, w, h, 60), (whole.clone(), 60, 31));
        for rows in [1, 7, 157, 190] {
            let mut job = ThumbnailScaler::new(&src, w, h, 60);
            let mut steps = 1;
            while !job.step(&src, rows) {
                steps += 1;
            }
            assert!(steps >= (157 + 31) / rows, "{rows}: {steps} steps");
            assert_eq!(
                job.finish(&src),
                (whole.clone(), 60, 31),
                "{rows} rows a step"
            );
        }
        // Nothing to resample: the source as it is, or nothing.
        let mut fits = ThumbnailScaler::new(&src, w, h, 400);
        assert!(fits.step(&src, 1));
        assert_eq!(fits.finish(&src), (src.clone(), w, h));
    }

    /// `resize` refuses a source shorter than w × h × 4: no thumbnail rather than a panic.
    #[test]
    fn unscalable_source_gives_no_thumbnail() {
        assert_eq!(scale_thumbnail(&[0; 4], 100, 100, 10), (vec![], 0, 0));
        assert_eq!(scale_thumbnail(&[], 0, 5, 60), (vec![], 0, 0));
    }

    fn frame(dark: bool, mode: RenderMode, backdrop: [u8; 4]) -> ThumbFrame {
        ThumbFrame {
            u: 1.0,
            checker_brightness: 0.75,
            dark,
            mode,
            checker: true,
            backdrop,
        }
    }

    /// §3 "Shadow ring": dark over #282828 → 72 / 110 / 124; light black over white → 248 / 233 / 225.
    #[test]
    fn ring_blends_in_linear_light() {
        let thumb = vec![255u8; 10 * 6 * 4];
        let (img, w, _) = render_framed(
            &thumb,
            10,
            6,
            &frame(true, RenderMode::Hardware, [0x28, 0x28, 0x28, 255]),
        );
        let g = |x: u32, y: u32| img[((y * w + x) * 4) as usize];
        assert_eq!([g(0, 0), g(1, 0), g(5, 0)], [72, 110, 124]);
        let (img, w, _) = render_framed(
            &thumb,
            10,
            6,
            &frame(false, RenderMode::Hardware, [255, 255, 255, 255]),
        );
        let g = |x: u32, y: u32| img[((y * w + x) * 4) as usize];
        assert_eq!([g(0, 0), g(1, 0), g(5, 0)], [248, 233, 225]);
    }

    /// P1/P2: Small checker (4 px, dark at the thumbnail's top-left) under the thumbnail;
    /// (255,0,0,128) over dark 143 → HW (209,103,103), SW (199,71,71).
    #[test]
    fn checker_is_anchored_at_the_thumbnail_and_mode_switches_space() {
        let clear = vec![0u8; 8 * 8 * 4];
        let (img, w, _) = render_framed(
            &clear,
            8,
            8,
            &frame(true, RenderMode::Hardware, [0x28, 0x28, 0x28, 255]),
        );
        let g = |x: u32, y: u32| img[((y * w + x) * 4) as usize];
        // Thumbnail pixel (0,0) is image pixel (1,1); (4,0) is the next (light) square.
        assert_eq!((g(1, 1), g(4, 1), g(5, 1), g(5, 5)), (143, 143, 191, 143));
        let red: Vec<u8> = std::iter::repeat_n([255u8, 0, 0, 128], 4)
            .flatten()
            .collect();
        let (img, w, _) = render_framed(
            &red,
            2,
            2,
            &frame(true, RenderMode::Hardware, [0, 0, 0, 255]),
        );
        let at = ((w + 1) * 4) as usize;
        assert_eq!(&img[at..at + 3], [209, 103, 103]);
        let (img, w, _) = render_framed(
            &red,
            2,
            2,
            &frame(true, RenderMode::Software, [0, 0, 0, 255]),
        );
        let at = ((w + 1) * 4) as usize;
        assert_eq!(&img[at..at + 3], [199, 71, 71]);
    }
}
