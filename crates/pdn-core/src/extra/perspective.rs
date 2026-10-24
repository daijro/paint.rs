//! Ported from PhotoCraft crates/algo/src/perspective.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT), with `Homography` (`apply`, `w`, `mul`, `inverse`, `square_to_quad`, `rect_to_quad`) and `convex_quad` from crates/algo/src/transform.rs, and `keep_convex` (with `turns`) from crates/ui-egui/src/transform_tool.rs for Move Selected Pixels' Resize / Warp (option key `distort`, R230). Changes: `map` gives NaN for a point a plane's homography sends past its horizon (upstream returned the point itself), so the mesh warp drops that triangle instead of folding it back over the image; `perspective_warp` (upstream's engine `perspective_surface`) reads the whole layer, takes a scale for the downsampled preview and polls the cancel; `straighten` and `Straighten` serve the canvas mode's Straighten (`tool::perspective_warp`) without `Straighten::parse`; no `plane_point` or `unit_corners` (the canvas mode draws the plane's outline only); `crop_size` and `rectify` (Crop to plane) are not upstream.
//!
//! Layers ▸ Perspective Warp: planes (quads) over the image whose corners are moved; the image
//! follows a piecewise-homography map.
//!
//! Each plane maps its source quad onto its destination quad with a homography
//! `M_i = H_dst ∘ H_src⁻¹` (through the unit square, so `(u, v)` are the plane's projective
//! coordinates). Two homographies that agree on an edge's endpoints map the edge onto the same
//! segment but generally with a different spacing along it, so on its own a piecewise map tears
//! at shared edges. For every edge shared by two planes we therefore take the average of both
//! planes' images of each edge point as the target and add the difference, faded linearly
//! across the plane (weight `1 − u`, `u`, `1 − v` or `v` from that edge to the opposite one).
//! The correction vanishes at the corners (shared corners have one destination), so the map is
//! exactly continuous across shared edges, exactly the homography for a lone plane, and exactly
//! the identity when nothing moved. Points outside every plane use the nearest plane, with
//! `(u, v)` clamped for the corrections.
//!
//! Corners that coincide (within half a pixel) in the source are linked: they share one
//! destination (the mean of the given ones), as Photoshop's snapped planes do.

use serde::{Deserialize, Serialize};

use super::geom::Rect;
use super::interrupt::{Cancelled, Interrupt};
use super::warp::{Interp, warp_mesh};
use crate::canvas::ImageData;

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

    /// The homogeneous `w` of `(x, y)`: its sign is the side of the horizon the point maps to.
    pub fn w(&self, x: f64, y: f64) -> f64 {
        let m = &self.0;
        m[6] * x + m[7] * y + m[8]
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
}

/// Is `q` (TL, TR, BR, BL) finite, strictly convex with every corner angle above ~0.06°, wound
/// clockwise on screen (y down) and at least 1 px² in area? The dialog keeps the plane to such
/// quads, whose horizon lies outside them.
pub fn convex_quad(q: &[[f64; 2]; 4]) -> bool {
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return false;
    }
    let mut area2 = 0.0;
    for i in 0..4 {
        let (a, b, c) = (q[i], q[(i + 1) % 4], q[(i + 2) % 4]);
        let (e1, e2) = ([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]);
        let cross = e1[0] * e2[1] - e1[1] * e2[0];
        // sin(turn) = cross / (|e1| |e2|): positive (clockwise on screen) and not a straight angle.
        if cross.is_nan() || cross <= 1e-3 * e1[0].hypot(e1[1]) * e2[0].hypot(e2[1]) {
            return false;
        }
        area2 += a[0] * b[1] - b[0] * a[1];
    }
    area2.is_finite() && area2 >= 2.0
}

