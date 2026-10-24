//! User-owned Unix IPC directory and socket. No endpoint cleanup without the held primary lock.
use interprocess::{
    local_socket::{GenericFilePath, ListenerOptions, Name, ToFsName},
    os::unix::local_socket::ListenerOptionsExt,
};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
fn uid() -> u32 {
    // SAFETY: geteuid has no arguments or memory contract.
    unsafe { libc::geteuid() }
}
fn validate(dir: &Path) -> io::Result<()> {
    let m = fs::symlink_metadata(dir)?;
    if !m.is_dir() || m.uid() != uid() || m.mode() & 0o777 != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe paint.rs IPC directory",
        ));
    }
    Ok(())
}
pub fn private_directory() -> io::Result<PathBuf> {
    let root = if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        let runtime = PathBuf::from(runtime);
        validate(&runtime)?;
        runtime
    } else {
        // macOS /tmp is a symlink; use its documented real location and validate it directly.
        let root = PathBuf::from(if cfg!(target_os = "macos") {
            "/private/tmp"
        } else {
            "/tmp"
        });
        let base = fs::symlink_metadata(&root)?;
        if !base.is_dir() || (base.uid() != 0 && base.uid() != uid()) || base.mode() & 0o1000 == 0 {
            return Err(io::Error::other("unsafe IPC parent"));
        }
        root
    };
    let dir = root.join(format!("paint.rs-{}", uid()));
    match fs::DirBuilder::new().mode(0o700).create(&dir) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e),
    }
    validate(&dir)?;
    Ok(dir)
}
pub fn lock_file(dir: &Path) -> io::Result<File> {
    validate(dir)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(dir.join("owner"))?;
    let m = file.metadata()?;
    if !m.is_file() || m.uid() != uid() || m.mode() & 0o777 != 0o600 || m.nlink() != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe IPC lock",
        ));
    }
    Ok(file)
}
pub fn endpoint(dir: &Path) -> io::Result<Name<'static>> {
    dir.join("launch.sock").to_fs_name::<GenericFilePath>()
}
pub fn remove_stale(dir: &Path) -> io::Result<()> {
    let file = dir.join("launch.sock");
    match fs::symlink_metadata(&file) {
        Ok(m) if m.file_type().is_socket() && m.uid() == uid() => fs::remove_file(file),
        Ok(_) => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unsafe stale IPC endpoint",
        )),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}
pub fn secure_listener(options: ListenerOptions<'_>) -> io::Result<ListenerOptions<'_>> {
    Ok(options.mode(0o600))
}
