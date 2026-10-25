//! The others in a live session, drawn over the canvas of the image they are on in their colour after the app's own
//! frame (`PaintApp::ui`), so the windows, menus and dialogs stay above them, and as dots on that image's tab. First
//! the gestures they have under way (a stroke at its brush width, a marquee's or lasso's rubber band, a ghost of where
//! a Move drag has the selection so far), then their selection bounds, then their pointers with a name tag. Presence
//! arrives about twenty times a second: each mark eases toward its person's latest state in image pixels, so this
//! person's own scrolling and zooming never lags them. Their states come from other people: what doesn't read as
//! numbers in range is skipped. After PhotoCraft's (MIT).

use super::LIVE;
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Shape, Stroke, Vec2, pos2, vec2};
use pdn_core::document::view::DocumentView;
use pdn_core::util::geom::Point;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::HashMap;

/// A point in image pixels.
type P = [f32; 2];

/// How long the others' marks take to go most (1 - 1/e) of the way to where they last were, in seconds.
const EASE: f32 = 0.08;
/// The most points of a remote path (twice what a presence carries).
const MAX_POINTS: usize = 400;

/// The canvas of the image shown: image pixels to screen points.
struct Xform<'a> {
    view: &'a DocumentView,
    /// The canvas texture's top-left, and the part of the canvas not under the rulers.
    origin: Pos2,
    rect: Rect,
    /// Screen points per image pixel.
    zoom: f32,
}

impl Xform<'_> {
    fn to_screen(&self, x: f32, y: f32) -> Pos2 {
        let s = self
            .view
            .to_screen_position(Point::new(f64::from(x), f64::from(y)));
        self.origin + vec2(s.x as f32, s.y as f32)
    }
}

/// A person's moving marks as last drawn, in image pixels: their pointer, a ghost's or rubber band's corners, and how
/// far (along the path) the drawn end of a stroke or lasso trails its newest point.
#[derive(Default)]
struct Shown {
    pointer: Vec<P>,
    ghost: Vec<P>,
    band: Vec<P>,
    tip: Option<P>,
    behind: f32,
}

thread_local! {
    static SHOWN: RefCell<HashMap<String, Shown>> = RefCell::default();
}

/// Where the others' marks may be, in image pixels: the image and as much again around it. Their states come from
/// other people, and a mark far out (or one a whole screen wide at a high zoom) would only cost drawing time.
#[derive(Clone, Copy)]
struct Bounds([f32; 4]);

impl Bounds {
    fn of(w: u32, h: u32) -> Self {
        let (w, h) = (w as f32, h as f32);
        let m = w.max(h);
        Self([-m, -m, w + m, h + m])
    }

    fn clamp(self, [x, y]: P) -> P {
        let [x0, y0, x1, y1] = self.0;
        [x.clamp(x0, x1), y.clamp(y0, y1)]
    }
}

fn numbers<const N: usize>(v: &Value, bounds: Bounds) -> Option<[f32; N]> {
    points(v, N, bounds)?.as_flattened().try_into().ok()
}

/// `[x, y, …]` with at most `max` points, all numbers, each clamped to `bounds`.
fn points(v: &Value, max: usize, bounds: Bounds) -> Option<Vec<P>> {
    let v = v
        .as_array()
        .filter(|v| v.len() % 2 == 0 && v.len() <= 2 * max)?;
    let n: Vec<f32> = v
        .iter()
        .map(|n| n.as_f64().filter(|n| n.is_finite()).map(|n| n as f32))
        .collect::<Option<_>>()?;
    Some(
        n.as_chunks::<2>()
            .0
            .iter()
            .map(|p| bounds.clamp(*p))
            .collect(),
    )
}

/// The four corners of the rectangle between `a` and `b`, clockwise from `a`.
fn rect(a: P, b: P) -> [P; 4] {
    [a, [b[0], a[1]], b, [a[0], b[1]]]
}

