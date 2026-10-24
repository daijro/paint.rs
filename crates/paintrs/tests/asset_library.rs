use paintrs::prefs::Prefs;
use paintrs::{
    assets::{Assets, IconSource},
    icon_pack::IconPackId,
    platform::{
        PickedFile,
        asset_library::{AssetLibrary, AssetStore, iso_to_ms, ms_to_iso},
    },
};
use pdn_core::{app::AppWorkspace, asset_pack::AssetPack};
use std::collections::BTreeMap;
const PNG: &[u8] = include_bytes!("../../../tests/fixtures/icon-64.png");
const XAML: &str = r#"<s:SimpleGeometryShape xmlns:s="clr-namespace:PaintDotNet.Shapes;assembly=PaintDotNet.Framework" xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.UI" DisplayName="Tri" Geometry="M0,0 L10,0 0,10Z"/>"#;
fn drain(m: &mut AssetLibrary, ctx: &egui::Context, a: &mut AppWorkspace, p: &mut Prefs) {
    for _ in 0..10000 {
        m.poll(ctx, a, p);
        if !m.busy() {
            return;
        }
        std::thread::yield_now();
    }
    panic!("asset operation did not finish");
}
#[test]
fn user_pack_wins_in_all_asset_paths_and_removal_restores_paint_rs() {
    let ctx = egui::Context::default();
    assert_eq!(
        Assets::icon_source(&ctx, "menu_file_new_icon.png"),
        IconSource::PaintRs
    );
    let old = Assets::icon(&ctx, "menu_file_new_icon.png").unwrap().id();
    let mut files = BTreeMap::new();
    for path in [
        "icons/menu_file_new_icon.png",
        "images/animation_busy_spinner.png",
        "cursors/paint_brush_tool_cursor.png",
    ] {
        files.insert(format!("assets/{path}"), PNG.to_vec());
    }
    Assets::set_pack(
        &ctx,
        Some(AssetPack {
            id: "manifest".into(),
            manifest: serde_json::json!({}),
            files,
            translations: Default::default(),
        }),
    );
    Assets::select(&ctx, IconPackId::User("manifest".into()));
    assert_eq!(
        Assets::icon_source(&ctx, "menu_file_new_icon.png"),
        IconSource::UserPack
    );
    assert_ne!(
        Assets::icon(&ctx, "menu_file_new_icon.png").unwrap().id(),
        old
    );
    assert!(Assets::image(&ctx, "animation_busy_spinner.png").is_some());
    assert!(Assets::cursor(&ctx, "paint_brush_tool_cursor").is_some());
    Assets::set_pack(&ctx, None);
    Assets::select(&ctx, IconPackId::PaintRs);
    assert_eq!(
        Assets::icon_source(&ctx, "menu_file_new_icon.png"),
        IconSource::PaintRs
    );
    assert_eq!(
        Assets::icon_source(&ctx, "settings_assets_24.png"),
        IconSource::PaintRs
    );
}
#[test]
fn shape_storage_reparses_replaces_case_and_keeps_failed_writes_out_of_catalog() {
    let store = AssetStore::memory();
    let mut m = AssetLibrary::new(store.clone());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    drain(&mut m, &ctx, &mut app, &mut prefs);
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "TÉST.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    drain(&mut m, &ctx, &mut app, &mut prefs);
    assert_eq!(m.shapes.len(), 1);
    let first = m.shapes[0].clone();
    assert!(app.shape_catalog.get(&first.id).is_some());
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "tést.xaml".into(),
            XAML.replace("Tri", "New").into_bytes(),
        )],
    );
    drain(&mut m, &ctx, &mut app, &mut prefs);
    assert_eq!(m.shapes.len(), 1);
    assert_eq!(m.shapes[0].id, first.id);
    assert_eq!(m.shapes[0].imported_at, first.imported_at);
    assert_eq!(app.shape_catalog.get(&first.id).unwrap().name, "New");
    store.fail_writes(true);
    m.remove_shape(&first.id);
    drain(&mut m, &ctx, &mut app, &mut prefs);
    assert!(app.shape_catalog.get(&first.id).is_some());
    assert_eq!(m.shapes.len(), 1);
    store.fail_writes(false);
    let mut loaded = AssetLibrary::new(store);
    drain(&mut loaded, &ctx, &mut app, &mut prefs);
    assert_eq!(loaded.shapes[0].name, "New");
    loaded.remove_shape(&first.id);
    drain(&mut loaded, &ctx, &mut app, &mut prefs);
    assert!(app.shape_catalog.get(&first.id).is_none());
}
#[test]
fn timestamp_wire_adapter_is_exact_and_rejects_invalid_records() {
    for ms in [0., -1., 951868800123., 1792536212345.] {
        assert_eq!(iso_to_ms(&ms_to_iso(ms).unwrap()).unwrap(), ms);
    }
    assert_eq!(ms_to_iso(0.).unwrap(), "1970-01-01T00:00:00.000Z");
    assert!(ms_to_iso(f64::NAN).is_err());
    assert!(iso_to_ms("bad").is_err());
}
fn pack_zip() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut w = zip::ZipWriter::new(Cursor::new(vec![]));
    for (name, data) in [
        (
            "asset-pack.json",
            &br#"{"formatVersion":1,"name":"Translated","id":"same-manifest"}"#[..],
        ),
        ("icons/menu_file_new_icon.png", PNG),
        ("lang/en.json", &br#"{"k":"English"}"#[..]),
        ("lang/de-DE.json", &br#"{"k":"Deutsch"}"#[..]),
    ] {
        w.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}
#[test]
fn every_pack_import_has_fresh_installed_id_and_reload_dictionaries_replace() {
    use paintrs::platform::asset_library::{ACTIVE_KEY, LOCALES_KEY, TRANSLATIONS_KEY};
    let store = AssetStore::memory();
    let mut m = AssetLibrary::new(store);
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    drain(&mut m, &ctx, &mut app, &mut prefs);
    for _ in 0..2 {
        m.import(
            "assets",
            vec![PickedFile::snapshot("same.zip".into(), pack_zip())],
        );
        drain(&mut m, &ctx, &mut app, &mut prefs);
    }
    assert_eq!(m.packs.len(), 2);
    assert_ne!(m.packs[0].id, m.packs[1].id);
    assert_eq!(m.packs[0].pack.id, "same-manifest");
    assert_ne!(m.packs[0].id, "same-manifest");
    m.prepare_language(&mut prefs, "de-AT", "en");
    let strings: serde_json::Value =
        serde_json::from_str(prefs.get(TRANSLATIONS_KEY).unwrap()).unwrap();
    assert_eq!(strings["en"]["k"], "English");
    assert_eq!(strings["de-DE"]["k"], "Deutsch");
    assert_eq!(strings.as_object().unwrap().len(), 2);
    m.prepare_language(&mut prefs, "fr", "en");
    let strings: serde_json::Value =
        serde_json::from_str(prefs.get(TRANSLATIONS_KEY).unwrap()).unwrap();
    assert_eq!(strings.as_object().unwrap().len(), 1);
    let active = m.active_id().to_owned();
    m.remove_pack(&active, &ctx, &mut prefs);
    drain(&mut m, &ctx, &mut app, &mut prefs);
    assert_eq!(prefs.get(ACTIVE_KEY), Some("default"));
    assert!(prefs.get(TRANSLATIONS_KEY).is_none());
    assert!(prefs.get(LOCALES_KEY).is_none());
    assert_eq!(m.packs.len(), 1);
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_sessions_preserve_other_records_and_report_corrupt_source_without_erasing() {
    let root = tempfile::tempdir().unwrap();
    let store = AssetStore::native(root.path().into());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    fn wait(m: &mut AssetLibrary, c: &egui::Context, a: &mut AppWorkspace, p: &mut Prefs) {
        let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while std::time::Instant::now() < end {
            m.poll(c, a, p);
            if !m.busy() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("native asset operation timed out");
    }
    let mut first = AssetLibrary::new(store.clone());
    let mut second = AssetLibrary::new(store.clone());
    wait(&mut first, &ctx, &mut app, &mut prefs);
    wait(&mut second, &ctx, &mut app, &mut prefs);
    for (manager, name) in [(&mut first, "One.xaml"), (&mut second, "Two.xaml")] {
        manager.import(
            "shapes",
            vec![PickedFile::snapshot(name.into(), XAML.as_bytes().to_vec())],
        );
        wait(manager, &ctx, &mut app, &mut prefs);
    }
    let mut restored = AssetLibrary::new(store.clone());
    wait(&mut restored, &ctx, &mut app, &mut prefs);
    assert_eq!(restored.shapes.len(), 2);
    let record = &restored.shapes[0];
    let file = root
        .path()
        .join("shapes")
        .join(format!("{}.json", record.id));
    let mut raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    raw["source"] = serde_json::json!("<bad/>");
    raw["size"] = serde_json::json!(6);
    std::fs::write(&file, serde_json::to_vec(&raw).unwrap()).unwrap();
    let original = std::fs::read(&file).unwrap();
    let mut restored = AssetLibrary::new(store);
    wait(&mut restored, &ctx, &mut app, &mut prefs);
    assert!(!restored.errors.is_empty());
    assert_eq!(std::fs::read(&file).unwrap(), original);
    assert!(raw["importedAt"].as_str().unwrap().ends_with('Z'));
    assert!(!root.path().join("packs").exists());
}
#[test]
fn image_namespace_does_not_override_an_icon_of_the_same_name() {
    let ctx = egui::Context::default();
    let files = BTreeMap::from([(
        "assets/images/menu_file_new_icon.png".to_owned(),
        PNG.to_vec(),
    )]);
    Assets::set_pack(
        &ctx,
        Some(AssetPack {
            id: "images".into(),
            manifest: serde_json::json!({}),
            files,
            translations: Default::default(),
        }),
    );
    Assets::select(&ctx, IconPackId::User("images".into()));
    assert_eq!(
        Assets::icon_source(&ctx, "menu_file_new_icon.png"),
        IconSource::PaintRs
    );
    assert!(!Assets::is_overridden(&ctx, "menu_file_new_icon.png"));
}
/// A user pack's `assets/images/` overrides the five UI images in every build (R221 review I1).
#[test]
fn user_pack_images_override_the_ui_images() {
    let ctx = egui::Context::default();
    let name = "anchor_chooser_control_anchor_image.png";
    let files = BTreeMap::from([(format!("assets/images/{name}"), PNG.to_vec())]);
    Assets::set_pack(
        &ctx,
        Some(AssetPack {
            id: "ui-images".into(),
            manifest: serde_json::json!({}),
            files,
            translations: Default::default(),
        }),
    );
    Assets::select(&ctx, IconPackId::User("ui-images".into()));
    assert_eq!(Assets::icon_source(&ctx, name), IconSource::UserPack);
    assert!(Assets::is_overridden(&ctx, name));
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn native_import_read_budget_is_independent_of_document_budget() {
    let file = tempfile::NamedTempFile::new().unwrap();
    file.as_file().set_len(512 * 1024 + 1).unwrap();
    assert!(paintrs::platform::native::read_bytes_within(file.path(), 512 * 1024).is_err());
    assert_eq!(
        paintrs::platform::native::read_bytes_within(file.path(), 512 * 1024 + 1)
            .unwrap()
            .len(),
        512 * 1024 + 1
    );
}

#[cfg(not(target_arch = "wasm32"))]
fn wait_native(
    m: &mut AssetLibrary,
    ctx: &egui::Context,
    app: &mut AppWorkspace,
    prefs: &mut Prefs,
) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        m.poll(ctx, app, prefs);
        if !m.busy() {
            break;
        }
        assert!(
            std::time::Instant::now() < end,
            "native asset operation timed out"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_clear_precommit_directory_error_preserves_records() {
    let root = tempfile::tempdir().unwrap();
    let mut m = AssetLibrary::new(AssetStore::native(root.path().into()));
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "First.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    let id = m.shapes[0].id.clone();
    let dir = root.path().join("shapes");
    let valid = dir.join(format!("{id}.json"));
    // Build a real I/O failure after a real deletion without depending on the filesystem's
    // insertion order (some enumerate newly created entries first, others last).
    let blocker = (0..32)
        .find_map(|i| {
            let path = dir.join(format!("zzzz-blocked-{i}.json"));
            std::fs::create_dir(&path).unwrap();
            for relocate in [false, true] {
                if relocate {
                    let held = root.path().join("held-record");
                    std::fs::rename(&valid, &held).unwrap();
                    std::fs::rename(held, &valid).unwrap();
                }
                let paths = std::fs::read_dir(&dir)
                    .unwrap()
                    .map(|e| e.unwrap().path())
                    .collect::<Vec<_>>();
                if paths.iter().position(|p| p == &valid).unwrap()
                    < paths.iter().position(|p| p == &path).unwrap()
                {
                    return Some(path);
                }
            }
            std::fs::remove_dir(path).unwrap();
            None
        })
        .unwrap_or_else(|| {
            // Directory-first filesystems cannot produce a later directory error. They still
            // pin precommit preservation; the isolated write-error case is order-independent.
            let path = dir.join("blocked-fallback.json");
            std::fs::create_dir(&path).unwrap();
            path
        });
    let original = std::fs::read(&valid).unwrap();
    m.clear_shapes();
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    assert!(
        valid.exists(),
        "a failed clear must preserve every original record"
    );
    assert_eq!(std::fs::read(&valid).unwrap(), original);
    assert_eq!(m.shapes.len(), 1);
    assert!(app.shape_catalog.get(&id).is_some());
    assert!(m.status.contains("1 remaining"));
    assert!(!m.errors.is_empty());
    assert!(
        blocker.is_dir(),
        "do not recursively delete unknown directory contents"
    );
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_missing_or_invalid_selected_pack_resets_reload_preferences() {
    use paintrs::platform::asset_library::{ACTIVE_KEY, LOCALES_KEY, TRANSLATIONS_KEY};
    for invalid in [false, true] {
        let root = tempfile::tempdir().unwrap();
        if invalid {
            std::fs::create_dir(root.path().join("packs")).unwrap();
            std::fs::write(
                root.path().join("packs/gone.json"),
                br#"{"id":"gone","name":"Broken"}"#,
            )
            .unwrap();
        }
        let mut prefs = Prefs::default();
        prefs.set(ACTIVE_KEY, "gone");
        prefs.set(TRANSLATIONS_KEY, r#"{"en":{"k":"stale"}}"#);
        prefs.set(LOCALES_KEY, r#"["en"]"#);
        let mut m = AssetLibrary::new(AssetStore::native(root.path().into()));
        let ctx = egui::Context::default();
        let mut app = AppWorkspace::default();
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        assert_eq!(m.active_id(), "default");
        assert_eq!(prefs.get(ACTIVE_KEY), Some("default"));
        assert!(prefs.get(TRANSLATIONS_KEY).is_none());
        assert!(prefs.get(LOCALES_KEY).is_none());
        if invalid {
            assert!(root.path().join("packs/gone.json").exists());
            assert!(!m.errors.is_empty());
        }
    }
}

#[cfg(unix)]
#[test]
fn review_native_clear_precommit_write_error_preserves_records() {
    const CHILD: &str = "PAINTRS_TASK54_CLEAR_FAILURE_CHILD";
    if let Some(root) = std::env::var_os(CHILD) {
        let root = std::path::PathBuf::from(root);
        let error = std::fs::write(root.join("rlimit-probe"), b"x").unwrap_err();
        assert_eq!(
            error.kind(),
            std::io::ErrorKind::FileTooLarge,
            "the child must actually enforce RLIMIT_FSIZE"
        );
        println!("kernel RLIMIT_FSIZE probe: {:?}", error.kind());
        let before = std::fs::read_dir(root.join("shapes"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|e| e == "json"))
            .map(|p| {
                let bytes = std::fs::read(&p).unwrap();
                (p, bytes)
            })
            .collect::<Vec<_>>();
        let mut m = AssetLibrary::new(AssetStore::native(root));
        let ctx = egui::Context::default();
        let mut app = AppWorkspace::default();
        let mut prefs = Prefs::default();
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        let ids = m.shapes.iter().map(|r| r.id.clone()).collect::<Vec<_>>();
        m.clear_shapes();
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        println!("precommit write failure: {}", m.status);
        assert!(
            m.status.starts_with("Could not clear all"),
            "the marker write must fail before commit"
        );
        assert_eq!(m.shapes.len(), ids.len());
        for id in ids {
            assert!(app.shape_catalog.get(&id).is_some());
        }
        for (path, bytes) in before {
            assert_eq!(std::fs::read(path).unwrap(), bytes);
        }
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut m = AssetLibrary::new(AssetStore::native(root.path().into()));
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "Original.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("trap '' XFSZ; ulimit -f 0; exec \"$1\" --exact \"$2\" --nocapture")
        .arg("task54-precommit-test")
        .arg(std::env::current_exe().unwrap())
        .arg("review_native_clear_precommit_write_error_preserves_records")
        .env(CHILD, root.path())
        .output()
        .unwrap();
    println!(
        "RLIMIT_FSIZE child raw exit: {:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
    assert!(
        output.status.success(),
        "child exit {:?}\nstdout: {}\nstderr: {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_clear_commit_survives_cleanup_interruption_and_later_put() {
    let root = tempfile::tempdir().unwrap();
    let store = AssetStore::native(root.path().into());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut stale = AssetLibrary::new(store.clone());
    wait_native(&mut stale, &ctx, &mut app, &mut prefs);
    stale.import(
        "shapes",
        vec![PickedFile::snapshot(
            "Same.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    wait_native(&mut stale, &ctx, &mut app, &mut prefs);
    let id = stale.shapes[0].id.clone();
    let payload = root.path().join("shapes").join(format!("{id}.json"));
    // Exact on-disk state after metadata commit but before payload cleanup/crash recovery.
    std::fs::write(
        root.path().join("shapes/.cleared-shapes-v1"),
        format!("paintrs-cleared-shapes-v1\n{id}\n"),
    )
    .unwrap();
    let mut reloaded = AssetLibrary::new(store.clone());
    let mut fresh = AppWorkspace::default();
    wait_native(&mut reloaded, &ctx, &mut fresh, &mut prefs);
    assert!(reloaded.shapes.is_empty());
    assert!(fresh.shape_catalog.get(&id).is_none());
    assert!(
        !payload.exists(),
        "a locked reload must retry committed cleanup"
    );
    stale.import(
        "shapes",
        vec![PickedFile::snapshot(
            "same.xaml".into(),
            XAML.replace("Tri", "Later").into_bytes(),
        )],
    );
    wait_native(&mut stale, &ctx, &mut app, &mut prefs);
    assert_eq!(stale.shapes[0].id, id);
    let mut final_load = AssetLibrary::new(store);
    let mut fresh = AppWorkspace::default();
    wait_native(&mut final_load, &ctx, &mut fresh, &mut prefs);
    assert_eq!(final_load.shapes.len(), 1);
    assert_eq!(final_load.shapes[0].name, "Later");
    assert!(fresh.shape_catalog.get(&id).is_some());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_clear_preserves_unknown_contents_in_place() {
    let root = tempfile::tempdir().unwrap();
    let store = AssetStore::native(root.path().into());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut m = AssetLibrary::new(store.clone());
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "Owned.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    let dir = root.path().join("shapes");
    std::fs::write(dir.join("notes.txt"), b"keep text").unwrap();
    std::fs::write(
        dir.join("unknown.json"),
        br#"{"id":"unknown","unrecognized":true}"#,
    )
    .unwrap();
    std::fs::create_dir(dir.join("unknown-directory")).unwrap();
    std::fs::write(dir.join("unknown-directory/content"), b"keep nested").unwrap();
    m.clear_shapes();
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    assert!(m.shapes.is_empty());
    assert!(m.status.starts_with("Removed all"));
    assert_eq!(std::fs::read(dir.join("notes.txt")).unwrap(), b"keep text");
    assert_eq!(
        std::fs::read(dir.join("unknown.json")).unwrap(),
        br#"{"id":"unknown","unrecognized":true}"#
    );
    assert_eq!(
        std::fs::read(dir.join("unknown-directory/content")).unwrap(),
        b"keep nested"
    );
    let mut restored = AssetLibrary::new(store);
    let mut fresh = AppWorkspace::default();
    wait_native(&mut restored, &ctx, &mut fresh, &mut prefs);
    assert!(restored.shapes.is_empty());
}
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_clear_marker_rejects_malformed_and_oversize_before_loading() {
    let root = tempfile::tempdir().unwrap();
    let store = AssetStore::native(root.path().into());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut m = AssetLibrary::new(store.clone());
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    m.import(
        "shapes",
        vec![PickedFile::snapshot(
            "Owned.xaml".into(),
            XAML.as_bytes().to_vec(),
        )],
    );
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    let id = m.shapes[0].id.clone();
    let payload = root.path().join("shapes").join(format!("{id}.json"));
    let original = std::fs::read(&payload).unwrap();
    let marker = root.path().join("shapes/.cleared-shapes-v1");
    for bytes in [
        b"unknown format\n".to_vec(),
        format!("paintrs-cleared-shapes-v1\n{}\n", "a".repeat(121)).into_bytes(),
        b"paintrs-cleared-shapes-v1\n../escape\n".to_vec(),
        b"paintrs-cleared-shapes-v1\nunterminated".to_vec(),
        format!("paintrs-cleared-shapes-v1\n{id}\n{id}\n").into_bytes(),
    ] {
        std::fs::write(&marker, &bytes).unwrap();
        let mut bad = AssetLibrary::new(store.clone());
        let mut fresh = AppWorkspace::default();
        wait_native(&mut bad, &ctx, &mut fresh, &mut prefs);
        assert!(!bad.errors.is_empty());
        assert!(bad.shapes.is_empty());
        assert_eq!(std::fs::read(&marker).unwrap(), bytes);
        assert_eq!(std::fs::read(&payload).unwrap(), original);
    }
    let file = std::fs::File::create(&marker).unwrap();
    file.set_len(512 * 1024 * 1024).unwrap();
    let mut bad = AssetLibrary::new(store);
    let mut fresh = AppWorkspace::default();
    wait_native(&mut bad, &ctx, &mut fresh, &mut prefs);
    assert!(bad.errors.iter().any(|e| e.contains("marker")));
    assert_eq!(std::fs::read(&payload).unwrap(), original);
}

#[cfg(unix)]
#[test]
fn review_native_clear_rejects_symlinked_marker_and_stable_lock_without_changes() {
    for marker in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        let mut app = AppWorkspace::default();
        let mut prefs = Prefs::default();
        let mut m = AssetLibrary::new(AssetStore::native(root.path().into()));
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        m.import(
            "shapes",
            vec![PickedFile::snapshot(
                "Original.xaml".into(),
                XAML.as_bytes().to_vec(),
            )],
        );
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        let id = m.shapes[0].id.clone();
        let payload = root.path().join("shapes").join(format!("{id}.json"));
        let original = std::fs::read(&payload).unwrap();
        let outside = root.path().join("unrelated");
        std::fs::write(&outside, b"preserve unrelated bytes").unwrap();
        let reserved = if marker {
            root.path().join("shapes/.cleared-shapes-v1")
        } else {
            root.path().join("shapes.lock")
        };
        if reserved.exists() {
            std::fs::remove_file(&reserved).unwrap();
        }
        std::os::unix::fs::symlink(&outside, &reserved).unwrap();
        m.clear_shapes();
        wait_native(&mut m, &ctx, &mut app, &mut prefs);
        assert_eq!(std::fs::read(&payload).unwrap(), original);
        assert_eq!(m.shapes.len(), 1);
        assert!(app.shape_catalog.get(&id).is_some());
        assert!(!m.errors.is_empty());
        assert_eq!(
            std::fs::read(&outside).unwrap(),
            b"preserve unrelated bytes"
        );
        assert!(
            std::fs::symlink_metadata(reserved)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_cleanup_warns_preserves_unknown_and_retries_on_load() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("shapes");
    std::fs::create_dir(&dir).unwrap();
    let payload = dir.join("blocked.json");
    std::fs::create_dir(&payload).unwrap();
    std::fs::write(payload.join("unknown"), b"must remain").unwrap();
    std::fs::write(
        dir.join(".cleared-shapes-v1"),
        b"paintrs-cleared-shapes-v1\nblocked\n",
    )
    .unwrap();
    let store = AssetStore::native(root.path().into());
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut m = AssetLibrary::new(store.clone());
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    assert!(m.shapes.is_empty());
    assert!(
        m.errors.iter().any(|e| e.contains("cleanup")),
        "a committed cleanup failure must be visible: {:?}",
        m.errors
    );
    assert_eq!(
        std::fs::read(payload.join("unknown")).unwrap(),
        b"must remain"
    );
    std::fs::remove_file(payload.join("unknown")).unwrap();
    std::fs::remove_dir(&payload).unwrap();
    std::fs::write(&payload, serde_json::to_vec(&serde_json::json!({"id":"blocked", "fileName":"Original.xaml", "name":"Tri", "size":XAML.len(), "source":XAML, "importedAt":"2026-10-20T00:00:00.000Z", "updatedAt":"2026-10-20T00:00:00.000Z"})).unwrap()).unwrap();
    let mut reloaded = AssetLibrary::new(store);
    wait_native(&mut reloaded, &ctx, &mut app, &mut prefs);
    assert!(reloaded.shapes.is_empty());
    assert!(!payload.exists(), "later locked load must finish cleanup");
    assert!(reloaded.errors.is_empty(), "{:?}", reloaded.errors);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn review_native_clear_committed_cleanup_warning_is_not_unconditional_success() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("shapes");
    std::fs::create_dir(&dir).unwrap();
    std::fs::create_dir(dir.join("blocked.json")).unwrap();
    std::fs::write(dir.join("blocked.json/unknown"), b"preserve").unwrap();
    std::fs::write(
        dir.join(".cleared-shapes-v1"),
        b"paintrs-cleared-shapes-v1\nblocked\n",
    )
    .unwrap();
    let ctx = egui::Context::default();
    let mut app = AppWorkspace::default();
    let mut prefs = Prefs::default();
    let mut m = AssetLibrary::new(AssetStore::native(root.path().into()));
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    m.clear_shapes();
    wait_native(&mut m, &ctx, &mut app, &mut prefs);
    assert!(m.shapes.is_empty());
    assert!(
        m.status.contains("cleanup pending"),
        "committed clear must distinguish pending physical cleanup: {}",
        m.status
    );
    assert!(!m.status.starts_with("Removed all"));
    assert!(m.errors.iter().any(|e| e.contains("cleanup")));
    assert_eq!(
        std::fs::read(dir.join("blocked.json/unknown")).unwrap(),
        b"preserve"
    );
}
