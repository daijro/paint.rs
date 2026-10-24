//! The Curves dialog's editor: Paint.NET's point editing on paintdotjs's 354×312 canvas.
//! Port of: js/src/interface/window/effect/CurveEditor.js (`draw`, `drawChannel`, the canvas
//! geometry), css/window.css (`.effect-curve-editor`, `.curves-content .effect-curve-editor`)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's units, hit test and edits; paintdotjs's look kept.

use pdn_core::canvas::{Canvas, LineJoin, Style};
use pdn_core::effect::adjustments::curve_samples;

/// A control point in curve units.
pub type Point = [i32; 2];

/// The identity curve, every channel's reset.
pub const IDENTITY: [Point; 2] = [[0, 0], [255, 255]];

/// §2.9 item 7: mouse → curve units on a w×h control, mx = int(0.5 + x·255/(w − 1)) and my =
/// int(0.5 + 255 − y·255/(h − 1)), each clamped to [0, 255].
pub fn to_curve_units(x: f32, y: f32, w: f32, h: f32) -> (i32, i32) {
    let mx = (0.5 + x as f64 * 255.0 / (w as f64 - 1.0)) as i32;
    let my = (0.5 + 255.0 - y as f64 * 255.0 / (h as f64 - 1.0)) as i32;
    (mx.clamp(0, 255), my.clamp(0, 255))
}

/// §2.9 item 7: the nearest point with dx² + dy² < 30 in curve units.
pub fn hit(points: &[Point], at: (i32, i32)) -> Option<usize> {
    let d2 = |p: &Point| (p[0] - at.0).pow(2) + (p[1] - at.1).pow(2);
    points
        .iter()
        .enumerate()
        .filter(|(_, p)| d2(p) < 30)
        .min_by_key(|(_, p)| d2(p))
        .map(|(i, _)| i)
}

fn is_end(x: i32) -> bool {
    x == 0 || x == 255
}

/// Sets the point at `p`'s x (insert or replace), keeping the list sorted by x.
fn put(points: &mut Vec<Point>, p: Point) {
    match points.binary_search_by_key(&p[0], |q| q[0]) {
        Ok(i) => points[i] = p,
        Err(i) => points.insert(i, p),
    }
}

/// The left-button drag in progress: per dragged channel, the x it last set and a point it is
/// passing over.
struct Drag {
    channels: Vec<usize>,
    last_x: Vec<Option<i32>>,
    saved: Vec<Option<Point>>,
}

/// The curve editor's state: one point list per channel (one in Luminosity mode, three in RGB), the
/// channels an edit touches, and the pointer.
pub struct CurveEditor {
    pub channels: Vec<Vec<Point>>,
    pub checked: Vec<bool>,
    /// The pointer in curve units while it is over the editor (the guides and the coordinates
    /// label).
    pub hover: Option<(i32, i32)>,
    drag: Option<Drag>,
}

impl CurveEditor {
    /// A one-channel editor.
    pub fn new(points: Vec<Point>) -> Self {
        Self::with_channels(vec![points])
    }

    pub fn with_channels(channels: Vec<Vec<Point>>) -> Self {
        Self {
            checked: vec![true; channels.len()],
            channels,
            hover: None,
            drag: None,
        }
    }

    /// The first channel's points.
    pub fn points(&self) -> &[Point] {
        &self.channels[0]
    }

    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// Mouse-down at `at`: a hit interior point leaves every checked channel that hit it (any
    /// button); a left press then tracks, re-adding it at the cursor. A left press on nothing adds
    /// a point to every checked channel. Returns whether the points changed.
    pub fn press(&mut self, at: (i32, i32), left: bool) -> bool {
        let hits: Vec<(usize, usize)> = (0..self.channels.len())
            .filter(|&c| self.checked[c])
            .filter_map(|c| hit(&self.channels[c], at).map(|i| (c, i)))
            .collect();
        let mut changed = false;
        let (channels, last_x) = if hits.is_empty() {
            let checked: Vec<usize> = (0..self.channels.len())
                .filter(|&c| self.checked[c])
                .collect();
            let n = checked.len();
            (checked, vec![None; n])
        } else {
            let mut last = vec![];
            for &(c, i) in &hits {
                let x = self.channels[c][i][0];
                if !is_end(x) {
                    self.channels[c].remove(i);
                    changed = true;
                }
                last.push(Some(x));
            }
            (hits.iter().map(|h| h.0).collect(), last)
        };
        if !left || channels.is_empty() {
            return changed;
        }
        let n = channels.len();
        self.drag = Some(Drag {
            channels,
            last_x,
            saved: vec![None; n],
        });
        self.drag_to(at) || changed
    }