/// What a person's state shows, in image pixels: the ghost's corners, a rubber band's two corners (and whether it is
/// an ellipse), a path (a stroke's with its width, or a lasso's) and the pointer.
struct Targets {
    ghost: Vec<P>,
    band: Vec<P>,
    ellipse: bool,
    path: Vec<P>,
    width: Option<f32>,
    pointer: Vec<P>,
}

fn targets(state: &Value, bounds: Bounds) -> Targets {
    let stroke = &state["stroke"];
    // The Paintbrush's largest size.
    let width = stroke["size"]
        .as_f64()
        .filter(|s| (0.0..=2000.0).contains(s))
        .map(|s| s as f32);
    let path = match width {
        Some(_) => points(&stroke["points"], MAX_POINTS, bounds),
        None => points(&state["lasso"], MAX_POINTS, bounds),
    };
    Targets {
        ghost: points(&state["ghost"], 4, bounds)
            .filter(|g| g.len() == 4)
            .unwrap_or_default(),
        band: numbers::<4>(&state["band"], bounds)
            .map(|[x0, y0, x1, y1]| vec![[x0, y0], [x1, y1]])
            .unwrap_or_default(),
        ellipse: state["ellipse"] == true,
        path: path.unwrap_or_default(),
        width,
        pointer: numbers::<2>(&state["pointer"], bounds)
            .map(|p| vec![p])
            .unwrap_or_default(),
    }
}

impl Shown {
    /// `k` of the way on toward `t`; a first sight, another kind of mark or a jump of more than `far` (screen points
    /// on `xf`) goes at once. Whether a mark is still on its way.
    fn step(&mut self, t: &Targets, k: f32, far: f32, xf: &Xform) -> bool {
        let screen = |p: P| xf.to_screen(p[0], p[1]);
        let ease = |s: &mut Vec<P>, t: &[P]| {
            if s.len() != t.len()
                || s.iter()
                    .zip(t)
                    .any(|(a, b)| (screen(*a) - screen(*b)).length() > far)
            {
                *s = t.to_vec();
                return false;
            }
            let mut moving = false;
            for (a, b) in s.iter_mut().zip(t) {
                *a = [a[0] + (b[0] - a[0]) * k, a[1] + (b[1] - a[1]) * k];
                match (screen(*a) - screen(*b)).length() < 0.5 {
                    true => *a = *b,
                    false => moving = true,
                }
            }
            moving
        };
        let mut moving = ease(&mut self.pointer, &t.pointer);
        moving |= ease(&mut self.ghost, &t.ghost);
        moving |= ease(&mut self.band, &t.band);
        // A path's newest end: what came since the newest point drawn is drawn as it comes.
        let tip = t.path.last().copied();
        if tip != self.tip {
            let since = self
                .tip
                .zip(tip)
                .and_then(|(old, _)| from_end(&t.path, old, 4.0 / xf.zoom.max(1e-3)));
            self.behind = since.map_or(0.0, |d| self.behind + d);
            self.tip = tip;
        }
        self.behind *= 1.0 - k;
        if self.behind * xf.zoom < 0.5 || self.behind * xf.zoom > far {
            self.behind = 0.0;
        }
        moving || self.behind > 0.0
    }
}

fn dist(a: P, b: P) -> f32 {
    (a[0] - b[0]).hypot(a[1] - b[1])
}

/// How far along `path` from `q` (within `tol` of it) to its newest end, searched from that end.
fn from_end(path: &[P], q: P, tol: f32) -> Option<f32> {
    let mut d = 0.0;
    for w in path.windows(2).rev() {
        let (a, b) = (w[0], w[1]);
        let len = dist(a, b);
        let t = if len > 0.0 {
            (((q[0] - a[0]) * (b[0] - a[0]) + (q[1] - a[1]) * (b[1] - a[1])) / (len * len))
                .clamp(0.0, 1.0)
        } else {
            0.0
        };
        let on = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
        if dist(on, q) <= tol {
            return Some(d + len * (1.0 - t));
        }
        d += len;
    }
    None
}

