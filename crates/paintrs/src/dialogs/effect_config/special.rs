use super::color_popup::{color_popup_activated, levels_popup_rgb};
use super::layouts::pan_fit;
use super::widgets::{
    RangeSpec, SelectLook, SliderState, Track, canvas_image, device_box, paint_canvas, range_line,
    select,
};
use super::{EffectConfigDialog, merge, num, set, set_triple, sync_curves, triple};
use crate::dialogs::curve_editor::{self, CurveColors};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::text::{self, FontSpec};
use crate::kit::{aa, px};
use crate::theme::{self, *};
use egui::{Color32, PointerButton, Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::canvas::{Canvas, ImageData, Style};
use pdn_core::effect::{Control, DialogLayout, Values, levels_ui};
use serde_json::json;

impl EffectConfigDialog {
    /// `createSpecializedSlider` rows (Hue / Saturation and Rotate / Zoom): the optional 600-weight
    /// label, then the slider line at `line_x` with `gap` between its columns.
    #[allow(clippy::too_many_arguments)]
    fn special_slider(
        &mut self,
        ui: &mut Ui,
        state: &mut SliderState,
        c: &Control,
        top: f32,
        x: f32,
        line_x: f32,
        right: f32,
        label_gap: Option<f32>,
        gap: f32,
        track: Option<&'static [u32]>,
    ) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let (key, label, rs) = RangeSpec::of(c, self.session.as_ref().map_or("", |s| s.def.id))?;
        let spec = FontSpec::new(DIALOG_FONT_SIZE).weight(600);
        let mut y = top;
        if let Some(g) = label_gap {
            text::draw(
                ui.painter(),
                Pos2::new(x, y),
                label,
                spec,
                theme.menu_item_text,
            );
            y += spec.resolved_line_height(ppp) + g;
        }
        let (nw, rw) = (EFFECT_SPECIAL_NUMBER, EFFECT_LINE_COLUMNS.1);
        let col = right - line_x - nw - rw - 2.0 * gap;
        let range = Rect::from_min_size(
            Pos2::new(line_x + EFFECT_RANGE_MARGIN, y),
            Vec2::new(col, EFFECT_LINE_HEIGHT),
        );
        let number = Rect::from_min_size(
            Pos2::new(
                line_x + col + gap,
                y + (EFFECT_LINE_HEIGHT - EFFECT_NUMBER_HEIGHT) / 2.0,
            ),
            Vec2::new(nw, EFFECT_NUMBER_HEIGHT),
        );
        let reset = Rect::from_min_size(
            Pos2::new(number.max.x + gap, y + (EFFECT_LINE_HEIGHT - rw) / 2.0),
            Vec2::splat(rw),
        );
        let mut style = NumberStyle::EFFECT_SPECIAL.decimals(if rs.step < 1.0 { 2 } else { 0 });
        style.wrap_endpoint = self.layout() == Some(DialogLayout::HueSaturation) && key == "hue";
        range_line(
            ui,
            state,
            key,
            rs,
            &mut self.values,
            range,
            number,
            Some(reset),
            Track::Special(track),
            style,
        )
    }

    pub(super) fn find(controls: &[Control], key: &str) -> Option<Control> {
        controls.iter().find(|c| c.key() == key).cloned()
    }

    /// `createHueSaturationContent`.
    pub(super) fn hue_saturation(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        controls: &[Control],
        now: f64,
    ) -> Option<bool> {
        let ppp = ui.ctx().pixels_per_point();
        let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(ppp);
        let (t, x, _) = EFFECT_HUE_PADDING;
        let (row_gap, margin) = EFFECT_HUE_GAP;
        let left = r.min.x + x;
        let right = r.max.x - x;
        let mut y = r.min.y + t;
        let mut state = self.slider_state(now);
        let mut changed = None;
        let tracks: [&'static [u32]; 3] = [
            &EFFECT_HUE_TRACK,
            &EFFECT_SATURATION_TRACK,
            &EFFECT_LIGHTNESS_TRACK,
        ];
        for (key, track) in ["hue", "saturation", "lightness"].into_iter().zip(tracks) {
            if let Some(c) = Self::find(controls, key) {
                let ch = self.special_slider(
                    ui,
                    &mut state,
                    &c,
                    y,
                    left,
                    left + margin,
                    right,
                    Some(EFFECT_SPECIAL_GAP.0),
                    EFFECT_SPECIAL_LINE_GAP.0,
                    Some(track),
                );
                changed = merge(changed, ch);
            }
            y += line + EFFECT_SPECIAL_GAP.0 + EFFECT_LINE_HEIGHT + row_gap;
        }
        self.put_slider_state(state);
        changed
    }

    /// `createRotateZoomContent`: the Roll / Rotate and Pan groups, Zoom, Quality, Tiling and
    /// Sampling.
    pub(super) fn rotate_zoom(
        &mut self,
        ui: &mut Ui,
        r: Rect,
        controls: &[Control],
        now: f64,
    ) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let h3 = FontSpec::new(DIALOG_FONT_SIZE).weight(600);
        let line = h3.resolved_line_height(ppp);
        let (t, x, _) = EFFECT_ROTATE_PADDING;
        let (gap, section_gap, pan_margin, body_gap, slider_gap) = EFFECT_ROTATE_GAPS;
        let left = r.min.x + x;
        let right = r.max.x - x;
        let mut y = r.min.y + t;
        let mut state = self.slider_state(now);
        let mut changed = None;
        let line_gap = EFFECT_SPECIAL_LINE_GAP.1;
        let sliders_x = left + EFFECT_ROTATE_COLUMN + body_gap;
        // Roll / Rotate.
        text::draw(
            ui.painter(),
            Pos2::new(left, y),
            "Roll / Rotate",
            h3,
            theme.menu_item_text,
        );
        y += line + section_gap;
        let body_h = 3.0 * EFFECT_LINE_HEIGHT + 2.0 * slider_gap;
        let roll = Rect::from_min_size(
            Pos2::new(left, y + (body_h - EFFECT_ROLL_SIZE) / 2.0),
            Vec2::splat(EFFECT_ROLL_SIZE),
        );
        changed = merge(changed, self.roll_visual(ui, roll));
        for (i, key) in ["angle", "rollDirection", "rollAmount"]
            .into_iter()
            .enumerate()
        {
            if let Some(c) = Self::find(controls, key) {
                let top = y + i as f32 * (EFFECT_LINE_HEIGHT + slider_gap);
                let ch = self.special_slider(
                    ui, &mut state, &c, top, sliders_x, sliders_x, right, None, line_gap, None,
                );
                changed = merge(changed, ch);
            }
        }
        y += body_h + gap;
        // Pan.
        text::draw(
            ui.painter(),
            Pos2::new(left, y),
            "Pan",
            h3,
            theme.menu_item_text,
        );
        y += line + section_gap;
        let pan_box = Vec2::new(EFFECT_PAN_SIZE.0 + 2.0, EFFECT_PAN_SIZE.1 + 2.0);
        let pan = Rect::from_min_size(Pos2::new(left, y), pan_box);
        changed = merge(changed, self.pan_visual(ui, pan));
        let sliders_h = 2.0 * EFFECT_LINE_HEIGHT + slider_gap;
        for (i, key) in ["panX", "panY"].into_iter().enumerate() {
            if let Some(c) = Self::find(controls, key) {
                let top = y
                    + (pan_box.y - sliders_h) / 2.0
                    + i as f32 * (EFFECT_LINE_HEIGHT + slider_gap);
                let ch = self.special_slider(
                    ui, &mut state, &c, top, sliders_x, sliders_x, right, None, line_gap, None,
                );
                changed = merge(changed, ch);
            }
        }
        y += pan_box.y + pan_margin + gap;
        // Zoom and Quality.
        for key in ["zoom", "quality"] {
            if let Some(c) = Self::find(controls, key) {
                let ch = self.special_slider(
                    ui,
                    &mut state,
                    &c,
                    y,
                    left,
                    left,
                    right,
                    Some(EFFECT_SPECIAL_GAP.1),
                    line_gap,
                    None,
                );
                changed = merge(changed, ch);
            }
            y += line + EFFECT_SPECIAL_GAP.1 + EFFECT_LINE_HEIGHT + gap;
        }
        self.put_slider_state(state);
        // Tiling and Sampling.
        let (sgap, sh, pad_l, pad_r, min_w) = EFFECT_SPECIAL_SELECT;
        for (key, label) in [("tiling", "Tiling Mode:"), ("sampling", "Sampling:")] {
            let Some(Control::Select { choices, .. }) = Self::find(controls, key) else {
                continue;
            };
            let lw = text::measure(ui.painter(), label, h3);
            text::draw(
                ui.painter(),
                Pos2::new(left, y + (sh - line) / 2.0),
                label,
                h3,
                theme.menu_item_text,
            );
            let widest = choices
                .iter()
                .map(|c| text::measure(ui.painter(), c.1, h3))
                .fold(0.0f32, f32::max);
            let w = (widest.ceil() + pad_l + pad_r + 2.0 + SELECT_INNER_PADDING).max(min_w);
            let at = Rect::from_min_size(Pos2::new(left + lw + sgap, y), Vec2::new(w, sh));
            let labels: Vec<&str> = choices.iter().map(|c| c.1).collect();
            let current = choices
                .iter()
                .position(|c| c.0 == pdn_core::effect::choice(&self.values, key))
                .unwrap_or(0);
            let look = SelectLook {
                native: false,
                pad_left: pad_l,
                font: h3,
            };
            if let Some(i) = select(
                ui,
                &mut self.open_select,
                ui.scope_id().with(("select", key)),
                at,
                label,
                &labels,
                current,
                look,
            ) {
                self.values.insert(key.into(), json!(choices[i].0));
                changed = Some(true);
            }
            y += sh + gap;
        }
        changed
    }

    /// `createRollVisual`: the 98×98 sphere; a drag inside the sphere rolls, outside it turns;
    /// Shift snaps; a double-click resets.
    fn roll_visual(&mut self, ui: &mut Ui, rect: Rect) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let (r, w, h) = device_box(rect, ppp);
        let id = ui.scope_id().with("roll");
        let response = ui.interact(r, id, Sense::click_and_drag());
        let (center, outer, sphere) = (49.0, 46.0, 31.0);
        let mut changed = None;
        let local = |p: Pos2| ((p.x - rect.min.x) as f64, (p.y - rect.min.y) as f64);
        let shift = ui.input(|i| i.modifiers.shift);
        if response.double_clicked() {
            for k in ["angle", "rollDirection", "rollAmount"] {
                set(&mut self.values, k, 0.0);
            }
            changed = Some(true);
        } else if let Some(p) = response.interact_pointer_pos() {
            let (x, y) = local(p);
            if response.drag_started()
                || (response.is_pointer_button_down_on() && self.roll_drag.is_none())
            {
                let amount = num(&self.values, "rollAmount") / 89.9;
                let dir = num(&self.values, "rollDirection").to_radians();
                let on_sphere = ((x - center).powi(2) + (y - center).powi(2)).sqrt() <= sphere;
                self.roll_drag = Some((p, (amount * dir.cos(), amount * dir.sin()), on_sphere));
            }
            if let Some((start, (sx, sy), on_sphere)) = self.roll_drag
                && response.is_pointer_button_down_on()
            {
                if on_sphere {
                    let k = 3.0 / (outer * 2.0 - 4.0);
                    let mut nx = sx + (p.x - start.x) as f64 * k;
                    let mut ny = sy + (p.y - start.y) as f64 * k;
                    let len = nx.hypot(ny);
                    if len > 1.0 {
                        nx /= len;
                        ny /= len;
                    }
                    if shift {
                        if nx.abs() > ny.abs() {
                            ny = 0.0;
                        } else {
                            nx = 0.0;
                        }
                    }
                    set(&mut self.values, "rollDirection", ny.atan2(nx).to_degrees());
                    set(&mut self.values, "rollAmount", 89.94 * nx.hypot(ny));
                } else {
                    let mut a = (-(y - center)).atan2(x - center).to_degrees();
                    if shift {
                        a = (a / 15.0).round() * 15.0;
                    }
                    set(&mut self.values, "angle", a);
                }
                changed = Some(false);
            }
        }
        if !response.is_pointer_button_down_on() {
            self.roll_drag = None;
        }
        let v = &self.values;
        let key = ["angle", "rollDirection", "rollAmount"].map(|k| num(v, k).to_bits());
        if self.roll_image.as_ref().is_none_or(|(k, _)| *k != key) {
            let cx = draw_roll(
                num(v, "angle"),
                num(v, "rollDirection"),
                num(v, "rollAmount"),
                theme.curve_grid,
            );
            self.roll_image = Some((key, canvas_image(&cx, w, h)));
        }
        if let Some((_, image)) = &self.roll_image {
            paint_canvas(ui, &mut self.canvases, id, r, image.clone());
        }
        changed
    }

    /// `createPanVisual`: the 100×76 thumbnail (Paint.NET-computed pixels, R21 (c)); a drag pans.
    fn pan_visual(&mut self, ui: &mut Ui, rect: Rect) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let painter = ui.painter().clone();
        let outer = px::snap_rect(rect, ppp);
        painter.rect_filled(outer, 0.0, theme.pdn_control_border);
        let inner = rect.shrink(1.0);
        let (r, w, h) = device_box(inner, ppp);
        let id = ui.scope_id().with("pan");
        let response = ui.interact(r, id, Sense::CLICK | Sense::DRAG);
        let (cw, ch) = (EFFECT_PAN_SIZE.0 as f64, EFFECT_PAN_SIZE.1 as f64);
        let (fit_w, fit_h) = match &self.session {
            Some(s) => pan_fit(s.source().width, s.source().height),
            None => (90.0, 66.0),
        };
        let mut changed = None;
        if let Some(p) = response.interact_pointer_pos()
            && response.is_pointer_button_down_on()
        {
            let (x, y) = ((p.x - inner.min.x) as f64, (p.y - inner.min.y) as f64);
            set(
                &mut self.values,
                "panX",
                ((x - cw / 2.0) / (fit_w / 2.0)).clamp(-10.0, 10.0),
            );
            set(
                &mut self.values,
                "panY",
                ((y - ch / 2.0) / (fit_h / 2.0)).clamp(-10.0, 10.0),
            );
            changed = Some(false);
        }
        if response.drag_stopped() {
            changed = Some(true);
        }
        if self.pan_thumb.is_none() {
            self.pan_thumb = self
                .session
                .as_ref()
                .map(|s| pan_thumbnail(s.source(), fit_w, fit_h));
        }
        let cx = draw_pan(
            self.pan_thumb.as_ref(),
            num(&self.values, "panX"),
            num(&self.values, "panY"),
            (fit_w, fit_h),
        );
        paint_canvas(ui, &mut self.canvases, id, r, canvas_image(&cx, w, h));
        text::record_pdn_rect(ui.ctx(), outer);
        changed
    }

    /// `createCurvesContent`: the Transfer Map select and coordinates, the editor, the channel
    /// checkboxes and the tip.
    pub(super) fn curves_content(&mut self, ui: &mut Ui, r: Rect) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let (t, x, _) = EFFECT_CURVES_PADDING;
        let (gap, select_gap) = EFFECT_CURVES_GAPS;
        let left = r.min.x + x;
        let right = r.max.x - x;
        let mut y = r.min.y + t;
        let mut changed = None;
        let rgb = pdn_core::effect::choice(&self.values, "channel") == "rgb";
        // Transfer Map.
        text::draw(
            ui.painter(),
            Pos2::new(left, y),
            "Transfer Map",
            spec,
            theme.menu_item_text,
        );
        let (sw, sh, pad_l, _) = EFFECT_CURVES_SELECT;
        let at = Rect::from_min_size(Pos2::new(left, y + line + select_gap), Vec2::new(sw, sh));
        let look = SelectLook {
            native: true,
            pad_left: pad_l,
            font: spec,
        };
        if let Some(i) = select(
            ui,
            &mut self.open_select,
            ui.scope_id().with("channel"),
            at,
            "Transfer Map",
            &["Luminosity", "RGB"],
            rgb as usize,
            look,
        ) {
            self.values
                .insert("channel".into(), json!(["luminosity", "rgb"][i]));
            changed = Some(true);
        }
        let row_bottom = at.max.y;
        y = row_bottom + gap;
        // The editor: the canvas content box inside its 1-px border.
        let (cw, ch, margin) = EFFECT_CURVE_CANVAS;
        let border = Rect::from_min_size(Pos2::new(left + margin, y), Vec2::new(cw, ch));
        let b = px::snap_rect(border, ppp);
        ui.painter().rect_filled(b, 0.0, theme.pdn_control_border);
        let content = border.shrink(1.0);
        let (cr, dw, dh) = device_box(content, ppp);
        let id = ui.scope_id().with("curve");
        let response = ui.interact(cr, id, Sense::click_and_drag());
        ui.ctx()
            .accesskit_node_builder(id, |n| n.set_label("Curve editor"));
        let Some(state) = &mut self.curves else {
            return changed;
        };
        let rgb = pdn_core::effect::choice(&self.values, "channel") == "rgb";
        let ed = if rgb {
            &mut state.rgb
        } else {
            &mut state.luminosity
        };
        let units = |p: Pos2| {
            curve_editor::to_curve_units(
                p.x - content.min.x,
                p.y - content.min.y,
                content.width(),
                content.height(),
            )
        };
        let hover = ui.input(|i| i.pointer.hover_pos());
        let pressed = ui.input(|i| {
            [PointerButton::Primary, PointerButton::Secondary]
                .into_iter()
                .find(|b| i.pointer.button_pressed(*b))
        });
        let mut edited = false;
        if let (Some(button), Some(p)) = (pressed, hover)
            && response.hovered()
        {
            edited |= ed.press(units(p), button == PointerButton::Primary);
            // A press on the canvas keeps the keyboard focus where it was.
            if let Some(f) = ui.ctx().memory(|m| m.focused()) {
                ui.ctx().memory_mut(|m| m.request_focus(f));
            }
        }
        if ed.dragging() {
            if ui.input(|i| i.pointer.primary_down()) {
                if let Some(p) = hover {
                    edited |= ed.drag(units(p));
                }
            } else {
                ed.release();
                changed = Some(true);
            }
        }
        ed.hover = if ed.dragging() || hover.is_some_and(|p| cr.contains(p)) {
            hover.map(units)
        } else {
            None
        };
        let hovered_units = ed.hover;
        if edited {
            changed = merge(changed, Some(false));
        }
        let colors: Vec<[u8; 3]> = if rgb {
            curve_editor::RGB.to_vec()
        } else {
            vec![curve_editor::LUMINOSITY]
        };
        let c3 = |c: Color32| [c.r(), c.g(), c.b()];
        let canvas = curve_editor::draw(
            ed,
            &colors,
            &CurveColors {
                background: c3(theme.curve_background),
                grid: c3(theme.curve_grid),
            },
            cw as u32,
            ch as u32,
        );
        let channels: Vec<bool> = ed.checked.clone();
        let image = canvas_image(&canvas, dw, dh);
        if let Some(s) = &self.curves {
            sync_curves(s, &mut self.values);
        }
        paint_canvas(ui, &mut self.canvases, id, cr, image);
        // The coordinates label, right-aligned above the editor.
        if let Some((mx, my)) = hovered_units {
            let s = format!("({mx}, {my})");
            let (_, pad_x, pad_b) = EFFECT_CURVES_COORDS;
            let w = text::css_width(&s, spec.size);
            text::draw(
                ui.painter(),
                Pos2::new(right - pad_x - w, row_bottom - pad_b - line),
                &s,
                spec,
                theme.menu_item_text,
            );
        }
        y += ch + gap;
        // The channel checkboxes: one 1fr column each.
        let names: &[&str] = if rgb {
            &["Red", "Green", "Blue"]
        } else {
            &["Luminosity"]
        };
        let col_w = (right - left) / names.len() as f32;
        let (my, mr, ml) = CHECKBOX_MARGIN;
        let disabled = names.len() == 1;
        for (i, name) in names.iter().enumerate() {
            let x0 = left + i as f32 * col_w;
            let boxed = Rect::from_min_size(Pos2::new(x0 + ml, y + my), Vec2::splat(CHECKBOX_SIZE));
            let mut on = channels.get(i).copied().unwrap_or(true);
            let before = on;
            inputs::checkbox(
                ui,
                ("channel", i),
                boxed,
                &mut on,
                !disabled,
                theme.window_background,
            );
            let color = if disabled {
                theme.subtle_text
            } else {
                theme.menu_item_text
            };
            let row_h = CHECKBOX_SIZE + 2.0 * my;
            text::draw(
                ui.painter(),
                Pos2::new(
                    boxed.max.x + mr + EFFECT_CHANNEL_GAP,
                    y + (row_h - line) / 2.0,
                ),
                name,
                spec,
                color,
            );
            if on != before
                && let Some(state) = &mut self.curves
            {
                let ed = if rgb {
                    &mut state.rgb
                } else {
                    &mut state.luminosity
                };
                ed.checked[i] = on;
            }
        }
        y += CHECKBOX_SIZE + 2.0 * my + gap;
        // The tip.
        text::draw(
            ui.painter(),
            Pos2::new(left, y + EFFECT_TIP_MARGIN.0),
            "Tip: Right-click to remove control points.",
            spec.bold(),
            theme.menu_item_text,
        );
        changed
    }

    /// `createLevelsContent` with Paint.NET's value rules (`levels_ui`).
    pub(super) fn levels_content(&mut self, ui: &mut Ui, r: Rect) -> Option<bool> {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let h3 = FontSpec::new(DIALOG_FONT_SIZE).weight(600);
        let line = h3.resolved_line_height(ppp);
        let (t, x, _) = EFFECT_LEVELS_PADDING;
        let left = r.min.x + x;
        let right = r.max.x - x;
        let top = r.min.y + t;
        let col = EFFECT_LEVELS_COLUMN;
        let gap = EFFECT_CONTENT_GAP;
        let hist_w = (right - left - 2.0 * col - 3.0 * gap) / 2.0;
        let xs = [
            left,
            left + hist_w + gap,
            left + hist_w + col + 2.0 * gap,
            left + hist_w + 2.0 * col + 3.0 * gap,
        ];
        let checked = ["red", "green", "blue"].map(|k| pdn_core::effect::flag(&self.values, k));
        let mut changed = None;
        // The headings with their `::after` lines, each cut at the next column.
        let titles = ["Input Histogram", "Input", "Output", "Output Histogram"];
        let (line_gap, line_dy) = EFFECT_H3_LINE;
        for (i, title) in titles.iter().enumerate() {
            let w = text::measure(ui.painter(), title, h3);
            text::draw(
                ui.painter(),
                Pos2::new(xs[i], top),
                title,
                h3,
                theme.menu_item_text,
            );
            let end = xs.get(i + 1).copied().unwrap_or(right);
            let a = Pos2::new(xs[i] + w + line_gap, top + line_dy);
            let s = px::snap_rect(Rect::from_min_max(a, Pos2::new(end, a.y + 1.0)), ppp);
            ui.painter().rect_filled(s, 0.0, theme.pdn_separator);
        }
        // The histograms (Paint.NET's statistics over the selection: R21 (c)).
        let hist_rect = |x: f32| {
            Rect::from_min_size(
                Pos2::new(x, top + line),
                Vec2::new(hist_w, EFFECT_LEVELS_HISTOGRAM),
            )
        };
        if let Some(h) = self.histogram.clone() {
            let params = pdn_core::effect::adjustments::LevelsParams::from_values(&self.values);
            for (i, output) in [(0usize, false), (3, true)] {
                let rect = hist_rect(xs[i]);
                let (dr, w, hh) = device_box(rect, ppp);
                let cx = draw_histogram(&h, &params, output, checked, w, hh);
                paint_canvas(
                    ui,
                    &mut self.canvases,
                    ui.scope_id().with(("hist", i)),
                    dr,
                    canvas_image(&cx, w, hh),
                );
                // The heading's text box reaches the canvas's first row; that row is compared.
                let pdn = Rect::from_min_max(Pos2::new(dr.min.x, dr.min.y + 1.0 / ppp), dr.max);
                text::record_pdn_rect(ui.ctx(), pdn);
            }
        }
        // The columns.
        let (nw, high_top, low_top, gamma_top) = EFFECT_LEVELS_NUMBER;
        let (sh, white_top, mid_top, black_top, sb) = EFFECT_LEVELS_SWATCH;
        let (track_top, track_w, track_h) = EFFECT_LEVELS_TRACK;
        let style = NumberStyle::EFFECT_SPECIAL;
        let swatch = |ui: &Ui, at: Pos2, fill: Color32| {
            let r = px::snap_rect(Rect::from_min_size(at, Vec2::new(nw, sh)), ppp);
            ui.painter().rect_filled(r, 0.0, theme.pdn_control_border);
            ui.painter()
                .rect_filled(r.shrink(px::border_width(sb, ppp)), 0.0, fill);
        };
        let mut triangles: Vec<([Pos2; 3], Color32)> = vec![];
        let mut tracks: Vec<(Rect, Vec<Vec<Color32>>)> = vec![];
        for output in [false, true] {
            let cx0 = if output { xs[2] } else { xs[1] };
            let num_x = if output { cx0 + col - nw } else { cx0 };
            let (lo_key, hi_key) = if output {
                ("outputLow", "outputHigh")
            } else {
                ("inputLow", "inputHigh")
            };
            for (key, ytop) in [(hi_key, high_top), (lo_key, low_top)] {
                let t3 = triple(&self.values, key).map(|v| v / 255.0);
                let shown = (levels_ui::displayed(t3, checked) * 255.0).round() as f64;
                let mut v = shown;
                let at = Rect::from_min_size(
                    Pos2::new(num_x, top + ytop),
                    Vec2::new(nw, EFFECT_NUMBER_HEIGHT),
                );
                inputs::number_input(
                    ui,
                    ("levels", key),
                    key,
                    at,
                    &mut v,
                    0.0,
                    255.0,
                    1.0,
                    true,
                    style,
                );
                if v != shown {
                    let next = levels_ui::set_by_mask(t3, checked, v.round() as f32 / 255.0);
                    set_triple(&mut self.values, key, next.map(|c| c * 255.0));
                    changed = merge(changed, Some(false));
                }
            }
            for (key, y, fill) in [
                (hi_key, white_top, Color32::WHITE),
                (lo_key, black_top, Color32::BLACK),
            ] {
                let at = Pos2::new(num_x, top + y);
                swatch(ui, at, fill);
                let response = ui.interact(
                    px::snap_rect(Rect::from_min_size(at, Vec2::new(nw, sh)), ppp),
                    ui.scope_id().with(("levels-swatch", key)),
                    Sense::click(),
                );
                ui.ctx().accesskit_node_builder(response.id, |n| {
                    n.set_role(egui::accesskit::Role::Button);
                    n.set_label(format!("{key} color"));
                });
                if response.double_clicked()
                    || (response.has_focus()
                        && !ui.input(|i| i.pointer.any_pressed() || i.pointer.any_released())
                        && color_popup_activated(ui, &response))
                {
                    let p = pdn_core::effect::adjustments::LevelsParams::from_values(&self.values);
                    let normalized = match key {
                        "inputLow" => p.lo,
                        "inputHigh" => p.hi,
                        "outputLow" => p.out_lo,
                        _ => p.out_hi,
                    };
                    self.open_color_popup(
                        key,
                        levels_popup_rgb(normalized),
                        false,
                        true,
                        &response,
                    );
                }
            }
            if output {
                let g = triple(&self.values, "gamma");
                let shown = levels_ui::displayed(g, checked) as f64;
                let mut v = (shown * 100.0).round() / 100.0;
                let before = v;
                let at = Rect::from_min_size(
                    Pos2::new(num_x, top + gamma_top),
                    Vec2::new(nw, EFFECT_NUMBER_HEIGHT),
                );
                inputs::number_input(
                    ui,
                    "gamma",
                    "gamma",
                    at,
                    &mut v,
                    0.1,
                    10.0,
                    0.1,
                    true,
                    style.decimals(2),
                );
                if v != before {
                    let next = levels_ui::set_gamma_by_mask(g, checked, v as f32);
                    set_triple(&mut self.values, "gamma", next);
                    changed = merge(changed, Some(false));
                }
                swatch(
                    ui,
                    Pos2::new(num_x, top + mid_top),
                    EFFECT_LEVELS_MID_SWATCH,
                );
            }
            // The gradient track and its handles.
            let tx = if output { cx0 } else { cx0 + col - track_w };
            let track =
                Rect::from_min_size(Pos2::new(tx, top + track_top), Vec2::new(track_w, track_h));
            let tr = px::snap_rect(track, ppp);
            let pixels = inputs::gradient_pixels(tr, EFFECT_LEVELS_TRACK_COLORS, ppp);
            inputs::paint_pixels(ui.painter(), tr.min, &pixels);
            let disp = |values: &Values, key: &str| {
                (levels_ui::displayed(triple(values, key).map(|v| v / 255.0), checked) * 255.0)
                    .round() as i32
            };
            let (lo, hi) = (disp(&self.values, lo_key), disp(&self.values, hi_key));
            let handles: Vec<i32> = if output {
                let g = levels_ui::displayed(triple(&self.values, "gamma"), checked);
                vec![lo, levels_ui::mid_handle(lo, hi, g), hi]
            } else {
                vec![lo, hi]
            };
            let id = ui.scope_id().with(("levels-track", output));
            let response = ui.interact(tr, id, Sense::click_and_drag());
            let value_at = |y: f32| {
                ((track.max.y - y) / track.height() * 255.0)
                    .round()
                    .clamp(0.0, 255.0) as i32
            };
            if let Some(p) = response.interact_pointer_pos() {
                let v = value_at(p.y);
                if ui.input(|i| i.pointer.primary_pressed()) {
                    let which = (0..handles.len())
                        .min_by_key(|&k| (handles[k] - v).abs())
                        .unwrap_or(0);
                    self.levels_drag = Some((output, which));
                }
                if let Some((o, which)) = self.levels_drag
                    && o == output
                    && response.is_pointer_button_down_on()
                {
                    let mut h3v = [
                        handles[0],
                        *handles.get(1).unwrap_or(&255),
                        *handles.last().unwrap_or(&255),
                    ];
                    let idx = if handles.len() == 2 {
                        // Input: [lo, hi] with no middle.
                        h3v = [handles[0], handles[1], 256];
                        which
                    } else {
                        which
                    };
                    let ordered = levels_ui::order_handles(h3v, idx, v);
                    let nv = ordered[idx];
                    let key = match (output, which, handles.len()) {
                        (_, 0, _) => Some(lo_key),
                        (false, 1, _) | (true, 2, _) => Some(hi_key),
                        _ => None,
                    };
                    match key {
                        Some(k) => {
                            let t3 = triple(&self.values, k).map(|c| c / 255.0);
                            let next = levels_ui::set_by_mask(t3, checked, nv as f32 / 255.0);
                            set_triple(&mut self.values, k, next.map(|c| c * 255.0));
                        }
                        None => {
                            let g = triple(&self.values, "gamma");
                            let target = levels_ui::gamma_from_mid(nv, lo, hi);
                            set_triple(
                                &mut self.values,
                                "gamma",
                                levels_ui::set_gamma_by_mask(g, checked, target),
                            );
                        }
                    }
                    changed = merge(changed, Some(false));
                }
            }
            if response.drag_stopped() {
                self.levels_drag = None;
                changed = Some(true);
            }
            let (hw, hh, out) = EFFECT_LEVELS_HANDLE;
            let hover = response.hovered() || response.dragged();
            let color = if hover {
                theme.menu_item_text
            } else {
                theme.pdn_control_border
            };
            for v in &handles {
                let cy = track.min.y + (1.0 - *v as f32 / 255.0) * track.height();
                for leftside in [true, false] {
                    let (x0, x1) = if leftside {
                        (track.min.x - out, track.min.x - out + hw)
                    } else {
                        (track.max.x + out, track.max.x + out - hw)
                    };
                    let poly = [
                        Pos2::new(x0, cy - hh / 2.0),
                        Pos2::new(x1, cy),
                        Pos2::new(x0, cy + hh / 2.0),
                    ];
                    triangles.push((poly, color));
                }
            }
            tracks.push((tr, pixels));
        }
        // The handles last, each over what lies under it (the tracks' triangles meet between
        // them).
        let bg = theme.window_background;
        let mut painted: std::collections::BTreeMap<(i32, i32), Color32> = Default::default();
        for (poly, color) in triangles {
            let (at, rows) = aa::polygon_coverage(&poly.map(|p| p * ppp));
            for (j, row) in rows.iter().enumerate() {
                for (i, &a) in row.iter().enumerate() {
                    if a == 0 {
                        continue;
                    }
                    let key = (at.x as i32 + i as i32, at.y as i32 + j as i32);
                    let p = Pos2::new(key.0 as f32 + 0.5, key.1 as f32 + 0.5) / ppp;
                    let under = painted.get(&key).copied().unwrap_or_else(|| {
                        tracks
                            .iter()
                            .find(|(r, _)| r.contains(p))
                            .and_then(|(r, pix)| {
                                let (x, y) = (
                                    ((p.x - r.min.x) * ppp) as usize,
                                    ((p.y - r.min.y) * ppp) as usize,
                                );
                                pix.get(y).and_then(|row| row.get(x)).copied()
                            })
                            .unwrap_or(bg)
                    });
                    painted.insert(key, aa::blit_aa(color, a, under));
                }
            }
        }
        for ((x, y), c) in painted {
            let at = Pos2::new(x as f32, y as f32) / ppp;
            ui.painter()
                .rect_filled(Rect::from_min_size(at, Vec2::splat(1.0 / ppp)), 0.0, c);
        }
        // The R, G, B checkboxes, centred in the two middle columns.
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let (cgap, lgap) = EFFECT_LEVELS_CHANNEL_GAPS;
        let (my, mr, ml) = CHECKBOX_MARGIN;
        let labels = ["R", "G", "B"];
        let widths: Vec<f32> = labels
            .iter()
            .map(|l| ml + CHECKBOX_SIZE + mr + lgap + text::css_width(l, spec.size))
            .collect();
        let total = widths.iter().sum::<f32>() + cgap * 2.0;
        let row_top = top + EFFECT_LEVELS_ROWS.0;
        let mid_l = xs[1];
        let mid_w = 2.0 * col + gap;
        let mut x = mid_l + (mid_w - total) / 2.0;
        let label_h = CHECKBOX_SIZE + 2.0 * my;
        let label_top = row_top + (EFFECT_LEVELS_ROWS.1 - label_h) / 2.0;
        for (i, (l, key)) in labels.iter().zip(["red", "green", "blue"]).enumerate() {
            let boxed = Rect::from_min_size(
                Pos2::new(x + ml, label_top + my),
                Vec2::splat(CHECKBOX_SIZE),
            );
            let mut on = checked[i];
            let response = inputs::checkbox(
                ui,
                ("levels-channel", i),
                boxed,
                &mut on,
                true,
                theme.window_background,
            );
            ui.ctx().accesskit_node_builder(response.id, |n| {
                n.set_role(egui::accesskit::Role::CheckBox);
                n.set_label(*l);
            });
            text::draw(
                ui.painter(),
                Pos2::new(boxed.max.x + mr + lgap, label_top + (label_h - line) / 2.0),
                l,
                spec,
                theme.menu_item_text,
            );
            if on != checked[i] {
                self.values.insert(key.into(), json!(on));
                changed = merge(changed, Some(false));
            }
            x += widths[i] + cgap;
        }
        changed
    }
}

