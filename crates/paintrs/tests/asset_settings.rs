use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use paintrs::{
    PaintApp,
    assets::Assets,
    icon_pack::{IconPackId, SELECTION_KEY},
    platform::{Files, PickedFile},
};
use pdn_core::{action::UiRequest, fonts::FontPolicy};
const XAML:&[u8]=br#"<s:SimpleGeometryShape xmlns:s="clr-namespace:PaintDotNet.Shapes;assembly=PaintDotNet.Framework" xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.UI" DisplayName="Tri" Geometry="M0,0 L10,0 0,10Z"/>"#;
fn harness() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280., 800.))
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, FontPolicy::Bundled);
            // Thumbnails on the frame that needs them: `run()` settles without a worker thread.
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run();
    h
}
fn settle(h: &mut Harness<'_, PaintApp>) {
    for _ in 0..100 {
        h.run();
        if !h.state().asset_library.busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("asset import did not finish");
}
#[test]
fn shapes_page_picker_import_remove_and_clear_cancellation() {
    let mut h = harness();
    let files = Files::in_memory();
    files.queue_pick(Ok(vec![PickedFile::snapshot(
        "Tri.xaml".into(),
        XAML.to_vec(),
    )]));
    files.install();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("shapes".into())));
    h.run();
    h.get_by_label("Add shapes…").click();
    settle(&mut h);
    assert_eq!(h.state().asset_library.shapes.len(), 1);
    assert_eq!(h.state().app.shape_catalog.custom.len(), 1);
    h.get_by_label("Clear all").click();
    h.run();
    h.get_by_label("Cancel").click();
    h.run();
    assert_eq!(h.state().asset_library.shapes.len(), 1);
    h.get_by_label("Remove Tri").click();
    settle(&mut h);
    assert!(h.state().asset_library.shapes.is_empty());
}
/// The removed paint.js classic pack is not listed.
#[test]
fn icon_packs_page_exposes_install_and_lists_no_classic_pack() {
    let mut h = harness();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("assets".into())));
    h.run();
    assert!(h.query_by_label("Install icon pack…").is_some());
    assert_eq!(Assets::selected(&h.ctx), IconPackId::PaintRs);
    assert_eq!(h.state().prefs.get(SELECTION_KEY), Some("paint-rs"));
    assert!(h.query_by_label("Active paint.rs").is_some());
    assert!(h.query_by_label("Use paint.js classic").is_none());
}

#[test]
fn ui_icon_selector_and_icon_packs_page_share_the_same_choice() {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (path, bytes) in [
        (
            "asset-pack.json",
            &br#"{"formatVersion":1,"name":"Choice Pack"}"#[..],
        ),
        (
            "icons/menu_file_new_icon.png",
            include_bytes!("../../../tests/fixtures/icon-64.png"),
        ),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut h = harness();
    h.state_mut().asset_library.import(
        "assets",
        vec![PickedFile::snapshot(
            "choice.zip".into(),
            zip.finish().unwrap().into_inner(),
        )],
    );
    settle(&mut h);
    let id = h.state().asset_library.packs[0].id.clone();
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("ui".into())));
    h.run();
    h.get_by_label("Icon pack:").click();
    h.run();
    assert!(h.query_by_label("paint.js Default").is_none());
    h.get_by_label("Choice Pack").click();
    h.run();
    assert_eq!(Assets::selected(&h.ctx), IconPackId::User(id.clone()));
    assert_eq!(
        h.state().prefs.get(SELECTION_KEY),
        Some(format!("user:{id}").as_str())
    );
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("assets".into())));
    h.run();
    h.get_by_label("Use paint.rs").click();
    h.run();
    assert_eq!(Assets::selected(&h.ctx), IconPackId::PaintRs);
    assert_eq!(h.state().prefs.get(SELECTION_KEY), Some("paint-rs"));
}

