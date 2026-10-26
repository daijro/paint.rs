//! Port of: paintdotjs js/platform/desktop/desktop.js; js/src/io/DocumentIO.js
//! Reference: paintdotjs @ b4e6fc5.
//! Native file adapters. Port: desktop/desktop.js; native save-and-close-flows §8.
use super::{Files, Pending, PickedFile};
use crate::dialogs::flows::{
    SaveDestination, SaveRequest, SaveTarget, TargetRequest, WriteOutcome,
};
use std::{
    cell::RefCell,
    collections::HashMap,
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    task::Poll,
};
#[derive(Clone)]
struct NativeTarget {
    destination: SaveDestination,
    path: PathBuf,
}
#[derive(Default)]
pub struct FileState {
    current: HashMap<u64, NativeTarget>,
    chosen: HashMap<u64, NativeTarget>,
    prepared: HashMap<u64, (tempfile::NamedTempFile, PathBuf)>,
    confirm: std::collections::HashSet<u64>,
}
pub fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Pending<Result<T, String>> {
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    std::thread::spawn(move || {
        let _ = send.send(work());
    });
    Pending::future(std::future::poll_fn(move |_| match receive.try_recv() {
        Ok(v) => Poll::Ready(v),
        Err(std::sync::mpsc::TryRecvError::Empty) => Poll::Pending,
        Err(_) => Poll::Ready(Err("The file operation stopped unexpectedly.".into())),
    }))
}
pub fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    read_bytes_within(path, pdn_core::io::codecs::MAX_DECODE_BYTES)
}
/// Asset imports reuse the bounded reader with their smaller admission budget.
pub fn read_bytes_within(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    read_open(std::fs::File::open(path).map_err(|e| e.to_string())?, limit)
}
/// R156 (BB27): `Ok(None)` for an absent path or a directory, classified by `io::ErrorKind`
/// (also when the file disappears before the authoritative open); `admit` judges an existing
/// file's name before its size or bytes. Other failures remain errors.
pub fn read_existing(
    path: &Path,
    admit: impl Fn(&str) -> Result<(), String>,
) -> Result<Option<Vec<u8>>, String> {
    use std::io::ErrorKind::{NotADirectory, NotFound};
    let absent = |e: &std::io::Error| matches!(e.kind(), NotFound | NotADirectory);
    match std::fs::metadata(path) {
        Err(e) if absent(&e) => return Ok(None),
        Err(e) => return Err(e.to_string()),
        Ok(m) if m.is_dir() => return Ok(None),
        Ok(_) => {}
    }
    admit(&path.to_string_lossy())?;
    let file = match std::fs::File::open(path) {
        Err(e) if absent(&e) => return Ok(None),
        file => file.map_err(|e| e.to_string())?,
    };
    if file.metadata().is_ok_and(|m| m.is_dir()) {
        return Ok(None);
    }
    read_open(file, pdn_core::io::codecs::MAX_DECODE_BYTES).map(Some)
}
fn read_open(file: std::fs::File, limit: u64) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let limit = limit.min(pdn_core::io::codecs::MAX_DECODE_BYTES);
    if file.metadata().map_err(|e| e.to_string())?.len() > limit {
        return Err(super::files::FILE_TOO_LARGE.into());
    }
    let mut bytes = vec![];
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > limit {
        return Err(super::files::FILE_TOO_LARGE.into());
    }
    Ok(bytes)
}
fn read_file(path: PathBuf) -> Result<PickedFile, String> {
    let bytes = read_bytes(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut f = PickedFile::snapshot(
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        bytes,
    );
    f.path = Some(path);
    Ok(f)
}
pub fn read_paths(paths: Vec<PathBuf>) -> Pending<Result<Vec<PickedFile>, String>> {
    background(move || paths.into_iter().map(read_file).collect())
}
/// One combined Images entry in `accept`'s order, then All files when asked (R156, BB27).
pub fn open_filters(accept: &str, all_files: bool) -> Vec<(&'static str, Vec<&str>)> {
    let images = accept
        .split(',')
        .filter(|s| !s.contains('/'))
        .map(|s| s.trim_start_matches('.'))
        .filter(|s| !s.is_empty())
        .collect();
    let mut filters = vec![("Images", images)];
    if all_files {
        // rfd's "any file" pattern: `*.*` on Windows, `*` through the portal.
        filters.push(("All files (*.*)", vec!["*"]));
    }
    filters
}
pub fn choose_open(multiple: bool, accept: &str) -> Pending<Result<Vec<PickedFile>, String>> {
    choose_open_with(multiple, accept, false)
}
pub fn choose_open_with(
    multiple: bool,
    accept: &str,
    all_files: bool,
) -> Pending<Result<Vec<PickedFile>, String>> {
    let mut dialog = rfd::FileDialog::new();
    for (name, extensions) in open_filters(accept, all_files) {
        dialog = dialog.add_filter(name, &extensions);
    }
    let paths = if multiple {
        dialog.pick_files().unwrap_or_default()
    } else {
        dialog.pick_file().into_iter().collect()
    };
    Pending::ready(Ok(paths
        .into_iter()
        .map(|path| {
            let mut f = PickedFile::snapshot(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                vec![],
            );
            f.path = Some(path);
            f
        })
        .collect()))
}
pub fn pick_open(multiple: bool, accept: &str) -> Pending<Result<Vec<PickedFile>, String>> {
    let result = choose_open(multiple, accept)
        .poll()
        .expect("native chooser result");
    match result {
        Ok(files) => read_paths(files.into_iter().filter_map(|f| f.path).collect()),
        Err(e) => Pending::ready(Err(e)),
    }
}

pub fn adopt_open(
    state: &Rc<RefCell<FileState>>,
    id: u64,
    file: &PickedFile,
    format: &'static str,
) {
    if let Some(path) = &file.path {
        state.borrow_mut().current.insert(
            id,
            NativeTarget {
                destination: SaveDestination {
                    name: file.name.clone(),
                    path: Some(path.to_string_lossy().into()),
                    format,
                },
                path: path.clone(),
            },
        );
    }
}
impl SaveTarget for Files {
    fn choose(&mut self, r: &TargetRequest) -> Result<Option<SaveDestination>, String> {
        if !self.system {
            return Ok(None);
        }
        let types = super::files::chooser_formats(r.initial_format);
        let mut dialog = rfd::FileDialog::new().set_file_name(&r.suggested_name);
        for f in &types {
            let extensions: Vec<_> = f
                .extensions
                .iter()
                .map(|e| e.trim_start_matches('.'))
                .collect();
            dialog = dialog.add_filter(f.name, &extensions);
        }
        let Some(mut path) = dialog.save_file() else {
            return Ok(None);
        };
        let appended = path.extension().is_none();
        if appended {
            path.set_extension(
                crate::dialogs::save_image::find_format(r.initial_format).extensions[0]
                    .trim_start_matches('.'),
            );
        }
        let name = path
            .file_name()
            .ok_or("Choose a file name.")?
            .to_string_lossy()
            .into_owned();
        let format = super::files::accepted_format(&name)?;
        if appended && path.exists() {
            self.state.borrow_mut().confirm.insert(r.workspace_id);
        }
        let destination = SaveDestination {
            name,
            path: Some(path.to_string_lossy().into()),
            format,
        };
        self.state.borrow_mut().chosen.insert(
            r.workspace_id,
            NativeTarget {
                destination: destination.clone(),
                path,
            },
        );
        Ok(Some(destination))
    }
    fn choose_pending(
        &mut self,
        r: TargetRequest,
    ) -> Pending<Result<Option<SaveDestination>, String>> {
        let chosen = self.choose(&r);
        let confirm = self.state.borrow_mut().confirm.remove(&r.workspace_id);
        if !confirm {
            return Pending::ready(chosen);
        }
        let Ok(Some(destination)) = chosen else {
            return Pending::ready(chosen);
        };
        let result = Rc::new(RefCell::new(None));
        let callback = result.clone();
        crate::dialogs::task::TaskDialog::show_then(
            crate::dialogs::task::TaskDialogSpec {
                title: "Confirm Save As".into(),
                message: Some(format!(
                    "Replace the existing file \"{}\"?",
                    destination.name
                )),
                choices: vec![
                    crate::dialogs::task::TaskChoice {
                        value: "replace",
                        title: "Replace".into(),
                        ..Default::default()
                    },
                    crate::dialogs::task::TaskChoice {
                        value: "cancel",
                        title: "Cancel".into(),
                        ..Default::default()
                    },
                ],
                cancel_value: "cancel",
                ..Default::default()
            },
            move |choice, _| {
                *callback.borrow_mut() = Some(Ok((choice == "replace").then_some(destination)));
            },
        );
        Pending::future(std::future::poll_fn(move |_| {
            result
                .borrow_mut()
                .take()
                .map_or(Poll::Pending, Poll::Ready)
        }))
    }
    fn prepare_pending(
        &mut self,
        id: u64,
        destination: &SaveDestination,
    ) -> Pending<Result<(), String>> {
        let result = (|| {
            let retained = {
                let state = self.state.borrow();
                state
                    .chosen
                    .get(&id)
                    .or_else(|| state.current.get(&id))
                    .filter(|t| t.destination == *destination)
                    .map(|t| t.path.clone())
            };
            let path = retained
                .or_else(|| destination.path.as_ref().map(PathBuf::from))
                .ok_or("No file destination was selected.")?;
            if path.metadata().is_ok_and(|m| m.permissions().readonly()) {
                return Err("The destination is read-only.".into());
            }
            let path = resolved(path);
            let temp = atomic_temp(&path)?;
            self.state.borrow_mut().prepared.insert(id, (temp, path));
            Ok(())
        })();
        Pending::ready(result)
    }
    fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
        Err("Use the prepared file writer.".into())
    }
    fn write_pending(&mut self, r: SaveRequest) -> Pending<Result<WriteOutcome, String>> {
        let Some((mut temp, path)) = self.state.borrow_mut().prepared.remove(&r.workspace_id)
        else {
            return Pending::ready(Err("Output was not prepared.".into()));
        };
        background(move || {
            temp.write_all(&r.bytes).map_err(|e| e.to_string())?;
            atomic_commit(temp, &path)?;
            Ok(WriteOutcome::Written)
        })
    }
    fn existing(&self, id: u64) -> Option<SaveDestination> {
        self.state
            .borrow()
            .current
            .get(&id)
            .map(|t| t.destination.clone())
    }
    fn abandon(&mut self, id: u64) {
        let mut state = self.state.borrow_mut();
        state.chosen.remove(&id);
        state.prepared.remove(&id);
    }
    fn committed(&mut self, id: u64, d: &SaveDestination) {
        let mut state = self.state.borrow_mut();
        let target = state
            .chosen
            .remove(&id)
            .or_else(|| state.current.get(&id).cloned())
            .filter(|t| t.destination == *d)
            .or_else(|| {
                d.path.as_ref().map(|p| NativeTarget {
                    destination: d.clone(),
                    path: PathBuf::from(p),
                })
            });
        if let Some(target) = target {
            state.current.insert(id, target);
        }
    }
}
pub fn shell_open(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let program = "xdg-open";
    std::process::Command::new(program)
        .arg(path)
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}
thread_local! {static PRINT_FILES:RefCell<Vec<tempfile::NamedTempFile>>=const{RefCell::new(vec![])};}
pub fn print_png(bytes: Vec<u8>) -> Pending<Result<(), String>> {
    // shortcut: no cross-platform print dialog; upgrade to OS print APIs if needed.
    let result = (|| {
        let mut file = tempfile::Builder::new()
            .suffix(".png")
            .tempfile()
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        shell_open(file.path())?;
        PRINT_FILES.with(|files| files.borrow_mut().push(file));
        Ok(())
    })();
    Pending::ready(result)
}
/// desktop main.js commandLineFiles: filter flags/non-files, resolve and deduplicate.
pub fn argv_files(args: impl IntoIterator<Item = std::ffi::OsString>) -> Vec<PathBuf> {
    let mut files = vec![];
    for arg in args {
        if arg.to_string_lossy().starts_with('-') {
            continue;
        }
        let path = PathBuf::from(arg);
        let Ok(absolute) = std::path::absolute(path) else {
            continue;
        };
        let mut path = PathBuf::new();
        for part in absolute.components() {
            match part {
                std::path::Component::ParentDir => {
                    path.pop();
                }
                std::path::Component::CurDir => {}
                part => path.push(part.as_os_str()),
            }
        }
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_ascii_lowercase();
        if path.is_file()
            && matches!(
                ext.as_str(),
                "pdn"
                    | "png"
                    | "jpg"
                    | "jpeg"
                    | "jpe"
                    | "webp"
                    | "gif"
                    | "bmp"
                    | "tif"
                    | "tiff"
                    | "jxl"
                    | "avif"
                    | "heic"
                    | "heif"
                    | "dds"
                    | "tga"
                    | "jxr"
                    | "wdp"
                    | "wmp"
                    | "psd"
                    | "psb"
                    | "json"
            )
            && !files.contains(&path)
        {
            files.push(path);
        }
    }
    files
}

