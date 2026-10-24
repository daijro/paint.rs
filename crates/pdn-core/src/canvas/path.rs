//! Canvas path model (HTML spec semantics) converted to tiny-skia paths.
use tiny_skia::{Path, PathBuilder};

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verb {
    Move(f64, f64),
    Line(f64, f64),
    Quad(f64, f64, f64, f64),
    Cubic(f64, f64, f64, f64, f64, f64),
    Close,
    Arc(AnalyticArc),
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Path2D {
    verbs: Vec<Verb>,
    flags: Vec<PathFlags>,
    start: Option<(f64, f64)>,
    current: Option<(f64, f64)>,
}

/// XAML segment options; ordinary Canvas builders always use the default options.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct PathFlags {
    pub filled: bool,
    pub stroked: bool,
    pub smooth: bool,
}
impl Default for PathFlags {
    fn default() -> Self {
        Self {
            filled: true,
            stroked: true,
            smooth: false,
        }
    }
}
/// An affine ellipse, C + U cos(t) + V sin(t). Kept analytical until raster conversion.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct AnalyticArc {
    pub center: [f64; 2],
    pub u: [f64; 2],
    pub v: [f64; 2],
    pub start: f64,
    pub sweep: f64,
}
impl AnalyticArc {
    pub fn point(self, t: f64) -> [f64; 2] {
        let (s, c) = t.sin_cos();
        [
            self.center[0] + self.u[0] * c + self.v[0] * s,
            self.center[1] + self.u[1] * c + self.v[1] * s,
        ]
    }
    pub fn mapped(self, m: &[f64; 6]) -> Self {
        let point = |p: [f64; 2]| {
            let q = Path2D::map(m, p[0], p[1]);
            [q.0, q.1]
        };
        let vector = |p: [f64; 2]| [m[0] * p[0] + m[2] * p[1], m[1] * p[0] + m[3] * p[1]];
        Self {
            center: point(self.center),
            u: vector(self.u),
            v: vector(self.v),
            ..self
        }
    }
}

impl Path2D {
    fn push(&mut self, verb: Verb) {
        match verb {
            Verb::Move(x, y) => {
                self.start = Some((x, y));
                self.current = Some((x, y));
            }
            Verb::Line(x, y) | Verb::Quad(_, _, x, y) | Verb::Cubic(_, _, _, _, x, y) => {
                self.current = Some((x, y))
            }
            Verb::Close => self.current = self.start,
            Verb::Arc(a) => {
                let p = a.point(a.start + a.sweep);
                self.current = Some((p[0], p[1]));
            }
        }
        self.verbs.push(verb);
        self.flags.push(PathFlags::default());
    }
    pub(crate) fn verbs(&self) -> &[Verb] {
        &self.verbs
    }
    pub(crate) fn flags(&self) -> &[PathFlags] {
        &self.flags
    }
    pub(crate) fn annotate_from(&mut self, start: usize, flags: PathFlags) {
        self.flags[start..].fill(flags);
    }
    pub(crate) fn analytic_arc(&mut self, arc: AnalyticArc) {
        let p = arc.point(arc.start + arc.sweep);
        self.push(Verb::Arc(arc));
        self.current = Some((p[0], p[1]));
    }
    pub(crate) fn fill_path(&self) -> Self {
        let mut out = Self::new();
        let mut included = true;
        for (i, v) in self.verbs.iter().enumerate() {
            if matches!(v, Verb::Move(..)) {
                included = self.flags[i].filled;
            }
            if included {
                out.push(v.clone());
            }
        }
        out
    }
    pub(crate) fn lowered_arcs(&self) -> Self {
        let mut out = Self::new();
        for (i, verb) in self.verbs.iter().enumerate() {
            let before = out.verbs.len();
            if let Verb::Arc(a) = *verb {
                let count = (a.sweep.abs() / std::f64::consts::FRAC_PI_2).ceil().max(1.) as usize;
                let step = a.sweep / count as f64;
                let k = 4. / 3. * (step / 4.).tan();
                for j in 0..count {
                    let t = a.start + j as f64 * step;
                    let t2 = t + step;
                    let p = a.point(t);
                    let e = a.point(t2);
                    let tangent = |t: f64| {
                        [
                            -a.u[0] * t.sin() + a.v[0] * t.cos(),
                            -a.u[1] * t.sin() + a.v[1] * t.cos(),
                        ]
                    };
                    let d = tangent(t);
                    let d2 = tangent(t2);
                    out.bezier_curve_to(
                        p[0] + k * d[0],
                        p[1] + k * d[1],
                        e[0] - k * d2[0],
                        e[1] - k * d2[1],
                        e[0],
                        e[1],
                    );
                }
            } else {
                out.push(verb.clone());
            }
            out.annotate_from(before, self.flags[i]);
        }
        out
    }