/// `path` without its last `behind` pixels.
fn cut(path: &[P], mut behind: f32) -> Vec<P> {
    let mut end = path.len();
    while behind > 0.0 && end > 1 {
        let (a, b) = (path[end - 2], path[end - 1]);
        let len = dist(a, b);
        if behind < len {
            let t = behind / len;
            let mut kept = path[..end - 1].to_vec();
            kept.push([b[0] + (a[0] - b[0]) * t, b[1] + (a[1] - b[1]) * t]);
            return kept;
        }
        behind -= len;
        end -= 1;
    }
    path[..end].to_vec()
}

/// A translucent stroke `w` wide along `path` (screen points), as one mesh so that no part of it covers another: round
/// caps, mitred joins (at most twice as long), its edges feathered over `aa` points as egui feathers its own shapes.
fn stroke(path: &[Pos2], w: f32, ink: Color32, aa: f32) -> Shape {
    let mut pts: Vec<Pos2> = Vec::with_capacity(path.len());
    for &p in path {
        if pts.last().is_none_or(|q| q.distance(p) > 0.01) {
            pts.push(p);
        }
    }
    let n = pts.len();
    let (inner, outer) = ((w - aa).max(0.0) / 2.0, (w + aa) / 2.0);
    let clear = Color32::TRANSPARENT;
    let dir = |i: usize| (pts[i + 1] - pts[i]).normalized();
    let mut mesh = egui::Mesh::default();
    // Per point, along its normal: the left edge's feather and inside, the right edge's inside and feather.
    let normals: Vec<Vec2> = (0..n)
        .map(|i| match i {
            _ if n == 1 => Vec2::Y,
            0 => dir(0).rot90(),
            _ if i == n - 1 => dir(n - 2).rot90(),
            _ => {
                let (a, b) = (dir(i - 1).rot90(), dir(i).rot90());
                let m = (a + b).normalized();
                m / m.dot(a).max(0.5)
            }
        })
        .collect();
    for (p, normal) in pts.iter().zip(&normals) {
        for (off, c) in [(outer, clear), (inner, ink), (-inner, ink), (-outer, clear)] {
            mesh.colored_vertex(*p + *normal * off, c);
        }
    }
    for a in (0..n.saturating_sub(1) as u32).map(|i| 4 * i) {
        for k in 0..3 {
            mesh.add_triangle(a + k, a + k + 1, a + 4 + k);
            mesh.add_triangle(a + k + 1, a + 5 + k, a + 4 + k);
        }
    }
    // Half a disc out of each end (a whole one around a single point), from the end's left edge to its right.
    let mut cap = |i: usize, out: Vec2| {
        let (p, left) = (pts[i], normals[i]);
        let at = 4 * i as u32;
        let centre = mesh.vertices.len() as u32;
        mesh.colored_vertex(p, ink);
        let (mut last_in, mut last_out) = (at + 1, at);
        for k in 1..=12 {
            let (sin, cos) = (std::f32::consts::PI * k as f32 / 12.0).sin_cos();
            let (inside, edge) = match k {
                12 => (at + 2, at + 3),
                _ => {
                    let u = left * cos + out * sin;
                    let v = mesh.vertices.len() as u32;
                    mesh.colored_vertex(p + u * inner, ink);
                    mesh.colored_vertex(p + u * outer, clear);
                    (v, v + 1)
                }
            };
            mesh.add_triangle(centre, last_in, inside);
            mesh.add_triangle(last_in, last_out, inside);
            mesh.add_triangle(last_out, edge, inside);
            (last_in, last_out) = (inside, edge);
        }
    };
    match n {
        0 => {}
        1 => {
            cap(0, Vec2::X);
            cap(0, -Vec2::X);
        }
        _ => {
            cap(0, -dir(0));
            cap(n - 1, dir(n - 2));
        }
    }
    Shape::mesh(mesh)
}