/// The turn at each corner of a quad (cross products of consecutive edges): all the same sign
/// for a convex quad, mixed for a concave or self-intersecting one.
fn turns(q: &[[f64; 2]; 4]) -> [f64; 4] {
    std::array::from_fn(|k| {
        let (a, b, c) = (q[k], q[(k + 1) % 4], q[(k + 2) % 4]);
        (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
    })
}

/// A corner drag from quad `from` towards `to` stops where the quad would turn concave or fold
/// over itself, as Photoshop's Distort does: as far towards `to` as it stays convex with
/// `from`'s winding. A `from` that is not convex is left free.
pub fn keep_convex(from: [[f64; 2]; 4], to: [[f64; 2]; 4]) -> [[f64; 2]; 4] {
    let convex =
        |q: &[[f64; 2]; 4], sign: f64, min_turn: f64| turns(q).iter().all(|t| t * sign > min_turn);
    let t0 = turns(&from);
    let sign = t0[0].signum();
    // A sliver: a thousandth of the starting quad's smallest turn, so the stop is short of flat.
    let min_turn = t0.iter().map(|t| t.abs()).fold(f64::INFINITY, f64::min) * 1e-3;
    if !(min_turn.is_finite() && min_turn > 0.0 && convex(&from, sign, 0.0))
        || convex(&to, sign, min_turn)
    {
        return to;
    }
    let lerp = |t: f64| -> [[f64; 2]; 4] {
        std::array::from_fn(|k| {
            [
                from[k][0] + (to[k][0] - from[k][0]) * t,
                from[k][1] + (to[k][1] - from[k][1]) * t,
            ]
        })
    };
    let (mut lo, mut hi) = (0.0f64, 1.0f64);
    for _ in 0..40 {
        let mid = (lo + hi) / 2.0;
        if convex(&lerp(mid), sign, min_turn) {
            lo = mid
        } else {
            hi = mid
        }
    }
    lerp(lo)
}

/// One plane: source quad and destination quad, corners clockwise from top-left, layer pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Plane {
    pub src: [[f64; 2]; 4],
    pub dst: [[f64; 2]; 4],
}

impl Plane {
    pub fn identity(q: [[f64; 2]; 4]) -> Self {
        Plane { src: q, dst: q }
    }
}

const LINK_EPS: f64 = 0.5;

fn unit_to_quad(q: &[[f64; 2]; 4]) -> Option<Homography> {
    Homography::rect_to_quad([0.0, 0.0, 1.0, 1.0], *q)
}

/// Groups of linked corners: `(plane, corner)` pairs that coincide in the source.
pub fn linked_corners(planes: &[Plane]) -> Vec<Vec<(usize, usize)>> {
    let mut groups: Vec<Vec<(usize, usize)>> = Vec::new();
    for (pi, p) in planes.iter().enumerate() {
        for (ci, c) in p.src.iter().enumerate() {
            let g = groups.iter_mut().find(|g| {
                let (a, b) = g[0];
                let o = planes[a].src[b];
                (o[0] - c[0]).abs() <= LINK_EPS && (o[1] - c[1]).abs() <= LINK_EPS
            });
            match g {
                Some(g) => g.push((pi, ci)),
                None => groups.push(vec![(pi, ci)]),
            }
        }
    }
    groups
}

/// Snaps linked corners to one source and one destination position (their means).
pub fn unify(planes: &mut [Plane]) {
    for g in linked_corners(planes) {
        if g.len() < 2 {
            continue;
        }
        let n = g.len() as f64;
        let mean = |f: &dyn Fn(&Plane) -> [[f64; 2]; 4]| {
            g.iter().fold([0.0, 0.0], |a, &(p, c)| {
                let q = f(&planes[p])[c];
                [a[0] + q[0] / n, a[1] + q[1] / n]
            })
        };
        let (s, d) = (mean(&|p| p.src), mean(&|p| p.dst));
        for &(p, c) in &g {
            planes[p].src[c] = s;
            planes[p].dst[c] = d;
        }
    }
}

/// Straighten modes (Photoshop's Warp-mode buttons).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Straighten {
    Horizontal,
    Vertical,
    Auto,
}

/// Makes the destination edges that are near-horizontal exactly horizontal and/or near-vertical
/// exactly vertical (within 45°), moving linked corners together.
pub fn straighten(planes: &mut [Plane], mode: Straighten) {
    unify(planes);
    let groups = linked_corners(planes);
    let group_of =
        |p: usize, c: usize| groups.iter().position(|g| g.contains(&(p, c))).unwrap_or(0);
    let mut pos: Vec<[f64; 2]> = groups.iter().map(|g| planes[g[0].0].dst[g[0].1]).collect();
    let edges: Vec<(usize, usize)> = (0..planes.len())
        .flat_map(|p| (0..4).map(move |k| (p, k)))
        .map(|(p, k)| (group_of(p, k), group_of(p, (k + 1) % 4)))
        .collect();
    for (a, b) in edges {
        let (pa, pb) = (pos[a], pos[b]);
        let (dx, dy) = ((pb[0] - pa[0]).abs(), (pb[1] - pa[1]).abs());
        if dy <= dx && matches!(mode, Straighten::Horizontal | Straighten::Auto) {
            let y = (pa[1] + pb[1]) / 2.0;
            pos[a][1] = y;
            pos[b][1] = y;
        } else if dx < dy && matches!(mode, Straighten::Vertical | Straighten::Auto) {
            let x = (pa[0] + pb[0]) / 2.0;
            pos[a][0] = x;
            pos[b][0] = x;
        }
    }
    for (g, p) in groups.iter().zip(pos) {
        for &(pi, ci) in g {
            planes[pi].dst[ci] = p;
        }
    }
}

