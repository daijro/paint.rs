//! Extra (paint.rs): text that follows a shape and turns, the Text tool's own take on dpy's
//! Circle, Wave, Spiral and Rotate Text (the toolbar's Shape, Bend and Angle, and the rotate
//! handle). Each glyph is placed whole on the shape's path at the middle of its advance, turned to
//! the path; the lines below the first follow it at their baselines' distance; then the whole
//! text turns about its click point, counter-clockwise by the angle. Colour (bitmap) glyphs are
//! placed but not turned.

use super::{DrawItem, TextLayout, TextLine, TextSettings};
use crate::util::geom::Rectangle;
use std::f64::consts::{FRAC_PI_2, TAU};
use tiny_skia::Transform;

/// The stored names are a text layer's spec (S§4.5, R43).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TextShape {
    #[default]
    #[serde(rename = "straight")]
    Straight,
    /// An arc of a circle: the whole circle at a bend of 100; negative bends hang below it.
    #[serde(rename = "circle")]
    Circle,
    #[serde(rename = "wave")]
    Wave,
    /// Winding inward, clockwise (counter-clockwise for negative bends).
    #[serde(rename = "spiral")]
    Spiral,
}

impl TextShape {
    pub fn from_value(v: &str) -> Self {
        match v {
            "circle" => Self::Circle,
            "wave" => Self::Wave,
            "spiral" => Self::Spiral,
            _ => Self::Straight,
        }
    }
    pub fn value(self) -> &'static str {
        match self {
            Self::Straight => "straight",
            Self::Circle => "circle",
            Self::Wave => "wave",
            Self::Spiral => "spiral",
        }
    }
}

/// `p` turned `degrees` counter-clockwise (on screen) about `pivot`.
pub(super) fn turn_point(p: [f64; 2], pivot: [f64; 2], degrees: f64) -> [f64; 2] {
    let (s, c) = degrees.to_radians().sin_cos();
    let (dx, dy) = (p[0] - pivot[0], p[1] - pivot[1]);
    [pivot[0] + c * dx + s * dy, pivot[1] - s * dx + c * dy]
}

/// A path as points and their running lengths; `at` runs its ends on along their tangents.
struct Polyline {
    pts: Vec<[f64; 2]>,
    cum: Vec<f64>,
}

impl Polyline {
    fn new(points: impl IntoIterator<Item = [f64; 2]>) -> Option<Self> {
        let (mut pts, mut cum) = (Vec::<[f64; 2]>::new(), vec![]);
        for p in points {
            let len = match pts.last() {
                Some(q) => (p[0] - q[0]).hypot(p[1] - q[1]),
                None => {
                    pts.push(p);
                    cum.push(0.0);
                    continue;
                }
            };
            if len > 1e-9 {
                pts.push(p);
                cum.push(cum.last().unwrap() + len);
            }
        }
        (pts.len() >= 2).then_some(Self { pts, cum })
    }

    #[cfg(test)]
    fn length(&self) -> f64 {
        *self.cum.last().unwrap()
    }

    /// The point `s` along the path and its unit tangent.
    fn at(&self, s: f64) -> ([f64; 2], [f64; 2]) {
        let i = self.cum.partition_point(|&c| c <= s);
        let i = i.saturating_sub(1).min(self.pts.len() - 2);
        let (a, b) = (self.pts[i], self.pts[i + 1]);
        let len = self.cum[i + 1] - self.cum[i];
        let t = [(b[0] - a[0]) / len, (b[1] - a[1]) / len];
        let d = s - self.cum[i];
        ([a[0] + t[0] * d, a[1] + t[1] * d], t)
    }
}

/// How the glyphs of one layout land: on `path` (from the text's left edge, at its first
/// baseline) or straight, then turned about `pivot`.
pub(super) struct Placement {
    path: Option<Polyline>,
    x0: f64,
    b0: f64,
    /// The turn's sine and cosine.
    turn: (f64, f64),
    pivot: [f64; 2],
}

impl Placement {
    /// `None` when the text lies as laid out: straight (or unbent) and unturned.
    pub(super) fn new(layout: &TextLayout, s: &TextSettings, click: [f64; 2]) -> Option<Self> {
        Self::turned(layout, s, click, s.angle)
    }

