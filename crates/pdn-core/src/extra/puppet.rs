//! Ported from PhotoCraft crates/algo/src/puppet.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: straight RGBA8 `ImageData` (whole layer) instead of `Surface`; no pin rotation or depth (`PuppetPin::{rotate, depth}`, fixed-rotation triangles, the rotated start, `draw_order`), no linked layer mask (`render_mask`, `puppet_warp_with_mask`) and no warm start (`solve`'s `init`); `parse` replaced by serde; `render_deformed` takes a scale for the dialog's downsampled preview and a clip, and a mesh too far to render gives `Ok(None)` there; the outline-in test buckets rows by the field's block size (not the spacing), and `MAX_OUTLINE_POINTS` is a hard cap: past it the smallest loops are left out of the mesh (the largest always stays) and their pixels are drawn where they are (`PuppetMesh::rest`, ruling R85), as upstream's soft cap meshes every loop and leaves the unpinned ones at rest; `deformed` solves without the per-round energies `solve` reports; the cancel is polled after the mesh, after the solver's set-up, once per solve round and inside the render; rayon through `extra::par`.
//!
//! Layers ▸ Puppet Warp: pins on a triangle mesh over the layer's opaque region, deformed
//! as-rigidly-as-possible.
//!
//! * **Mesh.** The layer's coverage (alpha above a threshold) becomes a signed distance field,
//!   offset by `expansion` to grow or shrink the region. Its outline is traced, simplified and
//!   split into sides no longer than the density's spacing; a triangular lattice at that spacing
//!   fills the inside, and the Delaunay triangulation of those points with the outline's sides
//!   recovered, minus the triangles outside the outline, is the mesh. Separate shapes get
//!   separate meshes.
//! * **Deformation.** The per-triangle ARAP energy of Liu et al. 2008 (the triangle form of
//!   Sorkine & Alexa 2007), `E = Σ_t Σ_(i,j)∈t w_ij |(v_i − v_j) − R_t (x_i − x_j)|²` with
//!   cotangent weights, minimized by alternating a local step (best rotation per triangle from
//!   the 2×2 covariance: `atan2(S10 − S01, S00 + S11)`) and a global step (one sparse SPD solve
//!   per axis: a banded Cholesky factor, else Jacobi-preconditioned conjugate gradients). Pins
//!   are stiff soft constraints on the barycentric point under them. The first global step uses
//!   `R = I` (a harmonic deformation), which is the classic initialisation. Modes change the
//!   local step: Rigid fits rotations, Distort fits similarities (rotation × scale, i.e. ASAP),
//!   and Normal fits rotation × √scale, in between. Mesh components without a pin stay where
//!   they are.
//! * **Rendering.** Triangles are rasterized with [`super::warp::warp_triangles`]
//!   (tile-parallel, premultiplied, bilinear/bicubic).

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::geom::Rect;
use super::interrupt::{Cancelled, Interrupt};
use super::par;
use super::warp::{Interp, warp_triangles};
use crate::canvas::ImageData;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PuppetMode {
    Rigid,
    #[default]
    Normal,
    Distort,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PuppetDensity {
    Fewer,
    #[default]
    Normal,
    More,
}

impl PuppetDensity {
    fn cells(self) -> f64 {
        match self {
            PuppetDensity::Fewer => 12.0,
            PuppetDensity::Normal => 20.0,
            PuppetDensity::More => 32.0,
        }
    }
}

/// A pin: where it was placed (`src`) and where it is now (`dst`), layer pixels.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PuppetPin {
    pub src: [f64; 2],
    pub dst: [f64; 2],
}

/// A complete puppet warp (the effect's value).
#[derive(Clone, Debug, PartialEq)]
pub struct PuppetWarp {
    pub pins: Vec<PuppetPin>,
    pub mode: PuppetMode,
    pub density: PuppetDensity,
    /// Grows (or with a negative value shrinks) the mesh beyond the opaque region, px.
    pub expansion: f64,
}

impl PuppetWarp {
    pub fn is_identity(&self) -> bool {
        self.pins.iter().all(|p| p.src == p.dst)
    }
}

/// A triangle mesh in layer pixels.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PuppetMesh {
    pub verts: Vec<[f64; 2]>,
    pub tris: Vec<[usize; 3]>,
    /// Grid spacing (px).
    pub spacing: f64,
    /// The pixels of the loops the outline cap left out of the mesh, as runs of field blocks:
    /// they stay where they are, as an unpinned component would (ruling R85).
    pub rest: Vec<Rect>,
}

/// Coverage threshold: alpha above this (0.05) counts as part of the puppet.
const ALPHA_MIN: u8 = 12;

/// The outline field's longest side, in blocks: how finely the mesh edge follows the outline.
const FIELD_BLOCKS: f64 = 1024.0;

/// The puppet's region as a signed distance field over a block grid: negative inside the
/// opaque area grown (or shrunk) by the expansion, positive outside, in px.
struct Field {
    x0: f64,
    y0: f64,
    /// Block size (px).
    g: f64,
    bw: usize,
    bh: usize,
    /// Signed distance from each block centre to the opaque outline (px, negative inside).
    sd: Vec<f32>,
    expansion: f64,
}

impl Field {
    fn new(src: &ImageData, bounds: Rect, expansion: f64) -> Field {
        let long = f64::from(bounds.width().max(bounds.height()));
        let grow = expansion.max(0.0).ceil();
        let g = ((long + 2.0 * grow + 8.0) / FIELD_BLOCKS).ceil().max(1.0) as i32;
        let area = bounds.inflate(grow as i32 + 2 * g);
        let bw = ((area.width() as i32 + g - 1) / g).max(1) as usize;
        let bh = ((area.height() as i32 + g - 1) / g).max(1) as usize;
        let mut cov = vec![false; bw * bh];
        let w = src.width as usize;
        // One block row per task (rows are independent).
        par::chunks_mut(&mut cov, bw, true, |by, out| {
            let r = Rect::new(
                area.x0,
                area.y0 + by as i32 * g,
                area.x0 + bw as i32 * g,
                area.y0 + (by as i32 + 1) * g,
            )
            .intersect(&bounds);
            for y in r.y0..r.y1 {
                for x in r.x0..r.x1 {
                    if src.data[(y as usize * w + x as usize) * 4 + 3] > ALPHA_MIN
                        && let Some(o) = out.get_mut(((x - area.x0) / g) as usize)
                    {
                        *o = true;
                    }
                }
            }
        });
        let outside = edt(&cov, true, bw, bh);
        let inside = edt(&cov, false, bw, bh);
        let gf = f64::from(g);
        let sd = cov
            .iter()
            .zip(outside.iter().zip(&inside))
            .map(|(&c, (&o, &i))| {
                if c {
                    (-(i.sqrt() - 0.5) * gf) as f32
                } else {
                    ((o.sqrt() - 0.5) * gf) as f32
                }
            })
            .collect();
        Field {
            x0: f64::from(area.x0),
            y0: f64::from(area.y0),
            g: gf,
            bw,
            bh,
            sd,
            expansion,
        }
    }

    /// The field at a block centre (exact, no interpolation).
    fn at(&self, bx: usize, by: usize) -> f64 {
        self.sd
            .get(by * self.bw + bx)
            .map_or(f64::MAX, |&d| f64::from(d) - self.expansion)
    }

    /// The field at `p` (bilinear between block centres; clamped to the grid, which ends
    /// outside the region).
    fn value(&self, p: [f64; 2]) -> f64 {
        let fx = ((p[0] - self.x0) / self.g - 0.5).clamp(0.0, (self.bw - 1) as f64);
        let fy = ((p[1] - self.y0) / self.g - 0.5).clamp(0.0, (self.bh - 1) as f64);
        let (ix, iy) = (fx.floor() as usize, fy.floor() as usize);
        let (ix1, iy1) = ((ix + 1).min(self.bw - 1), (iy + 1).min(self.bh - 1));
        let (tx, ty) = (fx - ix as f64, fy - iy as f64);
        let top = self.at(ix, iy) * (1.0 - tx) + self.at(ix1, iy) * tx;
        let bottom = self.at(ix, iy1) * (1.0 - tx) + self.at(ix1, iy1) * tx;
        top * (1.0 - ty) + bottom * ty
    }

