use super::layouts::pick_hex;
use super::{EffectConfigDialog, merge};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::px;
use crate::kit::text::{self, FontSpec};
use crate::theme::{self, *};
use egui::{Id, Key, Pos2, Rect, Response, Sense, Ui, Vec2};
use pdn_core::effect::bb;
use pdn_core::render::color_ui::{
    PickMods, WidgetState, hex_key_up, hex_leave, percent_from_slider, slider_from_percent,
    wheel_pick, widget_params,
};
use pdn_core::util::color::{Color, HsvColor};
use serde_json::{Value, json};

/// Primary activation opens the child; secondary assignment keeps the existing palette shortcut.
pub(super) fn color_popup_activated(ui: &Ui, response: &Response) -> bool {
    inputs::take_enter(ui, response);
    inputs::button_activated(ui, response)
}

/// R153/Q56: this private editor extends alpha rows without changing Colors-form RGB parsing.
fn alpha_popup_hex(text: &str, current: Color, leave: bool) -> Option<Color> {
    let text = text.trim();
    let text = text.strip_prefix('#').unwrap_or(text);
    if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let [rgb_width, rgba_width] = bb::conventions::EFFECT_ALPHA_HEX_EXACT_WIDTH;
    if leave && text.len() > rgba_width {
        return Some(Color::WHITE);
    }
    match text.len() {
        n if n == rgb_width => {
            let rgb = u32::from_str_radix(text, 16).ok()?;
            Some(Color::new(
                (rgb >> 16) as u8,
                (rgb >> 8) as u8,
                rgb as u8,
                current.alpha,
            ))
        }
        n if n == rgba_width => {
            let [r, g, b, a] = u32::from_str_radix(text, 16).ok()?.to_be_bytes();
            Some(Color::new(r, g, b, a))
        }
        _ => None,
    }
}

/// Native endpoint display conversion: multiply in float32, round away, clamp to a byte.
pub(super) fn levels_popup_rgb(normalized: [f32; 3]) -> Color {
    let [r, g, b] = normalized.map(|v| (v * 255.0).round().clamp(0.0, 255.0) as u8);
    Color::new(r, g, b, 255)
}

/// Local child of one effect row. `initial` deliberately retains raw JSON and missing keys.
pub(super) struct ColorPopup {
    key: &'static str,
    initial: Option<Value>,
    color: Color,
    hsv: HsvColor,
    with_alpha: bool,
    levels: bool,
    opener: Id,
    anchor: Rect,
    rect: Rect,
    opened_frame: u64,
    tab_frame: Option<u64>,
    hex: String,
    hex_focused: bool,
    hex_edited: bool,
    // Field/editor pairs let Tab initialize the existing number editor and select its text.
    editors: Vec<(Id, Id)>,
}

impl ColorPopup {
    #[allow(clippy::too_many_arguments)]
    fn new(
        key: &'static str,
        initial: Option<Value>,
        color: Color,
        with_alpha: bool,
        levels: bool,
        opener: Id,
        anchor: Rect,
    ) -> Self {
        let mut p = Self {
            key,
            initial,
            color,
            hsv: HsvColor::from_rgb(color.red, color.green, color.blue),
            with_alpha,
            levels,
            opener,
            anchor,
            rect: Rect::NOTHING,
            opened_frame: u64::MAX,
            tab_frame: None,
            hex: String::new(),
            hex_focused: false,
            hex_edited: false,
            editors: vec![],
        };
        p.sync_hex();
        p
    }

    fn sync_hex(&mut self) {
        self.hex = pick_hex(self.color, self.with_alpha)[1..].to_ascii_uppercase();
        self.hex_edited = false;
    }

    fn set_rgb(&mut self, color: Color) {
        if (color.red, color.green, color.blue)
            != (self.color.red, self.color.green, self.color.blue)
        {
            let mut hsv = HsvColor::from_rgb(color.red, color.green, color.blue);
            if hsv.s == 0 {
                hsv.h = self.hsv.h;
            }
            self.hsv = hsv;
        }
        self.color = color;
    }

    fn set_hsv(&mut self, hsv: HsvColor) {
        self.hsv = hsv;
        let [r, g, b] = hsv.to_rgb();
        self.color = Color::new(r, g, b, self.color.alpha);
    }

    fn value(&self) -> Value {
        if self.levels {
            json!([self.color.red, self.color.green, self.color.blue])
        } else {
            json!(pick_hex(self.color, self.with_alpha))
        }
    }

