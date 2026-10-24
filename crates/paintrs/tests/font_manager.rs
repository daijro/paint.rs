use paintrs::platform::{
    Files, PickedFile,
    fonts::{FontManager, FontStore},
};
use pdn_core::fonts::{FaceStyle, FontPolicy, registered_face};
fn file(name: &str, bytes: &[u8]) -> PickedFile {
    PickedFile::snapshot(name.into(), bytes.to_vec())
}
fn drain(m: &mut FontManager) {
    for _ in 0..100 {
        m.poll();
        if !m.busy() {
            return;
        }
    }
    panic!("font work did not finish");
}
#[test]
fn imports_validate_before_storage_keep_unique_families_and_remove_registration() {
    let store = FontStore::memory();
    let mut m = FontManager::new(store.clone(), FontPolicy::Bundled);
    drain(&mut m);
    m.import(vec![
        file("Good.ttf", pdn_core::fonts::SELAWIK_REGULAR),
        file("bad.ttf", b"bad"),
        file("Good.ttf", pdn_core::fonts::CARLITO_REGULAR),
    ]);
    drain(&mut m);
    assert_eq!(m.registry.records.len(), 2);
    let records = m.registry.records.clone();
    assert_ne!(records[0].family, records[1].family);
    assert!(m.status.contains("1 could not be added"), "{}", m.status);
    assert!(registered_face(&records[0].family, FaceStyle::Regular).is_some());
    let mut restored = FontManager::new(store, FontPolicy::Bundled);
    drain(&mut restored);
    assert_eq!(restored.registry.records.len(), 2);
    m.remove(&records[0].id);
    drain(&mut m);
    assert!(registered_face(&records[0].family, FaceStyle::Regular).is_none());
    assert_eq!(m.registry.records.len(), 1);
}
#[test]
fn failed_storage_never_installs_or_loses_metadata() {
    let store = FontStore::memory();
    let mut m = FontManager::new(store.clone(), FontPolicy::Bundled);
    drain(&mut m);
    store.fail_writes(true);
    m.import(vec![file("Good.ttf", pdn_core::fonts::SELAWIK_REGULAR)]);
    drain(&mut m);
    assert!(m.registry.records.is_empty());
    store.fail_writes(false);
    m.import(vec![file("Good.ttf", pdn_core::fonts::SELAWIK_REGULAR)]);
    drain(&mut m);
    let old = m.registry.records.clone();
    store.fail_writes(true);
    m.remove(&old[0].id);
    drain(&mut m);
    assert_eq!(m.registry.records, old);
    assert!(registered_face(&old[0].family, FaceStyle::Regular).is_some());
}
#[test]
fn pending_picker_and_native_bundle_use_original_bytes() {
    let files = Files::in_memory();
    files.queue_pick(Ok(vec![file(
        "Picked.ttf",
        pdn_core::fonts::CARLITO_REGULAR,
    )]));
    let mut m = FontManager::new(FontStore::memory(), FontPolicy::Bundled);
    drain(&mut m);
    m.pick(&files);
    drain(&mut m);
    assert_eq!(m.registry.records.len(), 1);
    m.ensure_family("Inter", FaceStyle::Regular);
    drain(&mut m);
    let bytes = registered_face("Inter", FaceStyle::Regular).unwrap();
    let expected = include_bytes!("../../../assets/font/inter/Inter-Variable.ttf");
    assert_eq!(&*bytes, expected);
    let old_status = m.status.clone();
    files.queue_pick(Ok(vec![]));
    m.pick(&files);
    drain(&mut m);
    assert_eq!(
        m.status, old_status,
        "Chooser cancellation must not report an import failure"
    );
    m.ensure_family("Anton", FaceStyle::Regular);
    drain(&mut m);
    assert!(registered_face("Anton", FaceStyle::Regular).is_some());
    m.ensure_family("Noto Color Emoji", FaceStyle::Regular);
    drain(&mut m);
    assert!(registered_face("Noto Color Emoji", FaceStyle::Regular).is_some());
    // R183 follow-up: each script fallback loads its bundled file under its private name; the
    // public family is never served from it, so an installed font of that name keeps its faces.
    for (family, file, _) in pdn_core::fonts::SCRIPT_FALLBACKS {
        m.ensure_family(family, FaceStyle::Regular);
        drain(&mut m);
        let path = format!("{}/../../assets/fonts/{file}", env!("CARGO_MANIFEST_DIR"));
        let bytes = registered_face(family, FaceStyle::Regular).expect(family);
        assert_eq!(&*bytes, &std::fs::read(path).unwrap()[..], "{family}");
        let public = family.trim_start_matches("paintrs fallback: ");
        m.ensure_family(public, FaceStyle::Regular);
        drain(&mut m);
        assert!(
            registered_face(public, FaceStyle::Regular).is_none(),
            "{public}"
        );
    }
}
fn settle(h: &mut egui_kittest::Harness<'_, paintrs::PaintApp>) {
    for _ in 0..60 {
        h.ctx.request_repaint();
        h.run();
        if !h.state().fonts.busy()
            && !h
                .state()
                .shell
                .settings_imports
                .iter()
                .any(|i| i.page == "fonts")
        {
            return;
        }
    }
    panic!("font UI work did not settle: {}", h.state().fonts.status);
}
#[test]
fn fonts_page_consumes_only_font_imports_and_supports_remove_and_clear_cancel() {
    use egui_kittest::{Harness, kittest::Queryable};
    use paintrs::platform::files::PendingImport;
    use pdn_core::action::UiRequest;
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = paintrs::PaintApp::new_with_policy(cc, FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    {
        let s = h.state_mut();
        s.shell.settings_imports.push_back(PendingImport {
            page: "shapes",
            files: vec![file("keep.xaml", b"keep")],
            silent: false,
        });
        s.shell.settings_imports.push_back(PendingImport {
            page: "fonts",
            files: vec![file("Visible.ttf", pdn_core::fonts::CARLITO_REGULAR)],
            silent: false,
        });
        s.app
            .ui_requests
            .push(UiRequest::OpenSettings(Some("fonts".into())));
    }
    settle(&mut h);
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    assert_eq!(h.state().shell.settings_imports.len(), 1);
    assert_eq!(h.state().shell.settings_imports[0].page, "shapes");
    h.get_by_label("Clear all").click();
    h.run();
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    h.get_by_label("Remove Visible").click();
    settle(&mut h);
    assert!(h.state().fonts.registry.records.is_empty());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_storage_reloads_and_rejects_traversal_and_corrupt_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let store = FontStore::native(dir.path().into());
    let mut m = FontManager::new(store.clone(), FontPolicy::Bundled);
    for _ in 0..1000 {
        m.poll();
        if !m.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    m.import(vec![file("Saved.ttf", pdn_core::fonts::SELAWIK_REGULAR)]);
    for _ in 0..1000 {
        m.poll();
        if !m.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(m.registry.records.len(), 1);
    let mut m = FontManager::new(store, FontPolicy::Bundled);
    for _ in 0..1000 {
        m.poll();
        if !m.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(m.registry.records.len(), 1);
    let mut bad = m.registry.records[0].clone();
    bad.id = "../escape".into();
    std::fs::write(
        dir.path().join("fonts.json"),
        serde_json::to_vec(&vec![bad]).unwrap(),
    )
    .unwrap();
    let mut m = FontManager::new(FontStore::native(dir.path().into()), FontPolicy::Bundled);
    for _ in 0..1000 {
        m.poll();
        if !m.busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    m.import(vec![file(
        "No overwrite.ttf",
        pdn_core::fonts::SELAWIK_REGULAR,
    )]);
    drain(&mut m);
    assert!(m.registry.records.is_empty());
    assert!(
        std::fs::read_to_string(dir.path().join("fonts.json"))
            .unwrap()
            .contains("../escape")
    );
}
#[test]
fn font_drop_inside_fonts_modal_imports_without_closing_settings() {
    use egui_kittest::Harness;
    use pdn_core::action::UiRequest;
    #[derive(Debug)]
    struct Drop;
    impl egui::DroppedFile for Drop {
        fn path(&self) -> &std::path::Path {
            std::path::Path::new("Modal.ttf")
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(pdn_core::fonts::CARLITO_REGULAR.to_vec())
        }
    }
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = paintrs::PaintApp::new_with_policy(cc, FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("fonts".into())));
    h.run();
    h.input_mut().dropped_files.push(std::sync::Arc::new(Drop));
    h.step();
    settle(&mut h);
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    assert!(h.state().shell.settings_dialog.is_open());
}
#[test]
fn missing_document_family_keeps_its_requested_display_name() {
    use pdn_core::tool::types::{ToolSettings, ToolType, option_definitions};
    let defs = option_definitions(ToolType::Text, &ToolSettings::default());
    let def = defs.iter().find(|d| d.key == "fontFamily").unwrap();
    let requested = serde_json::json!("Task53 absent family");
    assert_eq!(
        paintrs::shell::tool_menu::face(def, &requested).label,
        "Task53 absent family"
    );
}
#[test]
fn selected_bundled_ui_family_changes_advance_and_metrics() {
    let bytes = include_bytes!("../../../assets/font/inter/Inter-Variable.ttf");
    pdn_core::fonts::register_lazy_face("Inter", FaceStyle::Regular, bytes.to_vec()).unwrap();
    let ctx = egui::Context::default();
    paintrs::install_fonts(&ctx, FontPolicy::Bundled, "Inter");
    let width = paintrs::kit::text::css_width("F", 12.);
    let metrics = paintrs::kit::text::ui_metrics();
    let mut measured = 0.;
    let mut output = ctx.run_ui(Default::default(), |ui| {
        measured = paintrs::kit::text::measure(
            ui.painter(),
            "F",
            paintrs::kit::text::FontSpec::new(12.).weight(500),
        );
    });
    output.textures_delta.clear();
    paintrs::install_fonts(&ctx, FontPolicy::Bundled, "Selawik");
    assert!(
        measured < width - 0.01,
        "Inter medium must use its variable weight advance instead of the regular face: {measured} vs {width}"
    );
    assert_eq!(width, 7.09375); // Original Inter F advance 1209/2048 at 12px, ceil to 1/64px.
    assert_eq!(
        (metrics.units_per_em, metrics.ascent, metrics.descent),
        (2048, 2269, 660)
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_write_failure_keeps_installed_metadata_and_registered_bytes() {
    fn wait(m: &mut FontManager) {
        for _ in 0..1000 {
            m.poll();
            if !m.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        panic!("native font work did not finish");
    }
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("store");
    let mut m = FontManager::new(FontStore::native(root.clone()), FontPolicy::Bundled);
    wait(&mut m);
    m.import(vec![file("Keep.ttf", pdn_core::fonts::SELAWIK_REGULAR)]);
    wait(&mut m);
    let record = m.registry.records[0].clone();
    let metadata = std::fs::read(root.join("fonts.json")).unwrap();
    // A real blocked storage root, after startup, exercises IO failure without a test-only store.
    let moved = dir.path().join("saved");
    std::fs::rename(&root, &moved).unwrap();
    std::fs::write(&root, b"blocked").unwrap();
    m.import(vec![file("Reject.ttf", pdn_core::fonts::CARLITO_REGULAR)]);
    wait(&mut m);
    assert_eq!(m.registry.records, vec![record.clone()]);
    assert!(!m.errors().is_empty());
    m.remove(&record.id);
    wait(&mut m);
    assert_eq!(m.registry.records, vec![record.clone()]);
    assert_eq!(std::fs::read(moved.join("fonts.json")).unwrap(), metadata);
    assert_eq!(
        &*registered_face(&record.family, FaceStyle::Regular).unwrap(),
        pdn_core::fonts::SELAWIK_REGULAR
    );
    std::fs::remove_file(&root).unwrap();
    std::fs::rename(moved, &root).unwrap();
    let mut restored = FontManager::new(FontStore::native(root), FontPolicy::Bundled);
    wait(&mut restored);
    assert_eq!(restored.registry.records, vec![record]);
}

#[test]
fn unavailable_user_storage_does_not_block_bundled_document_faces() {
    let mut m = FontManager::new(FontStore::Unavailable, FontPolicy::Bundled);
    drain(&mut m);
    assert!(!m.ready());
    m.ensure_family("Pacifico", FaceStyle::Regular);
    drain(&mut m);
    assert_eq!(
        &*registered_face("Pacifico", FaceStyle::Regular)
            .expect("Bundled fonts remain usable when user storage fails"),
        include_bytes!("../../../assets/font/pacifico/Pacifico-Regular.ttf")
    );
    assert!(!m.ready());
}

fn duplicate_font_rows() -> egui_kittest::Harness<'static, paintrs::PaintApp> {
    let mut h = egui_kittest::Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = paintrs::PaintApp::new_with_policy(cc, FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    // Match normal app glyph policy so the regression reports the control failure, not the ID-clash warning glyph.
    h.ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    h.run();
    h.state_mut().fonts.import(vec![
        file("Same.ttf", pdn_core::fonts::SELAWIK_REGULAR),
        file("Same.ttf", pdn_core::fonts::CARLITO_REGULAR),
    ]);
    settle(&mut h);
    h.state_mut()
        .app
        .ui_requests
        .push(pdn_core::action::UiRequest::OpenSettings(Some(
            "fonts".into(),
        )));
    h.run();
    settle(&mut h);
    h
}
#[test]
fn same_filename_remove_controls_target_each_physical_row() {
    use egui_kittest::kittest::{NodeT, Queryable};
    for row in 0..2 {
        let mut h = duplicate_font_rows();
        let before = h.state().fonts.registry.records.clone();
        let mut nodes: Vec<_> = h
            .query_all_by_label("Remove Same")
            .map(|n| (n.accesskit_node().id(), n.rect()))
            .collect();
        nodes.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
        assert_eq!(
            nodes.len(),
            2,
            "Both same-labelled controls must be accessible"
        );
        assert_ne!(nodes[0].0, nodes[1].0);
        assert!(nodes[0].1.bottom() < nodes[1].1.top());
        println!("row {row}, accessible Remove controls: {nodes:?}");
        // Native caption space changes the centred dialog origin. Click the actual
        // physical row through pointer input, preserving the same-label ID regression.
        let at = nodes[row].1.center();
        h.hover_at(at);
        h.run();
        h.drag_at(at);
        h.run();
        h.drop_at(at);
        h.run();
        settle(&mut h);
        assert_eq!(
            h.state().fonts.registry.records,
            vec![before[1 - row].clone()],
            "Physical row {row} must remove exactly its installed ID"
        );
        assert!(registered_face(&before[row].family, FaceStyle::Regular).is_none());
        assert!(registered_face(&before[1 - row].family, FaceStyle::Regular).is_some());
    }
}
#[test]
fn same_filename_remove_controls_have_distinct_keyboard_stops() {
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut h = duplicate_font_rows();
    let before = h.state().fonts.registry.records.clone();
    let mut stops = vec![];
    for _ in 0..24 {
        h.key_press(egui::Key::Tab);
        h.run();
        for n in h.query_all_by_label("Remove Same") {
            if n.accesskit_node().is_focused()
                && !stops.iter().any(|(id, _)| *id == n.accesskit_node().id())
            {
                stops.push((n.accesskit_node().id(), n.rect()));
            }
        }
        if stops.len() == 2 {
            break;
        }
    }
    assert_eq!(
        stops.len(),
        2,
        "Keyboard must reach both same-labelled installed-font controls"
    );
    assert!(stops[0].1.top() < stops[1].1.top());
    h.key_press(egui::Key::Enter);
    h.run();
    settle(&mut h);
    assert_eq!(h.state().fonts.registry.records, vec![before[0].clone()]);
}
