//! Ported from PhotoCraft crates/algo/src/vanishing.rs, with the `Homography`, the Keys bicubic and the `warp` sampling of crates/algo/src/transform.rs, @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: one plane, so `Scene`, `Edge`, `tear_off`, `plane_at`, `Lifted::project`, the clone stamp (`clone_stroke`, `sample`, `Window`) and serde are dropped, and `Lifted` keeps only its 3-D corners; `frame` is `Scene::frame` for a lone plane with vp_ui's finite-length guard and no red plane; `Homography` keeps `apply`, `mul`, `inverse`, `rect_to_quad` (with `square_to_quad`) and `local_scale`; `paste` draws straight RGBA8 (`ImageData`) onto a layer, or a smaller proxy of it, in place: transform.rs's `warp` sampling (premultiplied Keys bicubic at A = −0.75, taps clamped to the image, edge coverage, colour divided by the unclamped alpha, then clipped to 0–1 as 8-bit storage clips it) only at destination pixels whose centre lies on the plane (upstream's `quad_mask`) and inside `clip`, composited source-over as vp_cmds' `over`; sampling only inside the convex plane keeps every source point in front of the horizon, so `warp`'s horizon clipping and tiles are not needed, and a destination pixel's footprint is measured at the pixel instead of per tile; `src` is the whole image, where upstream pastes the clipboard surface's `content_bounds()`; the large-downscale prefilter resizes with paint.rs's `resample::resize` (Bicubic, companded: MagicScaler's Catmull-Rom) where upstream's `resize_surface` uses Keys at A = −0.75, and its factor is rounded up to a step of 2^(1/8) (so a scale above 2^(−1/8) ≈ 0.917 is not prefiltered, where upstream prefilters below 0.999) so that `paste_cached` can keep one reduced copy across the dialog's drag frames; source points that are not finite or lie past the image's filter margin are skipped; rows run through `extra::par` and poll the cancel; tests: the paste half of `paste_and_clone_in_perspective` (`paste_in_perspective`), and transform.rs's `rotation_keeps_the_colour_of_a_soft_edged_sprite` and `large_downscale_is_prefiltered` on `paste`.
//!
//! Effects ▸ Distort ▸ Vanishing Point: a perspective plane and pasting onto it.
//!
//! A plane is a quad the user aligns with a flat surface in the photo. Its two vanishing points
//! (where opposite edges meet) give the camera's focal length for a centred principal point,
//! `f² = −(v₁ − c)·(v₂ − c)` (R. Hartley, A. Zisserman, *Multiple View Geometry*, 2nd ed., §8.8:
//! orthogonal vanishing directions), which lifts the quad to 3-D: each corner is back-projected
//! onto the plane through the vanishing directions' normal. That gives the plane's metric aspect,
//! so a pasted image keeps its proportions. Pasting maps the image's rectangle into the plane's
//! metric coordinates and warps it with the composed homography, clipped to the plane.

use crate::canvas::ImageData;
use crate::extra::geom::Rect;
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::par;
use crate::resample::{self, Gamma, ResizeMode};

type V3 = [f64; 3];

fn sub(a: V3, b: V3) -> V3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn mul(a: V3, k: f64) -> V3 {
    [a[0] * k, a[1] * k, a[2] * k]
}
fn dot(a: V3, b: V3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: V3, b: V3) -> V3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn norm(a: V3) -> V3 {
    mul(a, 1.0 / dot(a, a).sqrt().max(1e-300))
}

/// Intersection of lines (a, b) and (c, d) in homogeneous coordinates.
fn meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> V3 {
    let l1 = cross([a[0], a[1], 1.0], [b[0], b[1], 1.0]);
    let l2 = cross([c[0], c[1], 1.0], [d[0], d[1], 1.0]);
    cross(l1, l2)
}

/// 3×3 projective matrix, row-major: `[x', y', w'] = H · [x, y, 1]`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography(pub [f64; 9]);

impl Homography {
    pub fn apply(&self, x: f64, y: f64) -> (f64, f64) {
        let m = &self.0;
        let w = m[6] * x + m[7] * y + m[8];
        (
            (m[0] * x + m[1] * y + m[2]) / w,
            (m[3] * x + m[4] * y + m[5]) / w,
        )
    }

    pub fn mul(&self, o: &Homography) -> Homography {
        let (a, b) = (&self.0, &o.0);
        let mut r = [0.0; 9];
        for i in 0..3 {
            for j in 0..3 {
                r[i * 3 + j] = (0..3).map(|k| a[i * 3 + k] * b[k * 3 + j]).sum();
            }
        }
        Homography(r)
    }

    pub fn inverse(&self) -> Option<Homography> {
        let m = &self.0;
        let c = [
            m[4] * m[8] - m[5] * m[7],
            m[2] * m[7] - m[1] * m[8],
            m[1] * m[5] - m[2] * m[4],
            m[5] * m[6] - m[3] * m[8],
            m[0] * m[8] - m[2] * m[6],
            m[2] * m[3] - m[0] * m[5],
            m[3] * m[7] - m[4] * m[6],
            m[1] * m[6] - m[0] * m[7],
            m[0] * m[4] - m[1] * m[3],
        ];
        let det = m[0] * c[0] + m[1] * c[3] + m[2] * c[6];
        if det.abs() < 1e-12 {
            return None;
        }
        Some(Homography(c.map(|v| v / det)))
    }

