//! The canvas view's render target, in Paint.NET's display blending space (R22).
//! Paint.NET's hardware and software paths, constant colours, the final scRGB → display step and
//! the display premultiply.

use std::sync::LazyLock;

use crate::pixel::gamma::{SRGB8_TO_LINEAR, linear_to_srgb8, quantize};
use crate::pixel::{IntRect, Px};

/// Paint.NET's canvas paths (`canvas-rendering.md §2.4`, R22): Hardware blends the render layers
/// in linear light (premultiplied f32; f16 scRGB in Paint.NET), Software in 8-bit sRGB.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMode {
    Hardware,
    Software,
}

impl RenderMode {
    /// `graphics.hardwareAcceleration`.
    pub fn from_setting(hardware_acceleration: bool) -> Self {
        if hardware_acceleration {
            RenderMode::Hardware
        } else {
            RenderMode::Software
        }
    }
}

/// The canvas view in the mode's blending space: premultiplied f32 RGBA per device pixel; in
/// Software every write is quantized to k/255, as a B8G8R8A8 target stores it. Every write is
/// clipped to `clip()` (the whole target unless `set_clip` narrows it to a dirty rect).
pub struct DisplayTarget {
    pub width: u32,
    pub height: u32,
    pub mode: RenderMode,
    clip: IntRect,
    px: Vec<[f32; 4]>,
}

/// `DisplayTarget::to_space` for a `mode` target.
pub(crate) fn to_space(mode: RenderMode, srgb: Px) -> [f32; 4] {
    let a = srgb[3] as f32 / 255.0;
    let c = |v: u8| match mode {
        RenderMode::Hardware => SRGB8_TO_LINEAR[v as usize] * a,
        RenderMode::Software => v as f32 / 255.0 * a,
    };
    [c(srgb[0]), c(srgb[1]), c(srgb[2]), a]
}

/// What a `mode` target stores for `v`: Software quantizes to k/255.
pub(crate) fn store(mode: RenderMode, v: [f32; 4]) -> [f32; 4] {
    match mode {
        RenderMode::Hardware => v,
        RenderMode::Software => v.map(|c| quantize(c) as f32 / 255.0),
    }
}

/// Source-over of premultiplied `src` onto `dst`. The shortcuts give the same result: opaque
/// replaces, transparent black keeps.
pub(crate) fn blend_over(mode: RenderMode, dst: &mut [f32; 4], src: [f32; 4]) {
    if src[3] >= 1.0 {
        *dst = store(mode, src);
    } else if src != [0.0; 4] {
        let k = 1.0 - src[3];
        *dst = store(mode, std::array::from_fn(|c| src[c] + dst[c] * k));
    }
}

/// Runs `f(first_row, band)` over rows `rows` of the `width`-wide `buf`, in bands of `band` rows:
/// in parallel on native targets, in order on wasm. Each pixel's result never depends on the
/// banding, so both give the same output.
pub(crate) fn for_bands<T: Send>(
    buf: &mut [T],
    width: usize,
    rows: std::ops::Range<usize>,
    band: usize,
    f: impl Fn(usize, &mut [T]) + Sync + Send,
) {
    if rows.is_empty() || width == 0 {
        return;
    }
    let first = rows.start;
    let slice = &mut buf[first * width..rows.end * width];
    let run = |(i, chunk): (usize, &mut [T])| f(first + i * band, chunk);
    #[cfg(not(target_arch = "wasm32"))]
    {
        use rayon::prelude::*;
        slice.par_chunks_mut(band * width).enumerate().for_each(run);
    }
    #[cfg(target_arch = "wasm32")]
    slice.chunks_mut(band * width).enumerate().for_each(run);
}

/// The one-row rects of `spans` (sorted by row) in rows `y0..y1`.
fn spans_in(spans: &[IntRect], y0: i32, y1: i32) -> &[IntRect] {
    let a = spans.partition_point(|s| s.y < y0);
    &spans[a..a + spans[a..].partition_point(|s| s.y < y1)]
}

