//! Task50 platform/continuation regressions, using the real PaintApp UI owner.
use egui_kittest::Harness;
use paintrs::{
    PaintApp,
    dialogs::flows::{self, SaveDestination, SaveRequest, SaveTarget, TargetRequest, WriteOutcome},
    platform::{Files, Pending, PickedFile},
};
use std::{
    cell::{Cell, RefCell},
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};
/// Thumbnails scale in the frame that needs them (R203): a worker's late result would ask for
/// frames in the middle of a later `Harness::run` and, under load, overrun its step budget.
fn harness() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
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
fn png() -> Vec<u8> {
    let image = pdn_core::canvas::ImageData {
        width: 13,
        height: 9,
        data: pdn_core::testing::fixture_rgba(13, 9, 4),
    };
    pdn_core::io::encode::encode(
        &image,
        "png",
        &pdn_core::io::encode::SaveOptions::default_for("png"),
    )
    .unwrap()
}
#[derive(Debug)]
struct DropSource {
    path: PathBuf,
    bytes: Vec<u8>,
}
impl egui::DroppedFile for DropSource {
    fn path(&self) -> &Path {
        &self.path
    }
    fn bytes(&self) -> Result<Vec<u8>, String> {
        Ok(self.bytes.clone())
    }
}
#[test]
fn modal_blocks_drop_and_queued_delivery_runs_once_after_close() {
    let mut h = harness();
    h.state_mut()
        .shell
        .dialogs
        .push(Box::new(paintrs::dialogs::new_file::NewFileDialog::new(
            800, 600,
        )));
    let source = PickedFile::snapshot("incoming.png".into(), png());
    h.state_mut()
        .queue_picked_files(vec![source.clone(), source]);
    h.input_mut().dropped_files.push(Arc::new(DropSource {
        path: "drop.png".into(),
        bytes: png(),
    }));
    h.step();
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 800);
    h.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 13);
    assert!(!h.state().shell.dialogs.is_open());
    let id = h.state().app.active_ws().unwrap().id;
    h.run();
    assert_eq!(h.state().app.active_ws().unwrap().id, id);
}
struct Deferred {
    ready: Rc<Cell<bool>>,
    writes: Rc<Cell<usize>>,
}
impl SaveTarget for Deferred {
    fn choose(&mut self, _: &TargetRequest) -> Result<Option<SaveDestination>, String> {
        panic!("existing target")
    }
    fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
        panic!("pending writer")
    }
    fn write_pending(&mut self, r: SaveRequest) -> Pending<Result<WriteOutcome, String>> {
        assert!(r.bytes.starts_with(b"\x89PNG"));
        let ready = self.ready.clone();
        let writes = self.writes.clone();
        Pending::future(std::future::poll_fn(move |_| {
            if ready.get() {
                writes.set(writes.get() + 1);
                std::task::Poll::Ready(Ok(WriteOutcome::Written))
            } else {
                std::task::Poll::Pending
            }
        }))
    }
}
#[test]
fn pending_save_masks_drop_and_defers_incoming_files_until_confirmed_write() {
    let mut h = harness();
    let ready = Rc::new(Cell::new(false));
    let writes = Rc::new(Cell::new(0));
    flows::set_save_target(Some(Rc::new(RefCell::new(Deferred {
        ready: ready.clone(),
        writes: writes.clone(),
    }))));
    let handle = {
        let state = h.state_mut();
        let ws = state.app.active_ws_mut().unwrap();
        ws.set_file_info(
            Some("saved.png".into()),
            Some("saved.png".into()),
            Some("png".into()),
            serde_json::json!({"quality":17}),
        );
        ws.set_dirty(true);
        flows::save_active(
            &mut state.app,
            &mut state.settings,
            &mut state.shell.settings_changed,
            false,
        )
    };
    assert_eq!(handle.poll(), None);
    {
        let state = h.state_mut();
        let second = flows::save_active(
            &mut state.app,
            &mut state.settings,
            &mut state.shell.settings_changed,
            false,
        );
        assert_eq!(second.poll(), Some(false));
    }
    h.state_mut()
        .queue_picked_files(vec![PickedFile::snapshot("next.png".into(), png())]);
    h.input_mut().dropped_files.push(Arc::new(DropSource {
        path: "blocked.png".into(),
        bytes: png(),
    }));
    h.step();
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 800);
    assert!(h.state().app.active_ws().unwrap().dirty);
    ready.set(true);
    h.run();
    assert_eq!(handle.poll(), Some(true));
    assert_eq!(writes.get(), 1);
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 13);
    let count = h.state().app.documents.len();
    h.run();
    assert_eq!(writes.get(), 1);
    assert_eq!(h.state().app.documents.len(), count);
    flows::set_save_target(None);
}
struct OrderedWrites {
    ready: Rc<Cell<usize>>,
    scheduled: Rc<Cell<usize>>,
    completed: Rc<RefCell<Vec<u64>>>,
    second: Result<WriteOutcome, String>,
}
impl SaveTarget for OrderedWrites {
    fn choose(&mut self, _: &TargetRequest) -> Result<Option<SaveDestination>, String> {
        panic!("existing target")
    }
    fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
        panic!("pending writer")
    }
    fn write_pending(&mut self, r: SaveRequest) -> Pending<Result<WriteOutcome, String>> {
        let ordinal = self.scheduled.get() + 1;
        self.scheduled.set(ordinal);
        let ready = self.ready.clone();
        let completed = self.completed.clone();
        let mut outcome = Some(if ordinal == 2 {
            self.second.clone()
        } else {
            Ok(WriteOutcome::Written)
        });
        Pending::future(std::future::poll_fn(move |_| {
            if ready.get() < ordinal {
                return std::task::Poll::Pending;
            }
            completed.borrow_mut().push(r.workspace_id);
            std::task::Poll::Ready(outcome.take().unwrap())
        }))
    }
}
#[test]
fn pending_save_all_preserves_order_and_stops_on_failure_or_export() {
    for second in [
        Ok(WriteOutcome::Written),
        Err("late failure".into()),
        Ok(WriteOutcome::DownloadStarted),
    ] {
        let success = second == Ok(WriteOutcome::Written);
        let mut h = harness();
        let state = h.state_mut();
        state.app = pdn_core::testing::fresh_doc(2, 2, None);
        state.app.documents[0].is_initial_workspace = false;
        for _ in 0..2 {
            state
                .app
                .create_blank_document_in_new_workspace(2, 2, 96., false);
        }
        let ids = state.app.documents.iter().map(|w| w.id).collect::<Vec<_>>();
        for (i, ws) in state.app.documents.iter_mut().enumerate() {
            let name = format!("image{i}.png");
            ws.set_file_info(
                Some(name.clone()),
                Some(name),
                Some("png".into()),
                serde_json::json!({"quality":17}),
            );
            ws.set_dirty(true);
        }
        state.app.set_active_document_workspace(Some(2));
        let ready = Rc::new(Cell::new(0));
        let scheduled = Rc::new(Cell::new(0));
        let completed = Rc::new(RefCell::new(vec![]));
        flows::set_save_target(Some(Rc::new(RefCell::new(OrderedWrites {
            ready: ready.clone(),
            scheduled: scheduled.clone(),
            completed: completed.clone(),
            second,
        }))));
        let result = flows::save_all(
            &mut state.app,
            &mut state.settings,
            &mut state.shell.settings_changed,
        );
        assert_eq!(result.poll(), None);
        assert_eq!(scheduled.get(), 1);
        assert_eq!(h.state().app.active_ws().unwrap().id, ids[0]);
        ready.set(1);
        h.run();
        assert_eq!(*completed.borrow(), ids[..1]);
        assert_eq!(scheduled.get(), 2);
        assert_eq!(result.poll(), None);
        assert_eq!(h.state().app.active_ws().unwrap().id, ids[1]);
        ready.set(2);
        h.run();
        assert_eq!(*completed.borrow(), ids[..2]);
        if success {
            assert_eq!(result.poll(), None);
            assert_eq!(scheduled.get(), 3);
            ready.set(3);
            h.run();
            assert_eq!(*completed.borrow(), ids);
            assert_eq!(h.state().app.active_ws().unwrap().id, ids[2]);
        } else {
            assert_eq!(scheduled.get(), 2);
            assert_eq!(h.state().app.active_ws().unwrap().id, ids[1]);
            assert!(h.state().app.documents[1].dirty);
            assert!(h.state().app.documents[2].dirty);
        }
        assert_eq!(result.poll(), Some(success));
        let count = completed.borrow().len();
        h.run();
        assert_eq!(completed.borrow().len(), count);
        flows::set_save_target(None);
    }
}
#[test]
fn native_prepared_write_commits_real_encoded_bytes() {
    use flows::SaveTarget;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.png");
    std::fs::write(&path, b"old").unwrap();
    let mut files = Files::system();
    let destination = SaveDestination {
        name: "saved.png".into(),
        path: Some(path.to_string_lossy().into()),
        format: "png",
    };
    assert!(
        files
            .prepare_pending(7, &destination)
            .poll()
            .unwrap()
            .is_ok()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let bytes = png();
    let mut write = files.write_pending(SaveRequest {
        workspace_id: 7,
        destination: destination.clone(),
        bytes: bytes.clone(),
    });
    let result = loop {
        if let Some(r) = write.poll() {
            break r;
        }
        std::thread::yield_now();
    };
    assert_eq!(result.unwrap(), WriteOutcome::Written);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    files.committed(7, &destination);
    assert_eq!(files.existing(7), Some(destination));
}
#[test]
fn native_preparation_failure_and_abandon_do_not_touch_destination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("saved.png");
    std::fs::write(&path, b"old").unwrap();
    let mut files = Files::system();
    let d = SaveDestination {
        name: "saved.png".into(),
        path: Some(path.to_string_lossy().into()),
        format: "png",
    };
    assert!(files.prepare_pending(1, &d).poll().unwrap().is_ok());
    files.abandon(1);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(std::fs::read(&path).unwrap(), b"old");
    let d = SaveDestination {
        path: Some(dir.path().join("missing/a.png").to_string_lossy().into()),
        ..d
    };
    assert!(files.prepare_pending(1, &d).poll().unwrap().is_err());
}

