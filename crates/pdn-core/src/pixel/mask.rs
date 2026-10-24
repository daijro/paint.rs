//! Selection clipping masks: Paint.NET's scanline rasterizer and the HQ 4×4 box filter.
//! Paint.NET's scan conversion, mask renderers and HQ coverage LUT. Ruling (P1): its scan
//! conversion rule is normative.

use super::IntRect;
use crate::render::display::for_bands;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionQuality {
    Aliased,
    HighQuality,
}

impl SelectionQuality {
    /// The `renderingQuality` setting value: `"aliased"` → Aliased, anything else → HighQuality.
    pub fn from_value(v: &str) -> Self {
        if v == "aliased" {
            SelectionQuality::Aliased
        } else {
            SelectionQuality::HighQuality
        }
    }
}

/// α(n) = ⌊(255n + 8)/16⌋ for n covered sub-samples of 16.
pub const HQ_LUT: [u8; 17] = [
    0, 16, 32, 48, 64, 80, 96, 112, 128, 143, 159, 175, 191, 207, 223, 239, 255,
];

/// A8 coverage over `rect`.
#[derive(Clone, Debug, PartialEq)]
pub struct Mask {
    pub rect: IntRect,
    pub data: Vec<u8>,
}

impl Mask {
    pub fn opaque(rect: IntRect) -> Mask {
        Mask {
            rect,
            data: vec![255; (rect.w * rect.h) as usize],
        }
    }

    /// 0 outside `rect`.
    pub fn get(&self, x: i32, y: i32) -> u8 {
        if !self.rect.contains(x, y) {
            return 0;
        }
        let (dx, dy) = ((x - self.rect.x) as usize, (y - self.rect.y) as usize);
        self.data[dy * self.rect.w as usize + dx]
    }

    /// The `r`-sized slice of this mask (0 outside its rect), for `render_content`'s clip.
    pub fn crop(&self, r: IntRect) -> Vec<u8> {
        let mut out = vec![0; (r.w * r.h) as usize];
        let i = r.intersect(&self.rect);
        for y in i.y..i.bottom() {
            let src =
                ((y - self.rect.y) as usize * self.rect.w as usize) + (i.x - self.rect.x) as usize;
            let dst = ((y - r.y) as usize * r.w as usize) + (i.x - r.x) as usize;
            out[dst..dst + i.w as usize].copy_from_slice(&self.data[src..src + i.w as usize]);
        }
        out
    }
}

/// The edges of a closed polygon: i → i+1 plus the implicit closing edge.
fn edges(poly: &[[f64; 2]]) -> impl Iterator<Item = ([f64; 2], [f64; 2])> + '_ {
    (0..poly.len()).map(move |i| (poly[i], poly[(i + 1) % poly.len()]))
}

/// Every polygon rectilinear with integer vertices (an empty list counts).
pub fn is_pixelated(polys: &[Vec<[f64; 2]>]) -> bool {
    polys.iter().all(|p| {
        p.iter().all(|v| v[0].fract() == 0.0 && v[1].fract() == 0.0)
            && edges(p).all(|(a, b)| a[0] == b[0] || a[1] == b[1])
    })
}