    // A successful explicit edit also canonicalizes an unchanged raw token.
    fn apply_hex(&mut self, leave: bool) -> bool {
        if !self.hex_edited {
            return false;
        }
        let parsed = if self.with_alpha {
            alpha_popup_hex(&self.hex, self.color, leave)
        } else {
            let rgb = if leave {
                let (v, text) = hex_leave(&self.hex);
                self.hex = text;
                Some(v)
            } else {
                hex_key_up(&self.hex)
            };
            rgb.map(|v| Color::new((v >> 16) as u8, (v >> 8) as u8, v as u8, 255))
        };
        if let Some(color) = parsed {
            self.set_rgb(color);
        }
        if leave {
            if self.with_alpha {
                self.sync_hex();
            } else {
                self.hex_edited = false;
            }
        }
        parsed.is_some()
    }
}

/// Fixed-size content scrolls into view without scaling the wheel at small viewports.
fn popup_editor_visible(ui: &Ui, rect: Rect) {
    let clip = ui.clip_rect();
    let target = Rect::from_center_size(rect.center(), rect.size().min(clip.size()));
    if !clip.contains_rect(target) {
        ui.scroll_to_rect(target, None);
        ui.ctx()
            .request_discard("bring the focused popup editor into the viewport");
    }
}

/// Q57 port geometry; wheel math remains the unscaled shared Task29 geometry.
fn color_popup_rect(anchor: Rect, screen: Rect, alpha: bool) -> Rect {
    let size = Vec2::new(
        EFFECT_COLOR_POPUP_WIDTH,
        if alpha {
            EFFECT_COLOR_POPUP_HEIGHT.1
        } else {
            EFFECT_COLOR_POPUP_HEIGHT.0
        },
    );
    let visible = size.min(screen.size());
    let y = if anchor.max.y + size.y <= screen.max.y {
        anchor.max.y
    } else if anchor.min.y - size.y >= screen.min.y {
        anchor.min.y - size.y
    } else {
        anchor.max.y.clamp(screen.min.y, screen.max.y - visible.y)
    };
    Rect::from_min_size(
        Pos2::new(
            anchor.min.x.clamp(screen.min.x, screen.max.x - visible.x),
            y,
        ),
        visible,
    )
}

impl EffectConfigDialog {
    pub(super) fn open_color_popup(
        &mut self,
        key: &'static str,
        color: Color,
        alpha: bool,
        levels: bool,
        response: &Response,
    ) {
        if !response.enabled() {
            return;
        }
        if let Some(select) = self.open_select.take() {
            response
                .ctx
                .memory_mut(|m| m.set_focus_lock_filter(select.id, egui::EventFilter::default()));
        }
        let mut popup = ColorPopup::new(
            key,
            self.values.get(key).cloned(),
            color,
            alpha,
            levels,
            response.id,
            response.rect,
        );
        popup.opened_frame = response.ctx.cumulative_frame_nr();
        self.color_popup = Some(popup);
        response
            .ctx
            .request_discard("register effect color popup and install its first focus filter");
    }

    fn close_color_popup(&mut self, keep: bool, ctx: &egui::Context) -> Option<bool> {
        let p = self.color_popup.take()?;
        ctx.memory_mut(|m| {
            for (field, edit) in &p.editors {
                m.set_focus_lock_filter(*field, egui::EventFilter::default());
                m.set_focus_lock_filter(*edit, egui::EventFilter::default());
            }
            m.request_focus(p.opener);
        });
        if !keep {
            let changed = self.values.get(p.key) != p.initial.as_ref();
            match p.initial {
                Some(value) => {
                    self.values.insert(p.key.into(), value);
                }
                None => {
                    self.values.remove(p.key);
                }
            }
            return changed.then_some(true);
        }
        None
    }

