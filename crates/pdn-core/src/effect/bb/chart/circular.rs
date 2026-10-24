//! BB spec: effects/render-chart-or-graph.md "Per chart type" (Radar … Pyramid), "3-D"; common.md §5.10
//!
//! The pie-like and circular charts' geometry: slice angles, the pie's disc (flat, or tilted by
//! R194 Q50), Funnel and Pyramid bands and their solids, and the Radar/Polar circle. R149 Q48
//! (`conventions::CHART_NONPOSITIVE_RADIAL_EMPTY`) gives nonpositive radial values no extent.
use super::axes::Scale;
use super::model::ChartType;
use super::three_d::{self, FRUSTUM, polygon};
use crate::effect::bb::conventions::{
    CHART_3D_OBLIQUE_RIGHT_ANGLE_V1 as VIEW, CHART_NONPOSITIVE_RADIAL_EMPTY,
};
use tiny_skia::{FillRule, Path, PathBuilder};

/// "Pie": an exploded slice moves out by 10 % of the diameter, 0.2 radii.
pub const EXPLODE: f64 = 0.2;
/// "Doughnut": the hole is 60 % of the radius.
pub const DOUGHNUT_HOLE: f64 = 0.6;
/// "Funnel": the neck is 5 % wide over the last 5 % of the height.
pub const FUNNEL_NECK: f64 = 0.05;

/// "Pie": each slice's start and sweep in degrees, clockwise from 3 o'clock, proportional to
/// the absolute values. A total of 0 draws no pie: no slices.
pub fn slice_angles(values: &[f64]) -> Vec<(f64, f64)> {
    let total = |scale: f64| values.iter().map(|v| v.abs() * scale).sum::<f64>();
    // R145: n finite values each scaled by 2^-(⌈log2 n⌉ + 1) cannot overflow their sum.
    let scale = match total(1.0).is_finite() {
        true => 1.0,
        false => 0.5f64.powi((values.len() as f64).log2().ceil() as i32 + 1),
    };
    let sum = total(scale);
    if sum == 0.0 {
        return Vec::new();
    }
    let mut before = 0.0;
    values
        .iter()
        .map(|v| {
            let w = v.abs() * scale;
            let slice = (before / sum * 360.0, w / sum * 360.0);
            before += w;
            slice
        })
        .collect()
}

/// A slice's offset in radii: out along its bisector when exploded.
pub fn push((start, sweep): (f64, f64), exploded: bool) -> [f64; 2] {
    if !exploded {
        return [0.0; 2];
    }
    let (s, c) = (start + sweep / 2.0).to_radians().sin_cos();
    [EXPLODE * c, EXPLODE * s]
}

/// "Funnel", "Pyramid": each point's band of depths (0 the top, 1 the bottom), its height its
/// share of the Q48-clamped values (normalised by the largest first: no overflow), the Funnel's
/// first point on top and the Pyramid's at the bottom. Empty when no value is positive.
pub fn bands(values: &[f64], pyramid: bool) -> Vec<(f64, f64)> {
    let v: Vec<f64> = values
        .iter()
        .map(|&v| CHART_NONPOSITIVE_RADIAL_EMPTY(v))
        .collect();
    let max = v.iter().copied().fold(0.0, f64::max);
    if max <= 0.0 {
        return Vec::new();
    }
    let sum: f64 = v.iter().map(|x| x / max).sum();
    let mut at = 0.0;
    v.iter()
        .map(|x| {
            let band = (at, (at + x / max / sum).min(1.0));
            at = band.1;
            match pyramid {
                true => (1.0 - band.1, 1.0 - band.0),
                false => band,
            }
        })
        .collect()
}

/// The outline's half-width at depth `u`, a fraction of half the area's width: the Funnel
/// narrows straight from full width to its neck, the Pyramid widens from its apex.
pub fn half_width(t: ChartType, u: f64) -> f64 {
    match t {
        ChartType::Pyramid => u,
        _ => (1.0 - u).max(FUNNEL_NECK),
    }
}

fn unit(deg: f64) -> [f64; 2] {
    let (s, c) = deg.to_radians().sin_cos();
    [c, s]
}

