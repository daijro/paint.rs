//! The pixel grid canvas layer.
//! Port of: js/src/engine/document/render/renderer/SurfaceBoxGridRenderer.js (its role only; the
//! pixels follow Paint.NET)
//! Reference: paintdotjs @ b4e6fc5

use super::display::DisplayTarget;
use super::view::ViewTransform;
use crate::document::workspace::DocumentWorkspace;
use crate::pixel::IntRect;

pub struct GridRenderer {
    pub visible: bool,
}

impl Default for GridRenderer {
    fn default() -> Self {
        Self { visible: true }
    }
}

/// §2.8 step 1: ½·clamp((s − 2)/4, 0, 1).
pub fn grid_opacity(s: f64) -> f32 {
    (0.5 * ((s - 2.0) / 4.0).clamp(0.0, 1.0)) as f32
}

/// §2.8 step 3: device columns of the vertical lines x = X₀ + i·s for i = −(g_w mod 2) … g_w,
/// X₀ = x0 + g_x·s the device left of G (`x0` = the image origin, `g_x`/`g_w` = G in image px).
/// A 1-px aliased line at x covers column ⌈x⌉ − 1 (pixel-centre rule).
// calibrate: B-C3 — which device column a grid line occupies.
pub fn grid_line_columns(s: f64, x0: f64, g_x: i32, g_w: u32) -> Vec<i32> {
    let left = x0 + g_x as f64 * s;
    (-((g_w % 2) as i64)..=g_w as i64)
        .map(|i| (left + i as f64 * s).ceil() as i32 - 1)
        .collect()
}

impl GridRenderer {
    /// §2.8: 1-px aliased lines through the 16×16 stipple (white where x + y is even, black where
    /// odd, in device coordinates), source-over at `grid_opacity`, clipped to G in device px.
    // calibrate: B-C3 — the stipple phase; each line is its own primitive, so crossings blend twice.
    pub fn render(&self, ws: &DocumentWorkspace, target: &mut DisplayTarget, t: &ViewTransform) {
        self.render_in(ws, target, t, None);
    }

