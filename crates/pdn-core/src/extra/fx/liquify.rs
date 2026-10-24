//! Ported from PhotoCraft crates/algo/src/liquify.rs, with the tests of crates/algo/tests/it/liquify_source_footprint.rs, @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: only the brushes the dialog offers (Forward Warp, Twirl clockwise and counterclockwise, Pucker, Bloat, Freeze, Thaw, Reconstruct), so Smooth, Push Left, the Freeze Lasso and the whole-field operations (Reconstruct All, Mask All / None / Invert) are dropped with `LiquifyTool::ALL`, `label`, `is_global` and the stroke's `amount`; the layer is straight RGBA8 covering the field bounds, which `apply_liquify` and `ProxyImage::new` read directly (premultiplied per tile footprint or proxy row, as upstream), one 8-bit depth instead of `Surface` formats; `apply_liquify` polls an `Interrupt` before each tile and returns `Err(Cancelled)`; the proxy's box filter averages only pixels inside the bounds (upstream also averaged in the transparent pixels past the right and bottom edges); `render_full_resolution` (the zoomed-in preview) dropped; `Rect::tiles` is a local helper; parallel paths go through `extra::par` (its `map` and `chunks_mut` are upstream's, with serial wasm twins); tests: the lasso, Reconstruct All, mask-button, Smooth, Push Left, sample-depth and full-resolution-preview parts dropped (`forward_warp_displaces_along_the_stroke`, `twirl_pucker_and_bloat_have_the_right_sense` and `replay_is_deterministic` are what is left of theirs), a frozen rectangle set node by node stands in for the lasso, and the source-footprint tests live in this module.
//!
//! Filter › Liquify: a dense displacement field edited by brush strokes, then used to resample
//! the layer once.
//!
//! The field stores, for every node of a regular grid (one node per `cell` document pixels), the
//! offset `d` such that output pixel `p` shows the source at `p + d(p)` (an inverse map, so
//! applying it never leaves holes). Strokes are plain data ([`LiquifyStroke`]: tool, brush and
//! points), applied in order by [`LiquifyField::apply_stroke`]; replaying the same strokes on the
//! same bounds gives the same field bit for bit, which is what makes the filter a replayable
//! command.
//!
//! Every tool is written as "where does the new output at `p` come from in the current output":
//! `q = M(p)` (Forward Warp: `p - kΔ`, Twirl: a rotation about the brush centre, Pucker/Bloat: a
//! scaling about it), so the new field is `d'(p) = q + d(q) - p`, sampled bilinearly from the
//! field as it was before the dab. Reconstruct scales `d` toward zero. Freeze/Thaw paint a mask
//! that scales every tool's strength down.
//!
//! [`apply_liquify`] upsamples the field with Catmull-Rom (bicubic) per pixel and samples the
//! source (premultiplied, bicubic), tile by tile in parallel; tiles where the field is zero are
//! copied untouched. [`ProxyImage::render`] is the cheap preview path: bilinear field and
//! bilinear source on a downsampled RGBA copy, re-rendered only where a dab changed things.

use crate::extra::geom::Rect;
use crate::extra::interrupt::{Cancelled, Interrupt};
use crate::extra::par;
use serde::{Deserialize, Serialize};

/// The Liquify brushes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LiquifyTool {
    #[default]
    ForwardWarp,
    Reconstruct,
    TwirlCw,
    TwirlCcw,
    Pucker,
    Bloat,
    Freeze,
    Thaw,
}

impl LiquifyTool {
    /// Tools that act while the brush is held still (each recorded point is a dab).
    pub fn is_stationary(self) -> bool {
        self != LiquifyTool::ForwardWarp
    }
}

fn d_size() -> f64 {
    100.0
}
fn d_density() -> f64 {
    50.0
}
fn d_pressure() -> f64 {
    100.0
}
fn d_rate() -> f64 {
    80.0
}

/// One brush stroke: the tool, its brush options and the points it passed through
/// (`[x, y]` or `[x, y, pressure 0..1]`, document pixels).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiquifyStroke {
    #[serde(default)]
    pub tool: LiquifyTool,
    /// Brush diameter (px).
    #[serde(default = "d_size")]
    pub size: f64,
    /// How far the full effect reaches toward the brush edge, 0 (only the centre; feathered)
    /// ..100 (most of the brush). It always fades to nothing at the edge ([`falloff`]).
    #[serde(default = "d_density")]
    pub density: f64,
    /// Overall strength, 0..100.
    #[serde(default = "d_pressure")]
    pub pressure: f64,
    /// Speed of the stationary tools, 0..100.
    #[serde(default = "d_rate")]
    pub rate: f64,
    #[serde(default)]
    pub points: Vec<Vec<f64>>,
}

impl LiquifyStroke {
    pub fn new(tool: LiquifyTool, size: f64) -> Self {
        LiquifyStroke {
            tool,
            size,
            density: d_density(),
            pressure: d_pressure(),
            rate: d_rate(),
            points: Vec::new(),
        }
    }

    /// Validates the stroke (finite numbers, 2 or 3 per point).
    pub fn validate(&self) -> Result<(), String> {
        if !(self.size.is_finite() && self.size >= 1.0 && self.size <= 15000.0) {
            return Err(format!("brush size {} out of range 1..15000", self.size));
        }
        for p in &self.points {
            if !(2..=3).contains(&p.len()) || p.iter().any(|v| !v.is_finite()) {
                return Err("stroke points are [x, y] or [x, y, pressure]".into());
            }
        }
        Ok(())
    }
}

/// The displacement field plus the freeze mask, over `bounds` (document pixels).
#[derive(Clone, Debug)]
pub struct LiquifyField {
    pub bounds: Rect,
    /// Document pixels per field node.
    pub cell: f64,
    pub w: usize,
    pub h: usize,
    /// Inverse displacement per node, row-major.
    pub d: Vec<[f32; 2]>,
    /// Freeze mask per node (0 = free, 1 = frozen).
    pub freeze: Vec<f32>,
    /// Where the brush stroke in progress stands, so dab spacing runs on across its segments.
    cursor: Option<StrokeCursor>,
}

/// Two fields are equal when their displacement and freeze mask are: the stroke cursor is
/// transient brush state, not part of the result.
impl PartialEq for LiquifyField {
    fn eq(&self, o: &Self) -> bool {
        self.bounds == o.bounds
            && self.cell == o.cell
            && self.w == o.w
            && self.h == o.h
            && self.d == o.d
            && self.freeze == o.freeze
    }
}

/// The brush stroke in progress: the point its last segment ended at, its last dab and how far
/// the brush has travelled along the path since that dab.
#[derive(Clone, Copy, Debug, PartialEq)]
struct StrokeCursor {
    end: [f64; 2],
    last_dab: [f64; 2],
    carry: f64,
}

/// Dabs over this many field nodes compute their rows in parallel (an 800 px brush has about
/// 125,000); smaller ones are not worth the hand-off.
const PARALLEL_DAB_NODES: usize = 16_384;

/// Upstream's global tile grid (`photocraft_geom::TILE_SIZE`).
const TILE: i32 = 256;

/// Distance between dabs along a stroke for brush radius `r`: a twentieth of the radius, so a
/// drag is many small pushes and its edge stays smooth.
fn dab_spacing(r: f64) -> f64 {
    (r * 0.05).max(0.5)
}

/// Per-dab strength of a stationary tool (Twirl, Pucker, Bloat, Reconstruct, Freeze, Thaw)
/// while it moves: scaled to the spacing so its effect per distance travelled stays what it was
/// with dabs a fifth of the radius apart.
fn moving_weight(r: f64) -> f64 {
    dab_spacing(r) / (r * 0.2).max(0.5)
}

/// The default field resolution for `bounds`: 2 px per node up to 4 MP, 4 px beyond.
pub fn auto_cell(bounds: Rect) -> f64 {
    if u64::from(bounds.width()) * u64::from(bounds.height()) <= 4_200_000 {
        2.0
    } else {
        4.0
    }
}

/// The global `TILE`-px tiles that meet `b`, each cut to `b` (upstream's `Rect::tiles`).
fn tiles(b: Rect) -> Vec<Rect> {
    if b.is_empty() {
        return vec![];
    }
    let (tx0, ty0) = (b.x0.div_euclid(TILE), b.y0.div_euclid(TILE));
    let (tx1, ty1) = ((b.x1 - 1).div_euclid(TILE), (b.y1 - 1).div_euclid(TILE));
    (ty0..=ty1)
        .flat_map(|ty| {
            (tx0..=tx1).map(move |tx| {
                let (x, y) = (tx.saturating_mul(TILE), ty.saturating_mul(TILE));
                Rect::new(x, y, x.saturating_add(TILE), y.saturating_add(TILE)).intersect(&b)
            })
        })
        .collect()
}

