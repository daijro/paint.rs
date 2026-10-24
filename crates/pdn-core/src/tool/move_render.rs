//! Move Selected Pixels' pixel output: the transformed source through the replacing move kernel.
//! Task 28's paste renders through the same function.
//!
//! Rulings P2 (selection-and-move.md §6 items 3, 4): the GPU and Direct2D modes are CPU
//! approximations — Linear is exact f32 bilinear at M⁻¹(pixel centre) clamped to Q,
//! Multisample-linear the mean of four bilinear taps on the rotated grid, High-quality cubic
//! Catmull-Rom with the 1/8 pre-scale, Anisotropic the same cubic (a proxy for the D2D built-in);
//! the source-rect coverage is `pixel::mask::rasterize` of M(Q) at the shared quality.
// calibrate: B-S1 — the D2D/GPU resamplers (Linear, Multisample, Anisotropic, HQ cubic) and the
// source-rect coverage; only the NN and bilinear paths are pinned by tests.

use crate::document::surface::Surface;
use crate::extra::perspective::Homography;
use crate::pixel::gamma::quantize;
use crate::pixel::mask::{Mask, SelectionQuality, rasterize};
use crate::pixel::{IntRect, Px, ops::move_pixel};
use crate::render::display::{SPAN_GAP, SpanMask, for_bands, to_srgb8};
use crate::resample::magic::CATMULL_ROM;
use crate::resample::{Gamma, PremulImage, pdn::box_average, widen};
use crate::selection::matrix::Matrix;
use crate::util::geom::Point;

/// The move tools' interpolation mode (§2.0 Move row), with paintdotjs's setting values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveInterpolation {
    NearestNeighbor,
    Linear,
    MultisampleLinear,
    Anisotropic,
    HighQualityCubic,
}

impl MoveInterpolation {
    /// `"nearest"`, `"linear"`, `"multisampleLinear"`, `"anisotropic"`, `"highQualityCubic"`;
    /// anything else is the default, HighQualityCubic.
    pub fn from_value(v: &str) -> MoveInterpolation {
        match v {
            "nearest" => MoveInterpolation::NearestNeighbor,
            "linear" => MoveInterpolation::Linear,
            "multisampleLinear" => MoveInterpolation::MultisampleLinear,
            "anisotropic" => MoveInterpolation::Anisotropic,
            _ => MoveInterpolation::HighQualityCubic,
        }
    }
}

/// §2.9 step 1: NearestNeighbor when `f` is an integer translation, else the setting.
pub fn effective_interpolation(f: &Matrix, setting: MoveInterpolation) -> MoveInterpolation {
    if f.is_integer_translation() {
        MoveInterpolation::NearestNeighbor
    } else {
        setting
    }
}

/// Exactly `v.floor()`, without the libm call it is on x86-64 (no SSE4.1 rounding): below 2²³ the
/// truncation, one less for a negative fraction; integers (±0 too), NaN and ±∞ as they are.
fn floor32(v: f32) -> f32 {
    if v.is_nan() || v.abs() >= 8_388_608.0 {
        return v;
    }
    let t = v as i32 as f32;
    if t == v {
        v
    } else if t > v {
        t - 1.0
    } else {
        t
    }
}

/// `floor32` for f64 (below 2⁵²).
fn floor64(v: f64) -> f64 {
    if v.is_nan() || v.abs() >= 4_503_599_627_370_496.0 {
        return v;
    }
    let t = v as i64 as f64;
    if t == v {
        v
    } else if t > v {
        t - 1.0
    } else {
        t
    }
}