/// The parts of `area` in rows `y0..y1`: that band of it, or each of `spans` there, ∩ `area`.
pub(crate) fn parts(
    area: IntRect,
    spans: Option<&[IntRect]>,
    y0: i32,
    y1: i32,
) -> impl Iterator<Item = IntRect> + '_ {
    let (band, spans) = match spans {
        None => (
            Some(IntRect::new(area.x, y0, area.w, (y1 - y0) as u32)),
            &[][..],
        ),
        Some(s) => (None, spans_in(s, y0, y1)),
    };
    band.into_iter()
        .chain(spans.iter().copied())
        .map(move |r| r.intersect(&area))
        .filter(|r| !r.is_empty())
}

/// Runs closer than this many px are refreshed as one: a run costs about as much as that many
/// pixels.
pub const SPAN_GAP: u32 = 16;

/// One bit per pixel of `area`: rects are marked in it, then read back as one-row spans.
pub struct SpanMask {
    area: IntRect,
    words: usize,
    bits: Vec<u64>,
}

impl SpanMask {
    pub fn new(area: IntRect) -> Self {
        let words = (area.w as usize).div_ceil(64);
        SpanMask {
            area,
            words,
            bits: vec![0; words * area.h as usize],
        }
    }

    /// Row `y`'s bits, bit `i % 64` of word `i / 64` for column `area.x + i`.
    pub(crate) fn row_mut(&mut self, y: i32) -> &mut [u64] {
        let y = (y - self.area.y) as usize;
        &mut self.bits[y * self.words..][..self.words]
    }

    /// Marks `r` ∩ the area.
    pub fn mark(&mut self, r: IntRect) {
        let r = r.intersect(&self.area);
        if r.is_empty() {
            return;
        }
        let (x0, x1) = (
            (r.x - self.area.x) as usize,
            (r.right() - self.area.x) as usize,
        );
        let (w0, w1) = (x0 / 64, (x1 - 1) / 64);
        for y in (r.y - self.area.y) as usize..(r.bottom() - self.area.y) as usize {
            let row = &mut self.bits[y * self.words..][..self.words];
            for (w, word) in row.iter_mut().enumerate().take(w1 + 1).skip(w0) {
                let lo = if w == w0 { x0 % 64 } else { 0 };
                let hi = if w == w1 { (x1 - 1) % 64 + 1 } else { 64 };
                *word |= (u64::MAX >> (64 - (hi - lo))) << lo;
            }
        }
    }

    /// The marked pixels as one-row spans sorted by row, each row's runs merged across gaps of
    /// under `gap` px, and what refreshing them costs: their pixels plus `gap` per span.
    pub fn spans(&self, gap: u32) -> (Vec<IntRect>, u64) {
        let (n, mut out, mut cost) = (self.area.w as usize, vec![], 0u64);
        // The first bit at or after `x` that is `set`, or `n`.
        let next = |row: &[u64], x: usize, set: bool| {
            let flip = if set { 0 } else { u64::MAX };
            let mut w = x / 64;
            let mut bits = (row.get(w).copied().unwrap_or(flip) ^ flip) & (u64::MAX << (x % 64));
            while bits == 0 {
                w += 1;
                if w >= row.len() {
                    return n;
                }
                bits = row[w] ^ flip;
            }
            (w * 64 + bits.trailing_zeros() as usize).min(n)
        };
        for (j, row) in self.bits.chunks_exact(self.words).enumerate() {
            let y = self.area.y + j as i32;
            let mut push = |(a, b): (usize, usize)| {
                let x = self.area.x + a as i32;
                out.push(IntRect::new(x, y, (b - a) as u32, 1));
                cost += (b - a) as u64 + gap as u64;
            };
            let (mut run, mut x) = (None, 0);
            loop {
                let a = next(row, x, true);
                if a >= n {
                    break;
                }
                let b = next(row, a, false);
                run = match run {
                    Some((s, e)) if a - e < gap as usize => Some((s, b)),
                    Some(r) => {
                        push(r);
                        Some((a, b))
                    }
                    None => Some((a, b)),
                };
                x = b;
            }
            if let Some(r) = run {
                push(r);
            }
        }
        (out, cost)
    }
}

