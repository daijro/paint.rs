//! SettingsCanvasEntry.js and css/window.css. Reference: paintdotjs @ b4e6fc5.
//! Native checker brightness/surround/shadow: canvas-rendering.md §§2.6–2.7 and §5 Task51.
use super::EntryUi;
use crate::{
    kit::{
        aa, img,
        inputs::{self, NumberStyle},
        px,
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Color32, ColorImage, Pos2, Rect, Sense, Vec2};
use pdn_core::util::color::Color;

pub fn snap_brightness(v: f64) -> f64 {
    if !v.is_finite() {
        return 0.75;
    }
    (v.clamp(0.25, 1.) * 20.).round() / 20.
}
/// Skia restores the CSS opacity layer with separate truncated source/background products.
fn css_opacity(c: Color32, bg: Color32) -> Color32 {
    let scale = |c: u8| ((c as u32 * 184) >> 8) as u8;
    aa::blit_translucent(
        Color32::from_rgba_premultiplied(scale(c.r()), scale(c.g()), scale(c.b()), 184),
        255,
        bg,
    )
}
fn hex(color: &Color) -> String {
    color.to_hex()[..7].to_owned()
}
fn reset(e: &mut EntryUi, id: &str, r: Rect, enabled: bool) -> bool {
    let child =
        e.ui.new_child(egui::UiBuilder::new().max_rect(r).id_salt(id));
    let response = e.ui.interact(
        r,
        child.scope_id(),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    let t = theme::current(child.ctx());
    child.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Button);
        n.set_label("Reset");
        if !enabled {
            n.set_disabled();
        }
    });
    child.painter().rect_filled(r, 0., t.button_outline);
    child.painter().rect_filled(r.shrink(1.), 0., t.button);
    img::paint_icon(
        child.painter(),
        "reset_icon.png",
        px::snap_rect(
            Rect::from_min_size(
                Pos2::new(
                    (r.center().x - SETTINGS_RESET.2 / 2.).max(r.min.x + 4.),
                    r.min.y + 4.,
                ),
                Vec2::splat(SETTINGS_RESET.2),
            ),
            child.ctx().pixels_per_point(),
        ),
        t.button,
        if enabled {
            img::Filter::None
        } else {
            img::Filter::Grayscale
        },
    );
    inputs::take_enter(&child, &response);
    enabled && inputs::button_activated(&child, &response)
}