/// Cubic pieces (≤ 90°) of the unit circle from `start` sweeping `sweep` degrees (increasing
/// angles turn clockwise on screen), through the affine `map`; the builder is at the start.
fn arc_to(pb: &mut PathBuilder, start: f64, sweep: f64, map: &impl Fn([f64; 2]) -> [f64; 2]) {
    let n = (sweep.abs() / 90.0).ceil().max(1.0);
    let step = sweep / n;
    let k = 4.0 / 3.0 * (step.to_radians() / 4.0).tan();
    for i in 0..n as usize {
        let (a, b) = (
            unit(start + step * i as f64),
            unit(start + step * (i + 1) as f64),
        );
        let c1 = map([a[0] - k * a[1], a[1] + k * a[0]]);
        let c2 = map([b[0] + k * b[1], b[1] - k * b[0]]);
        let e = map(b);
        pb.cubic_to(
            c1[0] as f32,
            c1[1] as f32,
            c2[0] as f32,
            c2[1] as f32,
            e[0] as f32,
            e[1] as f32,
        );
    }
}

fn start_at(pb: &mut PathBuilder, p: [f64; 2]) {
    pb.move_to(p[0] as f32, p[1] as f32);
}

fn line_to(pb: &mut PathBuilder, p: [f64; 2]) {
    pb.line_to(p[0] as f32, p[1] as f32);
}

/// A pie's placement: its centre's screen point and its radius, flat or tilted (3-D: lying in
/// the model x–z plane, `disc_thickness` of its diameter thick, so R194's oblique projection
/// keeps its 3 o'clock radius level and slants the rest along the depth vector).
#[derive(Clone, Copy, Debug)]
pub struct Disc {
    pub centre: [f64; 2],
    pub radius: f64,
    pub tilted: bool,
}

impl Disc {
    /// "Sized to the smaller side": `size` of the largest disc that fits `area`, centred.
    pub fn fit([l, t, r, b]: [f64; 4], size: f64, tilted: bool) -> Self {
        let (lo, hi) = Disc {
            centre: [0.0; 2],
            radius: 1.0,
            tilted,
        }
        .bounds();
        let radius = size
            * ((r - l) / (hi[0] - lo[0]))
                .min((b - t) / (hi[1] - lo[1]))
                .max(0.0);
        let centre = [
            (l + r) / 2.0 - radius * (lo[0] + hi[0]) / 2.0,
            (t + b) / 2.0 - radius * (lo[1] + hi[1]) / 2.0,
        ];
        Disc {
            centre,
            radius,
            tilted,
        }
    }

    /// The thickness in radii.
    fn thickness(&self) -> f64 {
        if self.tilted {
            2.0 * VIEW.disc_thickness
        } else {
            0.0
        }
    }

    /// The projected bounds of this disc at radius 1 about 0: its rim, and its top face above.
    fn bounds(&self) -> ([f64; 2], [f64; 2]) {
        if !self.tilted {
            return ([-1.0; 2], [1.0; 2]);
        }
        let (x, z) = (
            (VIEW.project)([1.0, 0.0, 0.0]),
            (VIEW.project)([0.0, 0.0, 1.0]),
        );
        let up = (VIEW.project)([0.0, self.thickness(), 0.0]);
        let amp = [x[0].hypot(z[0]), x[1].hypot(z[1])];
        let lo = [0, 1].map(|i| up[i].min(0.0) - amp[i]);
        let hi = [0, 1].map(|i| up[i].max(0.0) + amp[i]);
        (lo, hi)
    }

    /// The affine map of the unit circle (x right, y clockwise) onto this disc's circle of `r`
    /// radii, pushed by `push` radii, on its top face (or, tilted, its bottom).
    pub fn map(&self, r: f64, push: [f64; 2], top: bool) -> impl Fn([f64; 2]) -> [f64; 2] {
        let d = *self;
        let y = if top { d.thickness() } else { 0.0 };
        move |u: [f64; 2]| {
            let (a, b) = (
                d.radius * (r * u[0] + push[0]),
                d.radius * (r * u[1] + push[1]),
            );
            if !d.tilted {
                return [d.centre[0] + a, d.centre[1] + b];
            }
            // Clockwise seen from above: toward the viewer, −z.
            let q = (VIEW.project)([a, d.radius * y, -b]);
            [d.centre[0] + q[0], d.centre[1] + q[1]]
        }
    }