/// §2.3 scan conversion at `scale`: half-open spans (row, x0, x1) in scaled pixels, even-odd
/// across all polygons; edges active iff minY ≤ y + ½ < maxY; spans cover pixels with
/// xL ≤ x + ½ ≤ xR; x advances incrementally in f64.
pub fn scans(polys: &[Vec<[f64; 2]>], scale: f64) -> Vec<(i32, i32, i32)> {
    struct Edge {
        min_y: f64,
        max_y: f64,
        x: f64,
        dxdy: f64,
    }
    // 1. Edge table (transformed endpoints, horizontal edges skipped).
    let mut pending: Vec<Edge> = polys
        .iter()
        .flat_map(|p| edges(p))
        .filter_map(|(a, b)| {
            let (a, b) = ([a[0] * scale, a[1] * scale], [b[0] * scale, b[1] * scale]);
            let (top, bot) = if a[1] <= b[1] { (a, b) } else { (b, a) };
            let dy = bot[1] - top[1];
            (dy != 0.0).then(|| Edge {
                min_y: top[1],
                max_y: bot[1],
                x: top[0],
                dxdy: (bot[0] - top[0]) / dy,
            })
        })
        .collect();
    if pending.is_empty() {
        return vec![];
    }
    let y_min = pending
        .iter()
        .map(|e| (e.min_y - 0.5).ceil())
        .fold(f64::INFINITY, f64::min) as i32;
    let y_max = pending
        .iter()
        .map(|e| (e.max_y - 0.5).floor())
        .fold(f64::NEG_INFINITY, f64::max) as i32;
    // 2. Sorted by minY; consumed from the front.
    pending.sort_by(|a, b| a.min_y.total_cmp(&b.min_y));
    let mut pending = pending.into_iter().peekable();
    let mut active: Vec<Edge> = vec![];
    let mut out = vec![];
    for y in y_min..=y_max {
        // 3. Activation: an edge is active iff minY ≤ yc < maxY.
        let yc = y as f64 + 0.5;
        while let Some(mut e) = pending.next_if(|e| e.min_y <= yc) {
            if e.max_y > yc {
                e.x += (yc - e.min_y) * e.dxdy;
                active.push(e);
            }
        }
        active.retain(|e| e.max_y > yc);
        // 4. Even-odd pairs, both ends inclusive at pixel centres, no overlap on a row.
        active.sort_by(|a, b| a.x.total_cmp(&b.x));
        let mut prev_end = i32::MIN;
        for [l, r] in active.as_chunks::<2>().0 {
            let x0 = prev_end.max((l.x - 0.5).ceil() as i32);
            let x1 = ((r.x - 0.5).floor() as i32).saturating_add(1);
            if x1 > x0 {
                out.push((y, x0, x1));
                prev_end = x1;
            }
        }
        // 5. Incremental advance.
        for e in &mut active {
            e.x += e.dxdy;
        }
    }
    out
}