pub(super) fn show(e: &mut EntryUi) {
    e.add_checkbox(
        "canvas.dropShadow",
        "Draw a shadow around the canvas",
        SETTINGS_CHECKBOX_ROWS.1,
    );
    let enabled = e.add_checkbox(
        "canvas.customBorder",
        "Use a custom color for the canvas border",
        SETTINGS_CHECKBOX_ROWS.1,
    );
    let t = theme::current(e.ui.ctx());
    let x = e.rect.min.x;
    let y = e.y + SETTINGS_COLOR_EDITOR.3;
    let narrow = e.ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT;
    let wheel_size = if narrow {
        SETTINGS_COLOR_WHEEL.1
    } else {
        SETTINGS_COLOR_WHEEL.0
    };
    let left = if narrow {
        x
    } else {
        x + SETTINGS_COLOR_EDITOR.0
    };
    let right = (left + SETTINGS_COLOR_EDITOR.1).min(e.rect.max.x - 17.);
    let mut color = Color::from_hex(e.cx.settings.get_str("canvas.borderColor"));
    let old = hex(&color);
    let swatch = Rect::from_min_size(Pos2::new(left, y + 2.), Vec2::splat(SETTINGS_COLOR_SWATCH));
    e.ui.painter().rect_filled(swatch, 0., t.pdn_control_border);
    e.ui.painter().rect_filled(
        swatch.shrink(2.),
        0.,
        Color32::from_rgb(color.red, color.green, color.blue),
    );
    let swatch_response = e.ui.interact(
        swatch,
        e.ui.scope_id().with("border-swatch"),
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    if inputs::button_activated(e.ui, &swatch_response) {
        use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogSpec, TaskTextField};
        use std::{cell::RefCell, rc::Rc};
        let field = Rc::new(RefCell::new(old.clone()));
        let value = field.clone();
        TaskDialog::show_then(
            TaskDialogSpec {
                title: "Canvas border color".into(),
                field: Some(TaskTextField {
                    label: "Hex color:".into(),
                    text: field,
                    ..Default::default()
                }),
                choices: vec![
                    TaskChoice {
                        value: "ok",
                        title: "OK".into(),
                        ..Default::default()
                    },
                    TaskChoice {
                        value: "cancel",
                        title: "Cancel".into(),
                        ..Default::default()
                    },
                ],
                cancel_value: "cancel",
                ..Default::default()
            },
            move |choice, cx| {
                if choice == "ok" {
                    let s = value.borrow();
                    if s.len() == 7
                        && s.starts_with('#')
                        && s[1..].bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        cx.settings
                            .set("canvas.borderColor", s.to_ascii_lowercase().into());
                        cx.settings_changed = true;
                    } else {
                        crate::platform::files::message(
                            "Canvas border color",
                            "Enter a color as #RRGGBB.".into(),
                        );
                    }
                }
            },
        );
    }
    let wheel = Rect::from_min_size(
        Pos2::new(left + SETTINGS_COLOR_SWATCH + SETTINGS_COLOR_GAP, y),
        Vec2::splat(wheel_size),
    );
    if e.state
        .wheel
        .as_ref()
        .is_none_or(|(on, bg, _, _)| *on != enabled || *bg != t.window_background)
    {
        let size = SETTINGS_COLOR_WHEEL.0 as usize;
        let coverage = |radius| {
            use pdn_core::canvas::{Canvas, FillRule, Style};
            let mut canvas = Canvas::new(size as u32, size as u32);
            canvas.set_fill_style(Style::Color([255; 4]));
            canvas.begin_path();
            canvas.arc(63., 63., radius, 0., std::f64::consts::TAU, false);
            canvas.fill(FillRule::NonZero);
            canvas
                .pixmap()
                .pixels()
                .iter()
                .map(|p| p.alpha())
                .collect::<Vec<_>>()
        };
        let outer_coverage = coverage(63.);
        let inner_coverage = coverage(62.);
        let mut image = ColorImage::filled([size, size], t.window_background);
        for py in 0..size {
            for px in 0..size {
                let dx = px as f64 + 0.5 - 63.;
                let dy = py as f64 + 0.5 - 63.;
                let radius = dx.hypot(dy);
                if radius > 64. {
                    continue;
                }
                let outer = outer_coverage[py * size + px] as f64 / 255.;
                let inner = inner_coverage[py * size + px] as f64 / 255.;
                if outer == 0. {
                    continue;
                }
                let hue = (dy.atan2(dx) / std::f64::consts::TAU + 0.25).rem_euclid(1.) * 6.;
                let sector = hue.floor() as usize;
                let f = hue.fract();
                let stops = [
                    [255., 0., 0.],
                    [255., 255., 0.],
                    [0., 255., 0.],
                    [0., 255., 255.],
                    [0., 0., 255.],
                    [255., 0., 255.],
                    [255., 0., 0.],
                ];
                let stop = radius / (63. * std::f64::consts::SQRT_2);
                let alpha = if stop < 0.15 {
                    1. - stop / 0.15 * 0.18
                } else {
                    (1. - (stop - 0.15) / 0.53) * 0.82
                }
                .clamp(0., 1.);
                let mut channels: [f64; 3] = std::array::from_fn(|c| {
                    let v = stops[sector][c] + (stops[sector + 1][c] - stops[sector][c]) * f;
                    (v + (255. - v) * alpha) * (1. - 0.35 * (outer - inner))
                });
                if !enabled {
                    let gray = channels[0] * 0.2126 + channels[1] * 0.7152 + channels[2] * 0.0722;
                    channels = [gray; 3];
                }
                let opacity = if enabled { 256 } else { 184 };
                let alpha_scale = (outer * opacity as f64).round() as u32;
                let premul: [u8; 3] = std::array::from_fn(|c| {
                    (((channels[c] * outer).round() as u32 * opacity) >> 8).min(255) as u8
                });
                image.pixels[py * size + px] = aa::blit_translucent(
                    Color32::from_rgba_premultiplied(
                        premul[0],
                        premul[1],
                        premul[2],
                        alpha_scale.min(255) as u8,
                    ),
                    255,
                    t.window_background,
                );
            }
        }
        e.state.wheel = Some((
            enabled,
            t.window_background,
            e.ui.ctx().load_texture(
                "settings-css-wheel",
                image.clone(),
                egui::TextureOptions::LINEAR,
            ),
            image,
        ));
    }
    e.ui.painter().image(
        e.state.wheel.as_ref().unwrap().2.id(),
        wheel,
        Rect::from_min_max(Pos2::ZERO, Pos2::new(1., 1.)),
        Color32::WHITE,
    );
    let response = e.ui.interact(
        wheel,
        e.ui.scope_id().with("border-wheel"),
        if enabled {
            Sense::CLICK | Sense::DRAG
        } else {
            Sense::hover()
        },
    );
    // As the wheel is drawn: hue clockwise from red at the top (the conic gradient's start),
    // saturation out to a ring 4 px inside the rim, so the cursor stays on the wheel.
    let reach = wheel.width() / 2. - 4.;
    if enabled
        && response.is_pointer_button_down_on()
        && let Some(p) = response.interact_pointer_pos()
    {
        let delta = p - wheel.center();
        let h = (delta.y.atan2(delta.x) as f64 / std::f64::consts::TAU + 0.25).rem_euclid(1.);
        color = Color::from_hsl(
            h,
            (delta.length() / reach).min(1.) as f64,
            color.get_lightness(),
            255,
        );
    }
    let angle = (color.get_hue() - 0.25) * std::f64::consts::TAU;
    let cursor = wheel.center()
        + Vec2::new(angle.cos() as f32, angle.sin() as f32) * color.get_saturation() as f32 * reach;
    // CSS 7px circular border with its 1px translucent outline. Raster independently of egui's tessellation.
    let ppp = e.ui.ctx().pixels_per_point();
    let base = &e.state.wheel.as_ref().unwrap().3;
    for cy in ((cursor.y - 5.) * ppp).floor() as i32..((cursor.y + 5.) * ppp).ceil() as i32 {
        for cx in ((cursor.x - 5.) * ppp).floor() as i32..((cursor.x + 5.) * ppp).ceil() as i32 {
            let pos = Pos2::new(cx as f32 / ppp, cy as f32 / ppp);
            let ix = (((pos.x - wheel.min.x) / wheel.width() * 126.).floor() as i32).clamp(0, 125)
                as usize;
            let iy = (((pos.y - wheel.min.y) / wheel.height() * 126.).floor() as i32).clamp(0, 125)
                as usize;
            let bg = base.pixels[iy * 126 + ix];
            let mut black = 0.;
            let mut white = 0.;
            for sy in 0..8 {
                for sx in 0..8 {
                    let p =
                        pos + Vec2::new((sx as f32 + 0.5) / 8. / ppp, (sy as f32 + 0.5) / 8. / ppp);
                    let d = p.distance(cursor);
                    if (3.5..4.5).contains(&d) {
                        black += 1.;
                    } else if (2.5..3.5).contains(&d) {
                        white += 1.;
                    }
                }
            }
            black /= 64.;
            white /= 64.;
            if black + white == 0. {
                continue;
            }
            let full_white = if enabled {
                Color32::WHITE
            } else {
                css_opacity(Color32::WHITE, t.window_background)
            };
            let window_part = if enabled {
                Color32::BLACK
            } else {
                Color32::from_gray(((t.window_background.r() as u32 * 72) >> 8) as u8)
            };
            let channel = |v: u8, w: u8, b: u8| {
                ((v as f32 * (1. - white - black * 0.45)
                    + w as f32 * white
                    + b as f32 * black * 0.45)
                    .round()) as u8
            };
            e.ui.painter().rect_filled(
                Rect::from_min_size(pos, Vec2::splat(1. / ppp)),
                0.,
                Color32::from_rgb(
                    channel(bg.r(), full_white.r(), window_part.r()),
                    channel(bg.g(), full_white.g(), window_part.g()),
                    channel(bg.b(), full_white.b(), window_part.b()),
                ),
            );
        }
    }
    let barx = wheel.max.x + SETTINGS_COLOR_GAP;
    let bar = Rect::from_min_size(
        Pos2::new(barx, y + 2.),
        Vec2::new(SETTINGS_COLOR_BARS.0, SETTINGS_COLOR_BARS.2),
    );
    // The colour's lightness ramp, white through it to black (grey while disabled).
    for i in 0..120 {
        let c = if enabled {
            let l = 1. - i as f64 / 119.;
            let c = Color::from_hsl(color.get_hue(), color.get_saturation(), l, 255);
            Color32::from_rgb(c.red, c.green, c.blue)
        } else {
            Color32::from_gray((255. - i as f32 / 119. * 250.).round() as u8)
        };
        e.ui.painter().rect_filled(
            Rect::from_min_size(
                bar.min + Vec2::new(0., i as f32),
                Vec2::new(SETTINGS_COLOR_BARS.0, 1.),
            ),
            0.,
            c,
        );
    }
    e.ui.painter().rect_stroke(
        Rect::from_min_max(bar.min + Vec2::new(2., 0.), bar.max - Vec2::new(2., 0.)),
        0.,
        egui::Stroke::new(1., t.pdn_control_border),
        egui::StrokeKind::Inside,
    );
    let vr = e.ui.interact(
        bar,
        e.ui.scope_id().with("border-value"),
        if enabled {
            Sense::click_and_drag()
        } else {
            Sense::hover()
        },
    );
    // The thumb's centre runs from 4 px above the bar's bottom (lightness 0) to 4 px below its
    // top (lightness 1).
    let travel = bar.height() - 8.;
    if enabled
        && vr.is_pointer_button_down_on()
        && let Some(p) = vr.interact_pointer_pos()
    {
        color = Color::from_hsl(
            color.get_hue(),
            color.get_saturation(),
            ((bar.max.y - 4. - p.y) / travel).clamp(0., 1.) as f64,
            255,
        );
    }
    if vr.clicked() {
        vr.request_focus();
    }
    e.ui.ctx().accesskit_node_builder(vr.id, |n| {
        n.set_role(egui::accesskit::Role::Slider);
        n.set_label("Canvas border brightness");
        if !enabled {
            n.set_disabled();
        }
    });
    if enabled && vr.has_focus() {
        e.ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                vr.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        let delta = e.ui.input(|i| {
            if !i.modifiers.is_none() {
                return 0.;
            }
            let up = i.key_pressed(egui::Key::ArrowUp) || i.key_pressed(egui::Key::ArrowRight);
            let down = i.key_pressed(egui::Key::ArrowDown) || i.key_pressed(egui::Key::ArrowLeft);
            f64::from(up as u8) - f64::from(down as u8)
        });
        if delta != 0. {
            color = Color::from_hsl(
                color.get_hue(),
                color.get_saturation(),
                (color.get_lightness() + delta / 255.).clamp(0., 1.),
                255,
            );
        }
    }
    let thumb = Rect::from_center_size(
        Pos2::new(
            bar.center().x,
            bar.max.y - 4. - color.get_lightness() as f32 * travel,
        ),
        Vec2::new(20., 8.),
    );
    e.ui.painter().rect_stroke(
        thumb.expand(1.),
        3.,
        egui::Stroke::new(1., Color32::from_gray(51)),
        egui::StrokeKind::Inside,
    );
    e.ui.painter().rect_stroke(
        thumb,
        2.,
        egui::Stroke::new(1., Color32::WHITE),
        egui::StrokeKind::Inside,
    );
    let neutral = Rect::from_min_size(
        Pos2::new(barx + SETTINGS_COLOR_BARS.3, y + 2.),
        Vec2::new(SETTINGS_COLOR_BARS.1, SETTINGS_COLOR_BARS.2),
    );
    for i in 0..120 {
        let gray = (255. - i as f32 / 119. * 136.).round() as u8;
        e.ui.painter().rect_filled(
            Rect::from_min_size(
                neutral.min + Vec2::new(0., i as f32),
                Vec2::new(SETTINGS_COLOR_BARS.1, 1.),
            ),
            0.,
            if enabled {
                Color32::from_gray(gray)
            } else {
                css_opacity(Color32::from_gray(gray), t.window_background)
            },
        );
    }
    e.ui.painter().rect_stroke(
        neutral,
        0.,
        egui::Stroke::new(1., t.pdn_control_border),
        egui::StrokeKind::Inside,
    );
    for (i, label) in ["R:", "G:", "B:"].iter().enumerate() {
        let cy = y + i as f32 * SETTINGS_COLOR_RGB.2;
        let mut value = match i {
            0 => color.red,
            1 => color.green,
            _ => color.blue,
        } as f64;
        text::draw(
            e.ui.painter(),
            Pos2::new(right - 84., cy + 6.),
            label,
            FontSpec::new(DIALOG_FONT_SIZE),
            t.muted_text,
        );
        let response = inputs::number_input(
            e.ui,
            (*label, "border"),
            label,
            Rect::from_min_size(
                Pos2::new(right - SETTINGS_COLOR_RGB.0, cy),
                Vec2::new(SETTINGS_COLOR_RGB.0, SETTINGS_COLOR_RGB.1),
            ),
            &mut value,
            0.,
            255.,
            1.,
            enabled,
            NumberStyle {
                font: SETTINGS_CHECKER_ROW.0,
                pad_left: 5.,
                pad_right: 5.,
                pad_y: 2.,
                ..NumberStyle::DIALOG
            },
        );
        e.ui.ctx().accesskit_node_builder(response.id, |n| {
            n.set_role(egui::accesskit::Role::SpinButton);
            n.set_label(*label);
        });
        match i {
            0 => color.red = value as u8,
            1 => color.green = value as u8,
            _ => color.blue = value as u8,
        }
    }
    if reset(
        e,
        "reset-border",
        Rect::from_min_size(
            Pos2::new(right - SETTINGS_RESET.0, y + SETTINGS_COLOR_RGB.3),
            Vec2::new(SETTINGS_RESET.0, SETTINGS_RESET.1),
        ),
        enabled,
    ) && enabled
    {
        color = Color::from_hex("#808080");
    }
    if old != hex(&color) {
        e.set("canvas.borderColor", hex(&color).into());
    }
    if !enabled {
        let mut spans = vec![Rect::from_min_max(
            Pos2::new(left, y),
            Pos2::new(right, y + SETTINGS_COLOR_EDITOR.2),
        )];
        for hole in [wheel, neutral] {
            let mut next = vec![];
            for rect in spans {
                let h = rect.intersect(hole);
                if !h.is_positive() {
                    next.push(rect);
                    continue;
                }
                for part in [
                    Rect::from_min_max(rect.min, Pos2::new(rect.max.x, h.min.y)),
                    Rect::from_min_max(Pos2::new(rect.min.x, h.max.y), rect.max),
                    Rect::from_min_max(Pos2::new(rect.min.x, h.min.y), Pos2::new(h.min.x, h.max.y)),
                    Rect::from_min_max(Pos2::new(h.max.x, h.min.y), Pos2::new(rect.max.x, h.max.y)),
                ] {
                    if part.is_positive() {
                        next.push(part);
                    }
                }
            }
            spans = next;
        }
        for rect in spans {
            e.ui.painter().rect_filled(
                rect,
                0.,
                Color32::from_rgba_unmultiplied(
                    t.window_background.r(),
                    t.window_background.g(),
                    t.window_background.b(),
                    73,
                ),
            );
        }
    }
    e.y = y + 136.;
    text::draw(
        e.ui.painter(),
        Pos2::new(x, e.y),
        "Transparency Checkerboard Brightness",
        FontSpec::new(SETTINGS_CHECKER_ROW.0),
        t.menu_item_text,
    );
    let row = e.y + SETTINGS_CHECKER_ROW.1;
    let range = Rect::from_min_size(
        Pos2::new(x + 10., row),
        Vec2::new((e.rect.width() - 143.).max(30.), SETTINGS_CHECKER_ROW.2),
    );
    let mut percent = e.cx.settings.get_f64("canvas.checkerboardBrightness");
    let previous = percent;
    let response = inputs::slider(
        e.ui,
        "checker-brightness",
        range,
        &mut percent,
        25.,
        100.,
        &[],
    );
    if response.clicked() {
        response.request_focus();
    }
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::Slider);
        n.set_label("Checkerboard brightness slider");
    });
    if response.has_focus() {
        e.ui.memory_mut(|m| {
            m.set_focus_lock_filter(
                response.id,
                egui::EventFilter {
                    horizontal_arrows: true,
                    vertical_arrows: true,
                    ..Default::default()
                },
            )
        });
        e.ui.input(|i| {
            if i.modifiers.is_none() {
                if i.key_pressed(egui::Key::ArrowLeft) || i.key_pressed(egui::Key::ArrowDown) {
                    percent -= 5.;
                }
                if i.key_pressed(egui::Key::ArrowRight) || i.key_pressed(egui::Key::ArrowUp) {
                    percent += 5.;
                }
                if i.key_pressed(egui::Key::PageDown) {
                    percent -= 5.;
                }
                if i.key_pressed(egui::Key::PageUp) {
                    percent += 5.;
                }
            }
        });
    }
    let mut value = percent / 100.;
    inputs::number_input(
        e.ui,
        "checker-value",
        "Transparency Checkerboard Brightness",
        Rect::from_min_size(
            Pos2::new(e.rect.max.x - 124., row + 0.5),
            Vec2::new(SETTINGS_CHECKER_ROW.3, SETTINGS_COLOR_RGB.1),
        ),
        &mut value,
        0.25,
        1.,
        0.05,
        true,
        NumberStyle {
            font: SETTINGS_CHECKER_ROW.0,
            decimals: Some(2),
            pad_left: 5.,
            pad_right: 5.,
            pad_y: 2.,
            ..NumberStyle::DIALOG
        },
    );
    value = snap_brightness(value);
    if reset(
        e,
        "reset-checker",
        Rect::from_min_size(
            Pos2::new(e.rect.max.x - SETTINGS_RESET.3, row + 1.5),
            Vec2::splat(SETTINGS_RESET.3),
        ),
        true,
    ) {
        value = 0.75;
    }
    if previous != (value * 100.).round() {
        e.set(
            "canvas.checkerboardBrightness",
            serde_json::json!((value * 100.).round()),
        );
    }
}