/// Upload rects covering `spans` (one-row rects sorted by row): per `band`-row band, the spans'
/// columns merged across gaps of under `band` px, each over the band's rows that have spans.
pub fn span_bands(spans: &[IntRect], band: u32) -> Vec<IntRect> {
    let band = band.max(1) as i32;
    let mut out = vec![];
    for chunk in spans.chunk_by(|a, b| a.y.div_euclid(band) == b.y.div_euclid(band)) {
        let (y0, y1) = (chunk[0].y, chunk[chunk.len() - 1].y + 1);
        let rect = |(a, b): (i32, i32)| IntRect::new(a, y0, (b - a) as u32, (y1 - y0) as u32);
        let mut cols: Vec<(i32, i32)> = chunk.iter().map(|s| (s.x, s.right())).collect();
        cols.sort_unstable();
        let mut run = cols[0];
        for &(a, b) in &cols[1..] {
            if a < run.1 + band {
                run.1 = run.1.max(b);
            } else {
                out.push(rect(run));
                run = (a, b);
            }
        }
        out.push(rect(run));
    }
    out
}

impl DisplayTarget {
    pub fn new(width: u32, height: u32, mode: RenderMode) -> Self {
        Self {
            width,
            height,
            mode,
            clip: IntRect::new(0, 0, width, height),
            px: vec![[0.0; 4]; width as usize * height as usize],
        }
    }

    /// Limits every following write to `clip` ∩ the target (`None` = the whole target).
    pub fn set_clip(&mut self, clip: Option<IntRect>) {
        let all = IntRect::new(0, 0, self.width, self.height);
        self.clip = clip.map_or(all, |c| c.intersect(&all));
    }

    pub fn clip(&self) -> IntRect {
        self.clip
    }

    /// A straight sRGB colour as a premultiplied colour in this target's space (linear via
    /// `SRGB8_TO_LINEAR` in Hardware; constant colours are linearized, `canvas-rendering.md §2.4`).
    pub fn to_space(&self, srgb: Px) -> [f32; 4] {
        to_space(self.mode, srgb)
    }

    fn index(&self, x: i32, y: i32) -> Option<usize> {
        self.clip
            .contains(x, y)
            .then(|| y as usize * self.width as usize + x as usize)
    }

    /// Copy-compositing fill (surround, checkerboard), clipped.
    pub fn fill(&mut self, rect: IntRect, srgb: Px) {
        let r = rect.intersect(&self.clip);
        if r.is_empty() {
            return;
        }
        let v = store(self.mode, self.to_space(srgb));
        let (w, cols) = (self.width as usize, r.x as usize..r.right() as usize);
        self.bands(r.y..r.bottom(), |_, band| {
            for row in band.chunks_mut(w) {
                row[cols.clone()].fill(v);
            }
        });
    }

    /// Copy one premultiplied colour already in target space (shadow band pixels).
    pub fn set(&mut self, x: i32, y: i32, premul: [f32; 4]) {
        if let Some(i) = self.index(x, y) {
            self.px[i] = store(self.mode, premul);
        }
    }

    /// Row `y`'s stored pixels (`y` inside the target).
    pub(crate) fn row(&self, y: i32) -> &[[f32; 4]] {
        let w = self.width as usize;
        &self.px[y as usize * w..][..w]
    }

    /// Transparent black outside the target.
    pub fn get(&self, x: i32, y: i32) -> [f32; 4] {
        let inside = x >= 0 && y >= 0 && (x as u32) < self.width && (y as u32) < self.height;
        if inside {
            self.px[y as usize * self.width as usize + x as usize]
        } else {
            [0.0; 4]
        }
    }

    /// Source-over of a premultiplied colour in target space.
    pub fn over(&mut self, x: i32, y: i32, premul: [f32; 4]) {
        if let Some(i) = self.index(x, y) {
            blend_over(self.mode, &mut self.px[i], premul);
        }
    }

    /// Source-over of a straight sRGB colour at `coverage` (0..=1).
    pub fn over_srgb(&mut self, x: i32, y: i32, srgb: Px, coverage: f32) {
        let s = self.to_space(srgb).map(|c| c * coverage);
        self.over(x, y, s);
    }

    /// Runs `paint(y, x0, row)` over the rows of the clip: `row` is row y's stored pixels from
    /// column x0. In parallel row bands on native targets.
    pub fn paint_rows(&mut self, paint: impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send) {
        let (c, w) = (self.clip, self.width as usize);
        let cols = c.x as usize..c.right() as usize;
        self.bands(c.y..c.bottom(), |y0, band| {
            for (j, row) in band.chunks_mut(w).enumerate() {
                paint(y0 + j as i32, c.x, &mut row[cols.clone()]);
            }
        });
    }