    /// The outline (where the field crosses zero) as closed loops, traced by marching squares
    /// over the block centres. The field's border blocks lie outside, so every loop closes.
    fn outline(&self) -> Vec<Vec<[f64; 2]>> {
        let (bw, bh) = (self.bw, self.bh);
        if bw < 2 || bh < 2 {
            return Vec::new();
        }
        let inside = |x: usize, y: usize| self.at(x, y) < 0.0;
        // A crossing is named by its block edge: 2·block for the edge to the right neighbour,
        // 2·block + 1 for the edge to the one below.
        let id = |x: usize, y: usize, down: bool| 2 * (y * bw + x) + usize::from(down);
        let point = |id: usize| {
            let b = id / 2;
            let (x, y) = (b % bw, b / bw);
            let (x2, y2) = if id.is_multiple_of(2) {
                (x + 1, y)
            } else {
                (x, y + 1)
            };
            let (fa, fb) = (self.at(x, y), self.at(x2, y2));
            let t = if (fa - fb).abs() > 1e-12 {
                (fa / (fa - fb)).clamp(0.0, 1.0)
            } else {
                0.5
            };
            let (dx, dy) = ((x2 - x) as f64, (y2 - y) as f64);
            [
                self.x0 + (x as f64 + 0.5 + dx * t) * self.g,
                self.y0 + (y as f64 + 0.5 + dy * t) * self.g,
            ]
        };
        let mut links: HashMap<usize, [usize; 2]> = HashMap::new();
        let mut link = |a: usize, b: usize| {
            for (p, q) in [(a, b), (b, a)] {
                let e = links.entry(p).or_insert([usize::MAX; 2]);
                if e[0] == usize::MAX {
                    e[0] = q;
                } else {
                    e[1] = q;
                }
            }
        };
        for y in 0..bh - 1 {
            for x in 0..bw - 1 {
                let c = [
                    inside(x, y),
                    inside(x + 1, y),
                    inside(x + 1, y + 1),
                    inside(x, y + 1),
                ];
                if c.iter().all(|&v| v == c[0]) {
                    continue;
                }
                // Top, right, bottom, left.
                let edges = [
                    id(x, y, false),
                    id(x + 1, y, true),
                    id(x, y + 1, false),
                    id(x, y, true),
                ];
                let cross = [c[0] != c[1], c[1] != c[2], c[3] != c[2], c[0] != c[3]];
                let ks: Vec<usize> = (0..4).filter(|&k| cross[k]).collect();
                if let [a, b] = ks[..] {
                    link(edges[a], edges[b]);
                } else if ks.len() == 4 {
                    // A saddle: the centre decides which diagonal pair is joined.
                    let centre = self.at(x, y)
                        + self.at(x + 1, y)
                        + self.at(x + 1, y + 1)
                        + self.at(x, y + 1)
                        < 0.0;
                    if c[0] == centre {
                        link(edges[0], edges[1]);
                        link(edges[2], edges[3]);
                    } else {
                        link(edges[3], edges[0]);
                        link(edges[1], edges[2]);
                    }
                }
            }
        }
        let mut keys: Vec<usize> = links.keys().copied().collect();
        keys.sort_unstable();
        let mut seen: HashSet<usize> = HashSet::with_capacity(keys.len());
        let mut loops = Vec::new();
        for &start in &keys {
            if seen.contains(&start) {
                continue;
            }
            let mut lp = Vec::new();
            let (mut prev, mut cur) = (usize::MAX, start);
            loop {
                seen.insert(cur);
                lp.push(point(cur));
                let Some(&[a, b]) = links.get(&cur) else {
                    break;
                };
                let next = if a != prev { a } else { b };
                if next == usize::MAX || seen.contains(&next) {
                    break;
                }
                (prev, cur) = (cur, next);
            }
            if lp.len() >= 3 {
                loops.push(lp);
            }
        }
        loops
    }
}

/// Squared Euclidean distance (in blocks) from every block to the nearest block whose `mask`
/// equals `site` (Felzenszwalb & Huttenlocher's separable lower-envelope transform). About
/// 1e20 when there is none.
fn edt(mask: &[bool], site: bool, w: usize, h: usize) -> Vec<f64> {
    const FAR: f64 = 1e20;
    let mut d: Vec<f64> = mask
        .iter()
        .map(|&m| if m == site { 0.0 } else { FAR })
        .collect();
    let mut f = vec![0.0; w.max(h)];
    let mut out = vec![0.0; w.max(h)];
    let mut v = vec![0usize; w.max(h)];
    let mut z = vec![0.0; w.max(h) + 1];
    for x in 0..w {
        for (y, fy) in f[..h].iter_mut().enumerate() {
            *fy = d[y * w + x];
        }
        edt_1d(&f[..h], &mut out[..h], &mut v, &mut z);
        for (y, o) in out[..h].iter().enumerate() {
            d[y * w + x] = *o;
        }
    }
    for row in d.chunks_mut(w) {
        f[..w].copy_from_slice(row);
        edt_1d(&f[..w], row, &mut v, &mut z);
    }
    d
}