    /// Unit square → quad (corners in order: (0,0), (1,0), (1,1), (0,1)).
    fn square_to_quad(q: [[f64; 2]; 4]) -> Option<Homography> {
        let [[x0, y0], [x1, y1], [x2, y2], [x3, y3]] = q;
        let (sx, sy) = (x0 - x1 + x2 - x3, y0 - y1 + y2 - y3);
        if sx.abs() < 1e-12 && sy.abs() < 1e-12 {
            // Affine.
            return Some(Homography([
                x1 - x0,
                x3 - x0,
                x0,
                y1 - y0,
                y3 - y0,
                y0,
                0.0,
                0.0,
                1.0,
            ]));
        }
        let (dx1, dx2, dy1, dy2) = (x1 - x2, x3 - x2, y1 - y2, y3 - y2);
        let den = dx1 * dy2 - dx2 * dy1;
        if den.abs() < 1e-12 {
            return None;
        }
        let g = (sx * dy2 - dx2 * sy) / den;
        let h = (dx1 * sy - sx * dy1) / den;
        Some(Homography([
            x1 - x0 + g * x1,
            x3 - x0 + h * x3,
            x0,
            y1 - y0 + g * y1,
            y3 - y0 + h * y3,
            y0,
            g,
            h,
            1.0,
        ]))
    }

    /// The transform taking rectangle `r` (x0, y0, x1, y1) onto `quad` (corners clockwise from
    /// top-left). `None` for degenerate quads.
    pub fn rect_to_quad(r: [f64; 4], quad: [[f64; 2]; 4]) -> Option<Homography> {
        let (w, h) = (r[2] - r[0], r[3] - r[1]);
        if w <= 0.0 || h <= 0.0 {
            return None;
        }
        let to_unit = Homography([
            1.0 / w,
            0.0,
            -r[0] / w,
            0.0,
            1.0 / h,
            -r[1] / h,
            0.0,
            0.0,
            1.0,
        ]);
        Some(Self::square_to_quad(quad)?.mul(&to_unit))
    }

    /// Approximate linear scale of the mapping near `(x, y)` (sqrt of the Jacobian determinant).
    pub fn local_scale(&self, x: f64, y: f64) -> f64 {
        let e = 0.5;
        let (a, b) = (self.apply(x - e, y), self.apply(x + e, y));
        let (c, d) = (self.apply(x, y - e), self.apply(x, y + e));
        let (jx, jy) = ((b.0 - a.0, b.1 - a.1), (d.0 - c.0, d.1 - c.1));
        (jx.0 * jy.1 - jx.1 * jy.0).abs().sqrt()
    }
}

/// Photoshop's Bicubic: the Keys cubic at a = −0.75 (Catmull-Rom is −0.5), the weights of the
/// four source pixels around a sample `t` (0..1) past the second.
fn bicubic(t: f64) -> [f64; 4] {
    const A: f64 = -0.75;
    let near = |x: f64| (A + 2.0) * x * x * x - (A + 3.0) * x * x + 1.0;
    let far = |x: f64| A * (x * x * x - 5.0 * x * x + 8.0 * x - 4.0);
    [far(1.0 + t), near(t), near(1.0 - t), far(2.0 - t)]
}

/// A perspective plane: corners in image px, clockwise from the plane's top-left.
#[derive(Clone, Debug, PartialEq)]
pub struct VpPlane {
    pub corners: [[f64; 2]; 4],
}

/// A plane lifted to 3-D.
#[derive(Clone, Debug)]
pub struct Lifted {
    /// 3-D corners (camera coordinates, plane at unit distance along its normal).
    pub pts: [V3; 4],
}

impl VpPlane {
    /// Unit square (u, v) → image.
    pub fn homography(&self) -> Option<Homography> {
        Homography::rect_to_quad([0.0, 0.0, 1.0, 1.0], self.corners)
    }

    /// Focal length (px) implied by the plane's two vanishing points, if both are finite and
    /// consistent with a camera centred at `c`.
    pub fn focal(&self, c: [f64; 2]) -> Option<f64> {
        let q = &self.corners;
        let v1 = meet(q[0], q[1], q[3], q[2]);
        let v2 = meet(q[0], q[3], q[1], q[2]);
        if v1[2].abs() < 1e-9 || v2[2].abs() < 1e-9 {
            return None;
        }
        let (a, b) = (
            [v1[0] / v1[2] - c[0], v1[1] / v1[2] - c[1]],
            [v2[0] / v2[2] - c[0], v2[1] / v2[2] - c[1]],
        );
        let f2 = -(a[0] * b[0] + a[1] * b[1]);
        (f2 > 1.0).then(|| f2.sqrt())
    }

    /// Lifts the plane to 3-D with focal `f` (camera at the origin looking along +z).
    pub fn lift(&self, f: f64, c: [f64; 2]) -> Lifted {
        let ray = |p: [f64; 2]| [(p[0] - c[0]) / f, (p[1] - c[1]) / f, 1.0];
        let q = &self.corners;
        // Vanishing directions (or the edge directions when they're parallel in the image).
        let dir = |a: [f64; 2], b: [f64; 2], cc: [f64; 2], d: [f64; 2]| -> V3 {
            let v = meet(a, b, cc, d);
            if v[2].abs() < 1e-9 * (v[0].abs() + v[1].abs()).max(1.0) {
                norm([v[0], v[1], 0.0])
            } else {
                norm(ray([v[0] / v[2], v[1] / v[2]]))
            }
        };
        let d1 = dir(q[0], q[1], q[3], q[2]);
        let d2 = dir(q[0], q[3], q[1], q[2]);
        let mut n = norm(cross(d1, d2));
        if n[2] < 0.0 {
            n = mul(n, -1.0);
        }
        let pts = q.map(|p| {
            let r = ray(p);
            let lam = 1.0 / dot(n, r).abs().max(1e-9);
            mul(r, lam)
        });
        Lifted { pts }
    }