/// The clipping mask over `rect`: aliased scans at 1× for Aliased or pixelated geometry, else
/// scans at 4× box-filtered through `HQ_LUT`.
pub fn rasterize(polys: &[Vec<[f64; 2]>], quality: SelectionQuality, rect: IntRect) -> Mask {
    let mut data = vec![0u8; (rect.w * rect.h) as usize];
    let w = rect.w as usize;
    let aliased = quality == SelectionQuality::Aliased || is_pixelated(polys);
    // Sub-rows per pixel row; the scans come sorted by row.
    let sub = if aliased { 1 } else { 4 };
    let spans = scans(polys, sub as f64);
    // Task 27p: row bands in parallel, each with its rows' scans (in the same order): the same
    // bytes as one pass.
    for_bands(&mut data, w, 0..rect.h as usize, 8, |y0, band| {
        let top = rect.y + y0 as i32;
        let (lo, hi) = (top * sub, (top + (band.len() / w) as i32) * sub);
        let first = spans.partition_point(|s| s.0 < lo);
        let n = spans[first..].partition_point(|s| s.0 < hi);
        for &(sy, x0, x1) in &spans[first..first + n] {
            let row = &mut band[(sy.div_euclid(sub) - top) as usize * w..][..w];
            if aliased {
                let (a, b) = (x0.max(rect.x), x1.min(rect.right()));
                if a < b {
                    row[(a - rect.x) as usize..(b - rect.x) as usize].fill(255);
                }
                continue;
            }
            // Sub-sample counts per pixel (≤ 16): the span's first and last pixels get their
            // part, the pixels between all 4.
            let (x, end) = (x0.max(rect.x * 4), x1.min(rect.right() * 4));
            if x >= end {
                continue;
            }
            let (a, b) = (x.div_euclid(4), (end - 1).div_euclid(4));
            let cells = &mut row[(a - rect.x) as usize..=(b - rect.x) as usize];
            if a == b {
                cells[0] += (end - x) as u8;
            } else {
                cells[0] += ((a + 1) * 4 - x) as u8;
                let (last, middle) = cells[1..].split_last_mut().expect("two cells");
                middle.iter_mut().for_each(|c| *c += 4);
                *last += (end - b * 4) as u8;
            }
        }
        if !aliased {
            // The box ¼ filter.
            band.iter_mut().for_each(|v| *v = HQ_LUT[*v as usize]);
        }
    });
    Mask { rect, data }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::IntRect;

    fn row(m: &Mask, y: i32, x0: i32, x1: i32) -> Vec<u8> {
        (x0..x1).map(|x| m.get(x, y)).collect()
    }

    fn rect(l: f64, t: f64, r: f64, b: f64) -> Vec<[f64; 2]> {
        vec![[l, t], [r, t], [r, b], [l, b]]
    }

    #[test]
    fn hq_lut() {
        assert_eq!(
            HQ_LUT,
            [
                0, 16, 32, 48, 64, 80, 96, 112, 128, 143, 159, 175, 191, 207, 223, 239, 255
            ]
        );
    }

    #[test]
    fn rect_edges_hq_and_aliased() {
        let r = [rect(0.5, 0.0, 10.5, 4.0)];
        let hq = rasterize(&r, SelectionQuality::HighQuality, IntRect::new(0, 0, 12, 4));
        let mut want = vec![128];
        want.extend([255; 9]);
        want.extend([128, 0]);
        assert_eq!(row(&hq, 1, 0, 12), want);
        let aliased = rasterize(&r, SelectionQuality::Aliased, IntRect::new(0, 0, 12, 4));
        let mut want = vec![255; 11];
        want.push(0);
        assert_eq!(row(&aliased, 1, 0, 12), want);
        let c = [rect(0.0, 0.5, 4.0, 2.5)];
        let hq = rasterize(&c, SelectionQuality::HighQuality, IntRect::new(0, 0, 4, 4));
        assert_eq!(
            (0..4).map(|y| hq.get(1, y)).collect::<Vec<_>>(),
            [128, 255, 128, 0]
        );
        let aliased = rasterize(&c, SelectionQuality::Aliased, IntRect::new(0, 0, 4, 4));
        assert_eq!(
            (0..4).map(|y| aliased.get(1, y)).collect::<Vec<_>>(),
            [255, 255, 0, 0]
        );
    }

    #[test]
    fn quad_and_triangle_rows() {
        let q = [vec![[16.0, 10.0], [44.0, 16.0], [30.0, 44.0], [12.0, 36.0]]];
        let m = rasterize(
            &q,
            SelectionQuality::HighQuality,
            IntRect::new(0, 0, 50, 50),
        );
        let mut want = vec![159];
        want.extend([255; 26]);
        want.push(191);
        assert_eq!(row(&m, 20, 14, 42), want);
        assert_eq!((m.get(13, 20), m.get(42, 20)), (0, 0));
        assert_eq!(row(&m, 10, 15, 20), [16, 223, 175, 128, 64]);
        let a = rasterize(&q, SelectionQuality::Aliased, IntRect::new(0, 0, 50, 50));
        assert_eq!(
            row(&a, 20, 13, 43),
            [vec![0], vec![255; 28], vec![0]].concat()
        );
        let t = [vec![[3.0, 3.0], [28.0, 6.0], [16.0, 29.0]]];
        let m = rasterize(
            &t,
            SelectionQuality::HighQuality,
            IntRect::new(0, 0, 32, 32),
        );
        assert_eq!(
            row(&m, 10, 5, 26),
            [vec![0, 64], vec![255; 18], vec![159]].concat()
        );
    }

    #[test]
    fn pixelated_geometry_is_always_aliased() {
        let r = [rect(2.0, 2.0, 6.0, 5.0)];
        assert!(is_pixelated(&r));
        assert!(!is_pixelated(&[rect(2.5, 2.0, 6.0, 5.0)]));
        let hq = rasterize(&r, SelectionQuality::HighQuality, IntRect::new(0, 0, 8, 8));
        assert_eq!(row(&hq, 3, 0, 8), [0, 0, 255, 255, 255, 255, 0, 0]);
        assert!(is_pixelated(&[]));
    }

    /// `crop` reads 0 where `r` leaves the mask's rect.
    #[test]
    fn crop_partly_outside() {
        let m = Mask {
            rect: IntRect::new(2, 1, 2, 2),
            data: vec![1, 2, 3, 4],
        };
        assert_eq!(
            m.crop(IntRect::new(1, 0, 3, 3)),
            [0, 0, 0, 0, 1, 2, 0, 3, 4]
        );
        assert_eq!(m.crop(IntRect::new(3, 2, 2, 1)), [4, 0]);
        assert_eq!(m.crop(IntRect::new(9, 9, 1, 1)), [0]);
    }

    #[test]
    fn even_odd_hole() {
        let r = [rect(0.0, 0.0, 6.0, 6.0), rect(2.0, 2.0, 4.0, 4.0)];
        let m = rasterize(&r, SelectionQuality::Aliased, IntRect::new(0, 0, 6, 6));
        assert_eq!(row(&m, 3, 0, 6), [255, 255, 0, 0, 255, 255]);
    }
}
