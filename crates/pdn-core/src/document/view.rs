//! Port of: js/src/engine/document/DocumentView.js, and the viewport parts of js/src/engine/AppView.js
//! (`setViewPosition`/`getViewX`/`updateCanvasBounds`/ctrl+wheel zoom/auto-scroll).
//! Reference: paintdotjs @ b4e6fc5
//! At 100 % one image pixel is one device pixel.
//!
//! `zoom` is device px per image px; positions and sizes stay in points (CSS px), converted with
//! `css_scale()` = zoom/ppp.
//!
//! The DOM scroll container (`#view` / `#environment`) is modelled by `view_width/view_height`
//! (set by the UI) and `viewport_x/viewport_y` (the scroll offset). Every viewport write clamps
//! to `[0, environment - view]` as `scrollLeft/scrollTop` assignment does in the browser.

use super::surface::Surface;
use crate::app::AppEvent;
use crate::render::view::ViewTransform;
use crate::util::geom::{Point, Rectangle};
use crate::util::utility;

/// `ui-drawn-graphics.md §2.10` "Zoom presets": 1 % … 10000 %.
pub const ZOOM_PRESETS: [f64; 45] = [
    1.0 / 100.0,
    1.0 / 88.0,
    1.0 / 76.0,
    1.0 / 64.0,
    1.0 / 56.0,
    1.0 / 48.0,
    1.0 / 40.0,
    1.0 / 32.0,
    1.0 / 28.0,
    1.0 / 24.0,
    1.0 / 20.0,
    1.0 / 16.0,
    1.0 / 14.0,
    1.0 / 12.0,
    1.0 / 10.0,
    1.0 / 8.0,
    1.0 / 6.0,
    1.0 / 5.0,
    1.0 / 4.0,
    1.0 / 3.0,
    1.0 / 2.0,
    2.0 / 3.0,
    1.0,
    3.0 / 2.0,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    8.0,
    10.0,
    12.0,
    14.0,
    16.0,
    20.0,
    24.0,
    28.0,
    32.0,
    40.0,
    48.0,
    56.0,
    64.0,
    76.0,
    88.0,
    100.0,
];

/// Zoom In: the first preset ≥ r + 0.005, clamped to the last.
pub fn next_larger_zoom(r: f64) -> f64 {
    ZOOM_PRESETS
        .into_iter()
        .find(|&p| p >= r + 0.005)
        .unwrap_or(ZOOM_PRESETS[44])
}

/// Zoom Out: the preset before the first preset ≥ r − 0.005, clamped to the
/// first (none: the last-but-one).
pub fn next_smaller_zoom(r: f64) -> f64 {
    match ZOOM_PRESETS.iter().position(|&p| p >= r - 0.005) {
        Some(i) => ZOOM_PRESETS[i.saturating_sub(1)],
        None => ZOOM_PRESETS[43],
    }
}

/// Ctrl+wheel (§2.10 "Wheel" steps 2–3): ×1.12 per notch (÷ for negative notches), snapped to the
/// nearest integer ratio when it lies within 5 % of it and closer than the old ratio, clamped.
/// Appendix B B-C12 calibrates the notch distances.
pub fn wheel_zoom_ratio(old: f64, notches: f64) -> f64 {
    let f = 1.12f64.powf(notches.abs());
    let new = if notches > 0.0 { old * f } else { old / f };
    let k = new.round();
    let gap = (k - new).abs();
    let new = if gap < 0.05 * k && gap < (k - old).abs() {
        k
    } else {
        new
    };
    new.clamp(0.01, 100.0)
}

/// Fit to Window (`canvas-rendering.md §2.5` step 9): min((Vw − 9u)/W, (Vh − 9u)/H, 1), the view in
/// device px; the 9u is subtracted once, not per side. Appendix B B-C9 calibrates it.
pub fn fit_zoom(view_w: f64, view_h: f64, w: u32, h: u32, u: f64) -> f64 {
    ((view_w - 9.0 * u) / w as f64)
        .min((view_h - 9.0 * u) / h as f64)
        .clamp(0.01, 1.0)
}