/// BB36: one file through a titled chooser with one filter.
pub fn choose_data(request: super::files::DataFile) -> Pending<Result<Vec<PickedFile>, String>> {
    let picked = rfd::FileDialog::new()
        .set_title(request.title)
        .add_filter(request.filter, &[request.extension])
        .pick_file();
    Pending::ready(Ok(picked
        .into_iter()
        .map(|path| {
            let name = path.file_name().unwrap_or_default().to_string_lossy();
            let mut f = PickedFile::snapshot(name.into(), vec![]);
            f.path = Some(path);
            f
        })
        .collect()))
}

/// BB36: a titled save chooser with one filter, whose extension a bare chosen name gets
/// (WinForms' AddExtension); then `save_bytes`' write.
pub fn save_data(
    request: super::files::DataFile,
    name: &str,
    bytes: Vec<u8>,
) -> Pending<Result<super::files::FileSaveOutcome, String>> {
    let chosen = rfd::FileDialog::new()
        .set_title(request.title)
        .add_filter(request.filter, &[request.extension])
        .set_file_name(name)
        .save_file();
    let Some(mut path) = chosen else {
        return Pending::ready(Ok(super::files::FileSaveOutcome::Cancelled));
    };
    if path.extension().is_none() {
        path.set_extension(request.extension);
    }
    write_bytes(path, bytes)
}