    pub fn new() -> Self {
        Self::default()
    }

    /// Transforms points as they are added (Canvas applies the CTM at path-building time).
    pub(crate) fn map(m: &[f64; 6], x: f64, y: f64) -> (f64, f64) {
        (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    }

    fn ensure_subpath(&mut self, x: f64, y: f64) {
        if self.current.is_none() {
            self.move_to(x, y);
        }
    }

    pub fn move_to(&mut self, x: f64, y: f64) {
        self.push(Verb::Move(x, y));
        self.start = Some((x, y));
        self.current = Some((x, y));
    }

    pub fn line_to(&mut self, x: f64, y: f64) {
        if self.current.is_none() {
            self.move_to(x, y);
            return;
        }
        self.push(Verb::Line(x, y));
        self.current = Some((x, y));
    }

    pub fn quadratic_curve_to(&mut self, cx: f64, cy: f64, x: f64, y: f64) {
        self.ensure_subpath(cx, cy);
        self.push(Verb::Quad(cx, cy, x, y));
        self.current = Some((x, y));
    }

    pub fn bezier_curve_to(&mut self, c1x: f64, c1y: f64, c2x: f64, c2y: f64, x: f64, y: f64) {
        self.ensure_subpath(c1x, c1y);
        self.push(Verb::Cubic(c1x, c1y, c2x, c2y, x, y));
        self.current = Some((x, y));
    }

    pub fn close_path(&mut self) {
        if let Some(s) = self.start {
            self.push(Verb::Close);
            self.current = Some(s);
        }
    }

    pub fn rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.move_to(x, y);
        self.line_to(x + w, y);
        self.line_to(x + w, y + h);
        self.line_to(x, y + h);
        self.close_path();
    }

    /// Already-transformed points (used by Canvas, which maps through the CTM first).
    pub(crate) fn push_mapped(&mut self, other: &Path2D, m: &[f64; 6]) {
        for (i, v) in other.verbs.iter().enumerate() {
            let before = self.verbs.len();
            match *v {
                Verb::Move(x, y) => {
                    let p = Self::map(m, x, y);
                    self.move_to(p.0, p.1);
                }
                Verb::Line(x, y) => {
                    let p = Self::map(m, x, y);
                    self.line_to(p.0, p.1);
                }
                Verb::Quad(a, b, x, y) => {
                    let (c, p) = (Self::map(m, a, b), Self::map(m, x, y));
                    self.quadratic_curve_to(c.0, c.1, p.0, p.1);
                }
                Verb::Cubic(a, b, c, d, x, y) => {
                    let (c1, c2, p) = (Self::map(m, a, b), Self::map(m, c, d), Self::map(m, x, y));
                    self.bezier_curve_to(c1.0, c1.1, c2.0, c2.1, p.0, p.1);
                }
                Verb::Close => self.close_path(),
                Verb::Arc(a) => self.analytic_arc(a.mapped(m)),
            }
            self.annotate_from(before, other.flags[i]);
        }
    }

