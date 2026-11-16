//! Installed plug-in files (Settings ▸ Plugins), kept in the `plugins/` folder of the app's data
//! folder on the desktop and in the browser's IndexedDB on the web ([`PluginStore`]), all of them
//! Effects ▸ Plugins items:
//! - Paint.NET plug-ins (`.dll`), loaded through the embedded .NET host (`pdn_core::effect::dotnet`:
//!   CoreCLR on the desktop, the Mono browser host on the web) by [`PluginManager::reload_effects`];
//! - PhotoCraft filters (`.wasm`, run sandboxed by `pdn_core::extra::plugins`), LightCraft looks
//!   (`.cube`, `.3dl`) and develop presets (`.xmp`), activated into `pdn_core::effect::wasm`
//!   ([`activate`]). A file that no longer loads is reported and skipped, never fatal.
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use pdn_core::effect::wasm;
use pdn_core::extra::fx::lut;
use pdn_core::extra::plugins::{Limits, Plugin, registry};

use super::{Files, Pending, PickedFile};

/// Installed plug-in files by name, with their bytes.
type NamedFiles = Vec<(String, Vec<u8>)>;

/// What the install picker offers: any plug-in, look or preset file, or a `.zip` of them.
pub const ACCEPT: &str = ".dll,.wasm,.cube,.3dl,.xmp,.zip";
/// The largest file installed.
const MAX_BYTES: u64 = 32 << 20;

/// Where the installed files live: by file name, their bytes.
#[derive(Clone)]
pub enum PluginStore {
    Memory(Rc<RefCell<BTreeMap<String, Vec<u8>>>>),
    #[cfg(not(target_arch = "wasm32"))]
    Native(std::path::PathBuf),
    /// The browser's IndexedDB (`plugin_web`): plug-ins persist across reloads and the Mono
    /// browser host loads them at start.
    #[cfg(target_arch = "wasm32")]
    Browser,
    Unavailable,
}

impl PluginStore {
    pub fn memory() -> Self {
        Self::Memory(Rc::default())
    }

    pub fn system() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::icon_pack::app_data_dir()
                .map(|d| Self::Native(d.join("plugins")))
                .unwrap_or(Self::Unavailable)
        }
        // The browser host (pdn_core::effect::dotnet) loads from IndexedDB.
        #[cfg(target_arch = "wasm32")]
        {
            Self::Browser
        }
    }

    /// The desktop folder (users may also drop files there by hand).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn folder(&self) -> Option<&std::path::Path> {
        match self {
            Self::Native(dir) => Some(dir),
            _ => None,
        }
    }

    /// The installed file names.
    fn load(&self) -> Pending<Result<Vec<String>, String>> {
        match self {
            Self::Memory(m) => Pending::ready(Ok(m.borrow().keys().cloned().collect())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(dir) => {
                let dir = dir.clone();
                super::native::background(move || {
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        return Ok(vec![]);
                    };
                    let mut files: Vec<_> = entries
                        .filter_map(|e| e.ok())
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|name| accepted(name))
                        .collect();
                    files.sort();
                    Ok(files)
                })
            }
            #[cfg(target_arch = "wasm32")]
            Self::Browser => Pending::future(async {
                super::plugin_web::list().await.map(|mut f| {
                    f.sort();
                    f
                })
            }),
            Self::Unavailable => Pending::ready(Ok(vec![])),
        }
    }

    /// Every installed file `pick` accepts, by name, with its bytes.
    fn contents(&self, pick: fn(&str) -> bool) -> Pending<Result<NamedFiles, String>> {
        let keep = move |files: NamedFiles| files.into_iter().filter(|(n, _)| pick(n)).collect();
        match self {
            Self::Memory(m) => Pending::ready(Ok(keep(
                m.borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect(),
            ))),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(dir) => {
                let dir = dir.clone();
                super::native::background(move || {
                    let Ok(entries) = std::fs::read_dir(&dir) else {
                        return Ok(vec![]);
                    };
                    Ok(entries
                        .filter_map(|e| e.ok())
                        .filter_map(|e| e.file_name().into_string().ok())
                        .filter(|n| accepted(n) && pick(n))
                        .filter_map(|n| std::fs::read(dir.join(&n)).ok().map(|b| (n, b)))
                        .collect())
                })
            }
            #[cfg(target_arch = "wasm32")]
            Self::Browser => {
                Pending::future(async move { super::plugin_web::all().await.map(keep) })
            }
            Self::Unavailable => Pending::ready(Ok(vec![])),
        }
    }

    fn put(&self, file: &str, bytes: &[u8]) -> Pending<Result<(), String>> {
        match self {
            Self::Memory(m) => {
                m.borrow_mut().insert(file.into(), bytes.to_vec());
                Pending::ready(Ok(()))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(dir) => Pending::ready(
                std::fs::create_dir_all(dir)
                    .and_then(|()| {
                        // Written beside it, then renamed over: a crash leaves the old file.
                        let tmp = dir.join(format!(".{file}.part"));
                        std::fs::write(&tmp, bytes)?;
                        std::fs::rename(&tmp, dir.join(file))
                    })
                    .map_err(|e| e.to_string()),
            ),
            #[cfg(target_arch = "wasm32")]
            Self::Browser => {
                Pending::future(super::plugin_web::put(file.to_owned(), bytes.to_vec()))
            }
            Self::Unavailable => Pending::ready(Err("Plug-in storage is unavailable".into())),
        }
    }

    fn remove(&self, file: &str) -> Pending<Result<(), String>> {
        match self {
            Self::Memory(m) => {
                m.borrow_mut().remove(file);
                Pending::ready(Ok(()))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(dir) => Pending::ready(match std::fs::remove_file(dir.join(file)) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
                _ => Ok(()),
            }),
            #[cfg(target_arch = "wasm32")]
            Self::Browser => Pending::future(super::plugin_web::delete(file.to_owned())),
            Self::Unavailable => Pending::ready(Ok(())),
        }
    }
}

