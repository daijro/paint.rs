//! AssetPackManager.js and ShapeManager.js, paintdotjs b4e6fc5; spec §0.6, R101–R103.
//! Persistent payloads live in platform stores; preferences contain selection and reload dictionaries.
use super::{Files, Pending, PickedFile};
use crate::{
    assets::Assets,
    icon_pack::{IconPackId, SELECTION_KEY},
    prefs::Prefs,
};
use pdn_core::{
    app::AppWorkspace,
    asset_pack::AssetPack,
    tool::xaml::{self, ParsedShape, ShapeRecord},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};
pub const ACTIVE_KEY: &str = "paintdotjs.assetPack.active.v1";
pub const TRANSLATIONS_KEY: &str = "paintdotjs.assetPack.translations.v1";
pub const LOCALES_KEY: &str = "paintdotjs.assetPack.locales.v1";
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Packs,
    Shapes,
}
impl Kind {
    pub fn page(self) -> &'static str {
        match self {
            Self::Packs => "assets",
            Self::Shapes => "shapes",
        }
    }
    pub fn store(self) -> &'static str {
        match self {
            Self::Packs => "packs",
            Self::Shapes => "shapes",
        }
    }
}
fn kind(page: &str) -> Kind {
    if page == "shapes" {
        Kind::Shapes
    } else {
        Kind::Packs
    }
}
pub fn matches(page: &str, name: &str) -> bool {
    name.to_lowercase()
        .ends_with(if page == "shapes" { ".xaml" } else { ".zip" })
}
#[derive(Clone, Default, Deserialize)]
pub struct Snapshot {
    pub packs: Vec<Value>,
    pub shapes: Vec<Value>,
    pub errors: Vec<String>,
}
#[derive(Default)]
pub struct MemoryStore {
    snapshot: Snapshot,
    fail: bool,
}
#[derive(Clone)]
pub enum AssetStore {
    Memory(Rc<RefCell<MemoryStore>>),
    #[cfg(not(target_arch = "wasm32"))]
    Native(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    Web,
    Unavailable,
}
impl AssetStore {
    pub fn memory() -> Self {
        Self::Memory(Rc::default())
    }
    pub fn system() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::icon_pack::app_data_dir()
                .map(Self::Native)
                .unwrap_or(Self::Unavailable)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::Web
        }
    }
    pub fn fail_writes(&self, fail: bool) {
        if let Self::Memory(m) = self {
            m.borrow_mut().fail = fail;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    pub fn native(root: std::path::PathBuf) -> Self {
        Self::Native(root)
    }
    pub fn load(&self) -> Pending<Result<Snapshot, String>> {
        match self {
            Self::Memory(m) => Pending::ready(Ok(m.borrow().snapshot.clone())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let root = root.clone();
                super::native::background(move || native::load(&root))
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web => Pending::future(super::asset_web::load()),
            Self::Unavailable => Pending::ready(Err("Asset storage is unavailable".into())),
        }
    }
    // Ok(warnings) is committed: callers must install the resulting record/catalog state even
    // when physical cleanup or its final durability barrier needs a later retry.
    fn change(&self, k: Kind, change: Change) -> Pending<Result<Vec<String>, String>> {
        match self {
            Self::Memory(m) => {
                let mut m = m.borrow_mut();
                if m.fail {
                    return Pending::ready(Err("Asset storage operation failed".into()));
                }
                let values = if k == Kind::Packs {
                    &mut m.snapshot.packs
                } else {
                    &mut m.snapshot.shapes
                };
                match change {
                    Change::Put(v) => {
                        values.retain(|old| old["id"] != v["id"]);
                        values.push(v)
                    }
                    Change::Remove(id) => values.retain(|v| v["id"].as_str() != Some(&id)),
                    Change::Clear => values.clear(),
                }
                Pending::ready(Ok(vec![]))
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let root = root.clone();
                super::native::background(move || native::change(&root, k, change))
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web => Pending::future(async move {
                super::asset_web::change(k, change).await.map(|()| vec![])
            }),
            Self::Unavailable => Pending::ready(Err("Asset storage is unavailable".into())),
        }
    }
}
pub(super) enum Change {
    Put(Value),
    Remove(String),
    Clear,
}
const MAX_STORAGE_ID_BYTES: usize = 120;
pub fn safe_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_STORAGE_ID_BYTES
        && id != "."
        && id != ".."
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn timestamp_format() -> Vec<time::format_description::BorrowedFormatItem<'static>> {
    time::format_description::parse_borrowed::<2>(
        "[year]-[month]-[day]T[hour]:[minute]:[second].[subsecond digits:3]Z",
    )
    .expect("fixed ISO format")
}
pub fn ms_to_iso(ms: f64) -> Result<String, String> {
    if !ms.is_finite() || ms.fract() != 0. || ms.abs() > 8_640_000_000_000_000. {
        return Err("Invalid Unix millisecond timestamp".into());
    }
    let dt = time::OffsetDateTime::from_unix_timestamp_nanos((ms as i128) * 1_000_000)
        .map_err(|e| e.to_string())?;
    if !(0..=9999).contains(&dt.year()) {
        return Err("Timestamp year is outside the ISO adapter".into());
    }
    dt.format(&timestamp_format()).map_err(|e| e.to_string())
}
pub fn iso_to_ms(iso: &str) -> Result<f64, String> {
    let date = time::PrimitiveDateTime::parse(iso, &timestamp_format())
        .map_err(|e| e.to_string())?
        .assume_utc();
    let ms = (date.unix_timestamp_nanos() / 1_000_000) as f64;
    if ms_to_iso(ms)? != iso {
        return Err("Stored timestamp must use canonical UTC ISO milliseconds".into());
    }
    Ok(ms)
}
fn now_ms() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as f64
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now()
    }
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ShapeWire {
    id: String,
    file_name: String,
    name: String,
    size: u64,
    source: String,
    imported_at: String,
    updated_at: String,
}
fn shape_value(r: &ShapeRecord) -> Result<Value, String> {
    serde_json::to_value(ShapeWire {
        id: r.id.clone(),
        file_name: r.file_name.clone(),
        name: r.name.clone(),
        size: r.size,
        source: r.source.clone(),
        imported_at: ms_to_iso(r.imported_at)?,
        updated_at: ms_to_iso(r.updated_at)?,
    })
    .map_err(|e| e.to_string())
}
fn shape_record(v: Value) -> Result<ShapeRecord, String> {
    let r: ShapeWire = serde_json::from_value(v).map_err(|e| e.to_string())?;
    if !safe_id(&r.id) || r.source.len() > 512 * 1024 || r.size != r.source.len() as u64 {
        return Err("Invalid stored shape identity or source length".into());
    }
    Ok(ShapeRecord {
        id: r.id,
        file_name: r.file_name,
        name: r.name,
        size: r.size,
        source: r.source,
        imported_at: iso_to_ms(&r.imported_at)?,
        updated_at: iso_to_ms(&r.updated_at)?,
    })
}
#[derive(Clone)]
pub struct PackRecord {
    pub id: String,
    pub pack: AssetPack,
    pub imported_at: String,
}
impl PackRecord {
    pub fn name(&self) -> &str {
        self.pack.manifest["name"]
            .as_str()
            .unwrap_or("Icon Pack")
            .trim()
    }
    fn value(&self) -> Value {
        let files=self.pack.files.iter().map(|(path,data)|json!({"path":path,"type":if path.to_ascii_lowercase().ends_with(".webp"){"image/webp"}else{"image/png"},"data":data})).collect::<Vec<_>>();
        let translations = self
            .pack
            .translations
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    serde_json::from_str::<Value>(v).expect("validated dictionary"),
                )
            })
            .collect::<BTreeMap<_, _>>();
        json!({"id":self.id,"name":self.name(),"author":self.pack.manifest.get("author").and_then(Value::as_str).unwrap_or("Unknown"),"version":self.pack.manifest.get("version").and_then(Value::as_str).unwrap_or("1.0.0"),"description":self.pack.manifest.get("description").and_then(Value::as_str).unwrap_or(""),"importedAt":self.imported_at,"iconCount":files.len(),"localeCount":translations.len(),"files":files,"translations":translations,"manifest":self.pack.manifest})
    }
    fn from_value(v: Value) -> Result<Self, String> {
        #[derive(Deserialize)]
        struct File {
            path: String,
            data: Vec<u8>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: String,
            name: String,
            author: String,
            version: String,
            description: String,
            imported_at: String,
            files: Vec<File>,
            translations: BTreeMap<String, Value>,
            manifest: Option<Value>,
        }
        let wire: Wire = serde_json::from_value(v).map_err(|e| e.to_string())?;
        if !safe_id(&wire.id) {
            return Err("Invalid installed pack identity".into());
        }
        iso_to_ms(&wire.imported_at)?;
        let manifest=wire.manifest.unwrap_or_else(||json!({"formatVersion":1,"name":wire.name,"author":wire.author,"version":wire.version,"description":wire.description}));
        let mut files = BTreeMap::new();
        for f in wire.files {
            if files.insert(f.path, f.data).is_some() {
                return Err("Duplicate stored asset path".into());
            }
        }
        let pack = AssetPack {
            id: manifest
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or(&wire.name)
                .into(),
            manifest,
            files,
            translations: wire
                .translations
                .into_iter()
                .map(|(k, v)| (k, v.to_string()))
                .collect(),
        };
        pack.validate().map_err(|e| e.to_string())?;
        Ok(Self {
            id: wire.id,
            pack,
            imported_at: wire.imported_at,
        })
    }
}
enum Event {
    Stored(Box<Event>, Vec<String>),
    Loaded(Snapshot),
    Picked(&'static str, Vec<PickedFile>),
    Pack(PackRecord),
    Shape(ShapeRecord, ParsedShape),
    Removed(Kind, String),
    Cleared,
    ClearFailed(Snapshot, String),
}
pub struct AssetLibrary {
    store: AssetStore,
    /// The installed plug-ins and looks (Settings ▸ Plugins), stored beside the packs.
    pub plugins: super::plugins::PluginManager,
    pub packs: Vec<PackRecord>,
    pub shapes: Vec<ShapeRecord>,
    pub errors: Vec<String>,
    pub status: String,
    pub normalized_language: Option<String>,
    preferences_changed: Cell<bool>,
    active: String,
    ready: bool,
    pending: Option<Pending<Result<Event, String>>>,
    queue: VecDeque<(&'static str, PickedFile)>,
}
impl AssetLibrary {
    pub fn new(store: AssetStore) -> Self {
        let load = store.load();
        let plugins = super::plugins::PluginManager::new(match &store {
            AssetStore::Memory(_) => super::plugins::PluginStore::memory(),
            _ => super::plugins::PluginStore::system(),
        });
        Self {
            plugins,
            store,
            packs: vec![],
            shapes: vec![],
            errors: vec![],
            status: String::new(),
            normalized_language: None,
            preferences_changed: Cell::new(false),
            active: "default".into(),
            ready: false,
            pending: Some(Pending::future(
                async move { load.await.map(Event::Loaded) },
            )),
            queue: VecDeque::new(),
        }
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
    pub fn active_id(&self) -> &str {
        &self.active
    }
    pub fn select_icon(
        &mut self,
        selection: IconPackId,
        ctx: &egui::Context,
        prefs: &mut Prefs,
    ) -> Result<(), String> {
        match &selection {
            IconPackId::User(id) => self.activate(id, ctx, prefs)?,
            _ => {
                // Built-in artwork does not apply an installed ZIP's image or translation paths.
                self.activate("default", ctx, prefs)?;
            }
        }
        Assets::select(ctx, selection.clone());
        prefs.set(SELECTION_KEY, &selection.saved());
        self.preferences_changed.set(true);
        Ok(())
    }
    pub fn locales(&self) -> Vec<String> {
        self.packs
            .iter()
            .find(|p| p.id == self.active)
            .map(|p| p.pack.translations.keys().cloned().collect())
            .unwrap_or_default()
    }
    pub(crate) fn take_preferences_changed(&self) -> bool {
        self.preferences_changed.replace(false)
    }
    /// Task55 calls this before its reload, including when the language changes after activation.
    pub fn prepare_language(&self, prefs: &mut Prefs, preference: &str, os_locale: &str) {
        self.preferences_changed.set(true);
        let Some(pack) = self.packs.iter().find(|p| p.id == self.active) else {
            prefs.remove(TRANSLATIONS_KEY);
            prefs.remove(LOCALES_KEY);
            return;
        };
        let codes = self.locales();
        if codes.is_empty() {
            prefs.remove(TRANSLATIONS_KEY);
            prefs.remove(LOCALES_KEY);
            return;
        }
        prefs.set(
            LOCALES_KEY,
            &serde_json::to_string(&codes).expect("locale strings"),
        );
        let request = if preference == "auto" {
            os_locale
        } else {
            preference
        }
        .replace('_', "-");
        let selected = codes
            .iter()
            .find(|c| c.eq_ignore_ascii_case(&request))
            .or_else(|| {
                codes.iter().find(|c| {
                    c.split('-')
                        .next()
                        .unwrap_or("")
                        .eq_ignore_ascii_case(request.split('-').next().unwrap_or(""))
                })
            });
        let mut dictionaries = BTreeMap::new();
        for code in [Some("en"), selected.map(String::as_str)]
            .into_iter()
            .flatten()
        {
            if let Some(raw) = pack.pack.translations.get(code) {
                dictionaries.insert(
                    code,
                    serde_json::from_str::<Value>(raw).expect("validated dictionary"),
                );
            }
        }
        if dictionaries.is_empty() {
            prefs.remove(TRANSLATIONS_KEY);
        } else {
            prefs.set(
                TRANSLATIONS_KEY,
                &serde_json::to_string(&dictionaries).expect("dictionaries"),
            );
        }
    }
    pub fn activate(
        &mut self,
        id: &str,
        ctx: &egui::Context,
        prefs: &mut Prefs,
    ) -> Result<(), String> {
        let pack = if id == "default" {
            None
        } else {
            Some(
                self.packs
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("The selected icon pack is not installed")?
                    .pack
                    .clone(),
            )
        };
        Assets::set_pack(ctx, pack);
        if id != "default" {
            let selected = IconPackId::User(id.to_owned());
            Assets::select(ctx, selected.clone());
            prefs.set(SELECTION_KEY, &selected.saved());
        }
        self.active = id.into();
        prefs.set(ACTIVE_KEY, id);
        // AssetPackManager normalizes explicit preferences against English plus this pack only.
        let mut settings: Value = prefs
            .get("paintdotjs.settings.v1")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_else(|| json!({}));
        let preference = settings["ui"]["language"]
            .as_str()
            .unwrap_or("auto")
            .to_owned();
        let mut normalized = preference.clone();
        if preference != "auto" {
            let mut codes = vec!["en".into()];
            codes.extend(self.locales());
            normalized = pdn_core::i18n::resolve_language(&preference, "en", &codes);
            if normalized != preference {
                self.normalized_language = Some(normalized.clone());
                settings["ui"]["language"] = json!(normalized);
                prefs.set("paintdotjs.settings.v1", &settings.to_string());
            }
        }
        self.prepare_language(prefs, &normalized, &crate::language::system_locale());
        Ok(())
    }
    pub fn import(&mut self, page: &'static str, files: Vec<PickedFile>) {
        self.queue.extend(
            files
                .into_iter()
                .filter(|f| {
                    matches(page, &f.name)
                        || (page == "assets"
                            && matches!(
                                f.mime.as_deref(),
                                Some("application/zip" | "application/x-zip-compressed")
                            ))
                })
                .map(|f| (page, f)),
        );
    }
    pub fn pick(&mut self, page: &'static str) {
        if self.busy() || !self.ready {
            return;
        }
        let pick = Files::current().choose_open(
            page == "shapes",
            if page == "shapes" { ".xaml" } else { ".zip" },
        );
        self.pending = Some(Pending::future(async move {
            pick.await.map(|files| Event::Picked(page, files))
        }));
    }
    pub fn remove_pack(&mut self, id: &str, ctx: &egui::Context, prefs: &mut Prefs) {
        if self.busy() || id == "default" {
            return;
        }
        if self.active == id {
            let _ = self.activate("default", ctx, prefs);
        }
        if Assets::selected(ctx) == IconPackId::User(id.to_owned()) {
            Assets::select(ctx, IconPackId::PaintRs);
            prefs.set(SELECTION_KEY, "paint-rs");
            self.preferences_changed.set(true);
        }
        self.remove(Kind::Packs, id);
    }
    pub fn remove_shape(&mut self, id: &str) {
        if !self.busy() {
            self.remove(Kind::Shapes, id);
        }
    }
    fn remove(&mut self, k: Kind, id: &str) {
        let id = id.to_owned();
        let write = self.store.change(k, Change::Remove(id.clone()));
        self.pending = Some(Pending::future(async move {
            write
                .await
                .map(|warnings| Event::Stored(Box::new(Event::Removed(k, id)), warnings))
        }));
    }
    pub fn clear_shapes(&mut self) {
        if self.busy() {
            return;
        }
        let write = self.store.change(Kind::Shapes, Change::Clear);
        let store = self.store.clone();
        self.pending = Some(Pending::future(async move {
            match write.await {
                Ok(warnings) => Ok(Event::Stored(Box::new(Event::Cleared), warnings)),
                Err(error) => {
                    // Reconcile precommit failures and storage changes made by other sessions.
                    let snapshot = store.load().await.map_err(|reload| {
                        format!(
                            "Clear failed: {error}; could not reload remaining shapes: {reload}"
                        )
                    })?;
                    Ok(Event::ClearFailed(snapshot, error))
                }
            }
        }));
    }
    pub fn poll(&mut self, ctx: &egui::Context, app: &mut AppWorkspace, prefs: &mut Prefs) {
        if self.plugins.poll() {
            ctx.request_repaint();
        }
        if self.plugins.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(10));
        }
        if let Some(result) = self.pending.as_mut().and_then(Pending::poll) {
            self.pending = None;
            let (result, warnings) = match result {
                Ok(Event::Stored(event, warnings)) => (Ok(*event), warnings),
                other => (other, vec![]),
            };
            match result {
                Ok(Event::Stored(..)) => unreachable!("stored event is unwrapped once"),
                Err(e) => {
                    self.status = format!("Could not complete asset operation: {e}");
                    self.errors.push(e);
                    self.ready = true;
                }
                Ok(Event::Loaded(snapshot)) => {
                    self.errors.extend(snapshot.errors);
                    for v in snapshot.packs {
                        match PackRecord::from_value(v) {
                            Ok(r) => self.packs.push(r),
                            Err(e) => self.errors.push(format!("Icon pack: {e}")),
                        }
                    }
                    self.restore_shapes(snapshot.shapes, app);
                    self.ready = true;
                    let id = prefs.get(ACTIVE_KEY).unwrap_or("default").to_owned();
                    if let Some(pack) = self.packs.iter().find(|p| p.id == id) {
                        self.active = id;
                        Assets::set_pack(ctx, Some(pack.pack.clone()));
                    } else {
                        // External deletion/corruption must repair the saved reload state too.
                        let _ = self.activate("default", ctx, prefs);
                        if matches!(Assets::selected(ctx), IconPackId::User(_)) {
                            Assets::select(ctx, IconPackId::PaintRs);
                            prefs.set(SELECTION_KEY, "paint-rs");
                        }
                    }
                }
                Ok(Event::Picked(page, files)) => self.import(page, files),
                Ok(Event::Pack(record)) => {
                    let id = record.id.clone();
                    let name = record.name().to_owned();
                    self.packs.push(record);
                    if let Err(e) = self.activate(&id, ctx, prefs) {
                        self.errors.push(e);
                    }
                    self.status = format!(
                        "Installed and activated {name}. Reload to apply its translations."
                    );
                }
                Ok(Event::Shape(record, parsed)) => {
                    self.shapes.retain(|r| r.id != record.id);
                    app.shape_catalog.register(record.id.clone(), parsed);
                    self.status = format!("Added {}.", record.file_name);
                    self.shapes.push(record);
                    self.sort_shapes();
                }
                Ok(Event::Removed(k, id)) => {
                    if k == Kind::Packs {
                        self.packs.retain(|p| p.id != id);
                    } else {
                        self.shapes.retain(|r| r.id != id);
                        app.shape_catalog.custom.retain(|(key, _)| *key != id);
                    }
                    self.status = "Removed.".into();
                }
                Ok(Event::ClearFailed(snapshot, error)) => {
                    self.errors.extend(snapshot.errors);
                    self.restore_shapes(snapshot.shapes, app);
                    self.status = format!(
                        "Could not clear all custom shapes: {error}; {} remaining.",
                        self.shapes.len()
                    );
                    self.errors.push(self.status.clone());
                }
                Ok(Event::Cleared) => {
                    for r in self.shapes.drain(..) {
                        app.shape_catalog.custom.retain(|(id, _)| *id != r.id);
                    }
                    self.errors.clear();
                    self.status = if warnings.is_empty() {
                        "Removed all custom shapes.".into()
                    } else {
                        "Cleared all custom shapes; cleanup pending.".into()
                    };
                }
            }
            if !warnings.is_empty() {
                self.status.push(' ');
                self.status.push_str(&warnings.join("; "));
                self.errors.extend(warnings);
            }
        }
        if self.pending.is_none()
            && self.ready
            && let Some((page, file)) = self.queue.pop_front()
        {
            self.status = format!("Installing {}…", file.name);
            let store = self.store.clone();
            let existing = self
                .shapes
                .iter()
                .find(|r| r.file_name.to_lowercase() == file.name.to_lowercase())
                .cloned();
            self.pending = Some(Pending::future(async move {
                let file = read_import(file, page).await?;
                let event = work(move || {
                    if page == "shapes" {
                        if file.bytes.len() > 512 * 1024 {
                            return Err("Shape source exceeds 512 KiB".into());
                        }
                        let source =
                            String::from_utf8(file.bytes.to_vec()).map_err(|e| e.to_string())?;
                        let parsed = xaml::parse(&source).map_err(|e| e.to_string())?;
                        let now = now_ms();
                        let record = ShapeRecord {
                            id: existing
                                .as_ref()
                                .map(|r| r.id.clone())
                                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
                            file_name: file.name,
                            name: parsed.name.clone(),
                            size: source.len() as u64,
                            source,
                            imported_at: existing.as_ref().map_or(now, |r| r.imported_at),
                            updated_at: now,
                        };
                        Ok(Event::Shape(record, parsed))
                    } else {
                        let pack = pdn_core::asset_pack::read_pack(&file.bytes)
                            .map_err(|e| e.to_string())?;
                        let raw = pack.id.to_lowercase();
                        let mut slug = String::new();
                        for c in raw.chars() {
                            if c.is_ascii_alphanumeric() || "._-".contains(c) {
                                slug.push(c);
                            } else if !slug.ends_with('-') {
                                slug.push('-');
                            }
                        }
                        let slug = slug.trim_matches('-').chars().take(48).collect::<String>();
                        Ok(Event::Pack(PackRecord {
                            id: format!(
                                "{}-{}",
                                if slug.is_empty() { "asset-pack" } else { &slug },
                                uuid::Uuid::new_v4()
                            ),
                            pack,
                            imported_at: ms_to_iso(now_ms())?,
                        }))
                    }
                })
                .await?;
                let value = match &event {
                    Event::Pack(r) => r.value(),
                    Event::Shape(r, _) => shape_value(r)?,
                    _ => unreachable!(),
                };
                let warnings = store.change(kind(page), Change::Put(value)).await?;
                Ok(Event::Stored(Box::new(event), warnings))
            }));
        }
        if self.busy() {
            ctx.request_repaint_after(std::time::Duration::from_millis(10));
        }
    }
    fn restore_shapes(&mut self, values: Vec<Value>, app: &mut AppWorkspace) {
        for record in self.shapes.drain(..) {
            app.shape_catalog.custom.retain(|(id, _)| *id != record.id);
        }
        for v in values {
            let label = v["fileName"].as_str().unwrap_or("Shape storage").to_owned();
            match shape_record(v) {
                Ok(mut r) => {
                    match xaml::parse(&r.source) {
                        Ok(parsed) => {
                            r.name = parsed.name.clone();
                            app.shape_catalog.register(r.id.clone(), parsed)
                        }
                        Err(e) => self.errors.push(format!("{label}: {e}")),
                    }
                    self.shapes.push(r)
                }
                Err(e) => self.errors.push(format!("{label}: {e}")),
            }
        }
        self.sort_shapes();
    }
    fn sort_shapes(&mut self) {
        self.shapes.sort_by(|a, b| {
            a.file_name
                .to_lowercase()
                .cmp(&b.file_name.to_lowercase())
                .then_with(|| a.file_name.cmp(&b.file_name))
        });
    }
}
fn read_import(file: PickedFile, page: &str) -> Pending<Result<PickedFile, String>> {
    let limit = if page == "shapes" {
        512 * 1024
    } else {
        pdn_core::asset_pack::MAX_ZIP_BYTES as u64
    };
    if file.bytes.len() as u64 > limit {
        return Pending::ready(Err("File is too large for this asset import".into()));
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(path) = file.path.clone() {
        return super::native::background(move || {
            let mut file = file;
            file.bytes = super::native::read_bytes_within(&path, limit)?.into();
            Ok(file)
        });
    }
    #[cfg(target_arch = "wasm32")]
    if file.handle.is_some() || file.dropped.as_ref().and_then(|d| d.web_file()).is_some() {
        return Pending::future(super::asset_web::read(file, limit));
    }
    Pending::future(async move {
        let file = file.reread().await?;
        if file.bytes.len() as u64 > limit {
            return Err("File is too large for this asset import".into());
        }
        Ok(file)
    })
}
fn work<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Pending<Result<T, String>> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        super::native::background(f)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Pending::future(async move { f() })
    }
}