    /// `for_bands` over this target's rows `rows` (full-width row bands of 32 rows); callers
    /// keep their writes inside `clip()`.
    pub(crate) fn bands(
        &mut self,
        rows: std::ops::Range<i32>,
        f: impl Fn(i32, &mut [[f32; 4]]) + Sync + Send,
    ) {
        let rows = rows.start.max(0) as usize..rows.end.max(0) as usize;
        let w = self.width as usize;
        for_bands(&mut self.px, w, rows, 32, |y, band| f(y as i32, band));
    }

    /// The upload bytes of one pixel: Hardware `linear_to_srgb8` per channel (the DWM conversion,
    /// round to nearest), Software the stored bytes; alpha is the stored coverage.
    // calibrate: B-C8 — the DWM's f16 scRGB → 8-bit rounding (±1 proposed).
    fn rgba8(&self, t: &ToSrgb8, p: [f32; 4]) -> [u8; 4] {
        match self.mode {
            RenderMode::Hardware => [t.get(p[0]), t.get(p[1]), t.get(p[2]), quantize(p[3])],
            RenderMode::Software => p.map(quantize),
        }
    }

    /// Writes `rect` (∩ the target) of the upload pixels into the target-sized `out`
    /// (row-major, `width` wide), each through `conv` (e.g. into the caller's texture type), in
    /// parallel rows on native targets.
    pub fn write_rgba8<T: Send>(
        &self,
        rect: IntRect,
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
    ) {
        self.write_rgba8_over(rect, None, out, conv, |_, _, _| {});
    }

    /// `write_rgba8` of `rect` (∩ `spans`, one-row rects sorted by row, when given), each row's
    /// pixels first painted over by `paint(y, x0, row)` (`paint_rows`) in a copy: the target
    /// keeps its own (the canvas view's overlays over its cached base).
    pub fn write_rgba8_over<T: Send>(
        &self,
        rect: IntRect,
        spans: Option<&[IntRect]>,
        out: &mut [T],
        conv: impl Fn([u8; 4]) -> T + Sync + Send,
        paint: impl Fn(i32, i32, &mut [[f32; 4]]) + Sync + Send,
    ) {
        let area = rect.intersect(&IntRect::new(0, 0, self.width, self.height));
        if area.is_empty() || spans.is_some_and(|s| s.is_empty()) {
            return;
        }
        let w = self.width as usize;
        let rows = area.y as usize..area.bottom() as usize;
        let t = &*TO_SRGB8;
        for_bands(out, w, rows, 32, |y0, band| {
            let mut row = vec![];
            let y1 = (y0 + band.len() / w) as i32;
            for r in parts(area, spans, y0 as i32, y1) {
                let (x0, x1) = (r.x as usize, r.right() as usize);
                for y in r.y as usize..r.bottom() as usize {
                    row.clear();
                    row.extend_from_slice(&self.px[y * w + x0..y * w + x1]);
                    paint(y as i32, r.x, &mut row);
                    let dst = &mut band[(y - y0) * w..][x0..x1];
                    for (o, &p) in dst.iter_mut().zip(&row) {
                        *o = conv(self.rgba8(t, p));
                    }
                }
            }
        });
    }

    /// The whole target as RGBA8 (`write_rgba8`).
    pub fn to_rgba8(&self) -> Vec<u8> {
        let mut out = vec![[0u8; 4]; self.px.len()];
        self.write_rgba8(IntRect::new(0, 0, self.width, self.height), &mut out, |p| p);
        out.into_flattened()
    }
}

/// `linear_to_srgb8` through its decision thresholds: `thr[k]` is the least f32 with output ≥ k,
/// `start[i]` the output at the f32 with bits i << START_SHIFT (positive f32s order like their
/// bits); the f64 curve per channel would cost three `powf` a pixel. A bucket of 2^15 f32s spans
/// at most one threshold, so `start` stays small (32 KiB) and one compare usually settles it.
struct ToSrgb8 {
    thr: [f32; 256],
    start: Vec<u8>,
}

const START_SHIFT: u32 = 15;