    /// [`Placement::new`] turned by `angle` degrees instead of the settings' angle.
    pub(super) fn turned(
        layout: &TextLayout,
        s: &TextSettings,
        click: [f64; 2],
        angle: f64,
    ) -> Option<Self> {
        let bend = (s.bend / 100.0).clamp(-1.0, 1.0);
        let curved = s.shape != TextShape::Straight && bend != 0.0;
        let angle = if angle.is_finite() { angle } else { 0.0 };
        if !curved && angle == 0.0 {
            return None;
        }
        let first = layout.lines.first()?;
        let b0 = first.origin[1] + first.baseline;
        let x0 = layout
            .lines
            .iter()
            .map(|l| l.origin[0])
            .fold(f64::MAX, f64::min);
        let right = layout
            .lines
            .iter()
            .map(|l| l.origin[0] + l.width)
            .fold(f64::MIN, f64::max);
        let w = (right - x0).max(1.0);
        let lh = layout.space_height.max(1.0);
        let path = match s.shape {
            _ if !curved => None,
            TextShape::Circle => circle(x0, b0, w, bend),
            TextShape::Wave => wave(x0, b0, w, lh, bend),
            TextShape::Spiral => spiral(x0, b0, w, lh * (layout.lines.len() as f64 + 0.3), bend),
            TextShape::Straight => None,
        };
        Some(Self {
            path,
            x0,
            b0,
            turn: angle.to_radians().sin_cos(),
            pivot: click,
        })
    }

    /// The unturned path's point at straight `x` and its unit tangent.
    fn frame(&self, x: f64) -> ([f64; 2], [f64; 2]) {
        match &self.path {
            Some(p) => p.at(x - self.x0),
            None => ([x, self.b0], [1.0, 0.0]),
        }
    }

    /// `p` turned about the pivot, counter-clockwise on screen.
    pub(super) fn turn(&self, p: [f64; 2]) -> [f64; 2] {
        let ((s, c), [px, py]) = (self.turn, self.pivot);
        let (dx, dy) = (p[0] - px, p[1] - py);
        [px + c * dx + s * dy, py - s * dx + c * dy]
    }

    /// Where the straight layout's point `p` lands.
    pub(super) fn map(&self, p: [f64; 2]) -> [f64; 2] {
        let (q, t) = self.frame(p[0]);
        let dy = p[1] - self.b0;
        self.turn([q[0] - t[1] * dy, q[1] + t[0] * dy])
    }

    /// The rigid move of a glyph whose advance's middle is at `cx`, on a baseline at `by`.
    fn glyph(&self, cx: f64, by: f64) -> Transform {
        let (q, t) = self.frame(cx);
        let n = [-t[1], t[0]];
        let dy = by - self.b0;
        let q = [q[0] + n[0] * dy, q[1] + n[1] * dy];
        // (X, Y) → q + t (X − cx) + n (Y − by), then the turn.
        let along = Transform::from_row(
            t[0] as f32,
            t[1] as f32,
            n[0] as f32,
            n[1] as f32,
            (q[0] - t[0] * cx - n[0] * by) as f32,
            (q[1] - t[1] * cx - n[1] * by) as f32,
        );
        let ((s, c), [px, py]) = (self.turn, self.pivot);
        let turn = Transform::from_row(
            c as f32,
            -s as f32,
            s as f32,
            c as f32,
            (px - c * px - s * py) as f32,
            (py + s * px - c * py) as f32,
        );
        along.post_concat(turn)
    }

