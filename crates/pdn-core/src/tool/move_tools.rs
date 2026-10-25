//! Move Selected Pixels and Move Selection on the transacted-tool core (`tool::transacted`), and
//! the transform control (handles, hit testing, gestures) Task 33's shape editing shares.
//! Port of: js/src/engine/document/tool/tool/move/{MoveToolBase,MoveTool,MoveSelectionTool}.js
//! (tool structure, pending context, keyboard handling, cursors);
//! js/src/engine/document/render/renderer/MoveNubRenderer.js (handle positions and hit sizes)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's transform UI, pixel output (`tool::move_render`), angle snap, nudge, minimum scale,
//! tint and handles; the core's Draw, Edit and Finish entries, undo re-opens and the commit-first
//! hook; the angle in the selection info; a paste begins here and floats with b = 0.
//!
//! Paint.NET's move tools keep one changes record: F = Base·Delta·Edit, with
//! Delta·Edit folded into the record's `m`. The first gesture of a transaction is its Draw entry
//! ("Move Pixels", "Rotate Selection" …), every later gesture or nudge burst an Edit entry, and the
//! commit "Finish Selection" (the Rulings' names).
//!
//! R230 (Extra) Distort: with the toggle on, a corner handle of Move Selected Pixels moves its
//! corner alone. The record then keeps the quad (the base bounds' corners on the canvas) beside
//! F: the pixels and the selection follow the rectangle → quad homography, and F only frames the
//! later gestures, each of which moves the quad by the canvas edit it makes to F (F′ = F·E: E),
//! so translate, rotate, scale and nudges keep working on the distorted pixels.

use std::sync::Arc;

use serde_json::Value;

use super::base::ToolBase;
use super::move_render::{
    MoveCache, MoveInterpolation, MoveRender, bake_stamps, render_moved_into,
};
use super::snap::{SNAP_PX, SnapLine, SnapTargets};
use super::transacted::{self, DynTransacted, Edit, State, Transacted, Transaction};
use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::document::surface::Surface;
use crate::document::workspace::DocumentWorkspace;
use crate::extra::perspective::{Homography, keep_convex};
use crate::history::{HistoryMemento, MementoKind, functions};
use crate::i18n::t;
use crate::pixel::mask::{self, Mask, SelectionQuality};
use crate::pixel::{IntRect, Px};
use crate::render::float::{Float, FloatSource, next_id};
use crate::render::nubs::{self, Handle, HandleShape};
use crate::render::{Renderer, RendererId};
use crate::resample::Gamma;
use crate::selection::matrix::{Matrix, MatrixOrder};
use crate::selection::{CombineMode, Selection, SelectionData, path::GraphicsPath};
use crate::util::geom::{Point, Rectangle};
use crate::util::utility::to_degrees;

/// §2.8 "Nudge": a burst commits after max(500, 1.5 × the default 500 ms keyboard delay) ms
/// without an arrow key (§6 item 8).
pub const NUDGE_COMMIT_MS: f64 = 750.0;

/// §2.8 "Translate": the drag delta truncated toward zero (paintdotjs rounds).
pub fn truncate_translation(dx: f64, dy: f64) -> (f64, f64) {
    (dx.trunc(), dy.trunc())
}

/// §2.8, the Shift angle snap: into [0, 360), the nearer multiple of 15 below or above ⌊θ⌋'s
/// with ties to the upper one, then into (−180, 180].
pub fn constrain_angle(theta: f64) -> f64 {
    if !theta.is_finite() {
        return theta;
    }
    // "Add 360 while θ < 0", without the loop.
    let theta = if theta < 0.0 {
        theta.rem_euclid(360.0)
    } else {
        theta
    };
    let lo = (theta.floor() / 15.0).floor() * 15.0;
    let hi = lo + 15.0;
    let a = if (theta - lo).abs() < (theta - hi).abs() {
        lo
    } else {
        hi
    };
    if a > 180.0 { a - 360.0 } else { a }
}

/// §2.8 "Scale pixel snap": with W′ = `base_extent` (the extent in canvas px at the drag start)
/// and g = k/W′, s′ = (round(s/g) + (round(W′) mod k)/2)·g, then |s′| ≥ 1e-7 (§3 "Min scale").
/// `round` is .NET's default `Math.Round` (ties to even).
pub fn snap_scale(s: f64, base_extent: f64, k: u32) -> f64 {
    let k = k.max(1) as f64;
    let mut out = s;
    if base_extent.is_finite() && base_extent > 0.0 {
        let g = k / base_extent;
        out = ((s / g).round_ties_even() + base_extent.round_ties_even().rem_euclid(k) / 2.0) * g;
    }
    if !out.is_finite() {
        return 1.0;
    }
    // A scale snapped to nothing keeps the drag's side (the flip, §2.8 "Scale pixel snap").
    if out.abs() < 1e-7 {
        out = 1e-7_f64.copysign(s);
    }
    out
}

/// A scale handle (paintdotjs `MoveToolBaseEdge`), in the opacity-pulse order (§2.10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    TopLeft,
    Top,
    TopRight,
    Right,
    BottomRight,
    Bottom,
    BottomLeft,
    Left,
}

impl Edge {
    pub const ALL: [Edge; 8] = [
        Edge::TopLeft,
        Edge::Top,
        Edge::TopRight,
        Edge::Right,
        Edge::BottomRight,
        Edge::Bottom,
        Edge::BottomLeft,
        Edge::Left,
    ];

    /// The handle's direction from the centre: −1, 0 or 1 per axis.
    pub fn dir(self) -> (f64, f64) {
        match self {
            Edge::TopLeft => (-1.0, -1.0),
            Edge::Top => (0.0, -1.0),
            Edge::TopRight => (1.0, -1.0),
            Edge::Right => (1.0, 0.0),
            Edge::BottomRight => (1.0, 1.0),
            Edge::Bottom => (0.0, 1.0),
            Edge::BottomLeft => (-1.0, 1.0),
            Edge::Left => (-1.0, 0.0),
        }
    }

    /// Its point on `bb` (base space).
    fn on(self, bb: &Rectangle) -> Point {
        let (dx, dy) = self.dir();
        let (cx, cy) = (bb.x + bb.width / 2.0, bb.y + bb.height / 2.0);
        Point::new(cx + dx * bb.width / 2.0, cy + dy * bb.height / 2.0)
    }
}

/// What the pointer is over (the transform handles' hit testing, §2.8 "Hit testing").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveHit {
    Scale(Edge),
    /// The translate (compass) handle.
    Translate,
    /// The rotate zone around the transformed bounds.
    Rotate,
    /// The rotation anchor.
    Anchor,
    /// Anywhere else: a translate drag.
    Background,
    /// Nothing to hit (a singular transform).
    None,
}

/// paintdotjs `MoveNubRenderer`: a circle handle's painted radius 4 plus the 8-DIP hit padding.
const CIRCLE_HIT_DIP: f64 = 4.0 + 8.0;
/// paintdotjs `MoveNubRenderer.compassRadius` 7 plus the padding (a box).
const COMPASS_HIT_DIP: f64 = 7.0 + 8.0;
/// paintdotjs `MoveToolBase.updateRotationIndicator`: the rotate zone reaches 32 DIP outside the
/// transformed bounds (Paint.NET's own rotate-zone size is not documented, §6 item 9).
const ROTATE_ZONE_DIP: f64 = 32.0;
/// §2.8 "Hit testing": the translate handle sits 45 hair widths beyond the bottom-right corner.
// calibrate: B-C11 — the hair width taken as one device px (as the Wand's compass), not × u.
const TRANSLATE_OFFSET_HW: f64 = 45.0;

fn apply(m: &Matrix, p: Point) -> Point {
    let mut v = [p];
    m.transform_points(&mut v);
    v[0]
}

/// R230: a distort's corners on the canvas, TL, TR, BR, BL of the base bounds.
pub type Quad = [[f64; 2]; 4];
/// S§4.6: a trail stamp, M and the quad it had.
pub type Stamp = (Matrix, Option<Quad>);

/// S§4.6, R253: a Shift step from `at` to `to` stamps both, so the place trailing began keeps a
/// copy too; a place the last stamp already holds is skipped.
fn push_stamps(trail: &mut Vec<Stamp>, at: Stamp, to: Stamp) {
    for s in [at, to] {
        if trail.last() != Some(&s) {
            trail.push(s);
        }
    }
}

/// `m` as a homography (both keep x′ = e00·x + e01·y + e02).
fn hom(m: &Matrix) -> Homography {
    let [a, b, c] = m.elements;
    Homography([a[0], a[1], a[2], b[0], b[1], b[2], c[0], c[1], c[2]])
}

fn map(h: &Homography, p: Point) -> Point {
    let (x, y) = h.apply(p.x, p.y);
    Point::new(x, y)
}

fn xy(p: Point) -> [f64; 2] {
    [p.x, p.y]
}

/// The rectangle's corners, TL, TR, BR, BL.
fn corners(bb: &Rectangle) -> [Point; 4] {
    [
        Point::new(bb.left(), bb.top()),
        Point::new(bb.right(), bb.top()),
        Point::new(bb.right(), bb.bottom()),
        Point::new(bb.left(), bb.bottom()),
    ]
}