#[test]
fn committed_drop_fixture_matches_test_helper() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/a.png");
    let expected = pdn_core::testing::fixture_rgba(13, 9, 4);
    if std::env::var_os("PAINTRS_WRITE_FILE_FIXTURE").is_some() {
        image::save_buffer(&path, &expected, 13, 9, image::ColorType::Rgba8).unwrap();
    }
    let image = image::open(path).unwrap().into_rgba8();
    assert_eq!(image.dimensions(), (13, 9));
    assert_eq!(image.as_raw(), &expected);
}

#[test]
fn reusable_save_service_writes_non_image_bytes_to_existing_target() {
    use paintrs::platform::files::{FileSaveOutcome, FileTarget};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data.xml");
    let target = FileTarget {
        name: "data.xml".into(),
        path: Some(path.clone()),
    };
    let mut save = Files::system().save("data.xml", b"<chart/>".to_vec(), Some(target));
    let result = loop {
        if let Some(r) = save.poll() {
            break r;
        }
        std::thread::yield_now();
    };
    assert!(matches!(result.unwrap(), FileSaveOutcome::Written(_)));
    assert_eq!(std::fs::read(path).unwrap(), b"<chart/>");
}

/// BB36 fix-1 re-review: Chart or Graph's Load rereads its file within the chart's 8 MiB limit,
/// the length checked before reading (not after reading up to the 512 MiB image limit).
#[test]
fn chart_data_reread_stops_at_the_chart_limit() {
    use pdn_core::effect::bb::conventions::CHART_MAX_ENCODED_BYTES;
    let dir = tempfile::tempdir().unwrap();
    let reread = |len: u64| {
        let path = dir.path().join(format!("{len}.xml"));
        std::fs::File::create(&path).unwrap().set_len(len).unwrap();
        let mut file = PickedFile::snapshot("chart.xml".into(), vec![]);
        file.path = Some(path);
        let mut read = file.reread_within(CHART_MAX_ENCODED_BYTES as u64);
        loop {
            if let Some(r) = read.poll() {
                break r;
            }
            std::thread::yield_now();
        }
    };
    let limit = CHART_MAX_ENCODED_BYTES as u64;
    assert!(reread(limit + 1).unwrap_err().contains("too large"));
    assert_eq!(reread(limit).unwrap().bytes.len() as u64, limit);
}

