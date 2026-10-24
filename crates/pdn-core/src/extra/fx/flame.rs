//! Ported from PhotoCraft crates/algo/src/render2.rs (Flame: `FlameType`, `FlameStyle`, `FlameShape`, `FlameSpec`, `flame_colour`, `draw_flame`, `flame`; `Spine`, `noise1`, `fbm1`; the Add and Lighten blends of `raster_tile` and `composite`) @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: `Prim` and `Rng` are `fx::render2`'s, and each primitive carries its `Blend` beside it (that port dropped the field); `noise1` hashes through `extra::rng::splitmix64`, as `Rng::sym` does; `composite` blends over a straight RGBA8 layer inside a clip rectangle, in 128-row bands through `extra::par`, polling `Interrupt` per primitive, and returns `None` once cancelled (no selection argument: the effect clips to the selection; no colour-model or depth conversion); the Normal blend is dropped (Flame draws none); `adjust_interval_for_loops` is dropped (the effect's path is an open curve, never a loop); serde derives dropped; `default_flame_path` moved into the tests; `Rect` is `extra::geom::Rect`.

use crate::extra::fx::render2::{Prim, Rng, lerp4};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::par;
use crate::extra::rng::splitmix64;

/// How a primitive combines with what is under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Blend {
    /// Additive light (glows).
    Add,
    /// Per-channel maximum of the premultiplied values (overlapping strands don't build up).
    Lighten,
}

/// Flame placement (Photoshop's six flame types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlameType {
    /// One flame whose spine is the path (path start = flame base).
    #[default]
    OneFlameAlongPath,
    /// Flames at `interval` along the path, rising from its left side.
    MultipleFlamesAlongPath,
    /// Flames at `interval` along the path, pointing along the path's direction.
    MultipleFlamesPathDirections,
    /// Like `multipleFlamesAlongPath` with strongly varying lengths.
    MultipleFlamesVariousLength,
    /// A candle flame at the start of each subpath.
    CandleLight,
    /// Flames at `interval` along the path, all pointing at `angle`.
    MultipleFlamesOneDirection,
}

/// Overall motion of the flame lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlameStyle {
    #[default]
    Normal,
    Violent,
    Flat,
}

/// How the flame lines converge towards the tip.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FlameShape {
    #[default]
    Parallel,
    ToCenter,
    Spread,
    Oval,
    Pointed,
}

/// Flame options (Photoshop's Flame dialog, Basic + Advanced).
#[derive(Clone, Debug, PartialEq)]
pub struct FlameSpec {
    pub flame_type: FlameType,
    /// Flame length in px (1–1000).
    pub length: f32,
    pub randomize_length: bool,
    /// Flame width in px (1–1000).
    pub width: f32,
    /// Direction for `multipleFlamesOneDirection` (degrees, 0 = up, clockwise).
    pub angle: f32,
    /// Spacing between flames along the path in px (1–1000).
    pub interval: f32,
    /// `Some` = custom flame colour; `None` = natural flame colours.
    pub color: Option<[f32; 4]>,
    /// 0–100.
    pub turbulent: f32,
    /// 0–100.
    pub jag: f32,
    /// 0–100 %.
    pub opacity: f32,
    /// Flame lines (complexity) 1–100.
    pub flame_lines: f32,
    /// 0–100: how unevenly the lines start along the flame.
    pub flame_bottom_alignment: f32,
    pub flame_style: FlameStyle,
    pub flame_shape: FlameShape,
    pub randomize_shapes: bool,
    /// Arrangement seed.
    pub seed: u32,
    /// 0 (draft) … 4 (fine): segments per flame line.
    pub quality: u32,
}

impl Default for FlameSpec {
    fn default() -> Self {
        FlameSpec {
            flame_type: FlameType::OneFlameAlongPath,
            length: 150.0,
            randomize_length: false,
            width: 40.0,
            angle: 0.0,
            interval: 60.0,
            color: None,
            turbulent: 25.0,
            jag: 25.0,
            opacity: 75.0,
            flame_lines: 20.0,
            flame_bottom_alignment: 20.0,
            flame_style: FlameStyle::Normal,
            flame_shape: FlameShape::Parallel,
            randomize_shapes: false,
            seed: 0,
            quality: 2,
        }
    }
}