    /// The screen point `r` radii out at `deg` (clockwise from 3 o'clock), pushed by `push`.
    pub fn at(&self, r: f64, deg: f64, push: [f64; 2], top: bool) -> [f64; 2] {
        self.map(r, push, top)(unit(deg))
    }

    /// A sector (or ring sector with a `hole`) of a face.
    fn sector(&self, start: f64, sweep: f64, hole: f64, push: [f64; 2], top: bool) -> Option<Path> {
        if sweep <= 0.0 {
            return None;
        }
        let (outer, inner) = (self.map(1.0, push, top), self.map(hole, push, top));
        let mut pb = PathBuilder::new();
        start_at(&mut pb, outer(unit(start)));
        arc_to(&mut pb, start, sweep, &outer);
        line_to(&mut pb, inner(unit(start + sweep)));
        if hole > 0.0 {
            arc_to(&mut pb, start + sweep, -sweep, &inner);
        }
        pb.close();
        pb.finish()
    }

    /// The curved side at `r` radii between the faces, from `start` sweeping `sweep`.
    fn band(&self, start: f64, sweep: f64, r: f64, push: [f64; 2]) -> Option<Path> {
        let (top, bottom) = (self.map(r, push, true), self.map(r, push, false));
        let mut pb = PathBuilder::new();
        start_at(&mut pb, top(unit(start)));
        arc_to(&mut pb, start, sweep, &top);
        line_to(&mut pb, bottom(unit(start + sweep)));
        arc_to(&mut pb, start + sweep, -sweep, &bottom);
        pb.close();
        pb.finish()
    }
}

/// The parts of the angles [a, a + sweep] inside the half-turn window from `lo`, modulo 360.
fn within(a: f64, sweep: f64, lo: f64) -> Vec<(f64, f64)> {
    (-2..=2)
        .filter_map(|k| {
            let w = lo + 360.0 * k as f64;
            let (s, e) = (a.max(w), (a + sweep).min(w + 180.0));
            (e > s).then_some((s, e - s))
        })
        .collect()
}

/// A path and the colour and rule it is filled with.
pub type Shape = (Option<Path>, u32, FillRule);

/// Pie and Doughnut slices (`hole` radii open): flat sectors in point order; tilted (R194 Q50)
/// the visible cut faces where an explosion opens a gap, then the visible rims and inner walls
/// (sides, ×0.8), then the tops (×1.2).
pub fn pie_shapes(
    d: &Disc,
    slices: &[(f64, f64)],
    pushes: &[[f64; 2]],
    colours: &[u32],
    hole: f64,
) -> Vec<Shape> {
    let items = slices.iter().zip(pushes).zip(colours);
    if !d.tilted {
        return items
            .map(|((&(a, sweep), &push), &rgb)| {
                (d.sector(a, sweep, hole, push, true), rgb, FillRule::Winding)
            })
            .collect();
    }
    let v = three_d::toward_viewer();
    // The rim's outward normal (cos θ, 0, −sin θ) faces the viewer within 90° of this angle.
    let facing = (-v[2]).atan2(v[0]).to_degrees();
    // A cut face shows only beside a gap: its slice or the next drawn slice across it pushed.
    let drawn: Vec<usize> = (0..slices.len().min(pushes.len()))
        .filter(|&i| slices[i].1 > 0.0)
        .collect();
    let pushed = |i: usize| pushes[i] != [0.0; 2];
    let mut gap = vec![[false; 2]; slices.len()];
    for (j, &i) in drawn.iter().enumerate() {
        let n = drawn.len();
        let (before, after) = (drawn[(j + n - 1) % n], drawn[(j + 1) % n]);
        gap[i] = [pushed(i) || pushed(before), pushed(i) || pushed(after)];
    }
    let (mut cuts, mut sides, mut tops) = (Vec::new(), Vec::new(), Vec::new());
    for (i, ((&(a, sweep), &push), &rgb)) in items.enumerate() {
        if sweep <= 0.0 {
            continue;
        }
        if sweep < 360.0 {
            for ((deg, out), open) in [(a, 1.0), (a + sweep, -1.0)].into_iter().zip(gap[i]) {
                if !open {
                    continue;
                }
                let (s, c) = deg.to_radians().sin_cos();
                let n = [out * s, 0.0, out * c];
                if n[0] * v[0] + n[2] * v[2] > 0.0 {
                    let u = unit(deg);
                    let at = |r: f64, top: bool| d.map(r, push, top)(u);
                    let face = [
                        at(hole, true),
                        at(1.0, true),
                        at(1.0, false),
                        at(hole, false),
                    ];
                    let rgb = three_d::shade(rgb, three_d::face_shade(n));
                    cuts.push((polygon(&face), rgb, FillRule::Winding));
                }
            }
        }
        for (lo, r) in [(facing - 90.0, 1.0), (facing + 90.0, hole)] {
            if r > 0.0 {
                for (b, part) in within(a, sweep, lo) {
                    let rgb = three_d::shade(rgb, VIEW.side);
                    sides.push((d.band(b, part, r, push), rgb, FillRule::Winding));
                }
            }
        }
        let rgb = three_d::shade(rgb, VIEW.top);
        tops.push((d.sector(a, sweep, hole, push, true), rgb, FillRule::Winding));
    }
    cuts.into_iter().chain(sides).chain(tops).collect()
}

