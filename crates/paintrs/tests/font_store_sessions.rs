#![cfg(not(target_arch = "wasm32"))]
use paintrs::platform::{
    PickedFile,
    fonts::{FontManager, FontStore},
};
use pdn_core::fonts::{FontPolicy, FontRecord, SELAWIK_REGULAR};
use std::{
    path::Path,
    time::{Duration, Instant},
};
fn wait(m: &mut FontManager) {
    let end = Instant::now() + Duration::from_secs(15);
    while Instant::now() < end {
        m.poll();
        if !m.busy() {
            return;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    panic!("font operation did not finish: {}", m.status);
}
fn manager(root: &Path) -> FontManager {
    let mut m = FontManager::new(FontStore::native(root.into()), FontPolicy::Bundled);
    wait(&mut m);
    m
}
fn add(m: &mut FontManager, name: &str) {
    m.import(vec![PickedFile::snapshot(
        name.into(),
        SELAWIK_REGULAR.to_vec(),
    )]);
    wait(m);
    assert!(m.errors().is_empty(), "{:?}", m.errors());
}
fn names(records: &[FontRecord]) -> Vec<String> {
    let mut names: Vec<_> = records.iter().map(|r| r.name.clone()).collect();
    names.sort();
    names
}
fn stored(root: &Path) -> Vec<FontRecord> {
    let m = manager(root);
    for r in &m.registry.records {
        assert_eq!(
            std::fs::read(root.join(format!("{}.{}", r.id, r.extension()))).unwrap(),
            SELAWIK_REGULAR
        );
    }
    m.registry.records
}
#[test]
fn stale_sessions_apply_only_requested_add_remove_and_clear() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut a = manager(root);
    let mut b = manager(root);
    add(&mut a, "A.ttf");
    let aid = a.registry.records[0].id.clone();
    add(&mut b, "B.ttf");
    assert_eq!(
        names(&b.registry.records),
        ["A", "B"],
        "Import must return the committed union, not the stale startup snapshot"
    );
    assert_eq!(names(&stored(root)), ["A", "B"]);
    let bid = b
        .registry
        .records
        .iter()
        .find(|r| r.name == "B")
        .unwrap()
        .id
        .clone();
    a.remove(&aid);
    wait(&mut a);
    assert_eq!(names(&a.registry.records), ["B"]);
    assert!(!root.join(format!("{aid}.ttf")).exists());
    add(&mut b, "C.ttf");
    assert_eq!(
        names(&b.registry.records),
        ["B", "C"],
        "A stale import must not resurrect deleted A"
    );
    a.remove(&bid);
    wait(&mut a);
    assert_eq!(
        names(&a.registry.records),
        ["C"],
        "A targeted delete must preserve unknown C"
    );
    assert!(!root.join(format!("{bid}.ttf")).exists());
    add(&mut a, "E.ttf");
    b.clear();
    wait(&mut b);
    assert!(b.registry.records.is_empty());
    assert!(b.status.contains("Removed 2"));
    assert!(stored(root).is_empty());
    add(&mut a, "D.ttf");
    assert_eq!(
        names(&stored(root)),
        ["D"],
        "A stale add after explicit Clear must not resurrect C"
    );
}
#[test]
fn native_process_writer() {
    let Some(root) = std::env::var_os("PAINTRS_FONT_SESSION_STORE") else {
        return;
    };
    let root = std::path::PathBuf::from(root);
    let label = std::env::var("PAINTRS_FONT_SESSION_LABEL").unwrap();
    let mut m = manager(&root);
    assert!(m.registry.records.is_empty());
    std::fs::write(root.join(format!("{label}.ready")), b"ready").unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while !root.join("start").exists() {
        assert!(
            Instant::now() < end,
            "parent did not release writer barrier"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let probe = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(root.join("fonts.lock"))
        .unwrap();
    assert!(
        matches!(probe.try_lock(), Err(std::fs::TryLockError::WouldBlock)),
        "The parent must hold the same process-shared lock inode"
    );
    println!("{label}: observed cross-process WouldBlock on the persistent fonts.lock inode");
    std::fs::write(
        root.join(format!("{label}.blocked")),
        b"blocked by parent file lock",
    )
    .unwrap();
    m.import(
        (0..8)
            .map(|i| PickedFile::snapshot(format!("{label}-{i}.ttf"), SELAWIK_REGULAR.to_vec()))
            .collect(),
    );
    wait(&mut m);
    assert!(m.errors().is_empty(), "{:?}", m.errors());
}
#[test]
fn overlapping_native_processes_preserve_every_committed_font() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut children = vec![];
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(root.join("fonts.lock"))
        .unwrap();
    lock.lock().unwrap();
    for label in ["ProcessA", "ProcessB"] {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .args(["--exact", "native_process_writer", "--nocapture"])
                .env("PAINTRS_FONT_SESSION_STORE", root)
                .env("PAINTRS_FONT_SESSION_LABEL", label)
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap(),
        );
    }
    let end = Instant::now() + Duration::from_secs(15);
    while !["ProcessA.ready", "ProcessB.ready"]
        .iter()
        .all(|p| root.join(p).exists())
    {
        assert!(
            Instant::now() < end,
            "writers did not reach shared-start barrier"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    std::fs::write(root.join("start"), b"start").unwrap();
    while !["ProcessA.blocked", "ProcessB.blocked"]
        .iter()
        .all(|p| root.join(p).exists())
    {
        assert!(
            Instant::now() < end,
            "writers did not observe OS lock contention"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    lock.unlock().unwrap();
    for child in children {
        let out = child.wait_with_output().unwrap();
        println!(
            "child process result: {}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            out.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let records = stored(root);
    let mut expected: Vec<_> = ["ProcessA", "ProcessB"]
        .into_iter()
        .flat_map(|label| (0..8).map(move |i| format!("{label}-{i}")))
        .collect();
    expected.sort();
    assert_eq!(
        names(&records),
        expected,
        "Independent processes must serialize read/modify/commit and retain all original blobs"
    );
}

#[cfg(unix)]
#[test]
fn native_lock_file_validation_preserves_storage_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("fonts");
    std::fs::create_dir(&root).unwrap();
    let outside = dir.path().join("outside");
    std::fs::write(&outside, b"preserve").unwrap();
    std::os::unix::fs::symlink(&outside, root.join("fonts.lock")).unwrap();
    let mut m = manager(&root);
    m.import(vec![PickedFile::snapshot(
        "Rejected.ttf".into(),
        SELAWIK_REGULAR.to_vec(),
    )]);
    wait(&mut m);
    assert!(!m.errors().is_empty());
    assert!(m.registry.records.is_empty());
    assert!(!root.join("fonts.json").exists());
    assert_eq!(std::fs::read(&outside).unwrap(), b"preserve");
    std::fs::remove_file(root.join("fonts.lock")).unwrap();
    std::fs::create_dir(root.join("fonts.lock")).unwrap();
    m.import(vec![PickedFile::snapshot(
        "Rejected.ttf".into(),
        SELAWIK_REGULAR.to_vec(),
    )]);
    wait(&mut m);
    assert!(!m.errors().is_empty());
    assert!(!root.join("fonts.json").exists());
}
#[test]
fn failed_stale_remove_preserves_newer_committed_state() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let mut a = manager(root);
    add(&mut a, "KeepA.ttf");
    let mut b = manager(root);
    let aid = b.registry.records[0].id.clone();
    add(&mut a, "KeepB.ttf");
    let before = std::fs::read(root.join("fonts.json")).unwrap();
    std::fs::rename(root.join("fonts.lock"), root.join("saved.lock")).unwrap();
    std::fs::create_dir(root.join("fonts.lock")).unwrap();
    b.remove(&aid);
    wait(&mut b);
    assert_eq!(names(&b.registry.records), ["KeepA"]);
    assert_eq!(std::fs::read(root.join("fonts.json")).unwrap(), before);
    assert_eq!(names(&stored(root)), ["KeepA", "KeepB"]);
    std::fs::remove_dir(root.join("fonts.lock")).unwrap();
    std::fs::rename(root.join("saved.lock"), root.join("fonts.lock")).unwrap();
    b.remove(&aid);
    wait(&mut b);
    assert_eq!(names(&b.registry.records), ["KeepB"]);
    assert_eq!(names(&stored(root)), ["KeepB"]);
}