/// One-dimensional squared distance transform of the sampled function `f`.
fn edt_1d(f: &[f64], out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut k = 0usize;
    v[0] = 0;
    z[0] = f64::NEG_INFINITY;
    z[1] = f64::INFINITY;
    let meet = |q: usize, p: usize| {
        let (qf, pf) = (q as f64, p as f64);
        ((f[q] + qf * qf) - (f[p] + pf * pf)) / (2.0 * (qf - pf))
    };
    for q in 1..n {
        let mut s = meet(q, v[k]);
        while k > 0 && s <= z[k] {
            k -= 1;
            s = meet(q, v[k]);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f64::INFINITY;
    }
    k = 0;
    for (q, o) in out.iter_mut().enumerate() {
        let qf = q as f64;
        while z[k + 1] < qf {
            k += 1;
        }
        let p = v[k];
        *o = (qf - p as f64).powi(2) + f[p];
    }
}

/// Distance from `p` to the segment `a`–`b`.
fn seg_dist(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    dist2(p, [a[0] + dx * t, a[1] + dy * t]).sqrt()
}

/// Simplifies a closed loop (Douglas–Peucker, within `tol` px), which keeps its corners.
fn simplify(lp: &[[f64; 2]], tol: f64) -> Vec<[f64; 2]> {
    let n = lp.len();
    if n < 4 {
        return lp.to_vec();
    }
    let far = (1..n)
        .max_by(|&a, &b| dist2(lp[0], lp[a]).total_cmp(&dist2(lp[0], lp[b])))
        .unwrap_or(n / 2);
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[far] = true;
    // Spans (a, b) of the loop; b == n stands for point 0.
    let mut stack = vec![(0, far), (far, n)];
    while let Some((a, b)) = stack.pop() {
        let (pa, pb) = (lp[a], lp[b % n]);
        let mut best = (0.0, 0);
        for (i, &p) in lp.iter().enumerate().take(b).skip(a + 1) {
            let d = seg_dist(p, pa, pb);
            if d > best.0 {
                best = (d, i);
            }
        }
        if best.0 > tol {
            keep[best.1] = true;
            stack.push((a, best.1));
            stack.push((best.1, b));
        }
    }
    let out: Vec<[f64; 2]> = (0..n).filter(|&i| keep[i]).map(|i| lp[i]).collect();
    if out.len() >= 3 {
        return out;
    }
    // A speck thinner than the tolerance: keep a few of its points.
    (0..n).step_by((n / 4).max(1)).map(|i| lp[i]).collect()
}

/// Splits the sides of a closed loop longer than `step` into equal parts.
fn densify(lp: &[[f64; 2]], step: f64) -> Vec<[f64; 2]> {
    let mut out = Vec::with_capacity(lp.len());
    for (k, &a) in lp.iter().enumerate() {
        let b = lp[(k + 1) % lp.len()];
        let parts = (dist2(a, b).sqrt() / step).ceil().clamp(1.0, 1e4) as usize;
        for i in 0..parts {
            out.push(lerp(a, b, i as f64 / parts as f64));
        }
    }
    out
}

fn lerp(a: [f64; 2], b: [f64; 2], t: f64) -> [f64; 2] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn orient(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

/// Positive when `d` lies inside the circumcircle of the positively oriented `a`, `b`, `c`.
fn in_circle(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> f64 {
    let (ax, ay) = (a[0] - d[0], a[1] - d[1]);
    let (bx, by) = (b[0] - d[0], b[1] - d[1]);
    let (cx, cy) = (c[0] - d[0], c[1] - d[1]);
    (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay)
}

const NONE: usize = usize::MAX;

/// A triangle of the Delaunay triangulation: vertices (positively oriented) and the neighbour
/// across the edge opposite each vertex.
#[derive(Clone, Copy)]
struct DTri {
    v: [usize; 3],
    n: [usize; 3],
    alive: bool,
}

/// Incremental Delaunay triangulation (Bowyer–Watson), started from a triangle around the area.
/// Vertices 0..3 are that triangle's.
struct Delaunay {
    pts: Vec<[f64; 2]>,
    tris: Vec<DTri>,
    stamp: Vec<u32>,
    pass: u32,
    last: usize,
}

impl Delaunay {
    fn new(lo: [f64; 2], hi: [f64; 2]) -> Delaunay {
        let (cx, cy) = (0.5 * (lo[0] + hi[0]), 0.5 * (lo[1] + hi[1]));
        let d = (hi[0] - lo[0]).max(hi[1] - lo[1]).max(1.0) * 4.0;
        let mut pts = vec![
            [cx - 2.0 * d, cy - d],
            [cx + 2.0 * d, cy - d],
            [cx, cy + 2.0 * d],
        ];
        if orient(pts[0], pts[1], pts[2]) < 0.0 {
            pts.swap(1, 2);
        }
        Delaunay {
            pts,
            tris: vec![DTri {
                v: [0, 1, 2],
                n: [NONE; 3],
                alive: true,
            }],
            stamp: vec![0],
            pass: 0,
            last: 0,
        }
    }

    /// The triangle holding `p`: a visibility walk from the last one made.
    fn locate(&self, p: [f64; 2]) -> Option<usize> {
        let mut t = if self.tris.get(self.last).is_some_and(|t| t.alive) {
            self.last
        } else {
            self.tris.iter().position(|t| t.alive)?
        };
        for _ in 0..self.tris.len() + 3 {
            let tri = self.tris.get(t)?;
            let step = (0..3).find(|&k| {
                orient(
                    self.pts[tri.v[(k + 1) % 3]],
                    self.pts[tri.v[(k + 2) % 3]],
                    p,
                ) < 0.0
            });
            match step {
                None => return Some(t),
                Some(k) if tri.n[k] != NONE => t = tri.n[k],
                Some(_) => return None,
            }
        }
        self.tris.iter().position(|tri| {
            tri.alive
                && (0..3).all(|k| {
                    orient(
                        self.pts[tri.v[(k + 1) % 3]],
                        self.pts[tri.v[(k + 2) % 3]],
                        p,
                    ) >= 0.0
                })
        })
    }

    /// Inserts `p`; returns its vertex index (an existing vertex's when `p` coincides with it).
    fn insert(&mut self, p: [f64; 2]) -> Option<usize> {
        let t0 = self.locate(p)?;
        if let Some(&v) = self.tris[t0]
            .v
            .iter()
            .find(|&&v| dist2(self.pts[v], p) < 1e-12)
        {
            return Some(v);
        }
        self.pass = self.pass.wrapping_add(1);
        let pass = self.pass;
        // The cavity: every triangle whose circumcircle holds `p`, grown from `t0`.
        let mut bad = vec![t0];
        self.stamp[t0] = pass;
        let mut i = 0;
        while let Some(&t) = bad.get(i) {
            i += 1;
            for k in 0..3 {
                let nb = self.tris[t].n[k];
                if nb == NONE || self.stamp[nb] == pass {
                    continue;
                }
                let [a, b, c] = self.tris[nb].v.map(|v| self.pts[v]);
                if in_circle(a, b, c, p) > 0.0 {
                    self.stamp[nb] = pass;
                    bad.push(nb);
                }
            }
        }
        let mut rim: Vec<(usize, usize, usize)> = Vec::new();
        for &t in &bad {
            let tri = self.tris[t];
            for k in 0..3 {
                let nb = tri.n[k];
                if nb == NONE || self.stamp[nb] != pass {
                    rim.push((tri.v[(k + 1) % 3], tri.v[(k + 2) % 3], nb));
                }
            }
            self.tris[t].alive = false;
        }
        let pi = self.pts.len();
        self.pts.push(p);
        let base = self.tris.len();
        for (i, &(a, b, o)) in rim.iter().enumerate() {
            self.tris.push(DTri {
                v: [a, b, pi],
                n: [NONE, NONE, o],
                alive: true,
            });
            self.stamp.push(0);
            if let Some(ot) = self.tris.get_mut(o) {
                for k in 0..3 {
                    let (x, y) = (ot.v[(k + 1) % 3], ot.v[(k + 2) % 3]);
                    if (x == b && y == a) || (x == a && y == b) {
                        ot.n[k] = base + i;
                    }
                }
            }
        }
        for (i, &(a, b, _)) in rim.iter().enumerate() {
            let across_b = rim.iter().position(|r| r.0 == b).map_or(NONE, |j| base + j);
            let across_a = rim.iter().position(|r| r.1 == a).map_or(NONE, |j| base + j);
            self.tris[base + i].n[0] = across_b;
            self.tris[base + i].n[1] = across_a;
        }
        self.last = base;
        Some(pi)
    }

    fn edges(&self) -> HashSet<(usize, usize)> {
        let mut e = HashSet::new();
        for t in self.tris.iter().filter(|t| t.alive) {
            for k in 0..3 {
                let (a, b) = (t.v[k], t.v[(k + 1) % 3]);
                e.insert((a.min(b), a.max(b)));
            }
        }
        e
    }
}

/// Even-odd point-in-polygon test against closed outline segments, bucketed by row.
struct Inside<'a> {
    pts: &'a [[f64; 2]],
    segs: &'a [(usize, usize)],
    y0: f64,
    row: f64,
    rows: Vec<Vec<usize>>,
}

impl<'a> Inside<'a> {
    fn new(pts: &'a [[f64; 2]], segs: &'a [(usize, usize)], row: f64) -> Self {
        let ys = || segs.iter().flat_map(|&(a, b)| [pts[a][1], pts[b][1]]);
        let y0 = ys().fold(f64::MAX, f64::min);
        let y1 = ys().fold(f64::MIN, f64::max);
        let n = if y1 >= y0 {
            ((y1 - y0) / row).floor().clamp(0.0, 1e5) as usize + 1
        } else {
            0
        };
        let mut rows = vec![Vec::new(); n];
        for (i, &(a, b)) in segs.iter().enumerate() {
            let (lo, hi) = (pts[a][1].min(pts[b][1]), pts[a][1].max(pts[b][1]));
            let (r0, r1) = (
                ((lo - y0) / row).floor() as usize,
                ((hi - y0) / row).floor() as usize,
            );
            for r in rows.iter_mut().take(r1 + 1).skip(r0) {
                r.push(i);
            }
        }
        Inside {
            pts,
            segs,
            y0,
            row,
            rows,
        }
    }

    fn contains(&self, p: [f64; 2]) -> bool {
        if p[1] < self.y0 {
            return false;
        }
        let Some(row) = self
            .rows
            .get(((p[1] - self.y0) / self.row).floor() as usize)
        else {
            return false;
        };
        let mut odd = false;
        for &i in row {
            let (a, b) = (self.pts[self.segs[i].0], self.pts[self.segs[i].1]);
            if (a[1] > p[1]) != (b[1] > p[1])
                && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0])
            {
                odd = !odd;
            }
        }
        odd
    }
}

/// Most outline points the mesh takes; a busier outline is simplified more coarsely, then its
/// smallest loops stay at rest (`cap`).
const MAX_OUTLINE_POINTS: usize = 20_000;

/// Builds the mesh over the covered part of `src` inside `bounds`.
///
/// The outline of the opaque region grown by `expansion` is traced, simplified (corners kept)
/// and split into sides no longer than the density's spacing; a triangular lattice at that
/// spacing fills the inside, at least half a step from the outline. The mesh is the
/// Delaunay triangulation of those points with the outline's sides recovered (split until
/// each is an edge), minus the triangles outside the outline.
pub fn build_mesh(
    src: &ImageData,
    bounds: Rect,
    density: PuppetDensity,
    expansion: f64,
) -> PuppetMesh {
    let bounds = bounds.intersect(&Rect::new(0, 0, src.width as i32, src.height as i32));
    if bounds.is_empty() {
        return PuppetMesh::default();
    }
    let e = if expansion.is_finite() {
        expansion.clamp(-200.0, 200.0)
    } else {
        0.0
    };
    // The density counts steps across the region the mesh covers (grown by the expansion).
    let long = f64::from(bounds.width().max(bounds.height())) + 2.0 * e.max(0.0);
    let spacing = (long / density.cells()).max(2.0);
    let field = Field::new(src, bounds, e);
    let loops = field.outline();
    let empty = PuppetMesh {
        spacing,
        ..Default::default()
    };
    if loops.is_empty() {
        return empty;
    }
    let mut tol = (0.5 * field.g).max(0.5);
    let rings = loop {
        let rings: Vec<Vec<[f64; 2]>> = loops
            .iter()
            .map(|l| densify(&simplify(l, tol), spacing))
            .collect();
        if rings.iter().map(Vec::len).sum::<usize>() <= MAX_OUTLINE_POINTS || tol > spacing {
            break rings;
        }
        tol *= 2.0;
    };
    // Still past the cap (thousands of specks the simplification can't shrink): the smallest
    // loops stay at rest, outside the mesh.
    let (kept, dropped) = cap(&rings, MAX_OUTLINE_POINTS);
    let rest = rest_blocks(&field, dropped.iter().map(|&i| &loops[i]), bounds);
    let (lo, hi) = (
        [field.x0, field.y0],
        [
            field.x0 + field.bw as f64 * field.g,
            field.y0 + field.bh as f64 * field.g,
        ],
    );
    let mut dl = Delaunay::new(lo, hi);
    let mut segs: Vec<(usize, usize)> = Vec::new();
    for ring in kept.iter().map(|&i| &rings[i]) {
        let idx: Vec<usize> = ring.iter().filter_map(|&p| dl.insert(p)).collect();
        for (k, &a) in idx.iter().enumerate() {
            let b = idx[(k + 1) % idx.len()];
            if a != b {
                segs.push((a, b));
            }
        }
    }
    // The inside: a triangular lattice, kept half a step clear of the outline.
    let rise = spacing * 3f64.sqrt() / 2.0;
    let rows = ((hi[1] - lo[1]) / rise).ceil() as usize;
    let cols = ((hi[0] - lo[0]) / spacing).ceil() as usize;
    for j in 0..=rows {
        for i in 0..=cols {
            let p = [
                lo[0] + (i as f64 + 0.5 * (j % 2) as f64) * spacing,
                lo[1] + j as f64 * rise,
            ];
            if field.value(p) < -0.5 * spacing {
                dl.insert(p);
            }
        }
    }
    // Recover the outline's sides: split a side the triangulation lacks at its midpoint.
    let mut budget = segs.len() * 2;
    for _ in 0..8 {
        let edges = dl.edges();
        let mut next = Vec::with_capacity(segs.len());
        let mut split = false;
        for &(a, b) in &segs {
            if budget == 0 || edges.contains(&(a.min(b), a.max(b))) {
                next.push((a, b));
                continue;
            }
            match dl.insert(lerp(dl.pts[a], dl.pts[b], 0.5)) {
                Some(c) if c != a && c != b => {
                    next.push((a, c));
                    next.push((c, b));
                    budget -= 1;
                    split = true;
                }
                _ => next.push((a, b)),
            }
        }
        segs = next;
        if !split {
            break;
        }
    }
    let inside = Inside::new(&dl.pts, &segs, field.g);
    let mut tris: Vec<[usize; 3]> = Vec::new();
    for t in dl
        .tris
        .iter()
        .filter(|t| t.alive && t.v.iter().all(|&v| v >= 3))
    {
        let [a, b, c] = t.v.map(|v| dl.pts[v]);
        let centroid = [(a[0] + b[0] + c[0]) / 3.0, (a[1] + b[1] + c[1]) / 3.0];
        if orient(a, b, c) > 1e-9 && inside.contains(centroid) {
            tris.push(t.v);
        }
    }
    if tris.is_empty() {
        return PuppetMesh { rest, ..empty };
    }
    // Keep the vertices in use, numbered for a narrow band in the solver's matrix.
    let pos = &dl.pts;
    let order = band_order(pos, &tris);
    let mut remap = vec![usize::MAX; pos.len()];
    for (new, &old) in order.iter().enumerate() {
        remap[old] = new;
    }
    PuppetMesh {
        verts: order.iter().map(|&i| pos[i]).collect(),
        tris: tris.iter().map(|t| t.map(|i| remap[i])).collect(),
        spacing,
        rest,
    }
}