#[test]
fn sparse_oversized_source_is_rejected_before_reading() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("large.png");
    let file = std::fs::File::create(&path).unwrap();
    file.set_len(pdn_core::io::codecs::MAX_DECODE_BYTES + 1)
        .unwrap();
    assert!(
        paintrs::platform::native::read_bytes(&path)
            .unwrap_err()
            .contains("too large")
    );
}

#[cfg(unix)]
#[test]
fn native_open_target_keeps_non_utf8_path_identity() {
    use std::os::unix::ffi::OsStringExt;
    let dir = tempfile::tempdir().unwrap();
    let path = dir
        .path()
        .join(std::ffi::OsString::from_vec(b"image\xff.png".to_vec()));
    std::fs::write(&path, b"old").unwrap();
    let mut file = PickedFile::snapshot(path.file_name().unwrap().to_string_lossy().into(), png());
    file.path = Some(path.clone());
    let mut files = Files::system();
    files.adopt_open(42, &file, "png");
    let destination = files.existing(42).unwrap();
    assert!(
        files
            .prepare_pending(42, &destination)
            .poll()
            .unwrap()
            .is_ok()
    );
    let mut write = files.write_pending(SaveRequest {
        workspace_id: 42,
        destination,
        bytes: png(),
    });
    loop {
        if let Some(r) = write.poll() {
            assert_eq!(r.unwrap(), WriteOutcome::Written);
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(std::fs::read(&path).unwrap(), png());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

struct DelayedChooser(Rc<Cell<bool>>);
impl SaveTarget for DelayedChooser {
    fn choose(&mut self, _: &TargetRequest) -> Result<Option<SaveDestination>, String> {
        unreachable!()
    }
    fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
        unreachable!()
    }
    fn choose_pending(
        &mut self,
        r: TargetRequest,
    ) -> Pending<Result<Option<SaveDestination>, String>> {
        let ready = self.0.clone();
        Pending::future(std::future::poll_fn(move |_| {
            if ready.get() {
                std::task::Poll::Ready(Ok(Some(SaveDestination {
                    name: r.suggested_name.clone(),
                    path: Some(r.suggested_name.clone()),
                    format: r.initial_format,
                })))
            } else {
                std::task::Poll::Pending
            }
        }))
    }
}
fn chooser_transition(drop: bool) {
    use egui_kittest::kittest::Queryable;
    let mut h = harness();
    let ready = Rc::new(Cell::new(false));
    flows::set_save_target(Some(Rc::new(RefCell::new(DelayedChooser(ready.clone())))));
    let state = h.state_mut();
    let result = flows::save_active(
        &mut state.app,
        &mut state.settings,
        &mut state.shell.settings_changed,
        true,
    );
    assert_eq!(result.poll(), None);
    h.state_mut()
        .queue_picked_files(vec![PickedFile::snapshot("incoming.png".into(), png())]);
    ready.set(true);
    if drop {
        h.input_mut().dropped_files.push(Arc::new(DropSource {
            path: "blocked.png".into(),
            bytes: png(),
        }));
    } else {
        let modifiers = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        h.input_mut().events.push(egui::Event::Key {
            key: egui::Key::N,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        });
    }
    h.step();
    h.run();
    assert!(
        h.query_by_label("Add layer").is_none(),
        "drop must remain masked when chooser queues configuration"
    );
    assert_eq!(
        h.state().app.active_ws().unwrap().document.layers.size(),
        1,
        "shortcut must remain masked when chooser queues configuration"
    );
    assert!(h.query_by_label("File type").is_some());
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 800);
    assert_eq!(result.poll(), None);
    h.key_press(egui::Key::Escape);
    h.run();
    assert_eq!(result.poll(), Some(false));
    assert_eq!(h.state().app.active_ws().unwrap().document.width, 13);
    let id = h.state().app.active_ws().unwrap().id;
    h.run();
    assert_eq!(h.state().app.active_ws().unwrap().id, id);
    flows::set_save_target(None);
}
#[test]
fn fix1_chooser_completion_masks_transition_frame_drop() {
    chooser_transition(true);
}
#[test]
fn fix1_chooser_completion_masks_transition_frame_shortcut() {
    chooser_transition(false);
}
#[test]
fn fix1_loaded_palette_save_preserves_uppercase_and_dotted_filenames() {
    use paintrs::platform::palettes::PaletteStore;
    for filename in ["Ocean.TXT", "Ocean.blue.TXT"] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(filename), "FF112233").unwrap();
        let store = PaletteStore::new(dir.path().into());
        let mut values = store.load().unwrap();
        *values.values_mut().next().unwrap() = "FF445566".into();
        store.save(&values).unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path()).unwrap().count(),
            1,
            "{filename}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(filename)).unwrap(),
            "FF445566"
        );
        assert_eq!(store.load().unwrap(), values);
    }
}
#[test]
fn fix1_safe_palette_save_does_not_write_or_discard_unmigrated_legacy() {
    use egui_kittest::kittest::{NodeT, Queryable};
    use paintrs::{forms::colors::USER_PALETTES_KEY, platform::palettes::PaletteStore};
    let dir = tempfile::tempdir().unwrap();
    let mut h = harness();
    let legacy =
        serde_json::json!({"Ocean.one":"FF112233","Ocean.two":"FF445566","../unsafe":"FF778899"});
    let state = h.state_mut();
    state.prefs.set(USER_PALETTES_KEY, &legacy.to_string());
    state.shell.forms.colors.load_prefs(&state.prefs);
    state
        .shell
        .forms
        .colors
        .enable_file_palettes(Some(PaletteStore::new(dir.path().into())));
    state
        .shell
        .forms
        .colors
        .palette_pick("colors.palette.saveAs");
    h.run();
    let save = h.get_by_label("Save");
    assert!(
        !save.accesskit_node().is_disabled(),
        "unrelated retained names must not disable My Palette"
    );
    save.click();
    h.run();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert!(dir.path().join("My Palette.txt").is_file());
    for name in ["Ocean.one", "Ocean.two", "../unsafe"] {
        assert!(
            h.state()
                .shell
                .forms
                .colors
                .user_palettes
                .contains_key(name)
        );
    }
    let state = h.state_mut();
    state.shell.forms.colors.persist(&mut state.prefs);
    let stored: serde_json::Value =
        serde_json::from_str(state.prefs.get(USER_PALETTES_KEY).unwrap()).unwrap();
    assert_eq!(
        stored.as_object().unwrap().len(),
        legacy.as_object().unwrap().len()
    );
    for (name, text) in legacy.as_object().unwrap() {
        let colors = pdn_core::render::color_ui::parse_palette(stored[name].as_str().unwrap());
        assert_eq!(
            colors[0],
            u32::from_str_radix(text.as_str().unwrap(), 16).unwrap()
        );
        assert_eq!(colors.len(), 96);
    }
}