/// The 3-D fit (scale, offset) of a Funnel or Pyramid body into `area`: its top section half the
/// area's width across, its height the area's.
pub fn stack_fit(t: ChartType, [l, top, r, b]: [f64; 4]) -> (f64, [f64; 2]) {
    let (r0, h) = ((r - l) / 2.0, b - top);
    let (x, z) = (
        (VIEW.project)([1.0, 0.0, 0.0]),
        (VIEW.project)([0.0, 0.0, 1.0]),
    );
    // A section's half-extents per unit: a circle's ellipse, or a square's corners.
    let extent = match t {
        ChartType::Pyramid => [x[0].abs() + z[0].abs(), x[1].abs() + z[1].abs()],
        _ => [x[0].hypot(z[0]), x[1].hypot(z[1])],
    };
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for (u, y) in [(0.0, h), (1.0, 0.0)] {
        let (c, w) = ((VIEW.project)([0.0, y, 0.0]), half_width(t, u) * r0);
        for i in 0..2 {
            lo[i] = lo[i].min(c[i] - w * extent[i]);
            hi[i] = hi[i].max(c[i] + w * extent[i]);
        }
    }
    three_d::fit((lo, hi), [l, top, r, b])
}

/// The convex hull of two round sections (radius, height) of a Funnel's body through `fit`, less
/// the upper section `a` (which the stack above covers; filled even-odd): in the plane where both
/// are circles, the arcs on their far sides joined by the outer tangents.
fn hull((k, o): (f64, [f64; 2]), (ra, ya): (f64, f64), (rb, yb): (f64, f64)) -> Option<Path> {
    let p = VIEW.project;
    let (x, y, z) = (p([1.0, 0.0, 0.0]), p([0.0, 1.0, 0.0]), p([0.0, 0.0, 1.0]));
    // Screen = o + k·(height·y + M·(radius·unit)), M's columns x and −z.
    let m = [[k * x[0], -k * z[0]], [k * x[1], -k * z[1]]];
    let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
    if det == 0.0 {
        return None;
    }
    let screen = |q: [f64; 2]| {
        [
            o[0] + m[0][0] * q[0] + m[0][1] * q[1],
            o[1] + m[1][0] * q[0] + m[1][1] * q[1],
        ]
    };
    let centre = |h: f64| {
        let s = [k * h * y[0], k * h * y[1]];
        [
            (m[1][1] * s[0] - m[0][1] * s[1]) / det,
            (m[0][0] * s[1] - m[1][0] * s[0]) / det,
        ]
    };
    let (ca, cb) = (centre(ya), centre(yb));
    let circle =
        |c: [f64; 2], r: f64| move |u: [f64; 2]| screen([c[0] + r * u[0], c[1] + r * u[1]]);
    let d = [cb[0] - ca[0], cb[1] - ca[1]];
    let dist = d[0].hypot(d[1]);
    let mut pb = PathBuilder::new();
    if dist <= (ra - rb).abs() {
        let (c, r) = if ra >= rb { (ca, ra) } else { (cb, rb) };
        let map = circle(c, r);
        start_at(&mut pb, map([1.0, 0.0]));
        arc_to(&mut pb, 0.0, 360.0, &map);
    } else {
        let e = [d[0] / dist, d[1] / dist];
        let (sigma, deg) = ((ra - rb) / dist, |n: [f64; 2]| {
            n[1].atan2(n[0]).to_degrees()
        });
        let tau = (1.0 - sigma * sigma).sqrt();
        let n1 = deg([sigma * e[0] - tau * e[1], sigma * e[1] + tau * e[0]]);
        let n2 = deg([sigma * e[0] + tau * e[1], sigma * e[1] - tau * e[0]]);
        // The arc from `a` to `b` that passes the direction `via`.
        let sweep = |a: f64, b: f64, via: f64| {
            let s = (b - a).rem_euclid(360.0);
            if (via - a).rem_euclid(360.0) < s {
                s
            } else {
                s - 360.0
            }
        };
        let (map_a, map_b) = (circle(ca, ra), circle(cb, rb));
        start_at(&mut pb, map_a(unit(n1)));
        arc_to(&mut pb, n1, sweep(n1, n2, deg([-e[0], -e[1]])), &map_a);
        line_to(&mut pb, map_b(unit(n2)));
        arc_to(&mut pb, n2, sweep(n2, n1, deg(e)), &map_b);
    }
    pb.close();
    let top = circle(ca, ra);
    start_at(&mut pb, top([1.0, 0.0]));
    arc_to(&mut pb, 0.0, 360.0, &top);
    pb.close();
    pb.finish()
}

