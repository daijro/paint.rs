//! Move Selected Pixels while the pointer drags (`AppWorkspace::float_moves`): the layer holds the
//! selection's pixels lifted out (§2.9 with nothing placed) and the canvas draws them every frame
//! at display resolution, over the view of the layers under them, sampled from the lifted pixels
//! through the move's transform. The drag's end renders them into the layer exactly, so only the
//! drag's own frames differ from a full render: bilinear sampling (box-halved levels zoomed out,
//! the view's ×c pixel blocks zoomed in) instead of the §2.9 samplers, a coverage-weighted mix at
//! the selection's edge instead of the move kernel, and the pixel grid hidden under them.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use super::display::{DisplayTarget, RenderMode, for_bands, store, to_space};
use super::view::ViewTransform;
use crate::document::surface::Surface;
use crate::extra::perspective::Homography;
use crate::pixel::mask::Mask;
use crate::pixel::{IntRect, Px};
use crate::selection::matrix::Matrix;
use crate::util::geom::Point;

/// The lifted pixels: straight RGBA8 over `q` (source px, row-major) and the selection's
/// coverage there.
pub struct FloatSource {
    /// Equal ids, equal pixels (and the same layers under them).
    pub id: u64,
    pub q: IntRect,
    pub px: Vec<Px>,
    pub mask: Vec<u8>,
}

/// Where the lifted pixels are this frame.
#[derive(Clone)]
pub struct Float {
    pub src: Arc<FloatSource>,
    /// The layer they belong to: the top visible one, blending Normal.
    pub layer: usize,
    /// Source → document; `h` instead under a distort (`m` then only sizes the sampling).
    pub m: Matrix,
    pub h: Option<Homography>,
    /// New with every placement: the canvas redraws for a new one.
    pub rev: u64,
}

static NEXT: AtomicU64 = AtomicU64::new(1);