#[test]
fn fix1_migration_respects_discovered_filename_identity() {
    use paintrs::platform::palettes::PaletteStore;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ocean.blue.TXT"), "FF112233").unwrap();
    let store = PaletteStore::new(dir.path().into());
    let old = std::collections::BTreeMap::from([("Ocean.blue.txt".into(), "FF445566".into())]);
    assert!(store.migrate(&old).unwrap().is_empty());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Ocean.blue.TXT")).unwrap(),
        "FF112233"
    );
}
#[test]
fn fix1_palette_destination_checks_keep_case_collision_and_path_guards() {
    use paintrs::platform::palettes::PaletteStore;
    use std::collections::BTreeMap;
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("Ocean.TXT"), "FF112233").unwrap();
    let store = PaletteStore::new(dir.path().into());
    for name in ["Ocean.one", "../outside", "NUL"] {
        assert!(
            store
                .save(&BTreeMap::from([(name.into(), "FF445566".into())]))
                .is_err(),
            "{name}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("Ocean.TXT")).unwrap(),
        "FF112233"
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    std::fs::create_dir(dir.path().join("Directory.txt")).unwrap();
    assert!(
        store
            .save(&BTreeMap::from([("Safe".into(), "FF445566".into())]))
            .is_err()
    );
    std::fs::remove_dir(dir.path().join("Directory.txt")).unwrap();
    #[cfg(unix)]
    {
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("Link.txt")).unwrap();
        assert!(
            store
                .save(&BTreeMap::from([("Safe".into(), "FF445566".into())]))
                .is_err()
        );
        assert!(!dir.path().join("Safe.txt").exists());
    }
}