/// Size of the tiles [`composite`] works in.
const CTILE: usize = 128;

/// Draws `prims` (in order) over the straight RGBA8 `w`-wide layer `buf`, inside `clip`. `None`
/// once cancelled (the layer is then partly drawn).
pub fn composite(
    buf: &mut [u8],
    w: usize,
    clip: Rect,
    prims: &[(Prim, Blend)],
    int: Interrupt,
) -> Option<()> {
    let h = buf.len() / (w * 4).max(1);
    let clip = clip.intersect(&Rect::new(0, 0, w as i32, h as i32));
    if clip.is_empty() {
        return (!int.cancelled()).then_some(());
    }
    let (nx, ny) = (w.div_ceil(CTILE), h.div_ceil(CTILE));
    // Bin primitives into tiles (order preserved within a tile).
    let mut bins: Vec<Vec<u32>> = vec![Vec::new(); nx * ny];
    for (k, (p, _)) in prims.iter().enumerate() {
        let b = p.bounds().intersect(&clip);
        if b.is_empty() {
            continue;
        }
        let tile = |v: i32| v as usize / CTILE;
        for ty in tile(b.y0)..=tile(b.y1 - 1) {
            for tx in tile(b.x0)..=tile(b.x1 - 1) {
                bins[ty * nx + tx].push(k as u32);
            }
        }
    }
    par::rows_mut(buf, w * 4 * CTILE, |ty, band| {
        for (tx, idx) in bins[ty * nx..(ty + 1) * nx].iter().enumerate() {
            if idx.is_empty() {
                continue;
            }
            let t = Rect::new(
                (tx * CTILE) as i32,
                (ty * CTILE) as i32,
                ((tx + 1) * CTILE).min(w) as i32,
                ((ty + 1) * CTILE).min(h) as i32,
            )
            .intersect(&clip);
            let Some(paint) = raster_tile(prims, idx, t, int) else {
                return;
            };
            let tw = t.width() as usize;
            for (i, s) in paint.iter().enumerate() {
                let sa = s[3].clamp(0.0, 1.0);
                if sa <= 0.0 {
                    continue;
                }
                let (x, r) = (t.x0 as usize + i % tw, t.y0 as usize % CTILE + i / tw);
                let d = &mut band[(r * w + x) * 4..][..4];
                let inv = 1.0 / s[3].max(1e-6);
                let da = f32::from(d[3]) / 255.0;
                let oa = sa + da * (1.0 - sa);
                for c in 0..3 {
                    let sc = (s[c] * inv).clamp(0.0, 1.0);
                    let dc = f32::from(d[c]) / 255.0;
                    let o = if oa > 1e-7 {
                        (sc * sa + dc * da * (1.0 - sa)) / oa
                    } else {
                        0.0
                    };
                    // As `shim::from_f32`: rounded half up.
                    d[c] = (o * 255.0 + 0.5) as u8;
                }
                d[3] = (oa * 255.0 + 0.5) as u8;
            }
        }
    });
    (!int.cancelled()).then_some(())
}