/// The most dashes a dashed line draws: past that, it is drawn solid.
const MAX_DASHES: usize = 2000;

/// The part of segment `a`–`b` inside `clip` (Liang–Barsky), if any.
fn clip_segment(a: Pos2, b: Pos2, clip: Rect) -> Option<(Pos2, Pos2)> {
    let d = b - a;
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, q) in [
        (-d.x, a.x - clip.min.x),
        (d.x, clip.max.x - a.x),
        (-d.y, a.y - clip.min.y),
        (d.y, clip.max.y - a.y),
    ] {
        if p == 0.0 {
            if q < 0.0 {
                return None;
            }
        } else {
            let t = q / p;
            if p < 0.0 {
                t0 = t0.max(t);
            } else {
                t1 = t1.min(t);
            }
        }
    }
    (t0 <= t1).then(|| (a + d * t0, a + d * t1))
}

/// `path` dashed (4 points on, 3 off) where it crosses `clip`: a dash per 7 points of what shows, so at most the
/// screen's worth, and solid past [`MAX_DASHES`].
fn dashed(path: &[Pos2], stroke: Stroke, clip: Rect) -> Vec<Shape> {
    let mut out = Vec::new();
    let mut budget = MAX_DASHES;
    for w in path.windows(2) {
        let Some((a, b)) = clip_segment(w[0], w[1], clip) else {
            continue;
        };
        let dashes = (a.distance(b) / 7.0).ceil() as usize;
        if dashes > budget {
            return vec![Shape::line(path.to_vec(), stroke)];
        }
        budget -= dashes;
        out.extend(Shape::dashed_line(&[a, b], stroke, 4.0, 3.0));
    }
    out
}

/// Relative luminance (sRGB) of `c`.
fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// `name` cut to 24 characters, with "…" when cut.
fn tag(name: &str) -> String {
    match name.chars().nth(24) {
        Some(_) => name.chars().take(23).chain(['…']).collect(),
        None => name.to_owned(),
    }
}

/// The pointer arrow, tip at (0, 0), in points.
const ARROW: [[f32; 2]; 7] = [
    [0.0, 0.0],
    [0.0, 16.0],
    [4.5, 12.2],
    [7.6, 18.6],
    [10.4, 17.3],
    [7.3, 11.0],
    [12.5, 11.0],
];

/// `points` filled with `fill` as a fan from the first (the arrow is star-shaped around its tip).
fn fan(points: &[Pos2], fill: Color32) -> Shape {
    let mut mesh = egui::Mesh::default();
    for p in points {
        mesh.colored_vertex(*p, fill);
    }
    for i in 1..points.len() as u32 - 1 {
        mesh.add_triangle(0, i, i + 1);
    }
    Shape::mesh(mesh)
}

/// A person's pointer: the arrow in their colour with a white outline and a soft shadow, and their name tag.
fn pointer(painter: &egui::Painter, tip: Pos2, color: Color32, name: &str) {
    let ppp = painter.pixels_per_point();
    let tip = (tip.to_vec2() * ppp).round().to_pos2() / ppp;
    let arrow = |off: Vec2| ARROW.map(|[x, y]| tip + off + vec2(x, y));
    painter.add(fan(&arrow(vec2(0.0, 1.0)), Color32::from_black_alpha(64)));
    painter.add(fan(&arrow(Vec2::ZERO), color));
    painter.add(Shape::closed_line(
        arrow(Vec2::ZERO).to_vec(),
        Stroke::new(1.5, Color32::WHITE),
    ));
    if name.is_empty() {
        return;
    }
    let ink = if luminance(color) > 0.55 {
        Color32::from_rgb(0x11, 0x11, 0x11)
    } else {
        Color32::WHITE
    };
    let text = painter.layout_no_wrap(tag(name), FontId::new(12.0, FontFamily::Proportional), ink);
    let pad = vec2(7.0, 3.0);
    let at = tip + vec2(12.0, 15.0);
    painter.rect_filled(Rect::from_min_size(at, text.size() + 2.0 * pad), 6.0, color);
    painter.galley(at + pad, text, ink);
}