pub fn save_bytes(
    name: &str,
    bytes: Vec<u8>,
    existing: Option<super::files::FileTarget>,
) -> Pending<Result<super::files::FileSaveOutcome, String>> {
    let path = existing
        .and_then(|t| t.path)
        .or_else(|| rfd::FileDialog::new().set_file_name(name).save_file());
    let Some(path) = path else {
        return Pending::ready(Ok(super::files::FileSaveOutcome::Cancelled));
    };
    write_bytes(path, bytes)
}

/// Final review I1: the atomic save every file writer shares. A temporary file beside `target`,
/// created 0o666 so the umask governs a new file's mode as it does any other program's.
/// Rename-based atomicity replaces the inode, so a hard link to an overwritten file keeps the
/// old content; atomicity wins over preserving hard links.
pub(crate) fn atomic_temp(target: &Path) -> Result<tempfile::NamedTempFile, String> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut builder = tempfile::Builder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(std::fs::Permissions::from_mode(0o666));
    }
    builder.tempfile_in(parent).map_err(|e| e.to_string())
}

/// Syncs `temp` and renames it over `target`, which keeps the mode it had (Unix; Windows keeps
/// its previous behaviour).
pub(crate) fn atomic_commit(temp: tempfile::NamedTempFile, target: &Path) -> Result<(), String> {
    #[cfg(unix)]
    if let Ok(old) = std::fs::metadata(target) {
        temp.as_file()
            .set_permissions(old.permissions())
            .map_err(|e| e.to_string())?;
    }
    temp.as_file().sync_all().map_err(|e| e.to_string())?;
    temp.persist(target).map_err(|e| e.to_string())?;
    Ok(())
}