/// Funnel and Pyramid segments in `area` from their `bands`: flat polygons; tilted (R194 Q50,
/// through `fit`) round or square sections stacked from the bottom up, each drawing only what
/// the segment above leaves visible: round sides ×0.8 and the Funnel's top lit ×1.2, square
/// faces shaded by their normals (the Pyramid's tops always lie under the next segment).
pub fn stack_shapes(
    t: ChartType,
    area: [f64; 4],
    bands: &[(f64, f64)],
    colours: &[u32],
    fit: Option<(f64, [f64; 2])>,
) -> Vec<Shape> {
    let [l, top, r, b] = area;
    let (r0, h, cx) = ((r - l) / 2.0, b - top, (l + r) / 2.0);
    let kink = 1.0 - FUNNEL_NECK;
    // A band's outline depths, with the Funnel's neck corner when inside it.
    let depths = |(u0, u1): (f64, f64)| {
        let neck = t != ChartType::Pyramid && u0 < kink && kink < u1;
        let mut d = vec![u0];
        d.extend(neck.then_some(kink));
        d.push(u1);
        d
    };
    let items = bands
        .iter()
        .zip(colours)
        .filter(|(band, _)| band.1 > band.0);
    let Some((k, o)) = fit else {
        return items
            .map(|(&band, &rgb)| {
                let right: Vec<_> = depths(band)
                    .into_iter()
                    .map(|u| [cx + half_width(t, u) * r0, top + u * h])
                    .collect();
                let left = right.iter().rev().map(|p| [2.0 * cx - p[0], p[1]]);
                let outline: Vec<_> = right.iter().copied().chain(left).collect();
                (polygon(&outline), rgb, FillRule::Winding)
            })
            .collect();
    };
    let at = |p: [f64; 3]| {
        let q = (VIEW.project)(p);
        [o[0] + k * q[0], o[1] + k * q[1]]
    };
    let section = |u: f64| (half_width(t, u) * r0, h * (1.0 - u));
    // Bottom up: lower segments are farther, and each upper one covers the top it rests on.
    let mut order: Vec<_> = items.collect();
    order.sort_by(|a, b| b.0.1.total_cmp(&a.0.1));
    let mut out = Vec::new();
    for &(&band, &rgb) in &order {
        let d = depths(band);
        for w in d.windows(2).rev() {
            let ((wa, ya), (wb, yb)) = (section(w[0]), section(w[1]));
            if t == ChartType::Pyramid {
                let square = |w: f64, y: f64| [[-w, y, -w], [w, y, -w], [w, y, w], [-w, y, w]];
                let ([p, q, r, s], [e, f, g, hh]) = (square(wb, yb), square(wa, ya));
                let sides = [FRUSTUM[0], FRUSTUM[2], FRUSTUM[3], FRUSTUM[4], FRUSTUM[5]];
                for face in three_d::convex_faces(&[p, q, r, s, e, f, g, hh], &sides) {
                    let rgb = three_d::shade(rgb, face.shade);
                    out.push((three_d::path(&face, at), rgb, FillRule::Winding));
                }
            } else {
                let rgb = three_d::shade(rgb, VIEW.side);
                out.push((hull((k, o), (wa, ya), (wb, yb)), rgb, FillRule::EvenOdd));
            }
        }
    }
    // The Funnel's top face, on its top segment.
    let top = order
        .last()
        .filter(|(band, _)| t != ChartType::Pyramid && band.0 == 0.0);
    if let Some(&(_, &rgb)) = top {
        let map = |u: [f64; 2]| at([r0 * u[0], h, -r0 * u[1]]);
        let mut pb = PathBuilder::new();
        start_at(&mut pb, map([1.0, 0.0]));
        arc_to(&mut pb, 0.0, 360.0, &map);
        pb.close();
        let rgb = three_d::shade(rgb, VIEW.top);
        out.push((pb.finish(), rgb, FillRule::Winding));
    }
    out
}

