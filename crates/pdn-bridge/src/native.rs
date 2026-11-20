//! CoreCLR hosting through hostfxr, and the safe wrapper over the managed C ABI.
//!
//! hostfxr strings use `char_t`: `char` (UTF-8) on Unix, `wchar_t` (UTF-16) on Windows.
//! Our own ABI strings (plugin paths, effect ids, error messages) are always UTF-8
//! `c_char`, independent of `char_t`.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::AtomicBool;

use crate::{Error, Result};

#[cfg(windows)]
type CharT = u16;
#[cfg(not(windows))]
type CharT = c_char;

// --- hostfxr + the managed exports (BGRA8 at this boundary) ---

type HostfxrHandle = *mut c_void;
type InitFn = unsafe extern "C" fn(*const CharT, *const c_void, *mut HostfxrHandle) -> c_int;
type InitCmdFn = unsafe extern "C" fn(
    c_int,
    *const *const CharT,
    *const InitParams,
    *mut HostfxrHandle,
) -> c_int;
#[repr(C)]
struct InitParams {
    size: usize,
    host_path: *const CharT,
    dotnet_root: *const CharT,
}
type GetDelegateFn = unsafe extern "C" fn(HostfxrHandle, c_int, *mut *mut c_void) -> c_int;
type GetPropFn = unsafe extern "C" fn(HostfxrHandle, *const CharT, *mut *const CharT) -> c_int;
type SetPropFn = unsafe extern "C" fn(HostfxrHandle, *const CharT, *const CharT) -> c_int;
type LoadAsmFn = unsafe extern "C" fn(
    *const CharT,
    *const CharT,
    *const CharT,
    *const CharT,
    *mut c_void,
    *mut *mut c_void,
) -> c_int;

type LoadPluginFn = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> *mut c_void;
type EnumerateFn = unsafe extern "C" fn(*mut c_void, *mut *mut c_char) -> *mut c_char;
type SchemaFn = unsafe extern "C" fn(*mut c_void, *const c_char, *mut *mut c_char) -> *mut c_char;
type RenderFn = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *mut u8,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    c_int,
    *mut c_int,
    *mut *mut c_char,
) -> c_int;
type SaveFn = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    *const c_char,
    *const u8,
    c_int,
    c_int,
    *mut c_int,
    *mut *mut c_char,
) -> *mut u8;
type OpenFn = unsafe extern "C" fn(
    *mut c_void,
    c_int,
    *const u8,
    c_int,
    *const c_char,
    *mut *mut u8,
    *mut c_int,
    *mut c_int,
    *mut *mut c_char,
) -> c_int;
type PressFn = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const c_char,
    *const u8,
    c_int,
    c_int,
    *mut *mut c_char,
) -> *mut c_char;
type FreePluginFn = unsafe extern "C" fn(*mut c_void);
type SetClipboardFn = unsafe extern "C" fn(*const u8, c_int, c_int, *mut *mut c_char) -> c_int;
type SetPermissionsFn = unsafe extern "C" fn(*const c_char, *mut *mut c_char) -> c_int;
type StoreResponseFn =
    unsafe extern "C" fn(*const c_char, *const c_char, *mut *mut c_char) -> c_int;
type FreeFn = unsafe extern "C" fn(*mut c_void);
/// paint.rs's text for plug-in strings (`Host::set_drawing_host`): UTF-16 text and its length, a UTF-8 family, an
/// em in pixels, GDI+'s style bits; returns a float buffer and its length (`pdn_core::effect::dotnet::text_outline`).
pub type TextOutlineFn =
    unsafe extern "C" fn(*const u16, c_int, *const c_char, f32, c_int, *mut c_int) -> *mut f32;
/// Releases a `TextOutlineFn` buffer, given its length.
pub type FreeFloatsFn = unsafe extern "C" fn(*mut f32, c_int);
/// paint.rs's image codecs for plug-in images: an encoded file and its length; returns a byte buffer and its length
/// (`pdn_core::effect::dotnet::decode_image`), or null.
pub type DecodeImageFn = unsafe extern "C" fn(*const u8, c_int, *mut c_int) -> *mut u8;
/// Releases a `DecodeImageFn` buffer, given its length.
pub type FreeBytesFn = unsafe extern "C" fn(*mut u8, c_int);
type SetDrawingHostFn =
    unsafe extern "C" fn(TextOutlineFn, FreeFloatsFn, DecodeImageFn, FreeBytesFn);
type CanvasOpenFn = unsafe extern "C" fn(
    *mut c_void,
    *const c_char,
    *const c_char,
    *const u8,
    c_int,
    c_int,
    *mut *mut c_char,
) -> c_int;
type CanvasInputFn = unsafe extern "C" fn(
    c_int,
    *const c_char,
    *const c_char,
    *const c_char,
    *mut c_int,
    *mut *mut c_char,
) -> *mut u8;
type CanvasPaintFn =
    unsafe extern "C" fn(c_int, *const c_char, *mut c_int, *mut *mut c_char) -> *mut u8;
type CanvasTokenFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> *mut c_char;
type CanvasCloseFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int;
type CanvasKeepFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> c_int;
type CanvasLayoutFn = unsafe extern "C" fn(c_int, *const c_char, *mut *mut c_char) -> *mut c_char;
type CanvasPressFn =
    unsafe extern "C" fn(c_int, *const c_char, *const c_char, *mut *mut c_char) -> *mut c_char;
type CanvasValuesFn = unsafe extern "C" fn(c_int, *mut *mut c_char) -> *mut c_char;
type CanvasProvideFileFn =
    unsafe extern "C" fn(c_int, *const c_char, *const u8, c_int, *mut *mut c_char) -> c_int;

/// The live-canvas exports (`Plugin::canvas_open`), when the host has them.
#[derive(Clone, Copy)]
struct CanvasExports {
    open: CanvasOpenFn,
    input: CanvasInputFn,
    paint: CanvasPaintFn,
    token: CanvasTokenFn,
    keep: CanvasKeepFn,
    close: CanvasCloseFn,
    /// Newer hosts only.
    layout: Option<CanvasLayoutFn>,
    press: Option<CanvasPressFn>,
    values: Option<CanvasValuesFn>,
    provide_file: Option<CanvasProvideFileFn>,
}

const HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER: c_int = 5;
// UNMANAGEDCALLERSONLY_METHOD = (const char_t*)-1 — the marker for an [UnmanagedCallersOnly] export.
const UNMANAGED_CALLERS_ONLY: *const CharT = usize::MAX as *const CharT;
const BRIDGE_TYPE: &str = "PaintDotNet.Bridge.Exports, PdnBridge";

/// CoreCLR turns a managed null dereference (routine in third-party plugin code) into a
/// NullReferenceException from its SIGSEGV handler, which runs on the thread's alternate signal
/// stack. Rust gives each of its threads an 8 KiB one (for the stack-overflow message), which that
/// handler overflows, killing the process. So before a thread first calls into .NET, give it a
/// 64 KiB one (CoreCLR gives its own threads 24 KiB). Put back at thread exit if still installed.
#[cfg(unix)]
fn signal_stack() {
    struct AltStack {
        mem: Vec<u8>,
        old: libc::stack_t,
    }
    impl Drop for AltStack {
        fn drop(&mut self) {
            unsafe {
                let mut cur: libc::stack_t = std::mem::zeroed();
                libc::sigaltstack(ptr::null(), &mut cur);
                if cur.ss_sp == self.mem.as_mut_ptr().cast() {
                    libc::sigaltstack(&self.old, ptr::null_mut());
                }
            }
        }
    }
    const SIZE: usize = 64 * 1024;
    thread_local!(static ALT: std::cell::OnceCell<Option<AltStack>> = const { std::cell::OnceCell::new() });
    ALT.with(|alt| {
        alt.get_or_init(|| unsafe {
            let mut old: libc::stack_t = std::mem::zeroed();
            libc::sigaltstack(ptr::null(), &mut old);
            if old.ss_flags & libc::SS_DISABLE == 0 && old.ss_size >= SIZE {
                return None;
            }
            let mut mem = vec![0u8; SIZE];
            let new = libc::stack_t {
                ss_sp: mem.as_mut_ptr().cast(),
                ss_flags: 0,
                ss_size: SIZE,
            };
            (libc::sigaltstack(&new, ptr::null_mut()) == 0).then_some(AltStack { mem, old })
        });
    });
}

#[cfg(not(unix))]
fn signal_stack() {}

#[derive(Clone, Copy)]
struct Exports {
    load_plugin: LoadPluginFn,
    enumerate: EnumerateFn,
    schema: SchemaFn,
    render: RenderFn,
    press: PressFn,
    save: SaveFn,
    open: OpenFn,
    free_plugin: FreePluginFn,
    free: FreeFn,
    set_clipboard: SetClipboardFn,
    set_drawing_host: SetDrawingHostFn,
    set_permissions: SetPermissionsFn,
    store_response: StoreResponseFn,

    canvas: Option<CanvasExports>,
}

/// The embedded CoreCLR + the managed `PdnBridge` exports. Lives for the process; the
/// hostfxr library and runtime context are intentionally leaked (standard for an embedded
/// runtime — tearing CoreCLR down mid-process is not worth the risk).
pub struct Host {
    exports: Exports,
}
// The exports are plain function pointers into CoreCLR, which is itself multi-threaded;
// each render instantiates a fresh effect, so concurrent calls are safe.
// ponytail: relies on CoreCLR thread-safety + per-call effect instantiation; add a global
// lock here if a plugin ever proves thread-hostile.
unsafe impl Send for Host {}
unsafe impl Sync for Host {}