fn color(p: &super::template::Presence) -> Color32 {
    Color32::from_rgb(p.color[0], p.color[1], p.color[2])
}

/// The others on each image's tab: a dot each in their colour, stacked down the tab's left edge (three at most).
fn dots(app: &crate::PaintApp, ui: &egui::Ui, live: &super::Live) {
    let edge = Stroke::new(1.0, Color32::from_black_alpha(90));
    for (doc, tab) in &app.shell.document_strip.tabs {
        let Some(uid) = app
            .app
            .documents
            .iter()
            .find(|ws| ws.document.id == *doc)
            .and_then(|ws| super::uid_of(live, ws.id))
        else {
            continue;
        };
        let on: Vec<_> = live
            .others
            .iter()
            .filter(|p| p.state["doc"].as_u64() == Some(uid))
            .take(3)
            .collect();
        let painter = ui.painter().with_clip_rect(*tab);
        for (i, p) in on.iter().enumerate() {
            let y = (i as f32 - (on.len() - 1) as f32 / 2.0) * 7.0;
            painter.circle(
                pos2(tab.left() + 6.0, tab.center().y + y),
                2.5,
                color(p),
                edge,
            );
        }
    }
}

/// The others on the image shown, on the Layers rows of their active layers: a dot each by the row's left edge (three at
/// most), drawn in the Layers window's own layer, the frame it was shown.
fn layer_dots(app: &crate::PaintApp, ui: &egui::Ui, live: &super::Live, here: u64) {
    let form = &app.shell.forms.layers;
    let (Some(ws), Some(layer)) = (app.app.active_ws(), form.shown_layer) else {
        return;
    };
    if form.shown_pass != ui.ctx().cumulative_pass_nr() {
        return;
    }
    let painter = ui.ctx().layer_painter(layer);
    let edge = Stroke::new(1.0, Color32::from_black_alpha(90));
    for (index, row) in &form.shown {
        let Some(id) = ws.document.layers.get_at(*index).map(|l| l.id) else {
            continue;
        };
        let on: Vec<_> = (live.others.iter())
            .filter(|p| {
                p.state["doc"].as_u64() == Some(here) && p.state["layers"][0].as_u64() == Some(id)
            })
            .take(3)
            .collect();
        let painter = painter.with_clip_rect(*row);
        for (i, p) in on.iter().enumerate() {
            let y = (i as f32 - (on.len() - 1) as f32 / 2.0) * 7.0;
            painter.circle(
                pos2(row.left() + 5.0, row.center().y + y),
                2.5,
                color(p),
                edge,
            );
        }
    }
}

/// After this long, a hint says others' changes wait for this person.
const HOLD_HINT: f64 = 1.5;

/// `text` in a dark pill centred on `canvas`, at its foot or (`top`) its head, wrapped to fit.
fn pill(ui: &egui::Ui, canvas: Rect, text: String, top: bool) {
    let painter = ui.painter().with_clip_rect(canvas);
    let pad = vec2(10.0, 5.0);
    let font = FontId::new(12.0, FontFamily::Proportional);
    let wrap = (canvas.width() - 4.0 * pad.x).clamp(40.0, 520.0);
    let text = painter.layout(text, font, Color32::WHITE, wrap);
    let size = text.size() + 2.0 * pad;
    let y = match top {
        true => canvas.top() + 12.0,
        false => canvas.bottom() - size.y - 12.0,
    };
    let at = pos2(canvas.center().x - size.x / 2.0, y);
    let back = Color32::from_black_alpha(170);
    painter.rect_filled(Rect::from_min_size(at, size), 8.0, back);
    painter.galley(at + pad, text, Color32::WHITE);
}