/// A circular chart, R149 Q45's Radar or Polar scale group: its drawn series, its spoke count
/// (Radar: the most points of its series), its value scale from 0 and its circle on screen.
#[derive(Clone, Debug)]
pub struct Radial {
    pub polar: bool,
    pub series: Vec<usize>,
    pub spokes: usize,
    pub scale: Scale,
    pub centre: [f64; 2],
    pub radius: f64,
}

impl Radial {
    /// The screen point `deg` clockwise from the top at value `v`; Q48 puts nonpositive values
    /// at the centre.
    // approximation: Polar's 0° direction, assumed at the top and clockwise (render-chart-or-graph.md)
    pub fn at(&self, deg: f64, v: f64) -> [f64; 2] {
        let r = self.radius * (CHART_NONPOSITIVE_RADIAL_EMPTY(v) / self.scale.hi);
        let (s, c) = deg.to_radians().sin_cos();
        [self.centre[0] + r * s, self.centre[1] - r * c]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pie: slices proportional to the absolute values, clockwise from 3 o'clock.
    #[test]
    fn slices_run_clockwise_from_three_o_clock() {
        let want = [(0.0, 22.5), (22.5, 56.25), (78.75, 281.25)];
        assert_eq!(slice_angles(&[4.0, 10.0, 50.0]), want);
        assert_eq!(slice_angles(&[-4.0, 10.0, -50.0]), want, "absolute values");
        assert!(
            slice_angles(&[0.0, -0.0]).is_empty(),
            "a total of 0 draws no pie"
        );
        assert!(slice_angles(&[]).is_empty());
        assert_eq!(slice_angles(&[0.0, 3.0]), [(0.0, 0.0), (0.0, 360.0)]);
        // R145: extreme finite values neither overflow nor vanish.
        let quarters = [(0.0, 90.0), (90.0, 90.0), (180.0, 90.0), (270.0, 90.0)];
        assert_eq!(slice_angles(&[f64::MAX; 4]), quarters);
        assert_eq!(slice_angles(&[5e-324; 4]), quarters);
    }

    /// Funnel and Pyramid heights: Q48 clamps nonpositive values to no height.
    #[test]
    fn funnel_and_pyramid_bands() {
        assert_eq!(
            bands(&[1.0, 3.0, -2.0, 4.0], false),
            [(0.0, 0.125), (0.125, 0.5), (0.5, 0.5), (0.5, 1.0)]
        );
        // The pyramid's first point is at the bottom.
        assert_eq!(
            bands(&[1.0, 3.0, 4.0], true),
            [(0.875, 1.0), (0.5, 0.875), (0.0, 0.5)]
        );
        assert!(bands(&[-1.0, 0.0], false).is_empty());
        assert_eq!(
            bands(&[f64::MAX, f64::MAX], false),
            [(0.0, 0.5), (0.5, 1.0)]
        );
        // The funnel narrows from full width to a 5 % neck over its last 5 %; the pyramid
        // widens from its apex.
        for (u, w) in [
            (0.0, 1.0),
            (0.5, 0.5),
            (0.95, 0.05),
            (0.97, 0.05),
            (1.0, 0.05),
        ] {
            assert!((half_width(ChartType::Funnel, u) - w).abs() < 1e-12, "{u}");
        }
        for u in [0.0, 0.25, 1.0] {
            assert_eq!(half_width(ChartType::Pyramid, u), u);
        }
    }

    /// The pie's disc: 2-D screen angles clockwise from 3 o'clock; 3-D tilted in x–z, 10 % of
    /// its diameter thick (R194 Q50).
    #[test]
    fn discs_place_angles() {
        let flat = Disc::fit([0.0, 0.0, 400.0, 200.0], 0.95, false);
        assert_eq!(flat.centre, [200.0, 100.0]);
        assert!((flat.radius - 95.0).abs() < 1e-12);
        let p = |d: &Disc, r, a| d.at(r, a, [0.0, 0.0], true);
        let close =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
        assert!(close(p(&flat, 1.0, 0.0), [295.0, 100.0]), "3 o'clock");
        assert!(
            close(p(&flat, 1.0, 90.0), [200.0, 195.0]),
            "clockwise: 6 o'clock next"
        );
        assert!(
            close(flat.at(0.5, 0.0, [0.0, 0.2], true), [247.5, 119.0]),
            "pushed"
        );
        let tilted = Disc::fit([0.0, 0.0, 400.0, 400.0], 1.0, true);
        let (s, c) = (0.5, 3f64.sqrt() / 2.0);
        let r = tilted.radius;
        // R194: x stays horizontal and the depth axis runs by (s, −s·c): the rim spans
        // 2·√(1 + s²)·R across and 2·s·c·R + 0.2R (the unscaled thickness) down, centred.
        let across = 1f64.hypot(s);
        assert!((r - 400.0 / (2.0 * across)).abs() < 1e-9);
        // Clockwise seen from above: 0° points right, 90° comes forward (−z), down-left.
        let o = tilted.at(0.0, 0.0, [0.0; 2], true);
        let rel = |q: [f64; 2]| [q[0] - o[0], q[1] - o[1]];
        assert!(close(rel(p(&tilted, 1.0, 0.0)), [r, 0.0]));
        assert!(close(rel(p(&tilted, 1.0, 90.0)), [-s * r, s * c * r]));
        let rim: Vec<_> = (0..360).map(|a| p(&tilted, 1.0, a as f64)).collect();
        let bottom: Vec<_> = (0..360)
            .map(|a| tilted.at(1.0, a as f64, [0.0; 2], false))
            .collect();
        let ys = rim.iter().chain(&bottom).map(|q| q[1]);
        let (top, low) = ys.fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), y| {
            (a.min(y), b.max(y))
        });
        assert!((low - top - (2.0 * s * c + 0.2) * r).abs() < 1e-6 * r);
        assert!(((low + top) / 2.0 - 200.0).abs() < 1e-6 * r);
        // The thickness is straight down on screen, unscaled; the widest points are ±√(1 + s²)R.
        let d = [p(&tilted, 1.0, 60.0), tilted.at(1.0, 60.0, [0.0; 2], false)];
        assert!(close(
            [d[1][0] - d[0][0], d[1][1] - d[0][1]],
            [0.0, 0.2 * r]
        ));
        let xs = rim.iter().map(|q| q[0] - 200.0);
        let widest = xs.fold(0.0, |m: f64, x| m.max(x.abs()));
        assert!((widest - across * r).abs() < 1e-3 * r);
    }

    /// Radar spokes and Polar degrees run clockwise from the top; Q48 puts nonpositive values at
    /// the centre.
    #[test]
    fn radial_angles() {
        let r = Radial {
            polar: false,
            series: vec![],
            spokes: 4,
            scale: crate::effect::bb::chart::axes::Scale::new(0.0, 10.0, 10),
            centre: [100.0, 100.0],
            radius: 50.0,
        };
        let close =
            |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).abs() < 1e-9 && (a[1] - b[1]).abs() < 1e-9;
        assert!(close(r.at(0.0, 10.0), [100.0, 50.0]), "top");
        assert!(close(r.at(90.0, 5.0), [125.0, 100.0]), "clockwise");
        assert!(close(r.at(180.0, 10.0), [100.0, 150.0]));
        assert!(
            close(r.at(45.0, -3.0), [100.0, 100.0]),
            "nonpositive at the centre"
        );
    }
}
