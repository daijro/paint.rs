//! The Color Picker.
//! Port of: js/src/engine/document/tool/tool/ColorPickerTool.js (tool structure, cursor, the
//! after-click switch; the sampling follows Paint.NET)
//! Reference: paintdotjs @ b4e6fc5

use super::{CursorRequest, KeyPress, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::pixel::Px;
use crate::render::nubs::PickerPreview;
use crate::render::{Renderer, RendererId};
use crate::util::color::Color;

/// §3.12 steps 3–4: the colour the picker takes at `q` with an `n`×`n` sample of the straight
/// RGBA8 `src` (`w × h`). N = 1 needs q inside the image and takes its pixel; N > 1 needs q inside
/// the image inflated by (N − 1)/2 and averages the N×N square centred on q ∩ the image: each
/// pixel /255 (in double, then f32), premultiplied, summed in f32 row by row, divided by the count
/// in f32, unpremultiplied by 1/a in double (a = 0 → transparent black), channels × 255 rounded
/// half away and clamped.
pub fn average_color(src: &[u8], w: u32, h: u32, q: (i32, i32), n: u32) -> Option<Px> {
    let (w, h) = (w as i64, h as i64);
    let (qx, qy) = (q.0 as i64, q.1 as i64);
    let k = (n.max(1) as i64 - 1) / 2;
    if qx < -k || qy < -k || qx >= w + k || qy >= h + k || src.len() < (w * h * 4) as usize {
        return None;
    }
    let px = |x: i64, y: i64| {
        let i = ((y * w + x) * 4) as usize;
        [src[i], src[i + 1], src[i + 2], src[i + 3]]
    };
    if n <= 1 {
        return Some(px(qx, qy));
    }
    let (x0, x1) = ((qx - k).max(0), (qx + k + 1).min(w));
    let (y0, y1) = ((qy - k).max(0), (qy + k + 1).min(h));
    let mut sum = [0f32; 4];
    for y in y0..y1 {
        for x in x0..x1 {
            let [r, g, b, a] = px(x, y).map(|c| (c as f64 / 255.0) as f32);
            for (s, v) in sum.iter_mut().zip([r * a, g * a, b * a, a]) {
                *s += v;
            }
        }
    }
    let count = ((x1 - x0) * (y1 - y0)) as f32;
    let [r, g, b, a] = sum.map(|s| s / count);
    if a == 0.0 {
        return Some([0; 4]);
    }
    let inv = 1.0 / a as f64;
    let byte = |v: f64| (v * 255.0).round().clamp(0.0, 255.0) as u8;
    Some([
        byte(r as f64 * inv),
        byte(g as f64 * inv),
        byte(b as f64 * inv),
        byte(a as f64),
    ])
}

/// The Color Picker: no transaction and no history (§8.1). Picks on the press and on every move
/// while pressed at ⌊pointer⌋; the first release stops picking and runs the after-click switch.
#[derive(Default)]
pub struct ColorPickerTool {
    /// Physical buttons held since the press, one bit per `MouseButton`.
    held: u8,
    /// Picking: from the first press until a release, or until a commit-first command set the
    /// tool aside (§8.9: picking stops, the after-click switch still runs at the release).
    picking: bool,
    /// A press is in progress (its release runs the after-click switch).
    pressed: bool,
    /// The pointer (document px, floored) last seen.
    last: Option<(i32, i32)>,
    preview: Option<RendererId>,
}

fn bit(b: MouseButton) -> u8 {
    1 << b as u8
}

/// `sampleSize` (1, 3, 5, 11, 31, 51; §4 "Picker sizes"), as a number or its string.
fn sample_size(app: &AppWorkspace) -> u32 {
    let v = app.tool_settings.get(ToolType::ColorPicker, "sampleSize");
    let n = v
        .as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(1.0);
    if n.is_finite() {
        n.clamp(1.0, 51.0) as u32
    } else {
        1
    }
}

impl ColorPickerTool {
    /// §3.12 steps 1–4 with `b`'s colour (left → Primary, right → Secondary); the source is the
    /// image composite with "Image" sampling or Ctrl, else the active layer.
    fn pick(&mut self, app: &mut AppWorkspace, b: MouseButton) {
        if !matches!(b, MouseButton::Left | MouseButton::Right) {
            return;
        }
        let (Some(q), Some(ws)) = (self.last, app.active_ws()) else {
            return;
        };
        let image = app
            .tool_settings
            .get(ToolType::ColorPicker, "sampleMode")
            .as_str()
            == Some("image")
            || app.control_key_down;
        // The workspace keeps the composite current synchronously (`DocumentWorkspace::invalidate`).
        let surface = if image {
            &ws.view.composition
        } else {
            match ws.document.layers.get_at(ws.active_layer) {
                Some(l) => &l.surface,
                None => return,
            }
        };
        let picked = average_color(
            surface.data(),
            surface.width(),
            surface.height(),
            q,
            sample_size(app),
        );
        if let Some([r, g, bl, a]) = picked {
            let c = Color::new(r, g, bl, a);
            match b {
                MouseButton::Right => app.set_colors(app.primary_color, c),
                _ => app.set_colors(c, app.secondary_color),
            }
        }
    }

    /// §8.7 item 5: moves pick with Left taking priority among the held buttons.
    fn repick(&mut self, app: &mut AppWorkspace) {
        if !self.picking {
            return;
        }
        let b = if self.held & bit(MouseButton::Left) != 0 {
            MouseButton::Left
        } else if self.held & bit(MouseButton::Right) != 0 {
            MouseButton::Right
        } else {
            return;
        };
        self.pick(app, b);
    }

    /// The sample square follows the pointer while it is over the canvas, or a press holds it.
    fn sync_preview(&self, app: &mut AppWorkspace) {
        let shown = self.pressed || app.zoom_anchor.is_some();
        let p = PickerPreview {
            cell: self.last.map_or([0, 0], |(x, y)| [x, y]),
            size: sample_size(app),
            visible: shown && self.last.is_some(),
        };
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        let Some(id) = self.preview else {
            return;
        };
        if matches!(ws.surface_box.renderer(id), Some(Renderer::PickerPreview(c)) if *c == p) {
            return;
        }
        if let Some(Renderer::PickerPreview(r)) = ws.surface_box.renderer_mut(id) {
            *r = p;
        }
    }

    fn track(&mut self, x: f64, y: f64) {
        if x.is_finite() && y.is_finite() {
            self.last = Some((x.floor() as i32, y.floor() as i32));
        }
    }
}

impl Tool for ColorPickerTool {
    fn tool_type(&self) -> ToolType {
        ToolType::ColorPicker
    }

    fn on_activate(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            let hidden = PickerPreview {
                cell: [0, 0],
                size: 1,
                visible: false,
            };
            self.preview = Some(ws.surface_box.add_renderer(Renderer::PickerPreview(hidden)));
        }
    }

    /// §8.9: a commit-first command (the tool set aside) stops the picking; the release still
    /// runs the after-click switch.
    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        self.picking = false;
        if let (Some(id), Some(ws)) = (self.preview.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
    }

    /// §8.7 item 5: the first press starts picking; a second press picks at once with its own
    /// button.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if app.active_ws().is_none() {
            return false;
        }
        self.track(x, y);
        if !self.pressed {
            (self.pressed, self.picking, self.held) = (true, true, 0);
        }
        self.held |= bit(b);
        if self.picking {
            self.pick(app, b);
        }
        self.sync_preview(app);
        true
    }

    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        self.track(x, y);
        self.repick(app);
        self.sync_preview(app);
        self.pressed
    }

    /// §3.12 step 5: the first release of any button stops picking and switches per `afterClick`
    /// (even when nothing was picked).
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !std::mem::take(&mut self.pressed) {
            return false;
        }
        (self.picking, self.held) = (false, 0);
        self.sync_preview(app);
        let after = app
            .tool_settings
            .get(ToolType::ColorPicker, "afterClick")
            .as_str()
            .unwrap_or("none")
            .to_string();
        match after.as_str() {
            "pencil" => app.set_active_tool_from_type(ToolType::Pencil),
            "previous" => {
                if let Some(t) = app.previous_tool.filter(|&t| t != ToolType::ColorPicker) {
                    app.set_active_tool_from_type(t);
                }
            }
            _ => {}
        }
        true
    }

    /// §8.4: every key event first re-picks at the last position with the held button; the key
    /// then acts (Escape and Enter do nothing while pressed: the generic Deselect waits for the
    /// buttons).
    fn on_key_press(&mut self, app: &mut AppWorkspace, _key: &KeyPress) -> bool {
        self.repick(app);
        false
    }

    /// Ctrl switches the source to the image: a key event, so it re-picks.
    fn on_modifier_keys_changed(&mut self, app: &mut AppWorkspace) {
        self.repick(app);
    }

    fn on_setting_changed(&mut self, app: &mut AppWorkspace, _key: &str) {
        self.sync_preview(app);
    }

    /// The square hides once the pointer leaves the canvas.
    fn on_frame(&mut self, app: &mut AppWorkspace) {
        self.sync_preview(app);
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image("color_picker_tool_sample_layer_cursor")
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
