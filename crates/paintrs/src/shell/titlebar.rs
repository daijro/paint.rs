//! Port of desktop/desktop.js, AppWorkspace.updateTitle and css/menu.css caption rectangles.
//! ui-text-and-glyphs §2.1; R47/R132 bundled grayscale UI text. Native glyphs are CSS chrome.
use super::{
    ShellLayout,
    window_close::{self, Intent},
};
use crate::{
    kit::{
        aa, px,
        text::{self, FontSpec},
    },
    theme::*,
};
use egui::{Color32, Pos2, Rect, ResizeDirection, Sense, Ui, Vec2, ViewportCommand};
use pdn_core::app::AppWorkspace;

pub fn title(app: &AppWorkspace) -> String {
    let product = format!("paint.rs {}", env!("CARGO_PKG_VERSION"));
    app.active_ws().map_or_else(
        || product.clone(),
        |ws| {
            let title = pdn_core::i18n::t("mainForm.title.format")
                .replace("{0}", &ws.get_friendly_name())
                .replace("{1}", &product);
            if ws.dirty { format!("*{title}") } else { title }
        },
    )
}

pub fn resize_at(screen: Rect, pos: Pos2, maximized: bool) -> Option<ResizeDirection> {
    if maximized || !screen.contains(pos) {
        return None;
    }
    let left = pos.x < screen.min.x + RESIZE_BORDER;
    let right = pos.x >= screen.max.x - RESIZE_BORDER;
    let top = pos.y < screen.min.y + RESIZE_BORDER;
    let bottom = pos.y >= screen.max.y - RESIZE_BORDER;
    use ResizeDirection::*;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(NorthWest),
        (_, true, true, _) => Some(NorthEast),
        (true, _, _, true) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, _, _, _) => Some(West),
        (_, true, _, _) => Some(East),
        (_, _, true, _) => Some(North),
        (_, _, _, true) => Some(South),
        _ if pos.x >= screen.max.x - 16.0 && pos.y >= screen.max.y - 16.0 => Some(SouthEast),
        _ => None,
    }
}

/// Caption-glyph stroke thickness in points: `max(1, round(ppp))` whole device pixels, like
/// the native caption buttons.
fn stroke(ppp: f32) -> f32 {
    ppp.round().max(1.0) / ppp
}

/// Edges of a caption-glyph square (`outer`, in points): the outer rect is snapped once to
/// device pixels and every edge is drawn inside it with the same thickness.
fn outline_rects(outer: Rect, ppp: f32) -> [Rect; 4] {
    let o = px::snap_rect(outer, ppp);
    let t = stroke(ppp);
    let (x0, y0, x1, y1) = (o.min.x, o.min.y, o.max.x, o.max.y);
    let r = |a: f32, b: f32, c: f32, d: f32| Rect::from_min_max(Pos2::new(a, b), Pos2::new(c, d));
    [
        r(x0, y0, x1, y0 + t),
        r(x0, y1 - t, x1, y1),
        r(x0, y0 + t, x0 + t, y1 - t),
        r(x1 - t, y0 + t, x1, y1 - t),
    ]
}

/// A horizontal caption-glyph line (`line`, in points): origin snapped, thickness `stroke`.
fn line_rect(line: Rect, ppp: f32) -> Rect {
    let o = px::snap_rect(line, ppp);
    Rect::from_min_max(o.min, Pos2::new(o.max.x, o.min.y + stroke(ppp)))
}