    /// Turn of each corner (`sin` of the angle between consecutive edges, positive when the
    /// corners run clockwise on screen); `None` when an edge is shorter than a pixel or a corner
    /// is not finite.
    fn turns(&self) -> Option<[f64; 4]> {
        let q = &self.corners;
        if q.iter().flatten().any(|v| !v.is_finite() || v.abs() > 1e9) {
            return None;
        }
        let e = |i: usize| [q[(i + 1) % 4][0] - q[i][0], q[(i + 1) % 4][1] - q[i][1]];
        let mut out = [0.0; 4];
        for (i, t) in out.iter_mut().enumerate() {
            let (a, b) = (e(i), e((i + 1) % 4));
            let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
            if la < 1.0 || lb < 1.0 {
                return None;
            }
            *t = (a[0] * b[1] - a[1] * b[0]) / (la * lb);
        }
        Some(out)
    }

    /// How usable the plane is, the way Photoshop colours its grid: [`PlaneCheck::Invalid`] for
    /// a quad that isn't convex (self-intersecting, a corner pushed inside, a near-straight or
    /// near-zero corner), [`PlaneCheck::Questionable`] for a convex quad whose perspective can't
    /// come from a camera centred at `center` (or with a corner sharper than 10°), else
    /// [`PlaneCheck::Valid`].
    pub fn check(&self, center: [f64; 2]) -> PlaneCheck {
        let Some(t) = self.turns() else {
            return PlaneCheck::Invalid;
        };
        let same_sign = t.iter().all(|v| *v > 0.0) || t.iter().all(|v| *v < 0.0);
        let min = t.iter().fold(f64::MAX, |m, v| m.min(v.abs()));
        if !same_sign || min < 1f64.to_radians().sin() || self.homography().is_none() {
            return PlaneCheck::Invalid;
        }
        if min < 10f64.to_radians().sin() {
            return PlaneCheck::Questionable;
        }
        // Both vanishing points finite but no real focal length fits them.
        let q = &self.corners;
        let (v1, v2) = (meet(q[0], q[1], q[3], q[2]), meet(q[0], q[3], q[1], q[2]));
        let finite = |v: V3| v[2].abs() > 1e-9 * (v[0].abs() + v[1].abs()).max(1.0);
        if finite(v1) && finite(v2) && self.focal(center).is_none() {
            return PlaneCheck::Questionable;
        }
        PlaneCheck::Valid
    }

    /// The same quad with its corners clockwise on screen, starting at the top-left one
    /// (the order [`paste`] expects), however they were clicked.
    pub fn normalized(&self) -> VpPlane {
        let mut q = self.corners;
        if self.turns().is_some_and(|t| t.iter().sum::<f64>() < 0.0) {
            q = [q[0], q[3], q[2], q[1]];
        }
        let first = (0..4)
            .min_by(|&a, &b| (q[a][0] + q[a][1]).total_cmp(&(q[b][0] + q[b][1])))
            .unwrap_or(0);
        VpPlane {
            corners: [
                q[first],
                q[(first + 1) % 4],
                q[(first + 2) % 4],
                q[(first + 3) % 4],
            ],
        }
    }
}

/// Plane validity ([`VpPlane::check`]): Photoshop draws a valid plane's grid blue, a doubtful
/// one yellow and an invalid one red.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlaneCheck {
    Valid,
    Questionable,
    Invalid,
}

impl Lifted {
    /// Metric side lengths (u along top edge, v along left edge).
    pub fn lengths(&self) -> (f64, f64) {
        let l = |a: V3, b: V3| dot(sub(a, b), sub(a, b)).sqrt();
        (
            (l(self.pts[0], self.pts[1]) + l(self.pts[3], self.pts[2])) / 2.0,
            (l(self.pts[0], self.pts[3]) + l(self.pts[1], self.pts[2])) / 2.0,
        )
    }
}

/// The camera of a `doc`-sized layer (upstream: the canvas's): its centre, and the focal length
/// a plane without two finite vanishing points falls back to, the longer side.
pub fn camera(doc: [u32; 2]) -> ([f64; 2], f64) {
    let (w, h) = (f64::from(doc[0]), f64::from(doc[1]));
    ([w / 2.0, h / 2.0], w.max(h))
}

/// `plane`'s unit square → image map and its metric side lengths under `camera(doc)` (upstream
/// `Scene::frame` for a lone plane); `None` for a red plane or a degenerate lift.
pub fn frame(plane: &VpPlane, doc: [u32; 2]) -> Option<(Homography, f64, f64)> {
    let (center, fallback) = camera(doc);
    if plane.check(center) == PlaneCheck::Invalid {
        return None;
    }
    let h = plane.homography()?;
    let f = plane.focal(center).unwrap_or(fallback);
    let (lu, lv) = plane.lift(f, center).lengths();
    (lu.is_finite() && lv.is_finite() && lu > 1e-12 && lv > 1e-12).then_some((h, lu, lv))
}