/// Not upstream (Crop to plane): the upright rectangle a quad (TL, TR, BR, BL) flattens to, as
/// perspective-crop tools size it: the mean lengths of its opposite edges, rounded, at least 1.
pub fn crop_size(q: &[[f64; 2]; 4]) -> (u32, u32) {
    let len = |a: usize, b: usize| (q[a][0] - q[b][0]).hypot(q[a][1] - q[b][1]);
    let side = |l: f64| l.round().clamp(1.0, f64::from(u32::MAX)) as u32;
    (
        side((len(0, 1) + len(3, 2)) / 2.0),
        side((len(0, 3) + len(1, 2)) / 2.0),
    )
}

/// Not upstream (Crop to plane): `src`'s quad `q` flattened onto a `w`×`h` image, its corners on
/// the image's corners, through [`perspective_warp`] (padded so the result fits).
pub fn rectify(src: &ImageData, q: [[f64; 2]; 4], w: u32, h: u32) -> ImageData {
    let (pw, ph) = (w.max(src.width), h.max(src.height));
    let mut padded = ImageData {
        width: pw,
        height: ph,
        data: vec![0; pw as usize * ph as usize * 4],
    };
    let row = src.width as usize * 4;
    for y in 0..src.height as usize {
        let at = y * pw as usize * 4;
        padded.data[at..at + row].copy_from_slice(&src.data[y * row..(y + 1) * row]);
    }
    let (fw, fh) = (f64::from(w), f64::from(h));
    let plane = Plane {
        src: q,
        dst: [[0.0, 0.0], [fw, 0.0], [fw, fh], [0.0, fh]],
    };
    let clip = Rect::new(0, 0, w as i32, h as i32);
    let none = Interrupt::NONE;
    let out =
        perspective_warp(&padded, &[plane], 1.0, clip, Interp::Bicubic, none).unwrap_or(padded);
    let mut data = Vec::with_capacity(w as usize * h as usize * 4);
    for y in 0..h as usize {
        let at = y * pw as usize * 4;
        data.extend_from_slice(&out.data[at..at + w as usize * 4]);
    }
    ImageData {
        width: w,
        height: h,
        data,
    }
}

/// Edge `k` of a plane runs from corner `k` to corner `k + 1`.
fn edge_uv(k: usize, u: f64, v: f64) -> ([f64; 2], f64) {
    // (point on the edge in plane coordinates, weight of that edge's correction)
    match k {
        0 => ([u, 0.0], 1.0 - v),
        1 => ([1.0, v], u),
        2 => ([u, 1.0], v),
        _ => ([0.0, v], 1.0 - u),
    }
}

struct PlaneMap {
    src: Homography,
    src_inv: Homography,
    dst: Homography,
    quad: [[f64; 2]; 4],
    /// For each edge: the neighbouring plane sharing it, if any.
    shared: [Option<usize>; 4],
}

/// A prepared perspective warp (forward map, source → destination).
pub struct PerspectiveMap {
    planes: Vec<PlaneMap>,
}