impl LiquifyField {
    pub fn new(bounds: Rect, cell: f64) -> Self {
        let cell = cell.clamp(1.0, 64.0);
        let w = (f64::from(bounds.width()) / cell).ceil() as usize + 2;
        let h = (f64::from(bounds.height()) / cell).ceil() as usize + 2;
        LiquifyField {
            bounds,
            cell,
            w,
            h,
            d: vec![[0.0; 2]; w * h],
            freeze: vec![0.0; w * h],
            cursor: None,
        }
    }

    /// Document position of node (i, j) (node 0 sits on the first pixel centre).
    #[inline]
    pub fn node_pos(&self, i: usize, j: usize) -> [f64; 2] {
        [
            f64::from(self.bounds.x0) + 0.5 + i as f64 * self.cell,
            f64::from(self.bounds.y0) + 0.5 + j as f64 * self.cell,
        ]
    }

    /// Field coordinates of a document point.
    #[inline]
    fn to_grid(&self, x: f64, y: f64) -> (f64, f64) {
        (
            (x - f64::from(self.bounds.x0) - 0.5) / self.cell,
            (y - f64::from(self.bounds.y0) - 0.5) / self.cell,
        )
    }

    pub fn is_identity(&self) -> bool {
        self.d.iter().all(|v| v[0] == 0.0 && v[1] == 0.0)
    }

    /// Largest displacement magnitude (px).
    pub fn max_displacement(&self) -> f32 {
        self.d.iter().map(|v| v[0].hypot(v[1])).fold(0.0, f32::max)
    }

    /// Bilinear displacement at a document point (zero outside the field).
    pub fn sample(&self, x: f64, y: f64) -> [f64; 2] {
        let (gx, gy) = self.to_grid(x, y);
        bilinear(&self.d, self.w, self.h, gx, gy)
    }

    /// Freeze value at a document point (bilinear).
    pub fn freeze_at(&self, x: f64, y: f64) -> f32 {
        let (gx, gy) = self.to_grid(x, y);
        let gx = gx.clamp(0.0, (self.w - 1) as f64);
        let gy = gy.clamp(0.0, (self.h - 1) as f64);
        let (i, j) = ((gx as usize).min(self.w - 2), (gy as usize).min(self.h - 2));
        let (fx, fy) = ((gx - i as f64) as f32, (gy - j as f64) as f32);
        let at = |i: usize, j: usize| self.freeze[j * self.w + i];
        (at(i, j) * (1.0 - fx) + at(i + 1, j) * fx) * (1.0 - fy)
            + (at(i, j + 1) * (1.0 - fx) + at(i + 1, j + 1) * fx) * fy
    }

    /// Applies one stroke. Returns the document rectangle whose output changed.
    pub fn apply_stroke(&mut self, s: &LiquifyStroke) -> Rect {
        self.apply_stroke_until(s, Interrupt::NONE)
            .unwrap_or_default()
    }

    /// [`Self::apply_stroke`], polling `stop` before each segment (after the opening dab): a
    /// long stroke stops partway with `Err(Cancelled)`, the field keeping the dabs so far.
    pub fn apply_stroke_until(
        &mut self,
        s: &LiquifyStroke,
        stop: Interrupt,
    ) -> Result<Rect, Cancelled> {
        self.cursor = None;
        let pts: Vec<[f64; 3]> = s
            .points
            .iter()
            .filter(|p| p.len() >= 2)
            .map(|p| [p[0], p[1], p.get(2).copied().unwrap_or(1.0)])
            .collect();
        let Some(&first) = pts.first() else {
            return Ok(Rect::EMPTY);
        };
        let mut dirty = self.stroke_begin(s, first);
        for w in pts.windows(2) {
            stop.check()?;
            dirty = dirty.union(&self.stroke_segment(s, w[0], w[1]));
        }
        Ok(dirty)
    }

    /// Starts a stroke at `p` (`[x, y, pressure]`): stationary tools dab once there. Interactive
    /// callers use this plus [`Self::stroke_segment`] per new point, which is exactly what
    /// [`Self::apply_stroke`] does with the whole point list.
    pub fn stroke_begin(&mut self, s: &LiquifyStroke, p: [f64; 3]) -> Rect {
        self.cursor = Some(StrokeCursor {
            end: [p[0], p[1]],
            last_dab: [p[0], p[1]],
            carry: 0.0,
        });
        if s.tool.is_stationary() {
            self.dab(s, [p[0], p[1]], [0.0, 0.0], p[2].clamp(0.0, 1.0), 1.0)
        } else {
            Rect::EMPTY
        }
    }

    /// Continues a stroke from `a` to `b`. Dabs fall every [`dab_spacing`] of distance travelled
    /// along the whole stroke, carried from one segment to the next, so the result is the same
    /// however finely the pointer path was sampled (a fast and a slow mouse give one warp). A
    /// stationary tool held still dabs once at `b`. When `a` is not where the previous segment
    /// ended, the stroke starts afresh at `a`.
    pub fn stroke_segment(&mut self, s: &LiquifyStroke, a: [f64; 3], b: [f64; 3]) -> Rect {
        self.walk_segment(s, a, b, true)
    }

    /// [`Self::stroke_segment`]; `skip` leaves out the dabs that cannot reach a field node (they
    /// would change nothing), which tests compare with the full walk.
    fn walk_segment(&mut self, s: &LiquifyStroke, a: [f64; 3], b: [f64; 3], skip: bool) -> Rect {
        let mut dirty = Rect::EMPTY;
        let (pa, pb) = (a[2].clamp(0.0, 1.0), b[2].clamp(0.0, 1.0));
        let stationary = s.tool.is_stationary();
        let cur = match self.cursor {
            Some(c) if c.end == [a[0], a[1]] => c,
            _ => StrokeCursor {
                end: [a[0], a[1]],
                last_dab: [a[0], a[1]],
                carry: 0.0,
            },
        };
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len = dx.hypot(dy);
        if len == 0.0 {
            self.cursor = Some(cur);
            return if stationary {
                self.dab(s, [b[0], b[1]], [0.0, 0.0], pb, 1.0)
            } else {
                dirty
            };
        }
        // NaN or a length that overflowed: nothing sensible to walk; the next segment restarts.
        if !len.is_finite() {
            self.cursor = None;
            return dirty;
        }
        let r = (s.size / 2.0).max(0.5);
        let spacing = dab_spacing(r);
        let weight = if stationary { moving_weight(r) } else { 1.0 };
        // Distance along this segment of its first dab, then one every `spacing`.
        let first = (spacing - cur.carry).clamp(0.0, spacing);
        // Distances are compared with a little slack: a dab that rounding puts a hair past the
        // segment's end still falls on it, so cutting one segment in two never adds or drops a dab.
        let slack = spacing * 1e-6;
        if first > len + slack {
            self.cursor = Some(StrokeCursor {
                end: [b[0], b[1]],
                last_dab: cur.last_dab,
                carry: cur.carry + len,
            });
            return dirty;
        }
        let last_k = ((len - first + slack) / spacing).floor().max(0.0);
        let at = |dist: f64| [a[0] + dx * dist / len, a[1] + dy * dist / len];
        let (mut k0, mut k1) = (0.0f64, last_k);
        if skip {
            match self.reachable_span(a, [dx, dy], len, r) {
                // A dragging dab is centred one spacing behind its position: pad by that and a step.
                Some((d0, d1)) => {
                    k0 = k0.max(((d0 - spacing - first) / spacing).floor() - 1.0);
                    k1 = k1.min(((d1 + spacing - first) / spacing).ceil() + 1.0);
                }
                None => k1 = -1.0,
            }
        }
        if k0 <= k1 {
            let mut prev = if k0 > 0.0 {
                at(first + (k0 - 1.0) * spacing)
            } else {
                cur.last_dab
            };
            for k in (k0 as u64)..=(k1 as u64) {
                let dist = (first + k as f64 * spacing).min(len);
                let c = at(dist);
                let delta = [c[0] - prev[0], c[1] - prev[1]];
                // Forward Warp centres the dab where the brush was (it drags what is under it).
                let centre = if stationary { c } else { prev };
                dirty =
                    dirty.union(&self.dab(s, centre, delta, pa + (pb - pa) * dist / len, weight));
                prev = c;
            }
        }
        let last = (first + last_k * spacing).min(len);
        self.cursor = Some(StrokeCursor {
            end: [b[0], b[1]],
            last_dab: at(last),
            carry: (len - last).clamp(0.0, spacing),
        });
        dirty
    }