/// What Vanishing Point applies: the plane, and where the clipboard image sits on it.
#[derive(Clone, Debug, PartialEq)]
pub struct VpPaste {
    pub plane: VpPlane,
    /// The image's top-left in plane coordinates (0..1 spans the plane).
    pub at: [f64; 2],
    /// The image's width as a fraction of the plane's width.
    pub width: f64,
}

impl VpPaste {
    /// The size in plane coordinates of an `iw × ih` image on a plane of metric side lengths
    /// `lu × lv`: `width` across, and the image's aspect kept in the plane's metric.
    pub fn extent(&self, lu: f64, lv: f64, iw: u32, ih: u32) -> [f64; 2] {
        [
            self.width,
            self.width * lu / lv * f64::from(ih) / f64::from(iw.max(1)),
        ]
    }
}

/// Pastes `src` (straight RGBA8) onto `p.plane` of `dst` in perspective, over what is there
/// (upstream `paste`, then vp_cmds' `over`). `dst` is a `doc`-sized layer or a smaller proxy of
/// it, whose pixels then stand for `doc / dst` document pixels per axis. Only pixels whose centre
/// lies on the plane and inside `clip` (`dst` pixels) change. Each row polls `ctl`; a cancel
/// returns `Err` with the rows already done written.
pub fn paste(
    dst: &mut ImageData,
    doc: [u32; 2],
    clip: Rect,
    p: &VpPaste,
    src: &ImageData,
    ctl: Interrupt,
) -> Result<(), Cancelled> {
    paste_until(dst, doc, clip, p, src, &mut None, &|| ctl.cancelled())
}

/// [`paste`], keeping `src`'s prefiltered copy in `cache` for the next paste of the same `src`
/// (the dialog's drag frames): it is redone only when the size it needs changes. Clear `cache`
/// when `src` changes.
pub fn paste_cached(
    dst: &mut ImageData,
    doc: [u32; 2],
    clip: Rect,
    p: &VpPaste,
    src: &ImageData,
    cache: &mut Option<ImageData>,
    ctl: Interrupt,
) -> Result<(), Cancelled> {
    paste_until(dst, doc, clip, p, src, cache, &|| ctl.cancelled())
}

/// [`paste`], stopping once `stop` (polled per plane row, and at the end) says so.
fn paste_until(
    dst: &mut ImageData,
    doc: [u32; 2],
    clip: Rect,
    p: &VpPaste,
    src: &ImageData,
    cache: &mut Option<ImageData>,
    stop: &(dyn Fn() -> bool + Sync),
) -> Result<(), Cancelled> {
    let (w, h) = (dst.width as usize, dst.height as usize);
    if w == 0 || h == 0 || src.width == 0 || src.height == 0 || doc[0] == 0 || doc[1] == 0 {
        return Ok(());
    }
    let Some((hp, lu, lv)) = frame(&p.plane, doc) else {
        return Ok(());
    };
    let Some(inv) = hp.inverse() else {
        return Ok(());
    };
    // Destination pixels per document pixel.
    let k = [w as f64 / f64::from(doc[0]), h as f64 / f64::from(doc[1])];
    // Image px → plane (u, v): u spans `width`, v keeps the metric aspect.
    let (sw, sh) = (f64::from(src.width), f64::from(src.height));
    let su = p.width / sw;
    let sv = p.width * lu / lv / sw;
    if !(su.is_finite() && sv.is_finite() && su > 0.0 && sv > 0.0) {
        return Ok(());
    }
    // Large reductions are prefiltered (bicubic alone aliases below ~50 %): the image is brought
    // down by the map's scale at its centre, so the warp samples it at about 1:1 there.
    let to_plane = Homography([su, 0.0, p.at[0], 0.0, sv, p.at[1], 0.0, 0.0, 1.0]);
    let to_dst = Homography([k[0], 0.0, 0.0, 0.0, k[1], 0.0, 0.0, 0.0, 1.0]);
    let scale = to_dst
        .mul(&hp)
        .mul(&to_plane)
        .local_scale(sw / 2.0, sh / 2.0);
    let img = match prefilter_size(src, scale) {
        Some(size) => {
            if cache.as_ref().is_none_or(|c| [c.width, c.height] != size) {
                *cache = prefilter(src, size);
            }
            cache.as_ref().unwrap_or(src)
        }
        None => src,
    };
    let f = [f64::from(img.width) / sw, f64::from(img.height) / sh];
    // Destination px → (plane u, v) and the image point it samples (`img` px).
    let at = |x: f64, y: f64| {
        let (u, v) = inv.apply(x / k[0], y / k[1]);
        (u, v, (u - p.at[0]) / su * f[0], (v - p.at[1]) / sv * f[1])
    };
    let (xs, ys) = (
        p.plane.corners.map(|c| c[0] * k[0]),
        p.plane.corners.map(|c| c[1] * k[1]),
    );
    let lo = |v: [f64; 4]| v.iter().fold(f64::MAX, |m, x| m.min(*x)).floor() as i32;
    let hi = |v: [f64; 4]| v.iter().fold(f64::MIN, |m, x| m.max(*x)).ceil() as i32;
    let rows = Rect::new(lo(xs), lo(ys), hi(xs), hi(ys))
        .intersect(&clip)
        .intersect(&Rect::new(0, 0, w as i32, h as i32));
    if rows.is_empty() {
        return Ok(());
    }
    let (iw, ih) = (f64::from(img.width), f64::from(img.height));
    par::rows_mut(&mut dst.data, w * 4, |y, row| {
        let yi = y as i32;
        if yi < rows.y0 || yi >= rows.y1 || stop() {
            return;
        }
        let cy = y as f64 + 0.5;
        for x in rows.x0..rows.x1 {
            let cx = f64::from(x) + 0.5;
            let (pu, pv, sx, sy) = at(cx, cy);
            // Upstream's `quad_mask`: only pixels whose centre lies on the plane.
            if !((0.0..=1.0).contains(&pu) && (0.0..=1.0).contains(&pv)) {
                continue;
            }
            let (u, v) = (sx - 0.5, sy - 0.5);
            // Past the image's 1-px filter margin, or not finite.
            if !(u >= -1.0 && v >= -1.0 && u <= iw && v <= ih) {
                continue;
            }
            // This pixel's extent in image pixels, for the edge coverage.
            let ((_, _, ax, ay), (_, _, bx, by)) = (at(cx + 1.0, cy), at(cx, cy + 1.0));
            let extent = |e: f64| if e.is_finite() && e > 1e-12 { e } else { 1.0 };
            let foot = [
                extent((ax - sx).abs() + (bx - sx).abs()),
                extent((ay - sy).abs() + (by - sy).abs()),
            ];
            if let Some(top) = sample(img, u, v, foot) {
                over(&mut row[x as usize * 4..][..4], top);
            }
        }
    });
    if stop() { Err(Cancelled) } else { Ok(()) }
}