impl Host {
    /// Start CoreCLR and resolve the managed exports. `bridge_dir` holds `PdnBridge.dll`,
    /// its `.runtimeconfig.json`, and the replacement `PaintDotNet.*.dll`.
    pub fn start(bridge_dir: &Path) -> Result<Host> {
        signal_stack();
        let hostfxr = find_hostfxr(bridge_dir)?;
        // Leak the library so the .so stays mapped and the raw fn pointers stay valid.
        let lib = unsafe { libloading::Library::new(&hostfxr) }
            .map_err(|e| Error(format!("loading {}: {e}", hostfxr.display())))?;

        unsafe {
            let get_delegate: libloading::Symbol<GetDelegateFn> =
                sym(&lib, b"hostfxr_get_runtime_delegate")?;
            let asm = chart_path(&bridge_dir.join("PdnBridge.dll"))?;
            let mut ctx: HostfxrHandle = ptr::null_mut();
            let rc = if hostfxr.parent() == Some(bridge_dir) {
                // Self-contained (the release bundle): hostfxr refuses runtimeconfig init for it, so start
                // it as its app would run, with the bundle as both the app host's folder and the dotnet root.
                let init: libloading::Symbol<InitCmdFn> =
                    sym(&lib, b"hostfxr_initialize_for_dotnet_command_line")?;
                let host = chart_path(&bridge_dir.join(if cfg!(windows) {
                    "PdnBridge.exe"
                } else {
                    "PdnBridge"
                }))?;
                let root = chart_path(bridge_dir)?;
                let params = InitParams {
                    size: size_of::<InitParams>(),
                    host_path: host.as_ptr(),
                    dotnet_root: root.as_ptr(),
                };
                init(1, [asm.as_ptr()].as_ptr(), &params, &mut ctx)
            } else {
                let init: libloading::Symbol<InitFn> =
                    sym(&lib, b"hostfxr_initialize_for_runtime_config")?;
                let config = chart_path(&bridge_dir.join("PdnBridge.runtimeconfig.json"))?;
                init(config.as_ptr(), ptr::null(), &mut ctx)
            };
            if rc < 0 || ctx.is_null() {
                return Err(Error(format!("hostfxr initialization failed: 0x{rc:08x}")));
            }
            prefer_bridge_copies(&lib, ctx, bridge_dir)?;

            let mut lafp: *mut c_void = ptr::null_mut();
            let rc = get_delegate(ctx, HDT_LOAD_ASSEMBLY_AND_GET_FUNCTION_POINTER, &mut lafp);
            if rc != 0 || lafp.is_null() {
                return Err(Error(format!(
                    "hostfxr_get_runtime_delegate failed: 0x{rc:08x}"
                )));
            }
            let load_asm: LoadAsmFn = std::mem::transmute(lafp);

            let ty = chart_str(BRIDGE_TYPE);
            let get = |name: &str| -> Result<*mut c_void> {
                let m = chart_str(name);
                let mut f: *mut c_void = ptr::null_mut();
                let rc = load_asm(
                    asm.as_ptr(),
                    ty.as_ptr(),
                    m.as_ptr(),
                    UNMANAGED_CALLERS_ONLY,
                    ptr::null_mut(),
                    &mut f,
                );
                if rc != 0 || f.is_null() {
                    return Err(Error(format!("resolving {name} failed: 0x{rc:08x}")));
                }
                Ok(f)
            };

            let exports = Exports {
                load_plugin: std::mem::transmute::<*mut c_void, LoadPluginFn>(get(
                    "PdnLoadPlugin",
                )?),
                enumerate: std::mem::transmute::<*mut c_void, EnumerateFn>(get("PdnEnumerate")?),
                schema: std::mem::transmute::<*mut c_void, SchemaFn>(get("PdnEffectSchema")?),
                render: std::mem::transmute::<*mut c_void, RenderFn>(get("PdnEffectRender")?),
                press: std::mem::transmute::<*mut c_void, PressFn>(get("PdnDialogPress")?),
                save: std::mem::transmute::<*mut c_void, SaveFn>(get("PdnFileTypeSave")?),
                open: std::mem::transmute::<*mut c_void, OpenFn>(get("PdnFileTypeOpen")?),
                free_plugin: std::mem::transmute::<*mut c_void, FreePluginFn>(get(
                    "PdnFreePlugin",
                )?),
                free: std::mem::transmute::<*mut c_void, FreeFn>(get("PdnFree")?),
                set_clipboard: std::mem::transmute::<*mut c_void, SetClipboardFn>(get(
                    "PdnSetClipboard",
                )?),
                set_drawing_host: std::mem::transmute::<*mut c_void, SetDrawingHostFn>(get(
                    "PdnSetDrawingHost",
                )?),
                set_permissions: std::mem::transmute::<*mut c_void, SetPermissionsFn>(get(
                    "PdnSetPermissions",
                )?),
                store_response: std::mem::transmute::<*mut c_void, StoreResponseFn>(get(
                    "PdnStoreNetworkResponse",
                )?),

                // Optional: a host without them still runs every other plug-in.
                canvas: (|| {
                    Ok::<_, Error>(CanvasExports {
                        open: std::mem::transmute::<*mut c_void, CanvasOpenFn>(get(
                            "PdnCanvasOpen",
                        )?),
                        input: std::mem::transmute::<*mut c_void, CanvasInputFn>(get(
                            "PdnCanvasInput",
                        )?),
                        paint: std::mem::transmute::<*mut c_void, CanvasPaintFn>(get(
                            "PdnCanvasPaint",
                        )?),
                        token: std::mem::transmute::<*mut c_void, CanvasTokenFn>(get(
                            "PdnCanvasToken",
                        )?),
                        keep: std::mem::transmute::<*mut c_void, CanvasKeepFn>(get(
                            "PdnCanvasKeep",
                        )?),
                        close: std::mem::transmute::<*mut c_void, CanvasCloseFn>(get(
                            "PdnCanvasClose",
                        )?),
                        layout: get("PdnCanvasLayout")
                            .ok()
                            .map(|f| std::mem::transmute::<*mut c_void, CanvasLayoutFn>(f)),
                        press: get("PdnCanvasPress")
                            .ok()
                            .map(|f| std::mem::transmute::<*mut c_void, CanvasPressFn>(f)),
                        values: get("PdnCanvasValues")
                            .ok()
                            .map(|f| std::mem::transmute::<*mut c_void, CanvasValuesFn>(f)),
                        provide_file: get("PdnCanvasProvideFile")
                            .ok()
                            .map(|f| std::mem::transmute::<*mut c_void, CanvasProvideFileFn>(f)),
                    })
                })()
                .ok(),
            };
            std::mem::forget(lib);
            Ok(Host { exports })
        }
    }