impl PerspectiveMap {
    /// Prepares the map. `None` if a quad is degenerate (or there are no planes).
    pub fn new(planes: &[Plane]) -> Option<Self> {
        if planes.is_empty() {
            return None;
        }
        let mut planes = planes.to_vec();
        unify(&mut planes);
        let near = |a: [f64; 2], b: [f64; 2]| {
            (a[0] - b[0]).abs() <= LINK_EPS && (a[1] - b[1]).abs() <= LINK_EPS
        };
        let mut maps = Vec::with_capacity(planes.len());
        for (i, p) in planes.iter().enumerate() {
            let src = unit_to_quad(&p.src)?;
            let src_inv = src.inverse()?;
            let dst = unit_to_quad(&p.dst)?;
            let mut shared = [None; 4];
            for (k, s) in shared.iter_mut().enumerate() {
                let (a, b) = (p.src[k], p.src[(k + 1) % 4]);
                *s = planes
                    .iter()
                    .enumerate()
                    .find(|(j, o)| {
                        *j != i
                            && (0..4).any(|m| {
                                let (c, d) = (o.src[m], o.src[(m + 1) % 4]);
                                (near(a, c) && near(b, d)) || (near(a, d) && near(b, c))
                            })
                    })
                    .map(|(j, _)| j);
            }
            maps.push(PlaneMap {
                src,
                src_inv,
                dst,
                quad: p.src,
                shared,
            });
        }
        Some(PerspectiveMap { planes: maps })
    }

    /// The plane containing `p` (or the nearest one) and `p`'s plane coordinates.
    fn locate(&self, x: f64, y: f64) -> (usize, f64, f64) {
        let mut best = (0, f64::MAX, 0.0, 0.0);
        for (i, pm) in self.planes.iter().enumerate() {
            let (u, v) = pm.src_inv.apply(x, y);
            if (-1e-9..=1.0 + 1e-9).contains(&u) && (-1e-9..=1.0 + 1e-9).contains(&v) {
                return (i, u, v);
            }
            let d = dist_to_quad(&pm.quad, [x, y]);
            if d < best.1 {
                best = (i, d, u, v);
            }
        }
        (best.0, best.2, best.3)
    }

    /// Maps a source point to the destination; NaN past the plane's horizon.
    pub fn map(&self, x: f64, y: f64) -> (f64, f64) {
        let (i, u, v) = self.locate(x, y);
        let pm = &self.planes[i];
        // Behind either homography's horizon the point wraps round infinity to a mirrored copy.
        if pm.src_inv.w(x, y) <= 0.0 || pm.dst.w(u, v) <= 0.0 {
            return (f64::NAN, f64::NAN);
        }
        let (mut ox, mut oy) = pm.dst.apply(u, v);
        let (cu, cv) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
        for (k, sh) in pm.shared.iter().enumerate() {
            let Some(j) = *sh else { continue };
            let (e, w) = edge_uv(k, cu, cv);
            if w <= 0.0 {
                continue;
            }
            let mine = pm.dst.apply(e[0], e[1]);
            let (qx, qy) = pm.src.apply(e[0], e[1]);
            let other = &self.planes[j];
            let (ou, ov) = other.src_inv.apply(qx, qy);
            let theirs = other.dst.apply(ou, ov);
            let target = ((mine.0 + theirs.0) / 2.0, (mine.1 + theirs.1) / 2.0);
            ox += w * (target.0 - mine.0);
            oy += w * (target.1 - mine.1);
        }
        (ox, oy)
    }

    /// Whether every plane maps onto itself (identity).
    pub fn is_identity(planes: &[Plane]) -> bool {
        planes.iter().all(|p| {
            p.src
                .iter()
                .zip(&p.dst)
                .all(|(a, b)| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9)
        })
    }
}

