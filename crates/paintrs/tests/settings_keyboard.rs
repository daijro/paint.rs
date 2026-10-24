//! Task52: real Settings capture/persistence and delivered document-navigation events.
use egui::{Event, Key, Modifiers};
use egui_kittest::{Harness, kittest::Queryable};
use paintrs::PaintApp;
use pdn_core::action::UiRequest;

fn harness() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_max_steps(64)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h
}
fn key(h: &mut Harness<'_, PaintApp>, key: Key, modifiers: Modifiers) {
    h.key_press_modifiers(modifiers, key);
    h.run();
}
fn settings(h: &mut Harness<'_, PaintApp>, section: &str) {
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some(section.into())));
    h.run();
}
fn documents(h: &mut Harness<'_, PaintApp>, n: usize) -> Vec<u64> {
    while h.state().app.documents.len() < n {
        h.state_mut()
            .app
            .create_blank_document_in_new_workspace(32, 32, 96., false);
    }
    h.state_mut().app.set_active_document_workspace(Some(0));
    h.run();
    h.state()
        .app
        .documents
        .iter()
        .map(|w| w.document.id)
        .collect()
}
fn active(h: &Harness<'_, PaintApp>) -> u64 {
    h.state().app.active_ws().unwrap().document.id
}

#[test]
fn keyboard_capture_moves_binding_persists_and_escape_only_cancels_capture() {
    let mut h = harness();
    settings(&mut h, "keyboard");
    h.get_by_label("Search actions").click();
    h.run();
    h.get_by_label("Search actions").type_text("copy");
    h.run();
    h.get_by_label("Shortcut for Copy").click();
    h.run();
    key(&mut h, Key::N, Modifiers::CTRL);
    let prefs = h.state().prefs.get("paintdotjs.shortcuts.v1").unwrap();
    let stored: serde_json::Value = serde_json::from_str(prefs).unwrap();
    assert_eq!(
        stored,
        serde_json::json!({"menu.edit.copy":"Ctrl+N","menu.file.new":null})
    );
    h.get_by_label("Shortcut for Copy").click();
    h.run();
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(h.state().shell.dialogs.is_open());
    key(&mut h, Key::Escape, Modifiers::NONE);
    assert!(!h.state().shell.dialogs.is_open());
}
#[test]
fn diagnostics_copy_uses_platform_clipboard() {
    let mut h = harness();
    h.state_mut().shell.clipboard = paintrs::platform::Clipboard::in_memory();
    settings(&mut h, "diagnostics");
    h.get_by_label("Copy to clipboard").click();
    h.run();
    let copied = h.state_mut().shell.clipboard.read_text().unwrap();
    assert!(copied.starts_with("Application: "));
    assert!(copied.contains("\nPlatform: "));
}
#[test]
fn document_navigation_wraps_reorders_and_selects_ninth_not_last() {
    let mut h = harness();
    let ids = documents(&mut h, 12);
    key(&mut h, Key::Tab, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(active(&h), ids[11]);
    key(&mut h, Key::PageDown, Modifiers::CTRL);
    assert_eq!(active(&h), ids[0]);
    key(&mut h, Key::Num9, Modifiers::ALT);
    assert_eq!(active(&h), ids[8]);
    key(&mut h, Key::PageDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(h.state().app.documents[9].document.id, ids[8]);
    assert_eq!(active(&h), ids[8]);
    key(&mut h, Key::Num9, Modifiers::CTRL);
    assert_eq!(active(&h), ids[9]);
}
#[test]
fn document_navigation_uses_repeated_down_not_up_and_exact_modifiers() {
    let mut h = harness();
    let ids = documents(&mut h, 3);
    for m in [
        Modifiers::CTRL | Modifiers::ALT,
        Modifiers::CTRL | Modifiers::SHIFT,
        Modifiers::ALT | Modifiers::SHIFT,
    ] {
        key(&mut h, Key::Num2, m);
        assert_eq!(active(&h), ids[0]);
    }
    key(&mut h, Key::Num9, Modifiers::CTRL);
    assert_eq!(active(&h), ids[0]);
    for (repeat, want) in [(false, ids[1]), (true, ids[2]), (true, ids[0])] {
        h.event(Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat,
            modifiers: Modifiers::CTRL,
        });
        h.run();
        assert_eq!(active(&h), want);
    }
    h.event(Event::Key {
        key: Key::Tab,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::CTRL,
    });
    h.run();
    assert_eq!(active(&h), ids[0]);
}
#[test]
fn navigation_modal_isolation_and_all_five_outside_buttons() {
    let mut h = harness();
    let ids = documents(&mut h, 2);
    settings(&mut h, "ui");
    key(&mut h, Key::Num2, Modifiers::ALT);
    assert_eq!(active(&h), ids[0]);
    key(&mut h, Key::Escape, Modifiers::NONE);
    for button in [
        egui::PointerButton::Primary,
        egui::PointerButton::Middle,
        egui::PointerButton::Secondary,
        egui::PointerButton::Extra1,
        egui::PointerButton::Extra2,
    ] {
        let pos = egui::pos2(1278., 798.);
        h.event(Event::PointerMoved(pos));
        h.event(Event::PointerButton {
            pos,
            button,
            pressed: true,
            modifiers: Modifiers::NONE,
        });
        h.run();
        key(&mut h, Key::Tab, Modifiers::CTRL);
        assert_eq!(active(&h), ids[0]);
        key(&mut h, Key::Num2, Modifiers::ALT);
        assert_eq!(active(&h), ids[1]);
        h.event(Event::PointerButton {
            pos,
            button,
            pressed: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
        key(&mut h, Key::Num1, Modifiers::ALT);
        assert_eq!(active(&h), ids[0]);
    }
}

#[test]
fn r22_switch_changes_live_history_layer_and_document_highlights() {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .wgpu()
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h.state_mut()
        .settings
        .set("ui.translucentWindows", false.into());
    h.run();
    let positions = [
        h.state().shell.forms.windows[1].rect().min + egui::vec2(190., 36.),
        h.state().shell.forms.windows[2].rect().min + egui::vec2(100., 35.),
        egui::pos2(390., 8.),
    ];
    let hardware = h.render().unwrap();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("graphics".into())));
    h.run();
    h.get_by_label("Use hardware acceleration for the user interface")
        .click();
    h.run();
    assert!(!h.state().settings.get_bool("graphics.hardwareAcceleration"));
    h.key_press(Key::Escape);
    h.run();
    let software = h.render().unwrap();
    for p in positions {
        let xy = (p.x as u32, p.y as u32);
        assert_ne!(
            hardware.get_pixel(xy.0, xy.1),
            software.get_pixel(xy.0, xy.1),
            "R22 selected fill at {p:?}"
        );
    }
}

#[test]
fn cycle_observes_all_five_buttons_in_one_raw_frame() {
    for button in [
        egui::PointerButton::Primary,
        egui::PointerButton::Secondary,
        egui::PointerButton::Middle,
        egui::PointerButton::Extra1,
        egui::PointerButton::Extra2,
    ] {
        let mut h = harness();
        let ids = documents(&mut h, 2);
        let pos = egui::pos2(1278., 798.);
        let edge = |pressed| Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let command = || Event::Key {
            key: Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        };
        h.input_mut()
            .events
            .extend([Event::PointerMoved(pos), edge(true)]);
        h.run();
        h.input_mut().events.extend([command(), edge(false)]);
        h.run();
        assert_eq!(
            active(&h),
            ids[0],
            "{button:?}: later release must not unblock earlier command"
        );
        h.input_mut().events.extend([command(), edge(true)]);
        h.run();
        assert_eq!(
            active(&h),
            ids[1],
            "{button:?}: later press must not block earlier command"
        );
        h.input_mut().events.extend([edge(false), command()]);
        h.run();
        assert_eq!(
            active(&h),
            ids[0],
            "{button:?}: earlier release permits the command"
        );
        h.input_mut()
            .events
            .extend([edge(true), command(), edge(true)]);
        h.run();
        assert_eq!(
            active(&h),
            ids[0],
            "{button:?}: duplicate press is idempotent"
        );
        h.input_mut()
            .events
            .extend([edge(false), command(), edge(false)]);
        h.run();
        assert_eq!(
            active(&h),
            ids[1],
            "{button:?}: duplicate release is idempotent"
        );
    }
}

fn click_at(h: &mut Harness<'_, PaintApp>, pos: egui::Pos2) {
    h.event(Event::PointerMoved(pos));
    h.run();
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run();
    }
}
fn click_brush_size(h: &mut Harness<'_, PaintApp>) {
    let combo = h.state().shell.tool_menu.anchor("width").unwrap();
    click_at(h, combo.center());
}