/// Rasterizes `prims[idx]` (in order) into a premultiplied RGBA buffer covering `t`; `None` once
/// cancelled.
fn raster_tile(
    prims: &[(Prim, Blend)],
    idx: &[u32],
    t: Rect,
    int: Interrupt,
) -> Option<Vec<[f32; 4]>> {
    let w = t.width() as usize;
    let mut buf = vec![[0.0f32; 4]; w * t.height() as usize];
    for &k in idx {
        if int.cancelled() {
            return None;
        }
        let (p, blend) = &prims[k as usize];
        let r = p.bounds().intersect(&t);
        if r.is_empty() {
            continue;
        }
        for y in r.y0..r.y1 {
            let row = (y - t.y0) as usize * w;
            for x in r.x0..r.x1 {
                let (cov, ct) = p.eval(x as f32 + 0.5, y as f32 + 0.5);
                if cov <= 0.0 {
                    continue;
                }
                let c = if p.c0 == p.c1 {
                    p.c0
                } else {
                    lerp4(p.c0, p.c1, ct)
                };
                let a = (c[3] * cov).clamp(0.0, 1.0);
                if a <= 0.0 {
                    continue;
                }
                let d = &mut buf[row + (x - t.x0) as usize];
                match blend {
                    Blend::Add => {
                        d[0] += c[0] * a;
                        d[1] += c[1] * a;
                        d[2] += c[2] * a;
                        d[3] += a * (1.0 - d[3]);
                    }
                    Blend::Lighten => {
                        d[0] = d[0].max(c[0] * a);
                        d[1] = d[1].max(c[1] * a);
                        d[2] = d[2].max(c[2] * a);
                        d[3] = d[3].max(a);
                    }
                }
            }
        }
    }
    Some(buf)
}

/// Smooth 1-D value noise in [-1, 1].
fn noise1(x: f32, seed: u64) -> f32 {
    // Upstream's `Rng(…).sym()`.
    let h = |i: i64| -> f32 {
        let mut s = (i as u64).wrapping_mul(0xD6E8_FEB8_6659_FD93) ^ seed;
        (splitmix64(&mut s) >> 40) as f32 / (1u64 << 24) as f32 * 2.0 - 1.0
    };
    let i = x.floor();
    let f = x - i;
    let i = i as i64;
    let t = f * f * (3.0 - 2.0 * f);
    h(i) + (h(i + 1) - h(i)) * t
}

/// Two octaves of [`noise1`].
fn fbm1(x: f32, seed: u64) -> f32 {
    (noise1(x, seed) + 0.5 * noise1(x * 2.13 + 7.1, seed ^ 0xA5A5)) / 1.5
}

/// A polyline parameterized by arc length.
struct Spine {
    pts: Vec<(f32, f32)>,
    cum: Vec<f32>,
}

impl Spine {
    fn new(pts: Vec<(f32, f32)>) -> Spine {
        let mut cum = vec![0.0];
        for w in pts.windows(2) {
            let l = ((w[1].0 - w[0].0).powi(2) + (w[1].1 - w[0].1).powi(2)).sqrt();
            cum.push(cum.last().copied().unwrap_or(0.0) + l);
        }
        Spine { pts, cum }
    }
    fn len(&self) -> f32 {
        self.cum.last().copied().unwrap_or(0.0)
    }
    /// Point and unit tangent at arc length `s`.
    fn at(&self, s: f32) -> ((f32, f32), (f32, f32)) {
        let n = self.pts.len();
        if n == 0 {
            return ((0.0, 0.0), (1.0, 0.0));
        }
        if n == 1 {
            return (self.pts[0], (0.0, -1.0));
        }
        let s = s.clamp(0.0, self.len());
        let k = match self
            .cum
            .binary_search_by(|v| v.partial_cmp(&s).unwrap_or(std::cmp::Ordering::Less))
        {
            Ok(i) => i.min(n - 2),
            Err(i) => i.saturating_sub(1).min(n - 2),
        };
        let (a, b) = (self.pts[k], self.pts[k + 1]);
        let seg = (self.cum[k + 1] - self.cum[k]).max(1e-6);
        let t = ((s - self.cum[k]) / seg).clamp(0.0, 1.0);
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let l = (dx * dx + dy * dy).sqrt().max(1e-6);
        ((a.0 + dx * t, a.1 + dy * t), (dx / l, dy / l))
    }
}