/// `createRollVisual.redraw` on its 98×98 canvas (dark theme shades).
fn draw_roll(angle: f64, roll_direction: f64, roll_amount: f64, grid: Color32) -> Canvas {
    use std::f64::consts::PI;
    let mut cx = Canvas::new(EFFECT_ROLL_SIZE as u32, EFFECT_ROLL_SIZE as u32);
    let (center, outer, sphere) = (49.0, 46.0, 31.0);
    let grid = Style::Color([grid.r(), grid.g(), grid.b(), 255]);
    cx.set_stroke_style(grid.clone());
    cx.set_line_width(1.4);
    cx.begin_path();
    cx.arc(center, center, outer, 0.0, PI * 2.0, false);
    cx.stroke();
    cx.save();
    cx.begin_path();
    cx.arc(center, center, sphere, 0.0, PI * 2.0, false);
    cx.clip(pdn_core::canvas::FillRule::NonZero);
    cx.translate(center, center);
    let grid_angle = angle * PI / 180.0;
    let direction = roll_direction * PI / 180.0;
    let roll_x = roll_amount * direction.cos() / 90.0;
    let roll_y = -roll_amount * direction.sin() / 90.0;
    let project = |x: f64, y: f64, z: f64| -> (f64, f64, f64) {
        let len = roll_x.hypot(roll_y);
        if len == 0.0 {
            return (x, y, z);
        }
        let axis = roll_y.atan2(roll_x);
        let (sa, ca) = (axis.sin(), axis.cos());
        let cos_roll = (1.0 - len * len).max(0.0).sqrt();
        let mut nx = ca * x - sa * y;
        let ny = sa * x + ca * y;
        let nz = len * nx + cos_roll * z;
        nx = cos_roll * nx - len * z;
        (ca * nx + sa * ny, -sa * nx + ca * ny, nz)
    };
    let point =
        |lon: f64, lat: f64| project(lon.cos() * lat.cos(), lon.sin() * lat.cos(), lat.sin());
    let segment = |cx: &mut Canvas, a: (f64, f64, f64), b: (f64, f64, f64), color: Style| {
        // Paint.NET draws only the visible hemisphere.
        if a.2 >= 0.03 || b.2 >= 0.03 {
            return;
        }
        cx.set_stroke_style(color);
        cx.set_line_width(1.0);
        cx.begin_path();
        cx.move_to(a.0 * sphere, a.1 * sphere);
        cx.line_to(b.0 * sphere, b.1 * sphere);
        cx.stroke();
    };
    for lat_i in (-12..=0).rev() {
        let lat = lat_i as f64 * PI / 24.0;
        let shade = pdn_core::js::math_round(100.0 + (lat_i as f64 + 12.0) / 12.0 * 100.0) as u8;
        let color = Style::Color([shade, shade, shade, 255]);
        let mut prev = point(-grid_angle - PI, lat);
        for k in -144..=144 {
            let lon = -grid_angle + k as f64 * PI / 144.0;
            let cur = point(lon, lat);
            segment(&mut cx, prev, cur, color.clone());
            prev = cur;
        }
    }
    let arc_color = Style::Color([225, 225, 225, (0.72f64 * 255.0).round() as u8]);
    for lon_i in -4..4 {
        let lon = -grid_angle + lon_i as f64 * PI / 4.0;
        let mut prev = point(lon, -PI / 2.0);
        for lat_i in -16..=0 {
            let lat = lat_i as f64 * PI / 32.0;
            let cur = point(lon, lat);
            segment(&mut cx, prev, cur, arc_color.clone());
            prev = cur;
        }
    }
    cx.restore();
    cx.set_stroke_style(grid);
    cx.begin_path();
    cx.arc(center, center, sphere, 0.0, PI * 2.0, false);
    cx.stroke();
    let a = -angle * PI / 180.0;
    cx.set_line_width(1.5);
    cx.begin_path();
    cx.move_to(center + a.cos() * sphere, center + a.sin() * sphere);
    cx.line_to(center + a.cos() * outer, center + a.sin() * outer);
    cx.stroke();
    cx
}