fn dist_to_quad(q: &[[f64; 2]; 4], p: [f64; 2]) -> f64 {
    (0..4)
        .map(|k| {
            let (a, b) = (q[k], q[(k + 1) % 4]);
            let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
            let l2 = dx * dx + dy * dy;
            let t = if l2 > 0.0 {
                (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            ((a[0] + t * dx - p[0]).powi(2) + (a[1] + t * dy - p[1]).powi(2)).sqrt()
        })
        .fold(f64::MAX, f64::min)
}

/// The whole of `src` through `planes`, written inside `clip` (upstream's `perspective_surface`).
/// `scale` is layer pixels per `src` pixel (the dialog's downsampled preview; 1 for the layer
/// itself). The identity, no planes or a degenerate plane give `src` back.
pub fn perspective_warp(
    src: &ImageData,
    planes: &[Plane],
    scale: f64,
    clip: Rect,
    interp: Interp,
    stop: Interrupt,
) -> Result<ImageData, Cancelled> {
    if PerspectiveMap::is_identity(planes) {
        return Ok(src.clone());
    }
    let Some(m) = PerspectiveMap::new(planes) else {
        return Ok(src.clone());
    };
    let f = |x: f64, y: f64| {
        let (u, v) = m.map(x * scale, y * scale);
        (u / scale, v / scale)
    };
    warp_mesh(src, &f, clip, interp, stop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: (f64, f64), b: (f64, f64), tol: f64) -> bool {
        (a.0 - b.0).abs() <= tol && (a.1 - b.1).abs() <= tol
    }

    #[test]
    fn identity_and_a_known_homography() {
        let q = [[10.0, 10.0], [90.0, 12.0], [85.0, 70.0], [12.0, 75.0]];
        let m = PerspectiveMap::new(&[Plane::identity(q)]).unwrap();
        for p in [(20.0, 20.0), (50.0, 40.0), (0.0, 0.0), (120.0, 90.0)] {
            assert!(
                close(m.map(p.0, p.1), p, 1e-9),
                "{p:?} → {:?}",
                m.map(p.0, p.1)
            );
        }
        // Square → trapezoid: corners land exactly, the centre lands where the homography says.
        let src = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let dst = [[20.0, 0.0], [80.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let m = PerspectiveMap::new(&[Plane { src, dst }]).unwrap();
        for (s, d) in src.iter().zip(&dst) {
            assert!(close(m.map(s[0], s[1]), (d[0], d[1]), 1e-9));
        }
        let h = Homography::rect_to_quad([0.0, 0.0, 100.0, 100.0], dst).unwrap();
        assert!(close(m.map(50.0, 50.0), h.apply(50.0, 50.0), 1e-9));
        // The diagonals' crossing stays the crossing: (50, 50) maps to the trapezoid's diagonal
        // intersection.
        let c = m.map(50.0, 50.0);
        assert!((c.0 - 50.0).abs() < 1e-9 && c.1 < 50.0, "{c:?}");
    }

    #[test]
    fn shared_edges_stay_continuous() {
        // Two planes sharing the edge x = 50 (with slightly mismatched corners that get linked).
        let a = Plane {
            src: [[0.0, 0.0], [50.0, 0.0], [50.0, 100.0], [0.0, 100.0]],
            dst: [[0.0, 10.0], [50.0, 0.0], [55.0, 100.0], [0.0, 90.0]],
        };
        let b = Plane {
            src: [[50.2, 0.0], [100.0, 0.0], [100.0, 100.0], [50.0, 100.2]],
            dst: [[50.0, 0.0], [110.0, 20.0], [100.0, 80.0], [55.0, 100.0]],
        };
        let m = PerspectiveMap::new(&[a, b]).unwrap();
        // After linking, the shared edge runs from (50.1, 0) to (50, 100.1).
        for t in [0.05, 0.25, 0.5, 0.77, 0.95] {
            let (x, y) = (50.1 - 0.1 * t, 100.1 * t);
            let l = m.map(x - 1e-6, y);
            let r = m.map(x + 1e-6, y);
            assert!(close(l, r, 1e-4), "t={t}: {l:?} vs {r:?}");
        }
        assert_eq!(linked_corners(&[Plane::identity([[0.0; 2]; 4])]).len(), 1);
    }

    /// Not upstream: a point the destination homography sends past its horizon maps to NaN (the
    /// warp then drops it) instead of to itself.
    #[test]
    fn points_past_the_horizon_map_to_nan() {
        let src = [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        // The top edge squeezed to 4 px: the source line the destination sends to infinity
        // (w = 1 − 0.96 v = 0) lies just below the bottom edge.
        let dst = [[48.0, 0.0], [52.0, 0.0], [100.0, 100.0], [0.0, 100.0]];
        let m = PerspectiveMap::new(&[Plane { src, dst }]).unwrap();
        for (x, y) in [(50.0, 50.0), (50.0, 103.0), (50.0, -400.0)] {
            let (u, v) = m.map(x, y);
            assert!(u.is_finite() && v.is_finite(), "({x}, {y})");
        }
        let past = m.map(50.0, 120.0);
        assert!(past.0.is_nan() && past.1.is_nan(), "{past:?}");
    }

    #[test]
    fn straighten_levels_and_plumbs_edges() {
        let mut ps = vec![Plane {
            src: [[0.0, 0.0], [100.0, 0.0], [100.0, 100.0], [0.0, 100.0]],
            dst: [[0.0, 3.0], [100.0, -2.0], [96.0, 100.0], [4.0, 104.0]],
        }];
        let before = ps[0].dst;
        straighten(&mut ps, Straighten::Vertical);
        assert_eq!(ps[0].dst[1][0], ps[0].dst[2][0]);
        assert_eq!(ps[0].dst[0][0], ps[0].dst[3][0]);
        assert_ne!(ps[0].dst[0][1], ps[0].dst[1][1]);
        straighten(&mut ps, Straighten::Auto);
        assert_eq!(ps[0].dst[0][1], ps[0].dst[1][1]);
        assert_eq!(ps[0].dst[2][1], ps[0].dst[3][1]);
        // Not upstream: Horizontal levels the top and bottom and leaves every x.
        let mut ps = vec![Plane {
            src: before,
            dst: before,
        }];
        straighten(&mut ps, Straighten::Horizontal);
        let d = ps[0].dst;
        assert_eq!((d[0][1], d[2][1]), (d[1][1], d[3][1]));
        assert_eq!(d.map(|p| p[0]), before.map(|p| p[0]));
    }

    /// Not upstream: Crop to plane's size is the mean of opposite edges, and `rectify` puts the
    /// quad's plane coordinates (u, v) at (u·w, v·h).
    #[test]
    fn rectify_flattens_a_quad_onto_its_crop_size() {
        let q = [[20.0, 14.0], [96.0, 26.0], [88.0, 82.0], [12.0, 66.0]];
        let (w, h) = crop_size(&q);
        let len = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
        assert_eq!(
            w,
            ((len(q[0], q[1]) + len(q[3], q[2])) / 2.0).round() as u32
        );
        assert_eq!(
            h,
            ((len(q[0], q[3]) + len(q[1], q[2])) / 2.0).round() as u32
        );
        // Quadrants of the plane in four colours, drawn by the inverse homography.
        let colors: [[u8; 4]; 4] = [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
        ];
        let quadrant = |u: f64, v: f64| colors[usize::from(u >= 0.5) + 2 * usize::from(v >= 0.5)];
        let inv = unit_to_quad(&q).unwrap().inverse().unwrap();
        let (iw, ih) = (110u32, 96u32);
        let mut data = Vec::new();
        for y in 0..ih {
            for x in 0..iw {
                let (u, v) = inv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                let inside = (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v);
                data.extend(if inside { quadrant(u, v) } else { [0; 4] });
            }
        }
        let img = ImageData {
            width: iw,
            height: ih,
            data,
        };
        let out = rectify(&img, q, w, h);
        assert_eq!((out.width, out.height), (w, h));
        let (mut seen, mut right) = (0, 0);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (
                    (f64::from(x) + 0.5) / f64::from(w),
                    (f64::from(y) + 0.5) / f64::from(h),
                );
                // Three pixels clear of the quadrant borders and the image's edge.
                let clear = |t: f64, n: u32| {
                    [t, (t - 0.5).abs(), 1.0 - t]
                        .iter()
                        .all(|d| d * f64::from(n) > 3.0)
                };
                if !(clear(u, w) && clear(v, h)) {
                    continue;
                }
                let i = (y * w + x) as usize * 4;
                seen += 1;
                let want = quadrant(u, v);
                if (0..4).all(|c| (i32::from(out.data[i + c]) - i32::from(want[c])).abs() <= 8) {
                    right += 1;
                }
            }
        }
        assert!(seen > 2000 && right == seen, "{right} of {seen}");
        // A crop larger than the image still fits (the source is padded).
        let big = rectify(&img, q, 160, 120);
        assert_eq!(
            (big.width, big.height, big.data.len()),
            (160, 120, 160 * 120 * 4)
        );
    }

    #[test]
    fn convex_quads_only() {
        let q = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(convex_quad(&q));
        // A dent, a bow tie, counter-clockwise and non-finite corners are not.
        assert!(!convex_quad(&[
            [0.0, 0.0],
            [10.0, 0.0],
            [3.0, 3.0],
            [0.0, 10.0]
        ]));
        assert!(!convex_quad(&[
            [0.0, 0.0],
            [10.0, 0.0],
            [0.0, 10.0],
            [10.0, 10.0]
        ]));
        assert!(!convex_quad(&[q[0], q[3], q[2], q[1]]));
        assert!(!convex_quad(&[[f64::NAN, 0.0], q[1], q[2], q[3]]));
    }
}