    /// Like `push_mapped`, but a leading `Move` in `other` becomes `line_to` when `self` already has a
    /// current point.
    pub(crate) fn push_mapped_continuing(&mut self, other: &Path2D, m: &[f64; 6]) {
        for (i, v) in other.verbs.iter().enumerate() {
            if let (0, Verb::Move(x, y)) = (i, v) {
                let p = Self::map(m, *x, *y);
                if self.current.is_some() {
                    self.line_to(p.0, p.1);
                } else {
                    self.move_to(p.0, p.1);
                }
                continue;
            }
            let single = Path2D {
                verbs: vec![v.clone()],
                flags: vec![other.flags[i]],
                start: None,
                current: None,
            };
            self.push_mapped(&single, m);
        }
    }

    /// HTML `ellipse()` (arc() is ellipse with rx = ry, rotation 0).
    #[allow(clippy::too_many_arguments)] // Canvas ellipse() signature
    pub fn ellipse(
        &mut self,
        x: f64,
        y: f64,
        rx: f64,
        ry: f64,
        rotation: f64,
        start: f64,
        end: f64,
        ccw: bool,
    ) {
        use std::f64::consts::{FRAC_PI_2, TAU};
        // Sweep per Blink's AdjustEndAngle. Unlike the HTML spec, a nonzero multiple of 2π drawn
        // "backwards" (e.g. `arc(x, y, r, 0, 2π, true)`) is a full turn, not an empty arc; Chromium
        // keeps that for backward compat and paintdotjs's gear hole relies on it (arc-ccw-full-turn).
        let mut sweep = end - start;
        if !ccw && sweep >= TAU || ccw && sweep <= -TAU {
            sweep = if ccw { -TAU } else { TAU };
        } else {
            sweep = sweep.rem_euclid(TAU);
            if ccw && sweep != 0.0 {
                sweep -= TAU;
            }
            if sweep == 0.0 && end != start {
                sweep = if ccw { -TAU } else { TAU };
            }
        }
        let (sin_r, cos_r) = rotation.sin_cos();
        let point = |t: f64| {
            let (s, c) = t.sin_cos();
            let (px, py) = (rx * c, ry * s);
            (x + px * cos_r - py * sin_r, y + px * sin_r + py * cos_r)
        };
        let p0 = point(start);
        if self.current.is_some() {
            self.line_to(p0.0, p0.1);
        } else {
            self.move_to(p0.0, p0.1);
        }
        let segments = (sweep.abs() / FRAC_PI_2).ceil().max(1.0) as usize;
        let step = sweep / segments as f64;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        let mut t = start;
        for _ in 0..segments {
            let t2 = t + step;
            let (s1, c1) = t.sin_cos();
            let (s2, c2) = t2.sin_cos();
            let local =
                |px: f64, py: f64| (x + px * cos_r - py * sin_r, y + px * sin_r + py * cos_r);
            let a = local(rx * (c1 - k * s1), ry * (s1 + k * c1));
            let b = local(rx * (c2 + k * s2), ry * (s2 - k * c2));
            let e = point(t2);
            self.bezier_curve_to(a.0, a.1, b.0, b.1, e.0, e.1);
            t = t2;
        }
    }

    /// HTML roundRect with one uniform radius (the only form paintdotjs uses).
    pub fn round_rect(&mut self, x: f64, y: f64, w: f64, h: f64, r: f64) {
        let r = r.min(w.abs() / 2.0).min(h.abs() / 2.0).max(0.0);
        use std::f64::consts::{FRAC_PI_2, PI};
        self.move_to(x + r, y);
        self.line_to(x + w - r, y);
        self.ellipse(x + w - r, y + r, r, r, 0.0, -FRAC_PI_2, 0.0, false);
        self.line_to(x + w, y + h - r);
        self.ellipse(x + w - r, y + h - r, r, r, 0.0, 0.0, FRAC_PI_2, false);
        self.line_to(x + r, y + h);
        self.ellipse(x + r, y + h - r, r, r, 0.0, FRAC_PI_2, PI, false);
        self.line_to(x, y + r);
        self.ellipse(x + r, y + r, r, r, 0.0, PI, 1.5 * PI, false);
        self.close_path();
        let start = (x, y);
        self.move_to(start.0, start.1);
    }

