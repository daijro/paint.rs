//! SettingsShapesEntry.js / ShapeManager.js and css/window.css, paintdotjs b4e6fc5.
use super::{EntryUi, asset_widgets as w, mix};
use crate::{
    dialogs::task::{TaskChoice, TaskDialog, TaskDialogHandle, TaskDialogSpec},
    kit::{
        img,
        scroll::{Scrollbar, THICKNESS},
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Pos2, Rect, Vec2};
#[derive(Default)]
pub(super) struct ShapesState {
    scroll: f64,
    clear: Option<TaskDialogHandle>,
}
pub fn show(e: &mut EntryUi) {
    e.add_heading(
        "Shapes",
        "Add Paint.NET XAML shapes to the Custom group in the Shapes tool.",
    );
    e.add_note("Shape files are stored locally in this app and are not uploaded.");
    let Some(m) = e.cx.asset_library.as_deref() else {
        return;
    };
    let ready = m.ready() && !m.busy();
    let records = m.shapes.clone();
    let errors = m.errors.clone();
    e.state.status = m.status.clone();
    let y = e.y;
    let add = w::button(
        e,
        "Add shapes…",
        "Add shapes…",
        Rect::from_min_size(
            Pos2::new(e.rect.min.x, y + 3.),
            Vec2::new(SETTINGS_BUTTON.0, ASSET_BUTTON_HEIGHT),
        ),
        ready,
    );
    let mut x = e.rect.min.x + SETTINGS_BUTTON.0 + ASSET_CONTROL_GAP;
    if !errors.is_empty() {
        let label = format!("View errors… ({})", errors.len());
        if w::button(
            e,
            &label,
            "View shape errors",
            Rect::from_min_size(
                Pos2::new(x, y + 3.),
                Vec2::new(
                    text::css_width(&label, DIALOG_FONT_SIZE) + SETTINGS_BUTTON.2,
                    ASSET_BUTTON_HEIGHT,
                ),
            ),
            true,
        ) {
            show_errors(&errors);
        }
        x += text::css_width(&label, DIALOG_FONT_SIZE) + SETTINGS_BUTTON.2 + ASSET_CONTROL_GAP;
    }
    let clear = w::button(
        e,
        "Clear all",
        "Clear all",
        Rect::from_min_size(
            Pos2::new(x, y + 3.),
            Vec2::new(SETTINGS_BUTTON.0, ASSET_BUTTON_HEIGHT),
        ),
        ready && !records.is_empty(),
    );
    e.y = y + 40.;
    let drop = w::drop_zone(
        e,
        "Drop Paint.NET .xaml shape files here",
        "shape-drop",
        ready,
    );
    w::section(e, "Installed shapes", 8.);
    let t = theme::current(e.ui.ctx());
    let list = Rect::from_min_max(
        Pos2::new(e.rect.min.x, e.y),
        Pos2::new(e.rect.max.x, e.ui.clip_rect().max.y - 14.),
    );
    let mut remove = None;
    if records.is_empty() {
        let empty = Rect::from_min_size(list.min, Vec2::new(list.width(), 70.));
        e.ui.painter().rect_filled(empty, 0., t.pdn_menu_border);
        e.ui.painter()
            .rect_filled(empty.shrink(1.), 0., t.number_input_background);
        text::draw(
            e.ui.painter(),
            empty.min + Vec2::splat(13.),
            "No custom shapes have been added.",
            FontSpec::new(DIALOG_FONT_SIZE),
            mix(t.menu_item_text, t.number_input_background, 0.66),
        );
    } else {
        let clip = e.ui.clip_rect();
        e.ui.set_clip_rect(clip.intersect(list));
        let height = records.len() as f64 * (ASSET_SHAPE_ROW_HEIGHT + ASSET_ROW_GAP) as f64
            - ASSET_ROW_GAP as f64;
        let max = (height - list.height() as f64).max(0.);
        if list.contains(e.ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO)) {
            e.state.shapes.scroll = (e.state.shapes.scroll
                - e.ui.input(|i| i.smooth_scroll_delta.y) as f64)
                .clamp(0., max);
        }
        for (i, r) in records.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(
                    list.min.x,
                    list.min.y + i as f32 * (ASSET_SHAPE_ROW_HEIGHT + ASSET_ROW_GAP)
                        - e.state.shapes.scroll as f32,
                ),
                Vec2::new(
                    list.width() - if max > 0. { THICKNESS } else { 0. },
                    ASSET_SHAPE_ROW_HEIGHT,
                ),
            );
            if !row.intersects(list) {
                continue;
            }
            e.ui.painter().rect_filled(row, 0., t.window_border);
            e.ui.painter()
                .rect_filled(row.shrink(1.), 0., t.number_input_background);
            let preview = Rect::from_min_size(
                row.min + Vec2::splat(ASSET_ROW_PADDING),
                Vec2::splat(ASSET_SHAPE_PREVIEW),
            );
            let bg = mix(t.window_background, t.number_input_background, 0.62);
            e.ui.painter().rect_filled(preview, 0., t.window_border);
            e.ui.painter().rect_filled(preview.shrink(1.), 0., bg);
            if let Some(p) = preview_image(e, &r.id) {
                img::paint_generated(
                    e.ui.painter(),
                    &format!("shape-preview-{}-{}", r.id, r.updated_at),
                    &p,
                    preview.shrink(4.),
                    bg,
                );
            } else {
                img::paint_icon(
                    e.ui.painter(),
                    "warning_icon.png",
                    preview.shrink(4.),
                    bg,
                    img::Filter::None,
                );
            }
            let name = Rect::from_min_size(
                Pos2::new(preview.max.x + 9., row.min.y + 13.),
                Vec2::new((row.width() - 110.).max(0.), 15.),
            );
            text::draw_ellipsized(
                e.ui.painter(),
                name,
                &r.name,
                FontSpec::new(DIALOG_FONT_SIZE).bold(),
                t.menu_item_text,
            );
            text::draw_ellipsized(
                e.ui.painter(),
                name.translate(Vec2::new(0., 18.)),
                &format!("{} · {}", r.file_name, size(r.size)),
                FontSpec::new(ASSET_METADATA_SIZE),
                mix(t.menu_item_text, t.number_input_background, 0.64),
            );
            let remove_rect = Rect::from_min_size(
                Pos2::new(
                    row.max.x - ASSET_ROW_PADDING - ASSET_REMOVE_SIZE.0,
                    row.center().y - ASSET_REMOVE_SIZE.1 / 2.,
                ),
                Vec2::new(ASSET_REMOVE_SIZE.0, ASSET_REMOVE_SIZE.1),
            );
            if w::button(e, "", &format!("Remove {}", r.name), remove_rect, ready) {
                remove = Some(r.id.clone());
            }
            img::paint_icon(
                e.ui.painter(),
                "cancel_icon.png",
                remove_rect.shrink(4.),
                t.button,
                img::Filter::None,
            );
        }
        e.ui.set_clip_rect(clip);
        if max > 0. {
            Scrollbar::vertical(Rect::from_min_size(
                Pos2::new(list.max.x - THICKNESS, list.min.y),
                Vec2::new(THICKNESS, list.height()),
            ))
            .id_salt("installed-shapes")
            .show(
                e.ui,
                height,
                list.height() as f64,
                &mut e.state.shapes.scroll,
            );
        }
    }
    if clear {
        e.state.shapes.clear = Some(TaskDialog::show(TaskDialogSpec {
            title: "Clear Custom Shapes".into(),
            icon: Some("warning_icon.png"),
            message: Some(format!(
                "Remove all {} custom shapes from {}? This cannot be undone.",
                records.len(),
                crate::dialogs::about::PRODUCT
            )),
            cancel_value: "cancel",
            choices: vec![
                TaskChoice {
                    value: "clear",
                    title: "Clear All Shapes".into(),
                    description: Some(format!(
                        "Delete every custom shape stored by {}.",
                        crate::dialogs::about::PRODUCT
                    )),
                    icon: Some("trash_can.png"),
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    description: Some("Keep the custom shapes.".into()),
                    icon: Some("cancel_icon.png"),
                },
            ],
            ..Default::default()
        }));
    }
    let confirmed = e.state.shapes.clear.as_ref().and_then(|h| h.poll());
    if confirmed.is_some() {
        e.state.shapes.clear = None;
    }
    if let Some(m) = e.cx.asset_library.as_deref_mut() {
        if add || drop {
            m.pick("shapes");
        }
        if let Some(id) = remove {
            m.remove_shape(&id);
        }
        if confirmed == Some("clear") {
            m.clear_shapes();
        }
    }
}
fn preview_image(e: &EntryUi, id: &str) -> Option<std::sync::Arc<image::RgbaImage>> {
    use pdn_core::{
        canvas::{Canvas, Style},
        tool::shapes::{DashStyle, DrawType},
    };
    let shape = e.cx.app.shape_catalog.get(id)?;
    let s = 34.;
    let ratio = shape.aspect_ratio();
    let (w, h) = if ratio > 1. {
        (s, s / ratio)
    } else {
        (s * ratio, s)
    };
    let r = [1. + (s - w) / 2., 1. + (s - h) / 2., w, h];
    let data =
        e.cx.app
            .shape_catalog
            .render_data(id, r, s / 20., DrawType::Both, DashStyle::Solid)
            .ok()??;
    let mut canvas = Canvas::new(36, 36);
    if let Some((path, rule)) = data.interior {
        canvas.set_fill_style(Style::Color(ASSET_SHAPE_PREVIEW_COLORS.0.to_array()));
        canvas.fill_path2d(&path, rule);
    }
    if let Some((path, rule)) = data.outline {
        canvas.set_fill_style(Style::Color(ASSET_SHAPE_PREVIEW_COLORS.1.to_array()));
        canvas.fill_path2d(&path, rule);
    }
    let pixels = canvas.get_image_data(0, 0, 36, 36);
    Some(std::sync::Arc::new(image::RgbaImage::from_raw(
        36,
        36,
        pixels.data,
    )?))
}
fn size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024. * 1024.))
    }
}
fn show_errors(errors: &[String]) {
    TaskDialog::show(TaskDialogSpec {
        title: "Shape errors".into(),
        icon: Some("warning_icon.png"),
        message: Some(errors.join("\n")),
        cancel_value: "close",
        choices: vec![TaskChoice {
            value: "close",
            title: "Close".into(),
            description: Some("Return to shape settings.".into()),
            icon: Some("cancel_icon.png"),
        }],
        ..Default::default()
    });
}
