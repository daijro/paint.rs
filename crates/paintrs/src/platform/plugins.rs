//! Installed plug-ins and looks (Settings ▸ Plugins and the store browser): the files the user
//! installed, kept in the app's data folder (desktop, `plugins/`) or the browser's storage (web),
//! and loaded at startup into `pdn_core::extra::plugins::registry` (PhotoCraft `.wasm` filters)
//! and `pdn_core::effect::wasm` (LightCraft `.cube`/`.3dl` looks), which make them Effects ▸
//! Plugins items. A file that no longer loads is reported and skipped, never fatal.
use std::cell::RefCell;
use std::collections::{BTreeMap, VecDeque};
use std::rc::Rc;

use pdn_core::effect::wasm;
use pdn_core::extra::fx::lut;
use pdn_core::extra::plugins::{Limits, Plugin, registry};

use super::{Files, Pending, PickedFile};

/// What the install picker offers.
pub const ACCEPT: &str = ".wasm,.cube,.3dl,.xmp";
/// The largest file installed (PhotoCraft's module limit).
const MAX_BYTES: u64 = 32 << 20;

/// Installed files as (file name, bytes).
type Stored = Vec<(String, Vec<u8>)>;

/// Where the installed files live: by file name, their bytes.
#[derive(Clone)]
pub enum PluginStore {
    Memory(Rc<RefCell<BTreeMap<String, Vec<u8>>>>),
    #[cfg(not(target_arch = "wasm32"))]
    Native(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    Web,
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
        #[cfg(target_arch = "wasm32")]
        {
            Self::Web
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

    fn load(&self) -> Pending<Result<Stored, String>> {
        match self {
            Self::Memory(m) => {
                let files = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                Pending::ready(Ok(files))
            }
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
                        .filter(|e| e.metadata().is_ok_and(|m| m.len() <= MAX_BYTES))
                        .filter_map(|e| {
                            let name = e.file_name().into_string().ok()?;
                            accepted(&name).then(|| Some((name, std::fs::read(e.path()).ok()?)))?
                        })
                        .collect();
                    files.sort();
                    Ok(files)
                })
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web => Pending::future(super::asset_web::plugin_load()),
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
            Self::Web => {
                let (file, bytes) = (file.to_owned(), bytes.to_vec());
                Pending::future(async move { super::asset_web::plugin_put(&file, &bytes).await })
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
            Self::Web => {
                let file = file.to_owned();
                Pending::future(async move { super::asset_web::plugin_remove(&file).await })
            }
            Self::Unavailable => Pending::ready(Ok(())),
        }
    }
}

/// A `.wasm` plug-in, a `.cube`/`.3dl` look or an `.xmp` preset, judged by the extension.
pub fn accepted(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".wasm", ".cube", ".3dl", ".xmp"]
        .iter()
        .any(|e| lower.ends_with(e))
        && !lower.starts_with('.')
}

/// One installed file.
#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    /// The stored file name.
    pub file: String,
    /// The effect it became (`menu.effects.<id>`).
    pub id: String,
    pub name: String,
    /// "Plug-in 1.0 by …" or "Look".
    pub detail: String,
}

/// Loads `bytes` (from a file named `name`) into the app: a plug-in into the registry, a look or a
/// preset into the installed looks. The stored file name is the plug-in's id (so a reinstall
/// replaces it) or the look's sanitised name.
pub fn activate(name: &str, bytes: &[u8]) -> Result<Item, String> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".wasm") {
        let plugin = Plugin::load(bytes, Limits::default()).map_err(|e| format!("{name}: {e}"))?;
        let m = plugin.manifest();
        let by = if m.author.is_empty() {
            String::new()
        } else {
            format!(" by {}", m.author)
        };
        let item = Item {
            file: format!("{}.wasm", m.id),
            id: m.id.clone(),
            name: m.name.clone(),
            detail: format!("Plug-in {}{by}", m.version).replace("  ", " "),
        };
        registry::install(plugin);
        return Ok(item);
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
        return Ok(Item {
            file: format!("{safe}.xmp"),
            id,
            name: stem.to_owned(),
            detail: "Camera Raw preset".into(),
        });
    }
    if lower.ends_with(".cube") || lower.ends_with(".3dl") {
        let table = lut::parse(name, bytes).map_err(|e| format!("{name}: {e}"))?;
        let id = format!("lut.{safe}");
        let title = Some(table.title.trim())
            .filter(|t| !t.is_empty())
            .unwrap_or(stem)
            .to_owned();
        let item = Item {
            file: format!("{safe}.{}", ext.to_ascii_lowercase()),
            id: id.clone(),
            name: title.clone(),
            detail: "Look".into(),
        };
        wasm::install_look(&id, &title, table);
        return Ok(item);
    }
    Err(format!(
        "{name} is not a plug-in (.wasm), a look (.cube, .3dl) or a preset (.xmp)."
    ))
}

fn deactivate(item: &Item) {
    if !wasm::remove_look(&item.id) {
        registry::remove(&item.id);
    }
}