#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{
        collections::BTreeSet,
        fs,
        io::{BufReader, Read, Write},
        path::Path,
    };
    // Same bounded native wire ceiling as records; no additional imported-shape count quota.
    const MAX_WIRE: u64 = pdn_core::asset_pack::MAX_EXTRACTED * 6 + 1024 * 1024;
    const MARKER: &str = ".cleared-shapes-v1";
    const MARKER_HEADER: &[u8] = b"paintrs-cleared-shapes-v1\n";
    fn directory(root: &Path, k: Kind) -> std::path::PathBuf {
        root.join(k.store())
    }
    fn directory_ok(path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_symlink() || !m.is_dir() => {
                Err("Invalid asset storage directory".into())
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
    fn regular(path: &Path) -> Result<(), String> {
        match fs::symlink_metadata(path) {
            Ok(m) if !m.file_type().is_file() => {
                Err("Asset storage requires contained regular files".into())
            }
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }
    fn shape_lock(root: &Path) -> Result<fs::File, String> {
        let path = root.join("shapes.lock");
        regular(&path)?;
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| e.to_string())?;
        // Keep this inode outside the record directory, as native font storage does.
        lock.lock().map_err(|e| e.to_string())?;
        Ok(lock)
    }
    fn read_marker(dir: &Path) -> Result<BTreeSet<String>, String> {
        let path = dir.join(MARKER);
        regular(&path).map_err(|e| format!("Invalid clear marker: {e}"))?;
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
            Err(e) => return Err(e.to_string()),
        };
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_WIRE {
            return Err("Clear marker exceeds native wire budget".into());
        }
        let mut reader = BufReader::new(file).take(MAX_WIRE + 1);
        let mut header = [0; MARKER_HEADER.len()];
        reader
            .read_exact(&mut header)
            .map_err(|e| format!("Invalid clear marker header: {e}"))?;
        if header != MARKER_HEADER {
            return Err("Unknown clear marker version".into());
        }
        let mut ids = BTreeSet::new();
        let mut line = [0u8; MAX_STORAGE_ID_BYTES];
        let mut length = 0;
        let mut total = MARKER_HEADER.len() as u64;
        // Validate the fixed-length ID buffer before allocating each owned String. No JSON DOM,
        // untrusted capacity hint, escaped ID, or unbounded read_line allocation is involved.
        for byte in reader.bytes() {
            total += 1;
            if total > MAX_WIRE {
                return Err("Clear marker exceeds native wire budget".into());
            }
            let byte = byte.map_err(|e| e.to_string())?;
            if byte == b'\n' {
                let id =
                    std::str::from_utf8(&line[..length]).map_err(|_| "Invalid clear marker ID")?;
                if !safe_id(id) {
                    return Err("Invalid clear marker ID".into());
                }
                if !ids.insert(id.to_owned()) {
                    return Err("Duplicate clear marker ID".into());
                }
                length = 0;
            } else {
                if length == line.len() {
                    return Err("Clear marker ID exceeds storage ID bound".into());
                }
                line[length] = byte;
                length += 1;
            }
        }
        if length != 0 {
            return Err("Truncated clear marker ID".into());
        }
        Ok(ids)
    }
    fn write_marker(dir: &Path, ids: &BTreeSet<String>) -> Result<(), String> {
        let path = dir.join(MARKER);
        regular(&path)?;
        let mut size = MARKER_HEADER.len() as u64;
        for id in ids {
            if !safe_id(id) {
                return Err("Invalid clear marker ID".into());
            }
            size = size
                .checked_add(id.len() as u64 + 1)
                .ok_or("Clear marker size overflow")?;
        }
        if size > MAX_WIRE {
            return Err("Clear marker exceeds native wire budget".into());
        }
        let mut temp = crate::platform::native::atomic_temp(&path)?;
        temp.write_all(MARKER_HEADER).map_err(|e| e.to_string())?;
        for id in ids {
            temp.write_all(id.as_bytes())
                .and_then(|()| temp.write_all(b"\n"))
                .map_err(|e| e.to_string())?;
        }
        crate::platform::native::atomic_commit(temp, &path)
    }
    fn record(path: &Path) -> Result<Value, String> {
        regular(path)?;
        let file = fs::File::open(path).map_err(|e| e.to_string())?;
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_WIRE {
            return Err("Stored asset record exceeds bounded wire size".into());
        }
        let mut bytes = vec![];
        file.take(MAX_WIRE + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() as u64 > MAX_WIRE {
            return Err("Stored asset record exceeds bounded wire size".into());
        }
        let value: Value = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        let id = value["id"].as_str().ok_or("Missing asset identity")?;
        if !safe_id(id) || path.file_name().and_then(|n| n.to_str()) != Some(&format!("{id}.json"))
        {
            return Err("Stored asset filename does not match identity".into());
        }
        Ok(value)
    }
    fn visit_records(
        dir: &Path,
        cleared: &BTreeSet<String>,
        mut visit: impl FnMut(Value),
        errors: &mut Vec<String>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
            let result = (|| {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) != Some("json") {
                    return Ok(None);
                }
                if path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .is_some_and(|id| cleared.contains(id))
                {
                    return Ok(None);
                }
                record(&path).map(Some)
            })();
            match result {
                Ok(Some(v)) => visit(v),
                Ok(None) => {}
                Err(e) => errors.push(e),
            }
        }
        Ok(())
    }
    pub fn load(root: &Path) -> Result<Snapshot, String> {
        directory_ok(root)?;
        let mut out = Snapshot::default();
        for kind in [Kind::Packs, Kind::Shapes] {
            let dir = directory(root, kind);
            directory_ok(&dir)?;
            if !dir.exists() {
                continue;
            }
            let _lock = if kind == Kind::Shapes {
                Some(shape_lock(root)?)
            } else {
                None
            };
            let mut cleared = if kind == Kind::Shapes {
                read_marker(&dir)?
            } else {
                BTreeSet::new()
            };
            if kind == Kind::Shapes {
                out.errors.extend(cleanup(&dir, &mut cleared, &mut Disk));
            }
            let values = if kind == Kind::Packs {
                &mut out.packs
            } else {
                &mut out.shapes
            };
            visit_records(&dir, &cleared, |value| values.push(value), &mut out.errors)?;
        }
        Ok(out)
    }
    trait ShapeIo {
        fn sync_dir(&mut self, dir: &Path) -> Result<(), String> {
            let mut options = fs::OpenOptions::new();
            options.read(true);
            #[cfg(windows)]
            {
                use std::os::windows::fs::OpenOptionsExt;
                // FILE_FLAG_BACKUP_SEMANTICS permits opening a directory handle. If this
                // filesystem cannot flush it, keep payloads and report the limitation.
                options.custom_flags(0x0200_0000);
            }
            options
                .open(dir)
                .and_then(|f| f.sync_all())
                .map_err(|e| e.to_string())
        }
        fn remove(&mut self, path: &Path) -> Result<(), String> {
            fs::remove_file(path).map_err(|e| e.to_string())
        }
        fn marker(&mut self, dir: &Path, ids: &BTreeSet<String>) -> Result<(), String> {
            write_marker(dir, ids)
        }
    }
    struct Disk;
    impl ShapeIo for Disk {}
    fn clear_shapes(
        dir: &Path,
        cleared: BTreeSet<String>,
        io: &mut impl ShapeIo,
    ) -> Result<Vec<String>, String> {
        let mut committed = cleared.clone();
        let mut errors = vec![];
        visit_records(
            dir,
            &cleared,
            |value| {
                if let Ok(record) = shape_record(value) {
                    committed.insert(record.id);
                }
            },
            &mut errors,
        )?;
        // Unreadable/unsafe entries make ownership uncertain: abort before the commit. Parsed
        // but unrecognized JSON is not an owned shape and is left in place.
        if !errors.is_empty() {
            return Err(format!(
                "Could not inspect all shape records: {}",
                errors.join("; ")
            ));
        }
        io.marker(dir, &committed)?; // The one visible logical commit; no payload removed yet.
        Ok(cleanup(dir, &mut committed, io))
    }
    fn cleanup(dir: &Path, cleared: &mut BTreeSet<String>, io: &mut impl ShapeIo) -> Vec<String> {
        let mut warnings = vec![];
        if cleared.is_empty() && !dir.join(MARKER).exists() {
            return warnings;
        }
        let warning = |detail: String| format!("Shape cleanup warning: {detail}");
        // A visible rename alone is not durable. Never unlink until its directory entry is
        // synced. This also covers recovery from an interrupted clear on a later locked load,
        // including an already-visible empty marker whose final prune sync failed.
        if let Err(e) = io.sync_dir(dir) {
            warnings.push(warning(format!(
                "could not sync clear marker directory: {e}"
            )));
            return warnings;
        }
        let mut retained = cleared.clone();
        for id in cleared.iter() {
            let path = dir.join(format!("{id}.json"));
            match fs::symlink_metadata(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    retained.remove(id);
                }
                Err(e) => warnings.push(warning(format!("{id}: {e}"))),
                Ok(_) => match record(&path).and_then(shape_record) {
                    Ok(record) if record.id == *id => match io.remove(&path) {
                        Ok(()) => {
                            retained.remove(id);
                        }
                        Err(e) => warnings.push(warning(format!("could not remove {id}: {e}"))),
                    },
                    _ => warnings.push(warning(format!(
                        "preserved unrecognized or unsafe contents at {id}.json"
                    ))),
                },
            }
        }
        // Keep tombstones until deletions are durable. A crash must never resurrect a deleted
        // record after a newer, pruned marker has become durable.
        if retained != *cleared {
            if let Err(e) = io.sync_dir(dir) {
                warnings.push(warning(format!("could not sync payload deletions: {e}")));
            } else if let Err(e) = io.marker(dir, &retained) {
                warnings.push(warning(format!("could not prune clear marker: {e}")));
            } else if let Err(e) = io.sync_dir(dir) {
                warnings.push(warning(format!("could not sync pruned clear marker: {e}")));
            } else {
                *cleared = retained;
            }
        }
        warnings
    }
    pub fn change(root: &Path, kind: Kind, change: Change) -> Result<Vec<String>, String> {
        change_with_io(root, kind, change, &mut Disk)
    }
    fn change_with_io(
        root: &Path,
        kind: Kind,
        change: Change,
        io: &mut impl ShapeIo,
    ) -> Result<Vec<String>, String> {
        directory_ok(root)?;
        let dir = directory(root, kind);
        directory_ok(&dir)?;
        fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let _lock = if kind == Kind::Shapes {
            Some(shape_lock(root)?)
        } else {
            None
        };
        let mut cleared = if kind == Kind::Shapes {
            read_marker(&dir)?
        } else {
            BTreeSet::new()
        };
        // Same stable lock spans recovery and the following mutation. Cleanup can never race
        // with a later successful Put of the same ID.
        let mut warnings = if kind == Kind::Shapes {
            cleanup(&dir, &mut cleared, io)
        } else {
            vec![]
        };
        match change {
            Change::Put(value) => {
                let id = value["id"]
                    .as_str()
                    .filter(|id| safe_id(id))
                    .ok_or("Invalid asset identity")?;
                let path = dir.join(format!("{id}.json"));
                regular(&path)?;
                let mut temp = crate::platform::native::atomic_temp(&path)?;
                serde_json::to_writer(&mut temp, &value).map_err(|e| e.to_string())?;
                temp.flush().map_err(|e| e.to_string())?;
                crate::platform::native::atomic_commit(temp, &path)?;
                if cleared.remove(id) {
                    io.sync_dir(&dir).map_err(|e| {
                        format!(
                            "Could not make replacement shape durable; clear marker retained: {e}"
                        )
                    })?;
                    io.marker(&dir, &cleared)?;
                    // The unmark is visible now. A failed barrier is a committed-with-warning
                    // outcome, so the manager installs the new record in its live catalog.
                    if let Err(e) = io.sync_dir(&dir) {
                        warnings.push(format!(
                            "Shape stored, but clear marker directory sync failed: {e}"
                        ));
                    }
                }
                Ok(warnings)
            }
            Change::Remove(id) => {
                if !safe_id(&id) {
                    return Err("Invalid asset identity".into());
                }
                let path = dir.join(format!("{id}.json"));
                regular(&path)?;
                match fs::remove_file(path) {
                    Ok(()) => Ok(warnings),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(warnings),
                    Err(e) => Err(e.to_string()),
                }
            }
            Change::Clear => {
                if kind != Kind::Shapes {
                    return Err("Only the shapes store supports Clear All".into());
                }
                warnings.extend(clear_shapes(&dir, cleared, io)?);
                Ok(warnings)
            }
        }
    }

    #[cfg(all(test, not(target_arch = "wasm32")))]
    mod clear_durability_tests {
        use super::*;
        use std::{collections::BTreeSet, path::Path};
        struct FaultIo {
            calls: Vec<&'static str>,
            syncs: usize,
            fail_sync: usize,
            fail_remove: bool,
            fail_prune: bool,
        }
        impl ShapeIo for FaultIo {
            fn sync_dir(&mut self, dir: &Path) -> Result<(), String> {
                self.calls.push("sync");
                self.syncs += 1;
                if self.syncs == self.fail_sync {
                    return Err("injected directory sync failure".into());
                }
                Disk.sync_dir(dir)
            }
            fn remove(&mut self, path: &Path) -> Result<(), String> {
                self.calls.push("unlink");
                if self.fail_remove {
                    return Err("injected unlink failure".into());
                }
                Disk.remove(path)
            }
            fn marker(&mut self, dir: &Path, ids: &BTreeSet<String>) -> Result<(), String> {
                self.calls.push("marker");
                if self.fail_prune && ids.is_empty() {
                    return Err("injected prune failure".into());
                }
                Disk.marker(dir, ids)
            }
        }
        fn fixture() -> (tempfile::TempDir, std::path::PathBuf) {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("original.json");
            std::fs::write(&path, serde_json::to_vec(&json!({"id":"original", "fileName":"Original.xaml", "name":"Original", "size":0, "source":"", "importedAt":"2026-10-20T00:00:00.000Z", "updatedAt":"2026-10-20T00:00:00.000Z"})).unwrap()).unwrap();
            (dir, path)
        }
        fn io(fail_sync: usize) -> FaultIo {
            FaultIo {
                calls: vec![],
                syncs: 0,
                fail_sync,
                fail_remove: false,
                fail_prune: false,
            }
        }
        #[test]
        fn clear_marker_is_synced_before_unlink_and_prune() {
            let (dir, path) = fixture();
            let mut io = io(usize::MAX);
            clear_shapes(dir.path(), BTreeSet::new(), &mut io).unwrap();
            assert_eq!(
                io.calls,
                ["marker", "sync", "unlink", "sync", "marker", "sync"]
            );
            assert!(!path.exists());
        }
        #[test]
        fn clear_failed_first_barrier_keeps_every_payload() {
            let (dir, path) = fixture();
            let original = std::fs::read(&path).unwrap();
            let mut io = io(1);
            let warnings = clear_shapes(dir.path(), BTreeSet::new(), &mut io).unwrap();
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains("clear marker directory"))
            );
            assert_eq!(
                std::fs::read(&path).unwrap(),
                original,
                "postcommit sync failure must not unlink payloads"
            );
            assert!(read_marker(dir.path()).unwrap().contains("original"));
            assert_eq!(io.calls, ["marker", "sync"]);
        }
        #[test]
        fn clear_failed_deletion_barrier_retains_tombstones() {
            let (dir, _) = fixture();
            let mut io = io(2);
            let warnings = clear_shapes(dir.path(), BTreeSet::new(), &mut io).unwrap();
            assert!(warnings.iter().any(|w| w.contains("payload deletions")));
            assert!(
                read_marker(dir.path()).unwrap().contains("original"),
                "a failed deletion barrier must not prune its tombstone"
            );
            assert_eq!(io.calls, ["marker", "sync", "unlink", "sync"]);
        }
        #[test]
        fn clear_unlink_and_prune_failures_warn_and_retry() {
            for unlink in [true, false] {
                let (dir, path) = fixture();
                let mut fault = io(usize::MAX);
                fault.fail_remove = unlink;
                fault.fail_prune = !unlink;
                let warnings = clear_shapes(dir.path(), BTreeSet::new(), &mut fault).unwrap();
                assert!(
                    warnings
                        .iter()
                        .any(|e| e.contains(if unlink { "unlink" } else { "prune" })),
                    "{warnings:?}"
                );
                assert_eq!(path.exists(), unlink);
                let mut cleared = read_marker(dir.path()).unwrap();
                assert!(cleared.contains("original"));
                let mut healthy = Disk;
                assert!(cleanup(dir.path(), &mut cleared, &mut healthy).is_empty());
                assert!(cleared.is_empty());
                assert!(!path.exists());
                assert!(read_marker(dir.path()).unwrap().is_empty());
            }
        }
        #[test]
        fn clear_failed_final_prune_barrier_is_retried_even_with_empty_marker() {
            let (dir, path) = fixture();
            let mut fault = io(3);
            let warnings = clear_shapes(dir.path(), BTreeSet::new(), &mut fault).unwrap();
            assert!(warnings.iter().any(|w| w.contains("pruned clear marker")));
            assert!(!path.exists());
            let mut cleared = read_marker(dir.path()).unwrap();
            assert!(cleared.is_empty(), "the pruned rename is already visible");
            let mut healthy = io(usize::MAX);
            assert!(cleanup(dir.path(), &mut cleared, &mut healthy).is_empty());
            assert_eq!(
                healthy.calls,
                ["sync"],
                "a later locked load/change retries the visible empty marker barrier"
            );
        }
        fn replacement() -> Value {
            let source = r#"<s:SimpleGeometryShape xmlns:s="clr-namespace:PaintDotNet.Shapes;assembly=PaintDotNet.Framework" xmlns="clr-namespace:PaintDotNet.UI.Media;assembly=PaintDotNet.UI" DisplayName="Later" Geometry="M0,0 L10,0 0,10Z"/>"#;
            json!({"id":"original", "fileName":"Same.xaml", "name":"Later", "size":source.len(), "source":source, "importedAt":"2026-10-20T00:00:00.000Z", "updatedAt":"2026-10-21T00:00:00.000Z"})
        }
        fn put_fixture() -> tempfile::TempDir {
            let root = tempfile::tempdir().unwrap();
            let (old, path) = fixture();
            let dir = root.path().join("shapes");
            std::fs::create_dir(&dir).unwrap();
            std::fs::rename(path, dir.join("original.json")).unwrap();
            drop(old);
            root
        }
        #[test]
        fn same_id_put_after_failed_clear_barrier_survives_later_cleanup() {
            let root = put_fixture();
            let dir = root.path().join("shapes");
            let mut failed = io(1);
            let warnings =
                change_with_io(root.path(), Kind::Shapes, Change::Clear, &mut failed).unwrap();
            assert!(!warnings.is_empty());
            assert!(dir.join("original.json").exists());
            change_with_io(
                root.path(),
                Kind::Shapes,
                Change::Put(replacement()),
                &mut Disk,
            )
            .unwrap();
            for _ in 0..2 {
                let snapshot = load(root.path()).unwrap();
                assert_eq!(snapshot.shapes.len(), 1);
                assert_eq!(snapshot.shapes[0]["name"], "Later");
                assert!(snapshot.errors.is_empty());
                assert!(read_marker(&dir).unwrap().is_empty());
            }
        }
        #[test]
        fn same_id_put_after_visible_unmark_sync_failure_updates_live_catalog() {
            let root = put_fixture();
            let dir = root.path().join("shapes");
            write_marker(&dir, &BTreeSet::from(["original".into()])).unwrap();
            let mut fault = io(3); // recovery sync, durable replacement sync, unmark sync
            fault.fail_remove = true; // retain the ID through the initial recovery attempt
            let value = replacement();
            let warnings = change_with_io(
                root.path(),
                Kind::Shapes,
                Change::Put(value.clone()),
                &mut fault,
            )
            .unwrap();
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains("Shape stored") && w.contains("directory sync")),
                "{warnings:?}"
            );
            assert_eq!(fault.calls, ["sync", "unlink", "sync", "marker", "sync"]);
            assert!(
                read_marker(&dir).unwrap().is_empty(),
                "unmark is already visible"
            );
            let snapshot = load(root.path()).unwrap();
            assert_eq!(snapshot.shapes[0]["name"], "Later");
            // The exact committed outcome used by the asynchronous worker must install the new
            // record, rather than leave the old catalog after reporting a misleading failure.
            let record = shape_record(value).unwrap();
            let parsed = xaml::parse(&record.source).unwrap();
            let mut manager = AssetLibrary::new(AssetStore::memory());
            manager.pending = Some(Pending::ready(Ok(Event::Stored(
                Box::new(Event::Shape(record, parsed)),
                warnings,
            ))));
            let ctx = egui::Context::default();
            let mut app = AppWorkspace::default();
            manager.poll(&ctx, &mut app, &mut Prefs::default());
            assert_eq!(manager.shapes[0].name, "Later");
            assert!(app.shape_catalog.get("original").is_some());
            assert!(manager.status.starts_with("Added Same.xaml."));
            assert!(manager.errors.iter().any(|e| e.contains("Shape stored")));
        }
    }
}