    /// Load a plugin DLL into its own load context.
    pub fn load_plugin(&self, dll: &Path) -> Result<Plugin> {
        signal_stack();
        let path = cstr(dll)?;
        let mut err: *mut c_char = ptr::null_mut();
        let handle = unsafe { (self.exports.load_plugin)(path.as_ptr(), &mut err) };
        self.check_err(err)?;
        if handle.is_null() {
            return Err(Error("plugin load returned no handle".into()));
        }
        Ok(Plugin {
            handle,
            exports: self.exports,
        })
    }

    /// Give the managed side paint.rs's text and image codecs: a System.Drawing without GDI+ (the browser host's,
    /// and its desktop stand-in) outlines plug-in strings with `outline` and reads the images it cannot with `decode`.
    pub fn set_drawing_host(
        &self,
        outline: TextOutlineFn,
        free_floats: FreeFloatsFn,
        decode: DecodeImageFn,
        free_bytes: FreeBytesFn,
    ) {
        signal_stack();
        unsafe { (self.exports.set_drawing_host)(outline, free_floats, decode, free_bytes) };
    }

    /// The outside requests plug-ins may make, as the gate's JSON (`{"<plug-in>":[{"category","target","deny"?}]}`),
    /// replacing the last set: a gated call with no entry fails with `pdn-permission:{…}`.
    pub fn set_permissions(&self, json: &str) -> Result<()> {
        signal_stack();
        let json = CString::new(json).map_err(|_| Error("the permissions have a NUL".into()))?;
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { (self.exports.set_permissions)(json.as_ptr(), &mut err) };
        self.check_err(err)
    }

    /// The response paint.rs got for a network request the host left to it (`key`, from its pdn-permission), as
    /// `{status, headers, body:<base64>}`: the plug-in's next run of the same request gets it.
    pub fn store_network_response(&self, key: &str, json: &str) -> Result<()> {
        signal_stack();
        let key = CString::new(key).map_err(|_| Error("the request key has a NUL".into()))?;
        let json = CString::new(json).map_err(|_| Error("the response has a NUL".into()))?;
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { (self.exports.store_response)(key.as_ptr(), json.as_ptr(), &mut err) };
        self.check_err(err)
    }

    fn check_err(&self, err: *mut c_char) -> Result<()> {
        if err.is_null() {
            return Ok(());
        }
        let msg = unsafe { CStr::from_ptr(err) }
            .to_string_lossy()
            .into_owned();
        unsafe { (self.exports.free)(err as *mut c_void) };
        Err(Error(msg))
    }
}

/// One loaded plugin. Carries the exports, so it is independent of the `Host` borrow;
/// the `Host` must outlive it (its exports point into the still-mapped runtime).
pub struct Plugin {
    handle: *mut c_void,
    exports: Exports,
}
unsafe impl Send for Plugin {}
unsafe impl Sync for Plugin {}

impl Plugin {
    /// JSON of the effects and FileTypes the plugin provides (`docs/pdn-bridge.md`).
    pub fn enumerate(&self) -> Result<String> {
        signal_stack();
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { (self.exports.enumerate)(self.handle, &mut err) };
        self.take_string(p, err)
    }