pub fn show(ui: &mut Ui, app: &AppWorkspace, layout: &ShellLayout, theme: &Theme) {
    let ctx = ui.ctx().clone();
    let screen = ctx.content_rect();
    let band = Rect::from_min_size(screen.min, Vec2::new(screen.width(), DESKTOP_TITLE_HEIGHT));
    let (maximized, focused) = ctx.input(|i| {
        (
            i.viewport().maximized.unwrap_or(false),
            i.viewport().focused.unwrap_or(i.focused),
        )
    });
    let painter = ui.painter().clone();
    painter.rect_filled(band, 0.0, theme.title_bar);
    let title = title(app);
    let title_key = egui::Id::unique("desktop-title");
    if ctx.data_mut(|d| d.get_temp::<String>(title_key)).as_ref() != Some(&title) {
        ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
        ctx.data_mut(|d| d.insert_temp(title_key, title.clone()));
    }
    if !cfg!(target_os = "macos") {
        let side = (16.0 * ctx.pixels_per_point()).round() as u32;
        crate::kit::img::paint_generated(
            &painter,
            "desktop-app-icon",
            &crate::assets::Assets::app_artwork(&ctx, side),
            Rect::from_min_size(band.min + Vec2::new(9.0, 7.0), Vec2::splat(16.0)),
            theme.title_bar,
        );
    }
    let caption_start = if cfg!(target_os = "macos") {
        screen.max.x
    } else {
        screen.max.x - 3.0 * CAPTION_WIDTH
    };
    let text_rect = Rect::from_min_max(
        Pos2::new(
            screen.min.x
                + if cfg!(target_os = "macos") {
                    80.0
                } else {
                    34.0
                },
            band.min.y,
        ),
        Pos2::new(caption_start, band.max.y),
    );
    text::draw_ellipsized(
        &painter,
        text_rect,
        &title,
        FontSpec::new(12.0).line_height(DESKTOP_TITLE_HEIGHT),
        theme.menu_item_text,
    );
    if !cfg!(target_os = "macos") {
        for index in 0..3 {
            let rect = Rect::from_min_size(
                Pos2::new(caption_start + index as f32 * CAPTION_WIDTH, screen.min.y),
                Vec2::new(CAPTION_WIDTH, DESKTOP_TITLE_HEIGHT),
            );
            let response =
                ui.interact(rect, ui.scope_id().with(("caption", index)), Sense::click());
            let pressed = response.is_pointer_button_down_on();
            let mut backdrop = theme.title_bar;
            if focused && (response.hovered() || pressed) {
                let fill = if index == 2 {
                    if pressed {
                        CAPTION_CLOSE_PRESSED
                    } else {
                        CAPTION_CLOSE_HOVER
                    }
                } else {
                    Color32::from_white_alpha(if pressed {
                        CAPTION_PRESSED_ALPHA
                    } else {
                        CAPTION_HOVER_ALPHA
                    })
                };
                painter.rect_filled(rect, 0.0, fill);
                if index == 2 {
                    backdrop = fill;
                }
            }
            let glyph = if theme.title_bar.r() > 128 {
                Color32::from_rgb(17, 17, 17)
            } else {
                Color32::WHITE
            };
            let glyph = glyph.gamma_multiply(if focused {
                1.0
            } else {
                CAPTION_INACTIVE_OPACITY
            });
            let ppp = ctx.pixels_per_point();
            let at = |x, y, w, h| Rect::from_min_size(rect.min + Vec2::new(x, y), Vec2::new(w, h));
            let paint = |r: Rect| {
                painter.rect_filled(r, 0.0, glyph);
            };
            let outline = |x, y, w, h| {
                for r in outline_rects(at(x, y, w, h), ppp) {
                    paint(r);
                }
            };
            match index {
                0 => paint(line_rect(at(18.0, 15.0, 10.0, 1.0), ppp)),
                1 if maximized => {
                    outline(17.0, 12.0, 8.0, 8.0);
                    outline(20.0, 9.0, 8.0, 8.0);
                }
                1 => {
                    outline(18.0, 10.0, 10.0, 10.0);
                }
                _ => {
                    aa::paint_close_x(
                        &painter,
                        rect.min + Vec2::new(23.0, 14.5),
                        ppp,
                        glyph,
                        backdrop,
                    );
                }
            }
            if response.clicked() {
                match index {
                    0 => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                    1 => ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized)),
                    _ => window_close::request(&ctx, Intent::CloseRoot),
                }
            }
        }
    }
    let pos = crate::kit::inputs::press_pos(&ctx, Some(egui::PointerButton::Primary));
    if let Some(pos) = pos {
        // Interactive strip/caption children own the press before draggable/resize chrome.
        if layout.document_strip.contains(pos) || (band.contains(pos) && pos.x >= caption_start) {
            return;
        }
        if let Some(direction) = resize_at(screen, pos, maximized) {
            ctx.send_viewport_cmd(ViewportCommand::BeginResize(direction));
        } else if band.contains(pos)
            || (layout.title_bar.contains(pos)
                && !layout.main_menu.contains(pos)
                && !layout.settings_menu.contains(pos))
        {
            // A native caption maximizes or restores on a double-click. StartDrag hands the pointer
            // to the window manager (Wayland's move grab), which swallows the second click, so the
            // two presses are compared here instead of waiting for egui's double-click.
            let id = egui::Id::unique("titlebar-last-press");
            let now = ctx.input(|i| i.time);
            let (delay, dist) = ctx.options(|o| {
                (
                    o.input_options.max_double_click_delay,
                    o.input_options.max_click_dist,
                )
            });
            let last = ctx.data_mut(|d| d.get_temp::<(f64, Pos2)>(id));
            if is_double_press(last, now, pos, delay, dist) {
                ctx.data_mut(|d| d.remove::<(f64, Pos2)>(id));
                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
            } else {
                ctx.data_mut(|d| d.insert_temp(id, (now, pos)));
                ctx.send_viewport_cmd(ViewportCommand::StartDrag);
            }
        }
    }
}