/// Natural flame colour at position `t` along the flame (0 = base, 1 = tip).
fn flame_colour(t: f32, custom: Option<[f32; 4]>) -> [f32; 4] {
    let a = (1.0 - t).powf(0.7) * 0.9 + 0.1 * (1.0 - t);
    match custom {
        Some(c) => {
            // Hot near-white core → the colour → a darker rim.
            let core = [c[0] * 0.4 + 0.6, c[1] * 0.4 + 0.6, c[2] * 0.4 + 0.6];
            let rim = [c[0] * 0.6, c[1] * 0.6, c[2] * 0.6];
            let k = t.clamp(0.0, 1.0);
            let rgb = if k < 0.35 {
                let u = k / 0.35;
                [
                    core[0] + (c[0] - core[0]) * u,
                    core[1] + (c[1] - core[1]) * u,
                    core[2] + (c[2] - core[2]) * u,
                ]
            } else {
                let u = (k - 0.35) / 0.65;
                [
                    c[0] + (rim[0] - c[0]) * u,
                    c[1] + (rim[1] - c[1]) * u,
                    c[2] + (rim[2] - c[2]) * u,
                ]
            };
            [rgb[0], rgb[1], rgb[2], a * c[3]]
        }
        None => {
            const STOPS: [(f32, [f32; 3]); 5] = [
                (0.0, [1.0, 0.98, 0.85]),
                (0.2, [1.0, 0.86, 0.35]),
                (0.45, [1.0, 0.55, 0.08]),
                (0.75, [0.9, 0.25, 0.02]),
                (1.0, [0.6, 0.08, 0.0]),
            ];
            let k = t.clamp(0.0, 1.0);
            let mut rgb = STOPS[4].1;
            for w in STOPS.windows(2) {
                if k <= w[1].0 {
                    let u = (k - w[0].0) / (w[1].0 - w[0].0);
                    rgb = [
                        w[0].1[0] + (w[1].1[0] - w[0].1[0]) * u,
                        w[0].1[1] + (w[1].1[1] - w[0].1[1]) * u,
                        w[0].1[2] + (w[1].1[2] - w[0].1[2]) * u,
                    ];
                    break;
                }
            }
            [rgb[0], rgb[1], rgb[2], a]
        }
    }
}

/// Upstream's `Prim::with_soft`.
fn soft(mut p: Prim, s: f32) -> Prim {
    p.soft = s;
    p
}