/// The pan thumbnail: the layer scaled to fit 90×66 by Paint.NET's bicubic scaler in linear
/// light (ui-drawn-graphics.md §2.8).
pub(super) fn pan_thumbnail(src: &ImageData, w: f64, h: f64) -> Canvas {
    let (tw, th) = ((w.round() as u32).max(1), (h.round() as u32).max(1));
    let data = pdn_core::resample::resize(
        &src.data,
        src.width,
        src.height,
        tw,
        th,
        pdn_core::resample::ResizeMode::Bicubic,
        pdn_core::resample::Gamma::Linear,
    );
    let mut cx = Canvas::new(tw, th);
    cx.put_image_data(
        &ImageData {
            width: tw,
            height: th,
            data,
        },
        0,
        0,
    );
    cx
}

/// `createPanVisual.redraw`: the checkerboard box (its CSS background), the thumbnail at 45 %,
/// the centre-to-pan line and the cross.
pub(super) fn draw_pan(
    thumb: Option<&Canvas>,
    pan_x: f64,
    pan_y: f64,
    (dw, dh): (f64, f64),
) -> Canvas {
    let (w, h) = (EFFECT_PAN_SIZE.0 as f64, EFFECT_PAN_SIZE.1 as f64);
    let mut cx = Canvas::new(w as u32, h as u32);
    // `rgb(191, 191, 191)` under `--image-transparency` (#8f8f8f / #bfbfbf 4-px checks).
    for y in 0..h as u32 / 4 + 1 {
        for x in 0..w as u32 / 4 + 1 {
            let c = if (x + y) % 2 == 0 { 0x8f } else { 0xbf };
            cx.set_fill_style(Style::Color([c, c, c, 255]));
            cx.fill_rect(x as f64 * 4.0, y as f64 * 4.0, 4.0, 4.0);
        }
    }
    if let Some(t) = thumb {
        cx.set_global_alpha(0.45);
        cx.draw_image_scaled(t, (w - dw) / 2.0, (h - dh) / 2.0, dw, dh);
        cx.set_global_alpha(1.0);
    }
    let raw_x = w / 2.0 + pan_x * dw / 2.0;
    let raw_y = h / 2.0 + pan_y * dh / 2.0;
    let (dx, dy) = (raw_x - w / 2.0, raw_y - h / 2.0);
    let edge = [
        1.0,
        if dx == 0.0 {
            f64::INFINITY
        } else {
            (w / 2.0 - 2.0) / dx.abs()
        },
        if dy == 0.0 {
            f64::INFINITY
        } else {
            (h / 2.0 - 2.0) / dy.abs()
        },
    ]
    .into_iter()
    .fold(f64::INFINITY, f64::min);
    cx.set_stroke_style(Style::Color([0x77, 0x77, 0x77, 255]));
    cx.set_line_width(2.0);
    cx.begin_path();
    cx.move_to(w / 2.0, h / 2.0);
    cx.line_to(w / 2.0 + dx * edge, h / 2.0 + dy * edge);
    cx.stroke();
    cx.set_stroke_style(Style::Color([255, 255, 255, 255]));
    cx.set_line_width(3.0);
    cx.begin_path();
    cx.move_to(raw_x - 6.0, raw_y);
    cx.line_to(raw_x + 6.0, raw_y);
    cx.move_to(raw_x, raw_y - 6.0);
    cx.line_to(raw_x, raw_y + 6.0);
    cx.stroke();
    cx.set_stroke_style(Style::Color([0x11, 0x11, 0x11, 255]));
    cx.set_line_width(2.0);
    cx.stroke();
    cx
}

