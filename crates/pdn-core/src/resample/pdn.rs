//! Paint.NET's own scalers: nearest neighbour, low-quality bilinear, Fant and the box average.
//! The box average also builds the brush-sprite mips.

use super::{PremulImage, size};

/// (d + 0.5)·s / n in f64, multiplying before dividing.
fn centre(d: u32, s: u32, n: u32) -> f64 {
    (d as f64 + 0.5) * s as f64 / n as f64
}

/// §2.2 step 6: sx = ⌊(x + 0.5)·W_s / W_d⌋ (same for y); raw byte copy, no colour conversion.
/// An empty source gives transparent black.
pub fn nearest_neighbor(src: &[u8], w: u32, h: u32, dw: u32, dh: u32) -> Vec<u8> {
    if w == 0 || h == 0 {
        return vec![0; size(dw, dh, 4)];
    }
    let xs: Vec<usize> = (0..dw).map(|x| centre(x, w, dw) as usize).collect();
    let mut out = Vec::with_capacity(size(dw, dh, 4));
    for y in 0..dh {
        let row = centre(y, h, dh) as usize * w as usize;
        for &sx in &xs {
            let i = (row + sx) * 4;
            out.extend_from_slice(&src[i..i + 4]);
        }
    }
    out
}

/// One axis of §2.2 step 7: (x0, x1, fx) with fx taken from the unclamped u.
fn llq_tap(d: u32, s: u32, n: u32) -> (usize, usize, f32) {
    let u = centre(d, s, n) - 0.5;
    let last = (s - 1) as f64;
    let x0 = u.clamp(0.0, last).floor();
    let x1 = (x0 + 1.0).min(last);
    (x0 as usize, x1 as usize, (u - x0) as f32)
}

/// `src_row(y, line)` fills row y of a premultiplied image; `emit(y, line)` takes output row y, in
/// order (`magic::scale_rows`'s convention).
type SrcRow<'a> = &'a mut dyn FnMut(usize, &mut [[f32; 4]]);
type Emit<'a> = &'a mut dyn FnMut(usize, &[[f32; 4]]);

/// §2.2 step 7: bilinear between texel centres, f32, taps summed left to right and divided by
/// the weight sum. Any zero dimension gives an empty or transparent result.
pub fn linear_low_quality(img: &PremulImage, dw: u32, dh: u32) -> PremulImage {
    whole(img, dw, dh, linear_low_quality_rows)
}

/// `linear_low_quality` streamed: output rows in order, holding two source rows. Sizes must be
/// non-zero.
pub(crate) fn linear_low_quality_rows(
    (w, h): (u32, u32),
    (dw, dh): (u32, u32),
    src_row: SrcRow,
    emit: Emit,
) {
    // quirk(pdn): document-ops-and-formats.md §2.2 step 7 — fx comes from the unclamped u, so
    // upsizing extrapolates at the top/left edges.
    let xs: Vec<_> = (0..dw).map(|x| llq_tap(x, w, dw)).collect();
    let blank = vec![[0f32; 4]; w as usize];
    // (source row index, pixels) for y0 and y1; y0 never decreases, so each row loads about once.
    let (mut r0, mut r1) = ((usize::MAX, blank.clone()), (usize::MAX, blank));
    let mut out = Vec::with_capacity(dw as usize);
    for y in 0..dh {
        let (y0, y1, fy) = llq_tap(y, h, dh);
        if r0.0 != y0 {
            if r1.0 == y0 {
                std::mem::swap(&mut r0, &mut r1);
            } else {
                src_row(y0, &mut r0.1);
                r0.0 = y0;
            }
        }
        if r1.0 != y1 {
            src_row(y1, &mut r1.1);
            r1.0 = y1;
        }
        out.clear();
        for &(x0, x1, fx) in &xs {
            let (wx0, wy0) = (1.0 - fx, 1.0 - fy);
            let wt = [wx0 * wy0, fx * wy0, wx0 * fy, fx * fy];
            let t = [r0.1[x0], r0.1[x1], r1.1[x0], r1.1[x1]];
            let div = wt[0] + wt[1] + wt[2] + wt[3];
            out.push(std::array::from_fn(|c| {
                (t[0][c] * wt[0] + t[1][c] * wt[1] + t[2][c] * wt[2] + t[3][c] * wt[3]) / div
            }));
        }
        emit(y as usize, &out);
    }
}

/// §2.2 step 8 box: destination pixel d covers [d·s/n, (d + 1)·s/n) in f64.
fn span(d: u32, s: u32, n: u32) -> (f64, f64) {
    (
        d as f64 * s as f64 / n as f64,
        (d + 1) as f64 * s as f64 / n as f64,
    )
}