/// One flame: its spine (base → tip), how wide it is, and its random stream.
fn draw_flame(
    out: &mut Vec<(Prim, Blend)>,
    spine: &Spine,
    width: f32,
    spec: &FlameSpec,
    rng: &mut Rng,
) {
    let len = spine.len();
    if len < 1.0 {
        return;
    }
    let lines = (spec.flame_lines.clamp(1.0, 100.0) * 0.6 + 3.0).round() as usize;
    let segs = [6usize, 10, 16, 24, 36][spec.quality.min(4) as usize]
        .max((len / 12.0) as usize)
        .min(160);
    let (mut turb, mut jag) = (
        spec.turbulent.clamp(0.0, 100.0) / 100.0,
        spec.jag.clamp(0.0, 100.0) / 100.0,
    );
    match spec.flame_style {
        FlameStyle::Violent => {
            turb = (turb * 1.8 + 0.2).min(2.0);
            jag = (jag * 1.8 + 0.15).min(2.0);
        }
        FlameStyle::Flat => {
            turb *= 0.35;
            jag *= 0.35;
        }
        FlameStyle::Normal => {}
    }
    let shape = if spec.randomize_shapes {
        [
            FlameShape::Parallel,
            FlameShape::ToCenter,
            FlameShape::Spread,
            FlameShape::Oval,
            FlameShape::Pointed,
        ][(rng.next_u64() % 5) as usize]
    } else {
        spec.flame_shape
    };
    let opacity = spec.opacity.clamp(0.0, 100.0) / 100.0;
    let half = width.max(1.0) / 2.0;
    let seed = rng.next_u64();
    // The whole flame sways; each line adds its own flicker on top.
    let sway = |u: f32| turb * half * 1.6 * fbm1(u * 1.7, seed ^ 0x5A5A) * u.powf(1.4);
    // A soft additive body glow along the spine.
    for k in 0..6 {
        let u = k as f32 / 6.0 * 0.75 + 0.05;
        let ((px, py), (dx, dy)) = spine.at(u * len);
        let o = sway(u);
        let c = flame_colour(u, spec.color);
        let r = half * (1.25 - u) + 2.0;
        let glow = Prim::ellipse(
            (px - dy * o, py + dx * o),
            r,
            r * 1.2,
            dy.atan2(dx),
            [c[0], c[1], c[2], 0.16 * opacity],
            [c[0], c[1] * 0.6, c[2] * 0.3, 0.0],
        );
        out.push((soft(glow, r.max(2.0)), Blend::Add));
    }
    let line_r = (half / lines as f32 * 2.2).clamp(0.8, 10.0);
    for li in 0..lines {
        let lane = if lines == 1 {
            0.0
        } else {
            li as f32 / (lines - 1) as f32 * 2.0 - 1.0
        };
        let lseed = seed ^ (li as u64).wrapping_mul(0x51_7CC1_B727_220A);
        let start = (spec.flame_bottom_alignment.clamp(0.0, 100.0) / 100.0) * rng.f() * 0.4;
        // Outer lines are shorter, giving the flame its tongue shape.
        let reach = (1.0 - 0.55 * lane.abs().powf(1.5)) * rng.range(0.7, 1.0);
        let end = (start + reach * (1.0 - start)).clamp(start + 0.05, 1.0);
        let lr = line_r * rng.range(0.6, 1.3);
        let mut prev: Option<((f32, f32), f32, f32)> = None;
        for k in 0..=segs {
            let u = start + (end - start) * k as f32 / segs as f32;
            let s = u * len;
            let ((px, py), (dx, dy)) = spine.at(s);
            let (nx, ny) = (-dy, dx);
            let spread = match shape {
                FlameShape::Parallel => 1.0 - 0.6 * u,
                FlameShape::ToCenter => 1.0 - u,
                FlameShape::Spread => 1.0 + u,
                FlameShape::Oval => (std::f32::consts::PI * (0.15 + 0.85 * u)).sin() * 1.3,
                FlameShape::Pointed => (1.0 - u).powi(2),
            };
            let flick =
                turb * half * 0.5 * fbm1(s / (len * 0.2 + 6.0) + li as f32 * 0.37, lseed) * u
                    + jag * half * 0.25 * noise1(s / 5.0, lseed ^ 0x77) * u;
            let off = lane * half * spread + sway(u) + flick;
            let p = (px + nx * off, py + ny * off);
            let r = lr * (1.0 - u * 0.75);
            // Lines fade out towards their own tip.
            let fade_t = ((u - start) / (end - start).max(1e-3)).clamp(0.0, 1.0);
            if let Some((q, rq, uq)) = prev {
                // Fade in over the first tenth (a soft base) and out towards the line's tip.
                let f = |c: [f32; 4], uu: f32, t: f32| {
                    [
                        c[0],
                        c[1],
                        c[2],
                        c[3] * opacity
                            * (1.0 - t.powi(3))
                            * ((uu - start) / 0.08).clamp(0.0, 1.0).sqrt(),
                    ]
                };
                let strand = Prim::capsule(
                    q,
                    p,
                    rq,
                    r,
                    f(flame_colour(uq, spec.color), uq, fade_t),
                    f(flame_colour(u, spec.color), u, fade_t),
                );
                out.push((soft(strand, 1.0 + lr * 0.6), Blend::Lighten));
            }
            prev = Some((p, r, u));
        }
    }
}