/// A plug-in, look or preset file, judged by its extension.
pub fn accepted(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".dll", ".dlc", ".json", ".wasm", ".cube", ".3dl", ".xmp"]
        .iter()
        .any(|e| lower.ends_with(e))
        && !lower.starts_with('.')
}

/// A Paint.NET plug-in (`.dll`), loaded by the .NET host rather than activated here.
fn is_dll(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".dll")
}

/// A Paint.NET plug-in or a text file one reads from beside itself (toe_head2001's option-based plug-ins take
/// their names and captions from `.dlc` files in their zip, Koh's GPU effects their settings from `.json` files): kept
/// for the .NET host, not activated here.
fn is_dotnet(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    is_dll(name) || lower.ends_with(".dlc") || lower.ends_with(".json")
}

/// Activates a PhotoCraft filter (`.wasm`), LightCraft look (`.cube`, `.3dl`) or develop preset
/// (`.xmp`) as an Effects ▸ Plugins item: its stored file name (a plug-in's is its id, so a
/// reinstall replaces it), its effect id and its display name.
pub fn activate(name: &str, bytes: &[u8]) -> Result<(String, String, String), String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".wasm") {
        let plugin = Plugin::load(bytes, Limits::default()).map_err(|e| format!("{name}: {e}"))?;
        let m = plugin.manifest();
        let out = (format!("{}.wasm", m.id), m.id.clone(), m.name.clone());
        registry::install(plugin);
        return Ok(out);
    }
    let (stem, ext) = name.rsplit_once('.').unwrap_or((name, ""));
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if lower.ends_with(".xmp") {
        let id = format!("preset.{safe}");
        wasm::install_preset(&id, stem, bytes).map_err(|e| format!("{name}: {e}"))?;
        return Ok((format!("{safe}.xmp"), id, stem.to_owned()));
    }
    if lower.ends_with(".cube") || lower.ends_with(".3dl") {
        let table = lut::parse(name, bytes).map_err(|e| format!("{name}: {e}"))?;
        let id = format!("lut.{safe}");
        let title = Some(table.title.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or(stem)
            .to_owned();
        wasm::install_look(&id, &title, table);
        return Ok((format!("{safe}.{}", ext.to_ascii_lowercase()), id, title));
    }
    Err(format!(
        "{name} is not a plug-in (.dll, .wasm), a look (.cube, .3dl) or a preset (.xmp)."
    ))
}

/// Undoes [`activate`] for effect `id`.
fn deactivate(id: &str) {
    if !wasm::remove_look(id) {
        registry::remove(id);
    }
}

/// Where PdnBridge.dll + its runtimeconfig + the replacement PaintDotNet.*.dll live:
/// `$PDN_BRIDGE_DIR`, else `pdn/` beside the executable (the app-local release layout).
#[cfg(not(target_arch = "wasm32"))]
fn bridge_dir() -> Option<std::path::PathBuf> {
    if let Ok(d) = std::env::var("PDN_BRIDGE_DIR") {
        return Some(d.into());
    }
    let local = std::env::current_exe().ok()?.parent()?.join("pdn");
    local.join("PdnBridge.dll").exists().then_some(local)
}

/// The base name of a path from a `.zip`, without directory parts.
fn base(name: &str) -> &str {
    name.rsplit(['/', '\\']).next().unwrap_or(name)
}

/// The accepted files of a `.zip` bundle (by their base names), within the install limits.
fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (mut out, mut total) = (vec![], 0u64);
    for i in 0..archive.len().min(256) {
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = base(file.name()).to_owned();
        if !file.is_file() || !accepted(&name) || file.size() > MAX_BYTES {
            continue;
        }
        total += file.size();
        if total > 4 * MAX_BYTES {
            return Err("the bundle is too large".into());
        }
        let mut data = Vec::new();
        file.take(MAX_BYTES + 1)
            .read_to_end(&mut data)
            .map_err(|e| e.to_string())?;
        out.push((name, data));
    }
    Ok(out)
}