/// A fresh `FloatSource::id` or `Float::rev`.
pub fn next_id() -> u64 {
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl FloatSource {
    /// `src` over `q` (∩ its bounds) with `mask`'s coverage.
    pub fn new(src: &Surface, q: IntRect, mask: &Mask) -> Self {
        let q = q.intersect(&src.int_bounds());
        let mut px = Vec::with_capacity(q.w as usize * q.h as usize);
        for y in q.y..q.bottom() {
            px.extend((q.x..q.right()).map(|x| src.pixel(x as u32, y as u32)));
        }
        FloatSource {
            id: next_id(),
            q,
            px,
            mask: mask.crop(q),
        }
    }
}

impl Float {
    /// The device pixels it can reach under `t` in a `w × h` image: its corners' bounds, a pixel
    /// wider (the bilinear reach), ∩ the image's pixel bounds and the viewport.
    pub fn device_bounds(&self, t: &ViewTransform, w: u32, h: u32) -> IntRect {
        let q = self.src.q;
        let image =
            t.image_pixel_bounds(w, h)
                .intersect(&IntRect::new(0, 0, t.viewport[0], t.viewport[1]));
        let corners = [
            (q.x, q.y),
            (q.right(), q.y),
            (q.right(), q.bottom()),
            (q.x, q.bottom()),
        ]
        .map(|(x, y)| Point::new(x as f64, y as f64));
        let doc = match self.h {
            // Past the horizon the corners wrap round infinity.
            Some(h) if corners.iter().any(|c| h.w(c.x, c.y) <= 0.0) => return image,
            Some(h) => corners.map(|c| {
                let (x, y) = h.apply(c.x, c.y);
                Point::new(x, y)
            }),
            None => {
                let mut c = corners;
                self.m.transform_points(&mut c);
                c
            }
        };
        let dev = doc.map(|p| (p.x * t.scale + t.origin[0], p.y * t.scale + t.origin[1]));
        let edge = |v: f64| v.clamp(-1e9, 1e9) as i32;
        let (x0, x1) = dev
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        let (y0, y1) = dev
            .iter()
            .fold((f64::MAX, f64::MIN), |(a, b), p| (a.min(p.1), b.max(p.1)));
        let (x0, y0) = (edge(x0.floor()) - 1, edge(y0.floor()) - 1);
        let (x1, y1) = (edge(x1.ceil()) + 1, edge(y1.ceil()) + 1);
        IntRect::new(x0, y0, (x1 - x0).max(0) as u32, (y1 - y0).max(0) as u32).intersect(&image)
    }

    /// Device px per source px along M's shorter axis under `t`.
    fn scale(&self, t: &ViewTransform) -> f64 {
        let mut v = [Point::new(1.0, 0.0), Point::new(0.0, 1.0)];
        self.m.transform_vectors(&mut v);
        t.scale * v[0].x.hypot(v[0].y).min(v[1].x.hypot(v[1].y))
    }

    /// The box-halved level a render under `t` samples: device px per level texel in [1, 2)
    /// zoomed out, 0 (the source) otherwise.
    pub fn level(&self, t: &ViewTransform) -> usize {
        let k = self.scale(t);
        if k >= 1.0 || k <= 0.0 || !k.is_finite() {
            0
        } else {
            ((1.0 / k).log2().ceil() as usize).min(16)
        }
    }
}

/// One box-halved level: colour × coverage (premultiplied, target space) and coverage per texel.
struct Level {
    w: usize,
    h: usize,
    px: Vec<[f32; 5]>,
}

/// A source's box-halved levels in a target's space, built as renders need them.
pub struct Levels {
    id: u64,
    mode: RenderMode,
    levels: Vec<Level>,
}

/// Level 0's texel (i, j) of `src` in `mode`'s space: colour × coverage and coverage; zero
/// outside.
fn source_texel(src: &FloatSource, mode: RenderMode, i: i64, j: i64) -> [f32; 5] {
    let (w, h) = (src.q.w as i64, src.q.h as i64);
    if i < 0 || j < 0 || i >= w || j >= h {
        return [0.0; 5];
    }
    let k = (j * w + i) as usize;
    let m = src.mask[k];
    if m == 0 {
        return [0.0; 5];
    }
    let c = m as f32 / 255.0;
    let p = to_space(mode, src.px[k]);
    [p[0] * c, p[1] * c, p[2] * c, p[3] * c, c]
}

impl Levels {
    /// `src`'s levels in `mode`, up to `n` (kept from `old` when it holds the same source).
    pub fn up_to(old: Option<Levels>, src: &FloatSource, mode: RenderMode, n: usize) -> Levels {
        let mut l = old
            .filter(|l| (l.id, l.mode) == (src.id, mode))
            .unwrap_or(Levels {
                id: src.id,
                mode,
                levels: vec![],
            });
        while l.levels.len() < n {
            let (pw, ph) = match l.levels.last() {
                Some(p) => (p.w, p.h),
                None => (src.q.w as usize, src.q.h as usize),
            };
            let (w, h) = (pw.div_ceil(2).max(1), ph.div_ceil(2).max(1));
            let mut px = vec![[0.0f32; 5]; w * h];
            let prev = l.levels.last();
            let texel = |i: usize, j: usize| -> [f32; 5] {
                match prev {
                    Some(p) if i < p.w && j < p.h => p.px[j * p.w + i],
                    Some(_) => [0.0; 5],
                    None => source_texel(src, mode, i as i64, j as i64),
                }
            };
            for_bands(&mut px, w, 0..h, 16, |y0, band| {
                for (r, row) in band.chunks_mut(w).enumerate() {
                    let j = 2 * (y0 + r);
                    for (i, o) in row.iter_mut().enumerate() {
                        let q = [
                            texel(2 * i, j),
                            texel(2 * i + 1, j),
                            texel(2 * i, j + 1),
                            texel(2 * i + 1, j + 1),
                        ];
                        *o = std::array::from_fn(|c| (q[0][c] + q[1][c] + q[2][c] + q[3][c]) / 4.0);
                    }
                }
            });
            l.levels.push(Level { w, h, px });
        }
        l
    }
}

/// Composites a float over rows of the canvas (`DisplayTarget::write_rgba8_over`'s painter):
/// within its device bounds, each pixel's sample X (colour × coverage c) is mixed in as the
/// layer would show it there: base·(1 − c) + X·α + below·(c − X.a·α), α the layer's opacity and
/// `below` the view of the layers under it.
pub struct FloatPainter<'a> {
    src: &'a FloatSource,
    level: Option<&'a Level>,
    /// Level texels per source px (×c: the pixel blocks of a source magnified, §2.5 step 3).
    per_texel: f64,
    c: f64,
    mode: RenderMode,
    inv: Option<Matrix>,
    hinv: Option<Homography>,
    t: ViewTransform,
    rect: IntRect,
    below: &'a DisplayTarget,
    opacity: f32,
    /// M⁻¹ without rotation or shear: each column's x taps (the same on every row).
    cols: Option<Vec<Tap>>,
}