    /// The layout with every glyph placed: each line at the origin, its items where they land.
    pub(super) fn apply(&self, layout: &TextLayout) -> TextLayout {
        let lines = layout
            .lines
            .iter()
            .map(|l| {
                let by = l.origin[1] + l.baseline;
                let moves: Vec<Transform> = l
                    .glyphs
                    .iter()
                    .map(|g| self.glyph(l.origin[0] + g.origin[0] + g.advance / 2.0, by))
                    .collect();
                let local = Transform::from_translate(l.origin[0] as f32, l.origin[1] as f32);
                let (items, item_glyphs): (Vec<DrawItem>, Vec<usize>) = l
                    .items
                    .iter()
                    .zip(&l.item_glyphs)
                    .filter_map(|(item, &g)| {
                        Some((item.clone().transform(moves[g].pre_concat(local))?, g))
                    })
                    .unzip();
                let bounds = items
                    .iter()
                    .map(DrawItem::bounds)
                    .reduce(|a, b| Rectangle::union(&a, &b))
                    .unwrap_or_else(|| self.quad_bounds(&super::line_bounds(l)));
                TextLine {
                    origin: [0.0, 0.0],
                    clip: Rectangle::absolute(
                        (bounds.left() - 1.0).floor(),
                        (bounds.top() - 1.0).floor(),
                        (bounds.right() + 1.0).ceil(),
                        (bounds.bottom() + 1.0).ceil(),
                    ),
                    item_glyphs,
                    items,
                    ..l.clone()
                }
            })
            .collect();
        TextLayout {
            lines,
            space_height: layout.space_height,
        }
    }

    /// The bounds of `r`'s corners where they land.
    pub(super) fn quad_bounds(&self, r: &Rectangle) -> Rectangle {
        let q = [
            [r.left(), r.top()],
            [r.right(), r.top()],
            [r.right(), r.bottom()],
            [r.left(), r.bottom()],
        ]
        .map(|p| self.map(p));
        let (xs, ys) = (q.map(|p| p[0]), q.map(|p| p[1]));
        let min = |v: [f64; 4]| v.into_iter().fold(f64::MAX, f64::min);
        let max = |v: [f64; 4]| v.into_iter().fold(f64::MIN, f64::max);
        Rectangle::absolute(min(xs), min(ys), max(xs), max(ys))
    }

    /// The bounds of the glyphs' ink boxes where they land.
    pub(super) fn ink_bounds(&self, layout: &TextLayout) -> Option<Rectangle> {
        let mut out: Option<Rectangle> = None;
        for l in &layout.lines {
            let by = l.origin[1] + l.baseline;
            for g in &l.glyphs {
                let Some(ink) = g.ink else { continue };
                let m = self.glyph(l.origin[0] + g.origin[0] + g.advance / 2.0, by);
                let (x, y) = (l.origin[0] + ink.left(), l.origin[1] + ink.top());
                let (r, b) = (x + ink.width, y + ink.height);
                for [px, py] in [[x, y], [r, y], [r, b], [x, b]] {
                    let mut p = tiny_skia::Point::from_xy(px as f32, py as f32);
                    m.map_points(std::slice::from_mut(&mut p));
                    let pt = Rectangle::new(p.x as f64, p.y as f64, 0.0, 0.0);
                    out = Some(out.map_or(pt, |o| Rectangle::union(&o, &pt)));
                }
            }
        }
        out
    }
}

/// An arc of `|bend|` × a whole circle as long as the text, its middle at the text's centre on
/// the first baseline: arched over its centre for positive bends, hanging below it for negative.
fn circle(x0: f64, b0: f64, w: f64, bend: f64) -> Option<Polyline> {
    let sweep = bend.abs() * TAU;
    let r = w / sweep;
    let xc = x0 + w / 2.0;
    let arched = bend > 0.0;
    let cy = if arched { b0 + r } else { b0 - r };
    let n = ((w / 2.0).ceil() as usize).clamp(16, 4096);
    Polyline::new((0..=n).map(|i| {
        let u = i as f64 / n as f64 - 0.5;
        let phi = if arched {
            -FRAC_PI_2 + u * sweep
        } else {
            FRAC_PI_2 - u * sweep
        };
        [xc + r * phi.cos(), cy + r * phi.sin()]
    }))
}

/// A sine wave of `bend` line heights, eight line heights long (gentle enough that letters in its
/// troughs keep apart), rising first.
fn wave(x0: f64, b0: f64, w: f64, lh: f64, bend: f64) -> Option<Polyline> {
    let (a, lambda) = (bend * lh, 8.0 * lh);
    let point = |u: f64| [x0 + u, b0 - a * (TAU * u / lambda).sin()];
    let du = lambda / 64.0;
    let mut pts = vec![point(0.0)];
    let mut len = 0.0;
    let mut u = 0.0;
    // As long as the text, and a wavelength more.
    while len < w + lambda && pts.len() < 100_000 {
        u += du;
        let (p, q) = (point(u), pts[pts.len() - 1]);
        len += (p[0] - q[0]).hypot(p[1] - q[1]);
        pts.push(p);
    }
    Polyline::new(pts)
}