    /// The distances along a segment from `a` (direction `dir`, length `len`) whose points lie
    /// within a dab radius `r` plus two nodes of the node grid, or `None` when none do. Only dabs
    /// near that span can touch a node; a segment to a far-off point has trillions of others
    /// (#937).
    fn reachable_span(&self, a: [f64; 3], dir: [f64; 2], len: f64, r: f64) -> Option<(f64, f64)> {
        if self.w == 0 || self.h == 0 {
            return None;
        }
        let pad = r + 2.0 * self.cell;
        let lo = [
            f64::from(self.bounds.x0) + 0.5 - pad,
            f64::from(self.bounds.y0) + 0.5 - pad,
        ];
        let hi = [
            lo[0] + (self.w - 1) as f64 * self.cell + 2.0 * pad,
            lo[1] + (self.h - 1) as f64 * self.cell + 2.0 * pad,
        ];
        let (mut u0, mut u1) = (0.0f64, 1.0f64);
        for ax in 0..2 {
            let d = dir[ax];
            if d == 0.0 {
                if a[ax].is_nan() || a[ax] < lo[ax] || a[ax] > hi[ax] {
                    return None;
                }
                continue;
            }
            let (v0, v1) = ((lo[ax] - a[ax]) / d, (hi[ax] - a[ax]) / d);
            u0 = u0.max(v0.min(v1));
            u1 = u1.min(v0.max(v1));
        }
        if u0.is_nan() || u1.is_nan() || u0 > u1 {
            return None;
        }
        Some((u0 * len, u1 * len))
    }

    /// One dab at `c` with brush motion `delta`, its strength scaled by `weight`. Returns the
    /// document rect it affected.
    fn dab(
        &mut self,
        s: &LiquifyStroke,
        c: [f64; 2],
        delta: [f64; 2],
        point_pressure: f64,
        weight: f64,
    ) -> Rect {
        let r = (s.size / 2.0).max(0.5);
        let strength = (s.pressure / 100.0).clamp(0.0, 1.0) * point_pressure * weight;
        let rate = (s.rate / 100.0).clamp(0.0, 1.0);
        let density = (s.density / 100.0).clamp(0.0, 1.0);
        if strength <= 0.0 {
            return Rect::EMPTY;
        }
        let (gx0, gy0) = self.to_grid(c[0] - r, c[1] - r);
        let (gx1, gy1) = self.to_grid(c[0] + r, c[1] + r);
        let i0 = gx0.floor().max(0.0) as usize;
        let j0 = gy0.floor().max(0.0) as usize;
        let i1 = (gx1.ceil().max(-1.0) as isize).min(self.w as isize - 1);
        let j1 = (gy1.ceil().max(-1.0) as isize).min(self.h as isize - 1);
        if i1 < i0 as isize || j1 < j0 as isize {
            return Rect::EMPTY;
        }
        let (i1, j1) = (i1 as usize, j1 as usize);
        // Snapshot of the affected nodes plus a margin (sampling reads around them).
        let m = ((delta[0].abs().max(delta[1].abs()) + r * 0.2) / self.cell).ceil() as usize + 2;
        let (si0, sj0) = (i0.saturating_sub(m), j0.saturating_sub(m));
        let (si1, sj1) = ((i1 + m).min(self.w - 1), (j1 + m).min(self.h - 1));
        let sw = si1 - si0 + 1;
        let sh = sj1 - sj0 + 1;
        let mut snap = Vec::with_capacity(sw * sh);
        for j in sj0..=sj1 {
            snap.extend_from_slice(&self.d[j * self.w + si0..=j * self.w + si1]);
        }
        let old = |gx: f64, gy: f64| -> [f64; 2] {
            let (lx, ly) = (gx - si0 as f64, gy - sj0 as f64);
            if lx >= 0.0 && ly >= 0.0 && lx <= (sw - 1) as f64 && ly <= (sh - 1) as f64 {
                bilinear(&snap, sw, sh, lx, ly)
            } else {
                // Outside the snapshot nothing changed: read the live field.
                bilinear(&self.d, self.w, self.h, gx, gy)
            }
        };
        // Every node reads only the snapshot and the field as it was before this dab (the writes
        // land after), so rows are independent: a large brush computes them in parallel.
        let inv_cell = 1.0 / self.cell;
        let node_row = |j: usize| -> Vec<(usize, [f32; 2], f32)> {
            let mut row = Vec::with_capacity(i1 - i0 + 1);
            for i in i0..=i1 {
                let p = self.node_pos(i, j);
                let (dx, dy) = (p[0] - c[0], p[1] - c[1]);
                let t2 = (dx * dx + dy * dy) / (r * r);
                if t2 >= 1.0 {
                    continue;
                }
                let fall = falloff(t2, density);
                let idx = j * self.w + i;
                let k_raw = strength * fall;
                if s.tool == LiquifyTool::Freeze || s.tool == LiquifyTool::Thaw {
                    let f = self.freeze[idx];
                    let v = if s.tool == LiquifyTool::Freeze {
                        (f as f64 + k_raw).min(1.0)
                    } else {
                        (f as f64 - k_raw).max(0.0)
                    };
                    row.push((idx, self.d[idx], v as f32));
                    continue;
                }
                let k = k_raw * (1.0 - f64::from(self.freeze[idx]));
                if k <= 0.0 {
                    continue;
                }
                let cur = self.d[idx];
                let new = match s.tool {
                    LiquifyTool::Reconstruct => {
                        let a = (1.0 - k * (0.1 + 0.4 * rate)) as f32;
                        [cur[0] * a, cur[1] * a]
                    }
                    _ => {
                        let q = match s.tool {
                            LiquifyTool::ForwardWarp => [p[0] - k * delta[0], p[1] - k * delta[1]],
                            LiquifyTool::TwirlCw | LiquifyTool::TwirlCcw => {
                                let th = k
                                    * rate
                                    * 0.12
                                    * if s.tool == LiquifyTool::TwirlCw {
                                        1.0
                                    } else {
                                        -1.0
                                    };
                                // Content turns by +th, so sample at the point turned by -th.
                                let (sn, cs) = (-th).sin_cos();
                                [c[0] + dx * cs - dy * sn, c[1] + dx * sn + dy * cs]
                            }
                            LiquifyTool::Pucker => {
                                let a = 1.0 + k * rate * 0.06;
                                [c[0] + dx * a, c[1] + dy * a]
                            }
                            _ => {
                                let a = 1.0 - k * rate * 0.06;
                                [c[0] + dx * a, c[1] + dy * a]
                            }
                        };
                        let (qx, qy) = (
                            (q[0] - p[0]) * inv_cell + i as f64,
                            (q[1] - p[1]) * inv_cell + j as f64,
                        );
                        let dq = old(qx, qy);
                        [(q[0] + dq[0] - p[0]) as f32, (q[1] + dq[1] - p[1]) as f32]
                    }
                };
                row.push((idx, new, self.freeze[idx]));
            }
            row
        };
        let rows = j1 - j0 + 1;
        let writes = par::map(rows, rows * (i1 - i0 + 1) > PARALLEL_DAB_NODES, |row| {
            node_row(j0 + row)
        });
        for (idx, d, f) in writes.into_iter().flatten() {
            self.d[idx] = d;
            self.freeze[idx] = f;
        }
        let pad = r.ceil() as i32 + 2;
        Rect::new(
            c[0].floor() as i32 - pad,
            c[1].floor() as i32 - pad,
            c[0].ceil() as i32 + pad,
            c[1].ceil() as i32 + pad,
        )
        .intersect(&self.bounds)
    }

    /// Replays strokes on a fresh field.
    pub fn from_strokes(bounds: Rect, cell: f64, strokes: &[LiquifyStroke]) -> Self {
        let mut f = LiquifyField::new(bounds, cell);
        for s in strokes {
            f.apply_stroke(s);
        }
        f
    }