/// One axis's bilinear taps: texels i0, i1 and the weight of i1.
type Tap = (i64, i64, f32);

impl<'a> FloatPainter<'a> {
    /// `f` under `t` in a `doc`-sized image, at layer `opacity` (0..=1), over `below`; `levels`
    /// must reach `f.level(t)`.
    pub fn new(
        f: &'a Float,
        t: &ViewTransform,
        doc: (u32, u32),
        opacity: f32,
        levels: &'a Levels,
        below: &'a DisplayTarget,
    ) -> Self {
        let n = f.level(t);
        let k = f.scale(t);
        let c = if n == 0 && k > 1.0 {
            k.ceil().min(256.0)
        } else {
            1.0
        };
        let mut p = FloatPainter {
            src: &f.src,
            level: n.checked_sub(1).and_then(|i| levels.levels.get(i)),
            per_texel: c / (1u64 << n) as f64,
            c,
            mode: below.mode,
            inv: f.h.is_none().then(|| f.m.invert()).flatten(),
            hinv: f.h.and_then(|h| h.inverse()),
            t: *t,
            rect: f.device_bounds(t, doc.0, doc.1),
            below,
            opacity,
            cols: None,
        };
        if let Some(m) = p.inv.map(|m| m.elements)
            && m[0][1] == 0.0
            && m[1][0] == 0.0
        {
            let (r, qx) = (p.rect, p.src.q.x as f64);
            let x = |d: i32| m[0][0] * ((d as f64 + 0.5 - t.origin[0]) / t.scale) + m[0][2] - qx;
            p.cols = Some((r.x..r.right()).map(|d| p.axis(x(d))).collect());
        }
        p
    }

    /// The taps at source px `v` (relative to Q) on the ×c virtual grid.
    fn axis(&self, v: f64) -> Tap {
        let c = self.c as i64;
        let v = v * self.per_texel - 0.5;
        let i = floor(v);
        let f = (v - i as f64) as f32;
        match c {
            1 => (i, i + 1, f),
            c => (i.div_euclid(c), (i + 1).div_euclid(c), f),
        }
    }

    fn texel(&self, i: i64, j: i64) -> [f32; 5] {
        match self.level {
            None => source_texel(self.src, self.mode, i, j),
            Some(l) if i >= 0 && j >= 0 && (i as usize) < l.w && (j as usize) < l.h => {
                l.px[j as usize * l.w + i as usize]
            }
            Some(_) => [0.0; 5],
        }
    }

    /// Bilinear at source px `u` (relative to Q) between texel centres of the ×c virtual grid,
    /// zero past the edges.
    fn sample(&self, u: [f64; 2]) -> [f32; 5] {
        let (x0, x1, fx) = self.axis(u[0]);
        let (y0, y1, fy) = self.axis(u[1]);
        let quad = match self.level {
            // Inside a stored level: its four texels as they are.
            Some(l) if x0 >= 0 && y0 >= 0 && (x1 as usize) < l.w && (y1 as usize) < l.h => {
                let (r0, r1) = (y0 as usize * l.w, y1 as usize * l.w);
                let (i0, i1) = (x0 as usize, x1 as usize);
                [l.px[r0 + i0], l.px[r0 + i1], l.px[r1 + i0], l.px[r1 + i1]]
            }
            _ => [
                self.texel(x0, y0),
                self.texel(x1, y0),
                self.texel(x0, y1),
                self.texel(x1, y1),
            ],
        };
        let [a, b, c, d] = quad;
        std::array::from_fn(|k| {
            let top = a[k] + (b[k] - a[k]) * fx;
            let bottom = c[k] + (d[k] - c[k]) * fx;
            top + (bottom - top) * fy
        })
    }