    /// Put `image` (straight RGBA8, `width`×`height`) on the clipboard plug-ins read, or empty it with `None`.
    /// The host has one clipboard for every plug-in: set it before rendering an effect that reads it.
    pub fn set_clipboard(&self, image: Option<(&[u8], u32, u32)>) -> Result<()> {
        signal_stack();
        let bgra = image.map(|(rgba, w, h)| {
            let mut b = rgba.to_vec();
            swap_rb(&mut b);
            (b, w, h)
        });
        let (ptr, w, h) = bgra
            .as_ref()
            .map_or((ptr::null(), 0, 0), |(b, w, h)| (b.as_ptr(), *w, *h));
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { (self.exports.set_clipboard)(ptr, w as c_int, h as c_int, &mut err) };
        self.check_err(err)
    }

    /// JSON schema of one effect's properties.
    pub fn effect_schema(&self, effect_id: &str) -> Result<String> {
        signal_stack();
        let id = CString::new(effect_id).map_err(|_| Error("effect id has a NUL".into()))?;
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { (self.exports.schema)(self.handle, id.as_ptr(), &mut err) };
        self.take_string(p, err)
    }

    /// Render one effect over an RGBA8 buffer in place. `roi` is (x, y, w, h); `cancel` is
    /// polled between render strips.
    #[allow(clippy::too_many_arguments)]
    pub fn render_effect(
        &self,
        effect_id: &str,
        props_json: Option<&str>,
        rgba: &mut [u8],
        width: u32,
        height: u32,
        roi: (i32, i32, i32, i32),
        cancel: &AtomicBool,
    ) -> Result<()> {
        signal_stack();
        let id = CString::new(effect_id).map_err(|_| Error("effect id has a NUL".into()))?;
        let props = props_json.map(|p| CString::new(p).unwrap());
        let props_ptr = props.as_ref().map_or(ptr::null(), |c| c.as_ptr());

        swap_rb(rgba); // RGBA -> BGRA for the managed side
        let mut err: *mut c_char = ptr::null_mut();
        let rc = unsafe {
            (self.exports.render)(
                self.handle,
                id.as_ptr(),
                props_ptr,
                rgba.as_mut_ptr(),
                width as c_int,
                height as c_int,
                roi.0,
                roi.1,
                roi.2,
                roi.3,
                cancel.as_ptr() as *mut c_int,
                &mut err,
            )
        };
        swap_rb(rgba); // BGRA -> RGBA (restores the buffer whether rendered, cancelled or failed)
        self.check_err(err)?;
        if rc != 0 {
            return Err(Error("render failed".into()));
        }
        Ok(())
    }