/// Which `rings` the mesh takes under `max` outline points: all when they fit, else the largest
/// (by area) while they fit, and always the largest. (kept, dropped) ring indices.
fn cap(rings: &[Vec<[f64; 2]>], max: usize) -> (Vec<usize>, Vec<usize>) {
    let mut order: Vec<usize> = (0..rings.len()).collect();
    if rings.iter().map(Vec::len).sum::<usize>() <= max {
        return (order, Vec::new());
    }
    let area = |r: &[[f64; 2]]| {
        let n = r.len();
        (0..n)
            .map(|k| r[k][0] * r[(k + 1) % n][1] - r[(k + 1) % n][0] * r[k][1])
            .sum::<f64>()
            .abs()
    };
    order.sort_by(|&a, &b| area(&rings[b]).total_cmp(&area(&rings[a])));
    let mut total = 0;
    // Once one is dropped every smaller one is too, so an island never outlives its hole.
    order.into_iter().partition(|&i| {
        let keep = total == 0 || total + rings[i].len() <= max;
        total += rings[i].len();
        keep
    })
}

/// The pixels of `loops` (outlines the cap dropped, as traced): per field block row, the runs
/// of covered blocks whose centres lie inside a loop (even-odd), clipped to `bounds`. A traced
/// contour passes strictly between a covered block's centre and an uncovered one's, so the test
/// never lands on the line.
fn rest_blocks<'a>(
    field: &Field,
    loops: impl Iterator<Item = &'a Vec<[f64; 2]>>,
    bounds: Rect,
) -> Vec<Rect> {
    let g = field.g;
    let block_x = |bx: isize| field.x0 as i32 + (bx as f64 * g) as i32;
    let mut out = Vec::new();
    for lp in loops {
        let n = lp.len();
        let lo = lp.iter().map(|p| p[1]).fold(f64::MAX, f64::min);
        let hi = lp.iter().map(|p| p[1]).fold(f64::MIN, f64::max);
        let by0 = ((lo - field.y0) / g - 0.5).ceil().max(0.0) as isize;
        let by1 = (((hi - field.y0) / g - 0.5).floor() as isize).min(field.bh as isize - 1);
        for by in by0..=by1 {
            let yc = field.y0 + (by as f64 + 0.5) * g;
            let mut xs: Vec<f64> = (0..n)
                .filter_map(|k| {
                    let (a, b) = (lp[k], lp[(k + 1) % n]);
                    ((a[1] > yc) != (b[1] > yc))
                        .then(|| a[0] + (yc - a[1]) / (b[1] - a[1]) * (b[0] - a[0]))
                })
                .collect();
            xs.sort_by(f64::total_cmp);
            let y = field.y0 as i32 + (by as f64 * g) as i32;
            for &[a, b] in xs.as_chunks::<2>().0 {
                let bx0 = ((a - field.x0) / g - 0.5).ceil().max(0.0) as isize;
                let bx1 = (((b - field.x0) / g - 0.5).floor() as isize).min(field.bw as isize - 1);
                let mut run = None;
                for bx in bx0..=bx1 + 1 {
                    let covered = bx <= bx1 && field.at(bx as usize, by as usize) < 0.0;
                    match (run, covered) {
                        (None, true) => run = Some(bx),
                        (Some(s), false) => {
                            let r = Rect::new(block_x(s), y, block_x(bx), y + g as i32);
                            let r = r.intersect(&bounds);
                            if !r.is_empty() {
                                out.push(r);
                            }
                            run = None;
                        }
                        _ => {}
                    }
                }
            }
        }
    }
    out
}

/// The vertices `tris` use, in reverse Cuthill–McKee order (breadth first from the topmost
/// vertex of each piece, lower degrees first), which keeps the solver's banded factor small.
fn band_order(pos: &[[f64; 2]], tris: &[[usize; 3]]) -> Vec<usize> {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); pos.len()];
    for t in tris {
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            adj[a].push(b);
            adj[b].push(a);
        }
    }
    for a in &mut adj {
        a.sort_unstable();
        a.dedup();
    }
    let mut starts: Vec<usize> = (0..pos.len()).filter(|&i| !adj[i].is_empty()).collect();
    starts.sort_by(|&a, &b| {
        pos[a][1]
            .total_cmp(&pos[b][1])
            .then(pos[a][0].total_cmp(&pos[b][0]))
    });
    let mut seen = vec![false; pos.len()];
    let mut order = Vec::with_capacity(starts.len());
    for s in starts {
        if seen[s] {
            continue;
        }
        seen[s] = true;
        let first = order.len();
        order.push(s);
        let mut i = first;
        while let Some(&v) = order.get(i) {
            i += 1;
            let mut next: Vec<usize> = adj[v].iter().copied().filter(|&w| !seen[w]).collect();
            next.sort_by_key(|&w| adj[w].len());
            for w in next {
                seen[w] = true;
                order.push(w);
            }
        }
    }
    order.reverse();
    order
}

/// A compressed sparse row matrix (symmetric, used by CG).
struct Csr {
    rows: Vec<usize>,
    cols: Vec<usize>,
    vals: Vec<f64>,
    diag: Vec<f64>,
}

impl Csr {
    fn from_triplets(n: usize, mut t: Vec<(usize, usize, f64)>) -> Csr {
        t.sort_by_key(|a| (a.0, a.1));
        let mut rows = vec![0usize; n + 1];
        let mut cols = Vec::with_capacity(t.len());
        let mut vals: Vec<f64> = Vec::with_capacity(t.len());
        let mut last = (usize::MAX, usize::MAX);
        for (i, j, v) in t {
            if (i, j) == last {
                if let Some(x) = vals.last_mut() {
                    *x += v;
                }
                continue;
            }
            last = (i, j);
            cols.push(j);
            vals.push(v);
            rows[i + 1] = cols.len();
        }
        for i in 0..n {
            rows[i + 1] = rows[i + 1].max(rows[i]);
        }
        let mut diag = vec![1.0; n];
        for (i, d) in diag.iter_mut().enumerate() {
            for k in rows[i]..rows[i + 1] {
                if cols[k] == i && vals[k] > 0.0 {
                    *d = vals[k];
                }
            }
        }
        Csr {
            rows,
            cols,
            vals,
            diag,
        }
    }

    fn mul(&self, x: &[f64], out: &mut [f64]) {
        for (i, o) in out.iter_mut().enumerate() {
            let mut s = 0.0;
            for k in self.rows[i]..self.rows[i + 1] {
                s += self.vals[k] * x[self.cols[k]];
            }
            *o = s;
        }
    }