/// A symlinked save destination is the file it points at, so the save replaces that file and
/// the link keeps pointing at the saved content.
// ponytail: a dangling link does not resolve and is replaced by the saved file.
fn resolved(path: PathBuf) -> PathBuf {
    match path.is_symlink().then(|| std::fs::canonicalize(&path)) {
        Some(Ok(target)) => target,
        _ => path,
    }
}

/// `atomic_temp` beside `path`'s resolved file, then `atomic_commit` in the background.
fn write_bytes(
    path: PathBuf,
    bytes: Vec<u8>,
) -> Pending<Result<super::files::FileSaveOutcome, String>> {
    use super::files::{FileSaveOutcome, FileTarget};
    let target = resolved(path.clone());
    let temp = match atomic_temp(&target) {
        Ok(t) => t,
        Err(e) => return Pending::ready(Err(e)),
    };
    background(move || {
        let mut temp = temp;
        temp.write_all(&bytes).map_err(|e| e.to_string())?;
        atomic_commit(temp, &target)?;
        Ok(FileSaveOutcome::Written(FileTarget {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            path: Some(path),
        }))
    })
}

#[cfg(all(test, unix))]
mod argv_tests {
    #[test]
    fn argv_resolves_without_replacing_symlink_identity() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("a.PNG");
        let alias = dir.path().join("alias.png");
        std::fs::write(&original, b"x").unwrap();
        std::os::unix::fs::symlink(&original, &alias).unwrap();
        let files = super::argv_files([
            original.clone().into_os_string(),
            alias.clone().into_os_string(),
            original.clone().into_os_string(),
            "--ignored".into(),
        ]);
        assert_eq!(files, vec![original, alias]);
    }
}