    /// Press the button `key` of a classic effect's own settings window over an RGBA8 image, the window loaded with
    /// `props_json` (its kept `$token`, then the user's values): the JSON of the token that makes and the window's
    /// settings after it (`{"token": ..., "properties": [...]}`).
    pub fn dialog_press(
        &self,
        effect_id: &str,
        props_json: Option<&str>,
        key: &str,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<String> {
        signal_stack();
        let id = CString::new(effect_id).map_err(|_| Error("effect id has a NUL".into()))?;
        let key = CString::new(key).map_err(|_| Error("button key has a NUL".into()))?;
        let props = props_json.map(|p| CString::new(p).unwrap());
        let props_ptr = props.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        let mut bgra = rgba.to_vec();
        swap_rb(&mut bgra);
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe {
            (self.exports.press)(
                self.handle,
                id.as_ptr(),
                props_ptr,
                key.as_ptr(),
                bgra.as_ptr(),
                width as c_int,
                height as c_int,
                &mut err,
            )
        };
        self.take_string(p, err)
    }

    /// Encode an RGBA8 image to the plugin FileType at `index`; returns the file bytes.
    pub fn filetype_save(
        &self,
        index: u32,
        props_json: Option<&str>,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        signal_stack();
        let mut bgra = rgba.to_vec();
        swap_rb(&mut bgra);
        let props = props_json.map(|p| CString::new(p).unwrap());
        let props_ptr = props.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        let mut out_len: c_int = 0;
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe {
            (self.exports.save)(
                self.handle,
                index as c_int,
                props_ptr,
                bgra.as_ptr(),
                width as c_int,
                height as c_int,
                &mut out_len,
                &mut err,
            )
        };
        self.check_err(err)?;
        if p.is_null() {
            return Err(Error("save produced no bytes".into()));
        }
        let bytes = unsafe { std::slice::from_raw_parts(p, out_len as usize) }.to_vec();
        unsafe { (self.exports.free)(p as *mut c_void) };
        Ok(bytes)
    }

    /// Decode file bytes to an RGBA8 image through the plugin FileType at `index`; `props_json` carries its
    /// windows' answers (`{"$windows": [...]}`, docs/pdn-bridge.md "FileType windows"), or is `None`.
    pub fn filetype_open(
        &self,
        index: u32,
        bytes: &[u8],
        props_json: Option<&str>,
    ) -> Result<(Vec<u8>, u32, u32)> {
        signal_stack();
        let mut out: *mut u8 = ptr::null_mut();
        let (mut w, mut h): (c_int, c_int) = (0, 0);
        let mut err: *mut c_char = ptr::null_mut();
        let props = props_json.map(|p| CString::new(p).unwrap());
        let props_ptr = props.as_ref().map_or(ptr::null(), |c| c.as_ptr());
        let rc = unsafe {
            (self.exports.open)(
                self.handle,
                index as c_int,
                bytes.as_ptr(),
                bytes.len() as c_int,
                props_ptr,
                &mut out,
                &mut w,
                &mut h,
                &mut err,
            )
        };
        self.check_err(err)?;
        if rc != 0 || out.is_null() {
            return Err(Error("open failed".into()));
        }
        let n = (w as usize) * (h as usize) * 4;
        let mut rgba = unsafe { std::slice::from_raw_parts(out, n) }.to_vec();
        unsafe { (self.exports.free)(out as *mut c_void) };
        swap_rb(&mut rgba);
        Ok((rgba, w as u32, h as u32))
    }

    /// Open the settings window of effect `effect_id` over an RGBA8 image and keep it open, with `props_json` (the
    /// user's values) applied, for its live canvas (`CanvasSession`). The host keeps one open: opening another
    /// closes it.
    pub fn canvas_open(
        &self,
        effect_id: &str,
        props_json: Option<&str>,
        rgba: &[u8],
        width: u32,
        height: u32,
    ) -> Result<CanvasSession> {
        signal_stack();
        let canvas = self
            .exports
            .canvas
            .ok_or_else(|| Error("the .NET plug-in host has no live canvas".into()))?;
        let id = CString::new(effect_id).map_err(|_| Error("effect id has a NUL".into()))?;
        let props = props_json.map(cstring).transpose()?;
        let mut bgra = rgba.to_vec();
        swap_rb(&mut bgra);
        let mut err: *mut c_char = ptr::null_mut();
        let session = unsafe {
            (canvas.open)(
                self.handle,
                id.as_ptr(),
                props.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                bgra.as_ptr(),
                width as c_int,
                height as c_int,
                &mut err,
            )
        };
        self.check_err(err)?;
        if session <= 0 {
            return Err(Error("the settings window did not open".into()));
        }
        Ok(CanvasSession {
            id: session,
            canvas,
            free: self.exports.free,
        })
    }

    fn take_string(&self, p: *mut c_char, err: *mut c_char) -> Result<String> {
        take_string(self.exports.free, p, err)
    }

    fn check_err(&self, err: *mut c_char) -> Result<()> {
        check_err(self.exports.free, err)
    }
}

fn cstring(s: &str) -> Result<CString> {
    CString::new(s).map_err(|_| Error("text has a NUL".into()))
}

fn take_string(free: FreeFn, p: *mut c_char, err: *mut c_char) -> Result<String> {
    check_err(free, err)?;
    if p.is_null() {
        return Err(Error("null result".into()));
    }
    let s = unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned();
    unsafe { free(p as *mut c_void) };
    Ok(s)
}

fn check_err(free: FreeFn, err: *mut c_char) -> Result<()> {
    if err.is_null() {
        return Ok(());
    }
    let msg = unsafe { CStr::from_ptr(err) }
        .to_string_lossy()
        .into_owned();
    unsafe { free(err as *mut c_void) };
    Err(Error(msg))
}

/// A plug-in's settings window kept open over an image, whose custom-drawn control paint.rs shows and sends input
/// to (`Plugin::canvas_open`). Dropping it closes the window.
pub struct CanvasSession {
    id: c_int,
    canvas: CanvasExports,
    free: FreeFn,
}

impl CanvasSession {
    /// Apply `props_json` (the window's other settings as the user left them), send `events_json` (the input since the
    /// last call: a JSON array, `docs/pdn-bridge.md` "live canvas") to the canvas `key`, and return what it draws
    /// now: straight RGBA8 at its size.
    pub fn input(&self, props_json: Option<&str>, key: &str, events_json: &str) -> Result<Vec<u8>> {
        signal_stack();
        let props = props_json.map(cstring).transpose()?;
        let (key, events) = (cstring(key)?, cstring(events_json)?);
        let (mut len, mut err): (c_int, *mut c_char) = (0, ptr::null_mut());
        let p = unsafe {
            (self.canvas.input)(
                self.id,
                props.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                key.as_ptr(),
                events.as_ptr(),
                &mut len,
                &mut err,
            )
        };
        self.take_rgba(p, len, err)
    }

    /// What the canvas `key` draws now, before any new input: straight RGBA8 at its size.
    pub fn paint(&self, key: &str) -> Result<Vec<u8>> {
        signal_stack();
        let key = cstring(key)?;
        let (mut len, mut err): (c_int, *mut c_char) = (0, ptr::null_mut());
        let p = unsafe { (self.canvas.paint)(self.id, key.as_ptr(), &mut len, &mut err) };
        self.take_rgba(p, len, err)
    }

    /// The window's token now, as the host keeps it: the `$token` the effect renders with.
    pub fn token(&self) -> Result<String> {
        signal_stack();
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { (self.canvas.token)(self.id, &mut err) };
        take_string(self.free, p, err)
    }