    /// Jacobi-preconditioned conjugate gradients, warm-started from `x`.
    fn solve(&self, b: &[f64], x: &mut [f64]) {
        let n = b.len();
        let mut r = vec![0.0; n];
        self.mul(x, &mut r);
        for i in 0..n {
            r[i] = b[i] - r[i];
        }
        let bn: f64 = b.iter().map(|v| v * v).sum::<f64>().sqrt().max(1e-30);
        let mut z: Vec<f64> = r.iter().zip(&self.diag).map(|(r, d)| r / d).collect();
        let mut p = z.clone();
        let mut rz: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
        let mut ap = vec![0.0; n];
        for _ in 0..(4 * n).clamp(50, 2000) {
            if r.iter().map(|v| v * v).sum::<f64>().sqrt() <= 1e-12 * bn {
                break;
            }
            self.mul(&p, &mut ap);
            let pap: f64 = p.iter().zip(&ap).map(|(a, b)| a * b).sum();
            if pap.abs() < 1e-300 {
                break;
            }
            let alpha = rz / pap;
            for i in 0..n {
                x[i] += alpha * p[i];
                r[i] -= alpha * ap[i];
            }
            for i in 0..n {
                z[i] = r[i] / self.diag[i];
            }
            let rz2: f64 = r.iter().zip(&z).map(|(a, b)| a * b).sum();
            let beta = rz2 / rz;
            rz = rz2;
            for i in 0..n {
                p[i] = z[i] + beta * p[i];
            }
        }
    }
}

/// Banded Cholesky factor `A = L Lᵀ` (the mesh numbers its vertices in reverse Cuthill–McKee
/// order, so the system's bandwidth is about one breadth-first front): factored once per pin
/// set, then each global step is two triangular solves.
struct BandChol {
    n: usize,
    p: usize,
    /// Row `i` holds `L[i][i - p ..= i]`.
    l: Vec<f64>,
}

impl BandChol {
    /// `None` when the band would be too large (or the matrix isn't positive definite).
    fn new(a: &Csr) -> Option<BandChol> {
        let n = a.diag.len();
        let mut p = 0;
        for i in 0..n {
            for k in a.rows[i]..a.rows[i + 1] {
                p = p.max(i.abs_diff(a.cols[k]));
            }
        }
        if n.saturating_mul(p + 1) > 40_000_000 {
            return None;
        }
        let w = p + 1;
        let mut l = vec![0.0f64; n * w];
        for i in 0..n {
            for k in a.rows[i]..a.rows[i + 1] {
                let j = a.cols[k];
                if j <= i {
                    l[i * w + (j + p - i)] = a.vals[k];
                }
            }
        }
        for i in 0..n {
            let lo = i.saturating_sub(p);
            for j in lo..=i {
                let jlo = j.saturating_sub(p).max(lo);
                let mut s = l[i * w + (j + p - i)];
                for k in jlo..j {
                    s -= l[i * w + (k + p - i)] * l[j * w + (k + p - j)];
                }
                if i == j {
                    if s <= 0.0 || !s.is_finite() {
                        return None;
                    }
                    l[i * w + p] = s.sqrt();
                } else {
                    l[i * w + (j + p - i)] = s / l[j * w + p];
                }
            }
        }
        Some(BandChol { n, p, l })
    }

    #[allow(clippy::needless_range_loop)] // band index arithmetic reads clearer with indices
    fn solve(&self, b: &[f64], x: &mut [f64]) {
        let (n, p, w) = (self.n, self.p, self.p + 1);
        let mut y = b.to_vec();
        for i in 0..n {
            let mut s = y[i];
            for k in i.saturating_sub(p)..i {
                s -= self.l[i * w + (k + p - i)] * y[k];
            }
            y[i] = s / self.l[i * w + p];
        }
        for i in (0..n).rev() {
            let mut s = y[i];
            for k in i + 1..(i + p + 1).min(n) {
                s -= self.l[k * w + (i + p - k)] * x[k];
            }
            x[i] = s / self.l[i * w + p];
        }
    }
}

/// A pin bound to the mesh: barycentric weights on a triangle's vertices.
#[derive(Clone, Debug)]
struct Bound {
    verts: [usize; 3],
    bary: [f64; 3],
}

/// Prepared ARAP solver for one mesh and a set of pin source positions.
pub struct PuppetSolver {
    pub mesh: PuppetMesh,
    /// Cotangent weight of each triangle's edges: `w[t][k]` for the edge opposite corner `k`.
    w: Vec<[f64; 3]>,
    bound: Vec<Bound>,
    /// Vertices of components without pins (held at rest).
    anchored: Vec<bool>,
    matrix: Csr,
    chol: Option<BandChol>,
    pin_weight: f64,
}

fn cot(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    // Cotangent of the angle at `a`.
    let (u, v) = ([b[0] - a[0], b[1] - a[1]], [c[0] - a[0], c[1] - a[1]]);
    let cross = (u[0] * v[1] - u[1] * v[0]).abs();
    if cross < 1e-12 {
        0.0
    } else {
        (u[0] * v[0] + u[1] * v[1]) / cross
    }
}

/// Barycentric coordinates of `p` in triangle `a b c`; `None` when degenerate.
fn bary(a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]) -> Option<[f64; 3]> {
    let det = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
    if det.abs() < 1e-12 {
        return None;
    }
    let l1 = ((p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])) / det;
    let l2 = ((b[0] - a[0]) * (p[1] - a[1]) - (p[0] - a[0]) * (b[1] - a[1])) / det;
    Some([1.0 - l1 - l2, l1, l2])
}

fn locate(mesh: &PuppetMesh, p: [f64; 2]) -> Bound {
    for t in &mesh.tris {
        let [a, b, c] = t.map(|i| mesh.verts[i]);
        if let Some(l) = bary(a, b, c, p)
            && l.iter().all(|&v| v >= -1e-9)
        {
            return Bound { verts: *t, bary: l };
        }
    }
    // Outside the mesh: the nearest vertex.
    let i = (0..mesh.verts.len())
        .min_by(|&a, &b| dist2(mesh.verts[a], p).total_cmp(&dist2(mesh.verts[b], p)))
        .unwrap_or(0);
    Bound {
        verts: [i, i, i],
        bary: [1.0, 0.0, 0.0],
    }
}

fn dist2(a: [f64; 2], b: [f64; 2]) -> f64 {
    (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)
}

/// The rest position under `p` on the deformed mesh (where a new pin attaches), topmost triangle
/// first; `None` off the mesh. From upstream's `puppet_ui.rs` (`rest_of`).
pub fn rest_of(mesh: &PuppetMesh, deformed: &[[f64; 2]], p: [f64; 2]) -> Option<[f64; 2]> {
    if deformed.len() != mesh.verts.len() {
        return None;
    }
    mesh.tris.iter().rev().find_map(|tri| {
        let [a, b, c] = tri.map(|i| deformed[i]);
        let l = bary(a, b, c, p).filter(|l| l.iter().all(|&v| v >= 0.0))?;
        let [ra, rb, rc] = tri.map(|i| mesh.verts[i]);
        Some([
            l[0] * ra[0] + l[1] * rb[0] + l[2] * rc[0],
            l[0] * ra[1] + l[1] * rb[1] + l[2] * rc[1],
        ])
    })
}