/// The size the prefilter brings `src` to for a map of local `scale`: reduced by `scale` (at most
/// 64×, as upstream), the factor rounded up to a step of 2^(1/8) so that nearby scales share one
/// copy. `None` when that is no reduction.
fn prefilter_size(src: &ImageData, scale: f64) -> Option<[u32; 2]> {
    if !(scale > 0.0 && scale.is_finite()) {
        return None;
    }
    let f = ((scale.log2() * 8.0 - 1e-6).ceil() / 8.0)
        .exp2()
        .max(1.0 / 64.0);
    if f >= 0.999 {
        return None;
    }
    let size = |n: u32| ((f64::from(n) * f).round() as u32).max(1);
    Some([size(src.width), size(src.height)])
}

/// `src` resized to `[w, h]` with paint.rs's Bicubic; `None` if the resize refuses the size.
fn prefilter(src: &ImageData, [w, h]: [u32; 2]) -> Option<ImageData> {
    let data = resample::resize(
        &src.data,
        src.width,
        src.height,
        w,
        h,
        ResizeMode::Bicubic,
        Gamma::Companded,
    );
    (data.len() == w as usize * h as usize * 4).then_some(ImageData {
        width: w,
        height: h,
        data,
    })
}

/// Upstream `warp`'s bicubic sample of `img` at `(u, v)` (pixel-index coordinates): Keys taps on
/// premultiplied pixels, clamped to the image (an edge row or column stretches), scaled by how
/// much of the destination pixel (`foot` image pixels across) the image covers. The colour is
/// divided by the unclamped alpha, so an overshoot does not lighten it (#6203), then clipped to
/// 0–1 as 8-bit storage clips it; the alpha is clamped. `None` where nothing covers the pixel.
fn sample(img: &ImageData, u: f64, v: f64, foot: [f64; 2]) -> Option<[f64; 4]> {
    let (w, h) = (img.width as i32, img.height as i32);
    let (ix, iy) = (u.floor() as i32, v.floor() as i32);
    let (wx, wy) = (bicubic(u - f64::from(ix)), bicubic(v - f64::from(iy)));
    let mut acc = [0.0f64; 4];
    for (j, wyj) in wy.iter().enumerate() {
        let yy = (iy - 1 + j as i32).clamp(0, h - 1) as usize;
        for (i, wxi) in wx.iter().enumerate() {
            let xx = (ix - 1 + i as i32).clamp(0, w - 1) as usize;
            let q = &img.data[(yy * img.width as usize + xx) * 4..][..4];
            let a = f64::from(q[3]) / 255.0;
            let k = wxi * wyj;
            for c in 0..3 {
                acc[c] += f64::from(q[c]) / 255.0 * a * k;
            }
            acc[3] += a * k;
        }
    }
    let (uc, vc) = (u + 0.5, v + 0.5);
    let cu = (uc.min(f64::from(w) - uc) / foot[0] + 0.5).clamp(0.0, 1.0);
    let cv = (vc.min(f64::from(h) - vc) / foot[1] + 0.5).clamp(0.0, 1.0);
    let acc = acc.map(|s| s * cu * cv);
    let al = acc[3].clamp(0.0, 1.0);
    (al > 0.0).then(|| {
        let c = |i: usize| (acc[i] / acc[3]).clamp(0.0, 1.0);
        [c(0), c(1), c(2), al]
    })
}