/// `createLevelsHistogram.redraw` with Paint.NET's histogram (selected pixels) and, for the
/// output, its Levels LUT.
fn draw_histogram(
    hist: &[[u32; 256]; 4],
    params: &pdn_core::effect::adjustments::LevelsParams,
    output: bool,
    checked: [bool; 3],
    w: usize,
    h: usize,
) -> Canvas {
    let mut cx = Canvas::new(w as u32, h as u32);
    let mut bins = [[0u64; 256]; 3];
    for c in 0..3 {
        let lut = params.lut(c);
        for i in 0..256 {
            let dest = if output {
                (lut[i] * 255.0).round().clamp(0.0, 255.0) as usize
            } else {
                i
            };
            bins[c][dest] += hist[c][i] as u64;
        }
    }
    let max = bins.iter().flatten().copied().max().unwrap_or(1).max(1) as f64;
    let colors = [[255, 91, 105], [92, 224, 111], [105, 121, 255]];
    let (wf, hf) = (w as f64, h as f64);
    for c in 0..3 {
        let [r, g, b] = colors[c];
        let (baseline, dir) = if output { (0.0, 1.0) } else { (wf, -1.0) };
        cx.begin_path();
        cx.move_to(baseline, hf);
        for i in 0..256 {
            let s = (bins[c][i.max(1) - 1] + bins[c][i] + bins[c][(i + 1).min(255)]) as f64 / 3.0;
            cx.line_to(
                baseline + dir * s / max * wf * 0.96,
                hf - i as f64 / 255.0 * hf,
            );
        }
        cx.line_to(baseline, 0.0);
        cx.close_path();
        let (fa, sa) = if checked[c] {
            (0.30, 0.95)
        } else {
            (0.10, 0.34)
        };
        cx.set_fill_style(Style::Color([r, g, b, (fa * 255.0f64).round() as u8]));
        cx.set_stroke_style(Style::Color([r, g, b, (sa * 255.0f64).round() as u8]));
        cx.set_line_width(if checked[c] { 1.5 } else { 1.0 });
        cx.fill(pdn_core::canvas::FillRule::NonZero);
        cx.stroke();
    }
    cx
}