/// Exact f32 bilinear over a `w × h` grid of premultiplied texels (`texel` takes indices already
/// clamped into the grid): texel centres at i + 0.5, clamp-to-edge.
fn bilinear(texel: impl Fn(usize, usize) -> [f32; 4], w: usize, h: usize, p: [f32; 2]) -> [f32; 4] {
    if w == 0 || h == 0 || !(p[0].is_finite() && p[1].is_finite()) {
        return [0.0; 4];
    }
    let axis = |v: f32, n: usize| {
        let f = v - 0.5;
        let i = floor32(f);
        // The integer-valued k clamped as an integer (the cast saturates): the float clamp's.
        let clamp = |k: f32| (k as i64).clamp(0, n as i64 - 1) as usize;
        (clamp(i), clamp(i + 1.0), f - i)
    };
    let (x0, x1, tx) = axis(p[0], w);
    let (y0, y1, ty) = axis(p[1], h);
    let (a, b, c, d) = (texel(x0, y0), texel(x1, y0), texel(x0, y1), texel(x1, y1));
    std::array::from_fn(|k| {
        let top = a[k] + (b[k] - a[k]) * tx;
        let bottom = c[k] + (d[k] - c[k]) * tx;
        top + (bottom - top) * ty
    })
}

/// Bilinear at `p` (document coordinates) over `src ∩ q`: premultiplied in `gamma`'s space,
/// texel centres at i + 0.5, clamped to Q.
pub fn sample_bilinear(src: &Surface, q: IntRect, p: [f32; 2], gamma: Gamma) -> [f32; 4] {
    let q = q.intersect(&src.int_bounds());
    let texel = |i: usize, j: usize| {
        widen(
            src.pixel(q.x as u32 + i as u32, q.y as u32 + j as u32),
            gamma,
        )
    };
    let local = [p[0] - q.x as f32, p[1] - q.y as f32];
    bilinear(texel, q.w as usize, q.h as usize, local)
}

/// Premultiplied (in `gamma`'s space) → straight RGBA8: a ≤ 0 → (0, 0, 0, 0); colour c/a encoded
/// with `linear_to_srgb8` (Linear, through its threshold table) or `quantize` (Companded); alpha
/// `quantize`d.
pub fn output_pixel(premul: [f32; 4], gamma: Gamma) -> Px {
    let a = premul[3];
    if a.is_nan() || a <= 0.0 {
        return [0; 4];
    }
    let c = |v: f32| match gamma {
        Gamma::Linear => to_srgb8(v / a),
        Gamma::Companded => quantize(v / a),
    };
    [c(premul[0]), c(premul[1]), c(premul[2]), quantize(a)]
}

/// One move render (§2.9). `m` = M = Delta·Edit maps `source` to the destination; `base_mask` is
/// `None` for paste and copy-behind (bm = 0).
pub struct MoveRender<'a> {
    pub layer: &'a Surface,
    pub source: &'a Surface,
    pub q: IntRect,
    pub m: Matrix,
    /// R230 (Extra) Distort: the projective map from `source` to the destination, used instead of
    /// `m` (which then only sizes the cubic pre-scale); the setting's sampler always (no NN
    /// shortcut).
    // ponytail: the pre-scale reads F's scale, not the distort's, so a corner squeezed past 1/8
    // aliases in the cubic modes; size it from the quad's smallest edge if that shows.
    pub h: Option<Homography>,
    pub interpolation: MoveInterpolation,
    pub gamma: Gamma,
    pub quality: SelectionQuality,
    pub final_mask: &'a Mask,
    pub base_mask: Option<&'a Mask>,
}

/// The source the D2D-path samplers read: Q in premultiplied f32, pre-scaled by 8 per axis while
/// that axis of M scales to ≤ 1/8 (cubic modes only, §3 "Mip pre-scale"); `k` is source px per
/// image px.
struct Sampled {
    img: PremulImage,
    k: [f64; 2],
}

/// The length of M's linear part applied to a unit axis.
fn axis_scale(m: &Matrix, v: Point) -> f64 {
    let mut v = [v];
    m.transform_vectors(&mut v);
    v[0].x.hypot(v[0].y)
}

/// The sampler `p` renders with: §2.9 step 1's, the setting's under a distort.
fn mode(p: &MoveRender) -> MoveInterpolation {
    match p.h {
        Some(_) => p.interpolation,
        None => effective_interpolation(&p.m, p.interpolation),
    }
}

/// What `Sampled` is made from: the source's content stamp (`Surface::revision`: equal stamps,
/// equal pixels), Q, the gamma and the pre-scale's box sizes in order.
type SampledKey = (u64, IntRect, Gamma, Vec<(u32, u32)>);