/// Whether a press at (`now`, `pos`) completes a double-click with the `last` drag-area press.
fn is_double_press(last: Option<(f64, Pos2)>, now: f64, pos: Pos2, delay: f64, dist: f32) -> bool {
    last.is_some_and(|(t, p)| now - t <= delay && p.distance(pos) <= dist)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_quick_second_press_in_place_is_a_double_click() {
        let p = Pos2::new(300.0, 10.0);
        assert!(is_double_press(
            Some((1.0, p)),
            1.2,
            p + Vec2::new(2.0, 1.0),
            0.3,
            6.0
        ));
        assert!(!is_double_press(None, 1.2, p, 0.3, 6.0));
        assert!(
            !is_double_press(Some((1.0, p)), 1.5, p, 0.3, 6.0),
            "too slow"
        );
        assert!(
            !is_double_press(Some((1.0, p)), 1.1, p + Vec2::new(20.0, 0.0), 0.3, 6.0),
            "moved"
        );
    }
    #[test]
    fn caption_glyph_edges_have_uniform_device_thickness() {
        for ppp in [1.0f32, 1.25, 1.5, 2.0] {
            let t = (ppp.round().max(1.0)) as i32;
            for (x, y) in [(18.0, 10.0), (17.0, 12.0), (20.0, 9.0), (3.3, 7.7)] {
                let outer = Rect::from_min_size(Pos2::new(x + 100.0, y), Vec2::splat(10.0));
                let [top, bottom, left, right] = outline_rects(outer, ppp);
                let dev = |v: f32| (v * ppp).round() as i32;
                assert_eq!(dev(top.height()), t, "top ppp {ppp}");
                assert_eq!(dev(bottom.height()), t, "bottom ppp {ppp}");
                assert_eq!(dev(left.width()), t, "left ppp {ppp}");
                assert_eq!(dev(right.width()), t, "right ppp {ppp}");
            }
            let line = line_rect(
                Rect::from_min_size(Pos2::new(118.0, 15.0), Vec2::new(10.0, 1.0)),
                ppp,
            );
            assert_eq!((line.height() * ppp).round() as i32, t, "line ppp {ppp}");
        }
    }
    #[test]
    fn caption_glyph_geometry_at_ppp_one_is_unchanged() {
        let outer = Rect::from_min_size(Pos2::new(118.0, 10.0), Vec2::splat(10.0));
        let [top, bottom, left, right] = outline_rects(outer, 1.0);
        assert_eq!(
            top,
            Rect::from_min_size(Pos2::new(118.0, 10.0), Vec2::new(10.0, 1.0))
        );
        assert_eq!(
            bottom,
            Rect::from_min_size(Pos2::new(118.0, 19.0), Vec2::new(10.0, 1.0))
        );
        assert_eq!(
            left,
            Rect::from_min_size(Pos2::new(118.0, 11.0), Vec2::new(1.0, 8.0))
        );
        assert_eq!(
            right,
            Rect::from_min_size(Pos2::new(127.0, 11.0), Vec2::new(1.0, 8.0))
        );
    }
    #[test]
    fn unchanged_title_does_not_request_another_native_repaint() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let app = AppWorkspace::default();
        let render = || {
            ctx.run_ui(egui::RawInput::default(), |ui| {
                let layout = ShellLayout::compute(ctx.content_rect(), true, 360.0, 0);
                show(ui, &app, &layout, &Theme::for_scheme(Scheme::Dark));
            })
        };
        let mut first = render();
        first.textures_delta.clear();
        let mut second = render();
        second.textures_delta.clear();
        assert!(
            !second
                .viewport_output
                .values()
                .flat_map(|v| &v.commands)
                .any(|c| matches!(c, ViewportCommand::Title(_)))
        );
    }
    #[test]
    fn resize_edges_and_handle_use_dips_and_disable_when_maximized() {
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 830.0));
        assert_eq!(
            resize_at(r, Pos2::new(2.0, 2.0), false),
            Some(ResizeDirection::NorthWest)
        );
        assert_eq!(
            resize_at(r, Pos2::new(1268.0, 818.0), false),
            Some(ResizeDirection::SouthEast)
        );
        assert_eq!(resize_at(r, Pos2::new(1268.0, 818.0), true), None);
        assert_eq!(resize_at(r, Pos2::new(100.0, 100.0), false), None);
    }
    #[test]
    fn title_tracks_active_document_and_dirty_state() {
        let mut app = AppWorkspace::default();
        let bare = title(&app);
        app.create_blank_document_in_new_workspace(1, 1, 96.0, true);
        assert!(title(&app).contains(&bare));
        app.active_ws_mut().unwrap().dirty = true;
        assert!(title(&app).starts_with('*'));
    }
}