/// The hint at the foot of the canvas while others' changes wait for this person to finish a gesture or close a
/// dialog, and at its head why an image the session doesn't have isn't shared.
fn hint(app: &crate::PaintApp, ui: &egui::Ui, live: &super::Live) {
    let Some((_, canvas)) = app.shell.canvas_view.shown else {
        return;
    };
    let shown = app.app.active_ws().map(|ws| ws.id);
    if let Some(a) = live.alone.iter().find(|a| Some(a.local) == shown) {
        pill(ui, canvas, a.why.text(), true);
    }
    let Some(since) = live.held else { return };
    let waited = ui.input(|i| i.time) - since;
    if waited < HOLD_HINT {
        return ui
            .ctx()
            .request_repaint_after_secs((HOLD_HINT - waited) as f32);
    }
    pill(ui, canvas, pdn_core::i18n::t("liveSession.waiting"), false);
}

/// Draws the others on the canvas of the image shown (those on it), on the tabs and on the Layers rows, and asks for
/// another frame while a mark is still on its way.
pub fn draw(app: &crate::PaintApp, ui: &egui::Ui) {
    LIVE.with_borrow(|live| {
        if !live.connected {
            return SHOWN.with_borrow_mut(HashMap::clear);
        }
        hint(app, ui, live);
        if live.others.is_empty() {
            return SHOWN.with_borrow_mut(HashMap::clear);
        }
        dots(app, ui, live);
        SHOWN.with_borrow_mut(|shown| {
            let ws = app.app.active_ws();
            let here = ws.and_then(|ws| super::uid_of(live, ws.id));
            let others: Vec<_> = live
                .others
                .iter()
                .filter(|p| here.is_some() && p.state["doc"].as_u64() == here)
                .collect();
            shown.retain(|id, _| others.iter().any(|p| p.id == *id));
            if let Some(here) = here {
                layer_dots(app, ui, live, here);
            }
            let (Some(ws), Some((origin, visible))) = (ws, app.shell.canvas_view.shown) else {
                return;
            };
            if others.is_empty() {
                return;
            }
            let mut xf = Xform {
                view: &ws.view,
                origin,
                rect: visible,
                zoom: 1.0,
            };
            xf.zoom = (xf.to_screen(1.0, 0.0) - xf.to_screen(0.0, 0.0)).x.abs();
            let painter = ui.painter().with_clip_rect(xf.rect);
            let k = 1.0 - (-ui.input(|i| i.stable_dt).clamp(0.0, 0.1) / EASE).exp();
            let far = xf.rect.width().max(xf.rect.height()) / 4.0;
            let screen = |p: &P| xf.to_screen(p[0], p[1]);
            let mut moving = false;
            let bounds = Bounds::of(ws.document.width, ws.document.height);
            let wanted: Vec<_> = others.iter().map(|p| targets(&p.state, bounds)).collect();
            // Dashes past the canvas edge don't show.
            let clip = xf.rect.expand(4.0);
            for (p, t) in others.iter().zip(&wanted) {
                moving |= shown.entry(p.id.clone()).or_default().step(t, k, far, &xf);
            }
            let peers: Vec<_> = others
                .iter()
                .zip(&wanted)
                .filter_map(|(p, t)| Some((*p, t, shown.get(&p.id)?)))
                .collect();
            for (p, t, s) in &peers {
                let color = color(p);
                if s.ghost.len() == 4 {
                    let fill = color.gamma_multiply(0.28);
                    painter.add(Shape::convex_polygon(
                        s.ghost.iter().map(screen).collect(),
                        fill,
                        Stroke::new(1.5, color.gamma_multiply(0.85)),
                    ));
                }
                let band = match s.band[..] {
                    [a, b] if t.ellipse => (0..=64)
                        .map(|i| {
                            let r = std::f32::consts::TAU * i as f32 / 64.0;
                            let c = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
                            screen(&[
                                c[0] + (b[0] - a[0]) / 2.0 * r.cos(),
                                c[1] + (b[1] - a[1]) / 2.0 * r.sin(),
                            ])
                        })
                        .collect(),
                    [a, b] => rect(a, b).iter().chain([&a]).map(screen).collect(),
                    _ => Vec::new(),
                };
                let path: Vec<Pos2> = cut(&t.path, s.behind).iter().map(screen).collect();
                match t.width {
                    Some(w) if !path.is_empty() => {
                        let aa = 1.0 / painter.pixels_per_point();
                        painter.add(stroke(
                            &path,
                            (w * xf.zoom).clamp(2.0, 4096.0),
                            color.gamma_multiply(0.7),
                            aa,
                        ));
                    }
                    _ => painter.extend(dashed(&path, Stroke::new(1.5, color), clip)),
                }
                painter.extend(dashed(&band, Stroke::new(1.5, color), clip));
            }
            for (p, ..) in &peers {
                if let Some([x0, y0, x1, y1]) = numbers::<4>(&p.state["selection"], bounds) {
                    // On pixel edges: a 2-point line there stays crisp.
                    let ppp = painter.pixels_per_point();
                    let corners = rect([x0, y0], [x1, y1])
                        .iter()
                        .map(|p| (screen(p).to_vec2() * ppp).round().to_pos2() / ppp)
                        .collect();
                    painter.add(Shape::closed_line(corners, Stroke::new(2.0, color(p))));
                }
            }
            for (p, _, s) in &peers {
                if let Some(at) = s
                    .pointer
                    .first()
                    .map(screen)
                    .filter(|at| xf.rect.contains(*at))
                {
                    pointer(&painter, at, color(p), &p.name);
                }
            }
            if moving {
                ui.ctx().request_repaint();
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_hostile_presence_is_clamped_to_the_image_and_its_dashes_stay_bounded() {
        let b = Bounds::of(100, 50);
        let t = targets(
            &json!({"band": [-9.9e6, -9.9e6, 9.9e6, 9.9e6], "lasso": [-1e300, 0, 1e300, 0], "pointer": [5e6, -5e6],
                "stroke": {"size": 1e9, "points": [0, 0]}, "ghost": [1, 2, 3]}),
            b,
        );
        assert_eq!(t.band, [[-100.0, -100.0], [200.0, 150.0]]);
        assert_eq!(t.pointer, [[200.0, -100.0]]);
        assert_eq!(
            (t.width, t.path),
            (None, vec![[-100.0, 0.0], [200.0, 0.0]]),
            "no brush that wide: the lasso"
        );
        assert!(t.ghost.is_empty());
        let red = Stroke::new(1.5, Color32::RED);
        let screen = Rect::from_min_size(Pos2::ZERO, vec2(1920.0, 1080.0));
        // A band past every edge of the screen draws nothing; a line across it, a screen's worth of dashes.
        let around = [
            pos2(-1e8, -1e8),
            pos2(1e8, -1e8),
            pos2(1e8, 1e8),
            pos2(-1e8, 1e8),
            pos2(-1e8, -1e8),
        ];
        assert!(dashed(&around, red, screen).is_empty());
        let across = dashed(&[pos2(-1e8, 500.0), pos2(1e8, 500.0)], red, screen);
        assert!(
            (1..=1920 / 7 + 2).contains(&across.len()),
            "{}",
            across.len()
        );
        // Hundreds of segments across it: solid, one shape.
        let zigzag: Vec<Pos2> = (0..400)
            .map(|i| pos2(if i % 2 == 0 { -1e6 } else { 1e6 }, i as f32))
            .collect();
        assert_eq!(dashed(&zigzag, red, screen).len(), 1);
        assert_eq!(
            clip_segment(
                pos2(-10.0, 5.0),
                pos2(30.0, 5.0),
                Rect::from_min_max(Pos2::ZERO, pos2(20.0, 10.0))
            ),
            Some((pos2(0.0, 5.0), pos2(20.0, 5.0)))
        );
    }
}