fn dist(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

/// `Utility.getAngleOfTransform`: the x axis's angle under `m`, degrees.
fn angle_of(m: &Matrix) -> f64 {
    let mut v = [Point::new(1.0, 0.0)];
    m.transform_vectors(&mut v);
    to_degrees(v[0].y.atan2(v[0].x))
}

/// `a·b` (a first).
fn then(a: &Matrix, b: &Matrix) -> Matrix {
    let mut r = *a;
    r.multiply(b, MatrixOrder::Append);
    r
}

/// The selection under `r`'s Base (as it was when the transaction began).
fn under_base(r: &MoveToolContext) -> Selection {
    let mut s = Selection::default();
    s.restore((*r.selection).clone());
    s
}

/// The active layer shows the way the canvas draws a float (`render::float`): visible, blending
/// Normal, with no shown layer above it.
fn floats(ws: &DocumentWorkspace) -> bool {
    use crate::document::layer::{BlendMode, Layer};
    let layers = &ws.document.layers;
    let shown = |l: &Layer| l.properties.visible && l.properties.opacity != 0;
    layers
        .get_at(ws.active_layer)
        .is_some_and(|l| shown(l) && l.properties.blend_mode == BlendMode::Normal)
        && layers.iter().skip(ws.active_layer + 1).all(|l| !shown(l))
}

/// The bounds of `bb`'s corners under `m`, kept in a range `IntRect::bound_of` can take.
fn transformed_bounds(m: &Matrix, bb: &Rectangle) -> Rectangle {
    points_bounds(corners(bb).map(|p| apply(m, p)))
}

/// The bounds of four points, kept in a range `IntRect::bound_of` can take.
fn points_bounds(corners: [Point; 4]) -> Rectangle {
    let lim = |v: f64| if v.is_nan() { 0.0 } else { v.clamp(-1e9, 1e9) };
    let xs = corners.map(|p| lim(p.x));
    let ys = corners.map(|p| lim(p.y));
    let min = |v: [f64; 4]| v.into_iter().fold(f64::INFINITY, f64::min);
    let max = |v: [f64; 4]| v.into_iter().fold(f64::NEG_INFINITY, f64::max);
    Rectangle::absolute(min(xs), min(ys), max(xs), max(ys))
}

/// The move tools' transform control: handle positions, the hover growth and hit testing.
/// Positions follow paintdotjs (the 8 scale handles at the corners and edge midpoints of the
/// transformed bounds); Paint.NET adds the rotation anchor (default F(centre of the base
/// bounds)) and the translate handle (§2.8 rows "Hit testing", "Rotate").
#[derive(Clone, Debug, Default)]
pub struct TransformHandles {
    /// The rotation anchor in base space; `None` = the centre of the base bounds.
    pub anchor: Option<Point>,
    /// DIP → device px (0 counts as 1).
    pub ui_scale: f64,
    /// The scale handles only (a translate or rotate drag hides every handle; a scale drag keeps
    /// them, paintdotjs `positionNubs`).
    pub hidden: bool,
    /// R230: a distorted selection's base space → canvas map, used instead of F.
    pub quad: Option<Homography>,
    /// The hovered handle and when the pointer entered it (s).
    hover: Option<(MoveHit, f64)>,
}

impl TransformHandles {
    fn u(&self) -> f64 {
        if self.ui_scale.is_finite() && self.ui_scale > 0.0 {
            self.ui_scale
        } else {
            1.0
        }
    }

    /// The anchor, the eight scale handles and the translate handle in canvas coordinates.
    fn points(&self, f: &Matrix, bb: &Rectangle, hw: f64) -> (Point, [Point; 8], Point) {
        let centre = Point::new(bb.x + bb.width / 2.0, bb.y + bb.height / 2.0);
        let to = |p: Point| self.quad.map_or_else(|| apply(f, p), |h| map(&h, p));
        let anchor = to(self.anchor.unwrap_or(centre));
        let scale = Edge::ALL.map(|e| to(e.on(bb)));
        let (tl, br) = (scale[0], scale[4]);
        let d = dist(tl, br);
        let dir = if d > 0.0 && d.is_finite() {
            Point::new((br.x - tl.x) / d, (br.y - tl.y) / d)
        } else {
            Point::new(
                std::f64::consts::FRAC_1_SQRT_2,
                std::f64::consts::FRAC_1_SQRT_2,
            )
        };
        let k = TRANSLATE_OFFSET_HW * hw;
        (
            anchor,
            scale,
            Point::new(br.x + dir.x * k, br.y + dir.y * k),
        )
    }

    /// The handles in the opacity-pulse order (anchor, NW, N, NE, E, SE, S, SW, W, translate),
    /// each pulsing (`nubs::pulse_opacity`) and grown while hovered (§2.10).
    pub fn handles(&self, f: &Matrix, base_bounds: &Rectangle, hw: f64, now_s: f64) -> Vec<Handle> {
        if self.hidden {
            return vec![];
        }
        let (anchor, scale, translate) = self.points(f, base_bounds, hw);
        let hits = std::iter::once(MoveHit::Anchor)
            .chain(Edge::ALL.map(MoveHit::Scale))
            .chain([MoveHit::Translate]);
        let points = std::iter::once(anchor).chain(scale).chain([translate]);
        hits.zip(points)
            .enumerate()
            .map(|(i, (hit, p))| Handle {
                canvas: [p.x, p.y],
                shape: if hit == MoveHit::Translate {
                    HandleShape::Compass
                } else {
                    HandleShape::Circle
                },
                opacity: nubs::pulse_opacity(i, now_s),
                hover: match self.hover {
                    Some((h, since)) if h == hit => {
                        ((now_s - since) / nubs::HOVER_GROW_S).clamp(0.0, 1.0) as f32
                    }
                    _ => 0.0,
                },
            })
            .collect()
    }

    /// The nearest handle within reach (circles 12 DIP, the compass a 15-DIP box; a tiny
    /// selection is dragged by the compass), else the transformed bounds (Background), else the
    /// rotate zone 32 DIP around them, else Background.
    /// `hw` is the canvas size of one device px.
    pub fn hit_test(&self, p: Point, f: &Matrix, base_bounds: &Rectangle, hw: f64) -> MoveHit {
        self.hit_test_with_anchor(p, f, base_bounds, hw, true)
    }

    /// Shapes hides its anchor in translate/scale modes; hidden controls cannot win hits.
    /// Move tools keep the ordinary `hit_test` path and its existing anchor behavior.
    pub fn hit_test_with_anchor(
        &self,
        p: Point,
        f: &Matrix,
        base_bounds: &Rectangle,
        hw: f64,
        anchor_visible: bool,
    ) -> MoveHit {
        if f.invert().is_none() || !(hw.is_finite() && hw > 0.0) {
            return MoveHit::None;
        }
        let px = self.u() * hw;
        let (anchor, scale, translate) = self.points(f, base_bounds, hw);
        let circle = CIRCLE_HIT_DIP * px;
        // The transformed bounds' outline (paintdotjs `isPointInsideTransform`).
        let quad = [scale[0], scale[2], scale[4], scale[6]];
        let mut sign = 0.0;
        let mut inside = true;
        let mut edge = f64::INFINITY;
        for i in 0..4 {
            let (a, b) = (quad[i], quad[(i + 1) % 4]);
            let cross = (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
            if cross.abs() >= 1e-7 {
                let s = cross.signum();
                if sign != 0.0 && s != sign {
                    inside = false;
                }
                sign = s;
            }
            let (dx, dy) = (b.x - a.x, b.y - a.y);
            let len2 = dx * dx + dy * dy;
            let t = if len2 > 0.0 {
                (((p.x - a.x) * dx + (p.y - a.y) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            edge = edge.min(dist(p, Point::new(a.x + t * dx, a.y + t * dy)));
        }
        let mut best: Option<(f64, MoveHit)> = None;
        let mut consider = |d: f64, inside: bool, hit: MoveHit| {
            if inside && best.is_none_or(|(b, _)| d < b) {
                best = Some((d, hit));
            }
        };
        if anchor_visible {
            consider(dist(p, anchor), dist(p, anchor) <= circle, MoveHit::Anchor);
        }
        for (e, c) in Edge::ALL.into_iter().zip(scale) {
            consider(dist(p, c), dist(p, c) <= circle, MoveHit::Scale(e));
        }
        let boxed = COMPASS_HIT_DIP * px;
        let near = (p.x - translate.x).abs() <= boxed && (p.y - translate.y).abs() <= boxed;
        consider(dist(p, translate), near, MoveHit::Translate);
        if let Some((_, hit)) = best {
            return hit;
        }
        if !inside && edge <= ROTATE_ZONE_DIP * px {
            MoveHit::Rotate
        } else {
            MoveHit::Background
        }
    }

    /// The pointer moved over `hit` at `now_s`: a handle starts (or keeps) its hover growth.
    pub fn set_hover(&mut self, hit: MoveHit, now_s: f64) {
        let handle = matches!(
            hit,
            MoveHit::Scale(_) | MoveHit::Anchor | MoveHit::Translate
        );
        self.hover = match self.hover {
            Some((h, since)) if h == hit => Some((h, since)),
            _ if handle => Some((hit, now_s)),
            _ => None,
        };
    }
}

/// What a pointer gesture does (its history name, §2.8 "Names").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Gesture {
    Translate,
    Rotate,
    Scale(Edge),
    /// R230: a corner (0–3: TL, TR, BR, BL) dragged alone.
    Distort(usize),
    Anchor,
    /// The paste that opened the transaction (§2.11 step 3).
    Paste,
}

/// The move tools' changes record: F = `base`·`m`.
#[derive(Clone, Debug)]
pub struct MoveToolContext {
    /// Base: the selection's interim transform when the transaction began.
    pub base: Matrix,
    /// The selection's bounds before its interim transform.
    pub base_bounds: Rectangle,
    /// M = Delta·Edit.
    pub m: Matrix,
    /// The rotation anchor in base space (`None` = the centre of `base_bounds`).
    pub anchor: Option<Point>,
    /// Leave a copy behind (Move Selected Pixels): set by a pointer edit that began with Ctrl
    /// (ruling R69, never by a nudge) and kept by every later edit of the transaction.
    pub copy: bool,
    /// The tracked toolbar values: `resampling`, `gammaCorrected` (null for Move Selection).
    settings: [Value; 2],
    /// A paste's bitmap: the source instead of the layer, at the selection's untransformed
    /// coordinates (§2.11 step 3), with no vacated area (b = 0, compositing.md §2.8).
    pub paste: Option<Arc<Surface>>,
    /// S§4.6 (Extra): the Shift trail, each stamp's M (and quad), oldest first. Stamps stay where
    /// they were left whatever later gestures do to M, so each keeps its whole M, not an offset.
    pub trail: Vec<Stamp>,
    /// R230 (Extra): the distorted corners, once a corner was dragged alone.
    pub quad: Option<Quad>,
    /// The selection when the transaction began (interim = `base`): a distort replaces the
    /// selection's paths, and these give them back (fixed per transaction; not part of equality).
    selection: Arc<SelectionData>,
    /// The last gesture (names only; not part of equality).
    gesture: Gesture,
}

impl PartialEq for MoveToolContext {
    fn eq(&self, o: &Self) -> bool {
        (
            &self.base,
            &self.base_bounds,
            &self.m,
            &self.anchor,
            self.copy,
            &self.settings,
            &self.trail,
            &self.quad,
        ) == (
            &o.base,
            &o.base_bounds,
            &o.m,
            &o.anchor,
            o.copy,
            &o.settings,
            &o.trail,
            &o.quad,
        ) && match (&self.paste, &o.paste) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (a, b) => a.is_none() && b.is_none(),
        }
    }
}

impl MoveToolContext {
    /// F = Base·M.
    pub fn transform(&self) -> Matrix {
        then(&self.base, &self.m)
    }

    /// R230: base space → canvas with `quad` as the distort (`None` for a degenerate one).
    fn homography(&self, quad: Quad) -> Option<Homography> {
        let bb = &self.base_bounds;
        Homography::rect_to_quad([bb.left(), bb.top(), bb.right(), bb.bottom()], quad)
            .filter(|h| h.inverse().is_some())
    }

    /// The distort's base space → canvas map.
    fn distort(&self) -> Option<Homography> {
        self.homography(self.quad?)
    }

    /// Base space → canvas: the distort, else F.
    fn to_canvas(&self, p: Point) -> Point {
        match self.distort() {
            Some(h) => map(&h, p),
            None => apply(&self.transform(), p),
        }
    }

    /// Canvas → base space.
    fn to_base(&self, p: Point) -> Option<Point> {
        match self.distort() {
            Some(h) => h.inverse().map(|h| map(&h, p)),
            None => self.transform().invert().map(|inv| apply(&inv, p)),
        }
    }

    /// The base bounds' corners on the canvas.
    fn corners(&self) -> Quad {
        self.quad
            .unwrap_or_else(|| corners(&self.base_bounds).map(|c| xy(self.to_canvas(c))))
    }

    /// Where stamp (or live copy) `s` puts the base bounds.
    fn bounds(&self, s: &Stamp) -> Rectangle {
        match s.1 {
            Some(q) => points_bounds(q.map(|c| Point::new(c[0], c[1]))),
            None => transformed_bounds(&then(&self.base, &s.0), &self.base_bounds),
        }
    }

    /// The pixels' source → destination map under quad `q`: the distort after Base⁻¹. A quad has
    /// a homography (the gesture keeps no other); without, the zero map renders nothing.
    fn pixel_map(&self, q: Option<Quad>) -> Option<Homography> {
        let q = q?;
        let h = self.homography(q).zip(self.base.invert());
        Some(h.map_or(Homography([0.0; 9]), |(h, inv)| h.mul(&hom(&inv))))
    }

    /// The selection the record shows when distorted: the transaction's own through the distort,
    /// as plain paths (a homography keeps straight edges straight, so mapping the vertices is
    /// exact); else that selection under F.
    fn shown_selection(&self) -> SelectionData {
        let mut data = (*self.selection).clone();
        match self.distort() {
            Some(h) => {
                let mut path = GraphicsPath::combine(&data.base, data.mode, &data.continuation);
                for p in path.polygons.iter_mut().flatten() {
                    *p = map(&h, *p);
                }
                SelectionData {
                    base: path,
                    continuation: GraphicsPath::default(),
                    mode: CombineMode::Xor,
                    interim: Matrix::identity(),
                    ..data
                }
            }
            None => {
                data.interim = self.transform();
                data
            }
        }
    }

    /// A gesture that edits F (F′ = F·`e`, `e` on the canvas) moves the quad by `e` too.
    fn edit_quad(&mut self, e: &Matrix) {
        if let Some(q) = self.quad {
            self.quad = Some(q.map(|c| xy(apply(e, Point::new(c[0], c[1])))));
        }
    }

    fn interpolation(&self) -> MoveInterpolation {
        MoveInterpolation::from_value(self.settings[0].as_str().unwrap_or_default())
    }

    /// The gamma toggle: `true` (or anything but `false`) is Linear.
    fn gamma(&self) -> Gamma {
        let v = &self.settings[1];
        if v.as_bool() == Some(false) || v.as_str() == Some("false") {
            Gamma::Companded
        } else {
            Gamma::Linear
        }
    }
}

/// A pointer drag in progress.
#[derive(Clone, Debug)]
struct Drag {
    button: MouseButton,
    start: Point,
    last: Point,
    /// The record at the press (its trail grows with the drag's stamps).
    from: MoveToolContext,
    gesture: Gesture,
    /// S§4.6: where the last applied step put the pixels, the press's placement at first.
    at: Stamp,
    /// Snap (Extra): a translate's targets and the moved bounds at the press, with Snap on.
    snap: Option<(SnapTargets, [f64; 4])>,
    /// The alignments the last step snapped to.
    lines: Vec<SnapLine>,
}

/// The gesture feeding the open Drawing or pointer edit.
#[derive(Clone, Debug)]
enum Live {
    Pointer(Box<Drag>),
    /// A nudge burst: the time of its last arrow key.
    Nudge(f64),
}

/// Move Selected Pixels (`PIXELS`) and Move Selection: one transaction per lift, on the
/// transacted core, so the commit-first hook (deactivate), undo re-opening, per-image parking and
/// the tool-entry stepping are the core's.
#[derive(Default)]
pub struct MoveToolBase<const PIXELS: bool> {
    tx: Transaction<MoveToolContext>,
    /// Pixels: the layer as it was when the transaction began (every render starts from it).
    bitmap: ToolBase,
    control: TransformHandles,
    live: Option<Live>,
    /// The latest pointer position of a drag, applied once per frame (paintdotjs's
    /// requestAnimationFrame throttle).
    pending: Option<Point>,
    handles: Option<RendererId>,
    /// Pixels: what the transaction's renders may have changed (Q ∪ the last footprint ∪ the
    /// trail's stamps).
    rendered: IntRect,
    base_mask: Option<(SelectionQuality, Arc<Mask>)>,
    /// The samplers' source, kept while its source pixels, Q, gamma and pre-scale hold.
    cache: MoveCache,
    /// The last render's work: stamp pixels composited plus pixels rendered (S§4.6's bound).
    work: u64,
    /// Pixels: the Shift trail baked so far, while the records only add stamps to it.
    trail_bake: Option<TrailBake>,
    /// The selection's interim transform and base bounds while Idle, by selection revision.
    idle: Option<(u64, Matrix, Rectangle)>,
    cursor: Option<CursorRequest>,
    /// R230: the selection's paths were last shown distorted (not the record's own).
    distorted: bool,
    /// Pixels: the pixels a pointer drag lifted out (`AppWorkspace::float_moves`) and what they
    /// were lifted for (Q, copy behind, Base, quality, paste).
    lift: Option<(LiftKey, Arc<FloatSource>)>,
}

/// What a lift's pixels and hole depend on (a pointer drag changes none of it).
type LiftKey = (IntRect, bool, Matrix, SelectionQuality, Option<usize>);

/// S§4.6 (Extra): the Shift trail baked into the untouched layer (`bake_stamps`) and what it was
/// baked for, under the transaction's Base (kept and dropped with bm).
// ponytail: layer-sized (5 B/px, beside the transaction's own copy of the layer); bound it to
// the stamps' union if memory bites.
struct TrailBake {
    quality: SelectionQuality,
    copy: bool,
    settings: [Value; 2],
    trail: Vec<Stamp>,
    /// The selection under Base over Q: the stamps' final masks.
    mask: Mask,
    px: Vec<Option<Px>>,
    /// The live copy's footprint at the last render.
    live: IntRect,
}

/// Move Selected Pixels.
pub type MoveTool = MoveToolBase<true>;
/// Move Selection.
pub type MoveSelectionTool = MoveToolBase<false>;

impl MoveTool {
    /// §2.11 step 3, beginning a paste (from Idle): the selection becomes `geometry` (relative to
    /// `bitmap`'s origin) and the transaction opens with Delta = Translate(`at`), an integer
    /// translation (NN, exact pixels); the drawing ends at once as "Paste", so the paste floats
    /// until its commit. False when a transaction is open.
    pub fn begin_paste(
        &mut self,
        app: &mut AppWorkspace,
        bitmap: Arc<Surface>,
        geometry: GraphicsPath,
        at: (i32, i32),
    ) -> bool {
        const ICON: &str = "assets/icons/menu_edit_paste_icon.png";
        if self.tx.state() != State::Idle {
            return false;
        }
        let settings = Self::toolbar(app);
        let Some(ws) = app.active_ws_mut() else {
            return false;
        };
        let name = Self::gesture_name(Gesture::Paste);
        let snapshot =
            HistoryMemento::new(&name, ICON, MementoKind::Selection(ws.selection.save()));
        ws.selection.reset();
        ws.selection
            .set_continuation_path(geometry, CombineMode::Replace);
        ws.selection.commit_continuation();
        let mut m = Matrix::identity();
        m.translate(at.0 as f64, at.1 as f64);
        let r = MoveToolContext {
            base: Matrix::identity(),
            base_bounds: ws.selection.create_path(false).get_bounds(),
            m,
            anchor: None,
            copy: false,
            settings,
            gesture: Gesture::Paste,
            paste: Some(bitmap),
            trail: vec![],
            quad: None,
            selection: Arc::new(ws.selection.save()),
        };
        transacted::begin_drawing(self, app, r, Some(snapshot));
        transacted::end(self, app);
        // The entry shows the Paste command's icon, not the tool's.
        if let Some(e) = app
            .active_ws_mut()
            .and_then(|ws| ws.history.undo.last_mut())
        {
            e.image = ICON.into();
        }
        true
    }
}

impl<const PIXELS: bool> MoveToolBase<PIXELS> {
    const PREFIX: &'static str = if PIXELS {
        "moveTool"
    } else {
        "moveSelectionTool"
    };

    /// The transaction's state.
    pub fn state(&self) -> State {
        self.tx.state()
    }

    /// The open record.
    pub fn context(&self) -> Option<&MoveToolContext> {
        self.tx.record()
    }

    fn gesture_name(g: Gesture) -> String {
        let key = match g {
            Gesture::Translate => "translate",
            Gesture::Rotate => "rotate",
            Gesture::Scale(_) => "scale",
            Gesture::Distort(_) => "distort",
            Gesture::Anchor => return t("moveSelectionTool.historyMemento.moveRotationAnchor"),
            Gesture::Paste => return t("commonAction.paste"),
        };
        t(&format!("{}.historyMemento.{key}", Self::PREFIX))
    }

    fn toolbar(app: &AppWorkspace) -> [Value; 2] {
        if PIXELS {
            ["resampling", "gammaCorrected"].map(|k| app.tool_settings.get(ToolType::Move, k))
        } else {
            [Value::Null, Value::Null]
        }
    }

    /// F and the base bounds: the record's, or the selection's while Idle; `None` without a
    /// selection.
    fn frame(&mut self, app: &AppWorkspace) -> Option<(Matrix, Rectangle)> {
        if let Some(r) = self.tx.record() {
            return Some((r.transform(), r.base_bounds));
        }
        let s = &app.active_ws()?.selection;
        if s.is_empty() {
            return None;
        }
        match self.idle {
            Some((rev, f, bb)) if rev == s.revision() => Some((f, bb)),
            _ => {
                let (f, bb) = (s.get_interim_transform(), s.create_path(false).get_bounds());
                self.idle = Some((s.revision(), f, bb));
                Some((f, bb))
            }
        }
    }

    /// The canvas size of one device px.
    fn hair_width(app: &AppWorkspace) -> Option<(f64, f64)> {
        let t = app.active_ws()?.view.view_transform();
        (t.scale > 0.0 && t.scale.is_finite()).then(|| (1.0 / t.scale, t.ui_scale))
    }

    fn hit(&mut self, app: &AppWorkspace, p: Point) -> MoveHit {
        let (Some((f, bb)), Some((hw, u))) = (self.frame(app), Self::hair_width(app)) else {
            return MoveHit::None;
        };
        self.control.ui_scale = u;
        self.control.hit_test(p, &f, &bb, hw)
    }

    /// Writes the handle list when it changed (each write redraws the overlays).
    fn sync_handles(&mut self, app: &mut AppWorkspace) {
        let list = match (self.frame(app), Self::hair_width(app)) {
            (Some((f, bb)), Some((hw, u))) => {
                self.control.ui_scale = u;
                self.control.handles(&f, &bb, hw, app.now_ms / 1000.0)
            }
            _ => vec![],
        };
        let (Some(id), Some(ws)) = (self.handles, app.active_ws_mut()) else {
            return;
        };
        if matches!(ws.surface_box.renderer(id), Some(Renderer::Handles(h)) if *h == list) {
            return;
        }
        if let Some(Renderer::Handles(h)) = ws.surface_box.renderer_mut(id) {
            *h = list;
        }
    }

    /// The pointer drag in progress, if the transaction still holds it (an undo or Enter may have
    /// ended it: its moves and release are then ignored).
    fn dragging(&mut self) -> Option<MouseButton> {
        let open = matches!(
            (self.tx.state(), self.tx.edit()),
            (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
        );
        if !open {
            self.live = None;
            self.pending = None;
            self.control.hidden = false;
        }
        match &self.live {
            Some(Live::Pointer(d)) => Some(d.button),
            _ => None,
        }
    }

    /// §2.8 "Begin on an empty selection" (a "Select All" entry first), then Idle → Drawing with
    /// the selection snapshot.
    fn begin(&mut self, app: &mut AppWorkspace, gesture: Gesture, copy: bool) {
        // S§4.5: moving a text layer's pixels asks to rasterize it first (presses and nudges).
        if PIXELS && !crate::document::text_layer::ensure_active_raster(app) {
            return;
        }
        let settings = Self::toolbar(app);
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        if ws.selection.is_empty() {
            ws.execute_function(functions::select_all);
        }
        let selection = ws.selection.save();
        let snapshot = HistoryMemento::new(
            &t(&format!("{}.name", Self::PREFIX)),
            &Self::TOOL.icon_src(),
            MementoKind::Selection(selection.clone()),
        );
        let r = MoveToolContext {
            base: ws.selection.get_interim_transform(),
            base_bounds: ws.selection.create_path(false).get_bounds(),
            m: Matrix::identity(),
            anchor: None,
            copy: PIXELS && copy,
            settings,
            gesture,
            paste: None,
            trail: vec![],
            quad: None,
            selection: Arc::new(selection),
        };
        transacted::begin_drawing(self, app, r, Some(snapshot));
    }

    /// Opens a gesture: Idle begins a transaction (Drawing); Dirty (or a settings window, which
    /// ends first with its entry) becomes a pointer edit. `copy` is the copy-behind of a new
    /// transaction.
    fn open_gesture(&mut self, app: &mut AppWorkspace, gesture: Gesture, copy: bool) {
        match (self.tx.state(), self.tx.edit()) {
            (State::Idle, _) => self.begin(app, gesture, copy),
            (State::Dirty, _) | (State::Editing, Some(Edit::Settings(_))) => {
                transacted::end(self, app);
                transacted::begin_edit(self, Edit::Pointer);
            }
            _ => {}
        }
    }

    /// S§4.6 (Extra): Shift on a Move Selected Pixels translate (a drag or an arrow, R227) leaves
    /// a trail (Paint.NET gives Shift no meaning there).
    fn trails(shift: bool) -> bool {
        PIXELS && shift
    }

    /// R230 (Extra): with Distort on, a Move Selected Pixels corner handle drags its corner
    /// alone (0–3: TL, TR, BR, BL).
    fn distort_corner(app: &AppWorkspace, e: Edge) -> Option<usize> {
        let on = PIXELS && app.tool_settings.get(ToolType::Move, "distort") == true;
        let corner = match e {
            Edge::TopLeft => 0,
            Edge::TopRight => 1,
            Edge::BottomRight => 2,
            Edge::BottomLeft => 3,
            _ => return None,
        };
        on.then_some(corner)
    }

    /// Snap (Extra): a translate's targets, built at the press with Snap on. Move Selected Pixels
    /// leaves out the layer it lifts from; where the pixels started is one: their place at the
    /// press and, after an earlier drag, where the move began (the hole the lift left).
    fn snap_targets(app: &AppWorkspace, from: &MoveToolContext) -> Option<(SnapTargets, [f64; 4])> {
        if app.tool_settings.get(Self::TOOL, "snap") != true {
            return None;
        }
        let ws = app.active_ws()?;
        let mut targets = SnapTargets::from_workspace(ws, PIXELS.then_some(ws.active_layer));
        let edges = |b: Rectangle| [b.left(), b.top(), b.right(), b.bottom()];
        let start = edges(from.bounds(&(from.m, from.quad)));
        targets.add_origin(start);
        let began = edges(from.bounds(&(Matrix::identity(), None)));
        // A paste began nowhere on the canvas.
        if from.paste.is_none() && began != start {
            targets.add_origin(began);
        }
        Some((targets, start))
    }

    /// Snap (Extra): the translate offset `d` with the moved bounds snapped within 8 screen px
    /// (PhotoCraft's Move tool); Ctrl held while dragging moves freely. Keeps the alignments.
    fn snap_step(app: &AppWorkspace, d: &mut Drag, offset: [f64; 2]) -> [f64; 2] {
        d.lines.clear();
        let (Some((targets, rect)), Some((hw, u))) = (&d.snap, Self::hair_width(app)) else {
            return offset;
        };
        if app.control_key_down {
            return offset;
        }
        let (offset, lines) = targets.snap_move(*rect, offset, SNAP_PX * u * hw);
        d.lines = lines;
        offset
    }

    /// The record the drag `d` makes at `p` (§2.8 rows "Translate", "Rotate", "Scale handles",
    /// "Shift on scale", "Scale pixel snap"); Shift and Alt are read live.
    fn gesture_record(app: &AppWorkspace, d: &mut Drag, p: Point) -> MoveToolContext {
        let mut r = d.from.clone();
        r.gesture = d.gesture;
        let f0 = d.from.transform();
        let bb = d.from.base_bounds;
        let centre = Point::new(bb.x + bb.width / 2.0, bb.y + bb.height / 2.0);
        let (dx, dy) = (p.x - d.start.x, p.y - d.start.y);
        let (shift, alt) = (app.shift_key_down, app.alt_key_down);
        match d.gesture {
            // A drag is never a paste; it only names the opening entry.
            Gesture::Translate | Gesture::Paste => {
                let [dx, dy] = Self::snap_step(app, d, [dx, dy]);
                let (tx, ty) = truncate_translation(dx, dy);
                let mut e = Matrix::identity();
                e.translate(tx, ty);
                r.m = then(&d.from.m, &e);
                r.edit_quad(&e);
                // S§4.6 (Extra), R253: Shift stamps where trailing began and each new offset.
                let stamp = (r.m, r.quad);
                if Self::trails(shift) {
                    push_stamps(&mut d.from.trail, d.at, stamp);
                    push_stamps(&mut r.trail, d.at, stamp);
                }
                d.at = stamp;
            }
            // The corner follows the pointer until the quad would fold (`keep_convex`, either
            // winding: a flipped F winds the other way); a quad with no homography is refused.
            Gesture::Distort(i) => {
                let q0 = d.from.corners();
                let mut q = q0;
                q[i] = [q0[i][0] + dx, q0[i][1] + dy];
                let q = keep_convex(q0, q);
                if d.from.homography(q).is_some() {
                    r.quad = Some(q);
                }
            }
            Gesture::Rotate => {
                let c = d.from.to_canvas(d.from.anchor.unwrap_or(centre));
                let mut angle = to_degrees(
                    (p.y - c.y).atan2(p.x - c.x) - (d.start.y - c.y).atan2(d.start.x - c.x),
                );
                if shift {
                    let before = angle_of(&f0);
                    angle = constrain_angle(before + angle) - before;
                }
                let mut e = Matrix::identity();
                e.rotate_at(angle, c);
                r.m = then(&d.from.m, &e);
                r.edit_quad(&e);
            }
            Gesture::Anchor => {
                let c = d.from.to_canvas(d.from.anchor.unwrap_or(centre));
                r.anchor = d.from.to_base(Point::new(c.x + dx, c.y + dy));
            }
            Gesture::Scale(edge) => {
                let (Some(f0_inv), Some(base_inv)) = (f0.invert(), d.from.base.invert()) else {
                    return r;
                };
                // The drag in base space (F⁻¹ at the drag start).
                let mut v = [Point::new(dx, dy)];
                f0_inv.transform_vectors(&mut v);
                let (ex, ey) = edge.dir();
                let (hx, vy) = (ex != 0.0 && bb.width > 0.0, ey != 0.0 && bb.height > 0.0);
                if !(hx || vy) {
                    return r;
                }
                // Alt moves both opposite edges (scale about the centre).
                let mult = if alt { 2.0 } else { 1.0 };
                let rx = if hx {
                    (bb.width + mult * ex * v[0].x) / bb.width
                } else {
                    f64::INFINITY
                };
                let ry = if vy {
                    (bb.height + mult * ey * v[0].y) / bb.height
                } else {
                    f64::INFINITY
                };
                let (mut sx, mut sy) = if shift {
                    let s = rx.min(ry);
                    (s, s)
                } else {
                    (if hx { rx } else { 1.0 }, if vy { ry } else { 1.0 })
                };
                let unit = |x: f64, y: f64| {
                    let mut v = [Point::new(x, y)];
                    f0.transform_vectors(&mut v);
                    v[0].x.hypot(v[0].y)
                };
                let k = |scalers: bool| {
                    if (shift && !scalers) || (alt && scalers) {
                        2
                    } else {
                        1
                    }
                };
                if hx || shift {
                    sx = snap_scale(sx, bb.width * unit(1.0, 0.0), k(hx));
                }
                if vy || shift {
                    sy = snap_scale(sy, bb.height * unit(0.0, 1.0), k(vy));
                }
                // The anchor: the opposite edge or corner (the centre with Alt, or on an axis
                // without moving edges).
                let anchor = |active: bool, e: f64, lo: f64, hi: f64, mid: f64| {
                    if alt || !active {
                        mid
                    } else if e > 0.0 {
                        lo
                    } else {
                        hi
                    }
                };
                let ax = anchor(hx, ex, bb.left(), bb.right(), centre.x);
                let ay = anchor(vy, ey, bb.top(), bb.bottom(), centre.y);
                let mut s = Matrix::identity();
                s.translate(-ax, -ay);
                s.scale(sx, sy);
                s.translate(ax, ay);
                // F′ = S·F0 (S in base space), so M′ = Base⁻¹·F′.
                r.m = then(&base_inv, &then(&s, &f0));
                r.edit_quad(&then(&f0_inv, &then(&s, &f0)));
            }
        }
        r
    }

    /// Applies the drag at `p`.
    fn drag_to(&mut self, app: &mut AppWorkspace, p: Point) {
        let Some(Live::Pointer(d)) = &mut self.live else {
            return;
        };
        d.last = p;
        let r = Self::gesture_record(app, d, p);
        transacted::update(self, app, r);
    }

    /// An arrow key: 1 px (Ctrl: 10 px) through the keyboard edit (§2.8 "Nudge"); with Shift,
    /// trail stamps where the step starts and lands, as the drag's (R227, R253).
    fn nudge(&mut self, app: &mut AppWorkspace, dx: f64, dy: f64, shift: bool) {
        if !matches!(self.live, Some(Live::Nudge(_))) {
            self.live = None;
            // A nudge never latches the copy-behind (R69): a new transaction moves the pixels,
            // an edit keeps the record's.
            self.open_gesture(app, Gesture::Translate, false);
        }
        let open = matches!(
            (self.tx.state(), self.tx.edit()),
            (State::Drawing, _) | (State::Editing, Some(Edit::Pointer))
        );
        let Some(r) = self.tx.record().filter(|_| open) else {
            return;
        };
        let mut n = r.clone();
        n.gesture = Gesture::Translate;
        let mut e = Matrix::identity();
        e.translate(dx, dy);
        n.m = then(&n.m, &e);
        n.edit_quad(&e);
        if Self::trails(shift) {
            push_stamps(&mut n.trail, (r.m, r.quad), (n.m, n.quad));
        }
        transacted::update(self, app, n);
        self.live = Some(Live::Nudge(app.now_ms));
    }

    /// A nudge burst ends with its entry.
    fn end_nudge(&mut self, app: &mut AppWorkspace) {
        if matches!(self.live, Some(Live::Nudge(_))) {
            self.live = None;
            if self.tx.edit() == Some(Edit::Pointer) || self.tx.state() == State::Drawing {
                transacted::end(self, app);
            }
        }
    }

    /// §2.8 "Enter or Esc": the drag or burst ends as it stands and the transaction commits
    /// ("Finish Selection"), then the Deselect command runs as its own entry (ruling R66).
    fn finish_and_deselect(&mut self, app: &mut AppWorkspace) {
        self.live = None;
        self.pending = None;
        self.control.hidden = false;
        transacted::commit(self, app);
        transacted::deselect(app);
    }

    /// The selection under Base at `quality` (bm), once per transaction and quality.
    fn base_mask(
        &mut self,
        r: &MoveToolContext,
        quality: SelectionQuality,
        doc: IntRect,
    ) -> Arc<Mask> {
        if let Some((q, m)) = &self.base_mask
            && *q == quality
        {
            return m.clone();
        }
        let m = under_base(r).clipping_mask(quality, doc);
        self.base_mask = Some((quality, m.clone()));
        m
    }

    /// §2.9: re-renders the affected rect (the integer bounds of Base·B ∪ F·B, B = `base_bounds`,
    /// plus what the last render touched) from the untouched layer, so preview and commit match.
    ///
    /// With `lift` (a pointer drag, `AppWorkspace::float_moves`) the layer keeps the pixels
    /// lifted out, rendered once with nothing placed, and the canvas draws them through
    /// `DocumentWorkspace::float`; otherwise they are rendered in and a float ends.
    fn render(
        &mut self,
        ws: &mut DocumentWorkspace,
        r: &MoveToolContext,
        quality: SelectionQuality,
        lift: bool,
    ) {
        let doc = IntRect::new(0, 0, ws.document.width, ws.document.height);
        let lift = lift && r.trail.is_empty() && floats(ws);
        if !lift {
            ws.float = None;
            self.lift = None;
        }
        if !self.bitmap.is_open() {
            self.bitmap.begin_bitmap_transaction(ws);
            self.rendered = IntRect::default();
            self.base_mask = None;
            self.trail_bake = None;
            self.lift = None;
        }
        let footprint = IntRect::bound_of(&r.bounds(&(r.m, r.quad))).intersect(&doc);
        // A paste samples its own bitmap and vacates nothing; a move samples the untouched layer
        // inside Q = the integer bounds of Base·`base_bounds`, ∩ layer.
        let (q, touched) = match &r.paste {
            Some(p) => (p.int_bounds(), footprint),
            None => {
                let q =
                    IntRect::bound_of(&transformed_bounds(&r.base, &r.base_bounds)).intersect(&doc);
                (q, q.union(&footprint))
            }
        };
        let key: LiftKey = (
            q,
            r.copy,
            r.base,
            quality,
            r.paste.as_ref().map(|p| Arc::as_ptr(p) as usize),
        );
        let place = |src: &Arc<FloatSource>, layer: usize| Float {
            src: src.clone(),
            layer,
            m: r.m,
            h: r.pixel_map(r.quad),
            rev: next_id(),
        };
        if lift
            && let Some((k, src)) = &self.lift
            && *k == key
        {
            ws.float = Some(Arc::new(place(src, ws.active_layer)));
            return;
        }
        // S§4.6 (Extra): the trail's stamps, each where its M (or quad) put the selection.
        let stamp = |s: &Stamp| {
            let b = IntRect::bound_of(&r.bounds(s)).intersect(&doc);
            (s.0, r.pixel_map(s.1), b)
        };
        let touched = r.trail.iter().fold(touched, |t, s| t.union(&stamp(s).2));
        // While the record only adds stamps to what is baked, a frame bakes the new ones and
        // renders the live copy over its old ∪ new footprint; else all again from the layer.
        let kept = self.trail_bake.take().filter(|b| {
            (b.quality, b.copy, &b.settings) == (quality, r.copy, &r.settings)
                && r.trail.starts_with(&b.trail)
        });
        let baked = kept.as_ref().map_or(0, |b| b.trail.len());
        let new: Vec<_> = r.trail[baked..].iter().map(stamp).collect();
        let rect = match &kept {
            Some(b) => new
                .iter()
                .fold(b.live.union(&footprint), |t, s| t.union(&s.2)),
            None => touched.union(&self.rendered),
        };
        self.rendered = touched;
        if rect.is_empty() {
            self.trail_bake = kept;
            return;
        }
        let mut bake = (!r.trail.is_empty()).then(|| {
            kept.unwrap_or_else(|| TrailBake {
                quality,
                copy: r.copy,
                settings: r.settings.clone(),
                trail: vec![],
                mask: under_base(r).clipping_mask_in(quality, q, q),
                px: vec![None; doc.w as usize * doc.h as usize],
                live: footprint,
            })
        });
        // A lift places nothing: tc = 0 leaves the hole (or the copy behind).
        let fm = match lift {
            true => Mask {
                rect: IntRect::default(),
                data: vec![],
            },
            false => ws.selection.clipping_mask_in(quality, doc, rect),
        };
        let bm = (!r.copy && r.paste.is_none()).then(|| self.base_mask(r, quality, doc));
        let cache = &mut self.cache;
        let mut work = rect.w as u64 * rect.h as u64;
        let mut lifted = None;
        self.bitmap.write_transaction(ws, rect, |original, layer| {
            let area = layer.int_bounds();
            let p = MoveRender {
                layer: original,
                source: r.paste.as_deref().unwrap_or(original),
                q,
                m: r.m,
                h: r.pixel_map(r.quad),
                interpolation: r.interpolation(),
                gamma: r.gamma(),
                quality,
                final_mask: &fm,
                base_mask: bm.as_deref(),
            };
            if let Some(b) = bake.as_mut() {
                work += bake_stamps(&p, &new, &b.mask, cache, &mut b.px);
                b.trail.extend_from_slice(&r.trail[baked..]);
                b.live = footprint;
            }
            if lift {
                let mask = under_base(r).clipping_mask_in(quality, q, q);
                lifted = Some(Arc::new(FloatSource::new(p.source, q, &mask)));
            }
            let under = bake.as_ref().map(|b| &b.px[..]);
            Some(render_moved_into(
                &p,
                under,
                rect,
                cache,
                layer.data_mut(),
                area,
            ))
        });
        self.trail_bake = bake;
        self.work = work;
        if let Some(src) = lifted {
            ws.float = Some(Arc::new(place(&src, ws.active_layer)));
            self.lift = Some((key, src));
        }
    }

    /// A float's pixels rendered into the layer (the preview without the float).
    fn render_in(&mut self, app: &mut AppWorkspace) {
        let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
        if let (true, Some(r), Some(ws)) = (PIXELS, self.tx.record().cloned(), app.active_ws_mut())
            && ws.float.is_some()
        {
            self.render(ws, &r, quality, false);
        }
    }

    /// A drag's float ends: its pixels are rendered into the layer.
    fn land(&mut self, app: &mut AppWorkspace) {
        if app.active_ws().is_some_and(|ws| ws.float.is_some()) {
            self.show(app);
        }
    }

    fn update_cursor(&mut self, app: &AppWorkspace, p: Point) {
        let closed = CursorRequest::Image("hand_closed_cursor");
        self.cursor = Some(match &self.live {
            Some(Live::Pointer(d)) => match d.gesture {
                Gesture::Translate => CursorRequest::Css("move"),
                _ => closed,
            },
            _ => match self.hit(app, p) {
                MoveHit::Background | MoveHit::None => CursorRequest::Css("move"),
                _ => CursorRequest::Image("hand_open_cursor"),
            },
        });
    }
}

impl<const PIXELS: bool> Transacted for MoveToolBase<PIXELS> {
    type Record = MoveToolContext;
    const TOOL: ToolType = if PIXELS {
        ToolType::Move
    } else {
        ToolType::MoveSelection
    };

    fn transaction(&mut self) -> &mut Transaction<MoveToolContext> {
        &mut self.tx
    }

    /// Draw and generic Edit: the gesture's name; Finish: "Finish Selection" for both tools.
    fn names(r: &MoveToolContext) -> [String; 3] {
        let g = Self::gesture_name(r.gesture);
        [
            g.clone(),
            g,
            t("moveSelectionTool.historyMemento.dropSelection"),
        ]
    }

    /// The transform (named by the gesture), the rotation anchor, Sampling and Gamma Mode.
    fn changed(a: &MoveToolContext, b: &MoveToolContext) -> Vec<String> {
        [
            (a.m != b.m || a.copy != b.copy || a.trail != b.trail || a.quad != b.quad)
                .then(|| Self::gesture_name(b.gesture)),
            (a.anchor != b.anchor)
                .then(|| t("moveSelectionTool.historyMemento.moveRotationAnchor")),
            (a.settings[0] != b.settings[0])
                .then(|| t("transactedTool.changed.moveToolInterpolationMode.historyMementoName")),
            (a.settings[1] != b.settings[1])
                .then(|| t("transactedTool.changed.gammaMode.historyMementoName")),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    fn settings(r: &MoveToolContext) -> Vec<(&'static str, Value)> {
        if PIXELS {
            vec![
                ("resampling", r.settings[0].clone()),
                ("gammaCorrected", r.settings[1].clone()),
            ]
        } else {
            vec![]
        }
    }

    fn with_toolbar(&mut self, app: &mut AppWorkspace, r: &MoveToolContext) -> MoveToolContext {
        MoveToolContext {
            settings: Self::toolbar(app),
            ..r.clone()
        }
    }

    /// The record's transform on the selection (and, for pixels, the layer re-rendered); with no
    /// record the layer gets its pixels back. Then the handles.
    fn show(&mut self, app: &mut AppWorkspace) {
        let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
        let record = self.tx.record().cloned();
        let lift = app.float_moves && matches!(self.live, Some(Live::Pointer(_)));
        if let Some(ws) = app.active_ws_mut() {
            match &record {
                // R230: a distorted selection is plain paths; the record's own come back after.
                Some(r) if r.quad.is_some() || self.distorted => {
                    ws.selection.restore(r.shown_selection());
                    self.distorted = r.quad.is_some();
                    if PIXELS {
                        self.render(ws, r, quality, lift);
                    }
                }
                Some(r) => {
                    ws.selection.set_interim_transform(r.transform());
                    if PIXELS {
                        self.render(ws, r, quality, lift);
                    }
                }
                None => {
                    ws.float = None;
                    self.lift = None;
                    self.bitmap.cancel_bitmap_transaction(ws);
                    self.rendered = IntRect::default();
                    self.base_mask = None;
                    self.trail_bake = None;
                    self.distorted = false;
                }
            }
        }
        self.control.quad = record.as_ref().and_then(|r| r.distort());
        self.control.anchor = record.and_then(|r| r.anchor);
        self.sync_handles(app);
    }

    /// §2.8 "Commit": the pixels entry (already rendered: commit = preview, §2.9 step 7) and, when
    /// the result has no scans (moved off the canvas), the selection reset. Move Selection's
    /// selection is already live, so only the reset can be inside.
    fn commit_inner(&mut self, app: &mut AppWorkspace) -> Option<HistoryMemento> {
        // Commit = preview (§2.9 step 7): a float's pixels are rendered in first.
        self.render_in(app);
        let ws = app.active_ws_mut()?;
        let name = t("moveSelectionTool.historyMemento.dropSelection");
        let icon = Self::TOOL.icon_src();
        let mut parts = vec![];
        if PIXELS {
            parts.extend(self.bitmap.commit_bitmap_transaction(ws, &name, &icon));
        }
        self.rendered = IntRect::default();
        self.base_mask = None;
        self.trail_bake = None;
        // The source is made again for the next transaction anyway (its pixels change); up to
        // w·h·16 bytes not to hold while Idle.
        self.cache = MoveCache::default();
        let doc = IntRect::new(0, 0, ws.document.width, ws.document.height);
        let off_canvas = mask::scans(&ws.selection.clipping_polygons(doc), 1.0).is_empty();
        if off_canvas {
            parts.push(HistoryMemento::new(
                &name,
                &icon,
                MementoKind::Selection(ws.selection.save()),
            ));
            ws.selection.reset();
        }
        match parts.len() {
            0 => None,
            1 => parts.pop(),
            _ => Some(HistoryMemento::new(
                &name,
                &icon,
                MementoKind::Compound(parts),
            )),
        }
    }
}

impl<const PIXELS: bool> Tool for MoveToolBase<PIXELS> {
    fn tool_type(&self) -> ToolType {
        Self::TOOL
    }

    /// Move Selection tints the selection, Move Selected Pixels does not (`canvas-rendering.md
    /// §2.9` step 5); both add the handle layer.
    fn on_activate(&mut self, app: &mut AppWorkspace) {
        transacted::activate(self);
        self.idle = None;
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = !PIXELS;
            self.handles = Some(ws.surface_box.add_renderer(Renderer::Handles(vec![])));
        }
        self.sync_handles(app);
    }

    /// Ends a gesture as it stands and commits (the commit-first hook and tool changes come here).
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        transacted::deactivate(self, app);
        self.cache = MoveCache::default();
        self.live = None;
        self.pending = None;
        self.control = TransformHandles::default();
        let handles = self.handles.take();
        if let Some(ws) = app.active_ws_mut() {
            ws.selection_renderer_mut().tinting = false;
            if let Some(id) = handles {
                ws.surface_box.remove_renderer(id);
            }
        }
    }

    /// The right button rotates anywhere; the left scales on a handle, rotates in the rotate
    /// zone, drags the anchor, else translates. A second button while one drags is ignored.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        if self.dragging().is_some() {
            return true;
        }
        self.end_nudge(app);
        let p = Point::new(x, y);
        let empty = app.active_ws().is_some_and(|ws| ws.selection.is_empty());
        let gesture = match (b, empty, self.hit(app, p)) {
            (MouseButton::Right, ..) => Gesture::Rotate,
            (_, true, _) => Gesture::Translate,
            (_, _, MoveHit::Scale(e)) => match Self::distort_corner(app, e) {
                Some(corner) => Gesture::Distort(corner),
                None => Gesture::Scale(e),
            },
            (_, _, MoveHit::Rotate) => Gesture::Rotate,
            (_, _, MoveHit::Anchor) => Gesture::Anchor,
            _ => Gesture::Translate,
        };
        // §2.8 "Ctrl": held when a pointer edit begins, that edit leaves a copy behind (R69). The
        // copy is the untouched layer kept under the render (bm = 0), so a later edit without Ctrl
        // must not clear the flag: only undoing the Ctrl edit takes the copy back.
        let copy = PIXELS && app.control_key_down;
        self.open_gesture(app, gesture, copy);
        let Some(mut from) = self.tx.record().cloned() else {
            return true;
        };
        if copy && !from.copy {
            from.copy = true;
            transacted::update(self, app, from.clone());
        }
        let snap = (gesture == Gesture::Translate)
            .then(|| Self::snap_targets(app, &from))
            .flatten();
        self.live = Some(Live::Pointer(Box::new(Drag {
            button: b,
            start: p,
            last: p,
            at: (from.m, from.quad),
            from,
            gesture,
            snap,
            lines: vec![],
        })));
        self.control.hidden = matches!(gesture, Gesture::Translate | Gesture::Rotate);
        self.sync_handles(app);
        self.update_cursor(app, p);
        true
    }

    /// A drag keeps only the latest point; `on_frame` applies it (one move per frame). Otherwise
    /// the hovered handle grows and the cursor follows.
    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        let p = Point::new(x, y);
        if self.dragging().is_some() {
            self.pending = Some(p);
            return true;
        }
        let hit = self.hit(app, p);
        self.control.set_hover(hit, app.now_ms / 1000.0);
        self.update_cursor(app, p);
        self.sync_handles(app);
        false
    }

    /// The release applies its point and ends the drag: the Draw or Edit entry.
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        let Some(button) = self.dragging() else {
            return false;
        };
        if b != button {
            return true;
        }
        let p = Point::new(x, y);
        self.pending = None;
        self.drag_to(app, p);
        self.live = None;
        self.land(app);
        self.control.hidden = false;
        transacted::end(self, app);
        self.update_cursor(app, p);
        self.sync_handles(app);
        true
    }

    /// Arrow keys nudge by every held arrow (not during a drag); Enter and Escape commit and
    /// deselect (§2.8).
    fn on_key_press(&mut self, app: &mut AppWorkspace, key: &KeyPress) -> bool {
        if key.meta {
            return false;
        }
        let arrow = super::arrow_nudge(app, &key.key);
        match (arrow, key.key.as_str()) {
            (Some((dx, dy)), _) if !key.alt => {
                if self.dragging().is_none() && (dx, dy) != (0.0, 0.0) {
                    let step = if key.ctrl { 10.0 } else { 1.0 };
                    self.nudge(app, dx * step, dy * step, key.shift);
                }
                true
            }
            (_, "Enter" | "Escape") if !(key.ctrl || key.alt) => {
                self.finish_and_deselect(app);
                true
            }
            _ => false,
        }
    }

    /// Shift and Alt act at once on a drag.
    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        if self.dragging().is_some()
            && let Some(Live::Pointer(d)) = &self.live
        {
            let p = d.last;
            self.pending = None;
            self.drag_to(app, p);
        }
    }

    /// Sampling and Gamma Mode are tracked (a settings edit while Dirty); the shared selection
    /// quality just re-renders.
    fn on_setting_changed(&mut self, app: &mut AppWorkspace, key: &str) {
        if PIXELS && matches!(key, "resampling" | "gammaCorrected") {
            transacted::setting_changed(self, app);
        } else if key == "renderingQuality" && self.tx.is_open() {
            self.show(app);
        }
    }

    fn on_executing_history_memento(&mut self, _app: &mut AppWorkspace) {
        self.pending = None;
    }

    /// The throttled drag point, the nudge burst's idle commit, and the handle pulse.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        if self.dragging().is_some()
            && let Some(p) = self.pending.take()
        {
            self.drag_to(app, p);
        }
        if let Some(Live::Nudge(at)) = self.live
            && app.now_ms - at >= NUDGE_COMMIT_MS
        {
            self.end_nudge(app);
        }
        self.sync_handles(app);
    }

    /// A nudge burst's idle commit needs frames even with animations off (no ants repaints).
    fn needs_ticks(&self) -> bool {
        matches!(self.live, Some(Live::Nudge(_)))
    }

    /// The Finish button: Dirty only (`painting-tools-2.md §7.6`).
    fn land_float(&mut self, app: &mut AppWorkspace) {
        self.render_in(app);
    }

    fn commit_pending(&mut self, app: &mut AppWorkspace) -> bool {
        transacted::finish_button(self, app)
    }

    /// Drag deltas are in canvas px (doubles, §2.8 "Coordinates").
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }

    fn transacted(&mut self) -> Option<&mut dyn DynTransacted> {
        Some(self)
    }

    /// `ui-drawn-graphics.md §2.10` "Selection info": while moving, the angle of M (paintdotjs
    /// negates it: clockwise on screen is negative), in (−180, 180].
    // calibrate: "when moving" is read as "while the move transaction is open"; the alternatives
    // are "only while rotated" (paintdotjs) and "whenever a move tool is active" — a Paint.NET
    // screenshot of a plain translate settles it.
    fn status_angle(&self) -> Option<f64> {
        self.tx.record().map(|r| -angle_of(&r.m))
    }

    fn shift_trail_hint(&self) -> bool {
        PIXELS
            && match &self.live {
                Some(Live::Pointer(d)) => d.gesture == Gesture::Translate,
                Some(Live::Nudge(_)) => true,
                None => false,
            }
    }

    fn snap_lines(&self) -> &[SnapLine] {
        match &self.live {
            Some(Live::Pointer(d)) => &d.lines,
            _ => &[],
        }
    }

    fn cursor(&self) -> CursorRequest {
        self.cursor.unwrap_or(CursorRequest::Css("move"))
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// §2.8 rows "Hit testing": circles reach 12 DIP, the compass a 15-DIP box 45 hair widths
    /// beyond the bottom-right corner, the rotate zone 32 DIP outside the bounds.
    #[test]
    fn hit_testing_layout() {
        let c = TransformHandles {
            ui_scale: 1.0,
            ..Default::default()
        };
        let bb = Rectangle::new(100.0, 100.0, 200.0, 100.0);
        let f = Matrix::identity();
        let hit = |x, y| c.hit_test(Point::new(x, y), &f, &bb, 1.0);
        assert_eq!(hit(100.0, 100.0), MoveHit::Scale(Edge::TopLeft));
        assert_eq!(hit(311.0, 150.0), MoveHit::Scale(Edge::Right));
        assert_eq!(hit(200.0, 150.0), MoveHit::Anchor);
        assert_eq!(
            c.hit_test_with_anchor(Point::new(200.0, 150.0), &f, &bb, 1.0, false),
            MoveHit::Background
        );
        // Opting Shapes out does not alter subsequent ordinary Move-tool calls.
        assert_eq!(hit(200.0, 150.0), MoveHit::Anchor);
        assert_eq!(hit(150.0, 150.0), MoveHit::Background);
        let (_, _, tr) = c.points(&f, &bb, 1.0);
        assert_eq!(hit(tr.x + 14.0, tr.y - 14.0), MoveHit::Translate);
        assert_eq!(hit(150.0, 70.0), MoveHit::Rotate);
        assert_eq!(hit(150.0, 60.0), MoveHit::Background);
        // At zoom 2 (hw ½) the reaches halve in canvas px.
        assert_eq!(
            c.hit_test(Point::new(150.0, 85.0), &f, &bb, 0.5),
            MoveHit::Rotate
        );
        assert_eq!(
            c.hit_test(Point::new(150.0, 80.0), &f, &bb, 0.5),
            MoveHit::Background
        );
    }

    /// §2.10 order and pulse phases; a hovered handle grows over 0.2 s.
    #[test]
    fn handle_set_pulses_and_grows() {
        let mut c = TransformHandles::default();
        let bb = Rectangle::new(0.0, 0.0, 10.0, 10.0);
        let f = Matrix::identity();
        c.set_hover(MoveHit::Scale(Edge::Top), 1.0);
        let h = c.handles(&f, &bb, 1.0, 1.1);
        assert_eq!(h.len(), 10);
        assert_eq!(
            (h[0].canvas, h[2].canvas, h[9].shape),
            ([5.0, 5.0], [5.0, 0.0], HandleShape::Compass)
        );
        assert!((h[2].hover - 0.5).abs() < 1e-6 && h[1].hover == 0.0);
        assert_eq!(
            h.iter().map(|h| h.opacity).collect::<Vec<_>>(),
            (0..10)
                .map(|i| nubs::pulse_opacity(i, 1.1))
                .collect::<Vec<_>>()
        );
        c.hidden = true;
        assert!(c.handles(&f, &bb, 1.0, 1.1).is_empty());
    }

    use crate::app::AppEvent;
    use crate::selection::path::GraphicsPath;
    use crate::tool::{KeyPress, PointerInput, PointerSample, PointerType};

    fn input(x: f64, y: f64) -> PointerInput {
        PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![PointerSample {
                x,
                y,
                pressure: None,
                time_ms: 0.0,
            }],
        }
    }

    /// The active Move tool's samplers' source is gone.
    fn cache_empty(app: &mut AppWorkspace) -> bool {
        app.with_tool(|t, _| {
            t.as_any_mut()
                .downcast_mut::<MoveTool>()
                .map(|m| m.cache.is_empty())
        })
        .flatten()
        .expect("the Move tool")
    }

    /// `step`, then a frame: the layer's pixels that changed, and the rects invalidated.
    fn frame(
        app: &mut AppWorkspace,
        step: impl FnOnce(&mut AppWorkspace),
    ) -> (Vec<(i32, i32)>, Vec<Rectangle>) {
        let before = app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .clone();
        step(app);
        app.now_ms += 16.0;
        let now = app.now_ms;
        app.on_frame(now);
        let ws = app.active_ws().unwrap();
        let after = &ws.document.layers.get_at(0).unwrap().surface;
        let changed = (0..after.height() as i32)
            .flat_map(|y| (0..after.width() as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| after.pixel(x as u32, y as u32) != before.pixel(x as u32, y as u32))
            .collect();
        let rects = app.drain_events().into_iter().flat_map(|e| match e {
            AppEvent::DocumentInvalidated { rects } => rects,
            _ => vec![],
        });
        (changed, rects.collect())
    }

    /// Task 27r: a frame of a sparse Move Selected Pixels drag invalidates just the rows' runs it
    /// changed (every changed pixel in one), a dense one the whole render rect. Commit and
    /// deactivation drop the samplers' source (`MoveCache`: memory only).
    #[test]
    fn frames_invalidate_what_changed_and_commits_drop_the_source() {
        let mut app = crate::testing::fresh_doc(200, 150, Some(3));
        let mut cells = GraphicsPath::default();
        for (i, j) in (0..40).map(|k| (17 + k * 41 % 160, 9 + k * 23 % 130)) {
            let (x, y) = (i as f64, j as f64);
            let square = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
            cells
                .polygons
                .push(square.map(|(x, y)| Point::new(x, y)).to_vec());
        }
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation_path(cells, CombineMode::Replace);
        ws.selection.commit_continuation();
        app.drain_events();
        app.set_active_tool_from_type(ToolType::Move);
        let inside = |rects: &[Rectangle], (x, y): (i32, i32)| {
            rects
                .iter()
                .any(|r| r.contains(x as f64 + 0.5, y as f64 + 0.5))
        };
        // The pointer at (57, 32), on cell 1 (the view shows the document at 100 %).
        let o = app.active_ws().unwrap().view.view_transform().origin;
        let at = |x: f64, y: f64| (x + o[0], y + o[1]);
        let (x, y) = at(57.5, 32.5);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        for i in 1..5 {
            let (x, y) = at(57.5 + i as f64, 32.5 + (i / 2) as f64);
            let (changed, rects) = frame(&mut app, |app| {
                app.on_mouse_move(x, y, &input(x, y));
            });
            assert!(
                !changed.is_empty() && changed.iter().all(|&p| inside(&rects, p)),
                "frame {i}"
            );
            let area: f64 = rects.iter().map(|r| r.width * r.height).sum();
            assert!(
                rects.iter().all(|r| r.height == 1.0) && area < 3_000.0,
                "frame {i}: {area}"
            );
        }
        app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
        // Select All rotated: about every pixel changes, so the one render rect.
        app.on_key_press(&KeyPress {
            key: "Enter".into(),
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        });
        let ws = app.active_ws_mut().unwrap();
        ws.selection.set_continuation(
            &Rectangle::new(0.0, 0.0, 200.0, 150.0),
            CombineMode::Replace,
        );
        ws.selection.commit_continuation();
        app.update_tool_setting(ToolType::Move, "resampling", serde_json::json!("linear"));
        let (x, y) = at(150.0, 75.0);
        app.on_mouse_down(x, y, MouseButton::Right, &input(x, y));
        app.drain_events();
        let (x, y) = at(150.0, 95.0);
        let (changed, rects) = frame(&mut app, |app| {
            app.on_mouse_move(x, y, &input(x, y));
        });
        assert!(
            changed.len() > 20_000 && rects.len() == 1,
            "{} {}",
            changed.len(),
            rects.len()
        );
        assert!(changed.iter().all(|&p| inside(&rects, p)));
        app.on_mouse_up(x, y, MouseButton::Right, &input(x, y));
        assert!(!cache_empty(&mut app), "the rotate's source");
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());
        assert!(cache_empty(&mut app), "dropped at the commit");
        // Again, undone (no commit), then the tool deactivates for another document (it stays,
        // and comes back).
        let (x, y) = at(150.0, 75.0);
        app.on_mouse_down(x, y, MouseButton::Right, &input(x, y));
        let _ = frame(&mut app, |app| {
            app.on_mouse_move(x - 20.0, y + 10.0, &input(x - 20.0, y + 10.0));
        });
        app.on_mouse_up(
            x - 20.0,
            y + 10.0,
            MouseButton::Right,
            &input(x - 20.0, y + 10.0),
        );
        assert!(app.undo());
        assert!(!cache_empty(&mut app), "kept by the cancel");
        app.create_blank_document_in_new_workspace(50, 40, 96.0, true);
        assert!(cache_empty(&mut app), "dropped at the deactivation");
    }

    const RED: crate::pixel::Px = [255, 0, 0, 255];

    /// A 100×60 document with a selected 4×4 red square at (10, 10), `tool` active.
    fn red_square(tool: ToolType) -> AppWorkspace {
        let mut app = crate::testing::fresh_doc(100, 60, None);
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        layer.surface.fill_rect(IntRect::new(10, 10, 4, 4), RED);
        ws.selection
            .set_continuation(&Rectangle::new(10.0, 10.0, 4.0, 4.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        app.set_active_tool_from_type(tool);
        app
    }

    /// Document (x, y) in view coordinates.
    fn view(app: &AppWorkspace, (x, y): (f64, f64)) -> (f64, f64) {
        let o = app.active_ws().unwrap().view.view_transform().origin;
        (x + o[0], y + o[1])
    }

    /// Moves to document `p` and runs a frame (the drag applies it).
    fn move_to(app: &mut AppWorkspace, p: (f64, f64)) {
        let (x, y) = view(app, p);
        app.on_mouse_move(x, y, &input(x, y));
        app.now_ms += 16.0;
        let now = app.now_ms;
        app.on_frame(now);
    }

    /// Presses `button` at the first document point, visits the rest and releases at the last.
    fn drag(app: &mut AppWorkspace, button: MouseButton, path: &[(f64, f64)]) {
        let (x, y) = view(app, path[0]);
        app.on_mouse_down(x, y, button, &input(x, y));
        for &p in &path[1..] {
            move_to(app, p);
        }
        let (x, y) = view(app, *path.last().unwrap());
        app.on_mouse_up(x, y, button, &input(x, y));
    }

    /// Far from the square (a translate), then 1 px right per frame, `n` times.
    fn rightwards(n: usize) -> Vec<(f64, f64)> {
        (0..=n).map(|i| (60.5 + i as f64, 45.5)).collect()
    }

    fn row(app: &AppWorkspace, y: u32) -> Vec<crate::pixel::Px> {
        let ws = app.active_ws().unwrap();
        let s = &ws.document.layers.get_at(0).unwrap().surface;
        (0..s.width()).map(|x| s.pixel(x, y)).collect()
    }

    /// The open record's trail, as each stamp's translation.
    fn trail(app: &mut AppWorkspace) -> Vec<(f64, f64)> {
        app.with_tool(|t, _| {
            let a = t.as_any_mut();
            let r = if let Some(m) = a.downcast_mut::<MoveTool>() {
                m.context().cloned()
            } else {
                a.downcast_mut::<MoveSelectionTool>()
                    .and_then(|m| m.context().cloned())
            };
            r.map(|r| {
                r.trail
                    .iter()
                    .map(|(m, _)| (m.elements[0][2], m.elements[1][2]))
                    .collect()
            })
        })
        .flatten()
        .unwrap_or_default()
    }

    fn hint(app: &AppWorkspace) -> bool {
        app.tool.as_ref().unwrap().shift_trail_hint()
    }

    /// The columns of row `y` that are red.
    fn red_x(app: &AppWorkspace, y: u32) -> Vec<usize> {
        let r = row(app, y);
        (0..r.len()).filter(|&x| r[x] == RED).collect()
    }

    /// The snap lines the drag shows: (vertical, position, on the pixels before it).
    fn snap_lines(app: &AppWorkspace) -> Vec<(bool, f64, bool)> {
        let t = app.tool.as_ref().unwrap();
        t.snap_lines()
            .iter()
            .map(|l| (l.vertical, l.pos, l.before))
            .collect()
    }

    /// Snap (Extra): with Snap on, a translate's centre lands on the canvas centre within 8 screen
    /// px (its edges only ever go to edges) and the drag shows the lines; Ctrl held mid-drag moves
    /// freely until let go.
    #[test]
    fn snap_centres_the_square_on_the_canvas() {
        let mut app = red_square(ToolType::Move);
        app.update_tool_setting(ToolType::Move, "snap", serde_json::json!(true));
        // Moved by (35, 14) the square is (45, 24)–(49, 28): its centre (47, 26) 3 and 4 px from
        // the canvas centre (50, 30). Its right edge, 1 px from that centre line, stays an edge.
        let (x, y) = view(&app, (60.0, 45.0));
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        move_to(&mut app, (95.0, 59.0));
        assert_eq!(
            snap_lines(&app),
            [(true, 50.0, false), (false, 30.0, false)]
        );
        app.set_modifier_keys(true, false, false);
        assert!(snap_lines(&app).is_empty(), "Ctrl moves freely");
        app.set_modifier_keys(false, false, false);
        let (x, y) = view(&app, (95.0, 59.0));
        app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
        assert!(snap_lines(&app).is_empty(), "gone with the drag");
        assert_eq!(red_x(&app, 27), Vec::<usize>::new());
        assert_eq!(red_x(&app, 28), [48, 49, 50, 51]);
        assert_eq!(red_x(&app, 31), [48, 49, 50, 51]);
        // Snap off: a 2 px move Snap would pull back onto the centre lines.
        app.update_tool_setting(ToolType::Move, "snap", serde_json::json!(false));
        drag(&mut app, MouseButton::Left, &[(90.0, 55.0), (92.0, 57.0)]);
        assert_eq!(red_x(&app, 29), Vec::<usize>::new());
        assert_eq!(red_x(&app, 30), [50, 51, 52, 53]);
    }

    /// Snap (Extra): a small drag moves at once; dragged sideways the square stays level with
    /// where it started, the lines on its top, centre and bottom rows.
    #[test]
    fn a_sideways_snap_drag_stays_level_with_where_it_started() {
        let mut app = red_square(ToolType::Move);
        app.update_tool_setting(ToolType::Move, "snap", serde_json::json!(true));
        let (x, y) = view(&app, (60.0, 45.0));
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        move_to(&mut app, (63.0, 47.0));
        assert_eq!(red_x(&app, 12), [13, 14, 15, 16]);
        assert!(snap_lines(&app).is_empty());
        move_to(&mut app, (80.0, 46.0));
        assert_eq!(
            snap_lines(&app),
            [
                (false, 10.0, false),
                (false, 12.0, false),
                (false, 14.0, true)
            ]
        );
        let (x, y) = view(&app, (80.0, 46.0));
        app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
        assert_eq!(red_x(&app, 10), [30, 31, 32, 33]);
        assert_eq!(red_x(&app, 13), [30, 31, 32, 33]);
        assert_eq!(red_x(&app, 14), Vec::<usize>::new());
    }

    /// S§4.6, R253: Shift held from the press leaves a copy where the move started and at each
    /// new offset (0..=10), so the square's original place stays whole (no Ctrl needed). The
    /// status hint shows while the translate drags.
    #[test]
    fn shift_drag_leaves_a_trail() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        app.set_modifier_keys(false, true, false);
        let path = rightwards(10);
        let (x, y) = view(&app, path[0]);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        for &p in &path[1..] {
            move_to(&mut app, p);
            assert!(hint(&app), "a translate drag");
        }
        let (x, y) = view(&app, path[10]);
        app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
        assert!(!hint(&app), "released");
        let r = row(&app, 11);
        assert_eq!((r[9], r[24]), (white, white));
        assert!(r[10..=23].iter().all(|&p| p == RED), "{:?}", &r[8..26]);
        assert!(original_kept(&app), "the copy where the move started");
        assert_eq!(
            trail(&mut app),
            (0..=10).map(|k| (k as f64, 0.0)).collect::<Vec<_>>()
        );
    }

    /// R253: Shift pressed partway through a drag stamps where the pixels are at that moment,
    /// then each later offset, and nothing before it: the place the move started empties. Shift
    /// seen first by a step (no modifier event) stamps the place the step left the same way.
    #[test]
    fn shift_partway_through_a_drag_stamps_where_it_began() {
        for event in [true, false] {
            let mut app = red_square(ToolType::Move);
            let white = row(&app, 11)[0];
            let path = rightwards(5);
            let (x, y) = view(&app, path[0]);
            app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
            for &p in &path[1..=3] {
                move_to(&mut app, p);
            }
            if event {
                app.set_modifier_keys(false, true, false);
            } else {
                app.shift_key_down = true;
            }
            for &p in &path[4..] {
                move_to(&mut app, p);
            }
            let (x, y) = view(&app, path[5]);
            app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
            assert_eq!(
                trail(&mut app),
                [(3.0, 0.0), (4.0, 0.0), (5.0, 0.0)],
                "event {event}"
            );
            for (x, p) in row(&app, 11).into_iter().enumerate() {
                let want = match x {
                    10..13 => [0; 4],
                    13..19 => RED,
                    _ => white,
                };
                assert_eq!(p, want, "event {event}: {x}");
            }
        }
    }

    /// Ctrl latched at the press keeps the original; the trail follows as with Shift alone.
    #[test]
    fn ctrl_shift_keeps_the_original() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        app.set_modifier_keys(true, true, false);
        drag(&mut app, MouseButton::Left, &rightwards(10));
        let r = row(&app, 11);
        assert_eq!((r[9], r[24]), (white, white));
        assert!(r[10..=23].iter().all(|&p| p == RED), "{:?}", &r[8..26]);
        assert_eq!(trail(&mut app).len(), 11);
    }

    /// An offset equal to the last stamp (the start's, a repeated move, a modifier change) adds
    /// nothing; a revisited one stamps again.
    #[test]
    fn trail_deduplicates_repeated_offsets() {
        let mut app = red_square(ToolType::Move);
        app.set_modifier_keys(false, true, false);
        let path = [0.0, 0.0, 1.0, 1.0, 2.0, 2.0, 1.0].map(|d| (60.5 + d, 45.5));
        let (x, y) = view(&app, path[0]);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        for &p in &path[1..] {
            move_to(&mut app, p);
            app.set_modifier_keys(false, true, true);
            app.set_modifier_keys(false, true, false);
        }
        assert_eq!(
            trail(&mut app),
            [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (1.0, 0.0)]
        );
    }

    /// The whole Shift drag is its one Draw entry: undo restores the layer bit for bit, the copy
    /// where the move started included (R253); redo brings the trail back from the record.
    #[test]
    fn trail_drag_is_one_history_item() {
        let mut app = red_square(ToolType::Move);
        let surface = |app: &AppWorkspace| {
            let ws = app.active_ws().unwrap();
            ws.document
                .layers
                .get_at(0)
                .unwrap()
                .surface
                .data()
                .to_vec()
        };
        let before = surface(&app);
        let entries = app.active_ws().unwrap().history.undo.len();
        app.set_modifier_keys(false, true, false);
        drag(&mut app, MouseButton::Left, &rightwards(6));
        let after = surface(&app);
        assert!(after != before);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), entries + 1);
        assert!(original_kept(&app), "the copy where the move started");
        assert!(app.undo());
        assert!(surface(&app) == before, "undo restores exactly");
        assert!(trail(&mut app).is_empty());
        assert!(app.redo());
        assert!(surface(&app) == after, "redo renders the trail again");
        assert_eq!(trail(&mut app).len(), 7, "the start and 6 steps");
    }

    /// A Shift drag `n` px right, 1 px a frame, from `app`'s record: each frame's work.
    fn shift_drag_work(app: &mut AppWorkspace, n: usize) -> Vec<u64> {
        app.set_modifier_keys(false, true, false);
        let path = rightwards(n);
        let (x, y) = view(app, path[0]);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        let mut work = vec![];
        for &p in &path[1..] {
            move_to(app, p);
            crate::testing::with_tool(app, |m: &mut MoveTool, _| work.push(m.work));
        }
        work
    }

    /// The record rendered again from the untouched layer (no state kept from earlier frames),
    /// and that render's work.
    fn rebuilt(app: &mut AppWorkspace) -> (Vec<u8>, u64) {
        let mut work = 0;
        crate::testing::with_tool(app, |m: &mut MoveTool, app| {
            m.cache = MoveCache::default();
            m.trail_bake = None;
            m.show(app);
            work = m.work;
        });
        (crate::testing::layer_bytes(app, None), work)
    }

    /// S§4.6: a Shift drag's frame bakes only its new stamp and renders the live copy over its
    /// old ∪ new footprint, whatever the trail's length (the 4×4 stamp + the 5×4 footprints);
    /// the result is a full rebuild's, bit for bit. Off the NN path (the square rotated first)
    /// too.
    #[test]
    fn trail_frames_cost_the_same_at_any_length() {
        let mut app = red_square(ToolType::Move);
        let work = shift_drag_work(&mut app, 40);
        assert!(work[1..].iter().all(|&w| w == 16 + 20), "{work:?}");
        let frames = crate::testing::layer_bytes(&app, None);
        let (bytes, work) = rebuilt(&mut app);
        assert!(bytes == frames, "incremental = rebuilt");
        assert!(work >= 40 * 16, "the rebuild bakes every stamp: {work}");

        let mut app = red_square(ToolType::Move);
        let to = |deg: f64| {
            let a = deg.to_radians();
            (12.0 + 48.5 * a.cos(), 12.0 + 48.5 * a.sin())
        };
        drag(&mut app, MouseButton::Right, &[to(0.0), to(20.0)]);
        let work = shift_drag_work(&mut app, 40);
        assert!(work[2..].iter().all(|&w| w == work[1]), "{work:?}");
        assert!(work[1] < 4 * 7 * 7, "{work:?}");
        let frames = crate::testing::layer_bytes(&app, None);
        let (bytes, work) = rebuilt(&mut app);
        assert!(bytes == frames, "incremental = rebuilt, resampled");
        assert!(work >= 40 * 16, "the rebuild bakes every stamp: {work}");
    }

    /// Shift on the rotate gesture keeps Paint.NET's 15° snap and stamps nothing.
    #[test]
    fn shift_on_rotate_still_snaps_15_degrees() {
        let mut app = red_square(ToolType::Move);
        app.set_modifier_keys(false, true, false);
        // About the centre (12, 12): from 0° to 20°, which snaps to 15°.
        let to = |deg: f64| {
            let a = deg.to_radians();
            (12.0 + 48.5 * a.cos(), 12.0 + 48.5 * a.sin())
        };
        drag(&mut app, MouseButton::Right, &[to(0.0), to(10.0), to(20.0)]);
        let angle = app.with_tool(|t, _| t.status_angle()).flatten().unwrap();
        assert!((angle + 15.0).abs() < 1e-9, "{angle}");
        assert!(trail(&mut app).is_empty());
    }

    /// A stamp off the NN path (the square rotated first) resamples like the live copy: where the
    /// live copy at that offset is solid red, so is the stamp left there.
    #[test]
    fn rotated_trail_stamps_resample_like_the_live_copy() {
        let rotated = || {
            let mut app = red_square(ToolType::Move);
            app.set_modifier_keys(false, true, false);
            let to = |deg: f64| {
                let a = deg.to_radians();
                (12.0 + 48.5 * a.cos(), 12.0 + 48.5 * a.sin())
            };
            drag(&mut app, MouseButton::Right, &[to(0.0), to(20.0)]);
            app
        };
        let mut live = rotated();
        live.set_modifier_keys(false, false, false);
        drag(&mut live, MouseButton::Left, &[(60.5, 45.5), (80.5, 45.5)]);
        let mut stamped = rotated();
        let (x, y) = view(&stamped, (60.5, 45.5));
        stamped.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        move_to(&mut stamped, (80.5, 45.5));
        stamped.set_modifier_keys(false, false, false);
        move_to(&mut stamped, (90.5, 45.5));
        assert_eq!(trail(&mut stamped).len(), 2, "the start and +20");
        let mut solid = 0;
        for y in 6..18 {
            let (a, b) = (row(&live, y), row(&stamped, y));
            for x in 26..38 {
                if a[x] == RED {
                    assert_eq!(b[x], RED, "({x}, {y})");
                    solid += 1;
                }
            }
        }
        assert!(solid >= 4, "{solid}");
    }

    /// Move Selection moves the outline only: Shift leaves no trail and no hint.
    #[test]
    fn move_selection_ignores_shift_trail() {
        let mut app = red_square(ToolType::MoveSelection);
        let before = row(&app, 11);
        app.set_modifier_keys(false, true, false);
        let path = rightwards(5);
        let (x, y) = view(&app, path[0]);
        app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
        for &p in &path[1..] {
            move_to(&mut app, p);
            assert!(!hint(&app));
        }
        assert!(trail(&mut app).is_empty());
        assert_eq!(row(&app, 11), before);
    }

    fn press(app: &mut AppWorkspace, key: &str, shift: bool, ctrl: bool) {
        app.on_key_press(&KeyPress {
            key: key.into(),
            shift,
            ctrl,
            alt: false,
            meta: false,
        });
    }

    /// A frame past the nudge burst's idle commit.
    fn idle(app: &mut AppWorkspace) {
        let now = app.now_ms + NUDGE_COMMIT_MS + 1.0;
        app.on_frame(now);
    }

    /// R227, R253: Shift+arrows stamp where the burst started and where each step lands, as the
    /// drag's stamps (0..=3), so the original place stays whole; the status hint shows through
    /// the burst.
    #[test]
    fn shift_arrows_leave_a_trail() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        for _ in 0..3 {
            press(&mut app, "ArrowRight", true, false);
            assert!(hint(&app), "a nudge burst");
        }
        assert_eq!(
            trail(&mut app),
            [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0), (3.0, 0.0)]
        );
        let r = row(&app, 11);
        assert_eq!((r[9], r[17]), (white, white));
        assert!(r[10..=16].iter().all(|&p| p == RED), "{:?}", &r[8..19]);
        idle(&mut app);
        assert!(original_kept(&app), "the copy where the burst started");
    }

    /// R253: Shift pressed partway through a burst stamps where the pixels are at that moment,
    /// then each later step; the places before it empty as in a plain nudge.
    #[test]
    fn shift_partway_through_a_burst_stamps_where_it_began() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        press(&mut app, "ArrowRight", false, false);
        press(&mut app, "ArrowRight", false, false);
        press(&mut app, "ArrowRight", true, false);
        press(&mut app, "ArrowRight", true, false);
        assert_eq!(trail(&mut app), [(2.0, 0.0), (3.0, 0.0), (4.0, 0.0)]);
        for (x, p) in row(&app, 11).into_iter().enumerate() {
            let want = match x {
                10..12 => [0; 4],
                12..18 => RED,
                _ => white,
            };
            assert_eq!(p, want, "{x}");
        }
    }

    /// R227, R253: Ctrl+Shift+arrow steps 10 px, a stamp where the burst started and where each
    /// step lands. Ctrl on a key is the step, never the copy-behind (R69): the original place
    /// keeps the start's stamp.
    #[test]
    fn ctrl_shift_arrows_step_10_px() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        press(&mut app, "ArrowRight", true, true);
        press(&mut app, "ArrowRight", true, true);
        assert_eq!(trail(&mut app), [(0.0, 0.0), (10.0, 0.0), (20.0, 0.0)]);
        for (x, p) in row(&app, 11).into_iter().enumerate() {
            let want = match x {
                10..14 | 20..24 | 30..34 => RED,
                _ => white,
            };
            assert_eq!(p, want, "{x}");
        }
        idle(&mut app);
        assert!(original_kept(&app), "the start's stamp");
    }

    /// The copy-behind a Ctrl drag latched stays for the Shift+arrow burst that edits it: the
    /// original stays, as at the origin of a Ctrl+Shift drag; the drag's end, where the burst
    /// starts trailing, keeps a stamp (R253).
    #[test]
    fn shift_arrows_after_a_ctrl_drag_keep_the_original() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        app.set_modifier_keys(true, false, false);
        drag(&mut app, MouseButton::Left, &[(60.5, 45.5), (70.5, 45.5)]);
        app.set_modifier_keys(false, false, false);
        press(&mut app, "ArrowRight", true, true);
        press(&mut app, "ArrowRight", true, true);
        assert_eq!(trail(&mut app), [(10.0, 0.0), (20.0, 0.0), (30.0, 0.0)]);
        for (x, p) in row(&app, 11).into_iter().enumerate() {
            let red = matches!(x, 10..14 | 20..24 | 30..34 | 40..44);
            assert_eq!(p, if red { RED } else { white }, "{x}");
        }
    }

    /// The square's original 4×4 is all red: the copy a Ctrl edit left behind.
    fn original_kept(app: &AppWorkspace) -> bool {
        (10..14).all(|y| row(app, y)[10..14].iter().all(|&p| p == RED))
    }

    /// Drags the bottom-right corner of the selection's bounds by (`dx`, `dy`).
    fn drag_br(app: &mut AppWorkspace, dx: f64, dy: f64) {
        let b = selection_bounds(app);
        let p = (b.right(), b.bottom());
        drag(app, MouseButton::Left, &[p, (p.0 + dx, p.1 + dy)]);
    }

    fn names(app: &AppWorkspace) -> Vec<String> {
        let undo = &app.active_ws().unwrap().history.undo;
        undo.iter().map(|m| m.name.clone()).collect()
    }

    /// The user's report: Ctrl+Shift on a corner (Shift = aspect lock, no trail) scales the square
    /// down and leaves the original; the plain drag after it keeps the original.
    #[test]
    fn ctrl_scale_copy_survives_a_plain_drag() {
        let mut app = red_square(ToolType::Move);
        app.set_modifier_keys(true, true, false);
        drag_br(&mut app, -2.0, -2.0);
        app.set_modifier_keys(false, false, false);
        assert_eq!(selection_bounds(&app), Rectangle::new(10.0, 10.0, 2.0, 2.0));
        assert!(
            trail(&mut app).is_empty(),
            "Shift on a handle locks the aspect"
        );
        drag(&mut app, MouseButton::Left, &[(60.5, 45.5), (90.5, 45.5)]);
        assert!(original_kept(&app));
    }

    /// After a Ctrl drag no later edit of the same transaction (translate, Shift trail, Alt scale,
    /// a Distort corner, a rotate, arrows) takes the copy away, nor the Finish; the trail's stamps
    /// stay in the record the same way.
    #[test]
    fn later_edits_keep_the_ctrl_copy() {
        let mut app = red_square(ToolType::Move);
        app.set_modifier_keys(true, false, false);
        drag(&mut app, MouseButton::Left, &[(60.5, 45.5), (100.5, 45.5)]);
        app.set_modifier_keys(false, false, false);
        assert!(original_kept(&app), "the Ctrl drag");
        let far = [(90.5, 50.5), (91.5, 50.5)];
        drag(&mut app, MouseButton::Left, &far);
        assert!(original_kept(&app), "a plain drag");
        app.set_modifier_keys(false, true, false);
        drag(&mut app, MouseButton::Left, &[far[0], far[1], (92.5, 50.5)]);
        assert_eq!(trail(&mut app).len(), 3, "the start and 2 steps");
        assert!(original_kept(&app), "a Shift drag");
        app.set_modifier_keys(false, false, true);
        drag_br(&mut app, 1.0, 1.0);
        app.set_modifier_keys(false, false, false);
        assert!(original_kept(&app), "an Alt scale");
        distort(&mut app, true);
        drag_br(&mut app, 3.0, 1.0);
        distort(&mut app, false);
        assert!(quad(&mut app).is_some());
        assert!(original_kept(&app), "a Distort corner");
        drag(&mut app, MouseButton::Right, &[far[0], (90.5, 30.5)]);
        assert!(original_kept(&app), "a rotate");
        press(&mut app, "ArrowDown", false, false);
        press(&mut app, "ArrowDown", false, true);
        idle(&mut app);
        assert!(original_kept(&app), "arrows");
        assert_eq!(trail(&mut app).len(), 3, "the stamps stay");
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());
        assert!(original_kept(&app), "the Finish");
    }

    /// Undo steps back in order: a plain drag vacates, the Ctrl scale after it leaves the copy at
    /// the original place (R69), the drag and arrows after that keep it until the Ctrl edit's own
    /// undo vacates it again.
    #[test]
    fn undo_takes_the_ctrl_copy_back_in_order() {
        let mut app = red_square(ToolType::Move);
        let bytes = |app: &AppWorkspace| crate::testing::layer_bytes(app, None);
        drag(&mut app, MouseButton::Left, &[(60.5, 45.5), (70.5, 45.5)]);
        assert!(!original_kept(&app));
        let mut steps = vec![bytes(&app)];
        app.set_modifier_keys(true, true, false);
        drag_br(&mut app, -2.0, -2.0);
        app.set_modifier_keys(false, false, false);
        steps.push(bytes(&app));
        drag(&mut app, MouseButton::Left, &[(60.5, 45.5), (60.5, 55.5)]);
        steps.push(bytes(&app));
        press(&mut app, "ArrowRight", false, false);
        press(&mut app, "ArrowRight", false, false);
        idle(&mut app);
        steps.push(bytes(&app));
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());
        assert!(original_kept(&app));
        let n = names(&app);
        assert_eq!(
            n[n.len() - 5..],
            [
                "Move Pixels",
                "Resize Pixels",
                "Move Pixels",
                "Move Pixels",
                "Finish Selection"
            ]
        );
        for (i, want) in steps.iter().enumerate().rev() {
            assert!(app.undo());
            assert!(bytes(&app) == *want, "undo to step {i}");
            assert_eq!(original_kept(&app), i > 0, "step {i}");
        }
    }

    /// R227: a Shift+arrow burst is one entry with its stamps; undo takes the burst's moves and
    /// stamps back together, the earlier burst's staying, and the first burst's undo restores the
    /// original exactly. The second burst starts where the first one's last stamp is, so it adds
    /// no stamp there (R253).
    #[test]
    fn shift_arrow_burst_is_one_history_item() {
        let mut app = red_square(ToolType::Move);
        let before = crate::testing::layer_bytes(&app, None);
        let entries = app.active_ws().unwrap().history.undo.len();
        press(&mut app, "ArrowRight", true, false);
        press(&mut app, "ArrowRight", true, false);
        idle(&mut app);
        let first = crate::testing::layer_bytes(&app, None);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), entries + 1);
        press(&mut app, "ArrowDown", true, false);
        press(&mut app, "ArrowDown", true, false);
        idle(&mut app);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), entries + 2);
        assert_eq!(trail(&mut app).len(), 5);
        let second = crate::testing::layer_bytes(&app, None);
        assert!(app.undo());
        assert_eq!(trail(&mut app), [(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]);
        assert!(
            crate::testing::layer_bytes(&app, None) == first,
            "the first burst"
        );
        assert!(app.undo());
        assert!(trail(&mut app).is_empty());
        assert!(
            crate::testing::layer_bytes(&app, None) == before,
            "undo restores exactly"
        );
        assert!(app.redo() && app.redo());
        assert!(
            crate::testing::layer_bytes(&app, None) == second,
            "redo bakes the trail"
        );
        assert_eq!(trail(&mut app).len(), 5);
        assert!(
            original_kept(&app),
            "the first burst's start keeps its copy"
        );
    }

    /// Plain and Ctrl arrows nudge without stamps, as before.
    #[test]
    fn plain_arrows_leave_no_trail() {
        let mut app = red_square(ToolType::Move);
        let white = row(&app, 11)[0];
        press(&mut app, "ArrowRight", false, false);
        press(&mut app, "ArrowRight", false, true);
        assert!(trail(&mut app).is_empty());
        let r = row(&app, 11);
        assert_eq!((r[9], r[10], r[20], r[25]), (white, [0; 4], white, white));
        assert!(r[21..=24].iter().all(|&p| p == RED), "{:?}", &r[8..27]);
    }

    /// Move Selection moves the outline only: Shift+arrow leaves no trail and no hint.
    #[test]
    fn move_selection_shift_arrows_leave_no_trail() {
        let mut app = red_square(ToolType::MoveSelection);
        let before = row(&app, 11);
        press(&mut app, "ArrowRight", true, false);
        press(&mut app, "ArrowRight", true, false);
        assert!(!hint(&app));
        assert!(trail(&mut app).is_empty());
        assert_eq!(row(&app, 11), before);
    }

    fn distort(app: &mut AppWorkspace, on: bool) {
        app.update_tool_setting(ToolType::Move, "distort", serde_json::json!(on));
    }

    /// The open record's quad.
    fn quad(app: &mut AppWorkspace) -> Option<Quad> {
        let mut q = None;
        crate::testing::with_tool(app, |m: &mut MoveTool, _| {
            q = m.context().and_then(|r| r.quad)
        });
        q
    }

    fn selection_bounds(app: &AppWorkspace) -> Rectangle {
        app.active_ws().unwrap().selection.get_bounds()
    }

    /// `a` and `b` within 1e-9 px (a homography's rounding).
    fn same_rect(a: Rectangle, b: Rectangle) -> bool {
        [a.x - b.x, a.y - b.y, a.width - b.width, a.height - b.height]
            .iter()
            .all(|d| d.abs() < 1e-9)
    }

    /// Distort off: a corner handle scales about the opposite corner, as Paint.NET's (TR dragged
    /// (+2, −2) on the 4×4 square: 1.5× about (10, 14)).
    #[test]
    fn corner_drag_scales_with_distort_off() {
        let mut app = red_square(ToolType::Move);
        drag(&mut app, MouseButton::Left, &[(14.0, 10.0), (16.0, 8.0)]);
        assert_eq!(quad(&mut app), None);
        assert_eq!(selection_bounds(&app), Rectangle::new(10.0, 8.0, 6.0, 6.0));
    }

    /// A 100×60 document with a selected 24×24 gradient square at (20, 16), Move Selected Pixels
    /// active with Distort on, Bilinear and Companded (as the warp it is compared with).
    fn gradient_square() -> AppWorkspace {
        let mut app = crate::testing::fresh_doc(100, 60, None);
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        for y in 16..40u32 {
            for x in 20..44u32 {
                let px = [((x - 20) * 10) as u8, ((y - 16) * 10) as u8, 128, 255];
                layer.surface.set_pixel(x, y, px);
            }
        }
        ws.selection.set_continuation(
            &Rectangle::new(20.0, 16.0, 24.0, 24.0),
            CombineMode::Replace,
        );
        ws.selection.commit_continuation();
        app.set_active_tool_from_type(ToolType::Move);
        app.update_tool_setting(ToolType::Move, "resampling", serde_json::json!("linear"));
        app.update_tool_setting(ToolType::Move, "gammaCorrected", serde_json::json!(false));
        distort(&mut app, true);
        app
    }

    /// The layer's bytes.
    fn layer_bytes(app: &AppWorkspace) -> Vec<u8> {
        let ws = app.active_ws().unwrap();
        ws.document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .to_vec()
    }

    /// Pointer drags with the canvas float (`AppWorkspace::float_moves`): while one drags, the
    /// layer holds the pixels lifted out (the hole) and `DocumentWorkspace::float` places them;
    /// the release renders them in byte for byte as without the float, and so does a commit in
    /// the middle of a drag (a tool switch). Translating by fractions of a pixel, rotating (the right button),
    /// Ctrl's copy, a distort corner.
    #[test]
    fn float_drags_end_as_without_the_float() {
        type Setup = fn() -> AppWorkspace;
        type Case = (Setup, MouseButton, bool, Vec<(f64, f64)>);
        let square: Setup = || red_square(ToolType::Move);
        let cases: [Case; 4] = [
            (
                square,
                MouseButton::Left,
                false,
                vec![(11.5, 11.5), (12.2, 11.8), (13.9, 12.4)],
            ),
            (
                square,
                MouseButton::Right,
                false,
                vec![(11.5, 11.5), (14.0, 17.0), (20.0, 25.0)],
            ),
            (
                square,
                MouseButton::Left,
                true,
                vec![(11.5, 11.5), (15.5, 13.0), (19.25, 14.5)],
            ),
            (
                gradient_square,
                MouseButton::Left,
                false,
                vec![(44.0, 16.0), (41.0, 19.0), (38.0, 22.0)],
            ),
        ];
        for (i, (setup, button, ctrl, path)) in cases.into_iter().enumerate() {
            let run = |float: bool, commit_mid_drag: bool| {
                let mut app = setup();
                app.float_moves = float;
                app.control_key_down = ctrl;
                let before = layer_bytes(&app);
                let (x, y) = view(&app, path[0]);
                app.on_mouse_down(x, y, button, &input(x, y));
                app.control_key_down = false;
                for &p in &path[1..] {
                    move_to(&mut app, p);
                }
                let floating = app.active_ws().unwrap().float.is_some();
                assert_eq!(floating, float, "case {i}: a float while dragging");
                if float {
                    // Lifted: the pixels are out of the layer (Ctrl leaves them behind).
                    assert_eq!(layer_bytes(&app) != before, !ctrl, "case {i}");
                }
                if commit_mid_drag {
                    // A tool switch commits the drag as it stands.
                    app.set_active_tool_from_type(ToolType::RectangleSelect);
                } else {
                    let (x, y) = view(&app, *path.last().unwrap());
                    app.on_mouse_up(x, y, button, &input(x, y));
                }
                assert!(app.active_ws().unwrap().float.is_none(), "case {i}: landed");
                layer_bytes(&app)
            };
            assert!(run(false, false) == run(true, false), "case {i}: release");
            assert!(
                run(false, true) == run(true, true),
                "case {i}: commit mid-drag"
            );
        }
    }

    /// Every layer's bytes.
    fn all_bytes(app: &AppWorkspace) -> Vec<Vec<u8>> {
        let ws = app.active_ws().unwrap();
        let layers = ws.document.layers.iter();
        layers.map(|l| l.surface.data().to_vec()).collect()
    }

    fn key(key: &str) -> KeyPress {
        KeyPress {
            key: key.into(),
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    /// I1: Copy, Copy Merged and Cut in the middle of a float drag copy the moved pixels, not the
    /// hole the layer holds meanwhile, and Cut erases what it copied: as without the float.
    #[test]
    fn float_copy_and_cut_mid_drag_see_the_pixels() {
        for id in ["menu.edit.copy", "menu.edit.copyMerged", "menu.edit.cut"] {
            let run = |float: bool| {
                let mut app = red_square(ToolType::Move);
                app.float_moves = float;
                let (x, y) = view(&app, (11.5, 11.5));
                app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
                for p in [(15.5, 13.0), (30.5, 20.5)] {
                    move_to(&mut app, p);
                }
                crate::action::perform(&mut app, id);
                let clip = app
                    .clipboard
                    .item
                    .as_ref()
                    .map(|i| i.bitmap.data().to_vec());
                // The drag goes on.
                move_to(&mut app, (32.5, 21.5));
                let (x, y) = view(&app, (32.5, 21.5));
                app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
                (clip, layer_bytes(&app))
            };
            let (a, b) = (run(false), run(true));
            assert!(
                a.0.as_ref().is_some_and(|c| c.chunks(4).any(|p| p == RED)),
                "{id}"
            );
            assert!(a == b, "{id}");
        }
    }

    /// A second drag in the same transaction (a pointer edit, which `end` does not show), ended
    /// by the release, Enter or Escape mid-drag, gives the bytes it gives without the float.
    #[test]
    fn float_second_drags_end_as_without_the_float() {
        for end in ["release", "Enter", "Escape"] {
            let run = |float: bool| {
                let mut app = red_square(ToolType::Move);
                app.float_moves = float;
                let first = [(11.5, 11.5), (14.2, 12.7), (16.5, 13.5)];
                drag(&mut app, MouseButton::Left, &first);
                let (x, y) = view(&app, (17.5, 14.5));
                app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
                for p in [(30.3, 20.1), (45.7, 33.2)] {
                    move_to(&mut app, p);
                }
                assert_eq!(app.active_ws().unwrap().float.is_some(), float);
                match end {
                    "release" => {
                        let (x, y) = view(&app, (45.7, 33.2));
                        app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
                    }
                    k => {
                        app.on_key_press(&key(k));
                    }
                }
                assert!(app.active_ws().unwrap().float.is_none(), "{end}");
                layer_bytes(&app)
            };
            assert!(run(false) == run(true), "{end}");
        }
    }

    /// The float is used only where the canvas can draw it (the active layer shown, Normal, with
    /// no shown layer above); elsewhere the drag renders the layer as before. Either way every
    /// layer ends with the bytes it gets without the float.
    #[test]
    fn float_only_where_the_canvas_can_draw_it() {
        use crate::document::layer::{BlendMode, Layer};
        type Setup = fn(&mut DocumentWorkspace);
        let cases: [(&str, Setup, bool); 4] = [
            (
                "translucent",
                |ws| ws.document.layers.get_at_mut(0).unwrap().properties.opacity = 128,
                true,
            ),
            (
                "multiply",
                |ws| {
                    ws.document
                        .layers
                        .get_at_mut(0)
                        .unwrap()
                        .properties
                        .blend_mode = BlendMode::Multiply
                },
                false,
            ),
            (
                "hidden",
                |ws| ws.document.layers.get_at_mut(0).unwrap().properties.visible = false,
                false,
            ),
            (
                "layer above",
                |ws| {
                    ws.add_layer(Layer::create_layer(100, 60, "Top"));
                    ws.active_layer = 0;
                },
                false,
            ),
        ];
        for (name, setup, floats) in cases {
            let run = |float: bool| {
                let mut app = red_square(ToolType::Move);
                setup(app.active_ws_mut().unwrap());
                app.float_moves = float;
                let (x, y) = view(&app, (11.5, 11.5));
                app.on_mouse_down(x, y, MouseButton::Left, &input(x, y));
                for p in [(15.2, 13.6), (30.5, 20.5)] {
                    move_to(&mut app, p);
                }
                let floating = app.active_ws().unwrap().float.is_some();
                assert_eq!(floating, float && floats, "{name}");
                let (x, y) = view(&app, (30.5, 20.5));
                app.on_mouse_up(x, y, MouseButton::Left, &input(x, y));
                all_bytes(&app)
            };
            assert!(run(false) == run(true), "{name}");
        }
    }

    const SQUARE: Quad = [[20.0, 16.0], [44.0, 16.0], [44.0, 40.0], [20.0, 40.0]];
    const PULLED: Quad = [[20.0, 16.0], [38.0, 22.0], [44.0, 40.0], [20.0, 40.0]];

    /// R230: with Distort on, the top-right handle moves its corner alone; the selection is the
    /// quad, and the committed pixels are the original through the rectangle → quad homography:
    /// `perspective_warp`'s within 4 levels inside it, the corner it left vacated.
    #[test]
    fn distort_moves_only_the_dragged_corner() {
        use crate::extra::{geom::Rect, interrupt::Interrupt, perspective, warp::Interp};
        let mut app = gradient_square();
        let original = app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .clone();
        drag(
            &mut app,
            MouseButton::Left,
            &[(44.0, 16.0), (41.0, 19.0), (38.0, 22.0)],
        );
        assert_eq!(quad(&mut app), Some(PULLED));
        let path = app.active_ws().unwrap().selection.create_path(true);
        assert!(same_rect(
            path.get_bounds(),
            Rectangle::new(20.0, 16.0, 24.0, 24.0)
        ));
        let near = |p: &Point, q: [f64; 2]| (p.x - q[0]).abs() < 1e-9 && (p.y - q[1]).abs() < 1e-9;
        assert!(
            PULLED
                .iter()
                .all(|c| path.polygons[0].iter().any(|p| near(p, *c)))
        );
        assert!(!path.polygons[0].iter().any(|p| near(p, SQUARE[1])));
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());

        let mut src = original.get_image_data(0, 0, 100, 60);
        for (i, px) in src.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let (x, y) = ((i % 100) as i32, (i / 100) as i32);
            if !((20..44).contains(&x) && (16..40).contains(&y)) {
                px.fill(0);
            }
        }
        let planes = [perspective::Plane {
            src: SQUARE,
            dst: PULLED,
        }];
        let whole = Rect {
            x0: 0,
            y0: 0,
            x1: 100,
            y1: 60,
        };
        let want = perspective::perspective_warp(
            &src,
            &planes,
            1.0,
            whole,
            Interp::Bilinear,
            Interrupt::NONE,
        )
        .unwrap();
        let h = perspective::Homography::rect_to_quad([20.0, 16.0, 44.0, 40.0], PULLED)
            .and_then(|h| h.inverse())
            .unwrap();
        let ws = app.active_ws().unwrap();
        let got = &ws.document.layers.get_at(0).unwrap().surface;
        let mut compared = 0;
        for y in 0..60u32 {
            for x in 0..100u32 {
                let (u, v) = h.apply(x as f64 + 0.5, y as f64 + 0.5);
                if !((21.5..=42.5).contains(&u) && (17.5..=38.5).contains(&v)) {
                    continue;
                }
                let i = (y * 100 + x) as usize * 4;
                let (a, b) = (got.pixel(x, y), &want.data[i..i + 4]);
                assert!(
                    (0..4).all(|k| (a[k] as i32 - b[k] as i32).abs() <= 4),
                    "({x}, {y}): {a:?} vs {b:?}"
                );
                compared += 1;
            }
        }
        assert!(compared > 300, "{compared}");
        assert_eq!(got.pixel(43, 17), [0; 4], "vacated");
        assert_eq!(got.pixel(60, 30), [255; 4], "untouched");
    }

    /// The distort drag is one entry and the commit another; undo restores the pixels and the
    /// selection exactly, redo brings the distort back.
    #[test]
    fn distort_undo_restores_everything() {
        let mut app = gradient_square();
        let entries = app.active_ws().unwrap().history.undo.len();
        let bytes = crate::testing::layer_bytes(&app, None);
        let selection = app.active_ws().unwrap().selection.save();
        drag(&mut app, MouseButton::Left, &[(44.0, 16.0), (38.0, 22.0)]);
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());
        assert_eq!(app.active_ws().unwrap().history.undo.len(), entries + 2);
        let after = (
            crate::testing::layer_bytes(&app, None),
            app.active_ws().unwrap().selection.save(),
        );
        assert!(app.undo());
        assert_eq!(
            quad(&mut app),
            Some(PULLED),
            "the commit undone re-opens it"
        );
        assert!(app.undo());
        assert!(crate::testing::layer_bytes(&app, None) == bytes);
        assert_eq!(app.active_ws().unwrap().selection.save(), selection);
        assert!(app.redo() && app.redo());
        assert!(crate::testing::layer_bytes(&app, None) == after.0);
        assert_eq!(app.active_ws().unwrap().selection.save(), after.1);
    }

    /// An edit back from a distorted record to a plain one gives the selection its paths back.
    #[test]
    fn undoing_a_distort_edit_restores_the_plain_selection() {
        let mut app = gradient_square();
        // Far outside the square (a translate).
        drag(&mut app, MouseButton::Left, &[(80.0, 52.0), (83.0, 52.0)]);
        let moved = app.active_ws().unwrap().selection.create_path(true);
        drag(&mut app, MouseButton::Left, &[(47.0, 16.0), (41.0, 22.0)]);
        assert!(quad(&mut app).is_some());
        assert!(app.undo());
        assert_eq!(quad(&mut app), None);
        assert_eq!(app.active_ws().unwrap().selection.create_path(true), moved);
    }

    /// A corner dragged past the opposite one stops while the quad is still convex (no fold, no
    /// garbage); a mirrored selection (counter-clockwise quad) distorts too.
    #[test]
    fn non_convex_distort_stops_short() {
        let mut app = gradient_square();
        drag(&mut app, MouseButton::Left, &[(20.0, 16.0), (70.0, 58.0)]);
        let q = quad(&mut app).expect("a quad");
        assert!(perspective_ok(&q), "{q:?}");
        assert!(q[0][0] > 20.0 && q[0][0] < 44.0, "{q:?}");
        assert_eq!(&q[1..], &SQUARE[1..]);
        assert!(app.with_tool(|t, app| t.commit_pending(app)).unwrap());
        // Flipped: the left edge dragged past the right one, then a corner pulled in.
        let mut app = gradient_square();
        drag(&mut app, MouseButton::Left, &[(20.0, 28.0), (68.0, 28.0)]);
        drag(&mut app, MouseButton::Left, &[(68.0, 16.0), (64.0, 20.0)]);
        let q = quad(&mut app).expect("a quad");
        assert_eq!(q[0], [64.0, 20.0], "{q:?}");
    }

    /// An edge handle after a distort scales the quad as it scales F: the right handle (at the
    /// distorted edge's projective midpoint) dragged 12 px right is 1.5× about the left edge.
    #[test]
    fn edge_scale_after_a_distort_scales_the_quad() {
        let mut app = gradient_square();
        drag(&mut app, MouseButton::Left, &[(44.0, 16.0), (38.0, 22.0)]);
        let h = Homography::rect_to_quad([20.0, 16.0, 44.0, 40.0], PULLED).unwrap();
        let (x, y) = h.apply(44.0, 28.0);
        drag(&mut app, MouseButton::Left, &[(x, y), (x + 12.0, y)]);
        let q = quad(&mut app).unwrap();
        let want = [[20.0, 16.0], [47.0, 22.0], [56.0, 40.0], [20.0, 40.0]];
        assert!(
            q.iter()
                .flatten()
                .zip(want.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-9),
            "{q:?}"
        );
    }

    fn perspective_ok(q: &Quad) -> bool {
        use crate::extra::perspective::convex_quad;
        convex_quad(q) || convex_quad(&[q[0], q[3], q[2], q[1]])
    }

    /// The quad goes along with later gestures: a translate drag and a nudge move it by their
    /// offsets, and the selection with it; a Shift translate stamps the quad where it starts and
    /// where it lands (R253).
    #[test]
    fn later_moves_carry_the_quad() {
        let mut app = gradient_square();
        drag(&mut app, MouseButton::Left, &[(44.0, 16.0), (38.0, 22.0)]);
        drag(&mut app, MouseButton::Left, &[(80.0, 52.0), (85.0, 55.0)]);
        let shifted = |dx: f64, dy: f64| PULLED.map(|c| [c[0] + dx, c[1] + dy]);
        let close = |a: Quad, b: Quad| {
            a.iter()
                .flatten()
                .zip(b.iter().flatten())
                .all(|(a, b)| (a - b).abs() < 1e-9)
        };
        assert!(close(quad(&mut app).unwrap(), shifted(5.0, 3.0)));
        press(&mut app, "ArrowRight", false, false);
        assert!(close(quad(&mut app).unwrap(), shifted(6.0, 3.0)));
        assert!(same_rect(
            selection_bounds(&app),
            Rectangle::new(26.0, 19.0, 24.0, 24.0)
        ));
        idle(&mut app);
        press(&mut app, "ArrowDown", true, false);
        let mut stamps = vec![];
        crate::testing::with_tool(&mut app, |m: &mut MoveTool, _| {
            stamps = m.context().unwrap().trail.clone()
        });
        assert_eq!(stamps.len(), 2);
        assert!(close(stamps[0].1.unwrap(), shifted(6.0, 3.0)));
        assert!(close(stamps[1].1.unwrap(), shifted(6.0, 4.0)));
    }
}
