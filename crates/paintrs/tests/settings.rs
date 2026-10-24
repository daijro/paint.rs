//! Task51 Settings behavior and retained import ownership.
use egui_kittest::Harness;
use paintrs::{
    PaintApp,
    theme::{self, Scheme},
};
use pdn_core::action::UiRequest;

fn harness() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h
}

#[test]
fn settings_request_opens_modal_and_escape_closes_it() {
    let mut h = harness();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(None));
    h.run();
    assert!(h.state().shell.dialogs.is_open());
    h.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(!h.state().shell.dialogs.is_open());
}

#[test]
fn stored_scheme_is_applied_live_to_shared_theme() {
    let mut h = harness();
    for (value, scheme) in [
        ("light", Scheme::Light),
        ("blue", Scheme::Blue),
        ("dark", Scheme::Dark),
    ] {
        h.state_mut().settings.set("ui.colorScheme", value.into());
        h.run();
        assert_eq!(theme::current(&h.ctx).scheme, scheme);
    }
}

#[test]
fn settings_defaults_and_font_choices_follow_native_contract() {
    use paintrs::dialogs::settings::{
        canvas::snap_brightness,
        ui::{displayed_ui_font, ui_font_choices},
    };
    assert_eq!(
        [0.73, 0.2, 1.04, 0.6, 0.62].map(snap_brightness),
        [0.75, 0.25, 1., 0.6, 0.6]
    );
    let choices = ui_font_choices(pdn_core::fonts::FontPolicy::Bundled);
    assert_eq!(choices[0].0, "Segoe UI");
    assert!(choices.iter().any(|c| c.0 == "Inter"));
    assert_eq!(displayed_ui_font(&choices, "Selawik"), "Noto Sans");
    assert_eq!(displayed_ui_font(&choices, "Segoe UI"), "Segoe UI");
    assert_eq!(
        choices,
        ui_font_choices(pdn_core::fonts::FontPolicy::System)
    );
}

#[test]
fn reset_tool_settings_keeps_shared_values_consistent() {
    use pdn_core::tool::types::ToolType;
    let mut h = harness();
    let app = &mut h.state_mut().app;
    app.tool_settings
        .set(ToolType::PaintBrush, "width", serde_json::json!(13));
    paintrs::dialogs::settings::tools::reset_all(&mut app.tool_settings, 1.0, true);
    for tool in [ToolType::PaintBrush, ToolType::Eraser, ToolType::Shapes] {
        assert_eq!(app.tool_settings.get(tool, "width"), serde_json::json!(2));
    }
    assert_eq!(
        app.tool_settings.get(ToolType::Recolor, "tolerance"),
        serde_json::json!(50)
    );
}

#[test]
fn retained_imports_survive_settings_close_in_fifo_order() {
    use egui_kittest::kittest::Queryable;
    use paintrs::platform::{PickedFile, files};
    let mut h = harness();
    let sources = vec![
        PickedFile::snapshot("same.xaml".into(), vec![1]),
        PickedFile::snapshot("same.xaml".into(), vec![2]),
        PickedFile::snapshot("pack.zip".into(), vec![3]),
        PickedFile::snapshot("font.ttf".into(), vec![4]),
        PickedFile::snapshot("image.png".into(), vec![5]),
    ];
    let ids: Vec<_> = sources.iter().map(|s| s.id).collect();
    let batch = files::dropped(sources);
    assert_eq!(
        batch
            .imports
            .iter()
            .map(|p| (p.page, p.files.len(), p.silent))
            .collect::<Vec<_>>(),
        [
            ("shapes", 2, false),
            ("assets", 1, false),
            ("fonts", 1, true)
        ]
    );
    assert_eq!(
        batch
            .imports
            .iter()
            .flat_map(|p| p.files.iter().map(|f| f.id))
            .collect::<Vec<_>>(),
        ids[..4]
    );
    assert_eq!(
        batch.imports[0].files[1]
            .reread()
            .poll()
            .unwrap()
            .unwrap()
            .bytes
            .as_ref(),
        [2]
    );
    assert_eq!(batch.images[0].id, ids[4]);
    h.state_mut().shell.drop_batches.push_back(batch);
    h.run_steps(1);
    assert!(h.query_by_label("Open").is_none());
    if h.state().shell.dialogs.top_is("settings") {
        for pressed in [true, false] {
            h.event(egui::Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(1);
        }
    }
    for _ in 0..500 {
        if h.query_by_label("Open").is_some() {
            break;
        }
        h.ctx.request_repaint();
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        h.query_by_label("Open").is_some(),
        "the image remainder must survive navigation and closure"
    );
    h.get_by_label("Cancel").click();
    h.run_steps(1);
    assert_eq!(
        h.state().app.documents.len(),
        1,
        "Cancel must not replay the image remainder"
    );
}

fn key(h: &mut Harness<'_, PaintApp>, key: egui::Key, modifiers: egui::Modifiers) {
    for pressed in [true, false] {
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        h.run();
    }
}
fn click(h: &mut Harness<'_, PaintApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    }
}
fn open(h: &mut Harness<'_, PaintApp>, section: &str) {
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some(section.into())));
    h.run();
}