#[cfg(unix)]
fn accepted_palette_survives_denied_write(existing: bool) {
    use egui_kittest::kittest::{NodeT, Queryable};
    use paintrs::{
        forms::colors::{CURRENT_PALETTE_KEY, USER_PALETTES_KEY},
        kit::menu::MenuEntry,
        platform::palettes::PaletteStore,
    };
    use std::os::unix::fs::PermissionsExt;
    struct RestorePermissions(PathBuf, u32);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(self.1)).unwrap();
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let disk = dir.path().join("Ocean.blue.TXT");
    std::fs::write(&disk, "FF112233").unwrap();
    let mut h = harness();
    let legacy =
        serde_json::json!({"Ocean.one":"FF778899","Ocean.two":"FF445566","../unsafe":"FFABCDEF"});
    let state = h.state_mut();
    state.prefs.set(USER_PALETTES_KEY, &legacy.to_string());
    state.shell.forms.colors.load_prefs(&state.prefs);
    state
        .shell
        .forms
        .colors
        .enable_file_palettes(Some(PaletteStore::new(dir.path().into())));
    if existing {
        let id = state
            .shell
            .forms
            .colors
            .palette_menu_entries()
            .into_iter()
            .find_map(|e| match e {
                MenuEntry::Command { id, label, .. } if label == "Ocean.blue" => Some(id),
                _ => None,
            })
            .unwrap();
        state.shell.forms.colors.palette_pick(id);
    }
    let before = state.shell.forms.colors.user_palettes.clone();
    let colors = vec![pdn_core::util::color::Color::new(0x12, 0x34, 0x56, 255); 96];
    state.shell.forms.colors.palette = colors.clone();
    state
        .shell
        .forms
        .colors
        .palette_pick("colors.palette.saveAs");
    h.run();
    let name = if existing {
        "Ocean.blue"
    } else {
        "Accepted New"
    };
    if !existing {
        h.event(egui::Event::Text(name.into()));
        h.run();
    }
    assert_eq!(
        h.get_by_label("Palette name:").value().as_deref(),
        Some(name)
    );
    assert!(!h.get_by_label("Save").accesskit_node().is_disabled());
    let _restore = RestorePermissions(
        dir.path().into(),
        std::fs::metadata(dir.path()).unwrap().permissions().mode(),
    );
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o555)).unwrap();
    let denied=tempfile::NamedTempFile::new_in(dir.path()).expect_err("Unix permission fixture is inapplicable under elevated/bypassing rights; do not count it as a pass");
    assert_eq!(denied.kind(), std::io::ErrorKind::PermissionDenied);
    // Directory reads and candidate validation still succeed; only real output creation fails.
    let store = PaletteStore::new(dir.path().into());
    let destinations = store.destinations().unwrap();
    assert!(paintrs::platform::palettes::destination_name(name, &destinations).is_ok());
    h.get_by_label("Save").click();
    h.run();
    assert!(
        h.query_by_label("OK").is_some(),
        "actual write error must be reported after acceptance"
    );
    assert_eq!(
        h.state().shell.forms.colors.user_palettes.get(name),
        Some(&colors)
    );
    assert_eq!(h.state().shell.forms.colors.palette, colors);
    for (key, value) in before {
        if key != name {
            assert_eq!(
                h.state().shell.forms.colors.user_palettes.get(&key),
                Some(&value)
            );
        }
    }
    assert_eq!(std::fs::read_to_string(&disk).unwrap(), "FF112233");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let state = h.state_mut();
    state.shell.forms.colors.persist(&mut state.prefs);
    let stored: serde_json::Value =
        serde_json::from_str(state.prefs.get(USER_PALETTES_KEY).unwrap()).unwrap();
    assert_eq!(stored.as_object().unwrap().len(), 3);
    for (key, value) in legacy.as_object().unwrap() {
        assert_eq!(
            pdn_core::render::color_ui::parse_palette(stored[key].as_str().unwrap())[0],
            u32::from_str_radix(value.as_str().unwrap(), 16).unwrap()
        );
    }
    assert_eq!(
        pdn_core::render::color_ui::parse_palette(state.prefs.get(CURRENT_PALETTE_KEY).unwrap())[0],
        0xFF123456
    );
    h.get_by_label("OK").click();
    h.run();
    h.state_mut()
        .shell
        .forms
        .colors
        .palette_pick("colors.palette.saveAs");
    h.run();
    assert_eq!(
        h.get_by_label("Palette name:").value().as_deref(),
        Some(name),
        "accepted name must survive the write error"
    );
}
#[cfg(unix)]
#[test]
fn fix2_new_accepted_palette_survives_actual_denied_write() {
    accepted_palette_survives_denied_write(false);
}
#[cfg(unix)]
#[test]
fn fix2_existing_accepted_palette_survives_actual_denied_write() {
    accepted_palette_survives_denied_write(true);
}
/// File ▸ Batch Export: one image's file type changes alone, Apply to All gives it to every
/// image, and OK writes each under its own name (the untitled one numbered) in the images'
/// folder; the file that already exists is kept on Skip.
#[test]
fn batch_export_writes_every_image_with_its_own_settings() {
    use egui_kittest::kittest::Queryable;
    use pdn_core::action::{ActionOutcome, BATCH_EXPORT, perform};
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("two.webp"), b"old").unwrap();
    Files::system().install();
    let mut h = harness();
    let state = h.state_mut();
    state.app = pdn_core::testing::fresh_doc(24, 16, None);
    state.app.documents[0].is_initial_workspace = false;
    for (name, format) in [("one.jpg", "jpeg"), ("two.png", "png")] {
        let i = state
            .app
            .create_blank_document_in_new_workspace(20, 12, 96., false);
        let path = dir.path().join(name).to_string_lossy().into_owned();
        let ws = &mut state.app.documents[i];
        ws.set_file_info(
            Some(name.into()),
            Some(path),
            Some(format.into()),
            serde_json::Value::Null,
        );
    }
    let ActionOutcome::Ui(request) = perform(&mut state.app, BATCH_EXPORT) else {
        panic!("Batch Export is enabled with images open");
    };
    state.app.ui_requests.push(request);
    h.run();
    // The dialog's strip item, not the document strip's tab above it.
    let image = |h: &Harness<'static, PaintApp>, name: &str| {
        let lowest = h
            .get_all_by_label(name)
            .max_by(|a, b| a.rect().min.y.total_cmp(&b.rect().min.y))
            .unwrap();
        lowest.click();
    };
    let value = |h: &Harness<'static, PaintApp>, label: &str| h.get_by_label(label).value();
    image(&h, "one.jpg");
    h.run();
    assert_eq!(
        value(&h, "File type").as_deref(),
        Some("JPEG (.jpg, .jpeg, .jpe)")
    );
    h.get_by_label("File type").click();
    h.run();
    h.get_by_label("WebP (.webp)").click();
    h.run();
    assert_eq!(value(&h, "File name").as_deref(), Some("one.webp"));
    image(&h, "two.png");
    h.run();
    assert_eq!(value(&h, "File type").as_deref(), Some("PNG (.png)"));
    image(&h, "one.jpg");
    h.run();
    h.get_by_label("Apply to All").click();
    h.run();
    image(&h, "two.png");
    h.run();
    assert_eq!(value(&h, "File type").as_deref(), Some("WebP (.webp)"));
    assert_eq!(value(&h, "File name").as_deref(), Some("two.webp"));
    h.get_by_label("OK").click();
    h.run();
    h.get_by_label("Skip").click();
    for _ in 0..500 {
        h.run();
        if !h.state().shell.dialogs.is_open() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!h.state().shell.dialogs.is_open(), "the export finishes");
    for name in ["untitled_01.webp", "one.webp"] {
        let bytes = std::fs::read(dir.path().join(name)).unwrap();
        let format = pdn_core::io::codecs::detect_format(name, None, &bytes);
        assert_eq!(format, Some("webp"), "{name}");
    }
    assert_eq!(std::fs::read(dir.path().join("two.webp")).unwrap(), b"old");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 3);
}