enum Event {
    Loaded(Vec<String>),
    /// The installed PhotoCraft/LightCraft files read back at start, to activate.
    Activate(Vec<(String, Vec<u8>)>),
    Picked(Vec<PickedFile>),
    Read(PickedFile),
    Done,
    /// The store refused a file (the browser's storage full, say), which is then not installed.
    NotStored(String, String),
    /// The browser host booted and (re)loaded the installed plug-ins: the runtime download size
    /// (MiB) and any per-plug-in load errors.
    #[cfg(target_arch = "wasm32")]
    Installed {
        runtime_mib: f64,
        errors: Vec<(String, String)>,
    },
}

pub struct PluginManager {
    store: PluginStore,
    /// The installed file names.
    pub items: Vec<String>,
    /// The activated PhotoCraft/LightCraft files: file name → effect id.
    active: BTreeMap<String, String>,
    /// The last operation's outcome, for the Settings page.
    pub status: String,
    /// The installed plug-ins that did not load at the last (re)load: file name and reason.
    pub failed: Vec<(String, String)>,
    ready: bool,
    /// A store write or delete finished since the last (re)load.
    stale: bool,
    /// The first load has not finished yet.
    starting: bool,
    /// The one notice at startup that installed plug-ins did not load, for the app to show once.
    pub notice: Option<String>,
    #[cfg(test)]
    reloads: u32,
    pending: VecDeque<Pending<Result<Event, String>>>,
}