    pub fn is_empty(&self) -> bool {
        self.verbs.is_empty()
    }

    /// Port of Skia's `SkPath::contains` (what Chromium's isPointInPath ends in): f32 winding with
    /// every contour force-closed; a point on the boundary counts as inside, except where an even
    /// number of coincident, opposite edges meet under nonzero.
    /// ponytail: curves are flattened to lines at 0.01 px tolerance instead of Skia's analytic
    /// winding_quad/winding_cubic; port those if hit tests exactly on a curve ever matter.
    pub fn contains(&self, x: f32, y: f32, rule: super::FillRule) -> bool {
        if self.flags.iter().any(|f| !f.filled) {
            return self.fill_path().contains(x, y, rule);
        }
        let segs = self.flatten();
        let (mut w, mut on_curve) = (0, 0);
        for &(p0, p1) in &segs {
            w += winding_line(p0, p1, x, y, &mut on_curve);
        }
        let even_odd = rule == super::FillRule::EvenOdd;
        if even_odd {
            w &= 1;
        }
        if w != 0 {
            return true;
        }
        if on_curve <= 1 || on_curve & 1 == 1 || even_odd {
            return on_curve & 1 == 1;
        }
        // Even number of on-curve hits under nonzero: inside unless the touching edges cancel
        // (coincident tangents in opposite directions).
        let mut tangents: Vec<(f32, f32)> = vec![];
        for &(p0, p1) in &segs {
            if !between(p0.1, y, p1.1) || !between(p0.0, x, p1.0) {
                continue;
            }
            let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
            if ((x - p0.0) * dy - dx * (y - p0.1)).abs() > NEARLY_ZERO {
                continue;
            }
            if dx * dx + dy * dy <= NEARLY_ZERO {
                continue;
            }
            let opposite = tangents.iter().position(|t| {
                (t.0 * dy - t.1 * dx).abs() <= NEARLY_ZERO && dx * t.0 <= 0.0 && dy * t.1 <= 0.0
            });
            match opposite {
                Some(i) => {
                    tangents.swap_remove(i);
                }
                None => tangents.push((dx, dy)),
            }
        }
        !tangents.is_empty()
    }