/// What `render_moved_into` keeps between renders: the samplers' source, made again only when
/// its `SampledKey` changes (not each frame of a drag).
#[derive(Default)]
pub struct MoveCache {
    sampled: Option<(SampledKey, Sampled)>,
}

impl MoveCache {
    /// Holds no source.
    pub fn is_empty(&self) -> bool {
        self.sampled.is_none()
    }

    /// The source `p`'s samplers read over `q`, made again when its key changes; `None` on the
    /// NN path, which keeps the source it does not use.
    fn source(&mut self, p: &MoveRender, q: IntRect) -> Option<&Sampled> {
        let key = sampled_key(p, q)?;
        if self.sampled.as_ref().is_none_or(|(k, _)| *k != key) {
            let s = Sampled::new(p, q, &key.3);
            self.sampled = Some((key, s));
        }
        self.sampled.as_ref().map(|(_, s)| s)
    }
}

impl Sampled {
    /// The pre-scale's box sizes, in order: while an axis of M scales to ≤ 1/8 (in the cubic
    /// modes), that axis shrinks by 8.
    fn steps(p: &MoveRender, q: IntRect) -> Vec<(u32, u32)> {
        let mut steps = vec![];
        if matches!(
            p.interpolation,
            MoveInterpolation::HighQualityCubic | MoveInterpolation::Anisotropic
        ) {
            let s = [
                axis_scale(&p.m, Point::new(1.0, 0.0)),
                axis_scale(&p.m, Point::new(0.0, 1.0)),
            ];
            let (mut size, mut k) = ((q.w, q.h), [1.0, 1.0]);
            loop {
                let shrink = |n: u32, axis: usize| {
                    if s[axis] * k[axis] <= 0.125 && n > 1 {
                        n.div_ceil(8)
                    } else {
                        n
                    }
                };
                let next = (shrink(size.0, 0), shrink(size.1, 1));
                if next == size {
                    break;
                }
                k[0] *= size.0 as f64 / next.0 as f64;
                k[1] *= size.1 as f64 / next.1 as f64;
                steps.push(next);
                size = next;
            }
        }
        steps
    }

    fn new(p: &MoveRender, q: IntRect, steps: &[(u32, u32)]) -> Sampled {
        let crop = p.source.get_image_data(q.x, q.y, q.w, q.h);
        let mut img = PremulImage::from_rgba8(&crop.data, q.w, q.h, p.gamma);
        let mut k = [1.0, 1.0];
        for &(dw, dh) in steps {
            k[0] *= img.w as f64 / dw as f64;
            k[1] *= img.h as f64 / dh as f64;
            img = box_average(&img, dw, dh);
        }
        Sampled { img, k }
    }

    fn texel(&self, i: usize, j: usize) -> [f32; 4] {
        self.img.px[j * self.img.w as usize + i]
    }

    fn bilinear(&self, p: [f32; 2]) -> [f32; 4] {
        bilinear(
            |i, j| self.texel(i, j),
            self.img.w as usize,
            self.img.h as usize,
            p,
        )
    }

    /// Catmull-Rom (`kernel_weight(CatmullRom, ·)`: `CATMULL_ROM`), 4×4 taps clamped to the image.
    fn cubic(&self, p: [f32; 2]) -> [f32; 4] {
        let (w, h) = (self.img.w as i64, self.img.h as i64);
        if w == 0 || h == 0 || !(p[0].is_finite() && p[1].is_finite()) {
            return [0.0; 4];
        }
        let taps = |v: f32, n: i64| {
            let f = v as f64 - 0.5;
            let i = floor64(f);
            let t = f - i;
            let w = [t + 1.0, t, 1.0 - t, 2.0 - t].map(|d| CATMULL_ROM.value(d.abs()) as f32);
            let idx = [-1.0, 0.0, 1.0, 2.0].map(|o| ((i + o) as i64).clamp(0, n - 1) as usize);
            (idx, w)
        };
        let (xi, xw) = taps(p[0], w);
        let (yi, yw) = taps(p[1], h);
        let mut out = [0.0f32; 4];
        for (&j, &wy) in yi.iter().zip(&yw) {
            let row = &self.img.px[j * w as usize..][..w as usize];
            for (&i, &wx) in xi.iter().zip(&xw) {
                let t = row[i];
                for k in 0..4 {
                    out[k] += t[k] * wx * wy;
                }
            }
        }
        out
    }
}