    /// The tracking step: the point at the previous x goes (an endpoint stays), a passed-over
    /// point returns once the cursor leaves its x, and (mx, my) is set.
    pub fn drag(&mut self, at: (i32, i32)) -> bool {
        self.drag_to(at)
    }

    fn drag_to(&mut self, (mx, my): (i32, i32)) -> bool {
        let Some(d) = &mut self.drag else {
            return false;
        };
        for (k, &c) in d.channels.iter().enumerate() {
            let pts = &mut self.channels[c];
            if let Some(lx) = d.last_x[k].filter(|&x| !is_end(x)) {
                pts.retain(|p| p[0] != lx);
            }
            if let Some(s) = d.saved[k].filter(|s| s[0] != mx) {
                put(pts, s);
                d.saved[k] = None;
            }
            if d.saved[k].is_none()
                && d.last_x[k] != Some(mx)
                && let Some(p) = pts.iter().find(|p| p[0] == mx)
            {
                d.saved[k] = Some(*p);
            }
            put(pts, [mx, my]);
            d.last_x[k] = Some(mx);
        }
        true
    }

    pub fn release(&mut self) {
        self.drag = None;
    }

    /// Reset: {(0, 0), (255, 255)} in every channel.
    pub fn reset(&mut self) {
        for c in &mut self.channels {
            *c = IDENTITY.to_vec();
        }
        self.drag = None;
    }
}

/// A channel's stroke colour (`createCurvesContent`'s channel sets).
pub const LUMINOSITY: [u8; 3] = [0xF2, 0xF2, 0xF2];
pub const RGB: [[u8; 3]; 3] = [[0xFF, 0x4B, 0x4B], [0x45, 0xD6, 0x5A], [0x4B, 0x83, 0xFF]];

/// The colours `draw` reads from the theme (`--color-curve-background`, `--color-curve-grid`).
pub struct CurveColors {
    pub background: [u8; 3],
    pub grid: [u8; 3],
}

/// `CurveEditor.draw` on a w×h canvas: the background, the dashed quarter grid and diagonal, the
/// pointer guides, then each channel's curve through its 256-entry lookup and its points.
pub fn draw(ed: &CurveEditor, colors: &[[u8; 3]], theme: &CurveColors, w: u32, h: u32) -> Canvas {
    let rgba = |c: [u8; 3]| Style::Color([c[0], c[1], c[2], 255]);
    let (wf, hf) = (w as f64, h as f64);
    let to_x = |v: f64| v / 255.0 * (wf - 1.0);
    let to_y = |v: f64| (255.0 - v) / 255.0 * (hf - 1.0);
    let mut cx = Canvas::new(w, h);
    cx.set_fill_style(rgba(theme.background));
    cx.fill_rect(0.0, 0.0, wf, hf);
    cx.set_stroke_style(rgba(theme.grid));
    cx.set_line_width(1.0);
    cx.set_line_dash(vec![3.0, 2.0]);
    for i in 1..4 {
        let x = pdn_core::js::math_round((wf - 1.0) * i as f64 / 4.0) + 0.5;
        let y = pdn_core::js::math_round((hf - 1.0) * i as f64 / 4.0) + 0.5;
        cx.begin_path();
        cx.move_to(x, 0.0);
        cx.line_to(x, hf);
        cx.move_to(0.0, y);
        cx.line_to(wf, y);
        cx.stroke();
    }
    cx.begin_path();
    cx.move_to(0.0, hf - 1.0);
    cx.line_to(wf - 1.0, 0.0);
    cx.stroke();
    cx.set_line_dash(vec![]);
    if let Some((px, py)) = ed.hover {
        let (x, y) = (to_x(px as f64), to_y(py as f64));
        cx.save();
        cx.set_global_alpha(0.45);
        cx.begin_path();
        cx.move_to(x + 0.5, 0.0);
        cx.line_to(x + 0.5, hf);
        cx.move_to(0.0, y + 0.5);
        cx.line_to(wf, y + 0.5);
        cx.stroke();
        cx.restore();
    }
    for (c, points) in ed.channels.iter().enumerate() {
        let selected = ed.checked[c];
        let color = colors[c.min(colors.len() - 1)];
        let pts: Vec<[f64; 2]> = points.iter().map(|p| [p[0] as f64, p[1] as f64]).collect();
        // BitmapEffectEngine.createCurveLookup: the spline at x = 0..255, stored Uint8Clamped.
        let lookup: Vec<f64> = curve_samples(&pts, 256)
            .into_iter()
            .map(|v| pdn_core::js::clamped_u8(v * 255.0) as f64)
            .collect();
        cx.save();
        cx.set_global_alpha(if selected { 1.0 } else { 0.5 });
        cx.set_stroke_style(rgba(color));
        cx.set_line_width(if selected { 2.0 } else { 1.0 });
        cx.set_line_join(LineJoin::Round);
        cx.begin_path();
        cx.move_to(to_x(0.0), to_y(lookup[0]));
        for (x, v) in lookup.iter().enumerate().skip(1) {
            cx.line_to(to_x(x as f64), to_y(*v));
        }
        cx.stroke();
        for p in points {
            let (x, y) = (to_x(p[0] as f64), to_y(p[1] as f64));
            cx.set_fill_style(rgba(if selected { color } else { [0x88; 3] }));
            cx.set_stroke_style(rgba(if selected { [0xFF; 3] } else { [0x44; 3] }));
            cx.set_line_width(1.0);
            cx.begin_path();
            cx.arc(
                x,
                y,
                if selected { 4.0 } else { 2.5 },
                0.0,
                std::f64::consts::TAU,
                false,
            );
            cx.fill(pdn_core::canvas::FillRule::NonZero);
            cx.stroke();
        }
        cx.restore();
    }
    cx
}