/// The §2.2 step 8 box average, one output row at a time: per destination pixel, source rows
/// top→bottom and columns left→right, w = f32(overlap_x·overlap_y), f32 `sum += p·w`,
/// `div += w`, output sum/div per lane. Each pixel accumulates in exactly that order whatever row
/// the source arrives in, so streaming changes no bit.
struct BoxRows {
    size: (u32, u32),
    out: (u32, u32),
    xs: Vec<(f64, f64)>,
    line: Vec<[f32; 4]>,
    sum: Vec<[f32; 4]>,
    div: Vec<f32>,
}

impl BoxRows {
    fn new((w, h): (u32, u32), (dw, dh): (u32, u32)) -> Self {
        BoxRows {
            size: (w, h),
            out: (dw, dh),
            xs: (0..dw).map(|x| span(x, w, dw)).collect(),
            line: vec![[0.0; 4]; w as usize],
            sum: vec![[0.0; 4]; dw as usize],
            div: vec![0.0; dw as usize],
        }
    }

    /// Output row `y` into `out` (dw pixels).
    fn row(&mut self, y: usize, src_row: SrcRow, out: &mut [[f32; 4]]) {
        let (y0, y1) = span(y as u32, self.size.1, self.out.1);
        self.sum.fill([0.0; 4]);
        self.div.fill(0.0);
        for sy in y0.floor() as usize..y1.ceil() as usize {
            let oy = y1.min(sy as f64 + 1.0) - y0.max(sy as f64);
            src_row(sy, &mut self.line);
            for ((&(x0, x1), sum), div) in self.xs.iter().zip(&mut self.sum).zip(&mut self.div) {
                for sx in x0.floor() as usize..x1.ceil() as usize {
                    let ox = x1.min(sx as f64 + 1.0) - x0.max(sx as f64);
                    let wt = (ox * oy) as f32;
                    for (s, v) in sum.iter_mut().zip(self.line[sx]) {
                        *s += v * wt;
                    }
                    *div += wt;
                }
            }
        }
        for ((o, s), d) in out.iter_mut().zip(&self.sum).zip(&self.div) {
            *o = s.map(|s| s / d);
        }
    }
}

/// §2.2 step 8 box average. Any zero dimension gives an empty or transparent result.
pub fn box_average(img: &PremulImage, dw: u32, dh: u32) -> PremulImage {
    whole(img, dw, dh, box_average_rows)
}

/// `box_average` streamed. Sizes must be non-zero.
pub(crate) fn box_average_rows(size: (u32, u32), out: (u32, u32), src_row: SrcRow, emit: Emit) {
    let mut b = BoxRows::new(size, out);
    let mut line = vec![[0.0; 4]; out.0 as usize];
    for y in 0..out.1 as usize {
        b.row(y, src_row, &mut line);
        emit(y, &line);
    }
}

/// §2.2 step 8 Fant: same size → identity; both axes ≤ the source → box average; otherwise the
/// shrinking axis is box-averaged first (width before height), then Bilinear (Low Quality) does
/// the remaining enlargement.
pub fn fant(img: &PremulImage, dw: u32, dh: u32) -> PremulImage {
    if (img.w, img.h) == (dw, dh) {
        return img.clone();
    }
    whole(img, dw, dh, fant_rows)
}

/// `fant` streamed (a different size): the box-averaged intermediate rows are made as Bilinear
/// (Low Quality) asks for them. Sizes must be non-zero.
pub(crate) fn fant_rows((w, h): (u32, u32), (dw, dh): (u32, u32), src_row: SrcRow, emit: Emit) {
    if dw <= w && dh <= h {
        return box_average_rows((w, h), (dw, dh), src_row, emit);
    }
    let mid = if dw < w {
        (dw, h)
    } else if dh < h {
        (w, dh)
    } else {
        return linear_low_quality_rows((w, h), (dw, dh), src_row, emit);
    };
    let mut b = BoxRows::new((w, h), mid);
    linear_low_quality_rows(mid, (dw, dh), &mut |y, line| b.row(y, src_row, line), emit);
}

/// A streamed scaler over a whole `PremulImage`; any zero dimension gives an empty or
/// transparent result.
fn whole(
    img: &PremulImage,
    dw: u32,
    dh: u32,
    rows: fn((u32, u32), (u32, u32), SrcRow, Emit),
) -> PremulImage {
    if img.w == 0 || img.h == 0 || dw == 0 || dh == 0 {
        return PremulImage::zeroed(dw, dh);
    }
    let w = img.w as usize;
    let mut px = Vec::with_capacity(size(dw, dh, 1));
    rows(
        (img.w, img.h),
        (dw, dh),
        &mut |y, line| line.copy_from_slice(&img.px[y * w..][..w]),
        &mut |_, line| px.extend_from_slice(line),
    );
    PremulImage { w: dw, h: dh, px }
}