impl PluginManager {
    pub fn new(store: PluginStore) -> Self {
        let load = store.load();
        Self {
            store,
            items: vec![],
            active: BTreeMap::new(),
            status: String::new(),
            failed: vec![],
            ready: false,
            stale: false,
            starting: true,
            notice: None,
            #[cfg(test)]
            reloads: 0,
            pending: VecDeque::from([Pending::future(
                async move { load.await.map(Event::Loaded) },
            )]),
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn folder(&self) -> Option<&std::path::Path> {
        self.store.folder()
    }

    /// Installs and stores a file the user gave; a `.zip` bundle stores each `.dll` in it.
    pub fn install(&mut self, name: &str, bytes: &[u8]) {
        if bytes.starts_with(b"PK\x03\x04") {
            match unzip(bytes) {
                Ok(files) if !files.is_empty() => {
                    for (n, b) in files {
                        self.install(&n, &b);
                    }
                }
                Ok(_) => self.status = format!("{} holds no plug-ins.", base(name)),
                Err(e) => self.status = format!("{}: {e}", base(name)),
            }
            return;
        }
        let mut file = base(name).to_owned();
        if !accepted(&file) {
            self.status = format!(
                "{file} is not a plug-in (.dll, .wasm), a look (.cube, .3dl), a preset (.xmp) or a .zip of them."
            );
            return;
        }
        if bytes.len() as u64 > MAX_BYTES {
            self.status = format!("{file} is too large.");
            return;
        }
        self.status = format!("Installed {file}.");
        if !is_dotnet(&file) {
            match activate(&file, bytes) {
                Ok((stored, id, title)) => {
                    self.status = format!("Installed {title}.");
                    self.active.insert(stored.clone(), id);
                    file = stored;
                }
                Err(e) => {
                    self.status = e;
                    return;
                }
            }
        }
        let put = self.store.put(&file, bytes);
        let stored = file.clone();
        self.pending.push_back(Pending::future(async move {
            Ok(match put.await {
                Ok(()) => Event::Done,
                Err(e) => Event::NotStored(stored, e),
            })
        }));
        self.items.retain(|f| f != &file);
        self.items.push(file);
        self.items.sort_by_key(|f| f.to_lowercase());
    }

    /// Uninstalls file `file` and deletes it.
    pub fn remove(&mut self, file: &str) {
        let Some(at) = self.items.iter().position(|f| f == file) else {
            return;
        };
        let file = self.items.remove(at);
        if let Some(id) = self.active.remove(&file) {
            deactivate(&id);
        }
        self.status = format!("Removed {file}.");
        let remove = self.store.remove(&file);
        self.pending.push_back(Pending::future(async move {
            remove.await.map(|()| Event::Done)
        }));
    }

    /// Settings ▸ Plugins ▸ Install…: the picker, then each file read and installed.
    pub fn pick(&mut self) {
        let pick = Files::current().choose_open(true, ACCEPT);
        self.pending.push_back(Pending::future(
            async move { pick.await.map(Event::Picked) },
        ));
    }

    /// Dropped or picked files: the accepted ones are read and installed.
    pub fn import(&mut self, files: Vec<PickedFile>) {
        for file in files
            .into_iter()
            .filter(|f| accepted(&f.name) || f.name.to_ascii_lowercase().ends_with(".zip"))
        {
            let read = file.reread_within(MAX_BYTES);
            self.pending
                .push_back(Pending::future(async move { read.await.map(Event::Read) }));
        }
    }

    /// Advances the storage work; true when something changed.
    pub fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Some(result) = self.pending.front_mut().and_then(Pending::poll) {
            self.pending.pop_front();
            changed = true;
            match result {
                Ok(Event::Loaded(files)) => {
                    self.ready = true;
                    self.items = files;
                    self.reload_effects();
                    if self.items.iter().any(|f| !is_dotnet(f)) {
                        let read = self.store.contents(|f| !is_dotnet(f));
                        self.pending.push_back(Pending::future(async move {
                            read.await.map(Event::Activate)
                        }));
                    }
                }
                Ok(Event::Activate(files)) => {
                    let mut errors = vec![];
                    for (name, bytes) in files {
                        match activate(&name, &bytes) {
                            Ok((_, id, _)) => {
                                self.active.insert(name, id);
                            }
                            Err(e) => errors.push(e),
                        }
                    }
                    if !errors.is_empty() {
                        self.status = errors.join("  ");
                    }
                }
                Ok(Event::Picked(files)) => self.import(files),
                Ok(Event::Read(file)) => self.install(&file.name, &file.bytes),
                // A store write/delete finished, so the folder on disk is now current.
                Ok(Event::Done) => self.stale = true,
                Ok(Event::NotStored(file, e)) => {
                    self.items.retain(|f| f != &file);
                    if let Some(id) = self.active.remove(&file) {
                        deactivate(&id);
                    }
                    self.status = format!("{file} was not installed: {e}");
                }
                #[cfg(target_arch = "wasm32")]
                Ok(Event::Installed {
                    runtime_mib,
                    errors,
                }) => {
                    self.status = format!("Loaded plug-ins (.NET runtime {runtime_mib:.1} MiB).");
                    self.failed = errors;
                    self.loaded();
                }
                Err(e) => {
                    self.ready = true;
                    self.status = e;
                }
            }
        }
        // One reload once a batch (a pack's .zip, a multi-file drop) has all landed, not one per file.
        if self.stale && self.pending.is_empty() {
            self.stale = false;
            self.reload_effects();
        }
        changed
    }

    /// A (re)load finished: the first one raises the startup notice if any plug-in did not load.
    fn loaded(&mut self) {
        if !std::mem::take(&mut self.starting) || self.failed.is_empty() {
            return;
        }
        let n = self.failed.len();
        let mut text = if n == 1 {
            "A plug-in did not load:".to_string()
        } else {
            format!("{n} plug-ins did not load:")
        };
        for (file, reason) in self.failed.iter().take(5) {
            text.push_str(&format!("\n\n{file}: {reason}"));
        }
        if n > 5 {
            text.push_str(&format!("\n\n…and {} more.", n - 5));
        }
        text.push_str("\n\nSettings ▸ Plugins lists them. paint.rs works without them.");
        self.notice = Some(text);
    }

    /// (Re)load every installed `.dll` into Effects ▸ Plugins through the .NET host. Called
    /// after the folder is listed, installed to, or removed from; a no-op when no `.dll` is
    /// installed.
    fn reload_effects(&mut self) {
        #[cfg(test)]
        {
            self.reloads += 1;
        }
        let dlls: Vec<String> = self.items.iter().filter(|f| is_dll(f)).cloned().collect();
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.failed.clear();
            let (Some(dir), false) = (
                self.store.folder().map(|d| d.to_path_buf()),
                dlls.is_empty(),
            ) else {
                pdn_core::effect::dotnet::unload_all();
                self.loaded();
                return;
            };
            // Without the host no plug-in loads: each is listed with why.
            let host = bridge_dir()
                .ok_or_else(|| {
                    "the .NET plug-in host (pdn/PdnBridge.dll) was not found".to_string()
                })
                .and_then(|bdir| {
                    pdn_core::effect::dotnet::init(&bdir)
                        .map_err(|e| format!("the .NET plug-in host failed to start: {e}"))
                });
            pdn_core::effect::dotnet::unload_all();
            for file in dlls {
                let loaded = host
                    .clone()
                    .and_then(|()| pdn_core::effect::dotnet::load(&dir.join(&file)));
                match loaded {
                    Ok(skipped) if skipped.is_empty() => {}
                    Ok(skipped) => self
                        .failed
                        .push((file, pdn_core::effect::dotnet::partly(&skipped))),
                    Err(e) => self.failed.push((file, e)),
                }
            }
            self.loaded();
        }
        // The web host boots the .NET runtime lazily and loads from bytes, both asynchronous, so
        // the work is a queued future: read the installed plug-ins, boot, (re)load, register.
        // ponytail: reloads all plug-ins on any change; fine for a handful, revisit if it grows.
        #[cfg(target_arch = "wasm32")]
        {
            if dlls.is_empty() {
                pdn_core::effect::dotnet::unload_all();
                self.failed.clear();
                self.loaded();
                return;
            }
            let contents = self.store.contents(is_dotnet);
            self.pending.push_back(Pending::future(async move {
                let plugins = contents.await?;
                let (runtime_mib, errors) = pdn_core::effect::dotnet::install(plugins).await?;
                Ok(Event::Installed {
                    runtime_mib,
                    errors,
                })
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVERT: &[u8] =
        include_bytes!("../../../pdn-core/src/extra/plugins/testdata/invert.wasm");
    const CUBE: &str = "TITLE \"Test Look\"\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

    /// PhotoCraft filters and LightCraft looks install beside a Paint.NET `.dll`: each becomes an
    /// Effects ▸ Plugins item under its stored name (a plug-in's is its id), a bad module is
    /// reported and not stored, a new manager over the same store (the next start) activates them
    /// again, and removing one deactivates it.
    #[test]
    fn photocraft_and_lightcraft_beside_dll() {
        #[cfg(not(target_arch = "wasm32"))]
        let _host = host_lock();
        let store = PluginStore::memory();
        let mut m = PluginManager::new(store.clone());
        m.poll();
        m.install("Invert.wasm", INVERT);
        m.install("Film 01.cube", CUBE.as_bytes());
        m.install("Effect.dll", b"MZ fake assembly");
        m.poll();
        let installed = [
            "Effect.dll",
            "Film-01.cube",
            "org.photocraft.example.invert.wasm",
        ];
        assert_eq!(m.items, installed);
        assert!(pdn_core::effect::find("org.photocraft.example.invert").is_some());
        assert!(pdn_core::effect::find("lut.Film-01").is_some());
        m.install("notes.wasm", b"\0asm junk");
        assert!(m.status.starts_with("notes.wasm:"), "{}", m.status);
        assert_eq!(m.items, installed);

        pdn_core::effect::wasm::remove_look("lut.Film-01");
        let mut again = PluginManager::new(store.clone());
        while again.busy() {
            again.poll();
        }
        assert_eq!(again.items, installed);
        assert!(pdn_core::effect::find("lut.Film-01").is_some());

        again.remove("Film-01.cube");
        again.remove("org.photocraft.example.invert.wasm");
        again.poll();
        assert_eq!(again.items, ["Effect.dll"]);
        assert!(pdn_core::effect::find("lut.Film-01").is_none());
        assert!(pdn_core::effect::find("org.photocraft.example.invert").is_none());
    }

    /// Installing stores the file and lists it; a new manager over the same store loads it back;
    /// removing deletes both; a bundle stores each `.dll` and the `.dlc` and `.json` files beside them; a
    /// non-plug-in file is reported.
    #[test]
    fn install_load_and_remove() {
        #[cfg(not(target_arch = "wasm32"))]
        let _host = host_lock();
        let store = PluginStore::memory();
        let mut m = PluginManager::new(store.clone());
        m.poll();
        m.install("Invert.dll", b"MZ fake assembly");
        m.install("notes.txt", b"not a plug-in");
        m.poll();
        assert_eq!(m.items, ["Invert.dll"]);
        assert!(m.status.contains("not a plug-in"));

        // A bundle stores the plug-in files it holds (`.dll`, `.dlc`, `.json`) and skips the rest.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("effects/Warm.dll", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"MZ warm").unwrap();
        zip.start_file("effects/Warm.dlc", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"[EN-US]\nWarm=Warm").unwrap();
        zip.start_file("effects/Warm.json", opts).unwrap();
        std::io::Write::write_all(&mut zip, b"{}").unwrap();
        zip.start_file("readme.txt", opts).unwrap();
        let bundle = zip.finish().unwrap().into_inner();
        m.install("pack.zip", &bundle);
        m.poll();
        assert_eq!(m.items, ["Invert.dll", "Warm.dlc", "Warm.dll", "Warm.json"]);

        let mut again = PluginManager::new(store.clone());
        again.poll();
        assert_eq!(again.items, m.items);

        m.remove("Warm.dll");
        m.remove("Warm.dlc");
        m.remove("Warm.json");
        m.remove("Invert.dll");
        m.poll();
        assert!(m.items.is_empty());
        let PluginStore::Memory(files) = &store else {
            unreachable!()
        };
        assert!(files.borrow().is_empty());
    }

    /// The tests that load through the .NET host share its global registries: one at a time.
    #[cfg(not(target_arch = "wasm32"))]
    fn host_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A batch of installs (a pack's .zip) reloads the plug-ins once, not once per file.
    #[test]
    fn a_pack_reloads_once() {
        #[cfg(not(target_arch = "wasm32"))]
        let _host = host_lock();
        let mut m = PluginManager::new(PluginStore::memory());
        m.poll();
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for name in ["A.dll", "B.dll", "C.dll"] {
            zip.start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            std::io::Write::write_all(&mut zip, b"MZ junk").unwrap();
        }
        let reloads = m.reloads;
        m.install("pack.zip", &zip.finish().unwrap().into_inner());
        while m.busy() {
            m.poll();
        }
        assert_eq!(m.items, ["A.dll", "B.dll", "C.dll"]);
        assert_eq!(m.reloads, reloads + 1, "one reload for the whole pack");
    }

    /// Plug-ins that do not load are listed with why, and the first load raises one notice (a later
    /// failure is listed, not noticed).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn failed_plugins_are_listed_and_noticed_once() {
        let _host = host_lock();
        let folder = std::env::temp_dir().join(format!("paintrs-broken-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("Broken.dll"), b"MZ not a plug-in").unwrap();
        let mut m = PluginManager::new(PluginStore::Native(folder.clone()));
        while !m.ready() || m.busy() {
            m.poll();
        }
        assert_eq!(m.failed.len(), 1, "{:?}", m.failed);
        assert_eq!(m.failed[0].0, "Broken.dll");
        let notice = m.notice.take().expect("a startup notice");
        assert!(notice.contains("Broken.dll: "), "{notice}");
        m.install("Other.dll", b"MZ junk");
        while m.busy() {
            m.poll();
        }
        assert_eq!(m.failed.len(), 2);
        assert!(m.notice.is_none(), "the notice is for startup only");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Installs every plug-in DLL in `pack` through the app's plug-in manager (the published .NET
    /// host at $PDN_BRIDGE_DIR) and renders each loaded effect at its defaults through
    /// pdn_core::effect::apply (only the effects named in `only`, if any; all names are printed).
    /// Returns the effect names that rendered and changed the image, rendered unchanged, rendered
    /// fully transparent, and failed (with the host's notice).
    #[cfg(not(target_arch = "wasm32"))]
    fn render_pack(pack: &str, only: &[&str]) -> [Vec<String>; 4] {
        let _host = host_lock();
        let folder = std::env::temp_dir().join(format!("paintrs-pack-{}", std::process::id()));
        // A sandbox's process ids repeat from run to run: start empty, or an earlier run's packs load too.
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mut m = PluginManager::new(PluginStore::Native(folder.clone()));
        while !m.ready() {
            m.poll();
            std::thread::yield_now();
        }
        let mut files: Vec<_> = std::fs::read_dir(pack)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .collect();
        files.sort();
        for f in files {
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            if name.ends_with(".dll")
                && !name.starts_with("PaintDotNet.")
                && !name.starts_with("System.")
            {
                m.install(&name, &std::fs::read(&f).unwrap());
            }
        }
        while m.busy() {
            m.poll();
            std::thread::yield_now();
        }
        eprintln!("plug-in manager: {}", m.status);
        for (file, e) in &m.failed {
            eprintln!("load failed: {file}: {e}");
        }
        // PDN_PACK_IN=<48-wide straight BGRA> renders that image instead (the genuine oracle host's input),
        // and PDN_PACK_OUT=<dir> keeps each effect's output there as straight BGRA, to compare against it.
        let swap = |b: &[u8]| {
            b.as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[2], p[1], p[0], p[3]])
                .collect::<Vec<u8>>()
        };
        let src = match std::env::var("PDN_PACK_IN") {
            Ok(path) => {
                let bytes = std::fs::read(path).unwrap();
                pdn_core::canvas::ImageData {
                    width: 48,
                    height: bytes.len() as u32 / (48 * 4),
                    data: swap(&bytes),
                }
            }
            Err(_) => pdn_core::canvas::ImageData {
                width: 48,
                height: 36,
                data: (0..48 * 36 * 4)
                    .map(|i: u32| if i % 4 == 3 { 255 } else { (i * 7 % 251) as u8 })
                    .collect(),
            },
        };
        let out_dir = std::env::var("PDN_PACK_OUT").ok();
        // PDN_PACK_PROPS=<json file> replaces those properties' defaults (FillFromFile's FileName, set as the genuine
        // oracle host's --propsfile sets it).
        let props: serde_json::Value = std::env::var("PDN_PACK_PROPS")
            .map(|p| serde_json::from_slice(&std::fs::read(p).unwrap()).unwrap())
            .unwrap_or_default();
        let [mut changed, mut same, mut blank, mut failed] = [vec![], vec![], vec![], vec![]];
        let defs = pdn_core::effect::dotnet::effects();
        let name_of = |id| {
            pdn_core::effect::dotnet::name_of(id)
                .unwrap_or(id)
                .to_string()
        };
        let names: Vec<String> = defs.iter().map(|d| name_of(d.id)).collect();
        eprintln!("effects: {names:?}");
        for def in defs {
            let name = name_of(def.id);
            if !only.is_empty() && !only.contains(&name.as_str()) {
                continue;
            }
            eprintln!("rendering {name} ({})", def.id);
            let notice = std::sync::Mutex::new(None);
            let mut env = pdn_core::effect::EffectEnv::whole(src.width, src.height);
            env.notice = Some(&notice);
            let out =
                pdn_core::effect::apply(&src, &def, &pdn_core::effect::with(&def, &props), &env);
            if let Some(dir) = &out_dir {
                // Two effects can share a display name (PdnV5's two Square Blurs): those go by their id.
                let file = if names.iter().filter(|n| **n == name).count() > 1 {
                    def.id.to_string()
                } else {
                    name.replace('/', "_")
                };
                std::fs::write(format!("{dir}/{file}.bgra"), swap(&out.data)).unwrap();
            }
            match notice.into_inner().unwrap() {
                None if out.data.as_chunks::<4>().0.iter().all(|p| p[3] == 0) => blank.push(name),
                None if out.data != src.data => changed.push(name),
                None => same.push(name),
                Some(e) => failed.push(format!("{name}: {e}")),
            }
        }
        let _ = std::fs::remove_dir_all(&folder);
        for (label, v) in [
            ("rendered", &mut changed),
            ("unchanged", &mut same),
            ("blank", &mut blank),
            ("failed", &mut failed),
        ] {
            v.sort();
            eprintln!("{label} {}: {v:#?}", v.len());
        }
        [changed, same, blank, failed]
    }

    /// The app path end to end on the BoltBait pack ($PDN_BOLTBAIT_DIR, its bin). Nearly every
    /// BoltBait plug-in loads its icon through System.Drawing in its constructor, so this also
    /// proves the host's GDI+ (libgdiplus off Windows); Chart or Graph draws a WinForms chart (the
    /// host's chart control, an empty chart with no dialog). Skips unless $PDN_BRIDGE_DIR is set too.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn boltbait_pack_loads_and_renders() {
        let (Ok(_), Ok(pack)) = (
            std::env::var("PDN_BRIDGE_DIR"),
            std::env::var("PDN_BOLTBAIT_DIR"),
        ) else {
            eprintln!("skipping: set PDN_BRIDGE_DIR (a published bridge) and PDN_BOLTBAIT_DIR");
            return;
        };
        let [rendered, ..] = render_pack(&pack, &[]);
        for name in [
            "Chart or Graph",
            "Edge Detect Classic",
            "Gaussian Blur+",
            "Oil Painting+",
            "Pastel",
            "Sharpen Classic",
        ] {
            assert!(
                rendered.iter().any(|r| r.starts_with(name)),
                "{name} did not render: {rendered:?}"
            );
        }
    }

    /// A plug-in window's buttons change its state, which the token they make keeps, as Paint.NET keeps a window's
    /// token: Tartan's stripe lists start empty, "Horizontal" adds a stripe from the current width, spacing, style
    /// and colour, the list shows it, and the render draws it. Skips unless $PDN_BRIDGE_DIR and $PDN_TOEHEAD_DIR
    /// (toe_head2001's pack) are set.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn tartan_buttons_build_its_stripes() {
        use pdn_core::effect::{Control, dotnet};
        let (Ok(_), Ok(pack)) = (
            std::env::var("PDN_BRIDGE_DIR"),
            std::env::var("PDN_TOEHEAD_DIR"),
        ) else {
            eprintln!("skipping: set PDN_BRIDGE_DIR (a published bridge) and PDN_TOEHEAD_DIR");
            return;
        };
        let _host = host_lock();
        let folder = std::env::temp_dir().join(format!("paintrs-tartan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).unwrap();
        let mut m = PluginManager::new(PluginStore::Native(folder.clone()));
        while !m.ready() {
            m.poll();
            std::thread::yield_now();
        }
        let dll = std::path::Path::new(&pack).join("Tartan.dll");
        m.install("Tartan.dll", &std::fs::read(dll).unwrap());
        while m.busy() {
            m.poll();
            std::thread::yield_now();
        }
        let def = dotnet::effects()
            .into_iter()
            .find(|d| dotnet::name_of(d.id) == Some("Tartan"))
            .expect("Tartan loads");
        let button = |name: &str| {
            def.controls
                .iter()
                .find_map(|c| match c {
                    Control::Button { key, label } if *label == name => Some(*key),
                    _ => None,
                })
                .unwrap()
        };
        let list = def
            .controls
            .iter()
            .find_map(|c| match c {
                Control::List {
                    key,
                    label: "Horizontal Set",
                    ..
                } => Some(*key),
                _ => None,
            })
            .expect("its horizontal set");
        let src = pdn_core::canvas::ImageData {
            width: 64,
            height: 64,
            data: vec![255; 64 * 64 * 4],
        };
        let env = pdn_core::effect::EffectEnv::whole(64, 64);
        let mut values = pdn_core::effect::defaults(&def);
        assert!(pdn_core::effect::list_items(&values, list, &[]).is_empty());
        dotnet::press(def.id, button("Horizontal"), &src, &mut values, &env).unwrap();
        assert_eq!(pdn_core::effect::list_items(&values, list, &[]).len(), 1);
        let out = pdn_core::effect::apply(&src, &def, &values, &env);
        let colours: std::collections::HashSet<&[u8; 4]> =
            out.data.as_chunks::<4>().0.iter().collect();
        assert!(colours.len() > 1, "the stripe is drawn over the background");
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Any pack ($PDN_PACK_DIR, a folder of plug-in DLLs) through the same path, for triage: prints
    /// what failed to load and how each effect rendered at its defaults (PDN_PACK_ONLY=<name> runs
    /// one). Asserts nothing.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn plugin_pack_triage() {
        let (Ok(_), Ok(pack)) = (
            std::env::var("PDN_BRIDGE_DIR"),
            std::env::var("PDN_PACK_DIR"),
        ) else {
            eprintln!("skipping: set PDN_BRIDGE_DIR (a published bridge) and PDN_PACK_DIR");
            return;
        };
        let only = std::env::var("PDN_PACK_ONLY").ok();
        render_pack(&pack, only.as_deref().as_slice());
    }

    /// A third-party FileType plug-in through the app's Open and Save: the free Paint Shop Pro FileType
    /// ($PDN_PSP_DIR, else ref/pdn-plugins/pspformat/bin; see its SOURCES.md) installed through the
    /// plug-in manager is listed with its extensions and save options, an image saved with a
    /// non-default option is detected by its extension and opens back as it was. Skips without it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn filetype_plugin_opens_and_saves() {
        let dir = std::env::var("PDN_PSP_DIR").unwrap_or_else(|_| {
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../ref/pdn-plugins/pspformat/bin"
            )
            .into()
        });
        let dll = std::path::Path::new(&dir).join("PaintShopProFiletype.dll");
        if std::env::var("PDN_BRIDGE_DIR").is_err() || !dll.exists() {
            eprintln!(
                "skipping: set PDN_BRIDGE_DIR (a published bridge); needs {}",
                dll.display()
            );
            return;
        }
        let _host = host_lock();
        let folder = std::env::temp_dir().join(format!("paintrs-ft-{}", std::process::id()));
        let mut m = PluginManager::new(PluginStore::Native(folder.clone()));
        while !m.ready() {
            m.poll();
        }
        m.install("PaintShopProFiletype.dll", &std::fs::read(&dll).unwrap());
        while m.busy() {
            m.poll();
        }
        assert!(m.failed.is_empty(), "{:?}", m.failed);
        let format = crate::dialogs::save_image::formats()
            .find(|f| f.extensions.contains(&".pspimage"))
            .expect("not in the Save list");
        assert_eq!(format.id, "psp", "its first extension names it");
        assert!(image_accept_has(".pspimage"));
        let choice = format
            .controls
            .iter()
            .find_map(|c| match c {
                pdn_core::effect::Control::Select { key, choices, .. } => Some((*key, choices)),
                _ => None,
            })
            .expect("a save option to pick");
        let mut options = crate::dialogs::save_image::SaveImageDialog::default_options("psp");
        options
            .plugin
            .insert(choice.0.into(), choice.1.last().unwrap().0.into());
        let img = pdn_core::canvas::ImageData {
            width: 7,
            height: 5,
            data: (0..7 * 5 * 4)
                .map(|i: u32| if i % 4 == 3 { 255 } else { (i * 9) as u8 })
                .collect(),
        };
        let bytes = pdn_core::io::encode::encode(&img, "psp", &options).expect("save");
        let detected = pdn_core::io::codecs::detect_format("x.pspimage", None, &bytes);
        assert_eq!(detected, Some("psp"));
        let back = pdn_core::io::codecs::decode("psp", &bytes)
            .expect("open")
            .image;
        assert_eq!(back.data, img.data);
        m.remove("PaintShopProFiletype.dll");
        while m.busy() {
            m.poll();
        }
        assert!(
            pdn_core::io::plugins::formats().is_empty(),
            "still listed after removal"
        );
        let _ = std::fs::remove_dir_all(&folder);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn image_accept_has(ext: &str) -> bool {
        super::super::files::image_accept()
            .split(',')
            .any(|e| e == ext)
    }

    /// The same on Paint.NET's PdnV5EffectSamples ($PDN_V5_SAMPLES_DIR, the net9 build's folder):
    /// the PDN 5 GPU API over opend2d, and the CPU BitmapEffect. PDN_PACK_ONLY=<name> runs one effect;
    /// without it this checks the ones that render (ComputeSharp pixel shaders, ICC colour management,
    /// text outlines and gradient fills through command lists, blurs, scene-position and sample-map
    /// distortions over Paint.NET's own shaders, and the BitmapEffect).
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn pdnv5_samples_load_and_render() {
        let (Ok(_), Ok(pack)) = (
            std::env::var("PDN_BRIDGE_DIR"),
            std::env::var("PDN_V5_SAMPLES_DIR"),
        ) else {
            eprintln!("skipping: set PDN_BRIDGE_DIR (a published bridge) and PDN_V5_SAMPLES_DIR");
            return;
        };
        if let Ok(name) = std::env::var("PDN_PACK_ONLY") {
            render_pack(&pack, &[&name]);
            return;
        }
        let names = [
            "As-if Color Space",
            "Bandlimited Synthesis Shader",
            "Gaussian Bokeh",
            "Iris Blur",
            "Night Circuit Shader",
            "Outlined Text with Shadow",
            "Polarized Mandelleaves",
            "Rainbow",
            "Random Noise",
            "Ripple",
            "Square Blur",
            "Wavey Transform",
            "Zone Plate Shader",
        ];
        let [rendered, ..] = render_pack(&pack, &names);
        for name in names {
            assert!(
                rendered.iter().any(|r| r == name),
                "{name} did not render: {rendered:?}"
            );
        }
    }
}