    /// Catmull-Rom (bicubic) displacement at grid coordinates.
    fn sample_cubic(&self, gx: f64, gy: f64, wx: &[f64; 4], wy: &[f64; 4]) -> [f64; 2] {
        let (ix, iy) = (gx.floor() as isize, gy.floor() as isize);
        let mut acc = [0.0f64; 2];
        for (jj, wyj) in wy.iter().enumerate() {
            let y = (iy - 1 + jj as isize).clamp(0, self.h as isize - 1) as usize;
            let row = &self.d[y * self.w..(y + 1) * self.w];
            for (ii, wxi) in wx.iter().enumerate() {
                let x = (ix - 1 + ii as isize).clamp(0, self.w as isize - 1) as usize;
                let k = wxi * wyj;
                acc[0] += f64::from(row[x][0]) * k;
                acc[1] += f64::from(row[x][1]) * k;
            }
        }
        acc
    }

    /// Whether any node influencing document rect `r` is displaced.
    fn touches(&self, r: Rect) -> bool {
        let (gx0, gy0) = self.to_grid(f64::from(r.x0), f64::from(r.y0));
        let (gx1, gy1) = self.to_grid(f64::from(r.x1), f64::from(r.y1));
        let i0 = (gx0.floor() as isize - 2).clamp(0, self.w as isize - 1) as usize;
        let j0 = (gy0.floor() as isize - 2).clamp(0, self.h as isize - 1) as usize;
        let i1 = (gx1.ceil() as isize + 2).clamp(0, self.w as isize - 1) as usize;
        let j1 = (gy1.ceil() as isize + 2).clamp(0, self.h as isize - 1) as usize;
        (j0..=j1).any(|j| {
            self.d[j * self.w + i0..=j * self.w + i1]
                .iter()
                .any(|v| v[0] != 0.0 || v[1] != 0.0)
        })
    }
}

/// A dab's strength at squared distance `t2` (in brush radii, 0..1) for Density `density` (0..1):
/// 1 at the centre, and down to 0 at the rim with a flat slope at every density, as in
/// Photoshop, where Density sets how far the full effect reaches toward the edge (0: only the
/// centre, the rest feathered; 1: most of the brush) but the effect always fades out at it.
/// A brush that stopped short of 0 at its rim moved the pixels inside it and left those just
/// outside, tearing the image along every dab's circle (#2606).
pub fn falloff(t2: f64, density: f64) -> f64 {
    let t2 = t2.clamp(0.0, 1.0);
    let density = if density.is_nan() {
        0.5
    } else {
        density.clamp(0.0, 1.0)
    };
    // Feathered: (1 - t²)³. Dense: (1 - t⁴)², flat in the middle. Both are 0 with zero slope at
    // t = 1, and their slopes stay small enough that a dab every fifth of the radius never folds
    // the image over itself.
    let feathered = (1.0 - t2).powi(3);
    let dense = (1.0 - t2 * t2).powi(2);
    feathered + (dense - feathered) * density
}

fn bilinear(d: &[[f32; 2]], w: usize, h: usize, gx: f64, gy: f64) -> [f64; 2] {
    if !(gx > -1.0 && gy > -1.0 && gx < w as f64 && gy < h as f64) {
        return [0.0, 0.0];
    }
    let gx = gx.clamp(0.0, (w - 1) as f64);
    let gy = gy.clamp(0.0, (h - 1) as f64);
    let (i, j) = (
        (gx as usize).min(w.saturating_sub(2)),
        (gy as usize).min(h.saturating_sub(2)),
    );
    let (fx, fy) = (gx - i as f64, gy - j as f64);
    let i1 = (i + 1).min(w - 1);
    let j1 = (j + 1).min(h - 1);
    let at = |x: usize, y: usize| d[y * w + x];
    let (a, b, c, e) = (at(i, j), at(i1, j), at(i, j1), at(i1, j1));
    let l = |k: usize| {
        (f64::from(a[k]) * (1.0 - fx) + f64::from(b[k]) * fx) * (1.0 - fy)
            + (f64::from(c[k]) * (1.0 - fx) + f64::from(e[k]) * fx) * fy
    };
    [l(0), l(1)]
}

fn catmull_rom(t: f64) -> [f64; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        -0.5 * t3 + t2 - 0.5 * t,
        1.5 * t3 - 2.5 * t2 + 1.0,
        -1.5 * t3 + 2.0 * t2 + 0.5 * t,
        0.5 * t3 - 0.5 * t2,
    ]
}