    /// Row `y`'s pixels from column `x0` (`row`, in target space).
    pub fn paint(&self, y: i32, x0: i32, row: &mut [[f32; 4]]) {
        let r = self.rect;
        if y < r.y || y >= r.bottom() {
            return;
        }
        let lo = r.x.max(x0);
        let hi = r.right().min(x0 + row.len() as i32);
        if lo >= hi {
            return;
        }
        let t = &self.t;
        let doc = |x: i32| {
            Point::new(
                (x as f64 + 0.5 - t.origin[0]) / t.scale,
                (y as f64 + 0.5 - t.origin[1]) / t.scale,
            )
        };
        let q = self.src.q;
        // M⁻¹ is affine: the row's source positions step evenly.
        let affine = self.inv.map(|inv| {
            let mut p = [doc(lo), doc(lo + 1)];
            inv.transform_points(&mut p);
            (p[0], Point::new(p[1].x - p[0].x, p[1].y - p[0].y))
        });
        let below = self.below.row(y);
        // Axis-aligned: the row's two source rows blended once, then one lerp a pixel.
        let blended = self.cols.as_ref().map(|cols| {
            let m = self.inv.expect("cols come from M⁻¹").elements;
            let (y0, y1, fy) = self.axis(m[1][1] * doc(lo).y + m[1][2] - q.y as f64);
            let cols = &cols[(lo - r.x) as usize..(hi - r.x) as usize];
            let first = cols.iter().map(|c| c.0).min().unwrap_or(0);
            let last = cols.iter().map(|c| c.1).max().unwrap_or(0);
            let lerp =
                |a: [f32; 5], b: [f32; 5]| std::array::from_fn(|k| a[k] + (b[k] - a[k]) * fy);
            let texels: Vec<[f32; 5]> = match self.level {
                // A stored level: its rows' slices, zero past its edges.
                Some(l) => {
                    let w = l.w as i64;
                    let zero = [0.0; 5];
                    let row = |j: i64| {
                        (j >= 0 && (j as usize) < l.h).then(|| &l.px[j as usize * l.w..][..l.w])
                    };
                    let n = (last - first + 1) as usize;
                    let (a, b) = (first.clamp(0, w), (last + 1).clamp(0, w));
                    let mut texels = vec![zero; ((a - first).max(0) as usize).min(n)];
                    let span = a as usize..b.max(a) as usize;
                    match (row(y0), row(y1)) {
                        (Some(r0), Some(r1)) => {
                            let pairs = r0[span.clone()].iter().zip(&r1[span]);
                            texels.extend(pairs.map(|(a, b)| lerp(*a, *b)));
                        }
                        (Some(r), None) => texels.extend(r[span].iter().map(|a| lerp(*a, zero))),
                        (None, Some(r)) => texels.extend(r[span].iter().map(|b| lerp(zero, *b))),
                        (None, None) => {}
                    }
                    texels.resize(n, zero);
                    texels
                }
                None => (first..=last)
                    .map(|i| lerp(self.texel(i, y0), self.texel(i, y1)))
                    .collect(),
            };
            (cols, first, texels)
        });
        for x in lo..hi {
            let o = &mut row[(x - x0) as usize];
            if let Some((cols, first, texels)) = &blended {
                let (i0, i1, fx) = cols[(x - lo) as usize];
                let (p, n) = (texels[(i0 - first) as usize], texels[(i1 - first) as usize]);
                self.mix(
                    o,
                    std::array::from_fn(|k| p[k] + (n[k] - p[k]) * fx),
                    below[x as usize],
                );
                continue;
            }
            let u = match (affine, self.hinv) {
                (Some((p, d)), _) => {
                    let i = (x - lo) as f64;
                    Point::new(p.x + d.x * i, p.y + d.y * i)
                }
                (None, Some(h)) => {
                    let p = doc(x);
                    let (x, y) = h.apply(p.x, p.y);
                    Point::new(x, y)
                }
                (None, None) => return,
            };
            let u = [u.x - q.x as f64, u.y - q.y as f64];
            if u[0].abs() < 1e9 && u[1].abs() < 1e9 {
                self.mix(o, self.sample(u), below[x as usize]);
            }
        }
    }