/// §2.9 steps 2–6 over `rect` (clipped to the layer; straight RGBA8 of the clipped rect): X from
/// the 40.24 NN copy or the samplers, s = coverage of M(Q) (the NN copy's own mask on that path),
/// tc = min(s, fm), bm, then `move_pixel(L, X, tc, bm)`. Commit renders the same (§2.9 step 7).
pub fn render_moved(p: &MoveRender, rect: IntRect) -> Vec<u8> {
    let rect = rect.intersect(&p.layer.int_bounds());
    let mut out = vec![0; (rect.w * rect.h * 4) as usize];
    // shortcut: computes an unused diff here; add a no-diff path if this pixel-only helper gets hot.
    let _ = render_moved_into(p, None, rect, &mut MoveCache::default(), &mut out, rect);
    out
}

/// One M's samplers (§2.9 steps 2–3): the live copy's, or a trail stamp's.
struct Placement<'s> {
    inv: Option<Matrix>,
    /// R230: the distort's destination → source map (`inv` is then `None`).
    hinv: Option<Homography>,
    mode: MoveInterpolation,
    /// O = M⁻¹(½, ½)·2²⁴ and the per-pixel increments, truncated to i64: the NN path's source
    /// pixel, and a trail stamp's final-mask pixel.
    fixed: Option<([i64; 2], [i64; 2], [i64; 2])>,
    sampled: Option<&'s Sampled>,
    /// s off the NN path: the coverage of M(Q).
    coverage: Option<Mask>,
    /// §3 "Multisample taps": u = (0.125, 0.375) and its rotation by 90°, mapped by M⁻¹'s linear
    /// part (unmapped under a distort: its taps are mapped one by one).
    u: [Point; 2],
}

/// The samplers' source `p`'s M needs over `q`: `None` on the NN path or without M⁻¹.
fn sampled_key(p: &MoveRender, q: IntRect) -> Option<SampledKey> {
    let invertible =
        p.h.map_or(p.m.invert().is_some(), |h| h.inverse().is_some());
    (mode(p) != MoveInterpolation::NearestNeighbor && !q.is_empty() && invertible)
        .then(|| (p.source.revision(), q, p.gamma, Sampled::steps(p, q)))
}

impl<'s> Placement<'s> {
    /// `p.m`'s samplers, the coverage over `rect`.
    fn new(p: &MoveRender, q: IntRect, rect: IntRect, sampled: Option<&'s Sampled>) -> Self {
        let hinv = p.h.and_then(|h| h.inverse()).filter(|_| !q.is_empty());
        let inv = p.m.invert().filter(|_| !q.is_empty() && p.h.is_none());
        const ONE: f64 = (1u64 << 24) as f64;
        let fixed = inv.map(|inv| {
            let mut pts = [
                Point::new(0.5, 0.5),
                Point::new(1.5, 0.5),
                Point::new(0.5, 1.5),
            ];
            inv.transform_points(&mut pts);
            let fixed = |v: f64| (v * ONE) as i64;
            let o = [fixed(pts[0].x), fixed(pts[0].y)];
            let dx = [fixed(pts[1].x - pts[0].x), fixed(pts[1].y - pts[0].y)];
            let dy = [fixed(pts[2].x - pts[0].x), fixed(pts[2].y - pts[0].y)];
            (o, dx, dy)
        });
        let coverage = sampled.map(|_| {
            let mut corners = [
                Point::new(q.x as f64, q.y as f64),
                Point::new(q.right() as f64, q.y as f64),
                Point::new(q.right() as f64, q.bottom() as f64),
                Point::new(q.x as f64, q.bottom() as f64),
            ];
            let poly = match p.h {
                // Q's corners past the distort's horizon would wrap round infinity: no coverage.
                Some(h) if corners.iter().any(|c| h.w(c.x, c.y) <= 0.0) => vec![],
                Some(h) => corners.map(|c| h.apply(c.x, c.y).into()).to_vec(),
                None => {
                    p.m.transform_points(&mut corners);
                    corners.map(|c| [c.x, c.y]).to_vec()
                }
            };
            rasterize(&[poly], p.quality, rect)
        });
        let mut u = [Point::new(0.125, 0.375), Point::new(-0.375, 0.125)];
        if let Some(inv) = inv {
            inv.transform_vectors(&mut u);
        }
        Placement {
            inv,
            hinv,
            mode: mode(p),
            fixed,
            sampled,
            coverage,
            u,
        }
    }