/// `top` (straight colour and alpha, 0–1, alpha above 0) over the straight RGBA8 pixel `d`
/// (vp_cmds' `over`).
fn over(d: &mut [u8], top: [f64; 4]) {
    let ta = top[3];
    let da = f64::from(d[3]) / 255.0;
    let oa = ta + da * (1.0 - ta);
    for c in 0..3 {
        let v = (top[c] * ta + f64::from(d[c]) / 255.0 * da * (1.0 - ta)) / oa;
        d[c] = (v * 255.0 + 0.5) as u8;
    }
    d[3] = (oa * 255.0 + 0.5) as u8;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, solid};
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn add(a: V3, b: V3) -> V3 {
        [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
    }

    /// A ground plane seen by a camera with f = 500 px: projects 3-D points.
    fn project(f: f64, c: [f64; 2], p: V3) -> [f64; 2] {
        [c[0] + f * p[0] / p[2], c[1] + f * p[1] / p[2]]
    }

    fn floor_quad(f: f64, c: [f64; 2]) -> VpPlane {
        // A 2 × 1 rectangle on a tilted plane, rotated about the vertical axis too.
        let (rx, ry) = (0.6f64, 0.4f64);
        let rot = |p: V3| {
            let (s, co) = rx.sin_cos();
            let q = [p[0], co * p[1] - s * p[2], s * p[1] + co * p[2]];
            let (s2, c2) = ry.sin_cos();
            [c2 * q[0] + s2 * q[2], q[1], -s2 * q[0] + c2 * q[2]]
        };
        let corners = [
            [-1.0, -0.5, 0.0],
            [1.0, -0.5, 0.0],
            [1.0, 0.5, 0.0],
            [-1.0, 0.5, 0.0],
        ]
        .map(|p: V3| project(f, c, add(rot(p), [0.0, 0.0, 4.0])));
        VpPlane { corners }
    }

    #[test]
    fn focal_and_metric_aspect_from_one_plane() {
        let c = [400.0, 300.0];
        let pl = floor_quad(500.0, c);
        let f = pl.focal(c).unwrap();
        assert!((f - 500.0).abs() < 1.0, "{f}");
        let (lu, lv) = pl.lift(f, c).lengths();
        assert!((lu / lv - 2.0).abs() < 0.01, "aspect {}", lu / lv);
    }

    #[test]
    fn plane_check_and_corner_order() {
        let c = [400.0, 300.0];
        assert_eq!(floor_quad(500.0, c).check(c), PlaneCheck::Valid);
        let square = VpPlane {
            corners: [[10.0, 10.0], [110.0, 10.0], [110.0, 110.0], [10.0, 110.0]],
        };
        assert_eq!(square.check(c), PlaneCheck::Valid, "fronto-parallel");
        let bowtie = VpPlane {
            corners: [[10.0, 10.0], [110.0, 110.0], [110.0, 10.0], [10.0, 110.0]],
        };
        assert_eq!(bowtie.check(c), PlaneCheck::Invalid);
        let dent = VpPlane {
            corners: [[10.0, 10.0], [110.0, 10.0], [40.0, 40.0], [10.0, 110.0]],
        };
        assert_eq!(dent.check(c), PlaneCheck::Invalid, "concave");
        let sliver = VpPlane {
            corners: [[10.0, 10.0], [300.0, 10.0], [300.0, 40.0], [200.0, 40.0]],
        };
        assert_eq!(
            sliver.check(c),
            PlaneCheck::Questionable,
            "a corner sharper than 10°"
        );
        let nan = VpPlane {
            corners: [
                [f64::NAN, 10.0],
                [110.0, 10.0],
                [110.0, 110.0],
                [10.0, 110.0],
            ],
        };
        assert_eq!(nan.check(c), PlaneCheck::Invalid);
        // Clicked counter-clockwise from the bottom-right: clockwise from the top-left.
        let ccw = VpPlane {
            corners: [[110.0, 110.0], [110.0, 10.0], [10.0, 10.0], [10.0, 110.0]],
        };
        assert_eq!(ccw.normalized(), square);
    }

    /// Four points clicked in any order make the plane whose unit square's corners (0, 0),
    /// (1, 0), (1, 1) and (0, 1) land on them, clockwise from the top-left.
    #[test]
    fn plane_from_four_points_maps_corners() {
        let (tl, tr, br, bl) = ([70.0, 60.0], [170.0, 60.0], [220.0, 150.0], [20.0, 150.0]);
        for clicked in [[bl, br, tr, tl], [tr, tl, bl, br], [tl, tr, br, bl]] {
            let p = VpPlane { corners: clicked }.normalized();
            assert_eq!(p.corners, [tl, tr, br, bl], "{clicked:?}");
            let h = p.homography().unwrap();
            for (uv, want) in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
                .into_iter()
                .zip([tl, tr, br, bl])
            {
                let (x, y) = h.apply(uv[0], uv[1]);
                assert!(
                    (x - want[0]).abs() < 1e-9 && (y - want[1]).abs() < 1e-9,
                    "{uv:?} → ({x}, {y}), not {want:?}"
                );
            }
        }
    }

    /// The paste half of upstream's `paste_and_clone_in_perspective`: the pasted rectangle lands
    /// inside the plane, around plane (0.5, 0.5), and not at its corner.
    #[test]
    fn paste_in_perspective() {
        let c = [400.0, 300.0];
        let plane = floor_quad(500.0, c);
        let p = VpPaste {
            plane: plane.clone(),
            at: [0.25, 0.25],
            width: 0.5,
        };
        let img = solid(100, 50, [255, 0, 0, 255]);
        let mut out = solid(800, 600, [0, 0, 0, 0]);
        let all = Rect::new(0, 0, 800, 600);
        paste(&mut out, [800, 600], all, &p, &img, Interrupt::NONE).unwrap();
        let h = plane.homography().unwrap();
        let (x, y) = h.apply(0.5, 0.5);
        assert!(px(&out, x as u32, y as u32)[3] > 230, "{x}, {y}");
        let (x, y) = h.apply(0.1, 0.1);
        assert!(px(&out, x as u32, y as u32)[3] < 26, "{x}, {y}");
    }

    /// transform.rs's #6203 test on `paste`: a turned sprite whose edge pixels are half
    /// transparent overshoots its alpha past 1 under bicubic; the colour stays the sprite's
    /// instead of lightening into a border.
    #[test]
    fn rotation_keeps_the_colour_of_a_soft_edged_sprite() {
        let colour = [204, 153, 102];
        let img = from_fn(40, 40, |x, y| {
            let inside = |a: u32, b: u32| (a..b).contains(&x) && (a..b).contains(&y);
            match (inside(11, 29), inside(10, 30)) {
                (true, _) => [colour[0], colour[1], colour[2], 255],
                (_, true) => [colour[0], colour[1], colour[2], 128],
                _ => [0, 0, 0, 0],
            }
        });
        // The image fills a square plane turned 45° (a fronto-parallel diamond) at 1:1.
        let (c, k) = (35.0, 20.0 * std::f64::consts::SQRT_2);
        let plane = VpPlane {
            corners: [[c, c - k], [c + k, c], [c, c + k], [c - k, c]],
        };
        let p = VpPaste {
            plane,
            at: [0.0, 0.0],
            width: 1.0,
        };
        let mut out = solid(70, 70, [0, 0, 0, 0]);
        paste(
            &mut out,
            [70, 70],
            Rect::new(0, 0, 70, 70),
            &p,
            &img,
            Interrupt::NONE,
        )
        .unwrap();
        let mut seen = 0;
        for (x, y) in (0..70).flat_map(|y| (0..70).map(move |x| (x, y))) {
            let q = px(&out, x, y);
            if q[3] > 0 {
                seen += 1;
                for i in 0..3 {
                    assert!(q[i].abs_diff(colour[i]) <= 1, "({x}, {y}): {q:?}");
                }
            }
        }
        assert!(seen > 300, "{seen}");
    }

    /// transform.rs's `large_downscale_is_prefiltered` on `paste`: a 1-px checkerboard shrunk 8×
    /// comes out about 50 % grey, not aliased black or white.
    #[test]
    fn large_downscale_is_prefiltered() {
        let img = from_fn(64, 64, |x, y| {
            let v = if (x + y) % 2 == 1 { 255 } else { 0 };
            [v, v, v, 255]
        });
        let p = VpPaste {
            plane: VpPlane {
                corners: [[0.0, 0.0], [8.0, 0.0], [8.0, 8.0], [0.0, 8.0]],
            },
            at: [0.0, 0.0],
            width: 1.0,
        };
        let mut out = solid(8, 8, [0, 0, 0, 0]);
        paste(
            &mut out,
            [8, 8],
            Rect::new(0, 0, 8, 8),
            &p,
            &img,
            Interrupt::NONE,
        )
        .unwrap();
        let q = px(&out, 4, 4);
        assert!((f64::from(q[0]) / 255.0 - 0.5).abs() < 0.1, "{q:?}");
        assert_eq!(q[3], 255);
    }

    /// A 48×40 noise layer, a plane and a red image placed to cover all of it.
    fn noise_and_plane() -> (ImageData, VpPaste) {
        let noise = from_fn(48, 40, |x, y| {
            let mut s = u64::from(y * 48 + x);
            let b = crate::extra::rng::splitmix64(&mut s).to_le_bytes();
            [b[0], b[1], b[2], b[3]]
        });
        let p = VpPaste {
            plane: VpPlane {
                corners: [[10.0, 6.0], [40.0, 8.0], [44.0, 34.0], [6.0, 30.0]],
            },
            at: [-0.5, -0.5],
            width: 3.0,
        };
        (noise, p)
    }

    /// Only pixels whose centre lies on the plane and inside the clip change, to the image
    /// composited over them; every other pixel keeps its bytes.
    #[test]
    fn paste_keeps_to_the_plane_and_the_clip() {
        let (noise, p) = noise_and_plane();
        let red = solid(16, 16, [255, 0, 0, 255]);
        let clip = Rect::new(0, 0, 30, 40);
        let mut out = noise.clone();
        paste(&mut out, [48, 40], clip, &p, &red, Interrupt::NONE).unwrap();
        let inv = p.plane.homography().unwrap().inverse().unwrap();
        let mut on = 0;
        for (x, y) in (0..40).flat_map(|y| (0..48).map(move |x| (x, y))) {
            let (u, v) = inv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let inside = (0.0..=1.0).contains(&u)
                && (0.0..=1.0).contains(&v)
                && clip.contains(x as i32, y as i32);
            if inside {
                on += 1;
                assert_eq!(px(&out, x, y), [255, 0, 0, 255], "({x}, {y})");
            } else {
                assert_eq!(px(&out, x, y), px(&noise, x, y), "({x}, {y})");
            }
        }
        assert!(on > 200, "{on}");
    }

    /// Hostile values paste nothing and never panic: a corner that is not finite or past 1e9, a
    /// bow-tie, a placement that is not finite or far away, a vanishing width.
    #[test]
    fn hostile_values_paste_nothing() {
        let (noise, good) = noise_and_plane();
        let red = solid(16, 16, [255, 0, 0, 255]);
        let mut cases = vec![];
        for bad in [f64::NAN, f64::INFINITY, 2e9] {
            let mut p = good.clone();
            p.plane.corners[2][0] = bad;
            cases.push(p);
        }
        let mut bowtie = good.clone();
        bowtie.plane.corners.swap(1, 2);
        cases.push(bowtie);
        for at in [[f64::NAN, 0.0], [1e300, -1e300]] {
            cases.push(VpPaste { at, ..good.clone() });
        }
        for width in [1e-300, f64::NAN, 0.0] {
            cases.push(VpPaste {
                width,
                ..good.clone()
            });
        }
        for p in cases {
            let mut out = noise.clone();
            let all = Rect::new(0, 0, 48, 40);
            paste(&mut out, [48, 40], all, &p, &red, Interrupt::NONE).unwrap();
            assert_eq!(out.data, noise.data, "{p:?}");
        }
    }

    /// Each plane row polls the stop: one that turns true on its fifth poll leaves exactly four
    /// rows pasted and reports `Err`.
    #[test]
    fn a_cancel_stops_partway_through_the_rows() {
        let p = VpPaste {
            plane: VpPlane {
                corners: [[4.0, 4.0], [36.0, 4.0], [36.0, 36.0], [4.0, 36.0]],
            },
            at: [0.0, 0.0],
            width: 1.0,
        };
        let red = solid(32, 32, [255, 0, 0, 255]);
        let before = solid(40, 40, [0, 0, 0, 255]);
        let polls = AtomicUsize::new(0);
        let stop = || polls.fetch_add(1, Ordering::Relaxed) >= 4;
        let mut out = before.clone();
        let all = Rect::new(0, 0, 40, 40);
        assert_eq!(
            paste_until(&mut out, [40, 40], all, &p, &red, &mut None, &stop),
            Err(Cancelled)
        );
        let rows = (0..40)
            .filter(|&y| (0..40).any(|x| px(&out, x, y) != px(&before, x, y)))
            .count();
        assert_eq!(rows, 4);
        // Uncancelled, all 32 rows of the plane are pasted.
        let mut whole = before.clone();
        paste(&mut whole, [40, 40], all, &p, &red, Interrupt::NONE).unwrap();
        let rows = (0..40)
            .filter(|&y| (0..40).any(|x| px(&whole, x, y) != px(&before, x, y)))
            .count();
        assert_eq!(rows, 32);
    }

    /// `paste_cached` reuses the prefiltered copy while the size it needs stays the same (the
    /// image moved along a fronto-parallel plane) and redoes it when that changes (a wider
    /// paste). A marker painted into the cached copy shows which one was sampled.
    #[test]
    fn the_prefilter_cache_is_reused_until_its_size_changes() {
        let img = from_fn(64, 64, |x, y| {
            let v = if (x + y) % 2 == 1 { 255 } else { 0 };
            [v, v, v, 255]
        });
        let plane = VpPlane {
            corners: [[0.0, 0.0], [16.0, 0.0], [16.0, 16.0], [0.0, 16.0]],
        };
        let at = |at: [f64; 2], width: f64| VpPaste {
            plane: plane.clone(),
            at,
            width,
        };
        let all = Rect::new(0, 0, 32, 32);
        let run = |p: &VpPaste, cache: &mut Option<ImageData>| {
            let mut out = solid(32, 32, [0, 0, 0, 0]);
            paste_cached(&mut out, [32, 32], all, p, &img, cache, Interrupt::NONE).unwrap();
            out
        };
        let mut cache = None;
        let first = run(&at([0.0, 0.0], 0.5), &mut cache);
        let mut fresh = solid(32, 32, [0, 0, 0, 0]);
        paste(
            &mut fresh,
            [32, 32],
            all,
            &at([0.0, 0.0], 0.5),
            &img,
            Interrupt::NONE,
        )
        .unwrap();
        assert_eq!(first, fresh, "the cached paste is the plain one");
        let copy = cache.as_mut().unwrap();
        assert_eq!((copy.width, copy.height), (8, 8), "64 px onto 8 px");
        copy.data = solid(8, 8, [0, 255, 0, 255]).data;
        let moved = run(&at([0.25, 0.25], 0.5), &mut cache);
        assert_eq!(
            px(&moved, 8, 8),
            [0, 255, 0, 255],
            "the cached copy, reused"
        );
        let wider = run(&at([0.0, 0.0], 1.0), &mut cache);
        assert_eq!(cache.as_ref().map(|c| (c.width, c.height)), Some((16, 16)));
        assert_ne!(px(&wider, 8, 8), [0, 255, 0, 255], "redone at the new size");
    }

    /// A proxy half the layer's size samples the same plane at its own pixel centres: a pixel
    /// in the middle of the plane takes the image's colour there.
    #[test]
    fn a_proxy_maps_its_pixels_to_the_document() {
        let (_, p) = noise_and_plane();
        let red = solid(16, 16, [255, 0, 0, 255]);
        let mut proxy = solid(24, 20, [0, 0, 255, 255]);
        let all = Rect::new(0, 0, 24, 20);
        paste(&mut proxy, [48, 40], all, &p, &red, Interrupt::NONE).unwrap();
        // Proxy (12, 10) is document (25, 21), on the plane; proxy (1, 1) is (3, 3), off it.
        assert_eq!(px(&proxy, 12, 10), [255, 0, 0, 255]);
        assert_eq!(px(&proxy, 1, 1), [0, 0, 255, 255]);
    }
}