/// An Archimedean spiral `gap` apart per turn, as long as the text, starting at the top over its
/// centre and winding inward 1 + 3 × |bend| turns (fewer when the text is too short for them).
fn spiral(x0: f64, b0: f64, w: f64, gap: f64, bend: f64) -> Option<Polyline> {
    // Length ≈ 2π T (R − gap T / 2); the innermost turn stays gap / 2 from the centre.
    let r_for = |t: f64| w / (TAU * t) + gap * t / 2.0;
    let mut turns = 1.0 + 3.0 * bend.abs();
    while turns > 0.25 && r_for(turns) - gap * turns < gap / 2.0 {
        turns -= 0.05;
    }
    let r0 = r_for(turns);
    let cw = bend > 0.0;
    let (cx, cy) = (x0 + w / 2.0, if cw { b0 + r0 } else { b0 - r0 });
    let phi0 = if cw { -FRAC_PI_2 } else { FRAC_PI_2 };
    let (mut pts, mut len, mut k) = (Vec::<[f64; 2]>::new(), 0.0, 0.0);
    loop {
        let r = r0 - gap * k / TAU;
        let phi = if cw { phi0 + k } else { phi0 - k };
        let p = [cx + r * phi.cos(), cy + r * phi.sin()];
        if let Some(q) = pts.last() {
            len += (p[0] - q[0]).hypot(p[1] - q[1]);
        }
        pts.push(p);
        if len >= w + gap || r <= gap / 4.0 || pts.len() >= 100_000 {
            break;
        }
        k += (2.0 / r.max(1.0)).min(0.05);
    }
    Polyline::new(pts)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Within 0.05 px (the arcs are sampled every 2 px).
    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 0.05 && (a[1] - b[1]).abs() < 0.05
    }

    #[test]
    fn polyline_runs_on_past_its_ends() {
        let p = Polyline::new([[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]]).unwrap();
        assert_eq!(p.length(), 20.0);
        assert!(close(p.at(5.0).0, [5.0, 0.0]));
        assert!(close(p.at(15.0).0, [10.0, 5.0]) && close(p.at(15.0).1, [0.0, 1.0]));
        assert!(close(p.at(-2.0).0, [-2.0, 0.0]));
        assert!(close(p.at(25.0).0, [10.0, 15.0]));
    }

    #[test]
    fn shapes_are_as_long_as_the_text() {
        let (w, lh) = (300.0, 20.0);
        let full = circle(10.0, 50.0, w, 1.0).unwrap();
        assert!((full.length() - w).abs() < 1.0);
        // A whole circle closes on itself; its top (the middle) is on the baseline.
        assert!(close(full.pts[0], *full.pts.last().unwrap()));
        assert!(close(full.at(w / 2.0).0, [160.0, 50.0]));
        // A half circle hanging below: its middle on the baseline, the ends above it.
        let low = circle(10.0, 50.0, w, -0.5).unwrap();
        assert!(close(low.at(w / 2.0).0, [160.0, 50.0]) && low.pts[0][1] < 50.0);
        assert!(wave(0.0, 0.0, w, lh, 0.5).unwrap().length() >= w);
        for bend in [0.1, 1.0, -1.0] {
            let s = spiral(0.0, 0.0, w, lh * 1.3, bend).unwrap();
            assert!(s.length() >= w * 0.98, "{bend}: {}", s.length());
        }
    }

    #[test]
    fn a_turn_is_counter_clockwise_about_the_pivot() {
        let p = Placement {
            path: None,
            x0: 0.0,
            b0: 0.0,
            turn: 90f64.to_radians().sin_cos(),
            pivot: [10.0, 10.0],
        };
        // Right of the pivot goes above it on screen.
        assert!(close(p.map([20.0, 10.0]), [10.0, 0.0]));
        // The glyph move agrees with the point map.
        let m = p.glyph(20.0, 10.0);
        let mut q = tiny_skia::Point::from_xy(25.0, 7.0);
        m.map_points(std::slice::from_mut(&mut q));
        assert!(close([q.x as f64, q.y as f64], p.map([25.0, 7.0])));
    }
}