impl PuppetSolver {
    /// Prepares the solver for pins placed at `pin_src`.
    pub fn new(mesh: PuppetMesh, pin_src: &[[f64; 2]]) -> Self {
        let nv = mesh.verts.len();
        let w: Vec<[f64; 3]> = mesh
            .tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|i| mesh.verts[i]);
                [0.5 * cot(a, b, c), 0.5 * cot(b, c, a), 0.5 * cot(c, a, b)]
            })
            .collect();
        // Without vertices there is nothing to bind a pin to (`locate` would fall back to a
        // vertex 0 that doesn't exist, #951); every pin loop zips `bound`, so the pins are then
        // ignored.
        let bound: Vec<Bound> = if nv == 0 {
            Vec::new()
        } else {
            pin_src.iter().map(|p| locate(&mesh, *p)).collect()
        };
        // Connected components (union-find over triangle edges).
        let mut parent: Vec<usize> = (0..nv).collect();
        fn find(p: &mut [usize], mut i: usize) -> usize {
            while p[i] != i {
                p[i] = p[p[i]];
                i = p[i];
            }
            i
        }
        for t in &mesh.tris {
            for k in 1..3 {
                let (a, b) = (find(&mut parent, t[0]), find(&mut parent, t[k]));
                if a != b {
                    parent[a] = b;
                }
            }
        }
        let mut pinned_root = vec![false; nv];
        for b in &bound {
            for &v in &b.verts {
                let r = find(&mut parent, v);
                pinned_root[r] = true;
            }
        }
        let anchored: Vec<bool> = (0..nv)
            .map(|v| !pinned_root[find(&mut parent, v)])
            .collect();
        let mut trip: Vec<(usize, usize, f64)> = Vec::with_capacity(mesh.tris.len() * 12 + nv);
        let mut diag_sum = 0.0;
        for (t, wt) in mesh.tris.iter().zip(&w) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let wk = wt[k];
                trip.push((i, i, wk));
                trip.push((j, j, wk));
                trip.push((i, j, -wk));
                trip.push((j, i, -wk));
                diag_sum += 2.0 * wk;
            }
        }
        let pin_weight = 1e4 * (diag_sum / nv.max(1) as f64).max(1.0);
        for b in &bound {
            for a in 0..3 {
                for c in 0..3 {
                    trip.push((b.verts[a], b.verts[c], pin_weight * b.bary[a] * b.bary[c]));
                }
            }
        }
        for (v, &an) in anchored.iter().enumerate() {
            if an {
                trip.push((v, v, pin_weight));
            }
        }
        let matrix = Csr::from_triplets(nv, trip);
        let chol = BandChol::new(&matrix);
        PuppetSolver {
            mesh,
            w,
            bound,
            anchored,
            matrix,
            chol,
            pin_weight,
        }
    }

    /// The ARAP energy of `v` for the given per-triangle linear maps (plus the pin terms).
    fn energy_with(&self, v: &[[f64; 2]], rots: &[[f64; 4]], pin_dst: &[[f64; 2]]) -> f64 {
        let x = &self.mesh.verts;
        let mut e = 0.0;
        for ((t, wt), r) in self.mesh.tris.iter().zip(&self.w).zip(rots) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let d = [v[i][0] - v[j][0], v[i][1] - v[j][1]];
                let ex = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                let re = [r[0] * ex[0] + r[1] * ex[1], r[2] * ex[0] + r[3] * ex[1]];
                e += wt[k] * ((d[0] - re[0]).powi(2) + (d[1] - re[1]).powi(2));
            }
        }
        for (b, d) in self.bound.iter().zip(pin_dst) {
            let p = (0..3).fold([0.0, 0.0], |a, k| {
                [
                    a[0] + b.bary[k] * v[b.verts[k]][0],
                    a[1] + b.bary[k] * v[b.verts[k]][1],
                ]
            });
            e += self.pin_weight * dist2(p, *d);
        }
        e
    }

    fn local(&self, v: &[[f64; 2]], mode: PuppetMode) -> Vec<[f64; 4]> {
        let x = &self.mesh.verts;
        self.mesh
            .tris
            .iter()
            .zip(&self.w)
            .map(|(t, wt)| {
                let mut s = [0.0f64; 4];
                let mut norm = 0.0;
                for k in 0..3 {
                    let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                    let d = [v[i][0] - v[j][0], v[i][1] - v[j][1]];
                    let e = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                    s[0] += wt[k] * d[0] * e[0];
                    s[1] += wt[k] * d[0] * e[1];
                    s[2] += wt[k] * d[1] * e[0];
                    s[3] += wt[k] * d[1] * e[1];
                    norm += wt[k] * (e[0] * e[0] + e[1] * e[1]);
                }
                let (c, sn) = (s[0] + s[3], s[2] - s[1]);
                let th = sn.atan2(c);
                let scale = match mode {
                    PuppetMode::Rigid => 1.0,
                    PuppetMode::Normal => (c.hypot(sn) / norm.max(1e-12)).sqrt(),
                    PuppetMode::Distort => c.hypot(sn) / norm.max(1e-12),
                };
                let scale = if scale.is_finite() && scale > 0.0 {
                    scale
                } else {
                    1.0
                };
                let (sth, cth) = th.sin_cos();
                [scale * cth, -scale * sth, scale * sth, scale * cth]
            })
            .collect()
    }

    fn global(&self, rots: &[[f64; 4]], pin_dst: &[[f64; 2]], v: &mut [[f64; 2]]) {
        let nv = v.len();
        let x = &self.mesh.verts;
        let mut bx = vec![0.0; nv];
        let mut by = vec![0.0; nv];
        for ((t, wt), r) in self.mesh.tris.iter().zip(&self.w).zip(rots) {
            for k in 0..3 {
                let (i, j) = (t[(k + 1) % 3], t[(k + 2) % 3]);
                let e = [x[i][0] - x[j][0], x[i][1] - x[j][1]];
                let re = [r[0] * e[0] + r[1] * e[1], r[2] * e[0] + r[3] * e[1]];
                bx[i] += wt[k] * re[0];
                by[i] += wt[k] * re[1];
                bx[j] -= wt[k] * re[0];
                by[j] -= wt[k] * re[1];
            }
        }
        for (b, d) in self.bound.iter().zip(pin_dst) {
            for k in 0..3 {
                bx[b.verts[k]] += self.pin_weight * b.bary[k] * d[0];
                by[b.verts[k]] += self.pin_weight * b.bary[k] * d[1];
            }
        }
        for (i, &an) in self.anchored.iter().enumerate() {
            if an {
                bx[i] += self.pin_weight * x[i][0];
                by[i] += self.pin_weight * x[i][1];
            }
        }
        let mut sx: Vec<f64> = v.iter().map(|p| p[0]).collect();
        let mut sy: Vec<f64> = v.iter().map(|p| p[1]).collect();
        match &self.chol {
            Some(c) => {
                c.solve(&bx, &mut sx);
                c.solve(&by, &mut sy);
            }
            None => {
                self.matrix.solve(&bx, &mut sx);
                self.matrix.solve(&by, &mut sy);
            }
        }
        for (i, p) in v.iter_mut().enumerate() {
            *p = [sx[i], sy[i]];
        }
    }

    /// Deforms the mesh from rest: `pin_dst` (one per pin given to [`Self::new`]) and
    /// `iterations` local/global rounds after a first `R = I` step. Returns the deformed
    /// vertices and the rigid ARAP energy after each round.
    pub fn solve(
        &self,
        pin_dst: &[[f64; 2]],
        mode: PuppetMode,
        iterations: usize,
    ) -> (Vec<[f64; 2]>, Vec<f64>) {
        let mut energies = Vec::with_capacity(iterations);
        let v = self.rounds(
            pin_dst,
            mode,
            iterations,
            Interrupt::NONE,
            Some(&mut energies),
        );
        (v.unwrap_or_default(), energies)
    }

    /// [`Self::solve`]'s vertices without the energies, `stop` polled once per round.
    pub fn deformed(
        &self,
        pin_dst: &[[f64; 2]],
        mode: PuppetMode,
        iterations: usize,
        stop: Interrupt,
    ) -> Result<Vec<[f64; 2]>, Cancelled> {
        self.rounds(pin_dst, mode, iterations, stop, None)
    }

    fn rounds(
        &self,
        pin_dst: &[[f64; 2]],
        mode: PuppetMode,
        iterations: usize,
        stop: Interrupt,
        mut energies: Option<&mut Vec<f64>>,
    ) -> Result<Vec<[f64; 2]>, Cancelled> {
        let mut v = self.mesh.verts.clone();
        if v.is_empty() {
            return Ok(v);
        }
        let identity = vec![[1.0, 0.0, 0.0, 1.0]; self.mesh.tris.len()];
        self.global(&identity, pin_dst, &mut v);
        for _ in 0..iterations {
            stop.check()?;
            let rots = self.local(&v, mode);
            self.global(&rots, pin_dst, &mut v);
            if let Some(e) = energies.as_deref_mut() {
                let rigid = self.local(&v, PuppetMode::Rigid);
                e.push(self.energy_with(&v, &rigid, pin_dst));
            }
        }
        Ok(v)
    }

    /// Energy of the current state with the rigid best fit (for monitoring).
    pub fn energy(&self, v: &[[f64; 2]], pin_dst: &[[f64; 2]]) -> f64 {
        let rots = self.local(v, PuppetMode::Rigid);
        self.energy_with(v, &rots, pin_dst)
    }
}

/// Number of local/global rounds used by the command.
pub const ITERATIONS: usize = 40;

/// The deformed mesh of a puppet warp: (solver, deformed vertices); `stop` is polled after the
/// mesh, after the solver's set-up and once per round.
pub fn deform(
    src: &ImageData,
    bounds: Rect,
    w: &PuppetWarp,
    iterations: usize,
    stop: Interrupt,
) -> Result<(PuppetSolver, Vec<[f64; 2]>), Cancelled> {
    let mesh = build_mesh(src, bounds, w.density, w.expansion);
    stop.check()?;
    let pin_src: Vec<[f64; 2]> = w.pins.iter().map(|p| p.src).collect();
    let solver = PuppetSolver::new(mesh, &pin_src);
    stop.check()?;
    let dst: Vec<[f64; 2]> = w.pins.iter().map(|p| p.dst).collect();
    let v = solver.deformed(&dst, w.mode, iterations, stop)?;
    Ok((solver, v))
}

/// The largest area (px) a puppet warp's result may span: 2³⁰. A pin dragged absurdly far
/// stretches the mesh over far more (#1019).
pub const MAX_WARP_PIXELS: f64 = (1u64 << 30) as f64;