    /// `render` inside just `spans` (one-row rects, none overlapping) ∩ the clip, or the whole
    /// clip (`None`): per pixel, its vertical line, then its horizontal one.
    pub fn render_in(
        &self,
        ws: &DocumentWorkspace,
        target: &mut DisplayTarget,
        t: &ViewTransform,
        spans: Option<&[IntRect]>,
    ) {
        let (s, opacity) = (t.scale, grid_opacity(t.scale));
        if !self.visible || opacity.is_nan() || opacity <= 0.0 {
            return;
        }
        let image = &ws.view.composition;
        // G = the integer bounding rect of (the viewport in image px ∩ the image), per axis.
        let span = |origin: f64, view: u32, n: u32| {
            let lo = ((-origin / s).floor()).clamp(0.0, n as f64) as i32;
            let hi = (((view as f64 - origin) / s).ceil()).clamp(0.0, n as f64) as i32;
            (lo, (hi - lo).max(0) as u32)
        };
        let (gx, gw) = span(t.origin[0], target.width, image.width());
        let (gy, gh) = span(t.origin[1], target.height, image.height());
        if gw == 0 || gh == 0 {
            return;
        }
        // Device pixels whose centre lies in G's device rect, ∩ the target's clip.
        let c = target.clip();
        let clip = |origin: f64, g: i32, n: u32, lo: i32, hi: i32| {
            let (l, r) = (origin + g as f64 * s, origin + (g as f64 + n as f64) * s);
            let pix = |v: f64| ((v - 0.5).ceil()).clamp(lo as f64, hi as f64) as i32;
            pix(l)..pix(r)
        };
        let cols = clip(t.origin[0], gx, gw, c.x, c.right());
        let rows = clip(t.origin[1], gy, gh, c.y, c.bottom());
        let stipple = |x: i32, y: i32| {
            if (x + y).rem_euclid(2) == 0 {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        };
        // Both non-decreasing.
        let (vx, hy) = (
            grid_line_columns(s, t.origin[0], gx, gw),
            grid_line_columns(s, t.origin[1], gy, gh),
        );
        fn within(lines: &[i32], r: std::ops::Range<i32>) -> &[i32] {
            &lines[lines.partition_point(|&v| v < r.start)..lines.partition_point(|&v| v < r.end)]
        }
        let g = IntRect::new(cols.start, rows.start, cols.len() as u32, rows.len() as u32);
        let parts: Vec<IntRect> = match spans {
            None => vec![g],
            Some(spans) => spans.iter().map(|p| p.intersect(&g)).collect(),
        };
        for p in parts.into_iter().filter(|p| !p.is_empty()) {
            let (cols, rows) = (p.x..p.right(), p.y..p.bottom());
            for &x in within(&vx, cols.clone()) {
                for y in rows.clone() {
                    target.over_srgb(x, y, stipple(x, y), opacity);
                }
            }
            for &y in within(&hy, rows.clone()) {
                for x in cols.clone() {
                    target.over_srgb(x, y, stipple(x, y), opacity);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::display::RenderMode;

    /// canvas-rendering.md §5 Task 21 "Grid opacity" and "Grid, HW, 50 % white stipple pixel".
    #[test]
    fn opacity_and_stipple() {
        assert_eq!(
            [2.0, 2.5, 3.0, 4.0, 5.0, 6.0, 8.0].map(grid_opacity),
            [0.0, 0.0625, 0.125, 0.25, 0.375, 0.5, 0.5]
        );
        let mut t = DisplayTarget::new(1, 1, RenderMode::Hardware);
        t.fill(crate::pixel::IntRect::new(0, 0, 1, 1), [0, 0, 0, 255]);
        t.over_srgb(0, 0, [255, 255, 255, 255], 0.5);
        assert_eq!(t.to_rgba8()[0], 188);
        let mut t = DisplayTarget::new(1, 1, RenderMode::Software);
        t.fill(crate::pixel::IntRect::new(0, 0, 1, 1), [0, 0, 0, 255]);
        t.over_srgb(0, 0, [255, 255, 255, 255], 0.5);
        assert_eq!(t.to_rgba8()[0], 128);
    }

    #[test]
    fn lines_every_image_pixel_at_integer_zoom() {
        // s = 4, origin 0: a line at device x = 4i covers column 4i − 1 (pixel-centre rule, B-C3).
        let cols = grid_line_columns(4.0, 0.0, 0, 5);
        assert!(
            [3, 7, 11, 15, 19].iter().all(|c| cols.contains(c)),
            "{cols:?}"
        );
        assert!(!cols.contains(&4));
    }

    /// The renderer draws a line on the last device column of every image pixel, over the
    /// stipple, and nothing at s ≤ 2.
    #[test]
    fn renders_lines_inside_the_image() {
        let mut ws = DocumentWorkspace::new(4, 3, 96.0);
        ws.set_grid_visible(true);
        let grid = GridRenderer::default();
        let draw = |s: f64| {
            let t = ViewTransform {
                scale: s,
                origin: [2.0, 1.0],
                viewport: [30, 20],
                ui_scale: 1.0,
            };
            let mut target = DisplayTarget::new(30, 20, RenderMode::Software);
            target.fill(
                crate::pixel::IntRect::new(0, 0, 30, 20),
                [100, 100, 100, 255],
            );
            grid.render(&ws, &mut target, &t);
            target.to_rgba8()
        };
        assert!(draw(2.0).chunks(4).all(|p| p[0] == 100));
        // s = 4: opacity ¼; columns 5, 9, 13, 17 (x = 2 + 4i − 1), rows 4, 8, 12; column 1 (left of
        // G) is clipped. Black over 100 → 75, white → 139, a crossing blends twice → 56.
        let f = draw(4.0);
        let at = |x: usize, y: usize| f[(y * 30 + x) * 4];
        assert_eq!(
            [at(5, 2), at(5, 3), at(6, 2), at(1, 2)],
            [75, 139, 100, 100]
        );
        assert_eq!(
            [at(17, 3), at(18, 3), at(6, 4), at(5, 4)],
            [139, 100, 139, 56]
        );
    }
}