/// The zoom slider's preset index (§2.10 "Zoom slider" step 2): k + (ln r − ln r_k)/(ln r_{k+1} −
/// ln r_k) between presets k and k + 1; exact presets give k.
pub fn zoom_slider_index(r: f64) -> f64 {
    let p = &ZOOM_PRESETS;
    let k = p.iter().rposition(|&x| x <= r).unwrap_or(0);
    if k >= p.len() - 1 {
        return k as f64;
    }
    k as f64 + (r.ln() - p[k].ln()) / (p[k + 1].ln() - p[k].ln())
}

/// The preset a zoom-slider index selects.
pub fn zoom_from_slider_index(i: u32) -> f64 {
    ZOOM_PRESETS[(i as usize).min(ZOOM_PRESETS.len() - 1)]
}

/// Device-px slack when flooring pointer positions: egui hands them over in f32 points, so a
/// pointer on device px 107 at 1.25× can read 106.9999.
const POINTER_SLACK: f64 = 0.01;

pub struct DocumentView {
    /// Device px per image px (1.0 = 100 %).
    pub zoom: f64,
    /// Device px per point (the OS scale factor), kept current by the UI.
    pub ppp: f64,
    pub zoom_to_window: bool,
    pub viewport_x: f64,
    pub viewport_y: f64,
    pub view_width: f64,
    pub view_height: f64,
    /// Canvas that combines all layers; always the document's size.
    pub composition: Surface,
    pub composition_revision: u64,
    /// Outbox for the JS `app.fire(...)` calls; drained by `AppWorkspace::drain_events`.
    pub events: Vec<AppEvent>,
    /// `AppView.lastViewWidth/Height` (`undefined` until the first `updateCanvasBounds`).
    last_view_size: Option<(f64, f64)>,
    native_text_scroll: bool,
}

