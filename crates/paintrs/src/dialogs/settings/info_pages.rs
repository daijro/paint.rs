//! SettingsPenEntry, SettingsGraphicsEntry, SettingsColorManagementEntry, SettingsDiagnosticsEntry
//! and css/window.css. Reference: paintdotjs b4e6fc5. Display switch: spec §0.10 R22,
//! canvas-rendering §2.4; diagnostic value exclusion: Ruling (P3) 6.
use super::{EntryUi, mix};
use crate::{
    kit::{
        px,
        text::{self, FontSpec},
    },
    theme::{self, *},
};
use egui::{Pos2, Rect, Vec2};

pub(super) fn pen(e: &mut EntryUi) {
    e.add_heading("Pen & Tablet", "");
    e.add_checkbox(
        "pen.pointerInput",
        "Enable pen and tablet input",
        SETTINGS_CHECKBOX_ROWS.0,
    );
    e.add_note(
        "Pressure information is used by compatible drawing tools when the platform provides it.",
    );
    e.add_note("Pointer input is available in this application.");
    if e.add_button(
        "Open Windows Pen & Ink settings",
        cfg!(target_os = "windows"),
    ) {
        e.ui.ctx()
            .open_url(egui::OpenUrl::new_tab("ms-settings:pen"));
    }
}
pub(super) fn graphics(e: &mut EntryUi) {
    e.add_heading("Graphics", "");
    e.add_checkbox(
        "graphics.hardwareAcceleration",
        "Use hardware acceleration for the user interface",
        SETTINGS_CHECKBOX_ROWS.0,
    );
    e.add_note("Graphics acceleration is available.");
    disabled_device(e);
    e.add_note("Rendering-device selection is not available.");
}
pub(super) fn color_management(e: &mut EntryUi) {
    e.add_heading("Color Management", "");
    e.add_checkbox_enabled(
        "colorManagement.advancedColor",
        "Use Windows advanced color",
        SETTINGS_CHECKBOX_ROWS.0,
        false,
    );
    e.add_note("Direct Windows advanced-color control is not available.");
    if e.add_button("Open Windows display settings", cfg!(target_os = "windows")) {
        e.ui.ctx()
            .open_url(egui::OpenUrl::new_tab("ms-settings:display"));
    }
    e.add_section_heading("Status");
    e.add_note("Canvas colors are rendered in the sRGB color space.");
}
fn disabled_device(e: &mut EntryUi) {
    let t = theme::current(e.ui.ctx());
    let mut t = t;
    t.menu_item_text = mix(t.menu_item_text, t.window_background, SETTINGS_MIX.3);
    t.pdn_control_border = mix(t.pdn_control_border, t.window_background, SETTINGS_MIX.3);
    t.number_input_background = mix(
        t.number_input_background,
        t.window_background,
        SETTINGS_MIX.3,
    );
    let narrow = e.ui.ctx().content_rect().width() <= SETTINGS_BREAKPOINT;
    let label = "Rendering device:";
    text::draw(
        e.ui.painter(),
        Pos2::new(e.rect.min.x, e.y + if narrow { 0. } else { 11.5 }),
        label,
        FontSpec::new(DIALOG_FONT_SIZE),
        t.menu_item_text,
    );
    let x = if narrow {
        e.rect.min.x
    } else {
        e.rect.min.x + text::css_width(label, DIALOG_FONT_SIZE) + SETTINGS_FIELD.0
    };
    let r = Rect::from_min_size(
        Pos2::new(
            x,
            e.y + if narrow {
                SETTINGS_FIELD_NARROW.0
            } else {
                SETTINGS_FIELD.3
            },
        ),
        Vec2::new(SETTINGS_FIELD.1, SETTINGS_FIELD.2),
    );
    let response = e.ui.interact(
        r,
        e.ui.scope_id().with("rendering-device"),
        egui::Sense::hover(),
    );
    e.ui.ctx().accesskit_node_builder(response.id, |n| {
        n.set_role(egui::accesskit::Role::ComboBox);
        n.set_label(label);
        n.set_value("Automatic");
        n.set_disabled();
    });
    e.ui.painter().rect_filled(r, 0., t.pdn_control_border);
    e.ui.painter()
        .rect_filled(r.shrink(1.), 0., t.number_input_background);
    text::draw(
        e.ui.painter(),
        r.min + Vec2::new(5., 6.5),
        "Automatic",
        FontSpec::new(DIALOG_FONT_SIZE),
        t.menu_item_text,
    );
    for (j, row) in SELECT_ARROW.iter().enumerate() {
        for (i, &a) in row.iter().enumerate() {
            if a > 0 {
                e.ui.painter().rect_filled(
                    Rect::from_min_size(
                        Pos2::new(
                            r.max.x - SELECT_ARROW_RIGHT + i as f32,
                            r.min.y + 11. + j as f32,
                        ),
                        Vec2::splat(1.),
                    ),
                    0.,
                    crate::kit::aa::blit_aa(t.menu_item_text, a, t.number_input_background),
                );
            }
        }
    }
    e.y += if narrow {
        SETTINGS_FIELD_NARROW.1
    } else {
        SETTINGS_FIELD.4
    };
}