/// Builds flame primitives. `paths` are flattened polylines in document pixels.
pub fn flame(spec: &FlameSpec, paths: &[Vec<(f32, f32)>]) -> Vec<(Prim, Blend)> {
    let mut out = Vec::new();
    let mut rng = Rng::new(spec.seed, 0xF1A3);
    let length = spec.length.clamp(1.0, 1000.0);
    let width = spec.width.clamp(1.0, 1000.0);
    let interval = spec.interval.clamp(1.0, 1000.0);
    let pick_len = |rng: &mut Rng, var: f32| {
        if spec.randomize_length || var > 0.0 {
            length * rng.range(1.0 - var.max(0.4), 1.0 + var.max(0.4) * 0.5)
        } else {
            length
        }
    };
    for path in paths.iter().filter(|p| !p.is_empty()) {
        let spine = Spine::new(path.clone());
        let total = spine.len();
        match spec.flame_type {
            FlameType::OneFlameAlongPath if path.len() >= 2 && total >= 1.0 => {
                draw_flame(&mut out, &spine, width, spec, &mut rng)
            }
            FlameType::CandleLight | FlameType::OneFlameAlongPath => {
                // A candle: a short teardrop rising straight up from the start.
                let base = path[0];
                let h = length.min(400.0);
                let candle = Spine::new(vec![base, (base.0, base.1 - h)]);
                let c = FlameSpec {
                    flame_shape: FlameShape::Oval,
                    turbulent: spec.turbulent * 0.3,
                    jag: spec.jag * 0.2,
                    ..spec.clone()
                };
                draw_flame(&mut out, &candle, (width * 0.6).max(4.0), &c, &mut rng);
                let core = flame_colour(0.05, spec.color);
                let o = 0.9 * spec.opacity.clamp(0.0, 100.0) / 100.0;
                let glow = Prim::ellipse(
                    (base.0, base.1 - h * 0.28),
                    (width * 0.12).max(1.5),
                    h * 0.22,
                    0.0,
                    [core[0], core[1], core[2], o],
                    [core[0], core[1], core[2], 0.0],
                );
                out.push((soft(glow, 2.0), Blend::Add));
            }
            _ => {
                let c = (total / interval).floor();
                let count = if c.is_finite() && c >= 0.0 {
                    (c as usize).saturating_add(1).min(2048)
                } else {
                    1
                };
                for k in 0..count {
                    if out.len() >= 65536 {
                        break;
                    }
                    let s = k as f32 * interval;
                    let ((x, y), (dx, dy)) = spine.at(s);
                    let dir = match spec.flame_type {
                        FlameType::MultipleFlamesPathDirections => (dx, dy),
                        FlameType::MultipleFlamesOneDirection => {
                            let a = spec.angle.to_radians();
                            (a.sin(), -a.cos())
                        }
                        // Left of the path direction (up for a left-to-right path).
                        _ => (dy, -dx),
                    };
                    let var = if spec.flame_type == FlameType::MultipleFlamesVariousLength {
                        0.7
                    } else {
                        0.0
                    };
                    let l = pick_len(&mut rng, var);
                    let fl = Spine::new(vec![(x, y), (x + dir.0 * l, y + dir.1 * l)]);
                    draw_flame(&mut out, &fl, width, spec, &mut rng);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Upstream's path for a document without one: a gentle arc across the lower part.
    fn default_flame_path(canvas: Rect) -> Vec<(f32, f32)> {
        let (w, h) = (canvas.width() as f32, canvas.height() as f32);
        (0..=32)
            .map(|k| {
                let t = k as f32 / 32.0;
                (
                    canvas.x0 as f32 + w * (0.15 + 0.7 * t),
                    canvas.y0 as f32 + h * (0.8 - 0.08 * (std::f32::consts::PI * t).sin()),
                )
            })
            .collect()
    }

    /// Upstream `renderers_are_deterministic_and_seeded` (its Flame part): every flame type
    /// draws something along the default path.
    #[test]
    fn renderers_are_deterministic_and_seeded() {
        let canvas = Rect::new(0, 0, 400, 300);
        let path = default_flame_path(canvas);
        for t in [
            FlameType::OneFlameAlongPath,
            FlameType::MultipleFlamesAlongPath,
            FlameType::MultipleFlamesPathDirections,
            FlameType::MultipleFlamesVariousLength,
            FlameType::CandleLight,
            FlameType::MultipleFlamesOneDirection,
        ] {
            assert!(
                !flame(
                    &FlameSpec {
                        flame_type: t,
                        ..Default::default()
                    },
                    std::slice::from_ref(&path)
                )
                .is_empty(),
                "{t:?}"
            );
        }
    }

    /// Upstream engine `flame_follows_the_work_path`: a vertical path at x = 20 of a 160 × 120
    /// document (width 16, length 60); what is drawn (the primitives' bounds inside the document,
    /// as upstream's `composite` reports them) hugs the path.
    #[test]
    fn flame_follows_the_work_path() {
        let spec = FlameSpec {
            width: 16.0,
            length: 60.0,
            ..Default::default()
        };
        let prims = flame(&spec, &[vec![(20.0, 110.0), (20.0, 20.0)]]);
        let doc = Rect::new(0, 0, 160, 120);
        let b = prims.iter().fold(Rect::EMPTY, |u, (p, _)| {
            u.union(&p.bounds().intersect(&doc))
        });
        assert!(!b.is_empty());
        assert!(
            b.x0 < 20 && b.x1 < 60,
            "flame bounds {b:?} hug the path at x=20"
        );
    }

    /// The primitive list is bounded: upstream stops starting flames past 65 536 primitives, and
    /// one flame adds at most 6 glows and 63 lines of 160 segments.
    #[test]
    fn flame_primitive_count_is_bounded() {
        let spec = FlameSpec {
            flame_type: FlameType::MultipleFlamesAlongPath,
            interval: 1.0,
            flame_lines: 100.0,
            quality: 4,
            length: 1000.0,
            ..Default::default()
        };
        let path = vec![(0.0, 0.0), (20_000.0, 0.0)];
        let n = flame(&spec, &[path]).len();
        assert!((65_536..65_536 + 6 + 63 * 160).contains(&n), "{n}");
    }

    /// Additive glows sum their premultiplied colour; Lighten strands keep the per-channel
    /// maximum, so overlapping strands don't build up. The tile is then drawn source-over.
    #[test]
    fn composite_adds_glows_and_lightens_strands() {
        let disc = |c: [f32; 4]| Prim::disc((8.0, 8.0), 6.0, c);
        let red = [1.0, 0.0, 0.0, 0.5];
        let all = Rect::new(0, 0, 16, 16);
        let at = |buf: &[u8]| -> [u8; 4] { buf[(8 * 16 + 8) * 4..][..4].try_into().unwrap() };
        let mut buf = vec![0u8; 16 * 16 * 4];
        let add = [(disc(red), Blend::Add), (disc(red), Blend::Add)];
        composite(&mut buf, 16, all, &add, Interrupt::NONE).unwrap();
        // Premultiplied red 0.5 + 0.5 = 1 over alpha 0.5 + 0.5 · 0.5 = 0.75: straight red clamps
        // to 1.
        assert_eq!(at(&buf), [255, 0, 0, 191]);
        let mut buf = vec![0u8; 16 * 16 * 4];
        let lighten = [(disc(red), Blend::Lighten), (disc(red), Blend::Lighten)];
        composite(&mut buf, 16, all, &lighten, Interrupt::NONE).unwrap();
        assert_eq!(at(&buf), [255, 0, 0, 128]);
        // Over an opaque blue pixel: source-over, alpha stays 1.
        let mut buf = [0u8, 0, 255, 255].repeat(16 * 16);
        composite(&mut buf, 16, all, &lighten, Interrupt::NONE).unwrap();
        assert_eq!(at(&buf), [128, 0, 128, 255]);
    }

    /// Nothing outside the clip changes, and a cancelled run reports it.
    #[test]
    fn composite_stays_inside_the_clip_and_stops_when_cancelled() {
        let prims = [(Prim::disc((8.0, 8.0), 8.0, [1.0; 4]), Blend::Add)];
        let clip = Rect::new(4, 4, 12, 12);
        let mut buf = vec![0u8; 16 * 16 * 4];
        composite(&mut buf, 16, clip, &prims, Interrupt::NONE).unwrap();
        for (i, p) in buf.chunks(4).enumerate() {
            let (x, y) = ((i % 16) as i32, (i / 16) as i32);
            assert_eq!(p[3] == 255, clip.contains(x, y), "({x}, {y}) {p:?}");
        }
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut buf = vec![0u8; 16 * 16 * 4];
        let all = Rect::new(0, 0, 16, 16);
        assert!(composite(&mut buf, 16, all, &prims, Interrupt(Some(&flag))).is_none());
    }
}