    /// Mixes sample `s` (colour × coverage, coverage) into pixel `o` over `below` (see the type).
    fn mix(&self, o: &mut [f32; 4], s: [f32; 5], below: [f32; 4]) {
        let cover = s[4].min(1.0);
        if cover <= 0.0 {
            return;
        }
        let a = self.opacity;
        let xa = s[3] * a;
        *o = store(
            self.mode,
            if cover >= 1.0 && xa >= 1.0 {
                // Covered and opaque: the sample alone.
                [s[0], s[1], s[2], s[3]]
            } else {
                std::array::from_fn(|k| o[k] * (1.0 - cover) + s[k] * a + below[k] * (cover - xa))
            },
        );
    }
}

/// `v.floor()` as an integer without the libm call (|v| < 1e9 here).
fn floor(v: f64) -> i64 {
    let t = v as i64;
    t - ((t as f64) > v) as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::view::{CanvasStyle, render_view};

    /// An opaque 4 × 4 red square lifted with full coverage, drawn translated by whole pixels at
    /// 100 %: the float shows exactly its pixels where it lands and the base elsewhere; a
    /// half-covered selection mixes half of it in.
    #[test]
    fn whole_pixel_moves_show_the_lifted_pixels() {
        let mut src = Surface::create(8, 8);
        src.fill_rect(IntRect::new(0, 0, 4, 4), [255, 0, 0, 255]);
        let q = IntRect::new(0, 0, 4, 4);
        let mut m = Matrix::identity();
        m.translate(2.0, 3.0);
        let lift = Arc::new(FloatSource::new(&src, q, &Mask::opaque(q)));
        let f = Float {
            src: lift.clone(),
            layer: 0,
            m,
            h: None,
            rev: next_id(),
        };
        let t = ViewTransform {
            scale: 1.0,
            origin: [0.0, 0.0],
            viewport: [8, 8],
            ui_scale: 1.0,
        };
        let mode = RenderMode::Hardware;
        let style = CanvasStyle::for_scheme(false, false, None, 1.0, mode);
        let mut below = DisplayTarget::new(8, 8, mode);
        render_view(
            &Surface::create(8, 8),
            1,
            &mut Default::default(),
            &t,
            &style,
            &mut below,
        );
        let levels = Levels::up_to(None, &lift, mode, f.level(&t));
        let p = FloatPainter::new(&f, &t, (8, 8), 1.0, &levels, &below);
        assert_eq!(f.device_bounds(&t, 8, 8), IntRect::new(1, 2, 6, 6));
        let base = [0.5, 0.5, 0.5, 1.0];
        for y in 0..8 {
            let mut row = vec![base; 8];
            p.paint(y, 0, &mut row);
            for (x, px) in row.iter().enumerate() {
                let inside = (2..6).contains(&x) && (3..7).contains(&y);
                let want = if inside { [1.0, 0.0, 0.0, 1.0] } else { base };
                assert_eq!(*px, want, "({x}, {y})");
            }
        }
        let mut half = Mask::opaque(q);
        half.data.fill(128);
        let lift = Arc::new(FloatSource::new(&src, q, &half));
        let f = Float { src: lift, ..f };
        let p = FloatPainter::new(&f, &t, (8, 8), 1.0, &levels, &below);
        let mut row = vec![base; 8];
        p.paint(4, 0, &mut row);
        let c = 128.0 / 255.0;
        let b = below.get(3, 4);
        let want: [f32; 4] = std::array::from_fn(|k| {
            base[k] * (1.0 - c) + [1.0, 0.0, 0.0, 1.0][k] * c + b[k] * (c - c)
        });
        assert!(
            row[3].iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-6),
            "{:?}",
            row[3]
        );
    }

    /// Without rotation the rows blend their two source rows once and lerp per pixel: the same
    /// samples as the per-pixel bilinear, zoomed in (the ×c grid) and out (a level).
    #[test]
    fn axis_aligned_rows_sample_as_the_general_path() {
        let mut src = Surface::create(40, 30);
        for y in 0..30 {
            for x in 0..40 {
                src.fill_rect(
                    IntRect::new(x, y, 1, 1),
                    [(x * 6) as u8, (y * 8) as u8, 99, 200],
                );
            }
        }
        let q = IntRect::new(3, 2, 30, 25);
        let lift = Arc::new(FloatSource::new(&src, q, &Mask::opaque(q)));
        let mut m = Matrix::identity();
        m.scale(1.3, 0.9);
        m.translate(5.25, -2.6);
        let f = Float {
            src: lift.clone(),
            layer: 0,
            m,
            h: None,
            rev: next_id(),
        };
        for scale in [1.7, 0.3] {
            let t = ViewTransform {
                scale,
                origin: [4.4, 3.1],
                viewport: [80, 60],
                ui_scale: 1.0,
            };
            let mode = RenderMode::Hardware;
            let levels = Levels::up_to(None, &lift, mode, f.level(&t));
            let below = DisplayTarget::new(80, 60, mode);
            let fast = FloatPainter::new(&f, &t, (40, 30), 0.8, &levels, &below);
            assert!(fast.cols.is_some());
            let mut general = FloatPainter::new(&f, &t, (40, 30), 0.8, &levels, &below);
            general.cols = None;
            for y in 0..60 {
                let (mut a, mut b) = (vec![[0.25; 4]; 80], vec![[0.25; 4]; 80]);
                fast.paint(y, 0, &mut a);
                general.paint(y, 0, &mut b);
                for (x, (p, q)) in a.iter().zip(&b).enumerate() {
                    let near = p.iter().zip(q).all(|(p, q)| (p - q).abs() < 1e-5);
                    assert!(near, "{scale} ({x}, {y}): {p:?} vs {q:?}");
                }
            }
        }
    }

    /// Zoomed out the float samples a box-halved level (device px per texel in [1, 2)): a fine
    /// checker averages to grey instead of aliasing.
    #[test]
    fn zoomed_out_samples_a_halved_level() {
        let mut src = Surface::create(64, 64);
        for y in 0..64 {
            for x in 0..64 {
                let v = if (x + y) % 2 == 0 { 255 } else { 0 };
                src.fill_rect(IntRect::new(x, y, 1, 1), [v, v, v, 255]);
            }
        }
        let q = IntRect::new(0, 0, 64, 64);
        let lift = Arc::new(FloatSource::new(&src, q, &Mask::opaque(q)));
        let f = Float {
            src: lift.clone(),
            layer: 0,
            m: Matrix::identity(),
            h: None,
            rev: next_id(),
        };
        let t = ViewTransform {
            scale: 0.3,
            origin: [0.0, 0.0],
            viewport: [20, 20],
            ui_scale: 1.0,
        };
        assert_eq!(f.level(&t), 2);
        let mode = RenderMode::Software;
        let levels = Levels::up_to(None, &lift, mode, 2);
        let below = DisplayTarget::new(20, 20, mode);
        let p = FloatPainter::new(&f, &t, (64, 64), 1.0, &levels, &below);
        let mut row = vec![[0.0; 4]; 20];
        p.paint(8, 0, &mut row);
        assert!(
            row[8].iter().take(3).all(|v| (v - 0.5).abs() < 0.02),
            "{:?}",
            row[8]
        );
    }
}