#[test]
fn focused_numeric_editor_preserves_raw_frame_button_precedence() {
    let mut h = harness();
    let ids = documents(&mut h, 2);
    click_brush_size(&mut h);
    let pos = egui::pos2(1278., 798.);
    h.input_mut().events.extend([
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Extra2,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run();
    assert!(h.ctx.text_edit_focused());
    h.input_mut().events.extend([
        Event::Key {
            key: Key::PageDown,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        },
        Event::PointerButton {
            pos,
            button: egui::PointerButton::Extra2,
            pressed: false,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run();
    assert_eq!(active(&h), ids[0]);
    key(&mut h, Key::PageDown, Modifiers::CTRL);
    assert_eq!(active(&h), ids[1]);
}

#[test]
fn delivered_navigation_from_numeric_editor_focuses_new_canvas() {
    let mut h = harness();
    let ids = documents(&mut h, 2);
    click_brush_size(&mut h);
    assert!(h.ctx.text_edit_focused());
    key(&mut h, Key::Num2, Modifiers::CTRL);
    assert_eq!(active(&h), ids[1]);
    assert!(!h.ctx.text_edit_focused());
    assert_eq!(
        h.ctx.memory(|m| m.focused()),
        Some(egui::Id::unique("canvas-input"))
    );
}
#[test]
fn same_or_missing_document_does_not_deactivate_tool_and_zero_is_actual_size() {
    let mut h = harness();
    let id = active(&h);
    let generation = h.state().app.tool_generation;
    key(&mut h, Key::Tab, Modifiers::CTRL);
    key(&mut h, Key::Num1, Modifiers::ALT);
    assert_eq!(active(&h), id);
    assert_eq!(h.state().app.tool_generation, generation);
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .view
        .set_zoom_center(2.);
    key(&mut h, Key::Num0, Modifiers::CTRL);
    assert_eq!(active(&h), id);
    assert_eq!(h.state().app.active_ws().unwrap().view.zoom, 1.);
    h.state_mut().app.active = None;
    key(&mut h, Key::Tab, Modifiers::CTRL);
    assert!(h.state().app.active.is_none());
    key(&mut h, Key::Num1, Modifiers::ALT);
    assert_eq!(active(&h), id);
    h.state_mut().app.close_document_workspace(0);
    h.run();
    key(&mut h, Key::Tab, Modifiers::CTRL);
    key(&mut h, Key::Num1, Modifiers::ALT);
    assert!(h.state().app.documents.is_empty());
}
#[test]
fn direct_document_key_ends_old_stroke_and_later_release_does_not_paint_new_image() {
    let mut h = harness();
    h.state_mut().app.documents[0].is_initial_workspace = false;
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(800, 600, 96., false);
    h.state_mut().app.set_active_document_workspace(Some(0));
    h.run();
    let tool = pdn_core::tool::ToolType::PaintBrush;
    for (k, v) in [
        ("width", serde_json::json!(4)),
        ("hardness", serde_json::json!(100)),
        ("smoothing", false.into()),
        ("antialias", false.into()),
    ] {
        h.state_mut().app.tool_settings.set(tool, k, v);
    }
    let n = h.state().app.documents[0].history.undo.len();
    let a = egui::pos2(400., 300.);
    let b = egui::pos2(550., 300.);
    h.event(Event::PointerMoved(a));
    h.run();
    h.event(Event::PointerButton {
        pos: a,
        button: egui::PointerButton::Extra1,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    h.event(Event::PointerMoved(b));
    h.run();
    key(&mut h, Key::Num2, Modifiers::ALT);
    assert_eq!(h.state().app.active, Some(1));
    assert_eq!(h.state().app.documents[0].history.undo.len(), n + 1);
    let old = h.state().app.documents[0]
        .document
        .layers
        .get_at(0)
        .unwrap()
        .surface
        .data()
        .to_vec();
    assert!(
        old.as_chunks::<4>()
            .0
            .iter()
            .filter(|p| **p != [255; 4])
            .count()
            > 100
    );
    h.event(Event::PointerMoved(egui::pos2(650., 400.)));
    h.event(Event::PointerButton {
        pos: b,
        button: egui::PointerButton::Extra1,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run();
    assert!(
        h.state()
            .app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| *p == [255; 4])
    );
    key(&mut h, Key::Num1, Modifiers::ALT);
    assert_eq!(
        h.state()
            .app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data(),
        old
    );
    assert_eq!(h.state().app.active_ws().unwrap().history.undo.len(), n + 1);
}
#[test]
fn keyboard_clear_and_reset_all_keep_unknown_overrides() {
    let mut h = harness();
    h.state_mut().shortcuts = pdn_core::action::ShortcutRegistry::load(Some(
        r#"{"future.action":"Alt+F12","menu.edit.copy":"Ctrl+N","menu.file.new":null}"#,
    ));
    settings(&mut h, "keyboard");
    h.get_by_label("Search actions").click();
    h.run();
    h.get_by_label("Search actions").type_text("copy");
    h.run();
    h.get_by_label("Shortcut for Copy").click();
    h.run();
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert!(h.state().shortcuts.shortcut("menu.edit.copy").is_none());
    h.get_by_label("Reset all shortcuts").click();
    h.run();
    let stored: serde_json::Value =
        serde_json::from_str(h.state().prefs.get("paintdotjs.shortcuts.v1").unwrap()).unwrap();
    assert_eq!(stored, serde_json::json!({"future.action":"Alt+F12"}));
    assert_eq!(
        h.state()
            .shortcuts
            .shortcut("menu.file.new")
            .unwrap()
            .to_string()
            .as_deref(),
        Some("Ctrl+N")
    );
    assert_eq!(
        h.state()
            .shortcuts
            .shortcut("menu.edit.copy")
            .unwrap()
            .to_string()
            .as_deref(),
        Some("Ctrl+C")
    );
}
#[test]
fn captured_tab_is_a_shortcut_and_does_not_activate_dialog_buttons() {
    for (modifiers, combo) in [
        (Modifiers::NONE, "Tab"),
        (Modifiers::CTRL, "Ctrl+Tab"),
        (Modifiers::SHIFT, "Shift+Tab"),
    ] {
        let mut h = harness();
        settings(&mut h, "keyboard");
        h.get_by_label("Search actions").click();
        h.run();
        h.get_by_label("Search actions").type_text("copy");
        h.run();
        h.get_by_label("Shortcut for Copy").click();
        h.run();
        key(&mut h, Key::Tab, modifiers);
        assert_eq!(
            h.state()
                .shortcuts
                .shortcut("menu.edit.copy")
                .unwrap()
                .to_string()
                .as_deref(),
            Some(combo)
        );
        assert!(h.state().shell.dialogs.is_open());
        assert!(
            h.ctx.memory(|m| m.focused()).is_none(),
            "captured {combo} must not advance focus"
        );
    }
}

#[test]
fn selecting_keyboard_page_focuses_search() {
    use egui_kittest::kittest::NodeT;
    let mut h = harness();
    settings(&mut h, "ui");
    h.get_by_label("Keyboard Shortcuts").click();
    h.run();
    assert!(
        h.get_by_label("Search actions")
            .accesskit_node()
            .is_focused()
    );
}

#[test]
fn text_ctrl_tab_resets_pulse_before_navigation_but_outer_page_commands_do_not() {
    fn alpha_after(key_name: Key, m: Modifiers) -> u8 {
        let mut h = harness();
        h.input_mut().focused = true;
        h.input_mut()
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .focused = Some(true);
        h.state_mut().clock = paintrs::Clock::Virtual(0.);
        h.state_mut().settings.set("ui.animations", false.into());
        key(&mut h, Key::T, Modifiers::NONE);
        click_at(&mut h, egui::pos2(500., 350.));
        h.event(Event::Text("A".into()));
        h.run();
        // Prime the caret's visible phase before holding modifiers. This isolates key routing
        // from the separate rising-Ctrl/Shift hook and the initial unsampled pulse target.
        h.state_mut().advance_clock(16);
        h.run();
        h.event(Event::ModifiersChanged(m));
        h.run();
        h.state_mut().advance_clock(350);
        h.run();
        h.event(Event::Key {
            key: key_name,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: m,
        });
        h.run();
        h.state_mut().advance_clock(50);
        h.event(Event::ModifiersChanged(Modifiers::NONE));
        h.run();
        let app = &mut h.state_mut().app;
        app.with_tool(|tool, app| {
            tool.as_any_mut()
                .downcast_mut::<pdn_core::tool::text::TextTool>()
                .unwrap()
                .caret_alpha(app.now_ms)
        })
        .unwrap()
    }
    assert_eq!(alpha_after(Key::Tab, Modifiers::CTRL), 255);
    assert_eq!(alpha_after(Key::PageDown, Modifiers::CTRL), 0);
    assert_eq!(
        alpha_after(Key::PageUp, Modifiers::CTRL | Modifiers::SHIFT),
        0
    );
}
#[test]
fn keyboard_document_selection_reveals_thumbnail_and_reorder_respects_strip_capture() {
    use egui_kittest::kittest::NodeT;
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_max_steps(64)
        .with_size(egui::vec2(800., 600.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    let ids = documents(&mut h, 12);
    for (i, ws) in h.state_mut().app.documents.iter_mut().enumerate() {
        ws.file_name = Some(format!("Image {}", i + 1));
    }
    h.run();
    key(&mut h, Key::Num9, Modifiers::ALT);
    assert_eq!(active(&h), ids[8]);
    let ninth = h.get_by_label("Image 9");
    assert!(ninth.accesskit_node().is_selected().unwrap());
    // Desktop: 54-DIP item + 30-DIP caption reserve + two 1-DIP borders.
    assert_eq!(ninth.rect().width(), 86.);
    let before: Vec<_> = h
        .state()
        .app
        .documents
        .iter()
        .map(|w| w.document.id)
        .collect();
    let pos = ninth.rect().min + egui::vec2(4., 4.);
    h.event(Event::PointerMoved(pos));
    h.run();
    h.event(Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run();
    key(&mut h, Key::PageDown, Modifiers::CTRL | Modifiers::SHIFT);
    assert_eq!(
        h.state()
            .app
            .documents
            .iter()
            .map(|w| w.document.id)
            .collect::<Vec<_>>(),
        before
    );
}

#[test]
fn diagnostics_unavailable_clipboard_does_not_report_copy_success() {
    let mut h = harness();
    paintrs::kit::text::enable_text_recording(&h.ctx);
    settings(&mut h, "diagnostics");
    h.get_by_label("Copy to clipboard").click();
    h.run();
    let texts: Vec<_> = paintrs::kit::text::take_text_runs(&h.ctx)
        .into_iter()
        .filter_map(|r| r.2)
        .collect();
    assert!(texts.iter().any(|t| t.contains("unavailable")));
    assert!(!texts.iter().any(|t| t.contains("Diagnostics copied")));
}
#[test]
fn search_clear_button_restores_full_list_and_focus() {
    use egui_kittest::kittest::NodeT;
    let mut h = harness();
    settings(&mut h, "keyboard");
    h.get_by_label("Search actions").click();
    h.run();
    h.get_by_label("Search actions").type_text("layer");
    h.run();
    assert_eq!(
        h.get_by_label("Search actions")
            .accesskit_node()
            .value()
            .as_deref(),
        Some("layer")
    );
    assert!(h.query_by_label("Shortcut for Copy").is_none());
    h.get_by_label("Clear search").click();
    h.run();
    assert!(h.query_by_label("Shortcut for Copy").is_some());
    assert!(
        h.get_by_label("Search actions")
            .accesskit_node()
            .is_focused()
    );
}
#[test]
fn row_reset_restores_default_without_restoring_displaced_action() {
    use egui_kittest::kittest::NodeT;
    let mut h = harness();
    h.state_mut().shortcuts = pdn_core::action::ShortcutRegistry::load(Some(
        r#"{"menu.edit.copy":"Ctrl+N","menu.file.new":null}"#,
    ));
    settings(&mut h, "keyboard");
    h.get_by_label("Search actions").click();
    h.run();
    h.get_by_label("Search actions").type_text("copy");
    h.run();
    h.get_by_label("Reset shortcut for Copy").click();
    h.run();
    assert_eq!(
        h.state()
            .shortcuts
            .shortcut("menu.edit.copy")
            .unwrap()
            .to_string()
            .as_deref(),
        Some("Ctrl+C")
    );
    assert!(h.state().shortcuts.shortcut("menu.file.new").is_none());
    assert!(
        h.get_by_label("Reset shortcut for Copy")
            .accesskit_node()
            .is_disabled()
    );
}

#[test]
fn physical_button_survives_capture_loss_for_next_frame_navigation() {
    let mut h = harness();
    let ids = documents(&mut h, 2);
    let pos = egui::pos2(640., 400.);
    let edge = |pressed| Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    let command = || Event::Key {
        key: Key::Tab,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::CTRL,
    };
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), edge(true)]);
    h.run();
    assert_ne!(
        h.state().app.canvas_buttons,
        0,
        "must acquire canvas capture"
    );
    h.input_mut().events.push(Event::PointerGone);
    h.run_steps(1);
    assert_eq!(
        h.state().app.canvas_buttons,
        0,
        "PointerGone releases tool capture"
    );
    assert!(
        h.ctx.input(|i| i.pointer.primary_down()),
        "physical button is still held"
    );
    h.input_mut()
        .events
        .extend([Event::PointerMoved(pos), command()]);
    h.run_steps(1);
    assert_eq!(
        active(&h),
        ids[0],
        "capture loss cannot release the next-frame physical guard"
    );
    h.input_mut().events.extend([edge(false), command()]);
    h.run_steps(1);
    assert_eq!(
        active(&h),
        ids[1],
        "an actual physical release permits navigation"
    );
}

#[test]
fn terminal_capture_ends_before_later_keys_in_the_same_raw_frame() {
    for (first, modifiers, expected, later, open) in [
        (Key::Escape, Modifiers::NONE, Some("Ctrl+C"), Key::N, true),
        (Key::Delete, Modifiers::NONE, None, Key::N, true),
        (Key::L, Modifiers::CTRL, Some("Ctrl+L"), Key::N, true),
        (
            Key::Escape,
            Modifiers::NONE,
            Some("Ctrl+C"),
            Key::Escape,
            false,
        ),
    ] {
        let mut h = harness();
        settings(&mut h, "keyboard");
        h.get_by_label("Search actions").click();
        h.run();
        h.get_by_label("Search actions").type_text("copy");
        h.run();
        h.get_by_label("Shortcut for Copy").click();
        h.run();
        let event = |key, modifiers| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        h.input_mut().events.extend([
            event(first, modifiers),
            event(
                later,
                if later == Key::Escape {
                    Modifiers::NONE
                } else {
                    Modifiers::CTRL
                },
            ),
        ]);
        h.run();
        assert_eq!(
            h.state()
                .shortcuts
                .shortcut("menu.edit.copy")
                .and_then(|s| s.to_string())
                .as_deref(),
            expected,
            "terminal {first:?} must end that capture"
        );
        assert_eq!(
            h.state().shell.dialogs.is_open(),
            open,
            "later {later:?} must retain dialog handling"
        );
        assert_eq!(
            h.state().app.documents.len(),
            1,
            "modal later keys cannot create a document"
        );
    }
}

#[test]
fn strip_capture_at_reorder_key_respects_raw_event_order() {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary, Secondary};
    for focused in [false, true] {
        let mut observed = vec![];
        // Native §9.2: strip capture, rather than any physical button, blocks reorder.
        for case in [
            "press-key",
            "key-press",
            "prior-press",
            "release-key",
            "key-release",
            "press-release-key",
            "duplicate-press",
            "duplicate-release",
            "close",
            "empty",
            "outside",
            "secondary",
            "middle",
            "extra1",
            "extra2",
        ] {
            let mut h = harness();
            let ids = documents(&mut h, 2);
            for (i, w) in h.state_mut().app.documents.iter_mut().enumerate() {
                w.file_name = Some(format!("Order {}", i + 1));
            }
            h.run();
            if focused {
                click_brush_size(&mut h);
                assert!(h.ctx.text_edit_focused());
            }
            let tab = h.get_by_label("Order 1").rect();
            let pos = match case {
                "close" => egui::pos2(tab.max.x - 11., tab.min.y + 10.),
                "empty" => egui::pos2(tab.max.x + tab.width() + 5., tab.center().y),
                "outside" => egui::pos2(1278., 798.),
                _ => tab.min + egui::vec2(4., 4.),
            };
            let button = match case {
                "secondary" => Secondary,
                "middle" => Middle,
                "extra1" => Extra1,
                "extra2" => Extra2,
                _ => Primary,
            };
            let edge = |pressed| Event::PointerButton {
                pos,
                button,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let command = || Event::Key {
                key: Key::PageDown,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::CTRL | Modifiers::SHIFT,
            };
            h.input_mut().events.push(Event::PointerMoved(pos));
            h.run();
            if matches!(
                case,
                "prior-press" | "release-key" | "key-release" | "duplicate-release"
            ) {
                h.input_mut().events.push(edge(true));
                h.run();
            }
            let events = match case {
                "key-press" => vec![command(), edge(true)],
                "prior-press" => vec![command()],
                "release-key" => vec![edge(false), command()],
                "key-release" => vec![command(), edge(false)],
                "press-release-key" => vec![edge(true), edge(false), command()],
                "duplicate-press" => vec![edge(true), command(), edge(true)],
                "duplicate-release" => vec![edge(false), command(), edge(false)],
                _ => vec![edge(true), command()],
            };
            h.input_mut().events.extend(events);
            h.run_steps(1);
            let order: Vec<_> = h
                .state()
                .app
                .documents
                .iter()
                .map(|w| w.document.id)
                .collect();
            assert_eq!(
                active(&h),
                ids[0],
                "{focused}/{case}: reordering retains identity"
            );
            observed.push((case, order == [ids[1], ids[0]]));
        }
        assert_eq!(
            observed,
            vec![
                ("press-key", false),
                ("key-press", true),
                ("prior-press", false),
                ("release-key", true),
                ("key-release", false),
                ("press-release-key", true),
                ("duplicate-press", false),
                ("duplicate-release", true),
                ("close", true),
                ("empty", true),
                ("outside", true),
                ("secondary", true),
                ("middle", true),
                ("extra1", true),
                ("extra2", true)
            ],
            "focused={focused}: capture is evaluated at each delivered command"
        );
    }
}

#[test]
fn shortcut_capture_owns_keys_before_an_outside_press_in_raw_order() {
    let mut observed = vec![];
    for case in [
        "separate-key-press",
        "key-press",
        "press-key",
        "key-duplicate-press",
        "press-release-key",
    ] {
        let mut h = harness();
        settings(&mut h, "keyboard");
        h.get_by_label("Search actions").click();
        h.run();
        h.get_by_label("Search actions").type_text("copy");
        h.run();
        h.get_by_label("Shortcut for Copy").click();
        h.run();
        let pos = h.get_by_label("Search actions").rect().center();
        h.input_mut().events.push(Event::PointerMoved(pos));
        h.run();
        let edge = |pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let command = || Event::Key {
            key: Key::N,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        };
        if case == "separate-key-press" {
            h.input_mut().events.push(command());
            h.run_steps(1);
        }
        let events = match case {
            "separate-key-press" => vec![edge(true)],
            "key-press" => vec![command(), edge(true)],
            "key-duplicate-press" => vec![command(), edge(true), edge(true)],
            "press-release-key" => vec![edge(true), edge(false), command()],
            _ => vec![edge(true), command()],
        };
        h.input_mut().events.extend(events);
        h.run_steps(1);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{case}: no modal shortcut leakage"
        );
        assert!(
            h.get_by_label("Search actions").is_focused(),
            "{case}: later pointer owns search"
        );
        let copy = h
            .state()
            .shortcuts
            .shortcut("menu.edit.copy")
            .and_then(|s| s.to_string());
        let stored = h
            .state()
            .prefs
            .get("paintdotjs.shortcuts.v1")
            .map(|p| serde_json::from_str::<serde_json::Value>(p).unwrap());
        observed.push((case, copy, stored));
    }
    let moved = Some(serde_json::json!({"menu.edit.copy":"Ctrl+N","menu.file.new":null}));
    assert_eq!(
        observed,
        vec![
            ("separate-key-press", Some("Ctrl+N".into()), moved.clone()),
            ("key-press", Some("Ctrl+N".into()), moved.clone()),
            ("press-key", Some("Ctrl+C".into()), None),
            ("key-duplicate-press", Some("Ctrl+N".into()), moved),
            ("press-release-key", Some("Ctrl+C".into()), None)
        ],
        "capture owns a key until the outside press, not the frame's final focus"
    );
}

#[test]
fn new_shortcut_capture_starts_after_its_pointer_focus_event() {
    let mut observed = vec![];
    for case in ["key-click", "click-key", "key-click-key"] {
        let mut h = harness();
        settings(&mut h, "keyboard");
        h.get_by_label("Search actions").click();
        h.run();
        h.get_by_label("Search actions").type_text("copy");
        h.run();
        let pos = h.get_by_label("Shortcut for Copy").rect().center();
        h.input_mut().events.push(Event::PointerMoved(pos));
        h.run();
        assert!(h.get_by_label("Search actions").is_focused());
        let edge = |pressed| Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let command = |key| Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::CTRL,
        };
        let events = match case {
            "key-click" => vec![command(Key::N), edge(true), edge(false)],
            "click-key" => vec![edge(true), edge(false), command(Key::N)],
            _ => vec![command(Key::N), edge(true), edge(false), command(Key::P)],
        };
        h.input_mut().events.extend(events);
        h.run_steps(1);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{case}: Settings retains modal ownership"
        );
        assert_eq!(
            h.state().app.documents.len(),
            1,
            "{case}: the earlier key cannot leak to New"
        );
        assert_eq!(
            h.state()
                .shortcuts
                .shortcut("menu.file.new")
                .and_then(|s| s.to_string()),
            if case == "click-key" {
                None
            } else {
                Some("Ctrl+N".into())
            },
            "{case}: a pre-focus key must not displace New's binding"
        );
        observed.push((
            case,
            h.state()
                .shortcuts
                .shortcut("menu.edit.copy")
                .and_then(|s| s.to_string()),
            h.get_by_label("Shortcut for Copy").is_focused(),
        ));
    }
    assert_eq!(
        observed,
        vec![
            ("key-click", Some("Ctrl+C".into()), true),
            ("click-key", Some("Ctrl+N".into()), false),
            ("key-click-key", Some("Ctrl+P".into()), false)
        ],
        "a newly focused capture owns only keys after its pointer boundary"
    );
}

#[test]
fn tab_focus_acquires_capture_without_binding_the_navigation_tab() {
    let mut observed = vec![];
    for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
        let mut h = harness();
        settings(&mut h, "keyboard");
        h.get_by_label("Search actions").click();
        h.run();
        h.get_by_label("Search actions").type_text("copy");
        h.run();
        if modifiers.shift {
            h.state_mut().shortcuts.set_shortcut(
                "menu.edit.copy",
                Some(pdn_core::util::shortcut::ShortcutKey::from_combo("Ctrl+K")),
            );
            h.run();
            h.get_by_label("Reset shortcut for Copy").focus();
            h.run();
            assert!(h.get_by_label("Reset shortcut for Copy").is_focused());
        }
        key(&mut h, Key::Tab, modifiers);
        let binding = h
            .state()
            .shortcuts
            .shortcut("menu.edit.copy")
            .and_then(|s| s.to_string());
        let focused = h.get_by_label("Shortcut for Copy").is_focused();
        key(&mut h, Key::N, Modifiers::CTRL);
        let after = h
            .state()
            .shortcuts
            .shortcut("menu.edit.copy")
            .and_then(|s| s.to_string());
        assert!(h.state().shell.dialogs.is_open());
        observed.push((binding, focused, after));
    }
    assert_eq!(
        observed,
        vec![
            (Some("Ctrl+C".into()), true, Some("Ctrl+N".into())),
            (Some("Ctrl+K".into()), true, Some("Ctrl+N".into()))
        ],
        "the Tab/Shift+Tab that acquired focus is navigation; the next frame's key belongs to capture"
    );
}

#[test]
fn owned_shortcut_precedes_settings_navigation_and_close_in_raw_order() {
    let mut all = vec![];
    for target in ["graphics", "footer-close", "title-close"] {
        let mut observed = vec![];
        for case in [
            "separate-key-click",
            "key-click",
            "click-key",
            "key-click-escape",
        ] {
            let mut h = harness();
            settings(&mut h, "keyboard");
            h.get_by_label("Search actions").click();
            h.run();
            h.get_by_label("Search actions").type_text("copy");
            h.run();
            h.get_by_label("Shortcut for Copy").click();
            h.run();
            let pos = match target {
                "graphics" => h.get_by_label("Graphics").rect().center(),
                "footer-close" => h.get_by_label("Close").rect().center(),
                _ => h
                    .ctx
                    .read_response(
                        egui::Id::unique(("paintrs.dialog", 0_u64))
                            .with("paintrs.dialog_frame")
                            .with("close"),
                    )
                    .unwrap()
                    .rect
                    .center(),
            };
            h.input_mut().events.push(Event::PointerMoved(pos));
            h.run();
            let edge = |pressed| Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            let key = |key, modifiers| Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers,
            };
            if case == "separate-key-click" {
                h.input_mut().events.push(key(Key::N, Modifiers::CTRL));
                h.run_steps(1);
            }
            let events = match case {
                "separate-key-click" => vec![edge(true), edge(false)],
                "click-key" => vec![edge(true), edge(false), key(Key::N, Modifiers::CTRL)],
                "key-click-escape" => vec![
                    key(Key::N, Modifiers::CTRL),
                    edge(true),
                    edge(false),
                    key(Key::Escape, Modifiers::NONE),
                ],
                _ => vec![key(Key::N, Modifiers::CTRL), edge(true), edge(false)],
            };
            h.input_mut().events.extend(events);
            h.run_steps(1);
            assert_eq!(
                h.state().app.documents.len(),
                1,
                "{target}/{case}: modal input cannot leak to New"
            );
            assert_eq!(
                h.state().shell.settings_dialog.section(),
                if target == "graphics" {
                    "graphics"
                } else {
                    "keyboard"
                }
            );
            assert_eq!(
                h.state().shell.dialogs.is_open(),
                target == "graphics" && case != "key-click-escape",
                "{target}/{case}: navigation, close and later Escape retain their routing"
            );
            if target == "graphics" && case != "key-click-escape" {
                assert!(
                    h.get_by_label("Graphics").is_focused(),
                    "{case}: the click seats navigation focus"
                );
            }
            let binding = h
                .state()
                .shortcuts
                .shortcut("menu.edit.copy")
                .and_then(|s| s.to_string());
            let stored = h
                .state()
                .prefs
                .get("paintdotjs.shortcuts.v1")
                .map(|p| serde_json::from_str::<serde_json::Value>(p).unwrap());
            observed.push((binding, stored));
        }
        all.push((target, observed));
    }
    let moved = Some(serde_json::json!({"menu.edit.copy":"Ctrl+N","menu.file.new":null}));
    let expected = vec![
        (Some("Ctrl+N".into()), moved.clone()),
        (Some("Ctrl+N".into()), moved.clone()),
        (Some("Ctrl+C".into()), None),
        (Some("Ctrl+N".into()), moved),
    ];
    for (target, observed) in all {
        assert_eq!(
            observed, expected,
            "{target}: shared Settings routing must honor the capture's ordered prefix"
        );
    }
}