/// 0–1 to a byte, rounded half up, clamped (`extra::shim::from_f32`).
fn byte(v: f64) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// Resamples `src` (straight RGBA8 covering `field.bounds`, row-major) through the field: each
/// pixel shows the source at `p + d(p)` (bicubic field, bicubic premultiplied source, edge pixels
/// repeat past the bounds). Untouched tiles are copied. `Err(Cancelled)` once `stop` fires; it is
/// polled before each 256-px tile.
pub fn apply_liquify(
    src: &[u8],
    field: &LiquifyField,
    stop: Interrupt,
) -> Result<Vec<u8>, Cancelled> {
    let b = field.bounds;
    let mut out = src.to_vec();
    if field.is_identity() || b.is_empty() {
        return Ok(out);
    }
    let bw = b.width() as usize;
    // A 1D Catmull-Rom stencil's absolute weights sum to at most 5/4, so the 2D
    // interpolated displacement can reach 25/16 of the largest nodal displacement.
    let maxd =
        ((f64::from(field.max_displacement()) * (25.0 / 16.0)).ceil() as i32).saturating_add(3);
    let has_freeze = field.freeze.iter().any(|&f| f > 0.0);
    let tiles: Vec<Rect> = tiles(b).into_iter().filter(|r| field.touches(*r)).collect();
    let work = |t: &Rect| -> Option<(Rect, Vec<u8>)> {
        if stop.cancelled() {
            return None;
        }
        let w = t.width() as usize;
        let h = t.height() as usize;
        // Field per pixel (bicubic), rows share their y weights.
        let mut disp = vec![[0.0f64; 2]; w * h];
        let (mut fx0, mut fy0, mut fx1, mut fy1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
        let mut moved = false;
        for y in 0..h {
            let py = f64::from(t.y0) + y as f64 + 0.5;
            let gy = (py - f64::from(b.y0) - 0.5) / field.cell;
            let wy = catmull_rom(gy - gy.floor());
            for x in 0..w {
                let px = f64::from(t.x0) + x as f64 + 0.5;
                let gx = (px - f64::from(b.x0) - 0.5) / field.cell;
                let wx = catmull_rom(gx - gx.floor());
                let mut d = field.sample_cubic(gx, gy, &wx, &wy);
                if has_freeze {
                    let mask = f64::from(field.freeze_at(px, py)).clamp(0.0, 1.0);
                    if mask > 0.0 {
                        let linear = field.sample(px, py);
                        d[0] = d[0] * (1.0 - mask) + linear[0] * mask;
                        d[1] = d[1] * (1.0 - mask) + linear[1] * mask;
                    }
                }
                if d[0] != 0.0 || d[1] != 0.0 {
                    moved = true;
                }
                disp[y * w + x] = d;
                let (sx, sy) = (px + d[0], py + d[1]);
                fx0 = fx0.min(sx);
                fy0 = fy0.min(sy);
                fx1 = fx1.max(sx);
                fy1 = fy1.max(sy);
            }
        }
        if !moved {
            return None;
        }
        let foot = Rect::new(
            fx0.floor() as i32 - 3,
            fy0.floor() as i32 - 3,
            fx1.ceil() as i32 + 3,
            fy1.ceil() as i32 + 3,
        )
        .intersect(&t.inflate(maxd))
        .intersect(&b);
        if foot.is_empty() {
            return None;
        }
        // The footprint, premultiplied.
        let fw = foot.width() as usize;
        let mut px = vec![0.0f32; fw * foot.height() as usize * 4];
        for (y, row) in px.chunks_exact_mut(fw * 4).enumerate() {
            let s = ((foot.y0 - b.y0) as usize + y) * bw + (foot.x0 - b.x0) as usize;
            for (p, q) in row
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(src[s * 4..(s + fw) * 4].as_chunks::<4>().0)
            {
                let al = f32::from(q[3]) / 255.0;
                p[0] = f32::from(q[0]) / 255.0 * al;
                p[1] = f32::from(q[1]) / 255.0 * al;
                p[2] = f32::from(q[2]) / 255.0 * al;
                p[3] = al;
            }
        }
        let at = |x: i32, y: i32| -> usize {
            let x = x.clamp(foot.x0, foot.x1 - 1);
            let y = y.clamp(foot.y0, foot.y1 - 1);
            ((y - foot.y0) as usize * fw + (x - foot.x0) as usize) * 4
        };
        let snap = |t: f64| {
            if (t - t.round()).abs() < 1e-7 {
                t.round()
            } else {
                t
            }
        };
        let mut outp = vec![0u8; w * h * 4];
        let mut acc = [0.0f64; 4];
        for y in 0..h {
            for x in 0..w {
                let d = disp[y * w + x];
                let u = snap(f64::from(t.x0) + x as f64 + d[0]);
                let v = snap(f64::from(t.y0) + y as f64 + d[1]);
                let (ix, iy) = (u.floor() as i32, v.floor() as i32);
                let (wx, wy) = (
                    catmull_rom(u - f64::from(ix)),
                    catmull_rom(v - f64::from(iy)),
                );
                acc.fill(0.0);
                for (j, wyj) in wy.iter().enumerate() {
                    for (i, wxi) in wx.iter().enumerate() {
                        let k = wxi * wyj;
                        if k == 0.0 {
                            continue;
                        }
                        let o = at(ix - 1 + i as i32, iy - 1 + j as i32);
                        for (c, s) in acc.iter_mut().enumerate() {
                            *s += f64::from(px[o + c]) * k;
                        }
                    }
                }
                let o = (y * w + x) * 4;
                let al = acc[3].clamp(0.0, 1.0);
                outp[o + 3] = byte(al);
                for c in 0..3 {
                    outp[o + c] = if al > 0.0 { byte(acc[c] / al) } else { 0 };
                }
            }
        }
        Some((*t, outp))
    };
    let done = par::map(tiles.len(), true, |i| work(&tiles[i]));
    stop.check()?;
    for (r, v) in done.into_iter().flatten() {
        let w = r.width() as usize;
        for (y, row) in v.chunks_exact(w * 4).enumerate() {
            let o = ((r.y0 - b.y0) as usize + y) * bw + (r.x0 - b.x0) as usize;
            out[o * 4..(o + w) * 4].copy_from_slice(row);
        }
    }
    Ok(out)
}

/// A downsampled, premultiplied RGBA copy of a layer for the interactive preview.
#[derive(Clone, Debug)]
pub struct ProxyImage {
    /// Document region the proxy covers (the field bounds).
    pub bounds: Rect,
    /// Document pixels per proxy pixel.
    pub scale: f64,
    pub w: usize,
    pub h: usize,
    /// Premultiplied RGBA, row-major.
    pub px: Vec<[f32; 4]>,
}

impl ProxyImage {
    /// Box-downsamples `src` (straight RGBA8 covering `bounds`) so the longer side is at most
    /// `max_side`.
    pub fn new(src: &[u8], bounds: Rect, max_side: usize) -> Self {
        let side = bounds.width().max(bounds.height()).max(1) as f64;
        let scale = (side / max_side.max(1) as f64).max(1.0).ceil();
        let w = ((f64::from(bounds.width()) / scale).ceil() as usize).max(1);
        let h = ((f64::from(bounds.height()) / scale).ceil() as usize).max(1);
        let k = scale as usize;
        let (bw, bh) = (bounds.width() as usize, bounds.height() as usize);
        let mut px = vec![[0.0f32; 4]; w * h];
        par::chunks_mut(&mut px, w, true, |y, row| {
            for (x, o) in row.iter_mut().enumerate() {
                let mut acc = [0.0f32; 4];
                let mut cnt = 0.0f32;
                for yy in y * k..((y + 1) * k).min(bh) {
                    for xx in x * k..((x + 1) * k).min(bw) {
                        let p = &src[(yy * bw + xx) * 4..][..4];
                        let a = f32::from(p[3]) / 255.0;
                        acc[0] += f32::from(p[0]) / 255.0 * a;
                        acc[1] += f32::from(p[1]) / 255.0 * a;
                        acc[2] += f32::from(p[2]) / 255.0 * a;
                        acc[3] += a;
                        cnt += 1.0;
                    }
                }
                if cnt > 0.0 {
                    *o = acc.map(|v| v / cnt);
                }
            }
        });
        ProxyImage {
            bounds,
            scale,
            w,
            h,
            px,
        }
    }

    /// Renders proxy pixels `r` (proxy coordinates) through the field into `out` (premultiplied
    /// RGBA8, `w × h`): bilinear field, bilinear source.
    pub fn render(&self, field: &LiquifyField, r: [usize; 4], out: &mut [[u8; 4]]) {
        let [x0, y0, x1, y1] = [
            r[0].min(self.w),
            r[1].min(self.h),
            r[2].min(self.w),
            r[3].min(self.h),
        ];
        if x0 >= x1 || y0 >= y1 {
            return;
        }
        let s = self.scale;
        let (bx, by) = (f64::from(self.bounds.x0), f64::from(self.bounds.y0));
        let row = |y: usize, line: &mut [[u8; 4]]| {
            let py = by + (y as f64 + 0.5) * s;
            for (x, slot) in line.iter_mut().enumerate().take(x1).skip(x0) {
                let px = bx + (x as f64 + 0.5) * s;
                let d = field.sample(px, py);
                let (u, v) = ((px + d[0] - bx) / s - 0.5, (py + d[1] - by) / s - 0.5);
                let (iu, iv) = (u.floor(), v.floor());
                let (fu, fv) = ((u - iu) as f32, (v - iv) as f32);
                let at = |x: f64, y: f64| {
                    self.px[(y.clamp(0.0, (self.h - 1) as f64) as usize) * self.w
                        + x.clamp(0.0, (self.w - 1) as f64) as usize]
                };
                let (a, b, c, e) = (
                    at(iu, iv),
                    at(iu + 1.0, iv),
                    at(iu, iv + 1.0),
                    at(iu + 1.0, iv + 1.0),
                );
                *slot = std::array::from_fn(|k| {
                    let v = (a[k] * (1.0 - fu) + b[k] * fu) * (1.0 - fv)
                        + (c[k] * (1.0 - fu) + e[k] * fu) * fv;
                    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
                });
            }
        };
        let rows = &mut out[y0 * self.w..y1 * self.w];
        // A dab's few thousand pixels are faster on this thread than waking the pool.
        par::chunks_mut(rows, self.w, (x1 - x0) * (y1 - y0) > 150_000, |i, line| {
            row(y0 + i, line)
        });
    }

    /// The proxy pixel rectangle covering document rect `r`.
    pub fn proxy_rect(&self, r: Rect) -> [usize; 4] {
        let s = self.scale;
        let f = |v: i32, o: i32| (f64::from(v - o) / s).max(0.0);
        [
            f(r.x0, self.bounds.x0).floor() as usize,
            f(r.y0, self.bounds.y0).floor() as usize,
            (f(r.x1, self.bounds.x0).ceil() as usize + 1).min(self.w),
            (f(r.y1, self.bounds.y0).ceil() as usize + 1).min(self.h),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The segment walk before #937: every dab, whether or not it can reach the field.
    fn full_walk(f: &mut LiquifyField, s: &LiquifyStroke, a: [f64; 3], b: [f64; 3]) -> Rect {
        f.walk_segment(s, a, b, false)
    }

    /// A w×h straight RGBA8 layer from `f(x, y)`.
    fn image(w: usize, h: usize, f: impl Fn(usize, usize) -> [u8; 4]) -> Vec<u8> {
        let f = &f;
        (0..h)
            .flat_map(|y| (0..w).flat_map(move |x| f(x, y)))
            .collect()
    }

    fn pixel(img: &[u8], w: usize, x: usize, y: usize) -> [u8; 4] {
        img[(y * w + x) * 4..][..4].try_into().unwrap()
    }

    /// Freezes the nodes at x in `[x0, x1)`, y in `[y0, y1]`: what upstream's lasso over that
    /// rectangle sets (its scanline fill takes x0 ≤ x < x1 on the node rows within y0..=y1).
    fn freeze_rect(f: &mut LiquifyField, x0: f64, x1: f64, y0: f64, y1: f64) {
        for j in 0..f.h {
            for i in 0..f.w {
                let p = f.node_pos(i, j);
                if (x0..x1).contains(&p[0]) && (y0..=y1).contains(&p[1]) {
                    f.freeze[j * f.w + i] = 1.0;
                }
            }
        }
    }

    #[test]
    fn a_segment_walks_only_the_dabs_that_can_reach_the_field() {
        // #937: a segment to a far-off point walked one dab per spacing step along its whole
        // length (~3e14 for 1e15 px). Skipping the dabs that can't reach a field node must leave
        // every result bit-identical to the full walk.
        use LiquifyTool::*;
        let bounds = Rect::new(-7, 3, 89, 67);
        let segments: [([f64; 3], [f64; 3]); 10] = [
            ([10.0, 20.0, 1.0], [70.0, 50.0, 0.5]),        // inside
            ([-900.0, 30.0, 1.0], [40.0, 31.0, 1.0]),      // in from far left
            ([40.0, 30.0, 1.0], [40.0, 4000.0, 0.2]),      // out downwards
            ([-500.0, -500.0, 1.0], [600.0, 600.0, 1.0]),  // diagonal through
            ([-300.0, -9.0, 1.0], [300.0, -9.0, 1.0]), // parallel, just above (the brush reaches)
            ([-300.0, -400.0, 1.0], [300.0, -400.0, 1.0]), // parallel, far above (reaches nothing)
            ([88.5, -50.0, 1.0], [88.5, 120.0, 1.0]),  // vertical, along the right edge
            ([200.0, 200.0, 1.0], [201.0, 900.0, 1.0]), // entirely outside
            ([0.0, 0.0, 1.0], [0.25, 0.1, 1.0]),       // shorter than one step
            ([60.0, 40.0, 1.0], [-2000.0, 45.0, 1.0]), // continues the stroke before it, out left
        ];
        for tool in [
            ForwardWarp,
            Reconstruct,
            TwirlCw,
            TwirlCcw,
            Pucker,
            Bloat,
            Freeze,
            Thaw,
        ] {
            for size in [6.0, 40.0] {
                for (a, b) in segments {
                    let s = LiquifyStroke::new(tool, size);
                    let (mut fast, mut full) = (
                        LiquifyField::new(bounds, 2.0),
                        LiquifyField::new(bounds, 2.0),
                    );
                    // Some displacement first, so Reconstruct has something to undo.
                    for f in [&mut fast, &mut full] {
                        f.stroke_segment(
                            &LiquifyStroke::new(ForwardWarp, 30.0),
                            [20.0, 20.0, 1.0],
                            [60.0, 40.0, 1.0],
                        );
                    }
                    let dirty = fast.stroke_segment(&s, a, b);
                    assert_eq!(
                        dirty,
                        full_walk(&mut full, &s, a, b),
                        "{tool:?} {size} {a:?} {b:?}"
                    );
                    assert!(
                        fast.d == full.d && fast.freeze == full.freeze,
                        "{tool:?} {size} {a:?} {b:?}"
                    );
                }
            }
        }
    }

    /// The same path sampled by `parts` pointer events per corner-to-corner leg, unevenly.
    fn sampled(path: &[[f64; 3]], parts: usize) -> Vec<Vec<f64>> {
        let mut out = vec![path[0].to_vec()];
        for w in path.windows(2) {
            let (a, b) = (w[0], w[1]);
            // Uneven steps, as a real pointer gives: t grows quadratically.
            for k in 1..=parts {
                let t = (k * k) as f64 / (parts * parts) as f64;
                out.push((0..3).map(|i| a[i] + (b[i] - a[i]) * t).collect());
            }
        }
        out
    }

    /// #2606: dabs restarted at every pointer event, so a fast mouse (few far-apart events) and a
    /// slow one (many close ones) warped the same path differently. Spacing now runs on along the
    /// whole stroke: any sampling of a path gives the same field.
    #[test]
    fn the_result_does_not_depend_on_how_finely_the_path_was_sampled() {
        use LiquifyTool::*;
        let path = [
            [20.0, 20.0, 1.0],
            [62.0, 26.0, 0.6],
            [70.0, 50.0, 0.9],
            [28.0, 55.0, 1.0],
            [30.0, 30.0, 1.0],
        ];
        for tool in [ForwardWarp, TwirlCw, Bloat, Reconstruct, Freeze] {
            for size in [12.0, 40.0] {
                let field = |parts: usize| {
                    let mut warp = LiquifyStroke::new(ForwardWarp, 30.0);
                    warp.points = vec![vec![30.0, 30.0], vec![50.0, 40.0]];
                    let mut s = LiquifyStroke::new(tool, size);
                    s.points = sampled(&path, parts);
                    // Some displacement first, so Reconstruct has something to change.
                    LiquifyField::from_strokes(bounds(), 2.0, &[warp, s])
                };
                let coarse = field(1);
                for parts in [3, 17, 60] {
                    let fine = field(parts);
                    let dd = coarse
                        .d
                        .iter()
                        .zip(&fine.d)
                        .map(|(p, q)| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()))
                        .fold(0.0f32, f32::max);
                    let df = coarse
                        .freeze
                        .iter()
                        .zip(&fine.freeze)
                        .map(|(p, q)| (p - q).abs())
                        .fold(0.0f32, f32::max);
                    assert!(
                        dd < 1e-3 && df < 1e-4,
                        "{tool:?} {size}px, {parts} events per leg: field differs by {dd} px, mask by {df}"
                    );
                }
            }
        }
    }

    /// A long drag whose events land exactly on dab positions, along a diagonal: rounding must not
    /// add or drop the dab at a segment's end, or the deep displacement a long Forward Warp builds
    /// up differs by a whole dab between a slow and a fast pointer.
    #[test]
    fn a_long_diagonal_drag_gives_one_field_at_any_event_spacing() {
        let field = |step: f64| {
            let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
            s.points = (0..=(300.0 / step) as usize)
                .map(|k| vec![40.0 + k as f64 * step * 0.6, 20.0 + k as f64 * step * 0.8])
                .collect();
            LiquifyField::from_strokes(Rect::new(0, 0, 320, 300), 2.0, &[s])
        };
        let (slow, fast) = (field(2.0), field(60.0));
        let dd = slow
            .d
            .iter()
            .zip(&fast.d)
            .map(|(p, q)| (p[0] - q[0]).abs().max((p[1] - q[1]).abs()))
            .fold(0.0f32, f32::max);
        assert!(
            slow.max_displacement() > 50.0,
            "the drag builds up a deep displacement"
        );
        assert!(dd < 1e-3, "slow and fast pointer fields differ by {dd} px");
    }

    #[test]
    fn a_stroke_interrupted_by_a_jump_restarts_its_spacing() {
        // A segment that does not start where the last one ended begins a new run of dabs there.
        let s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 20.0);
        let mut f = LiquifyField::new(bounds(), 2.0);
        f.stroke_begin(&s, [10.0, 10.0, 1.0]);
        f.stroke_segment(&s, [10.0, 10.0, 1.0], [10.3, 10.0, 1.0]);
        let mut g = LiquifyField::new(bounds(), 2.0);
        f.stroke_segment(&s, [40.0, 30.0, 1.0], [50.0, 30.0, 1.0]);
        g.stroke_begin(&s, [40.0, 30.0, 1.0]);
        g.stroke_segment(&s, [40.0, 30.0, 1.0], [50.0, 30.0, 1.0]);
        assert_eq!(f, g);
        // Hostile points: nothing panics and the next segment starts afresh.
        f.stroke_segment(&s, [f64::NAN, 3.0, 1.0], [4.0, f64::INFINITY, 1.0]);
        f.stroke_segment(&s, [4.0, 3.0, f64::NAN], [9.0, 3.0, 1.0]);
    }

    #[test]
    fn a_segment_to_a_far_off_point_returns() {
        // The issue's request: about 3.3e14 steps before the fix. Also a segment whose length
        // overflows f64.
        let mut f = LiquifyField::new(Rect::new(0, 0, 24, 16), 2.0);
        let s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 30.0);
        let dirty = f.stroke_segment(&s, [0.0, 0.0, 1.0], [1e15, 0.0, 1.0]);
        assert!(!dirty.is_empty() && dirty.x1 <= 64, "{dirty:?}");
        f.stroke_segment(&s, [-1e308, 5.0, 1.0], [1e308, 5.0, 1.0]);
        f.stroke_segment(
            &LiquifyStroke::new(LiquifyTool::Bloat, 30.0),
            [5.0, -1e15, 1.0],
            [5.0, 1e15, 1.0],
        );
    }

    /// Upstream's `sample`: a 96×64 checker in red, ramps in green and blue, opaque.
    fn sample() -> Vec<u8> {
        image(96, 64, |x, y| {
            [
                (((x / 4 + y / 4) % 2) * 255) as u8,
                byte(x as f64 / 96.0),
                byte(y as f64 / 64.0),
                255,
            ]
        })
    }

    fn bounds() -> Rect {
        Rect::new(0, 0, 96, 64)
    }

    #[test]
    fn the_brush_fades_to_nothing_at_its_rim_at_every_density() {
        for density in [0.0, 0.25, 0.5, 0.75, 1.0, f64::NAN, -3.0, 7.0] {
            assert_eq!(
                falloff(0.0, density),
                1.0,
                "full strength at the centre ({density})"
            );
            assert_eq!(falloff(1.0, density), 0.0, "nothing at the rim ({density})");
            // Flat at the rim: the last 2 % of the radius moves at most a couple of percent.
            assert!(
                falloff(0.98f64.powi(2), density) < 0.02,
                "{density}: {}",
                falloff(0.98f64.powi(2), density)
            );
            let mut prev = 1.0;
            for k in 1..=100 {
                let v = falloff(k as f64 / 100.0, density);
                assert!(v <= prev && v >= 0.0, "{density}: not falling at t² = {k}%");
                prev = v;
            }
        }
        // Density widens the strong core: more strength at every radius, as in Photoshop.
        for k in 1..100 {
            let t2 = k as f64 / 100.0;
            assert!(
                falloff(t2, 1.0) >= falloff(t2, 0.5) && falloff(t2, 0.5) >= falloff(t2, 0.0),
                "t² = {t2}"
            );
        }
        assert!(falloff(0.5, 1.0) > 0.5 && falloff(0.5, 0.0) < 0.2);
    }

    /// #2606: the dab's strength used to stop at 1 - Density at its rim, so a Forward Warp moved
    /// everything just inside the brush and nothing just outside, tearing the image along every
    /// dab's circle (a hard-edged block at Density 0, half strength at the default 50).
    #[test]
    fn forward_warp_leaves_the_brush_rim_in_place() {
        // A drag of one dab spacing (a twentieth of the radius): one dab, centred where it started.
        let (r, drag) = (20.0, 1.0);
        for density in [0.0, 50.0, 100.0] {
            let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 2.0 * r);
            s.density = density;
            s.points = vec![vec![48.0, 32.0], vec![48.0 + drag, 32.0]];
            let f = LiquifyField::from_strokes(bounds(), 1.0, &[s]);
            let mut rim = 0;
            for j in 0..f.h {
                for i in 0..f.w {
                    let p = f.node_pos(i, j);
                    let t = (p[0] - 48.0).hypot(p[1] - 32.0) / r;
                    if (0.97..1.0).contains(&t) {
                        let d = f.d[j * f.w + i];
                        assert!(
                            f64::from(d[0].hypot(d[1])) < 0.03 * drag,
                            "density {density}: a node at {t:.3} r moved {d:?}"
                        );
                        rim += 1;
                    }
                }
            }
            assert!(rim > 20, "{rim} rim nodes");
            assert!(
                f.sample(48.0, 32.0)[0] < -0.9 * drag,
                "density {density}: the centre follows the brush"
            );
        }
    }

    #[test]
    fn no_strokes_is_identity() {
        let s = sample();
        let f = LiquifyField::new(bounds(), 2.0);
        assert_eq!(apply_liquify(&s, &f, Interrupt::NONE), Ok(s.clone()));
        // A field with exactly-zero strokes (zero pressure) is still identity.
        let mut st0 = LiquifyStroke::new(LiquifyTool::ForwardWarp, 30.0);
        st0.pressure = 0.0;
        st0.points = vec![vec![40.0, 30.0], vec![60.0, 30.0]];
        let f = LiquifyField::from_strokes(bounds(), 2.0, &[st0]);
        assert!(f.is_identity());
    }

    #[test]
    fn forward_warp_displaces_along_the_stroke() {
        let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
        s.points = vec![vec![40.0, 32.0], vec![52.0, 32.0]];
        let f = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
        // Content moved right: the output at the end point samples from the left.
        let d = f.sample(52.0, 32.0);
        assert!(d[0] < -4.0 && d[1].abs() < 0.5, "{d:?}");
        // A pixel-level check: a marker column follows the stroke.
        let img = image(96, 64, |x, y| {
            let v = if (40..42).contains(&x) && (28..36).contains(&y) {
                255
            } else {
                0
            };
            [v, v, v, 255]
        });
        let out = apply_liquify(&img, &f, Interrupt::NONE).unwrap();
        let brightest = (30..70).max_by_key(|&x| pixel(&out, 96, x, 32)[0]).unwrap();
        assert!(brightest > 44, "marker moved right: {brightest}");
    }

    #[test]
    fn reconstruct_brush_reduces_the_field_and_freeze_protects() {
        let mut s = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
        s.points = vec![vec![30.0, 32.0], vec![45.0, 32.0]];
        let mut f = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
        let before = f.max_displacement();
        let mut r = LiquifyStroke::new(LiquifyTool::Reconstruct, 80.0);
        r.points = vec![vec![40.0, 32.0]; 30];
        f.apply_stroke(&r);
        assert!(
            f.max_displacement() < before * 0.2,
            "{} vs {before}",
            f.max_displacement()
        );
        // Frozen area doesn't move.
        let mut fr = LiquifyStroke::new(LiquifyTool::Freeze, 60.0);
        fr.density = 100.0;
        fr.points = vec![vec![40.0, 32.0]];
        let mut g = LiquifyField::new(bounds(), 2.0);
        g.apply_stroke(&fr);
        assert!(g.freeze_at(40.0, 32.0) > 0.99);
        g.apply_stroke(&s);
        assert!(g.sample(40.0, 32.0)[0].abs() < 1e-3);
        // Thaw unfreezes.
        let mut th = fr.clone();
        th.tool = LiquifyTool::Thaw;
        g.apply_stroke(&th);
        assert!(g.freeze_at(40.0, 32.0) < 0.01);
    }

    #[test]
    fn fully_frozen_pixels_keep_their_mapping_when_an_adjacent_area_is_warped() {
        let src = image(96, 64, |x, y| {
            [byte(x as f64 / 96.0), byte(y as f64 / 64.0), 0, 255]
        });

        let mut mask_only = LiquifyField::new(bounds(), 2.0);
        freeze_rect(&mut mask_only, 0.0, 40.0, 0.0, 64.0);
        assert_eq!(
            apply_liquify(&src, &mask_only, Interrupt::NONE).unwrap(),
            src
        );

        let mut field = LiquifyField::new(bounds(), 2.0);
        let mut first_warp = LiquifyStroke::new(LiquifyTool::ForwardWarp, 40.0);
        first_warp.points = vec![vec![22.0, 32.0], vec![34.0, 32.0]];
        field.apply_stroke(&first_warp);

        freeze_rect(&mut field, 0.0, 40.0, 0.0, 64.0);
        let frozen_pixel = (37, 32);
        assert!(field.freeze_at(frozen_pixel.0 as f64 + 0.5, frozen_pixel.1 as f64 + 0.5) > 0.999);
        let out = |f: &LiquifyField, x: usize, y: usize| {
            pixel(&apply_liquify(&src, f, Interrupt::NONE).unwrap(), 96, x, y)
        };
        let before = out(&field, frozen_pixel.0, frozen_pixel.1);
        assert_ne!(
            before,
            pixel(&src, 96, frozen_pixel.0, frozen_pixel.1),
            "freezing must retain the earlier warp"
        );
        let neighbor_before = out(&field, 43, 32);

        let mut adjacent_warp = LiquifyStroke::new(LiquifyTool::ForwardWarp, 24.0);
        adjacent_warp.points = vec![vec![44.0, 32.0], vec![52.0, 32.0]];
        field.apply_stroke(&adjacent_warp);

        let after = out(&field, frozen_pixel.0, frozen_pixel.1);
        let neighbor_after = out(&field, 43, 32);
        assert_eq!(
            before, after,
            "a fully frozen pixel must keep its pre-neighbor-warp mapping"
        );
        assert_ne!(
            neighbor_before, neighbor_after,
            "an adjacent unfrozen pixel must still move"
        );
    }

    #[test]
    fn twirl_pucker_and_bloat_have_the_right_sense() {
        let at = |tool: LiquifyTool, pts: Vec<Vec<f64>>| {
            let mut s = LiquifyStroke::new(tool, 40.0);
            s.points = pts;
            LiquifyField::from_strokes(bounds(), 2.0, &[s])
        };
        let c = vec![vec![48.0, 32.0]; 10];
        // Pucker pulls content in: the output samples outward (d points away from the centre).
        let f = at(LiquifyTool::Pucker, c.clone());
        assert!(f.sample(58.0, 32.0)[0] > 0.1);
        let f = at(LiquifyTool::Bloat, c.clone());
        assert!(f.sample(58.0, 32.0)[0] < -0.1);
        // Twirl clockwise (y down): content right of centre moves down, so it samples from above.
        let f = at(LiquifyTool::TwirlCw, c.clone());
        assert!(f.sample(58.0, 32.0)[1] < -0.1, "{:?}", f.sample(58.0, 32.0));
        let f = at(LiquifyTool::TwirlCcw, c);
        assert!(f.sample(58.0, 32.0)[1] > 0.1);
    }

    #[test]
    fn replay_is_deterministic() {
        let mut s = LiquifyStroke::new(LiquifyTool::TwirlCw, 50.0);
        s.points = vec![
            vec![48.0, 32.0, 0.5],
            vec![50.0, 33.0, 1.0],
            vec![52.0, 34.0],
        ];
        let a = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
        let b = LiquifyField::from_strokes(bounds(), 2.0, std::slice::from_ref(&s));
        assert_eq!(a, b);
        assert_eq!(
            apply_liquify(&sample(), &a, Interrupt::NONE),
            apply_liquify(&sample(), &b, Interrupt::NONE)
        );
    }

    #[test]
    fn proxy_matches_the_full_render_roughly() {
        let src = sample();
        let mut s = LiquifyStroke::new(LiquifyTool::Bloat, 40.0);
        s.points = vec![vec![48.0, 32.0]; 8];
        let f = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
        let p = ProxyImage::new(&src, bounds(), 96);
        assert_eq!((p.w, p.h, p.scale), (96, 64, 1.0));
        let mut out = vec![[0u8; 4]; p.w * p.h];
        p.render(&f, [0, 0, p.w, p.h], &mut out);
        let full = apply_liquify(&src, &f, Interrupt::NONE).unwrap();
        let mut bad = 0;
        for y in 0..64 {
            for x in 0..96 {
                let q = pixel(&full, 96, x, y);
                let o = out[y * 96 + x];
                if (i32::from(o[1]) - i32::from(q[1])).abs() > 25 {
                    bad += 1;
                }
            }
        }
        assert!(bad < 96 * 64 / 50, "{bad}");
    }

    /// The proxy caps the longer side, and its box filter only averages pixels in the layer.
    #[test]
    fn proxy_caps_its_longer_side() {
        let src = image(10, 3, |_, _| [200, 100, 50, 255]);
        let p = ProxyImage::new(&src, Rect::new(0, 0, 10, 3), 4);
        assert_eq!((p.w, p.h, p.scale), (4, 1, 3.0));
        for q in &p.px {
            assert!((q[3] - 1.0).abs() < 1e-6, "{q:?}");
        }
    }

    /// A cancel set before a long stroke's replay stops it after its opening dab: the field has
    /// that one dab, not the stroke, and the replay reports `Err`.
    #[test]
    fn a_cancel_stops_partway_through_a_stroke() {
        let mut s = LiquifyStroke::new(LiquifyTool::Pucker, 30.0);
        s.points = (0..200)
            .map(|k| vec![20.0 + k as f64 * 0.25, 32.0])
            .collect();
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut cut = LiquifyField::new(bounds(), 2.0);
        assert_eq!(
            cut.apply_stroke_until(&s, Interrupt(Some(&flag))),
            Err(Cancelled)
        );
        let mut one = LiquifyField::new(bounds(), 2.0);
        one.stroke_begin(&s, [20.0, 32.0, 1.0]);
        assert!(!one.is_identity());
        assert_eq!(cut, one, "only the opening dab");
        assert_ne!(cut, LiquifyField::from_strokes(bounds(), 2.0, &[s]));
    }

    /// The tile cut of a cancelled apply: `Err` before any tile runs.
    #[test]
    fn apply_stops_when_cancelled() {
        let mut s = LiquifyStroke::new(LiquifyTool::Bloat, 40.0);
        s.points = vec![vec![48.0, 32.0]; 4];
        let f = LiquifyField::from_strokes(bounds(), 2.0, &[s]);
        let flag = std::sync::atomic::AtomicBool::new(true);
        assert_eq!(
            apply_liquify(&sample(), &f, Interrupt(Some(&flag))),
            Err(Cancelled)
        );
    }

    // Liquify source reads must depend on the mapping, not the output tile phase
    // (upstream crates/algo/tests/it/liquify_source_footprint.rs).

    /// Keeps the document size fixed while moving its edges relative to global tiles.
    fn shifted(shift: i32) -> Rect {
        Rect::new(shift, 0, shift + 1024, 512)
    }

    /// RGB ramps and alternating columns, so missing interpolation taps change the sampled
    /// colours. The layer covers the field bounds, so a translated copy is the same buffer.
    fn ramps() -> Vec<u8> {
        image(1024, 512, |x, y| {
            [
                byte(x as f64 / 1024.0),
                ((x % 2) * 255) as u8,
                byte(y as f64 / 512.0),
                255,
            ]
        })
    }

    /// Matching brush paths in document-relative coordinates, optionally beside a frozen region
    /// that gives the displacement field a sharp boundary (upstream's lasso over x 0..508).
    fn footprint_field(shift: i32, cell: f64, masked: bool) -> LiquifyField {
        let offset = f64::from(shift);
        let mut field = LiquifyField::new(shifted(shift), cell);
        if masked {
            freeze_rect(&mut field, offset, offset + 508.0, 0.0, 512.0);
        }
        let mut warp = LiquifyStroke::new(LiquifyTool::ForwardWarp, 512.0);
        warp.density = 100.0;
        warp.points = vec![vec![offset + 512.0, 256.5], vec![offset + 499.2, 256.5]];
        warp.validate()
            .expect("the fixture uses supported brush parameters");
        for _ in 0..8 {
            field.apply_stroke(&warp);
        }
        field
    }

    /// Checks that a half-tile translation preserves editable output across mesh sizes when
    /// image content and brush paths move together.
    fn check_tile_translation(masked: bool) {
        let src = ramps();
        for cell in [2.0, 4.0] {
            let field = footprint_field(0, cell, masked);
            let shifted_field = footprint_field(128, cell, masked);
            if masked {
                assert!(
                    field.d == shifted_field.d,
                    "translation must preserve the nodal displacement values"
                );
            }
            // The reported pixels lie entirely on the editable side of the mask.
            assert_eq!(field.freeze_at(509.5, 220.5), 0.0);
            assert_eq!(shifted_field.freeze_at(637.5, 220.5), 0.0);
            let output = apply_liquify(&src, &field, Interrupt::NONE).unwrap();
            let shifted_output = apply_liquify(&src, &shifted_field, Interrupt::NONE).unwrap();
            assert_ne!(
                pixel(&output, 1024, 509, 220),
                pixel(&src, 1024, 509, 220),
                "the fixture must actually warp the inspected strip"
            );
            // Translation changes which output tile contains this strip, while preserving the
            // relative source image, brush path and document edges.
            for y in 220..290 {
                for x in 509..512 {
                    assert_eq!(
                        pixel(&output, 1024, x, y),
                        pixel(&shifted_output, 1024, x, y),
                        "meshSize {cell}, masked={masked}: moving the document by half a tile changed ({x}, {y})"
                    );
                }
            }
        }
    }

    #[test]
    fn cubic_source_footprint_is_independent_of_output_tile_phase() {
        check_tile_translation(true);
    }

    #[test]
    fn ordinary_unmasked_warp_is_independent_of_output_tile_phase() {
        check_tile_translation(false);
    }

    #[test]
    fn source_reads_still_clamp_at_the_document_edge() {
        let mut warp = LiquifyStroke::new(LiquifyTool::ForwardWarp, 256.0);
        warp.density = 100.0;
        warp.points = vec![vec![1008.0, 256.5], vec![944.0, 256.5]];
        warp.validate()
            .expect("the edge fixture uses supported brush parameters");
        let field = LiquifyField::from_strokes(shifted(0), 4.0, &[warp]);
        assert!(
            field.sample(1023.5, 256.5)[0] > 4.0,
            "the fixture must sample beyond the right document edge"
        );
        let src = ramps();
        let output = apply_liquify(&src, &field, Interrupt::NONE).unwrap();
        assert_eq!(
            pixel(&output, 1024, 1023, 256),
            pixel(&src, 1024, 1023, 256),
            "an out-of-document source read must repeat the true document edge"
        );
    }
}