#[cfg(test)]
mod bb27_tests {
    use pdn_core::effect::bb::fill::admit_file;
    use pdn_core::io::codecs::MAX_DECODE_BYTES;
    /// R156: absent paths and directories are typed `None`; an existing file's extension is
    /// admitted before its size or bytes; other failures remain errors.
    #[test]
    fn read_existing_classifies_before_reading() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path();
        std::fs::write(p.join("a.png"), [1, 2, 3]).unwrap();
        for absent in [
            p.join("missing.png"),
            p.to_path_buf(),
            p.join("a.png/child.png"),
        ] {
            assert_eq!(
                super::read_existing(&absent, admit_file),
                Ok(None),
                "{absent:?}"
            );
        }
        assert_eq!(
            super::read_existing(&p.join("a.png"), admit_file),
            Ok(Some(vec![1, 2, 3]))
        );
        for (name, error) in [
            ("big.txt", ".txt is not supported."),
            ("big.png", "File is too large to open."),
        ] {
            let f = std::fs::File::create(p.join(name)).unwrap();
            f.set_len(MAX_DECODE_BYTES + 1).unwrap();
            assert_eq!(
                super::read_existing(&p.join(name), admit_file),
                Err(error.into())
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let locked = p.join("locked.png");
            std::fs::write(&locked, [1]).unwrap();
            std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
            // Root can still open it; anyone else gets the OS error, never a silent `None`.
            if std::fs::File::open(&locked).is_err() {
                assert!(
                    matches!(super::read_existing(&locked, admit_file), Err(e) if !e.is_empty())
                );
            }
        }
    }
    /// R156: the BB27 chooser keeps the ordered combined entry and adds All files; old callers
    /// keep their single Images entry.
    #[test]
    fn open_filters_add_all_files_only_when_asked() {
        assert_eq!(
            super::open_filters(".png,.jpg,.gif,.bmp", true),
            [
                ("Images", vec!["png", "jpg", "gif", "bmp"]),
                ("All files (*.*)", vec!["*"])
            ]
        );
        assert_eq!(
            super::open_filters(".pdn,image/png,.png", false),
            [("Images", vec!["pdn", "png"])]
        );
    }
}

/// Final review I1: both atomic writers (documents, and `write_bytes` for palettes and chart
/// data) give a new file the mode any new file gets, keep an overwritten file's mode, and save
/// through a symlink to its target.
#[cfg(all(test, unix))]
mod save_mode_tests {
    use crate::dialogs::flows::{SaveDestination, SaveRequest, SaveTarget};
    use std::os::unix::fs::{PermissionsExt, symlink};
    use std::path::Path;

    fn wait<T: 'static>(mut p: super::Pending<T>) -> T {
        loop {
            if let Some(v) = p.poll() {
                return v;
            }
            std::thread::yield_now();
        }
    }
    fn mode(p: &Path) -> u32 {
        std::fs::metadata(p).unwrap().permissions().mode() & 0o7777
    }
    fn document(path: &Path, bytes: &[u8]) {
        let mut files = super::Files::system();
        let destination = SaveDestination {
            name: "a.png".into(),
            path: Some(path.to_string_lossy().into()),
            format: "png",
        };
        wait(files.prepare_pending(1, &destination)).unwrap();
        let request = SaveRequest {
            workspace_id: 1,
            destination,
            bytes: bytes.to_vec(),
        };
        wait(files.write_pending(request)).unwrap();
    }
    fn data(path: &Path, bytes: &[u8]) {
        wait(super::write_bytes(path.to_path_buf(), bytes.to_vec())).unwrap();
    }

    #[test]
    fn saves_keep_modes_and_symlinks() {
        for save in [document as fn(&Path, &[u8]), data] {
            let dir = tempfile::tempdir().unwrap();
            let reference = dir.path().join("reference");
            std::fs::File::create(&reference).unwrap();
            let file = dir.path().join("a.png");
            save(&file, b"new");
            assert_eq!(mode(&file), mode(&reference), "new file: the umask governs");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
            save(&file, b"over");
            assert_eq!(mode(&file), 0o644, "overwrite keeps the mode");
            assert_eq!(std::fs::read(&file).unwrap(), b"over");
            let link = dir.path().join("link.png");
            symlink(&file, &link).unwrap();
            save(&link, b"linked");
            assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
            assert_eq!(std::fs::read(&file).unwrap(), b"linked");
            assert_eq!(mode(&file), 0o644);
        }
    }
}