/// `AppView.autoScrollIfNecessary`'s zone test: Paint.NET projects the pointer 2% farther from
/// the viewport centre; the scroll direction per axis (−1, 0, 1), `None` inside the viewport.
pub fn auto_scroll_direction(
    pointer: Point,
    view_bounds: (f64, f64, f64, f64),
) -> Option<(f64, f64)> {
    let (left, top, right, bottom) = view_bounds;
    let (center_x, center_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let projected_x = center_x + (pointer.x - center_x) * 1.02;
    let projected_y = center_y + (pointer.y - center_y) * 1.02;
    let direction = |p: f64, lo: f64, hi: f64| {
        if p < lo {
            -1.0
        } else if p > hi {
            1.0
        } else {
            0.0
        }
    };
    let d = (
        direction(projected_x, left, right),
        direction(projected_y, top, bottom),
    );
    (d != (0.0, 0.0)).then_some(d)
}

impl DocumentView {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            zoom: 1.0,
            ppp: 1.0,
            zoom_to_window: false,
            viewport_x: 0.0,
            viewport_y: 0.0,
            view_width: 0.0,
            view_height: 0.0,
            composition: Surface::create(width, height),
            composition_revision: 0,
            events: vec![],
            last_view_size: None,
            native_text_scroll: false,
        }
    }

    pub fn get_width(&self) -> f64 {
        self.composition.width() as f64
    }

    pub fn get_height(&self) -> f64 {
        self.composition.height() as f64
    }

    /// Points per image px: zoom/ppp.
    pub fn css_scale(&self) -> f64 {
        self.zoom / self.ppp
    }

    /// The OS scale changed (e.g. a monitor move): the zoom stays in device px, so the document's
    /// size in points changes; the document point at the viewport centre stays there (Fit to
    /// Window re-fits).
    pub fn set_pixels_per_point(&mut self, ppp: f64) {
        if ppp == self.ppp || !ppp.is_finite() || ppp <= 0.0 {
            return;
        }
        let centre = Point::new(self.view_width / 2.0, self.view_height / 2.0);
        let (b, css) = (self.get_render_bounds(), self.css_scale());
        let d = Point::new((centre.x - b.x) / css, (centre.y - b.y) / css);
        self.ppp = ppp;
        if self.zoom_to_window {
            self.fit_viewport();
            return;
        }
        let css = self.css_scale();
        let x = self.view_width - self.get_render_width().min(self.view_width) / 2.0 - centre.x;
        let y = self.view_height - self.get_render_height().min(self.view_height) / 2.0 - centre.y;
        self.set_view_position(x + d.x * css, y + d.y * css);
    }

    /// The device-px canvas transform: the render bounds' origin and the view size in device px,
    /// snapped at integer zoom and in Fit to Window (`canvas-rendering.md §2.5` step 7).
    pub fn view_transform(&self) -> ViewTransform {
        let b = self.get_render_bounds();
        let device = |v: f64| (v * self.ppp).round().max(0.0) as u32;
        ViewTransform {
            scale: self.zoom,
            origin: [b.x * self.ppp, b.y * self.ppp],
            viewport: [device(self.view_width), device(self.view_height)],
            ui_scale: self.ppp,
        }
        .snapped(self.zoom_to_window)
    }

    pub fn get_render_width(&self) -> f64 {
        self.get_width() * self.css_scale()
    }

    pub fn get_render_height(&self) -> f64 {
        self.get_height() * self.css_scale()
    }

    pub fn environment_size(&self) -> (f64, f64) {
        if self.zoom_to_window {
            return (self.view_width, self.view_height);
        }
        (
            (self.view_width + self.get_render_width()).max(self.view_width * 2.0),
            (self.view_height + self.get_render_height()).max(self.view_height * 2.0),
        )
    }

    /// `fitViewport` with Paint.NET's fit (`fit_zoom`) at the view's device size, centred.
    pub fn fit_viewport(&mut self) {
        self.zoom = fit_zoom(
            self.view_width * self.ppp,
            self.view_height * self.ppp,
            self.composition.width(),
            self.composition.height(),
            self.ppp,
        );
        self.center_view();
    }

    /// `setZoom(factor, pivotX, pivotY)`: the pivot's document point stays under the pivot.
    pub fn set_zoom(&mut self, factor: f64, pivot_x: f64, pivot_y: f64) {
        let factor = utility::clamp(factor, 0.01, 100.0);
        let old_bounds = self.get_render_bounds();
        let document_x = (pivot_x - old_bounds.x) / self.css_scale();
        let document_y = (pivot_y - old_bounds.y) / self.css_scale();
        // Any explicit scale change leaves Fit to Window.
        self.zoom_to_window = false;
        self.zoom = factor;
        let new_width = self.get_render_width();
        let new_height = self.get_render_height();
        let base_x = self.view_width - new_width.min(self.view_width) / 2.0;
        let base_y = self.view_height - new_height.min(self.view_height) / 2.0;
        let css = self.css_scale();
        self.set_view_position(
            base_x - (pivot_x - document_x * css),
            base_y - (pivot_y - document_y * css),
        );
    }

    /// `setZoom(factor)` with the default pivot (the view center).
    pub fn set_zoom_center(&mut self, factor: f64) {
        self.set_zoom(factor, self.view_width / 2.0, self.view_height / 2.0);
    }

    /// Zoom In to the next preset (§2.10 "Zoom presets"), anchored at `anchor` (the pointer, in
    /// points, when it lies inside the canvas viewport), else at the viewport centre.
    pub fn zoom_in(&mut self, anchor: Option<(f64, f64)>) {
        let (x, y) = anchor.unwrap_or((self.view_width / 2.0, self.view_height / 2.0));
        self.set_zoom(next_larger_zoom(self.zoom), x, y);
    }

    /// Zoom Out to the previous preset; anchored as `zoom_in`.
    pub fn zoom_out(&mut self, anchor: Option<(f64, f64)>) {
        let (x, y) = anchor.unwrap_or((self.view_width / 2.0, self.view_height / 2.0));
        self.set_zoom(next_smaller_zoom(self.zoom), x, y);
    }

    /// Ctrl+wheel by `notches` (positive = zoom in) about `pivot` (§2.10 "Wheel").
    pub fn wheel_zoom(&mut self, notches: f64, pivot: (f64, f64)) {
        self.set_zoom(wheel_zoom_ratio(self.zoom, notches), pivot.0, pivot.1);
    }

    /// `setViewPosition`: stores the raw value, then models the DOM round trip. The browser
    /// clamps `scrollLeft/Top` to `[0, environment - view]` and its scroll handler feeds the
    /// clamped offset back, except in Fit to Window (AppView.js skips the handler there), where
    /// the raw value stays.
    pub fn set_view_position(&mut self, x: f64, y: f64) {
        self.native_text_scroll = false;
        self.viewport_x = x;
        self.viewport_y = y;
        if !self.zoom_to_window {
            let (env_w, env_h) = self.environment_size();
            self.viewport_x = x.clamp(0.0, (env_w - self.view_width).max(0.0));
            self.viewport_y = y.clamp(0.0, (env_h - self.view_height).max(0.0));
        }
        self.events.push(AppEvent::UpdateViewport);
    }

    /// Text's native keydown visibility uses the full, unclipped viewport in canvas units.
    pub fn text_viewport_bounds(&self) -> Rectangle {
        let b = self.get_render_bounds();
        let scale = self.css_scale();
        Rectangle::new(
            -b.x / scale,
            -b.y / scale,
            self.view_width / scale,
            self.view_height / scale,
        )
    }
    pub fn has_native_text_scroll(&self) -> bool {
        self.native_text_scroll
    }
    /// text-tool-presentation§6: center assignment, native padding/clamp and floor snap.
    pub fn set_text_view_center(&mut self, center: [f64; 2]) {
        let scale = self.css_scale();
        let view = [self.view_width / scale, self.view_height / scale];
        let canvas = [self.get_width(), self.get_height()];
        let origin: [f64; 2] = std::array::from_fn(|i| {
            let pad = if self.zoom_to_window {
                0.0
            } else {
                (view[i] / 2.0).max(view[i] - canvas[i] / 2.0)
            };
            let min = (-pad).min((canvas[i] - view[i]) / 2.0);
            let max = min.max(canvas[i] - view[i] + pad);
            let mut o = (center[i] - view[i] / 2.0).clamp(min, max);
            if self.zoom_to_window || self.zoom.fract() == 0.0 {
                o += ((o * self.zoom).floor() - o * self.zoom) / self.zoom;
            }
            o
        });
        self.viewport_x = self.view_width - self.get_render_width().min(self.view_width) / 2.0
            + origin[0] * scale;
        self.viewport_y = self.view_height - self.get_render_height().min(self.view_height) / 2.0
            + origin[1] * scale;
        self.native_text_scroll = true;
        self.events.push(AppEvent::UpdateViewport);
    }

    /// What `#view.scrollLeft/scrollTop` read back after `AppView.setViewPosition(viewport)`:
    /// clamped to the scroll range, then snapped to whole CSS px. Chromium at DPR 1 rounds half
    /// up (probed: 10.5 -> 11, 10.49 -> 10, 632.5 -> 633; DPR 2 is unverified, see Task 23).
    /// The UI's scroll handler feeds this back through `set_view_position` one frame later,
    /// as AppView's async `scroll` listener does (skipped in Fit to Window).
    pub fn dom_scroll_position(&self) -> (f64, f64) {
        let (env_w, env_h) = self.environment_size();
        let max_x = (env_w - self.view_width).max(0.0);
        let max_y = (env_h - self.view_height).max(0.0);
        (
            crate::js::math_round(self.viewport_x.clamp(0.0, max_x)),
            crate::js::math_round(self.viewport_y.clamp(0.0, max_y)),
        )
    }

    pub fn shift_view_position(&mut self, delta_x: f64, delta_y: f64) {
        self.set_view_position(self.viewport_x + delta_x, self.viewport_y + delta_y);
    }

    pub fn set_zoom_to_window(&mut self, zoom_to_window: bool) {
        if self.zoom_to_window == zoom_to_window {
            return;
        }
        if zoom_to_window {
            self.zoom_to_window = true;
            self.fit_viewport();
        } else {
            // Reapply the fitted scale as an explicit scale.
            self.set_zoom_center(self.zoom);
        }
    }

    /// `painting-tools-1.md §3.13` step 3 (zoom to a rectangle): scale = ⌊10⁴·min(Vw/w, Vh/h)⌋/10⁴
    /// (view in device px) clamped to 1 %…10000 %, the rectangle centred; then once more with w and
    /// h padded by 1.5·hw (hw = 1/scale). Both passes centre the same point, so only the second
    /// scale is applied.
    pub fn zoom_to_rectangle(&mut self, rectangle: &Rectangle) {
        if rectangle.is_empty() {
            return;
        }
        let (vw, vh) = (self.view_width * self.ppp, self.view_height * self.ppp);
        let fit = |w: f64, h: f64| ((1e4 * (vw / w).min(vh / h)).floor() / 1e4).clamp(0.01, 100.0);
        let (w, h) = (rectangle.width, rectangle.height);
        let pad = 1.5 / fit(w, h);
        let scale = fit(w + pad, h + pad);
        self.set_zoom_to_window(false);
        let (b, css) = (self.get_render_bounds(), self.css_scale());
        let x = b.x + (rectangle.left() + w / 2.0) * css;
        let y = b.y + (rectangle.top() + h / 2.0) * css;
        self.set_zoom(scale, x, y);
        self.shift_view_position(x - self.view_width / 2.0, y - self.view_height / 2.0);
    }

    /// `toToolDocumentPosition` through the snapped `view_transform` (what the canvas shows):
    /// `point` (points from the canvas's device-aligned origin) lies in device pixel
    /// d = ⌊point·ppp⌋, whose top-left corner maps to (d − origin)/scale; floored unless
    /// `continuous` (the tool's `usesContinuousPointerCoordinates()`).
    pub fn to_document_position(&self, point: Point, continuous: bool) -> Point {
        let t = self.view_transform();
        let map = |v: f64, origin: f64| {
            let d = (v * self.ppp + POINTER_SLACK).floor();
            let p = (d - origin) / t.scale;
            if continuous { p } else { p.floor() }
        };
        Point::new(map(point.x, t.origin[0]), map(point.y, t.origin[1]))
    }

    /// The point (from the canvas origin) of document point `point` through `view_transform`.
    pub fn to_screen_position(&self, point: Point) -> Point {
        let t = self.view_transform();
        Point::new(
            (t.origin[0] + point.x * t.scale) / self.ppp,
            (t.origin[1] + point.y * t.scale) / self.ppp,
        )
    }

    pub fn get_visible_document_rect(&self) -> Rectangle {
        let bounds = self.get_render_bounds();
        let zoom = self.css_scale();
        let (w, h) = (self.get_width(), self.get_height());
        let left = utility::clamp(-bounds.left() / zoom, 0.0, w);
        let top = utility::clamp(-bounds.top() / zoom, 0.0, h);
        let right = utility::clamp((self.view_width - bounds.left()) / zoom, 0.0, w);
        let bottom = utility::clamp((self.view_height - bounds.top()) / zoom, 0.0, h);
        Rectangle::absolute(left, top, right, bottom)
    }

    pub fn get_render_bounds(&self) -> Rectangle {
        let render_width = self.get_render_width();
        let render_height = self.get_render_height();
        let x = self.view_width - render_width.min(self.view_width) / 2.0 - self.viewport_x;
        let y = self.view_height - render_height.min(self.view_height) / 2.0 - self.viewport_y;
        Rectangle::relative(x, y, render_width, render_height)
    }

    pub fn center_view(&mut self) {
        self.set_view_position(self.view_width / 2.0, self.view_height / 2.0);
    }

    /// `AppView.updateCanvasBounds(shiftView)` with the new `#view` client size: keeps the
    /// document point at the viewport center stable when the view resizes.
    pub fn update_canvas_bounds(&mut self, new_w: f64, new_h: f64, shift_view: bool) {
        // JS: `this.lastViewWidth ?? viewWidth`, so the first call shifts by 0.
        let (previous_w, previous_h) = self.last_view_size.unwrap_or((new_w, new_h));
        self.last_view_size = Some((new_w, new_h));
        self.view_width = new_w;
        self.view_height = new_h;
        if shift_view {
            let render_width = self.get_render_width();
            let render_height = self.get_render_height();
            let old_visible_w = render_width.min(previous_w);
            let old_visible_h = render_height.min(previous_h);
            let new_visible_w = render_width.min(new_w);
            let new_visible_h = render_height.min(new_h);
            self.shift_view_position(
                ((new_w - previous_w) - (new_visible_w - old_visible_w)) / 2.0,
                ((new_h - previous_h) - (new_visible_h - old_visible_h)) / 2.0,
            );
        } else {
            self.set_view_position(self.viewport_x, self.viewport_y);
        }
    }

    /// `AppView.onResize`: shifted resize, then re-fit when Fit to Window is on.
    pub fn on_resize(&mut self, width: f64, height: f64) {
        self.update_canvas_bounds(width, height, true);
        if self.zoom_to_window {
            self.fit_viewport();
        }
        self.events.push(AppEvent::Resize {
            w: width,
            h: height,
        });
    }

    /// `AppView.autoScrollIfNecessary`: `pointer` and `view_bounds` (left, top, right, bottom)
    /// share a coordinate space. Returns true when the view scrolled.
    pub fn auto_scroll(
        &mut self,
        pointer: Point,
        view_bounds: (f64, f64, f64, f64),
        elapsed_seconds: f64,
    ) -> bool {
        if elapsed_seconds <= 0.0 || self.zoom_to_window {
            return false;
        }
        let Some((direction_x, direction_y)) = auto_scroll_direction(pointer, view_bounds) else {
            return false;
        };
        let speed = 2000.0;
        let (env_w, env_h) = self.environment_size();
        let max_scroll_x = (env_w - self.view_width).max(0.0);
        let max_scroll_y = (env_h - self.view_height).max(0.0);
        let delta_x = utility::clamp(
            direction_x * speed * elapsed_seconds,
            -self.viewport_x,
            max_scroll_x - self.viewport_x,
        );
        let delta_y = utility::clamp(
            direction_y * speed * elapsed_seconds,
            -self.viewport_y,
            max_scroll_y - self.viewport_y,
        );
        if delta_x == 0.0 && delta_y == 0.0 {
            return false;
        }
        self.shift_view_position(delta_x, delta_y);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoom_is_device_pixels_per_image_pixel() {
        let mut v = DocumentView::new(800, 600);
        v.on_resize(1000.0, 700.0);
        v.set_pixels_per_point(2.0);
        v.zoom = 1.0;
        assert_eq!(
            (v.css_scale(), v.get_render_width(), v.get_render_height()),
            (0.5, 400.0, 300.0)
        );
        let t = v.view_transform();
        assert_eq!((t.scale, t.viewport), (1.0, [2000, 1400]));
        assert_eq!(t.origin[0], (v.get_render_bounds().x * 2.0).ceil());
    }

    /// ui-drawn-graphics.md §3 "Zoom steps"; painting-tools-1.md §6 Task 31 "Zoom".
    #[test]
    fn zoom_presets_step_like_paint_net() {
        let pct = |r: f64| (r * 100.0).round();
        let mut r = 1.0;
        let ups: Vec<f64> = (0..3)
            .map(|_| {
                r = next_larger_zoom(r);
                pct(r)
            })
            .collect();
        assert_eq!(ups, [150.0, 200.0, 300.0]);
        let mut r = 1.0;
        let downs: Vec<f64> = (0..3)
            .map(|_| {
                r = next_smaller_zoom(r);
                pct(r)
            })
            .collect();
        assert_eq!(downs, [67.0, 50.0, 33.0]);
        assert_eq!(next_larger_zoom(1.25), 1.5);
        assert_eq!(next_smaller_zoom(1.25), 1.0);
        assert_eq!(
            (
                pct(next_larger_zoom(0.4955)),
                pct(next_smaller_zoom(0.4955))
            ),
            (67.0, 33.0)
        );
        assert_eq!(
            (next_larger_zoom(100.0), next_smaller_zoom(0.01)),
            (100.0, 0.01)
        );
        assert_eq!(next_larger_zoom(1.0 / 3.0), 0.5);
        assert_eq!(ZOOM_PRESETS.len(), 45);
    }

    /// §3 "Ctrl+wheel".
    #[test]
    fn ctrl_wheel_multiplies_and_snaps() {
        assert!((wheel_zoom_ratio(1.0, 1.0) - 1.12).abs() < 1e-12);
        assert!((wheel_zoom_ratio(1.0, 2.0) - 1.2544).abs() < 1e-12);
        assert!((wheel_zoom_ratio(1.0, -1.0) - 1.0 / 1.12).abs() < 1e-12);
        assert_eq!(wheel_zoom_ratio(1.8, 1.0), 2.0);
        assert!((wheel_zoom_ratio(1.9, 1.0) - 2.128).abs() < 1e-12);
        assert_eq!(wheel_zoom_ratio(90.0, 10.0), 100.0);
    }

    /// canvas-rendering.md §5 Task 21 "Fit": 9 DIP subtracted once.
    #[test]
    fn fit_subtracts_nine_dips_once() {
        assert!((fit_zoom(1000.0, 700.0, 2000, 1000, 1.0) - 0.4955).abs() < 1e-12);
        assert!((fit_zoom(2000.0, 1400.0, 2000, 1000, 2.0) - 0.991).abs() < 1e-12);
        assert_eq!(fit_zoom(1265.0, 650.0, 800, 600, 1.0), 1.0);
    }

    /// canvas-rendering.md §5 Task 21 "Overscroll": V = 1000, s = 1 — W = 200 → range 1000
    /// (min −900, max 100, centred at −400); W = 3000 → range 3000 (min −500, max 2500).
    #[test]
    fn scroll_extents_equal_paint_net_overscroll() {
        let mut v = DocumentView::new(200, 100);
        v.on_resize(1000.0, 800.0);
        v.set_zoom_center(1.0);
        assert_eq!(v.environment_size().0, 1000.0 + 1000.0);
        v.center_view();
        assert_eq!(v.get_render_bounds().x, 400.0);
        let mut v = DocumentView::new(3000, 100);
        v.on_resize(1000.0, 800.0);
        v.set_zoom_center(1.0);
        assert_eq!(v.environment_size().0, 3000.0 + 1000.0);
    }

    /// ui-drawn-graphics.md §2.10 "Zoom slider" step 2.
    #[test]
    fn zoom_slider_maps_preset_indices() {
        assert_eq!(zoom_slider_index(1.0), 22.0);
        assert!((zoom_slider_index(1.25) - (22.0 + 1.25f64.ln() / 1.5f64.ln())).abs() < 1e-12);
        assert_eq!(
            (
                zoom_from_slider_index(0),
                zoom_from_slider_index(22),
                zoom_from_slider_index(44)
            ),
            (0.01, 1.0, 100.0)
        );
    }

    /// painting-tools-1.md §3.13 step 3: the ratio floors to 10⁻⁴, then a padded second pass.
    #[test]
    fn zoom_to_rectangle_floors_and_pads() {
        let mut v = DocumentView::new(800, 600);
        v.on_resize(1000.0, 700.0);
        v.zoom_to_rectangle(&Rectangle::new(100.0, 100.0, 300.0, 200.0));
        let first = (1e4 * (1000.0f64 / 300.0).min(700.0 / 200.0)).floor() / 1e4;
        let pad = 1.5 / first;
        let second = (1e4 * (1000.0 / (300.0 + pad)).min(700.0 / (200.0 + pad))).floor() / 1e4;
        assert_eq!(v.zoom, second);
    }
}