#[cfg(test)]
mod tests {
    use super::*;

    /// adjustments-and-effect-framework.md §2.9 Curves item 7, on paintdotjs's 354×312 canvas.
    #[test]
    fn curve_units_and_hits() {
        assert_eq!(to_curve_units(177.0, 156.0, 354.0, 312.0), (128, 127));
        assert_eq!(to_curve_units(147.0, 106.0, 354.0, 312.0), (106, 168));
        assert_eq!(to_curve_units(-5.0, 400.0, 354.0, 312.0), (0, 0));
        let points = [[0, 0], [128, 120], [255, 255]];
        assert_eq!(hit(&points, (131, 124)), Some(1)); // 9 + 16 = 25 < 30
        assert_eq!(hit(&points, (133, 124)), None); // 25 + 16 = 41
    }

    #[test]
    fn endpoints_move_vertically_and_interior_points_delete() {
        let mut ed = CurveEditor::new(vec![[0, 0], [255, 255]]);
        ed.press((0, 0), true);
        ed.drag((0, 60));
        assert_eq!(ed.points(), [[0, 60], [255, 255]]);
        ed.drag((40, 60)); // the endpoint is never removed: dragging away creates an interior point
        ed.release();
        assert_eq!(ed.points(), [[0, 60], [40, 60], [255, 255]]);
        ed.press((100, 100), true);
        ed.drag((100, 140));
        ed.release();
        assert_eq!(ed.points(), [[0, 60], [40, 60], [100, 140], [255, 255]]);
        ed.press((100, 140), false); // right button on an interior hit removes it
        ed.release();
        assert_eq!(ed.points(), [[0, 60], [40, 60], [255, 255]]);
        ed.press((255, 255), false); // never an endpoint
        ed.release();
        assert_eq!(ed.points(), [[0, 60], [40, 60], [255, 255]]);
    }

    /// §2.9 item 7: a point passed over returns once the cursor leaves its x; an edit touches
    /// the checked channels only.
    #[test]
    fn passed_over_points_return_and_unchecked_channels_stay() {
        let mut ed = CurveEditor::new(vec![[0, 0], [100, 50], [200, 200], [255, 255]]);
        ed.press((100, 50), true);
        ed.drag((200, 90));
        assert_eq!(ed.points(), [[0, 0], [200, 90], [255, 255]]);
        ed.drag((210, 90));
        ed.release();
        assert_eq!(ed.points(), [[0, 0], [200, 200], [210, 90], [255, 255]]);
        let mut rgb = CurveEditor::with_channels(vec![IDENTITY.to_vec(); 3]);
        rgb.checked[1] = false;
        rgb.press((128, 128), true);
        rgb.release();
        assert_eq!(rgb.channels[0], [[0, 0], [128, 128], [255, 255]]);
        assert_eq!(rgb.channels[1], IDENTITY);
        let mid = vec![[0, 0], [128, 128], [255, 255]];
        let mut rgb = CurveEditor::with_channels(vec![mid.clone(); 3]);
        rgb.checked[1] = false;
        rgb.press((128, 128), false);
        assert_eq!(
            [&rgb.channels[0], &rgb.channels[1], &rgb.channels[2]],
            [&IDENTITY.to_vec(), &mid, &IDENTITY.to_vec()]
        );
    }
}