    /// ⌊(O + x·dx + y·dy) ≫ 24⌋: the source pixel nearest M⁻¹ of (x, y)'s centre (under a
    /// distort, the one under its centre's preimage).
    fn source_px(&self, x: i32, y: i32) -> Option<(i64, i64)> {
        if let Some(h) = self.hinv {
            let (sx, sy) = h.apply(x as f64 + 0.5, y as f64 + 0.5);
            return Some((floor64(sx) as i64, floor64(sy) as i64));
        }
        let (o, dx, dy) = self.fixed.as_ref()?;
        let (x64, y64) = (x as i64, y as i64);
        // Wrapping int64 arithmetic: a huge M⁻¹ wraps, it does not trap.
        let at = |k: usize| {
            o[k].wrapping_add(x64.wrapping_mul(dx[k]))
                .wrapping_add(y64.wrapping_mul(dy[k]))
                >> 24
        };
        Some((at(0), at(1)))
    }

    /// X and tc = min(s, fm) at (x, y) (§2.9 steps 2–4): the 40.24 NN copy (its own mask: s = 255
    /// inside Q) or the samplers; `p` gives the source and the gamma.
    fn sample(&self, p: &MoveRender, q: IntRect, x: i32, y: i32, fm: u8) -> (Px, u8) {
        if self.mode == MoveInterpolation::NearestNeighbor {
            let Some((sx, sy)) = self.source_px(x, y) else {
                return ([0; 4], 0);
            };
            let inside = (q.x as i64..q.right() as i64).contains(&sx)
                && (q.y as i64..q.bottom() as i64).contains(&sy);
            return if inside {
                (p.source.pixel(sx as u32, sy as u32), fm)
            } else {
                ([0; 4], 0)
            };
        }
        let (Some(src), Some(cov)) = (self.sampled, &self.coverage) else {
            return ([0; 4], 0);
        };
        let tc = cov.get(x, y).min(fm);
        if tc == 0 {
            return ([0; 4], 0);
        }
        let centre = Point::new(x as f64 + 0.5, y as f64 + 0.5);
        let back = |p: Point| {
            let (x, y) = self.hinv.map_or((p.x, p.y), |h| h.apply(p.x, p.y));
            Point::new(x, y)
        };
        let mut c = [back(centre)];
        match self.inv {
            Some(inv) => inv.transform_points(&mut c),
            None if self.hinv.is_none() => return ([0; 4], 0),
            None => {}
        }
        // x / 1 is x: no division without a pre-scale.
        let div = |v: f64, k: f64| if k == 1.0 { v } else { v / k };
        let at = |v: Point| {
            [
                div(v.x - q.x as f64, src.k[0]) as f32,
                div(v.y - q.y as f64, src.k[1]) as f32,
            ]
        };
        let u = &self.u;
        let premul = match self.mode {
            MoveInterpolation::Linear => src.bilinear(at(c[0])),
            MoveInterpolation::MultisampleLinear => {
                // p ± u and p ± (−u.y, u.x); ¼ each (a distort maps each tap back).
                let tap = |k: usize| {
                    let (s, u) = (if k < 2 { 1.0 } else { -1.0 }, u[k % 2]);
                    match self.hinv {
                        Some(_) => back(Point::new(centre.x + s * u.x, centre.y + s * u.y)),
                        None => Point::new(c[0].x + s * u.x, c[0].y + s * u.y),
                    }
                };
                let taps = [0, 1, 2, 3].map(tap);
                let mut sum = [0.0f32; 4];
                for t in taps {
                    let v = src.bilinear(at(t));
                    for k in 0..4 {
                        sum[k] += v[k] * 0.25;
                    }
                }
                sum
            }
            _ => src.cubic(at(c[0])),
        };
        (output_pixel(premul, p.gamma), tc)
    }
}