fn diagnostic_values() -> Vec<(&'static str, String)> {
    let value = |s: &str| s.to_string();
    let mut rows = vec![
        (
            "Application",
            format!("paint.rs {}", env!("CARGO_PKG_VERSION")),
        ),
        (
            "User agent",
            format!(
                "paint.rs / {} / {}",
                std::env::consts::OS,
                std::env::consts::ARCH
            ),
        ),
        ("Platform", value(std::env::consts::OS)),
        (
            "Language",
            std::env::var("LANG").unwrap_or_else(|_| "Unknown".into()),
        ),
        (
            "Logical processors",
            std::thread::available_parallelism()
                .map_or_else(|_| value("Unknown"), |n| n.to_string()),
        ),
        ("Device memory", value("Unknown")),
        ("Pointer events", value("Available")),
        ("WebGL", value("Not used")),
        ("Electron", value("No")),
    ];
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        if let Some(window) = web_sys::window() {
            let navigator = js_sys::Reflect::get(&window, &JsValue::from_str("navigator")).ok();
            for (index, key) in [
                (1, "userAgent"),
                (2, "platform"),
                (3, "language"),
                (4, "hardwareConcurrency"),
                (5, "deviceMemory"),
            ] {
                if let Some(v) = navigator
                    .as_ref()
                    .and_then(|n| js_sys::Reflect::get(n, &JsValue::from_str(key)).ok())
                {
                    rows[index].1 = v
                        .as_string()
                        .or_else(|| {
                            v.as_f64().map(|v| {
                                if index == 5 {
                                    format!("{v} GB")
                                } else {
                                    v.to_string()
                                }
                            })
                        })
                        .unwrap_or_else(|| "Unknown".into());
                }
            }
        }
    }
    // Keep mutation target-independent for the wasm enrichment above.
    rows.shrink_to_fit();
    rows
}

pub(super) fn diagnostics(e: &mut EntryUi) {
    if e.state.diagnostics_copy_pending {
        if let Some(result) =
            e.cx.clipboard
                .as_deref_mut()
                .and_then(|c| c.poll_text_write())
        {
            e.state.diagnostics_copy_pending = false;
            e.state.status = match result {
                Ok(()) => "Diagnostics copied to the clipboard.".into(),
                Err(error) => format!("Clipboard unavailable: {error}"),
            };
        } else {
            e.ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(16));
        }
    }
    e.add_heading("Diagnostics", "");
    let rows = diagnostic_values();
    let t = theme::current(e.ui.ctx());
    let face = ttf_parser::Face::parse(pdn_core::canvas::text::NOTO_SANS_MONO, 0)
        .expect("bundled mono face");
    let units = face.units_per_em() as f32;
    let font = FontSpec::new(DIALOG_FONT_SIZE).face(
        "diagnostics-mono",
        face.ascender() as f32 / units,
        -face.descender() as f32 / units,
    );
    let table = Rect::from_min_size(
        Pos2::new(e.rect.min.x, e.y),
        Vec2::new(e.rect.width() + 2., DIAGNOSTICS_TABLE_HEIGHT),
    );
    let painter = e.ui.painter();
    painter.rect_filled(table, 0., t.pdn_menu_border);
    painter.rect_filled(table.shrink(1.), 0., t.number_input_background);
    let mut y = table.min.y + 1.;
    for (i, (label, value)) in rows.iter().enumerate() {
        let h = if i == 1 {
            DIAGNOSTICS_USER_AGENT_HEIGHT
        } else {
            DIAGNOSTICS_ROW
        };
        let key = Rect::from_min_size(
            Pos2::new(table.min.x + 1., y),
            Vec2::new(DIAGNOSTICS_COLUMN, h),
        );
        let val = Rect::from_min_max(Pos2::new(key.max.x, y), Pos2::new(table.max.x - 1., y + h));
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(key.max.x - 1., y), Vec2::new(1., h)),
            0.,
            px::over(t.number_input_background, t.window_border),
        );
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(key.min.x, y + h - 1.),
                Vec2::new(table.width() - 2., 1.),
            ),
            0.,
            px::over(t.number_input_background, t.window_border),
        );
        text::draw(
            painter,
            key.min + Vec2::new(DIAGNOSTICS_PAD.0, DIAGNOSTICS_PAD.1),
            label,
            font,
            mix(t.menu_item_text, t.number_input_background, SETTINGS_MIX.0),
        );
        let clip = painter.with_clip_rect(val.intersect(e.ui.clip_rect()));
        text::draw_pdn(
            &clip,
            val.min + Vec2::new(DIAGNOSTICS_PAD.0, DIAGNOSTICS_PAD.1),
            value,
            font,
            t.menu_item_text,
        );
        // Ruling (P3) 6: environment values vary; labels and grid remain judged chrome.
        text::record_pdn_rect(e.ui.ctx(), val.intersect(e.ui.clip_rect()));
        y += h;
    }
    e.horizontal_overflow();
    e.y = table.max.y + 10.;
    if e.add_button("Copy to clipboard", true) {
        let text = rows
            .iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(clipboard) = e.cx.clipboard.as_deref_mut() {
            clipboard.write_text(&text);
            e.state.diagnostics_copy_pending = true;
            e.state.status = "Copying diagnostics…".into();
            e.ui.ctx().request_repaint();
        } else {
            e.state.status = "Clipboard unavailable.".into();
        }
    }
}