/// Rasterizes the mesh deformation of `src` (the whole layer, `scale` layer pixels per `src`
/// pixel) into `clip`: the deformed triangles only, so pixels outside the mesh are dropped.
/// Without triangles (an expansion that shrinks every pixel away) nothing warps and `src` comes
/// back; `Ok(None)` when the deformed mesh would span more than [`MAX_WARP_PIXELS`]. The pixels of
/// `mesh.rest` are copied from `src` where they are.
pub fn render_deformed(
    src: &ImageData,
    mesh: &PuppetMesh,
    deformed: &[[f64; 2]],
    scale: f64,
    clip: Rect,
    interp: Interp,
    stop: Interrupt,
) -> Result<Option<ImageData>, Cancelled> {
    if mesh.tris.is_empty() || deformed.len() != mesh.verts.len() {
        return Ok(Some(src.clone()));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for p in deformed {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    }
    let area = (x1 - x0 + 1.0) * (y1 - y0 + 1.0);
    if area.is_nan() || area > MAX_WARP_PIXELS {
        return Ok(None);
    }
    let s = |p: [f64; 2]| [p[0] / scale, p[1] / scale];
    let verts: Vec<([f64; 2], [f64; 2])> = deformed
        .iter()
        .zip(&mesh.verts)
        .map(|(d, r)| (s(*d), s(*r)))
        .collect();
    let mut out = warp_triangles(src, &verts, &mesh.tris, clip, interp, stop)?;
    // R85: the pixels of the loops the cap left out stay where they are.
    let w = src.width as usize;
    let whole = Rect::new(0, 0, src.width as i32, src.height as i32);
    let scaled = |v: i32, up: bool| {
        let v = f64::from(v) / scale;
        (if up { v.ceil() } else { v.floor() }) as i32
    };
    for r in &mesh.rest {
        let r = Rect::new(
            scaled(r.x0, false),
            scaled(r.y0, false),
            scaled(r.x1, true),
            scaled(r.y1, true),
        )
        .intersect(&clip)
        .intersect(&whole);
        for y in r.y0..r.y1 {
            for x in r.x0..r.x1 {
                let i = (y as usize * w + x as usize) * 4;
                if src.data[i + 3] > 0 {
                    out.data[i..i + 4].copy_from_slice(&src.data[i..i + 4]);
                }
            }
        }
    }
    Ok(Some(out))
}

/// Applies a puppet warp to the whole of `src`, its mesh over the opaque pixels inside `bounds`,
/// written inside `clip`. The identity gives `src` back; `Ok(None)` when the deformed mesh would
/// span more than [`MAX_WARP_PIXELS`].
pub fn puppet_warp(
    src: &ImageData,
    bounds: Rect,
    w: &PuppetWarp,
    clip: Rect,
    interp: Interp,
    stop: Interrupt,
) -> Result<Option<ImageData>, Cancelled> {
    if w.is_identity() {
        return Ok(Some(src.clone()));
    }
    let (solver, v) = deform(src, bounds, w, ITERATIONS, stop)?;
    render_deformed(src, &solver.mesh, &v, 1.0, clip, interp, stop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px};
    use crate::extra::warp::tests::{content, whole};

    /// An L shape: a vertical bar (20..36 × 10..70) and a foot (36..70 × 56..70), in a 90×90
    /// layer.
    fn blob() -> ImageData {
        from_fn(90, 90, |x, y| {
            if (20..36).contains(&x) && (10..70).contains(&y) {
                [((x + y) % 7 * 42) as u8, 128, (y * 3) as u8, 255]
            } else if (36..70).contains(&x) && (56..70).contains(&y) {
                [51, ((x * 3) % 5 * 63) as u8, 204, 255]
            } else {
                [0; 4]
            }
        })
    }

    fn warp(pins: Vec<PuppetPin>, mode: PuppetMode, expansion: f64) -> PuppetWarp {
        PuppetWarp {
            pins,
            mode,
            density: PuppetDensity::Normal,
            expansion,
        }
    }

    fn pin(src: [f64; 2], dst: [f64; 2]) -> PuppetPin {
        PuppetPin { src, dst }
    }

    #[test]
    fn mesh_covers_the_shape_only() {
        let s = blob();
        let b = content(&s);
        let m = build_mesh(&s, b, PuppetDensity::Normal, 2.0);
        assert!(m.tris.len() > 20);
        let inside = |p: [f64; 2]| {
            m.tris.iter().any(|t| {
                let xs = t.map(|i| m.verts[i]);
                let lo = |c: usize| xs.iter().map(|v| v[c]).fold(f64::MAX, f64::min);
                let hi = |c: usize| xs.iter().map(|v| v[c]).fold(f64::MIN, f64::max);
                p[0] >= lo(0) && p[0] <= hi(0) && p[1] >= lo(1) && p[1] <= hi(1)
            })
        };
        assert!(inside([28.0, 30.0]) && inside([60.0, 62.0]));
        assert!(
            !inside([60.0, 20.0]),
            "the empty corner of the L has no mesh"
        );
        let more = build_mesh(&s, b, PuppetDensity::More, 2.0);
        let fewer = build_mesh(&s, b, PuppetDensity::Fewer, 2.0);
        assert!(more.tris.len() > m.tris.len() && m.tris.len() > fewer.tris.len());
    }

    fn in_mesh(m: &PuppetMesh, p: [f64; 2]) -> bool {
        m.tris.iter().any(|t| {
            let [a, b, c] = t.map(|i| m.verts[i]);
            let (d0, d1, d2) = (orient(a, b, p), orient(b, c, p), orient(c, a, p));
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        })
    }

    #[test]
    fn mesh_follows_the_outline() {
        // #3105: the mesh was every grid cell touching the shape, a staircase up to a cell
        // outside it. It now follows the outline grown by the expansion.
        let s = from_fn(200, 200, |x, y| {
            let r = (f64::from(x) + 0.5 - 100.0).hypot(f64::from(y) + 0.5 - 100.0);
            if r <= 60.0 {
                [77, 153, 230, 255]
            } else {
                [0; 4]
            }
        });
        let m = build_mesh(&s, content(&s), PuppetDensity::Normal, 2.0);
        let r = |v: &[f64; 2]| (v[0] - 100.0).hypot(v[1] - 100.0);
        assert!(
            m.verts.iter().all(|v| r(v) <= 63.0),
            "a vertex outside the grown disc"
        );
        assert!(
            m.verts.iter().filter(|v| r(v) > 61.0).count() >= 40,
            "the outline carries vertices"
        );
        let area: f64 = m
            .tris
            .iter()
            .map(|t| orient(m.verts[t[0]], m.verts[t[1]], m.verts[t[2]]).abs() / 2.0)
            .sum();
        let disc = std::f64::consts::PI * 62.0 * 62.0;
        assert!((area / disc - 1.0).abs() < 0.03, "{area} vs {disc}");
        // Sharp corners and a concave corner: every opaque pixel is still inside the mesh, even
        // without an expansion, and the empty corner of the L is not.
        let l = blob();
        let m = build_mesh(&l, content(&l), PuppetDensity::Fewer, 0.0);
        let bar = (20..36).flat_map(|x| (10..70).map(move |y| (x, y)));
        let foot = (36..70).flat_map(|x| (56..70).map(move |y| (x, y)));
        for (x, y) in bar.chain(foot) {
            let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
            assert!(in_mesh(&m, p), "({x}, {y}) is outside the mesh");
        }
        assert!(!in_mesh(&m, [40.0, 50.0]) && !in_mesh(&m, [60.0, 20.0]));
    }

    #[test]
    fn no_pins_or_unmoved_pins_are_identity() {
        let s = blob();
        let b = content(&s);
        for pins in [
            vec![],
            vec![
                pin([28.0, 20.0], [28.0, 20.0]),
                pin([60.0, 62.0], [60.0, 62.0]),
            ],
        ] {
            let w = warp(pins, PuppetMode::Normal, 2.0);
            let out = puppet_warp(&s, b, &w, whole(&s), Interp::Bicubic, Interrupt::NONE);
            assert_eq!(out.unwrap().unwrap().data, s.data);
        }
        // Not the short cut: an unmoved pin solved and rendered through the mesh is exact too.
        let w = warp(
            vec![pin([28.0, 20.0], [28.0, 20.0])],
            PuppetMode::Normal,
            2.0,
        );
        let (solver, v) = deform(&s, b, &w, ITERATIONS, Interrupt::NONE).unwrap();
        let out = render_deformed(
            &s,
            &solver.mesh,
            &v,
            1.0,
            whole(&s),
            Interp::Bicubic,
            Interrupt::NONE,
        );
        assert_eq!(out.unwrap().unwrap().data, s.data);
    }

    #[test]
    fn an_expansion_that_empties_the_mesh_warps_nothing() {
        // #951: with pins and no vertices, `locate` fell back to vertex 0 and the solver indexed
        // its empty union-find.
        let solver = PuppetSolver::new(PuppetMesh::default(), &[[10.0, 10.0]]);
        assert!(solver.mesh.verts.is_empty());
        let s = blob();
        let b = content(&s);
        let w = warp(
            vec![pin([28.0, 30.0], [38.0, 25.0])],
            PuppetMode::Normal,
            -200.0,
        );
        assert!(build_mesh(&s, b, w.density, w.expansion).tris.is_empty());
        // Nothing to warp: the pixels stay, rather than being dropped outside an empty mesh.
        let out = puppet_warp(&s, b, &w, whole(&s), Interp::Bilinear, Interrupt::NONE);
        assert_eq!(out.unwrap().unwrap().data, s.data);
    }

    #[test]
    fn a_pin_dragged_absurdly_far_is_refused_before_rendering() {
        // #1019: the renderer sized its tile bookkeeping by the deformed mesh, so one far pin
        // allocated hundreds of MB before drawing a pixel.
        let s = blob();
        let b = content(&s);
        let far = |dst: [f64; 2]| {
            let pins = vec![pin([28.0, 20.0], [28.0, 20.0]), pin([60.0, 62.0], dst)];
            puppet_warp(
                &s,
                b,
                &warp(pins, PuppetMode::Normal, 2.0),
                whole(&s),
                Interp::Bilinear,
                Interrupt::NONE,
            )
            .unwrap()
        };
        assert!(far([1048576.0, 1048576.0]).is_none());
        assert!(far([-1e7, 62.0]).is_none());
        let near = far([70.0, 66.0]).expect("a near pin warps");
        assert!(!content(&near).is_empty());
    }

    #[test]
    fn a_single_pin_translates_rigidly() {
        let s = blob();
        let b = content(&s);
        let w = warp(
            vec![pin([28.0, 30.0], [38.0, 25.0])],
            PuppetMode::Rigid,
            2.0,
        );
        let (solver, v) = deform(&s, b, &w, ITERATIONS, Interrupt::NONE).unwrap();
        for (d, x) in v.iter().zip(&solver.mesh.verts) {
            assert!(
                (d[0] - x[0] - 10.0).abs() < 1e-3 && (d[1] - x[1] + 5.0).abs() < 1e-3,
                "{d:?} {x:?}"
            );
        }
        let out = puppet_warp(&s, b, &w, whole(&s), Interp::Bilinear, Interrupt::NONE);
        let out = out.unwrap().unwrap();
        assert_eq!(content(&out), b.translate(10, -5));
        assert_eq!(px(&out, 30, 20), px(&s, 20, 25));
    }

    #[test]
    fn arap_energy_decreases_and_modes_differ() {
        let s = blob();
        let b = content(&s);
        let mesh = build_mesh(&s, b, PuppetDensity::Normal, 2.0);
        let src = [[28.0, 14.0], [64.0, 62.0], [28.0, 62.0]];
        let dst = [[10.0, 30.0], [70.0, 50.0], [28.0, 62.0]];
        let solver = PuppetSolver::new(mesh, &src);
        let (v, e) = solver.solve(&dst, PuppetMode::Rigid, 30);
        assert!(e.len() == 30);
        let start = solver.energy(&solver.solve(&dst, PuppetMode::Rigid, 0).0, &dst);
        assert!(e[0] <= start * (1.0 + 1e-9), "{} vs {start}", e[0]);
        for w in e.windows(2) {
            assert!(w[1] <= w[0] * (1.0 + 1e-6) + 1e-9, "{w:?}");
        }
        assert!(e[29] < start);
        // Pins are honoured closely.
        for (b, d) in solver.bound.iter().zip(&dst) {
            let p = (0..3).fold([0.0, 0.0], |a, k| {
                [
                    a[0] + b.bary[k] * v[b.verts[k]][0],
                    a[1] + b.bary[k] * v[b.verts[k]][1],
                ]
            });
            assert!(dist2(p, *d) < 0.25, "{p:?} vs {d:?}");
        }
        let (vd, _) = solver.solve(&dst, PuppetMode::Distort, 30);
        assert_ne!(v, vd);
    }

    /// I2: thousands of specks stay bounded: the outline keeps at most `MAX_OUTLINE_POINTS`
    /// (the smallest loops go first, the big shape stays), and the mesh builds quickly.
    #[test]
    fn many_specks_stay_within_the_outline_cap() {
        // 22,500 2×2 specks (one field block each, four outline points) around a 200 px square.
        let s = from_fn(1200, 1200, |x, y| {
            let square = (500..700).contains(&x) && (500..700).contains(&y);
            if square || (x % 8 < 2 && y % 8 < 2) {
                [255, 255, 255, 255]
            } else {
                [0; 4]
            }
        });
        let t = std::time::Instant::now();
        let m = build_mesh(&s, whole(&s), PuppetDensity::Normal, 0.0);
        let secs = t.elapsed().as_secs_f64();
        assert!(secs < 120.0, "{secs} s");
        assert!(
            m.verts.len() <= 3 * MAX_OUTLINE_POINTS,
            "{} vertices",
            m.verts.len()
        );
        assert!(in_mesh(&m, [600.0, 600.0]), "the square is kept");
    }

    /// The cap keeps the largest loops while they fit, and always the largest: one loop past
    /// the cap on its own used to leave every loop out and the mesh empty.
    #[test]
    fn the_cap_always_keeps_the_largest_loop() {
        let ring = |r: f64, n: usize| -> Vec<[f64; 2]> {
            (0..n)
                .map(|k| {
                    let t = k as f64 / n as f64 * std::f64::consts::TAU;
                    [r * t.cos(), r * t.sin()]
                })
                .collect()
        };
        // Points 10, 50, 5; areas middle, largest, smallest.
        let rings = [ring(20.0, 10), ring(90.0, 50), ring(3.0, 5)];
        assert_eq!(cap(&rings, 70), (vec![0, 1, 2], vec![]));
        assert_eq!(cap(&rings, 60), (vec![1, 0], vec![2]));
        assert_eq!(cap(&rings, 20), (vec![1], vec![0, 2]));
    }

    /// Ruling R85: specks the cap leaves out of the mesh stay exactly where they were, as the
    /// unpinned components the mesh does take (and upstream's soft cap would) stay at rest.
    #[test]
    fn dropped_specks_stay_where_they_are() {
        // 10,000 2×2 specks (each an outline loop of its own) around a 120 px square, pinned
        // and moved 10 px right.
        let s = from_fn(800, 800, |x, y| {
            let square = (340..460).contains(&x) && (340..460).contains(&y);
            if square || (x % 8 < 2 && y % 8 < 2) {
                [(x % 251) as u8, (y % 241) as u8, 160, 255]
            } else {
                [0; 4]
            }
        });
        let m = build_mesh(&s, whole(&s), PuppetDensity::Normal, 2.0);
        assert!(!m.rest.is_empty(), "the cap left loops out");
        let w = warp(
            vec![pin([400.0, 400.0], [410.0, 400.0])],
            PuppetMode::Rigid,
            2.0,
        );
        let out = puppet_warp(
            &s,
            whole(&s),
            &w,
            whole(&s),
            Interp::Bicubic,
            Interrupt::NONE,
        );
        let out = out.unwrap().unwrap();
        // Everything away from the square and where it went is as it was, dropped specks too.
        let near = Rect::new(330, 330, 480, 470);
        for (x, y) in (0..800).flat_map(|y| (0..800).map(move |x| (x, y))) {
            if !near.contains(x as i32, y as i32) {
                assert_eq!(px(&out, x, y), px(&s, x, y), "({x}, {y})");
            }
        }
        // An opaque pixel of a speck whose loop the cap left out.
        let (x, y) = m
            .rest
            .iter()
            .filter(|r| r.y0 > 600)
            .flat_map(|r| (r.y0..r.y1).flat_map(move |y| (r.x0..r.x1).map(move |x| (x, y))))
            .map(|(x, y)| (x as u32, y as u32))
            .find(|&(x, y)| px(&s, x, y)[3] == 255)
            .expect("a dropped speck low down");
        assert_eq!(px(&out, x, y), px(&s, x, y));
    }

    /// Not upstream: a cancel between the solve and the render stops the warp.
    #[test]
    fn a_cancelled_warp_returns_cancelled() {
        let s = blob();
        let w = warp(
            vec![pin([28.0, 30.0], [38.0, 25.0])],
            PuppetMode::Normal,
            2.0,
        );
        let flag = std::sync::atomic::AtomicBool::new(true);
        let out = puppet_warp(
            &s,
            content(&s),
            &w,
            whole(&s),
            Interp::Bicubic,
            Interrupt(Some(&flag)),
        );
        assert!(matches!(out, Err(Cancelled)));
    }

    /// Upstream UI's `rest_of`: a point on the deformed mesh maps back to its rest position;
    /// off the mesh there is none.
    #[test]
    fn rest_of_inverts_the_deformation() {
        let s = blob();
        let w = warp(
            vec![pin([28.0, 30.0], [38.0, 25.0])],
            PuppetMode::Rigid,
            2.0,
        );
        let (solver, v) = deform(&s, content(&s), &w, ITERATIONS, Interrupt::NONE).unwrap();
        let r = rest_of(&solver.mesh, &v, [40.0, 40.0]).unwrap();
        assert!(
            (r[0] - 30.0).abs() < 1e-3 && (r[1] - 45.0).abs() < 1e-3,
            "{r:?}"
        );
        assert!(rest_of(&solver.mesh, &v, [5.0, 85.0]).is_none());
    }
}