    pub(super) fn show_color_popup(
        &mut self,
        ui: &mut Ui,
        look: crate::kit::color::ColorLook,
    ) -> Option<bool> {
        let mut p = self.color_popup.take()?;
        let mut changed = None;
        let ctx = ui.ctx().clone();
        let opening = p.opened_frame == ctx.cumulative_frame_nr();
        let mut screen = ctx.content_rect();
        if ctx.data(|d| {
            d.get_temp::<bool>(Id::unique("desktop-shell"))
                .unwrap_or(false)
        }) {
            screen.min.y += DESKTOP_TITLE_HEIGHT;
        }
        screen = screen.shrink(EFFECT_COLOR_POPUP_PADDING);
        p.rect = px::snap_rect(
            color_popup_rect(p.anchor, screen, p.with_alpha),
            ctx.pixels_per_point(),
        );
        let outside = !opening
            && ui.input(|i| {
                (i.pointer.primary_pressed() || i.pointer.secondary_pressed())
                    && i.pointer
                        .interact_pos()
                        .is_some_and(|at| !p.rect.contains(at))
            });
        let escape = !opening && ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::Escape));
        let enter = !opening && ui.input(|i| i.modifiers.is_none() && i.key_pressed(Key::Enter));
        // Withhold Tab from egui's parent traversal, including the first input after opening.
        let tab = if opening {
            None
        } else {
            ui.input_mut(|i| {
                if i.consume_key(egui::Modifiers::SHIFT, Key::Tab) {
                    Some(-1)
                } else if i.consume_key(egui::Modifiers::NONE, Key::Tab) {
                    Some(1)
                } else {
                    None
                }
            })
        };
        let before = p.color;
        let mut hex_applied = false;
        let previous_focus = ctx.memory(|m| m.focused());
        let old_editors = p.editors.clone();
        // Seat the field before it draws so the shared number input initializes in this pass.
        if let Some(direction) = tab.filter(|_| p.tab_frame != Some(ctx.cumulative_frame_nr())) {
            let n = old_editors.len();
            let current = old_editors
                .iter()
                .position(|(f, e)| Some(*f) == previous_focus || Some(*e) == previous_focus)
                .unwrap_or(n - 1);
            let next = (current as isize + direction + n as isize) as usize % n;
            ctx.memory_mut(|m| m.request_focus(old_editors[next].0));
            p.tab_frame = Some(ctx.cumulative_frame_nr());
        }
        let mut paint = || {
            let mut child = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("effect-color-popup")
                    .max_rect(p.rect),
            );
            child.set_clip_rect(p.rect);
            // Pointer shield is not a Tab stop; all parent input is already withheld.
            child.interact(
                ctx.content_rect(),
                child.scope_id().with("shield"),
                Sense::CLICK | Sense::DRAG,
            );
            text::occlude(&ctx, p.rect);
            let theme = theme::current(&ctx);
            child
                .painter()
                .rect_filled(p.rect, 0.0, theme.pdn_control_border);
            child
                .painter()
                .rect_filled(p.rect.shrink(1.0), 0.0, theme.window_background);
            let size = Vec2::new(
                EFFECT_COLOR_POPUP_WIDTH,
                if p.with_alpha {
                    EFFECT_COLOR_POPUP_HEIGHT.1
                } else {
                    EFFECT_COLOR_POPUP_HEIGHT.0
                },
            );
            egui::ScrollArea::both()
                .id_salt("color-scroll")
                .auto_shrink([false, false])
                .show(&mut child, |ui| {
                    let at = ui.cursor().min;
                    ui.set_min_size(size);
                    p.editors.clear();
                    let pad = EFFECT_COLOR_POPUP_PADDING;
                    let wheel = Rect::from_min_size(
                        at + Vec2::splat(pad),
                        Vec2::splat(EFFECT_COLOR_POPUP_WHEEL),
                    );
                    let mods = ui.input(|i| PickMods {
                        shift: i.modifiers.shift,
                        alt: i.modifiers.alt,
                        ctrl: i.modifiers.ctrl,
                    });
                    let view = crate::kit::color::WheelView {
                        mods,
                        guide_opacity: if ui.rect_contains_pointer(wheel) {
                            1.0
                        } else {
                            0.0
                        },
                        ..crate::kit::color::WheelView::of(p.hsv)
                    };
                    let (response, geometry) =
                        crate::kit::color::color_wheel(ui, wheel, &view, look);
                    response.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::accesskit::Role::Image, true, "Color wheel")
                    });
                    let mut non_hex_changed = false;
                    let tap = crate::kit::color::tap(ui, &response);
                    if (response.is_pointer_button_down_on() || tap.is_some())
                        && let Some(pos) =
                            tap.map(|(_, pos)| pos).or(response.interact_pointer_pos())
                    {
                        let d = (pos - response.rect.min) * ctx.pixels_per_point();
                        let hsv = wheel_pick(
                            &geometry,
                            d.x.floor() as i32,
                            d.y.floor() as i32,
                            p.hsv,
                            mods,
                        );
                        if hsv != p.hsv {
                            p.set_hsv(hsv);
                            non_hex_changed = true;
                        }
                    }
                    let preview = Rect::from_min_size(
                        wheel.right_top() + Vec2::new(pad, 0.0),
                        Vec2::splat(EFFECT_COLOR_POPUP_PREVIEW),
                    );
                    crate::kit::color::color_preview(
                        ui,
                        preview,
                        p.color,
                        widget_params(WidgetState::Normal, ctx.pixels_per_point() as f64),
                        look,
                    );
                    let mut y = wheel.max.y + pad;
                    for (k, label) in ["Hue", "Saturation", "Value", "Alpha"]
                        .iter()
                        .enumerate()
                        .take(if p.with_alpha { 4 } else { 3 })
                    {
                        text::draw(
                            ui.painter(),
                            Pos2::new(at.x + pad, y + 4.0),
                            label,
                            FontSpec::new(DIALOG_FONT_SIZE),
                            theme.menu_item_text,
                        );
                        let mut value = match k {
                            0 => p.hsv.h as f64,
                            1 => p.hsv.s as f64,
                            2 => p.hsv.v as f64,
                            _ => p.color.alpha as f64,
                        };
                        let old = value;
                        if k >= 2 {
                            let stops = if k == 2 {
                                [0, 100]
                                    .into_iter()
                                    .map(|v| {
                                        let [r, g, b] = HsvColor::new(p.hsv.h, p.hsv.s, v).to_rgb();
                                        [r, g, b, 255]
                                    })
                                    .collect::<Vec<_>>()
                            } else {
                                (0..=255)
                                    .map(|a| [p.color.red, p.color.green, p.color.blue, a])
                                    .collect()
                            };
                            let mut byte = if k == 2 {
                                slider_from_percent(p.hsv.v)
                            } else {
                                p.color.alpha
                            };
                            let r = Rect::from_min_size(
                                Pos2::new(at.x + 70.0, y),
                                Vec2::new(85.0, 25.0),
                            );
                            if crate::kit::color::color_slider(ui, r, &stops, &mut byte, look)
                                .changed()
                            {
                                value = if k == 2 {
                                    percent_from_slider(byte) as f64
                                } else {
                                    byte as f64
                                };
                            }
                        }
                        let number =
                            Rect::from_min_size(Pos2::new(at.x + 164.0, y), Vec2::new(60.0, 25.0));
                        let field = ui.scope_id().with(("number_input", k));
                        p.editors.push((field.with("field"), field.with("edit")));
                        inputs::number_input(
                            ui,
                            k,
                            label,
                            number,
                            &mut value,
                            0.0,
                            if k == 0 {
                                360.0
                            } else if k == 3 {
                                255.0
                            } else {
                                100.0
                            },
                            1.0,
                            true,
                            NumberStyle::COLORS,
                        );
                        ui.ctx().accesskit_node_builder(field.with("field"), |n| {
                            n.set_role(egui::accesskit::Role::SpinButton);
                            n.set_label(*label);
                            n.set_value(value.to_string());
                        });
                        if ui.memory(|m| {
                            m.has_focus(field.with("field")) || m.has_focus(field.with("edit"))
                        }) {
                            popup_editor_visible(ui, number);
                        }
                        if value != old {
                            non_hex_changed = true;
                            let v = value as i32;
                            if k == 3 {
                                p.color.alpha = v as u8;
                            } else {
                                p.set_hsv(HsvColor::new(
                                    if k == 0 { v } else { p.hsv.h },
                                    if k == 1 { v } else { p.hsv.s },
                                    if k == 2 { v } else { p.hsv.v },
                                ));
                            }
                        }
                        y += 29.0;
                    }
                    if non_hex_changed {
                        p.sync_hex();
                    }
                    y += 4.0;
                    let hex_rect = Rect::from_min_size(
                        Pos2::new(at.x + pad, y),
                        Vec2::new(size.x - 2.0 * pad, 25.0),
                    );
                    let field = ui.scope_id().with(("text_input", "hex"));
                    p.editors.push((field.with("field"), field.with("edit")));
                    let r = inputs::text_input(
                        ui,
                        "hex",
                        "Hex",
                        hex_rect,
                        &mut p.hex,
                        (theme.menu_background, theme.menu_item_text),
                        TEXT_INPUT_PADDING.1,
                        DIALOG_FONT_SIZE,
                        true,
                        true,
                    );
                    let focused = inputs::text_input_focused(ui, "hex");
                    if focused {
                        popup_editor_visible(ui, hex_rect);
                    }
                    if !non_hex_changed {
                        p.hex_edited |= r.changed();
                        if r.changed() {
                            hex_applied |= p.apply_hex(false);
                        }
                        if (p.hex_focused && !focused) || enter || outside {
                            hex_applied |= p.apply_hex(true);
                        }
                    }
                    p.hex_focused = focused;
                });
        };
        if opening || escape {
            super::super::without_input(&ctx, paint);
        } else {
            paint();
        }
        if !escape && (p.color != before || hex_applied) {
            self.values.insert(p.key.into(), p.value());
            changed = Some(false);
        }
        // Every editor has the same local lock, cleared when the popup closes.
        if old_editors.is_empty() {
            let edit = p.editors.last().unwrap().1;
            let mut state = egui::text_edit::TextEditState::load(&ctx, edit).unwrap_or_default();
            state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(p.hex.len()),
                )));
            state.store(&ctx, edit);
            ctx.memory_mut(|m| m.request_focus(edit));
            ctx.request_discard("seat effect color popup editor before its next input");
        }
        for &(field, edit) in &p.editors {
            for id in [field, edit] {
                if ctx.memory(|m| m.has_focus(id) && !m.had_focus_last_frame(id)) {
                    ctx.request_discard("install effect color popup editor focus filter");
                }
                ctx.memory_mut(|m| {
                    m.set_focus_lock_filter(
                        id,
                        egui::EventFilter {
                            tab: true,
                            escape: true,
                            horizontal_arrows: true,
                            vertical_arrows: true,
                        },
                    )
                });
            }
        }
        self.color_popup = Some(p);
        if escape || enter || outside {
            changed = merge(changed, self.close_color_popup(!escape, &ctx));
            if outside {
                self.color_dismiss_gesture = ui.input(|i| i.pointer.any_down());
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{dialog, layer};
    use super::super::*;
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn bb25_opaque_hex_uses_existing_rgb_leave_policy() {
        for (text, want) in [
            ("1", "#000001"),
            ("#123aBc", "#123abc"),
            ("1000000", "#ffffff"),
            ("invalid", "#000000"),
        ] {
            let mut p = ColorPopup::new(
                "color",
                Some(json!("primary")),
                Color::WHITE,
                false,
                false,
                Id::NULL,
                Rect::NOTHING,
            );
            p.hex = text.into();
            p.hex_edited = true;
            p.apply_hex(true);
            assert_eq!(p.value(), json!(want));
        }
    }

    #[test]
    fn bb25_popup_geometry_flips_clamps_and_keeps_wheel_size() {
        let screen = Rect::from_min_max(Pos2::new(8.0, 38.0), Pos2::new(792.0, 592.0));
        let below = Rect::from_min_size(Pos2::new(700.0, 50.0), Vec2::new(50.0, 25.0));
        let r = color_popup_rect(below, screen, true);
        assert_eq!(r.size(), Vec2::new(232.0, 349.0));
        assert_eq!(r.min, Pos2::new(560.0, 75.0));
        let above = color_popup_rect(below.translate(Vec2::new(0.0, 450.0)), screen, true);
        assert_eq!(above.min.y, 151.0);
        let tiny = Rect::from_min_size(Pos2::new(8.0, 38.0), Vec2::new(164.0, 170.0));
        assert_eq!(color_popup_rect(below, tiny, true), tiny);
    }

    #[test]
    fn bb25_popup_escape_rejects_prior_generation_and_preview_off() {
        let (mut app, mut d) = dialog("bbGridCheckerboard", None);
        let source = layer(&app);
        let ctx = egui::Context::default();
        let generation = d.generation;
        let flag = Arc::new(AtomicBool::new(false));
        d.job_cancel = Some(flag.clone());
        d.color_popup = Some(ColorPopup::new(
            "color",
            d.values.get("color").cloned(),
            Color::BLACK,
            true,
            false,
            Id::NULL,
            Rect::NOTHING,
        ));
        d.values.insert("color".into(), json!("#ff000080"));
        if let Some(immediate) = d.close_color_popup(false, &ctx) {
            d.invalidate(42.0, immediate);
        }
        assert!(flag.load(Ordering::Relaxed));
        let mut settings = pdn_core::settings::AppSettings::load(None);
        let mut prefs = crate::prefs::Prefs::default();
        let mut cx = DialogCx {
            app: &mut app,
            settings: &mut settings,
            prefs: &mut prefs,
            settings_changed: false,
            shortcuts: None,
            clipboard: None,
            settings_imports: None,
            fonts: None,
            asset_library: None,
            seat_focus: false,
        };
        let result = |generation| JobResult {
            generation,
            result: Some((
                ImageData {
                    width: 4,
                    height: 3,
                    data: [255, 0, 0, 128].repeat(12),
                },
                None,
            )),
            elapsed: 1.0,
        };
        assert!(!d.accept_result(&mut cx, result(generation)));
        d.preview = false;
        assert!(!d.accept_result(&mut cx, result(d.generation)));
        assert_eq!(layer(cx.app), source);
        assert!(d.shown.is_none());
    }

    // R153/Q56: widths, byte order and pending edits are explicit popup policy.
    #[test]
    fn bb25_alpha_hex_widths_and_invalid_input() {
        let c = Color::new(255, 128, 0, 17);
        for width in 0..=10 {
            let text = "0".repeat(width);
            let live = alpha_popup_hex(&text, c, false);
            assert_eq!(live.is_some(), width == 6 || width == 8, "width {width}");
            assert_eq!(
                alpha_popup_hex(&text, c, true).is_some(),
                width == 6 || width >= 8,
                "leave width {width}"
            );
        }
        for text in [
            "",
            "1",
            "12345",
            "1234567",
            "##123456",
            "+123456",
            "0x123456",
            "12 3456",
            "１２３４５６",
            "gggggg",
        ] {
            assert_eq!(alpha_popup_hex(text, c, false), None, "{text}");
            assert_eq!(alpha_popup_hex(text, c, true), None, "{text}");
        }
        assert_eq!(
            alpha_popup_hex(" #123456 ", c, false),
            Some(Color::new(18, 52, 86, 17))
        );
        assert_eq!(
            alpha_popup_hex("12345678", c, false),
            Some(Color::new(18, 52, 86, 120))
        );
        for text in ["000000000", "123456789", &"F".repeat(4096)] {
            assert_eq!(alpha_popup_hex(text, c, false), None);
            assert_eq!(alpha_popup_hex(text, c, true), Some(Color::WHITE));
        }
    }

    #[test]
    fn bb25_exact_rgb_and_held_hue() {
        let mut p = ColorPopup::new(
            "color",
            Some(json!("#ff800011")),
            Color::new(255, 128, 0, 17),
            true,
            false,
            Id::NULL,
            Rect::NOTHING,
        );
        assert_eq!(p.color, Color::new(255, 128, 0, 17));
        p.color.alpha = 128;
        assert_eq!(p.value(), json!("#ff800080"));
        p.set_hsv(HsvColor::new(200, 0, 100));
        assert_eq!(p.hsv.h, 200);
        p.set_rgb(Color::WHITE);
        assert_eq!(p.hsv.h, 200);
    }

    #[test]
    fn bb25_popup_escape_restores_raw_value_or_absence_and_invalidates_preview() {
        for raw in [
            None,
            Some(json!("#fF800011")),
            Some(json!([0.5, 12.75, 253.125])),
        ] {
            let (_, mut d) = dialog("levelsEffect", None);
            match &raw {
                Some(v) => {
                    d.values.insert("inputLow".into(), v.clone());
                }
                None => {
                    d.values.remove("inputLow");
                }
            }
            d.color_popup = Some(ColorPopup::new(
                "inputLow",
                raw.clone(),
                Color::BLACK,
                false,
                true,
                Id::NULL,
                Rect::NOTHING,
            ));
            d.values.insert("inputLow".into(), json!([5, 6, 7]));
            let generation = d.generation;
            if let Some(immediate) = d.close_color_popup(false, &egui::Context::default()) {
                d.invalidate(42.0, immediate);
            }
            assert_eq!(d.values.get("inputLow"), raw.as_ref());
            assert_eq!((d.generation, d.due), (generation + 1, Some(42.0)));
        }
    }

    #[test]
    fn bb25_levels_picker_rounds_native_normalized_triples() {
        assert_eq!(
            levels_popup_rgb([0.5, 0.0, 1.0]),
            Color::new(128, 0, 255, 255)
        );
        assert_eq!(
            levels_popup_rgb([-1.0, 2.0, 127.5 / 255.0]),
            Color::new(0, 255, 128, 255)
        );
    }
}