/// S§4.6 (Extra): composites `stamps` (each M with its destination bounds), oldest first, into
/// `c`: `p.layer`-sized, row-major, `None` where no stamp covers the pixel yet (L and bm still to
/// apply). Each renders as the live copy would with its M (or distort), its final mask `mask` (the selection
/// under Base over Q) at its NN source pixel; the first to cover a pixel takes its bm. Returns
/// the pixels visited.
// ponytail: the NN-mapped mask equals the rasterized one for the integer translations a drag
// stamps; a stamp rotated or scaled beforehand gets a stepped selection edge. Rasterize each
// stamp's selection if that shows.
pub fn bake_stamps(
    p: &MoveRender,
    stamps: &[(Matrix, Option<Homography>, IntRect)],
    mask: &Mask,
    cache: &mut MoveCache,
    c: &mut [Option<Px>],
) -> u64 {
    let q = p.q.intersect(&p.source.int_bounds());
    let w = p.layer.width() as usize;
    let mut visited = 0;
    for &(m, h, b) in stamps {
        let b = b.intersect(&p.layer.int_bounds());
        if b.is_empty() {
            continue;
        }
        let s = MoveRender { m, h, ..*p };
        let at = Placement::new(&s, q, b, cache.source(&s, q));
        let fm = |x: i32, y: i32| {
            let Some((sx, sy)) = at.source_px(x, y) else {
                return 0;
            };
            match (i32::try_from(sx), i32::try_from(sy)) {
                (Ok(sx), Ok(sy)) => mask.get(sx, sy),
                _ => 0,
            }
        };
        for_bands(c, w, b.y as usize..b.bottom() as usize, 8, |y0, band| {
            for (j, line) in band.chunks_exact_mut(w).enumerate() {
                let y = (y0 + j) as i32;
                for x in b.x..b.right() {
                    let (sample, tc) = at.sample(p, q, x, y, fm(x, y));
                    let px = &mut line[x as usize];
                    if tc > 0 {
                        let (l, bm) = match *px {
                            Some(l) => (l, 0),
                            None => (
                                p.layer.pixel(x as u32, y as u32),
                                p.base_mask.map_or(0, |m| m.get(x, y)),
                            ),
                        };
                        *px = Some(move_pixel(l, sample, tc, bm));
                    }
                }
            }
        });
        visited += b.w as u64 * b.h as u64;
    }
    visited
}