#[test]
fn selected_page_delivers_only_its_pending_payload_and_reports_validation_failure() {
    use paintrs::platform::files::PendingImport;
    let mut h = harness();
    let shapes = PickedFile::snapshot("later.xaml".into(), XAML.to_vec());
    let shape_id = shapes.id;
    h.state_mut()
        .shell
        .settings_imports
        .push_back(PendingImport {
            page: "shapes",
            files: vec![shapes],
            silent: false,
        });
    h.state_mut()
        .shell
        .settings_imports
        .push_back(PendingImport {
            page: "assets",
            files: vec![PickedFile::snapshot("bad.zip".into(), vec![1, 2])],
            silent: false,
        });
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("assets".into())));
    h.run();
    settle(&mut h);
    assert_eq!(h.state().shell.settings_imports.len(), 1);
    assert_eq!(h.state().shell.settings_imports[0].files[0].id, shape_id);
    assert!(h.state().asset_library.packs.is_empty());
    assert!(!h.state().asset_library.errors.is_empty());
    h.state_mut()
        .app
        .ui_requests
        .push(UiRequest::OpenSettings(Some("shapes".into())));
    h.run();
    settle(&mut h);
    assert!(h.state().shell.settings_imports.is_empty());
    assert_eq!(h.state().asset_library.shapes.len(), 1);
}

#[test]
fn mixed_drop_preserves_classified_payloads_and_delivers_image_remainder_once() {
    use paintrs::platform::files;
    let mut h = harness();
    let initial_id = h.state().app.active_ws().unwrap().id;
    let payloads = vec![
        PickedFile::snapshot("first.xaml".into(), XAML.to_vec()),
        PickedFile::snapshot("second.zip".into(), vec![2]),
        PickedFile::snapshot("third.ttf".into(), vec![3]),
        PickedFile::snapshot(
            "fourth.png".into(),
            include_bytes!("../../../tests/fixtures/icon-64.png").to_vec(),
        ),
    ];
    let ids: Vec<_> = payloads.iter().map(|f| f.id).collect();
    let batch = files::dropped(payloads);
    assert_eq!(
        batch.imports.iter().map(|p| p.page).collect::<Vec<_>>(),
        ["shapes", "assets", "fonts"]
    );
    assert_eq!(
        batch
            .imports
            .iter()
            .flat_map(|p| p.files.iter().map(|f| f.id))
            .collect::<Vec<_>>(),
        ids[..3]
    );
    assert_eq!(batch.imports[0].files[0].bytes.as_ref(), XAML);
    assert_eq!(batch.imports[1].files[0].bytes.as_ref(), [2]);
    assert_eq!(batch.imports[2].files[0].bytes.as_ref(), [3]);
    assert!(batch.imports[2].silent);
    assert_eq!(batch.images[0].id, ids[3]);
    h.state_mut().shell.drop_batches.push_back(batch);
    h.run_steps(1);
    assert!(h.query_by_label("Open").is_none());
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
        "the image remainder retains its dialog continuation"
    );
    h.get_by_label("Open").click();
    for _ in 0..500 {
        h.run_steps(1);
        if !files::busy() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(h.state().app.documents.len(), 1);
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 64);
    let imported_id = h.state().app.active_ws().unwrap().id;
    assert_ne!(imported_id, initial_id);
    h.run_steps(1);
    assert_eq!(h.state().app.active_ws().unwrap().id, imported_id);
    assert_eq!(h.state().app.documents.len(), 1);
    assert!(h.state().shell.drop_batches.is_empty());
}

#[test]
fn two_image_drops_keep_fifo_until_the_first_choice_resolves() {
    use paintrs::platform::files;
    use std::{path::Path, sync::Arc};
    #[derive(Debug)]
    struct Source(&'static str, &'static [u8]);
    impl egui::DroppedFile for Source {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(self.1.to_vec())
        }
    }
    let mut h = harness();
    let initial = h.state().app.active_ws().unwrap().id;
    h.input_mut().dropped_files.push(Arc::new(Source(
        "first.png",
        include_bytes!("../../../tests/fixtures/a.png"),
    )));
    h.run_steps(1);
    assert!(h.query_by_label("Open").is_some());
    h.input_mut().dropped_files.push(Arc::new(Source(
        "second.png",
        include_bytes!("../../../tests/fixtures/icon-64.png"),
    )));
    h.run_steps(1);
    assert_eq!(
        h.state().shell.drop_batches.len(),
        1,
        "second raw drop was not queued"
    );
    h.get_by_label("Cancel").click();
    h.run_steps(1);
    assert_eq!(h.state().app.active_ws().unwrap().id, initial);
    for _ in 0..10 {
        if h.state().shell.drop_batches.is_empty() && h.query_by_label("Open").is_some() {
            break;
        }
        h.ctx.request_repaint();
        h.run_steps(1);
    }
    assert!(h.state().shell.drop_batches.is_empty());
    h.get_by_label("Open").click();
    for _ in 0..100 {
        h.run_steps(1);
        if !files::busy() && h.state().app.active_ws().unwrap().id != initial {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_ne!(h.state().app.active_ws().unwrap().id, initial);
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 64);
    assert_eq!(h.state().app.documents.len(), 1);
}