static TO_SRGB8: LazyLock<ToSrgb8> = LazyLock::new(|| {
    let mut thr = [0f32; 256];
    for (k, t) in thr.iter_mut().enumerate().skip(1) {
        // The output is monotonic in the value, and so in the bits of non-negative f32s.
        let (mut lo, mut hi) = (0u32, 1f32.to_bits());
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if (linear_to_srgb8(f32::from_bits(mid)) as usize) < k {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        *t = f32::from_bits(lo);
    }
    let start = (0..=1f32.to_bits() >> START_SHIFT)
        .map(|i| linear_to_srgb8(f32::from_bits(i << START_SHIFT)))
        .collect();
    ToSrgb8 { thr, start }
});

impl ToSrgb8 {
    /// Exactly `linear_to_srgb8(l)`.
    #[inline]
    fn get(&self, l: f32) -> u8 {
        let b = l.to_bits();
        // Not in (0, 1): NaN, negatives and 0 give 0; 1 and above 255.
        if b.wrapping_sub(1) >= 1f32.to_bits() - 1 {
            return if l >= 1.0 { 255 } else { 0 };
        }
        let mut k = self.start[(b >> START_SHIFT) as usize];
        while k < 255 && l >= self.thr[k as usize + 1] {
            k += 1;
        }
        k
    }
}

/// Exactly `linear_to_srgb8(l)`.
pub(crate) fn to_srgb8(l: f32) -> u8 {
    TO_SRGB8.get(l)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::IntRect;

    fn blend(mode: RenderMode, bg: u8, fg: [u8; 4]) -> u8 {
        let mut t = DisplayTarget::new(1, 1, mode);
        t.fill(IntRect::new(0, 0, 1, 1), [bg, bg, bg, 255]);
        t.over_srgb(0, 0, fg, 1.0);
        t.to_rgba8()[0]
    }

    /// canvas-rendering.md §5 "Blend": hardware mode blends in linear light, software in 8-bit sRGB.
    #[test]
    fn hardware_blends_in_linear_light() {
        assert_eq!(blend(RenderMode::Hardware, 191, [0, 0, 0, 128]), 139);
        assert_eq!(blend(RenderMode::Hardware, 143, [0, 0, 0, 128]), 103);
        assert_eq!(blend(RenderMode::Software, 191, [0, 0, 0, 128]), 95);
        assert_eq!(blend(RenderMode::Software, 143, [0, 0, 0, 128]), 71);
        assert_eq!(blend(RenderMode::Hardware, 143, [255, 255, 255, 128]), 209);
        assert_eq!(blend(RenderMode::Software, 143, [255, 255, 255, 128]), 199);
        assert_eq!(RenderMode::from_setting(true), RenderMode::Hardware);
        assert_eq!(RenderMode::from_setting(false), RenderMode::Software);
    }

    /// Marked rects come back as one-row spans, sorted by row, clipped to the area, across word
    /// boundaries, each row's runs merged across gaps under `gap`; the cost is their pixels plus
    /// `gap` per span.
    #[test]
    fn span_masks_read_back_merged_runs() {
        let r = |x, y, w, h| IntRect::new(x, y, w, h);
        let mut m = SpanMask::new(r(-10, 5, 200, 4));
        for rect in [
            r(-20, 5, 15, 1),
            r(0, 5, 64, 2),
            r(63, 6, 2, 1),
            r(80, 6, 1, 1),
        ] {
            m.mark(rect);
        }
        m.mark(r(120, 7, 100, 1));
        m.mark(r(0, 0, 50, 5));
        let (spans, cost) = m.spans(16);
        assert_eq!(spans, [r(-10, 5, 74, 1), r(0, 6, 81, 1), r(120, 7, 70, 1)]);
        assert_eq!(cost, 74 + 81 + 70 + 3 * 16);
        assert_eq!(m.spans(5).0[..2], [r(-10, 5, 5, 1), r(0, 5, 64, 1)]);
        // Gaps of 15 merge, 16 do not.
        let mut m = SpanMask::new(r(0, 0, 130, 1));
        for x in [
            r(0, 0, 1, 1),
            r(16, 0, 1, 1),
            r(33, 0, 1, 1),
            r(128, 0, 2, 1),
        ] {
            m.mark(x);
        }
        assert_eq!(
            m.spans(16).0,
            [r(0, 0, 17, 1), r(33, 0, 1, 1), r(128, 0, 2, 1)]
        );
        assert_eq!(SpanMask::new(r(0, 0, 64, 3)).spans(16), (vec![], 0));
    }

    /// A painted write paints a copy of each row's pixels (row y from column x0) inside the rect
    /// or its spans only, and leaves the target alone; `paint_rows` paints the target inside its
    /// clip; the upload bands cover every span: per 32-row band, its columns merged across gaps
    /// under 32.
    #[test]
    fn painted_writes_and_span_bands() {
        let mut t = DisplayTarget::new(100, 80, RenderMode::Software);
        t.fill(IntRect::new(0, 0, 100, 80), [10, 20, 30, 255]);
        t.fill(IntRect::new(0, 0, 50, 40), [200, 0, 0, 255]);
        let before = t.to_rgba8();
        let s = |x, y, w| IntRect::new(x, y, w, 1);
        let mut spans = vec![s(0, 0, 100)];
        for y in 1..70 {
            spans.extend([s(0, y, 2), s(98, y, 2)]);
        }
        spans.extend([s(10, 70, 5), s(40, 70, 5)]);
        let paint = |y: i32, x0: i32, row: &mut [[f32; 4]]| {
            for (i, p) in row.iter_mut().enumerate() {
                *p = [(x0 + i as i32) as f32 / 255.0, y as f32 / 255.0, 0.0, 1.0];
            }
        };
        let rect = IntRect::new(20, 10, 30, 50);
        for (area, spans) in [
            (IntRect::new(0, 0, 100, 80), Some(&spans[..])),
            (rect, None),
        ] {
            let mut out: Vec<[u8; 4]> = before.as_chunks().0.to_vec();
            t.write_rgba8_over(area, spans, &mut out, |p| p, paint);
            assert!(t.to_rgba8() == before);
            for (i, (a, b)) in out.iter().zip(before.as_chunks::<4>().0).enumerate() {
                let (x, y) = ((i % 100) as i32, (i / 100) as i32);
                let inside =
                    spans.map_or(rect.contains(x, y), |s| s.iter().any(|r| r.contains(x, y)));
                let want = if inside {
                    [x as u8, y as u8, 0, 255]
                } else {
                    *b
                };
                assert_eq!(*a, want, "({x},{y})");
            }
        }
        // Painting the target itself stays inside its clip.
        t.set_clip(Some(rect));
        t.paint_rows(paint);
        let after = t.to_rgba8();
        for (i, (a, b)) in after.chunks(4).zip(before.chunks(4)).enumerate() {
            let (x, y) = ((i % 100) as i32, (i / 100) as i32);
            let want = if rect.contains(x, y) {
                [x as u8, y as u8, 0, 255]
            } else {
                b.try_into().unwrap()
            };
            assert_eq!(a, want, "({x},{y})");
        }
        let r = IntRect::new;
        assert_eq!(
            span_bands(&spans, 32),
            [
                r(0, 0, 100, 32),
                r(0, 32, 2, 32),
                r(98, 32, 2, 32),
                r(0, 64, 45, 7),
                r(98, 64, 2, 7)
            ]
        );
    }

    /// The threshold table is `linear_to_srgb8` bit for bit.
    #[test]
    fn display_conversion_matches_the_curve() {
        let t = &*TO_SRGB8;
        let edges = t.thr[1..].iter().flat_map(|&v| {
            let b = v.to_bits();
            [b - 1, b, b + 1].map(f32::from_bits)
        });
        let dense = (0..=1_100_000).map(|i| i as f32 / 1_000_000.0);
        let odd = [f32::NAN, -1.0, -0.0, f32::INFINITY, 2.0, f32::MIN_POSITIVE];
        for l in edges.chain(dense).chain(odd) {
            assert_eq!(to_srgb8(l), linear_to_srgb8(l), "{l:e}");
        }
    }

    /// Every f32 bit pattern (Task 27p's move output encodes through the table; `quantize`
    /// truncates instead of flooring): `cargo test --release -p pdn-core --lib every_f32 --
    /// --ignored`.
    #[test]
    #[ignore = "2³² values: run in release"]
    fn every_f32_converts_like_the_curve() {
        use rayon::prelude::*;
        (0..=u32::MAX).into_par_iter().for_each(|b| {
            let l = f32::from_bits(b);
            assert_eq!(to_srgb8(l), linear_to_srgb8(l), "{b:#x}");
            let floored = if l.is_nan() {
                0
            } else {
                (l.clamp(0.0, 1.0) * 255.0 + 0.5).floor() as u8
            };
            assert_eq!(quantize(l), floored, "{b:#x}");
        });
    }
}