    /// Where the canvas `key` is in its window: "formW,formH,x,y,w,h", the window's client size and the canvas's
    /// bounds in it. `None` from a host without it.
    pub fn layout(&self, key: &str) -> Result<Option<String>> {
        let Some(layout) = self.canvas.layout else {
            return Ok(None);
        };
        signal_stack();
        let key = cstring(key)?;
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { layout(self.id, key.as_ptr(), &mut err) };
        take_string(self.free, p, err).map(Some)
    }

    /// Press the button `key` of the open window, after applying `props_json` (its other settings as the user left
    /// them): the window's token and settings after it, as `Plugin::dialog_press` gives them. `None` from a host
    /// without it.
    pub fn press(&self, props_json: Option<&str>, key: &str) -> Result<Option<String>> {
        let Some(press) = self.canvas.press else {
            return Ok(None);
        };
        signal_stack();
        let props = props_json.map(cstring).transpose()?;
        let key = cstring(key)?;
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe {
            press(
                self.id,
                props.as_ref().map_or(ptr::null(), |c| c.as_ptr()),
                key.as_ptr(),
                &mut err,
            )
        };
        take_string(self.free, p, err).map(Some)
    }

    /// The open window's settings now (`{"properties": [...]}`, as `press` gives them), which input to it can change.
    /// `None` from a host without it.
    pub fn values(&self) -> Result<Option<String>> {
        let Some(values) = self.canvas.values else {
            return Ok(None);
        };
        signal_stack();
        let mut err: *mut c_char = ptr::null_mut();
        let p = unsafe { values(self.id, &mut err) };
        take_string(self.free, p, err).map(Some)
    }

    /// Hand the open window the file `name` (`bytes`) its last press asked for (a press's `pick`): the next press of
    /// that button finds it chosen. `false` from a host without it.
    pub fn provide_file(&self, name: &str, bytes: &[u8]) -> Result<bool> {
        let Some(provide) = self.canvas.provide_file else {
            return Ok(false);
        };
        signal_stack();
        let name = cstring(name)?;
        let len =
            c_int::try_from(bytes.len()).map_err(|_| Error("the file is too large".into()))?;
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { provide(self.id, name.as_ptr(), bytes.as_ptr(), len, &mut err) };
        check_err(self.free, err).map(|()| true)
    }

    /// Keep the window's token past its closing, for Repeat: the user pressed OK. The host keeps the last one kept.
    pub fn keep(&self) -> Result<()> {
        signal_stack();
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { (self.canvas.keep)(self.id, &mut err) };
        check_err(self.free, err)
    }

    fn take_rgba(&self, p: *mut u8, len: c_int, err: *mut c_char) -> Result<Vec<u8>> {
        check_err(self.free, err)?;
        if p.is_null() {
            return Err(Error("the canvas drew nothing".into()));
        }
        let mut rgba = unsafe { std::slice::from_raw_parts(p, len.max(0) as usize) }.to_vec();
        unsafe { (self.free)(p as *mut c_void) };
        swap_rb(&mut rgba);
        Ok(rgba)
    }
}

impl Drop for CanvasSession {
    fn drop(&mut self) {
        signal_stack();
        let mut err: *mut c_char = ptr::null_mut();
        unsafe { (self.canvas.close)(self.id, &mut err) };
        let _ = check_err(self.free, err);
    }
}

impl Drop for Plugin {
    fn drop(&mut self) {
        signal_stack();
        unsafe { (self.exports.free_plugin)(self.handle) };
    }
}

fn swap_rb(buf: &mut [u8]) {
    for px in buf.as_chunks_mut::<4>().0 {
        px.swap(0, 2);
    }
}

unsafe fn sym<'a, T>(
    lib: &'a libloading::Library,
    name: &[u8],
) -> Result<libloading::Symbol<'a, T>> {
    unsafe { lib.get(name) }.map_err(|e| {
        Error(format!(
            "hostfxr missing {}: {e}",
            String::from_utf8_lossy(name)
        ))
    })
}

/// The shared framework's assemblies the bridge ships its own copy of, to bind in the default load
/// context instead: .NET 9+ keeps only a throwing stub of BinaryFormatter in the framework, and the
/// runtime's ResourceReader binds it there to read a .NET Framework plugin's serialized icon (nearly every
/// Red ochre plug-in loads one in its constructor). The System.Runtime.Serialization.Formatters package
/// beside `PdnBridge.dll` carries the real one.
const PREFERRED: &[&str] = &["System.Runtime.Serialization.Formatters.dll"];