#[test]
fn second_drop_waits_for_first_open_to_finish_reading() {
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    #[derive(Debug)]
    struct GatedSource(&'static str, &'static [u8], Arc<AtomicBool>);
    impl egui::DroppedFile for GatedSource {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            while !self.2.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(self.1.to_vec())
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let mut h = harness();
    let release = Release(Arc::new(AtomicBool::new(false)));
    h.input_mut().dropped_files.push(Arc::new(GatedSource(
        "first.png",
        include_bytes!("../../../tests/fixtures/a.png"),
        release.0.clone(),
    )));
    h.run_steps(1);
    h.input_mut().dropped_files.push(Arc::new(GatedSource(
        "second.png",
        include_bytes!("../../../tests/fixtures/icon-64.png"),
        Arc::new(AtomicBool::new(true)),
    )));
    h.run_steps(1);
    assert_eq!(h.state().shell.drop_batches.len(), 1);
    h.get_by_label("Open").click();
    h.run_steps(1);
    assert!(paintrs::platform::files::busy());
    h.run_steps(1);
    assert_eq!(
        h.state().shell.drop_batches.len(),
        1,
        "second batch overtook pending Open"
    );
    release.0.store(true, Ordering::Release);
    for _ in 0..100 {
        h.ctx.request_repaint();
        h.run_steps(1);
        if h.state().shell.drop_batches.is_empty() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(h.state().shell.drop_batches.is_empty());
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 13);
    assert!(h.query_by_label("Open").is_some());
    h.get_by_label("Cancel").click();
    h.run_steps(1);
}

#[test]
fn second_drop_waits_for_failed_layer_error_acknowledgement() {
    use std::{
        path::Path,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
        },
    };
    #[derive(Debug)]
    struct GatedSource(&'static str, &'static [u8], Arc<AtomicBool>);
    impl egui::DroppedFile for GatedSource {
        fn path(&self) -> &Path {
            Path::new(self.0)
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            while !self.2.load(Ordering::Acquire) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            Ok(self.1.to_vec())
        }
    }
    struct Release(Arc<AtomicBool>);
    impl Drop for Release {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }
    let mut h = harness();
    let release = Release(Arc::new(AtomicBool::new(false)));
    h.input_mut().dropped_files.push(Arc::new(GatedSource(
        "broken.png",
        &[1, 2, 3],
        release.0.clone(),
    )));
    h.run_steps(1);
    h.input_mut().dropped_files.push(Arc::new(GatedSource(
        "second.png",
        include_bytes!("../../../tests/fixtures/icon-64.png"),
        Arc::new(AtomicBool::new(true)),
    )));
    h.run_steps(1);
    assert_eq!(h.state().shell.drop_batches.len(), 1);
    h.get_by_label("Add layer").click();
    h.run_steps(1);
    assert!(paintrs::platform::files::busy());
    release.0.store(true, Ordering::Release);
    for _ in 0..100 {
        h.ctx.request_repaint();
        h.run_steps(1);
        if h.query_by_label("OK").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        h.query_by_label("OK").is_some(),
        "failed layer must show its error"
    );
    assert_eq!(
        h.state().shell.drop_batches.len(),
        1,
        "second batch overtook error acknowledgement"
    );
    h.input_mut().dropped_files.push(Arc::new(GatedSource(
        "ignored.png",
        include_bytes!("../../../tests/fixtures/icon-64.png"),
        Arc::new(AtomicBool::new(true)),
    )));
    h.run_steps(1);
    assert_eq!(
        h.state().shell.drop_batches.len(),
        1,
        "a raw drop onto the Open Image error dialog must remain masked"
    );
    h.get_by_label("OK").click();
    for _ in 0..10 {
        h.ctx.request_repaint();
        h.run_steps(1);
        if h.state().shell.drop_batches.is_empty() {
            break;
        }
    }
    assert!(h.state().shell.drop_batches.is_empty());
    assert!(h.query_by_label("Open").is_some());
}