/// The accepted files of a `.zip` bundle (by their base names), within the install limits.
fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::Read;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let (mut out, mut total) = (vec![], 0u64);
    for i in 0..archive.len().min(256) {
        let file = archive.by_index(i).map_err(|e| e.to_string())?;
        let name = file.name().rsplit('/').next().unwrap_or("").to_owned();
        if !file.is_file() || !accepted(&name) || file.size() > MAX_BYTES {
            continue;
        }
        total += file.size();
        if total > 2 * MAX_BYTES {
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
    Loaded(Vec<(String, Vec<u8>)>),
    Picked(Vec<PickedFile>),
    Read(PickedFile),
    Done,
}

pub struct PluginManager {
    store: PluginStore,
    pub items: Vec<Item>,
    /// The last operation's outcome, for the Settings page.
    pub status: String,
    pub errors: Vec<String>,
    ready: bool,
    pending: VecDeque<Pending<Result<Event, String>>>,
}

impl PluginManager {
    pub fn new(store: PluginStore) -> Self {
        let load = store.load();
        Self {
            store,
            items: vec![],
            status: String::new(),
            errors: vec![],
            ready: false,
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

    /// Whether an item with effect id `id` is installed (the store browser's Install button).
    pub fn installed(&self, id: &str) -> bool {
        self.items.iter().any(|i| i.id == id)
    }

    /// Installs and stores a file the user gave or the store browser downloaded; a `.zip` bundle
    /// installs each plug-in, look and preset in it.
    pub fn install(&mut self, name: &str, bytes: &[u8]) {
        if bytes.starts_with(b"PK\x03\x04") {
            match unzip(bytes) {
                Ok(files) if !files.is_empty() => {
                    for (n, b) in files {
                        self.install(&n, &b);
                    }
                }
                Ok(_) => self.fail(format!("{name} holds no plug-ins, looks or presets.")),
                Err(e) => self.fail(format!("{name}: {e}")),
            }
            return;
        }
        match activate(name, bytes) {
            Ok(item) => {
                self.status = format!("Installed {}.", item.name);
                let put = self.store.put(&item.file, bytes);
                self.pending.push_back(Pending::future(
                    async move { put.await.map(|()| Event::Done) },
                ));
                self.items
                    .retain(|i| i.id != item.id && i.file != item.file);
                self.items.push(item);
                self.items.sort_by_key(|i| i.name.to_lowercase());
            }
            Err(e) => self.fail(e),
        }
    }

    fn fail(&mut self, e: String) {
        self.status = e.clone();
        self.errors.push(e);
    }

    /// Uninstalls the item with effect id `id` and deletes its file.
    pub fn remove(&mut self, id: &str) {
        let Some(at) = self.items.iter().position(|i| i.id == id) else {
            return;
        };
        let item = self.items.remove(at);
        deactivate(&item);
        self.status = format!("Removed {}.", item.name);
        let remove = self.store.remove(&item.file);
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
        for file in files.into_iter().filter(|f| accepted(&f.name)) {
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
                    for (name, bytes) in files {
                        match activate(&name, &bytes) {
                            Ok(mut item) => {
                                item.file = name;
                                self.items.push(item);
                            }
                            Err(e) => self.errors.push(e),
                        }
                    }
                    self.items.sort_by_key(|i| i.name.to_lowercase());
                }
                Ok(Event::Picked(files)) => self.import(files),
                Ok(Event::Read(file)) => self.install(&file.name, &file.bytes),
                Ok(Event::Done) => {}
                Err(e) => {
                    self.ready = true;
                    self.status = e.clone();
                    self.errors.push(e);
                }
            }
        }
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INVERT: &[u8] =
        include_bytes!("../../../pdn-core/src/extra/plugins/testdata/invert.wasm");
    const CUBE: &str = "TITLE \"Test Look\"\nLUT_3D_SIZE 2\n0 0 0\n1 0 0\n0 1 0\n1 1 0\n0 0 1\n1 0 1\n0 1 1\n1 1 1\n";

    /// Installing stores the file and registers the effect; a new manager over the same store
    /// loads it back; removing deletes both; a bad file is reported, not stored.
    #[test]
    fn install_load_and_remove() {
        let store = PluginStore::memory();
        let mut m = PluginManager::new(store.clone());
        m.poll();
        m.install("Invert.wasm", INVERT);
        m.install("Film 01.cube", CUBE.as_bytes());
        m.install("notes.wasm", b"\0asm junk");
        m.poll();
        let names: Vec<_> = m
            .items
            .iter()
            .map(|i| (i.file.as_str(), i.name.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("org.photocraft.example.invert.wasm", "Invert (WebAssembly)"),
                ("Film-01.cube", "Test Look")
            ]
        );
        assert_eq!(m.errors.len(), 1);
        assert!(pdn_core::effect::find("org.photocraft.example.invert").is_some());
        assert!(pdn_core::effect::find("lut.Film-01").is_some());

        // A bundle installs what it holds.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        zip.start_file("looks/Warm.cube", opts).unwrap();
        std::io::Write::write_all(&mut zip, CUBE.as_bytes()).unwrap();
        zip.start_file("readme.txt", opts).unwrap();
        let bundle = zip.finish().unwrap().into_inner();
        m.install("pack.zip", &bundle);
        m.poll();
        assert!(m.items.iter().any(|i| i.file == "Warm.cube"));
        m.remove("lut.Warm");

        let mut again = PluginManager::new(store.clone());
        again.poll();
        assert_eq!(again.items, m.items);

        m.remove("lut.Film-01");
        m.remove("org.photocraft.example.invert");
        m.poll();
        assert!(m.items.is_empty());
        assert!(pdn_core::effect::find("lut.Film-01").is_none());
        let PluginStore::Memory(files) = &store else {
            unreachable!()
        };
        assert!(files.borrow().is_empty());
    }
}