/// Swap [`PREFERRED`] in the trusted platform assembly list (which the runtime reads when it starts, in
/// `hostfxr_get_runtime_delegate`) for the bridge's copies.
unsafe fn prefer_bridge_copies(
    lib: &libloading::Library,
    ctx: HostfxrHandle,
    bridge_dir: &Path,
) -> Result<()> {
    let get: libloading::Symbol<GetPropFn> =
        unsafe { sym(lib, b"hostfxr_get_runtime_property_value")? };
    let set: libloading::Symbol<SetPropFn> =
        unsafe { sym(lib, b"hostfxr_set_runtime_property_value")? };
    let key = chart_str("TRUSTED_PLATFORM_ASSEMBLIES");
    let mut value: *const CharT = ptr::null();
    if unsafe { get(ctx, key.as_ptr(), &mut value) } != 0 || value.is_null() {
        return Ok(());
    }
    let tpa = unsafe { from_chart(value) };
    let sep = if cfg!(windows) { ';' } else { ':' };
    let ours: Vec<PathBuf> = PREFERRED
        .iter()
        .map(|f| bridge_dir.join(f))
        .filter(|p| p.exists())
        .collect();
    let mut list: Vec<String> = tpa
        .split(sep)
        .filter(|p| {
            !ours
                .iter()
                .any(|o| Path::new(p).file_name() == o.file_name())
        })
        .map(str::to_owned)
        .collect();
    list.extend(ours.iter().map(|p| p.to_string_lossy().into_owned()));
    let joined = chart_str(&list.join(&sep.to_string()));
    match unsafe { set(ctx, key.as_ptr(), joined.as_ptr()) } {
        0 => Ok(()),
        rc => Err(Error(format!(
            "setting the trusted assemblies failed: 0x{rc:08x}"
        ))),
    }
}

/// A NUL-terminated hostfxr `char_t` string as text.
unsafe fn from_chart(p: *const CharT) -> String {
    let mut n = 0;
    while unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    let s = unsafe { std::slice::from_raw_parts(p, n) };
    #[cfg(windows)]
    {
        String::from_utf16_lossy(s)
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(&s.iter().map(|&c| c as u8).collect::<Vec<u8>>()).into_owned()
    }
}

/// A UTF-8 C string for our own ABI (plugin paths, effect ids). `to_string_lossy` keeps
/// this cross-platform; a non-UTF-8 path is best-effort (plugin paths are app-controlled).
fn cstr(path: &Path) -> Result<CString> {
    CString::new(path.to_string_lossy().into_owned())
        .map_err(|_| Error("path has a NUL byte".into()))
}

/// A NUL-terminated hostfxr `char_t` string from a path.
fn chart_path(path: &Path) -> Result<Vec<CharT>> {
    #[cfg(windows)]
    let mut v: Vec<CharT> = {
        use std::os::windows::ffi::OsStrExt;
        path.as_os_str().encode_wide().collect()
    };
    #[cfg(not(windows))]
    let mut v: Vec<CharT> = {
        use std::os::unix::ffi::OsStrExt;
        path.as_os_str()
            .as_bytes()
            .iter()
            .map(|&b| b as CharT)
            .collect()
    };
    if v.contains(&0) {
        return Err(Error("path has a NUL byte".into()));
    }
    v.push(0);
    Ok(v)
}

/// A NUL-terminated hostfxr `char_t` string from ASCII text (type/method names).
fn chart_str(s: &str) -> Vec<CharT> {
    #[cfg(windows)]
    {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }
    #[cfg(not(windows))]
    {
        s.bytes()
            .map(|b| b as CharT)
            .chain(std::iter::once(0))
            .collect()
    }
}

/// Find the hostfxr library: `$PDN_HOSTFXR`, then the bridge's own (a self-contained release bundles
/// the runtime beside `PdnBridge.dll`), then `$DOTNET_ROOT` and the per-OS default install roots,
/// newest version. `host/fxr/<ver>/{libhostfxr.so|.dylib|hostfxr.dll}`.
fn find_hostfxr(bridge_dir: &Path) -> Result<PathBuf> {
    if let Ok(p) = std::env::var("PDN_HOSTFXR") {
        return Ok(PathBuf::from(p));
    }
    let lib = if cfg!(windows) {
        "hostfxr.dll"
    } else if cfg!(target_os = "macos") {
        "libhostfxr.dylib"
    } else {
        "libhostfxr.so"
    };
    if bridge_dir.join(lib).exists() {
        return Ok(bridge_dir.join(lib));
    }
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(r) = std::env::var("DOTNET_ROOT") {
        roots.push(PathBuf::from(r));
    }
    #[cfg(windows)]
    {
        roots.push(PathBuf::from(r"C:\Program Files\dotnet"));
    }
    #[cfg(target_os = "macos")]
    {
        roots.push(PathBuf::from("/usr/local/share/dotnet"));
        roots.push(PathBuf::from("/opt/homebrew/share/dotnet"));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        roots.push(PathBuf::from("/usr/share/dotnet"));
        roots.push(PathBuf::from("/usr/lib/dotnet"));
    }
    for root in roots {
        let fxr = root.join("host").join("fxr");
        let Ok(entries) = std::fs::read_dir(&fxr) else {
            continue;
        };
        let mut versions: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
        versions.sort();
        if let Some(dir) = versions.last() {
            let so = dir.join(lib);
            if so.exists() {
                return Ok(so);
            }
        }
    }
    Err(Error(format!(
        "could not find {lib} (set DOTNET_ROOT or PDN_HOSTFXR)"
    )))
}