#[test]
fn review_topmost_asset_pages_retain_other_types_from_actual_raw_mixed_drop() {
    use std::{
        path::{Path, PathBuf},
        sync::Arc,
    };
    #[derive(Debug)]
    struct Source(PathBuf, Vec<u8>);
    impl egui::DroppedFile for Source {
        fn path(&self) -> &Path {
            &self.0
        }
        fn bytes(&self) -> Result<Vec<u8>, String> {
            Ok(self.1.clone())
        }
    }
    for page in ["assets", "shapes"] {
        let mut h = harness();
        Files::in_memory().install();
        h.state_mut()
            .app
            .ui_requests
            .push(UiRequest::OpenSettings(Some(page.into())));
        h.run();
        assert!(h.state().shell.dialogs.top_is("settings"));
        let root = tempfile::tempdir().unwrap();
        let payloads = [
            ("first.xaml", XAML),
            ("second.zip", &[2][..]),
            (
                "third.ttf",
                include_bytes!("../../../assets/font/carlito/Carlito-Regular.ttf"),
            ),
            (
                "fourth.png",
                include_bytes!("../../../tests/fixtures/icon-64.png"),
            ),
        ];
        for (name, data) in payloads {
            let path = root.path().join(name);
            std::fs::write(&path, data).unwrap();
            h.input_mut()
                .dropped_files
                .push(Arc::new(Source(path, data.to_vec())));
        }
        h.run_steps(1);
        let other = if page == "assets" { "shapes" } else { "assets" };
        let batch = h
            .state()
            .shell
            .drop_batches
            .front()
            .expect("mixed drop retained");
        assert!(
            batch.active == Some(other) || batch.imports.iter().any(|p| p.page == other),
            "{page} discarded the other asset type"
        );
        assert_eq!(
            batch.images.len(),
            1,
            "{page} discarded the image remainder"
        );
        assert!(h.query_by_label("Open").is_none());
        for _ in 0..500 {
            if h.query_by_label("Open").is_some() {
                break;
            }
            h.ctx.request_repaint();
            h.run_steps(1);
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(h.query_by_label("Open").is_some());
        assert!(
            h.state()
                .fonts
                .registry
                .records
                .iter()
                .any(|f| f.file_name == "third.ttf"),
            "{page} discarded the font"
        );
    }
}

#[test]
fn review_drop_zone_buttons_open_pickers_by_click_and_enter() {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (path, bytes) in [
        (
            "asset-pack.json",
            &br#"{"formatVersion":1,"name":"Picker Pack"}"#[..],
        ),
        ("lang/en.json", &br#"{"key":"Picker"}"#[..]),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let zip = zip.finish().unwrap().into_inner();
    for page in ["shapes", "assets"] {
        for keyboard in [false, true] {
            let mut h = harness();
            let files = Files::in_memory();
            let (name, bytes, label) = if page == "shapes" {
                (
                    "Picked.xaml",
                    XAML.to_vec(),
                    "Drop Paint.NET .xaml shape files here",
                )
            } else {
                ("Picked.zip", zip.clone(), "Drop an icon-pack ZIP here")
            };
            files.queue_pick(Ok(vec![PickedFile::snapshot(name.into(), bytes)]));
            files.install();
            h.state_mut()
                .app
                .ui_requests
                .push(UiRequest::OpenSettings(Some(page.into())));
            h.run();
            let zone = h.get_by_label(label);
            if keyboard {
                zone.focus();
                h.event(egui::Event::Key {
                    key: egui::Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                });
            } else {
                zone.click();
            }
            settle(&mut h);
            assert_eq!(
                if page == "shapes" {
                    h.state().asset_library.shapes.len()
                } else {
                    h.state().asset_library.packs.len()
                },
                1
            );
            assert!(
                h.state().shell.settings_dialog.is_open(),
                "Enter must not submit Settings"
            );
        }
    }
}

#[test]
fn successful_mixed_drop_installs_each_category_before_starting_the_next() {
    use paintrs::platform::files;
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (path, bytes) in [
        (
            "asset-pack.json",
            &br#"{"formatVersion":1,"name":"Drop Pack"}"#[..],
        ),
        ("lang/en.json", &br#"{"key":"Drop Pack"}"#[..]),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    let mut h = harness();
    h.state_mut()
        .shell
        .drop_batches
        .push_back(files::dropped(vec![
            PickedFile::snapshot("first.xaml".into(), XAML.to_vec()),
            PickedFile::snapshot("second.zip".into(), zip.finish().unwrap().into_inner()),
            PickedFile::snapshot(
                "third.ttf".into(),
                pdn_core::fonts::CARLITO_REGULAR.to_vec(),
            ),
            PickedFile::snapshot(
                "fourth.png".into(),
                include_bytes!("../../../tests/fixtures/icon-64.png").to_vec(),
            ),
        ]));
    let (mut saw_assets, mut saw_fonts) = (false, false);
    for _ in 0..500 {
        h.ctx.request_repaint();
        h.run_steps(1);
        match h
            .state()
            .shell
            .drop_batches
            .front()
            .and_then(|batch| batch.active)
        {
            Some("assets") => {
                saw_assets = true;
                assert_eq!(h.state().asset_library.shapes.len(), 1);
                assert!(h.query_by_label("Open").is_none());
            }
            Some("fonts") => {
                saw_fonts = true;
                assert_eq!(h.state().asset_library.shapes.len(), 1);
                assert_eq!(h.state().asset_library.packs.len(), 1);
                assert_eq!(h.state().asset_library.packs[0].name(), "Drop Pack");
                assert!(h.query_by_label("Open").is_none());
            }
            _ => {}
        }
        if h.query_by_label("Open").is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert!(
        saw_assets && saw_fonts,
        "both category boundaries must be observable"
    );
    assert_eq!(h.state().asset_library.shapes.len(), 1);
    assert_eq!(h.state().asset_library.packs.len(), 1);
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    assert!(h.query_by_label("Open").is_some());
}

#[test]
fn mixed_drop_waits_for_import_completion_before_image_choice() {
    use paintrs::platform::files;
    let mut h = harness();
    {
        let state = h.state_mut();
        state.shell.drop_batches.push_back(files::dropped(vec![
            PickedFile::snapshot("first.xaml".into(), XAML.to_vec()),
            PickedFile::snapshot("bad.zip".into(), vec![2]),
            PickedFile::snapshot(
                "third.ttf".into(),
                pdn_core::fonts::CARLITO_REGULAR.to_vec(),
            ),
            PickedFile::snapshot(
                "fourth.png".into(),
                include_bytes!("../../../tests/fixtures/icon-64.png").to_vec(),
            ),
        ]));
    }
    h.run_steps(1);
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
    assert!(
        h.query_by_label("Open").is_none(),
        "image choice appeared before classified imports completed"
    );
    for _ in 0..500 {
        if h.query_by_label("Open").is_some() {
            break;
        }
        h.ctx.request_repaint();
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(2));
        if h.state().fonts.importing() {
            assert_eq!(h.state().asset_library.shapes.len(), 1);
            assert!(!h.state().asset_library.errors.is_empty());
        }
    }
    assert_eq!(h.state().asset_library.shapes.len(), 1);
    assert!(!h.state().asset_library.errors.is_empty());
    assert_eq!(h.state().fonts.registry.records.len(), 1);
    assert!(!h.state().asset_library.busy());
    assert!(!h.state().fonts.importing());
    assert!(h.query_by_label("Open").is_some());
}