#[test]
fn settings_all_sections_select_one_dialog_and_unknown_falls_back() {
    let mut h = harness();
    for section in [
        "ui",
        "canvas",
        "tools",
        "pen",
        "graphics",
        "colorManagement",
        "diagnostics",
        "fonts",
        "shapes",
        "assets",
        "keyboard",
        "unknown",
    ] {
        open(&mut h, section);
        assert_eq!(
            h.state().shell.settings_dialog.section(),
            if section == "unknown" { "ui" } else { section }
        );
        assert_eq!(
            h.state().shell.dialogs.top_rect(&h.ctx).unwrap().size(),
            egui::vec2(874., 670.)
        );
    }
    key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(!h.state().shell.dialogs.is_open());
}
#[test]
fn ui_checkbox_space_release_is_one_stop_and_persists() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    open(&mut h, "ui");
    h.get_by_label("Animations").click();
    h.run();
    assert!(!h.state().settings.get_bool("ui.animations"));
    let animations = h.ctx.memory(|m| m.focused());
    key(&mut h, egui::Key::Tab, egui::Modifiers::NONE);
    assert_ne!(h.ctx.memory(|m| m.focused()), animations);
    h.get_by_label("Animations").click();
    h.run();
    let before = h.state().settings.get_bool("ui.animations");
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().settings.get_bool("ui.animations"), before);
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_ne!(h.state().settings.get_bool("ui.animations"), before);
    let stored: serde_json::Value = serde_json::from_str(
        h.state()
            .prefs
            .get(pdn_core::settings::STORAGE_KEY)
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        stored["ui"]["animations"],
        h.state().settings.get("ui.animations").clone()
    );
}
#[test]
fn canvas_editor_changes_surround_and_brightness_is_native_step() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    open(&mut h, "canvas");
    h.get_by_label("Use a custom color for the canvas border")
        .click();
    h.run();
    h.get_by_label("R:").click();
    h.run();
    key(&mut h, egui::Key::ArrowUp, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.get_str("canvas.borderColor"), "#818080");
    let slider = h.get_by_label("Checkerboard brightness slider").rect();
    click(&mut h, slider.center());
    let before = h.state().settings.get_f64("canvas.checkerboardBrightness");
    key(&mut h, egui::Key::ArrowRight, egui::Modifiers::NONE);
    assert_eq!(
        h.state().settings.get_f64("canvas.checkerboardBrightness"),
        before + 5.
    );
}
#[test]
fn tool_reset_button_persists_defaults() {
    use egui_kittest::kittest::Queryable;
    use pdn_core::tool::types::ToolType;
    let mut h = harness();
    open(&mut h, "tools");
    h.state_mut()
        .app
        .tool_settings
        .set(ToolType::PaintBrush, "width", serde_json::json!(19));
    h.get_by_label("Reset tool settings").click();
    h.run();
    let stored: serde_json::Value = serde_json::from_str(
        h.state()
            .prefs
            .get("paintdotjs.toolSettings.eraserTool")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(stored["width"], 2);
}

#[test]
fn settings_close_space_activates_on_release() {
    let mut h = harness();
    open(&mut h, "ui");
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(
        h.state().shell.dialogs.is_open(),
        "Close waits for Space release"
    );
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(!h.state().shell.dialogs.is_open());
}
#[test]
fn system_theme_is_observed_only_for_default_scheme() {
    let mut h = harness();
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();
    assert_eq!(theme::current(&h.ctx).scheme, Scheme::Light);
    h.input_mut().system_theme = Some(egui::Theme::Dark);
    h.run();
    assert_eq!(theme::current(&h.ctx).scheme, Scheme::Dark);
    h.state_mut().settings.set("ui.colorScheme", "blue".into());
    h.input_mut().system_theme = Some(egui::Theme::Light);
    h.run();
    assert_eq!(theme::current(&h.ctx).scheme, Scheme::Blue);
}
#[test]
fn silent_font_drop_keeps_its_source_and_image_open_continuation_runs_once() {
    use egui_kittest::kittest::Queryable;
    use paintrs::platform::{PickedFile, files};
    let mut h = harness();
    let before = h.state().app.active_ws().unwrap().id;
    let image = PickedFile::snapshot(
        "image.png".into(),
        include_bytes!("../../../tests/fixtures/a.png").to_vec(),
    );
    {
        let s = h.state_mut();
        s.shell.drop_batches.push_back(files::dropped(vec![
            PickedFile::snapshot("font.ttf".into(), pdn_core::fonts::CARLITO_REGULAR.to_vec()),
            image,
        ]));
    }
    h.run_steps(1);
    assert!(
        h.state().fonts.importing(),
        "the font import is still in flight"
    );
    assert!(
        h.query_by_label("Open").is_none(),
        "the image choice must wait for the font import"
    );
    assert!(!h.state().shell.settings_dialog.is_open());
    assert!(h.state().shell.settings_imports.is_empty());
    for _ in 0..30 {
        if !h.state().fonts.importing() && h.query_by_label("Open").is_some() {
            break;
        }
        h.ctx.request_repaint();
        h.run();
    }
    assert!(!h.state().fonts.importing());
    h.get_by_label("Open").click();
    h.run();
    let opened = h.state().app.active_ws().unwrap().id;
    assert_ne!(opened, before);
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 13);
    h.run();
    assert_eq!(h.state().app.active_ws().unwrap().id, opened);
    for _ in 0..20 {
        if !h.state().fonts.busy() {
            break;
        }
        h.ctx.request_repaint();
        h.run();
    }
    assert!(h.state().shell.settings_imports.is_empty());
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    assert_eq!(h.state().fonts.registry.records[0].file_name, "font.ttf");
}
#[test]
fn bundled_font_selection_changes_setting_and_disabled_reload_cannot_activate() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = harness();
    open(&mut h, "ui");
    assert!(h.get_by_label("Reload now").accesskit_node().is_disabled());
    h.get_by_label("UI font:").click();
    h.run();
    h.get_by_label("Inter").click();
    h.run();
    assert_eq!(h.state().settings.get_str("ui.fontFamily"), "Inter");
}
#[test]
fn canvas_custom_surround_has_no_shadow() {
    let style = pdn_core::render::view::CanvasStyle::for_scheme(
        false,
        false,
        Some([255, 136, 0, 255]),
        0.4,
        pdn_core::render::display::RenderMode::Hardware,
    );
    assert_eq!(style.surround, [255, 136, 0, 255]);
    assert_eq!(style.shadow, None);
}