/// `render_moved` into `out`, straight RGBA8 of `area` (`area.w` px a row), over `rect` ∩ the
/// layer ∩ `area` only (a move tool renders into its layer), with the samplers' source kept in
/// `cache` while its `SampledKey` holds. Rows render in parallel bands; every pixel is computed
/// alone, so the bytes are the same. Returns the pixels whose bytes changed in `out`: one-row
/// rects sorted by row, each row's runs merged across gaps under `SPAN_GAP` (the canvas redraws
/// only those, Task 27r).
///
/// S§4.6 (Extra) `under`: the trail's stamps (`bake_stamps`); the live copy renders over a pixel
/// one covers with bm = 0 (its bm is applied). Without it the bytes are the plain render's.
pub fn render_moved_into(
    p: &MoveRender,
    under: Option<&[Option<Px>]>,
    rect: IntRect,
    cache: &mut MoveCache,
    out: &mut [u8],
    area: IntRect,
) -> Vec<IntRect> {
    let rect = rect.intersect(&p.layer.int_bounds()).intersect(&area);
    if rect.is_empty() {
        return vec![];
    }
    let q = p.q.intersect(&p.source.int_bounds());
    // An empty final mask places nothing (tc = 0 everywhere): no samplers to build.
    let placing = !p.final_mask.rect.is_empty();
    let live = placing.then(|| Placement::new(p, q, rect, cache.source(p, q)));
    let w = p.layer.width() as usize;
    let under = |x: i32, y: i32| match under.and_then(|c| c[y as usize * w + x as usize]) {
        Some(l) => (l, 0),
        None => (
            p.layer.pixel(x as u32, y as u32),
            p.base_mask.map_or(0, |m| m.get(x, y)),
        ),
    };
    let (row, cols) = (area.w as usize * 4, (rect.x - area.x) as usize * 4..);
    let rows = (rect.y - area.y) as usize..(rect.bottom() - area.y) as usize;
    let changed = std::sync::Mutex::new(vec![]);
    for_bands(out, row, rows, 8, |y0, band| {
        // A bit per pixel whose bytes changed: each row is compared with a copy of what it held.
        let n = band.len() / row;
        let mut mask = SpanMask::new(IntRect::new(rect.x, area.y + y0 as i32, rect.w, n as u32));
        let mut was = vec![];
        for (j, line) in band.chunks_exact_mut(row).enumerate() {
            let y = area.y + (y0 + j) as i32;
            let line = &mut line[cols.clone()][..rect.w as usize * 4];
            let line = line.as_chunks_mut::<4>().0;
            was.clear();
            was.extend_from_slice(line);
            for (i, px) in line.iter_mut().enumerate() {
                let x = rect.x + i as i32;
                let (l, b) = under(x, y);
                let (sample, tc) = match &live {
                    Some(live) => live.sample(p, q, x, y, p.final_mask.get(x, y)),
                    None => ([0; 4], 0),
                };
                *px = move_pixel(l, sample, tc, b);
            }
            let words = mask.row_mut(y);
            for (w, (a, b)) in words.iter_mut().zip(was.chunks(64).zip(line.chunks(64))) {
                let pairs = a.iter().zip(b).enumerate();
                *w = pairs.fold(0, |bits, (i, (a, b))| bits | ((a != b) as u64) << i);
            }
        }
        let (runs, _) = mask.spans(SPAN_GAP);
        if !runs.is_empty() {
            changed.lock().expect("a band panicked").push((y0, runs));
        }
    });
    let mut changed = changed.into_inner().expect("a band panicked");
    changed.sort_unstable_by_key(|c| c.0);
    changed.into_iter().flat_map(|c| c.1).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.9 step 2: a non-integer M on the NN path samples ⌊(O + x·dx + y·dy) ≫ 24⌋ — a ½-px shift
    /// right still copies (the centre lands on the source pixel's right half).
    #[test]
    fn nn_fixed_point_truncates() {
        let mut src = Surface::create(4, 1);
        for x in 0..4 {
            src.fill_rect(IntRect::new(x, 0, 1, 1), [x as u8 * 50, 0, 0, 255]);
        }
        let all = IntRect::new(0, 0, 4, 1);
        let mut m = Matrix::identity();
        m.translate(0.5, 0.0);
        let opaque = Mask::opaque(all);
        let p = MoveRender {
            layer: &src,
            source: &src,
            q: all,
            m,
            h: None,
            interpolation: MoveInterpolation::NearestNeighbor,
            gamma: Gamma::Linear,
            quality: SelectionQuality::HighQuality,
            final_mask: &opaque,
            base_mask: None,
        };
        let out = render_moved(&p, all);
        // Pixel x samples ⌊x + ½ − ½⌋ = x.
        assert_eq!(
            out.chunks(4).map(|c| c[0]).collect::<Vec<_>>(),
            [0, 50, 100, 150]
        );
    }

    /// The floors are `floor` bit for bit: fractions either side of 0, integers, ±0, the 2²³ / 2⁵²
    /// edges, NaN and ±∞.
    #[test]
    fn floors_match_the_libm_floor() {
        let mut v32 = vec![0.0f32, -0.0, 0.5, -0.5, 1.0, -1.0, 2.5, -2.5, 1e-30, -1e-30];
        v32.extend([
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            f32::MAX,
            f32::MIN,
        ]);
        for e in [
            8_388_607.5f32,
            8_388_608.0,
            16_777_217.0,
            2_147_483_648.0,
            3e9,
        ] {
            v32.extend([
                e,
                -e,
                f32::from_bits(e.to_bits() - 1),
                -f32::from_bits(e.to_bits() + 1),
            ]);
        }
        v32.extend((0..20_000).map(|i| (i as f32 - 10_000.0) * 0.0137));
        for v in v32 {
            assert_eq!(floor32(v).to_bits(), v.floor().to_bits(), "{v:e}");
            let d = v as f64 * 1.000_000_1;
            assert_eq!(floor64(d).to_bits(), d.floor().to_bits(), "{d:e}");
        }
        for e in [
            4_503_599_627_370_495.5f64,
            4_503_599_627_370_496.0,
            9.3e18,
            1e300,
        ] {
            for d in [
                e,
                -e,
                f64::from_bits(e.to_bits() - 1),
                -f64::from_bits(e.to_bits() + 1),
            ] {
                assert_eq!(floor64(d).to_bits(), d.floor().to_bits(), "{d:e}");
            }
        }
        assert!(floor32(f32::NAN).is_nan() && floor64(f64::NAN).is_nan());
    }

    /// `floor32` over every f32 bit pattern: `cargo test --release -p pdn-core --lib every_f32 --
    /// --ignored`.
    #[test]
    #[ignore = "2³² values: run in release"]
    fn every_f32_floors_like_libm() {
        use rayon::prelude::*;
        (0..=u32::MAX).into_par_iter().for_each(|b| {
            let v = f32::from_bits(b);
            let (a, e) = (floor32(v), v.floor());
            assert!(
                a.to_bits() == e.to_bits() || (a.is_nan() && e.is_nan()),
                "{b:#x}"
            );
        });
    }

    /// The cubic weights at a texel centre are (0, 1, 0, 0): a pure ½-px-free sample is exact, and
    /// the multisample taps average to the same flat colour.
    #[test]
    fn samplers_reproduce_flat_colour() {
        let mut src = Surface::create(6, 6);
        src.fill_rect(IntRect::new(0, 0, 6, 6), [200, 100, 50, 255]);
        let all = IntRect::new(0, 0, 6, 6);
        let mut m = Matrix::identity();
        m.rotate_at(30.0, Point::new(3.0, 3.0));
        let opaque = Mask::opaque(all);
        for interpolation in [
            MoveInterpolation::Linear,
            MoveInterpolation::MultisampleLinear,
            MoveInterpolation::HighQualityCubic,
        ] {
            let p = MoveRender {
                layer: &src,
                source: &src,
                q: all,
                m,
                h: None,
                interpolation,
                gamma: Gamma::Linear,
                quality: SelectionQuality::HighQuality,
                final_mask: &opaque,
                base_mask: None,
            };
            let out = render_moved(&p, all);
            let c = (3 * 6 + 3) * 4;
            assert_eq!(&out[c..c + 4], &[200, 100, 50, 255], "{interpolation:?}");
        }
    }

    /// §3 "Mip pre-scale": a 1/10 scale (≤ 1/8) box-averages by 8 first, so a 2-px checker
    /// averages out.
    #[test]
    fn cubic_prescales_by_eight() {
        let mut src = Surface::create(32, 1);
        for x in 0..32 {
            let v = if x % 2 == 0 { 255 } else { 0 };
            src.fill_rect(IntRect::new(x, 0, 1, 1), [v, v, v, 255]);
        }
        let mut m = Matrix::identity();
        m.scale(1.0 / 10.0, 1.0);
        let p = MoveRender {
            layer: &src,
            source: &src,
            q: IntRect::new(0, 0, 32, 1),
            m,
            h: None,
            interpolation: MoveInterpolation::HighQualityCubic,
            gamma: Gamma::Companded,
            quality: SelectionQuality::HighQuality,
            final_mask: &Mask::opaque(IntRect::new(0, 0, 32, 1)),
            base_mask: None,
        };
        let s = Sampled::new(&p, p.q, &Sampled::steps(&p, p.q));
        assert_eq!((s.img.w, s.k), (4, [8.0, 1.0]));
        assert!(s.img.px.iter().all(|v| (v[0] - 0.5).abs() < 1e-6));
    }
}
