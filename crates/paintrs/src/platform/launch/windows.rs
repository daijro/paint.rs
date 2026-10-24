//! User-SID-only named pipe and lock directory. interprocess rejects remote pipe clients and
//! reserves the first pipe instance. Native identity/security APIs are from the cached SDK crate.
use interprocess::{
    local_socket::{GenericNamespaced, ListenerOptions, Name, ToNsName},
    os::windows::{
        local_socket::ListenerOptionsExt,
        named_pipe::{DuplexPipeStream, pipe_mode},
        security_descriptor::{AsSecurityDescriptor, SecurityDescriptor},
    },
};
use std::{
    fs::{self, File, OpenOptions},
    io,
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    time::Instant,
};
use widestring::U16CString;
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, LocalFree},
    Security::{Authorization::*, *},
    Storage::FileSystem::*,
    System::Threading::{
        GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};
fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}
fn sid_string(sid: PSID) -> io::Result<String> {
    let mut out = std::ptr::null_mut();
    // SAFETY: callers supply an OS-owned valid SID; conversion allocates a terminated local string.
    unsafe {
        if ConvertSidToStringSidW(sid, &mut out) == 0 {
            return Err(io::Error::last_os_error());
        }
        let s = U16CString::from_ptr_str(out).to_string_lossy();
        LocalFree(out.cast());
        Ok(s)
    }
}
fn user_sid() -> io::Result<String> {
    // SAFETY: GetCurrentProcess returns the valid pseudo-handle for this process.
    process_sid(unsafe { GetCurrentProcess() })
}
fn process_sid(process: HANDLE) -> io::Result<String> {
    // SAFETY: token data is held in an aligned buffer until the SID is copied; every handle is closed.
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
            return Err(io::Error::last_os_error());
        }
        let result = (|| {
            let mut len = 0;
            GetTokenInformation(token, TokenUser, std::ptr::null_mut(), 0, &mut len);
            if len == 0 {
                return Err(io::Error::last_os_error());
            }
            let mut data = vec![0usize; (len as usize).div_ceil(std::mem::size_of::<usize>())];
            if GetTokenInformation(token, TokenUser, data.as_mut_ptr().cast(), len, &mut len) == 0 {
                return Err(io::Error::last_os_error());
            }
            sid_string((*(data.as_ptr().cast::<TOKEN_USER>())).User.Sid)
        })();
        CloseHandle(token);
        result
    }
}
fn descriptor(directory: bool) -> io::Result<SecurityDescriptor> {
    let sid = user_sid()?;
    let flags = if directory { "OICI" } else { "" };
    let text = U16CString::from_str(format!("O:{sid}D:P(A;{flags};FA;;;{sid})"))
        .map_err(io::Error::other)?;
    SecurityDescriptor::deserialize(&text)
}
fn validate(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "IPC path is a reparse point",
        ));
    }
    let sid = user_sid()?;
    let mut owner = std::ptr::null_mut();
    let mut acl: *mut ACL = std::ptr::null_mut();
    let mut sd = std::ptr::null_mut();
    // SAFETY: SDK fills owner/ACL pointers into the allocated descriptor; inspect before LocalFree.
    unsafe {
        let code = GetNamedSecurityInfoW(
            wide(path).as_ptr(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            std::ptr::null_mut(),
            &mut acl,
            std::ptr::null_mut(),
            &mut sd,
        );
        if code != 0 {
            return Err(io::Error::from_raw_os_error(code as i32));
        }
        let result = (|| {
            if owner.is_null() || acl.is_null() || sid_string(owner)? != sid || (*acl).AceCount != 1
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "IPC ownership/ACL is not private",
                ));
            }
            let mut raw = std::ptr::null_mut();
            if GetAce(acl, 0, &mut raw) == 0 {
                return Err(io::Error::last_os_error());
            }
            let ace = &*raw.cast::<ACCESS_ALLOWED_ACE>();
            // ACCESS_ALLOWED_ACE_TYPE is 0 in the public WinNT ACE_HEADER ABI.
            if ace.Header.AceType != 0
                || ace.Mask != FILE_ALL_ACCESS
                || sid_string((&ace.SidStart as *const u32).cast_mut().cast())? != sid
            {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "IPC ACL grants another identity",
                ));
            }
            Ok(())
        })();
        LocalFree(sd.cast());
        result
    }
}
pub fn private_directory() -> io::Result<PathBuf> {
    let base = std::env::var_os("LOCALAPPDATA")
        .ok_or_else(|| io::Error::other("LOCALAPPDATA unavailable"))?;
    let dir = fs::canonicalize(base)?.join("paint.rs-launch");
    let sd = descriptor(true)?;
    let sa = SECURITY_ATTRIBUTES {
        nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_sd().cast_mut(),
        bInheritHandle: 0,
    };
    // SAFETY: directory path is terminated; security descriptor outlives synchronous creation.
    if unsafe { CreateDirectoryW(wide(&dir).as_ptr(), &sa) } == 0 {
        let e = io::Error::last_os_error();
        if e.kind() != io::ErrorKind::AlreadyExists {
            return Err(e);
        }
    }
    validate(&dir)?;
    Ok(dir)
}
pub fn lock_file(dir: &Path) -> io::Result<File> {
    validate(dir)?;
    let path = dir.join("owner");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(&path)?;
    validate(&path)?;
    Ok(file)
}
fn pipe_name() -> io::Result<String> {
    Ok(format!("paint.rs-{}", user_sid()?))
}
/// The generic local-socket connector in cached interprocess 2.4.5 discards wait_mode
/// on Windows. Its direct named-pipe connector preserves the remaining deadline.
pub fn connect(until: Instant) -> io::Result<DuplexPipeStream<pipe_mode::Bytes>> {
    let path = PathBuf::from(format!(r"\\.\pipe\{}", pipe_name()?));
    connect_path(&path, &user_sid()?, until)
}
fn connect_path(
    path: &Path,
    expected_sid: &str,
    until: Instant,
) -> io::Result<DuplexPipeStream<pipe_mode::Bytes>> {
    let stream = DuplexPipeStream::connect_by_path_with_wait_mode(
        path,
        interprocess::ConnectWaitMode::Timeout(super::remaining(until)?),
    )?;
    stream.set_nonblocking(true)?;
    // A predictable SID-derived name and listener ACL do not authenticate a pre-existing
    // server. Fail closed before writing the frame length, paths, or trusting its ACK.
    authenticate_server(&stream, expected_sid).map_err(|e| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!("launch server authentication failed: {e}"),
        )
    })?;
    super::remaining(until)?;
    Ok(stream)
}
fn authenticate_server(
    stream: &DuplexPipeStream<pipe_mode::Bytes>,
    expected_sid: &str,
) -> io::Result<()> {
    let pid = stream.peer_process_id()?;
    // SAFETY: query-only process handle; its token SID is copied before both handles close.
    let actual_sid = unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return Err(io::Error::last_os_error());
        }
        let result = process_sid(process);
        CloseHandle(process);
        result?
    };
    if actual_sid != expected_sid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "launch server belongs to another user",
        ));
    }
    Ok(())
}
pub fn endpoint(_dir: &Path) -> io::Result<Name<'static>> {
    pipe_name()?.to_ns_name::<GenericNamespaced>()
}
pub fn remove_stale(_dir: &Path) -> io::Result<()> {
    Ok(())
}
pub fn secure_listener(options: ListenerOptions<'_>) -> io::Result<ListenerOptions<'_>> {
    Ok(options.security_descriptor(descriptor(false)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use interprocess::os::windows::named_pipe::{PipeListenerOptions, pipe_mode};
    use std::{
        io::Read,
        time::{Duration, Instant},
    };

    fn test_path() -> PathBuf {
        PathBuf::from(format!(r"\\.\pipe\paintrs-test-{}", uuid::Uuid::new_v4()))
    }
    #[test]
    fn occupied_pipe_obeys_remaining_connection_deadline() {
        let path = test_path();
        let _listener = PipeListenerOptions::new()
            .path(path.as_path())
            .instance_limit(std::num::NonZeroU8::new(1))
            .create_duplex::<pipe_mode::Bytes>()
            .unwrap();
        // Do not accept: the first connection occupies the sole pipe instance.
        let _first = connect_path(
            &path,
            &user_sid().unwrap(),
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
        let began = Instant::now();
        let result = connect_path(
            &path,
            &user_sid().unwrap(),
            began + Duration::from_millis(80),
        );
        assert!(result.is_err());
        assert!(began.elapsed() < Duration::from_millis(500));
    }
    #[test]
    fn server_identity_mismatch_is_rejected_without_sending_paths() {
        let path = test_path();
        let listener = PipeListenerOptions::new()
            .path(path.as_path())
            .nonblocking(true)
            .create_duplex::<pipe_mode::Bytes>()
            .unwrap();
        // The real connected server belongs to this test process. Supply another expected
        // SID to exercise the same authentication rejection without provisioning an account.
        let result = connect_path(&path, "S-1-0-0", Instant::now() + Duration::from_secs(1));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        // A disconnected empty client may be consumed by accept itself; any connection
        // returned must contain no application bytes.
        match listener.accept() {
            Ok(mut stream) => {
                let mut byte = [0];
                assert!(!matches!(stream.read(&mut byte), Ok(1)));
            }
            Err(e) => assert!(matches!(
                e.kind(),
                io::ErrorKind::WouldBlock | io::ErrorKind::BrokenPipe
            )),
        }
    }
}