    /// Line segments of every contour, force-closed, in f32 (Skia path precision).
    fn flatten(&self) -> Vec<((f32, f32), (f32, f32))> {
        if self.verbs.iter().any(|v| matches!(v, Verb::Arc(_))) {
            return self.lowered_arcs().flatten();
        }
        fn pt(x: f64, y: f64) -> (f32, f32) {
            (x as f32, y as f32)
        }
        let mut segs = vec![];
        let (mut start, mut cur) = ((0.0, 0.0), (0.0, 0.0));
        let close = |segs: &mut Vec<_>, cur: (f32, f32), start: (f32, f32)| {
            if cur != start {
                segs.push((cur, start));
            }
        };
        // Curve segment count for 0.01 px: max flattening error is |B''|/8n², |B''| <= 6·d for
        // a cubic (d = largest second difference of the control points), 2·d for a quad.
        let n_for = |d: f64, k: f64| ((k * d / 0.08).sqrt().ceil() as usize).clamp(1, 1024);
        let mut fcur = (0.0f64, 0.0f64);
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) => {
                    close(&mut segs, cur, start);
                    (start, cur, fcur) = (pt(x, y), pt(x, y), (x, y));
                }
                Verb::Line(x, y) => {
                    segs.push((cur, pt(x, y)));
                    (cur, fcur) = (pt(x, y), (x, y));
                }
                Verb::Quad(cx, cy, x, y) => {
                    let (p0, p1, p2) = (fcur, (cx, cy), (x, y));
                    let d = (p0.0 - 2.0 * p1.0 + p2.0).hypot(p0.1 - 2.0 * p1.1 + p2.1);
                    let n = n_for(d, 2.0);
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        let u = 1.0 - t;
                        let q = pt(
                            u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0,
                            u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1,
                        );
                        segs.push((cur, q));
                        cur = q;
                    }
                    (cur, fcur) = (pt(x, y), (x, y));
                }
                Verb::Cubic(ax, ay, bx, by, x, y) => {
                    let (p0, p1, p2, p3) = (fcur, (ax, ay), (bx, by), (x, y));
                    let d = (p0.0 - 2.0 * p1.0 + p2.0)
                        .hypot(p0.1 - 2.0 * p1.1 + p2.1)
                        .max((p1.0 - 2.0 * p2.0 + p3.0).hypot(p1.1 - 2.0 * p2.1 + p3.1));
                    let n = n_for(d, 6.0);
                    for i in 1..=n {
                        let t = i as f64 / n as f64;
                        let u = 1.0 - t;
                        let (c0, c1, c2, c3) =
                            (u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t);
                        let q = pt(
                            c0 * p0.0 + c1 * p1.0 + c2 * p2.0 + c3 * p3.0,
                            c0 * p0.1 + c1 * p1.1 + c2 * p2.1 + c3 * p3.1,
                        );
                        segs.push((cur, q));
                        cur = q;
                    }
                    (cur, fcur) = (pt(x, y), (x, y));
                }
                Verb::Close => {
                    close(&mut segs, cur, start);
                    cur = start;
                    fcur = (start.0 as f64, start.1 as f64);
                }
                Verb::Arc(_) => unreachable!("arcs lowered above"),
            }
        }
        close(&mut segs, cur, start);
        segs
    }

    pub fn to_skia(&self) -> Option<Path> {
        if self.verbs.iter().any(|v| matches!(v, Verb::Arc(_))) {
            return self.lowered_arcs().to_skia();
        }
        let mut b = PathBuilder::new();
        for v in &self.verbs {
            match *v {
                Verb::Move(x, y) => b.move_to(x as f32, y as f32),
                Verb::Line(x, y) => b.line_to(x as f32, y as f32),
                Verb::Quad(a, c, x, y) => b.quad_to(a as f32, c as f32, x as f32, y as f32),
                Verb::Cubic(a, c, d, e, x, y) => {
                    b.cubic_to(a as f32, c as f32, d as f32, e as f32, x as f32, y as f32)
                }
                Verb::Close => b.close(),
                Verb::Arc(_) => unreachable!("arcs lowered above"),
            }
        }
        b.finish()
    }
}

/// Skia's SK_ScalarNearlyZero (1/4096), used by SkScalarNearlyZero/NearlyEqual.
const NEARLY_ZERO: f32 = 1.0 / 4096.0;

fn between(a: f32, b: f32, c: f32) -> bool {
    (a - b) * (c - b) <= 0.0
}

/// Skia `checkOnCurve`.
fn check_on_curve(x: f32, y: f32, start: (f32, f32), end: (f32, f32)) -> bool {
    if start.1 == end.1 {
        between(start.0, x, end.0) && x != end.0
    } else {
        x == start.0 && y == start.1
    }
}

/// Skia `winding_line`.
fn winding_line(p0: (f32, f32), p1: (f32, f32), x: f32, y: f32, on_curve: &mut i32) -> i32 {
    let (x0, x1) = (p0.0, p1.0);
    let (mut y0, mut y1) = (p0.1, p1.1);
    let dy = y1 - y0;
    let mut dir = 1;
    if y0 > y1 {
        std::mem::swap(&mut y0, &mut y1);
        dir = -1;
    }
    if y < y0 || y > y1 {
        return 0;
    }
    if check_on_curve(x, y, p0, p1) {
        *on_curve += 1;
        return 0;
    }
    if y == y1 {
        return 0;
    }
    let cross = (x1 - x0) * (y - p0.1) - dy * (x - x0);
    if cross == 0.0 {
        if x != x1 || y != p1.1 {
            *on_curve += 1;
        }
        dir = 0;
    } else if (cross > 0.0) as i32 - (cross < 0.0) as i32 == dir {
        dir = 0;
    }
    dir
}