#[test]
fn canvas_border_brightness_range_accepts_keyboard_input() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    open(&mut h, "canvas");
    h.get_by_label("Use a custom color for the canvas border")
        .click();
    h.run();
    click(&mut h, egui::pos2(740., 230.));
    let before = h.state().settings.get_str("canvas.borderColor").to_owned();
    key(&mut h, egui::Key::ArrowUp, egui::Modifiers::NONE);
    assert_ne!(h.state().settings.get_str("canvas.borderColor"), before);
}
#[test]
fn settings_shell_fits_a_small_viewport() {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(640., 480.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    open(&mut h, "ui");
    let rect = h.state().shell.dialogs.top_rect(&h.ctx).unwrap();
    assert_eq!(rect.size(), egui::vec2(608., 448.));
    assert!(
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(640., 480.)).contains_rect(rect)
    );
}

#[test]
fn settings_font_select_supports_last_choice_by_keyboard() {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    open(&mut h, "ui");
    h.get_by_label("UI font:").click();
    h.run();
    key(&mut h, egui::Key::End, egui::Modifiers::NONE);
    assert!(
        h.ctx
            .content_rect()
            .contains_rect(h.get_by_label("System Monospace").rect())
    );
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.get_str("ui.fontFamily"), "monospace");
}

#[test]
fn animations_disabled_suppresses_modal_attention() {
    let mut h = harness();
    h.state_mut().settings.set("ui.animations", false.into());
    open(&mut h, "ui");
    click(&mut h, egui::pos2(10., 400.));
    assert!(!h.state().shell.dialogs.attention_active(&h.ctx));
}

#[test]
fn light_zoom_selection_still_accepts_keyboard_replacement() {
    let mut h = harness();
    h.state_mut().settings.set("ui.colorScheme", "light".into());
    h.run();
    click(&mut h, egui::pos2(1022., 784.));
    assert!(h.state().shell.text_input_focused());
    h.event(egui::Event::Text("250".into()));
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert!(!h.state().shell.text_input_focused());
    assert_eq!(h.state().app.active_ws().unwrap().view.zoom, 2.5);
}
#[test]
fn settings_long_font_popup_scrolls_with_keyboard_in_small_viewport() {
    use egui_kittest::kittest::Queryable;
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(640., 480.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    open(&mut h, "ui");
    h.get_by_label("UI font:").click();
    h.run();
    key(&mut h, egui::Key::End, egui::Modifiers::NONE);
    assert!(
        h.ctx
            .content_rect()
            .contains_rect(h.get_by_label("System Monospace").rect())
    );
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.get_str("ui.fontFamily"), "monospace");
}

#[test]
fn open_font_popup_owns_wheel_until_closed() {
    use egui_kittest::kittest::Queryable;
    fn wheel(h: &mut Harness<'_, PaintApp>, at: egui::Pos2) {
        h.event(egui::Event::PointerMoved(at));
        h.run();
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            source: egui::MouseWheelSource::Wheel,
            phase: egui::TouchPhase::Move,
            delta: egui::vec2(0., -40.),
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(24);
    }
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(640., 480.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    open(&mut h, "ui");
    let page_before = h.get_by_label("Animations").rect();
    let nav_before = h.get_by_label("User Interface").rect();
    h.get_by_label("UI font:").click();
    h.run();
    let option_before = h.get_by_label("Open Sans").rect();
    wheel(&mut h, egui::pos2(300., 200.));
    assert!(
        h.get_by_label("Open Sans").rect().top() < option_before.top(),
        "The open list itself still scrolls"
    );
    wheel(&mut h, egui::pos2(80., 200.));
    key(&mut h, egui::Key::Escape, egui::Modifiers::NONE);
    assert!(h.state().shell.settings_dialog.is_open());
    assert_eq!(
        h.get_by_label("Animations").rect(),
        page_before,
        "Wheel over the popup must not scroll its page"
    );
    assert_eq!(
        h.get_by_label("User Interface").rect(),
        nav_before,
        "The popup also owns wheel input over navigation"
    );
    wheel(&mut h, egui::pos2(300., 200.));
    assert!(
        h.get_by_label("Animations").rect().top() < page_before.top(),
        "Ordinary page scrolling resumes after the popup closes"
    );
}

#[test]
fn keyboard_tabs_reveal_clipped_navigation_and_page_controls() {
    use egui_kittest::kittest::{NodeT, Queryable};
    fn tab_to(h: &mut Harness<'_, PaintApp>, label: &str, reverse: bool) {
        for _ in 0..32 {
            key(
                h,
                egui::Key::Tab,
                if reverse {
                    egui::Modifiers::SHIFT
                } else {
                    egui::Modifiers::NONE
                },
            );
            if let Some(node) = h.query_by_label(label)
                && node.accesskit_node().is_focused()
            {
                let rect = node.rect();
                let outer = h.state().shell.dialogs.top_rect(&h.ctx).unwrap();
                let body = egui::Rect::from_min_max(
                    outer.min
                        + egui::vec2(
                            theme::DIALOG_BORDER,
                            theme::DIALOG_BORDER + theme::SETTINGS_TITLE.0,
                        ),
                    outer.max
                        - egui::vec2(
                            theme::DIALOG_BORDER,
                            theme::DIALOG_BORDER + theme::SETTINGS_FOOTER.0,
                        ),
                );
                assert!(
                    body.contains_rect(rect),
                    "Focused {label} must be fully visible in the Settings body: {rect:?}"
                );
                return;
            }
        }
        panic!("Keyboard navigation must reach {label}, reverse={reverse}");
    }
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(640., 480.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    open(&mut h, "ui");
    assert!(h.get_by_label("Close").accesskit_node().is_focused());
    // All interactions below are real keys, with no pointer or requested focus.
    tab_to(&mut h, "Keyboard Shortcuts", false);
    assert_eq!(h.state().shell.settings_dialog.section(), "ui");
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(
        h.state().shell.settings_dialog.section(),
        "ui",
        "Navigation waits for Space release"
    );
    h.event(egui::Event::Key {
        key: egui::Key::Space,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().shell.settings_dialog.section(), "keyboard");
    tab_to(&mut h, "User Interface", true);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(h.state().shell.settings_dialog.section(), "ui");
    tab_to(&mut h, "Language:", true);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    key(&mut h, egui::Key::End, egui::Modifiers::NONE);
    key(&mut h, egui::Key::Enter, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.get_str("ui.language"), "en");
    tab_to(&mut h, "Animations", true);
    let before = h.state().settings.get_bool("ui.animations");
    key(&mut h, egui::Key::Space, egui::Modifiers::NONE);
    assert_eq!(h.state().settings.get_bool("ui.animations"), !before);
    tab_to(&mut h, "Language:", false);
    tab_to(&mut h, "Keyboard Shortcuts", true);
    assert_eq!(
        h.state().shell.settings_dialog.section(),
        "ui",
        "Tab alone preserves the selected section"
    );
}
