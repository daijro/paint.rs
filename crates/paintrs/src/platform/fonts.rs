//! FontManager.js storage and first-use loading. Reference: paintdotjs @ b4e6fc5.
//! R47: system/user fonts are document content; only bundled families can select chrome fonts.
use super::{Files, Pending, PickedFile};
use pdn_core::fonts::{self, FaceStyle, FontPolicy, FontRecord, FontRegistry};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};
pub const METADATA_KEY: &str = "paintdotjs-user-font-metadata-v1";
pub const ACCEPT: &str = ".ttf,.otf,.woff,.woff2";
#[derive(Clone, Default)]
pub struct MemoryFonts {
    records: Vec<FontRecord>,
    bytes: HashMap<String, Vec<u8>>,
    fail: bool,
}
#[derive(Clone)]
pub enum FontStore {
    Memory(Rc<RefCell<MemoryFonts>>),
    #[cfg(not(target_arch = "wasm32"))]
    Native(std::path::PathBuf),
    #[cfg(target_arch = "wasm32")]
    Web(Option<String>),
    Unavailable,
}
pub(super) enum FontChange {
    Add(FontRecord, Vec<u8>),
    Remove(String),
    Clear,
}
pub(super) struct StoreUpdate {
    pub records: Vec<FontRecord>,
    pub removed: usize,
}
impl FontChange {
    pub(super) fn apply(&self, mut records: Vec<FontRecord>) -> Result<StoreUpdate, String> {
        valid_records(&records)?;
        let removed = match self {
            Self::Add(record, bytes) => {
                fonts::validate_font(bytes).map_err(|e| e.to_string())?;
                if records.iter().any(|r| r.id == record.id) {
                    return Err("The font identity is already installed".into());
                }
                records.push(record.clone());
                0
            }
            Self::Remove(id) => {
                let before = records.len();
                records.retain(|r| r.id != *id);
                before - records.len()
            }
            Self::Clear => {
                let count = records.len();
                records.clear();
                count
            }
        };
        valid_records(&records)?;
        Ok(StoreUpdate { records, removed })
    }
}
impl FontStore {
    pub fn memory() -> Self {
        Self::Memory(Rc::default())
    }
    /// A memory store holding `record`, with its bytes unless `None` (a failing read).
    #[cfg(test)]
    pub fn memory_with(record: FontRecord, bytes: Option<Vec<u8>>) -> Self {
        let fonts = MemoryFonts {
            bytes: bytes.map(|b| (record.id.clone(), b)).into_iter().collect(),
            records: vec![record],
            fail: false,
        };
        Self::Memory(Rc::new(RefCell::new(fonts)))
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
    pub fn system(_cache: Option<String>) -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            crate::icon_pack::app_data_dir()
                .map(|p| Self::Native(p.join("fonts")))
                .unwrap_or(Self::Unavailable)
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self::Web(_cache)
        }
    }
    fn load(&self) -> Pending<Result<Vec<FontRecord>, String>> {
        match self {
            Self::Memory(m) => Pending::ready(Ok(m.borrow().records.clone())),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let root = root.clone();
                super::native::background(move || native::load(&root))
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web(cache) => {
                let cache = cache.clone();
                Pending::future(async move { super::font_web::load(cache.as_deref()).await })
            }
            Self::Unavailable => {
                Pending::ready(Err("Persistent font storage is not available".into()))
            }
        }
    }
    fn write(&self, change: FontChange) -> Pending<Result<StoreUpdate, String>> {
        match self {
            Self::Memory(m) => {
                let mut m = m.borrow_mut();
                let result = (|| {
                    if m.fail {
                        return Err("Font storage operation failed".into());
                    }
                    let update = change.apply(m.records.clone())?;
                    match change {
                        FontChange::Add(record, bytes) => {
                            m.bytes.insert(record.id, bytes);
                        }
                        FontChange::Remove(id) => {
                            m.bytes.remove(&id);
                        }
                        FontChange::Clear => m.bytes.clear(),
                    }
                    m.records = update.records.clone();
                    Ok(update)
                })();
                Pending::ready(result)
            }
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let root = root.clone();
                super::native::background(move || native::write(&root, change))
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web(_) => Pending::future(async move { super::font_web::write(change).await }),
            Self::Unavailable => {
                Pending::ready(Err("Persistent font storage is not available".into()))
            }
        }
    }
    fn read(&self, record: FontRecord) -> Pending<Result<Vec<u8>, String>> {
        match self {
            Self::Memory(m) => Pending::ready(
                m.borrow()
                    .bytes
                    .get(&record.id)
                    .cloned()
                    .ok_or("The stored font is missing".into()),
            ),
            #[cfg(not(target_arch = "wasm32"))]
            Self::Native(root) => {
                let root = root.clone();
                super::native::background(move || native::read(&root, &record))
            }
            #[cfg(target_arch = "wasm32")]
            Self::Web(_) => Pending::future(async move { super::font_web::read(&record.id).await }),
            Self::Unavailable => {
                Pending::ready(Err("Persistent font storage is not available".into()))
            }
        }
    }
}
pub fn valid_records(records: &[FontRecord]) -> Result<(), String> {
    let mut ids = HashSet::new();
    let mut families = HashSet::new();
    if records.len() > 10000 {
        return Err("Font metadata is too large".into());
    }
    for r in records {
        if !r.safe_id()
            || r.family.is_empty()
            || !r.family.starts_with("paintjs-")
            || !ids.insert(&r.id)
            || !families.insert(r.family.to_ascii_lowercase())
            || r.size > 64 * 1024 * 1024
        {
            return Err("Invalid font metadata".into());
        }
    }
    Ok(())
}
#[derive(Clone, Default)]
pub struct FontChoices(pub Vec<(String, String)>);
const CHOICES_ID: &str = "paintrs-document-font-choices";
#[derive(Clone, Default)]
struct PreviewRequests(Vec<String>);
#[derive(Clone, Default)]
struct PreviewRequested(HashSet<String>);
/// Visible rows request previews without holding or cloning the manager.
pub fn request_preview(ctx: &egui::Context, family: &str) {
    if fonts::registered_face(family, FaceStyle::Regular).is_some() {
        return;
    }
    let added = ctx.data_mut(|d| {
        if !d
            .get_temp_mut_or_default::<PreviewRequested>(egui::Id::unique("font-preview-requested"))
            .0
            .insert(family.to_ascii_lowercase())
        {
            return false;
        }
        d.get_temp_mut_or_default::<PreviewRequests>(egui::Id::unique("font-preview-requests"))
            .0
            .push(family.into());
        true
    });
    if added {
        ctx.request_repaint();
    }
}
pub fn choices(ctx: &egui::Context) -> Option<Vec<(String, String)>> {
    ctx.data(|d| d.get_temp::<FontChoices>(egui::Id::unique(CHOICES_ID)))
        .map(|c| c.0)
}
enum Job {
    Load,
    Import(PickedFile),
    Remove(String),
    Clear,
    Family(String, FaceStyle),
    Pick(Files),
}
enum Outcome {
    Loaded(Vec<FontRecord>),
    Imported(StoreUpdate, FontRecord, Vec<u8>),
    Removed(StoreUpdate),
    Face(String, FaceStyle, Vec<u8>),
    Picked(Vec<PickedFile>),
}
pub struct FontManager {
    pub registry: FontRegistry,
    pub status: String,
    store: FontStore,
    policy: FontPolicy,
    queue: VecDeque<Job>,
    pending: Option<Pending<Result<Outcome, String>>>,
    pending_import: bool,
    pending_family: Option<(String, FaceStyle)>,
    /// A failed face's retry deadline and its load error.
    retry_at: HashMap<(String, FaceStyle), (u64, String)>,
    initialized: bool,
    failed_load: bool,
    pub revision: u64,
    synced: u64,
    tried: HashSet<(String, FaceStyle)>,
    system: Vec<String>,
    batch_total: usize,
    batch_added: usize,
    batch_errors: Vec<String>,
}
impl FontManager {
    pub fn new(store: FontStore, policy: FontPolicy) -> Self {
        Self {
            registry: FontRegistry::default(),
            status: String::new(),
            store,
            policy,
            queue: VecDeque::from([Job::Load]),
            pending: None,
            pending_import: false,
            pending_family: None,
            retry_at: HashMap::new(),
            initialized: false,
            failed_load: false,
            revision: 1,
            synced: 0,
            tried: HashSet::new(),
            system: vec![],
            batch_total: 0,
            batch_added: 0,
            batch_errors: vec![],
        }
    }
    pub fn busy(&self) -> bool {
        self.pending.is_some() || !self.queue.is_empty()
    }
    pub fn importing(&self) -> bool {
        self.pending_import
            || self
                .queue
                .iter()
                .any(|j| matches!(j, Job::Import(_) | Job::Pick(_)))
    }
    pub fn ready(&self) -> bool {
        self.initialized && !self.failed_load
    }
    pub fn errors(&self) -> &[String] {
        &self.batch_errors
    }
    pub fn progress(&self) -> Option<(usize, usize, &str)> {
        self.importing().then_some((
            self.batch_added + self.batch_errors.len(),
            self.batch_total,
            self.status.as_str(),
        ))
    }
    pub fn import(&mut self, files: Vec<PickedFile>) {
        if self.importing() {
            self.status = "Wait for the current font import to finish.".into();
            return;
        }
        let files: Vec<_> = files
            .into_iter()
            .filter(|f| is_font_name(&f.name))
            .collect();
        if files.is_empty() {
            self.status = "Choose a TTF, OTF, WOFF, or WOFF2 font file.".into();
            return;
        }
        self.batch_total = files.len();
        self.batch_added = 0;
        self.batch_errors.clear();
        self.queue.extend(files.into_iter().map(Job::Import));
    }
    pub fn pick(&mut self, files: &Files) {
        if !self.importing() {
            self.queue.push_back(Job::Pick(files.clone()));
        }
    }
    pub fn remove(&mut self, id: &str) {
        if !self.importing() {
            self.queue.push_back(Job::Remove(id.into()));
        }
    }
    pub fn clear(&mut self) {
        if !self.importing() {
            self.queue.push_back(Job::Clear);
        }
    }
    pub fn ensure_family(&mut self, family: &str, style: FaceStyle) {
        let family = alias(family);
        let key = (family.to_ascii_lowercase(), style);
        if self
            .retry_at
            .get(&key)
            .is_some_and(|&(deadline, _)| now_ms() >= deadline)
        {
            self.retry_at.remove(&key);
            self.tried.remove(&key);
        }
        if !self.tried.insert(key) {
            return;
        }
        self.queue.push_back(Job::Family(family.into(), style));
    }
    /// Only the consumer still needing this face schedules a wakeup. Failed jobs do not run
    /// each frame, and a canceled/closed Text owner does not keep retrying in the background.
    pub fn retry_delay(&self, family: &str, style: FaceStyle) -> Option<std::time::Duration> {
        let family = alias(family);
        self.retry_at
            .get(&(family.to_ascii_lowercase(), style))
            .map(|&(deadline, _)| {
                std::time::Duration::from_millis(deadline.saturating_sub(now_ms()))
            })
    }
    /// A finished, never-retried load failure of this face (tests of R157/R195 consumers).
    #[cfg(test)]
    pub fn fail_family(&mut self, family: &str, style: FaceStyle, error: &str) {
        let key = (alias(family).to_ascii_lowercase(), style);
        self.tried.insert(key.clone());
        self.retry_at.insert(key, (u64::MAX, error.into()));
    }
    /// [`Self::fail_family`] whose 2 s retry wait is over (BB30: a styled face that failed).
    #[cfg(test)]
    pub fn fail_family_retry_due(&mut self, family: &str, style: FaceStyle, error: &str) {
        self.fail_family(family, style, error);
        let key = (alias(family).to_ascii_lowercase(), style);
        self.retry_at.insert(key, (0, error.into()));
    }
    /// R157: the load error of this face's last attempt while no retry is queued or running.
    pub fn family_error(&self, family: &str, style: FaceStyle) -> Option<&str> {
        let family = alias(family);
        self.retry_at
            .get(&(family.to_ascii_lowercase(), style))
            .map(|(_, error)| error.as_str())
    }
    /// R154: whether this face's load is queued or running.
    pub fn loading(&self, family: &str, style: FaceStyle) -> bool {
        let family = alias(family);
        self.pending_family
            .as_ref()
            .is_some_and(|(f, s)| f.eq_ignore_ascii_case(family) && *s == style)
            || self.queue.iter().any(
                |j| matches!(j, Job::Family(f, s) if f.eq_ignore_ascii_case(family) && *s == style),
            )
    }
    pub fn poll(&mut self) {
        // One completion/start per poll keeps import progress and input responsive.
        if let Some(p) = self.pending.as_mut() {
            let Some(result) = p.poll() else { return };
            self.pending = None;
            if let Some(key) = self.pending_family.take() {
                match &result {
                    Err(error) => {
                        let deadline = now_ms().saturating_add(2000);
                        self.retry_at.insert(key, (deadline, error.clone()));
                    }
                    Ok(_) => {
                        self.retry_at.remove(&key);
                    }
                }
            }
            let importing = std::mem::take(&mut self.pending_import);
            match result {
                Ok(Outcome::Loaded(records)) => {
                    self.registry.records = records;
                    self.initialized = true;
                    self.revision += 1;
                }
                Ok(Outcome::Imported(update, record, bytes)) => {
                    let _ = fonts::register_lazy_face(&record.family, FaceStyle::Regular, bytes);
                    self.replace_records(update.records);
                    self.batch_added += 1;
                }
                Ok(Outcome::Removed(update)) => {
                    let removed = update.removed;
                    self.replace_records(update.records);
                    self.status = format!(
                        "Removed {removed} custom font{}.",
                        if removed == 1 { "" } else { "s" }
                    );
                }
                Ok(Outcome::Face(family, style, bytes)) => {
                    if fonts::register_lazy_face(&family, style, bytes).is_ok() {
                        self.revision += 1;
                        self.status.clear();
                    }
                }
                Ok(Outcome::Picked(files)) => {
                    if !files.is_empty() {
                        self.import(files);
                    }
                }
                Err(error) => {
                    if !self.initialized {
                        self.initialized = true;
                        self.failed_load = true;
                    }
                    if importing {
                        self.batch_errors.push(error.clone());
                    }
                    self.status = error;
                }
            }
            if importing && !self.queue.iter().any(|j| matches!(j, Job::Import(_))) {
                self.status = if self.batch_errors.is_empty() {
                    format!(
                        "Added {} font{}.",
                        self.batch_added,
                        if self.batch_added == 1 { "" } else { "s" }
                    )
                } else {
                    format!(
                        "Added {}; {} could not be added.",
                        self.batch_added,
                        self.batch_errors.len()
                    )
                };
            }
        }
        let Some(job) = self.queue.pop_front() else {
            return;
        };
        // A failed user store must prevent metadata writes without disabling bundled/host text.
        if self.failed_load && !matches!(job, Job::Family(_, _)) {
            return;
        }
        let store = self.store.clone();
        let records = self.registry.records.clone();
        self.pending_import = matches!(job, Job::Import(_));
        self.pending_family = match &job {
            Job::Family(family, style) => Some((family.to_ascii_lowercase(), *style)),
            _ => None,
        };
        self.pending = Some(match job {
            Job::Load => {
                let p = store.load();
                Pending::future(async move {
                    let records = p.await?;
                    valid_records(&records)?;
                    Ok(Outcome::Loaded(records))
                })
            }
            Job::Import(file) => {
                self.status = format!(
                    "Adding font {} of {}: {}",
                    self.batch_added + self.batch_errors.len() + 1,
                    self.batch_total,
                    file.name
                );
                Pending::future(async move {
                    let file = file.reread().await?;
                    let now = now_ms();
                    let id = new_id();
                    let record = FontRecord::from_bytes(id, file.name.clone(), &file.bytes, now)
                        .map_err(|e| format!("{}: {e}", file.name))?;
                    let update = store
                        .write(FontChange::Add(record.clone(), file.bytes.to_vec()))
                        .await?;
                    Ok(Outcome::Imported(update, record, file.bytes.to_vec()))
                })
            }
            Job::Remove(id) => Pending::future(async move {
                Ok(Outcome::Removed(store.write(FontChange::Remove(id)).await?))
            }),
            Job::Clear => Pending::future(async move {
                Ok(Outcome::Removed(store.write(FontChange::Clear).await?))
            }),
            Job::Pick(files) => {
                Pending::future(
                    async move { Ok(Outcome::Picked(files.pick_open(true, ACCEPT).await?)) },
                )
            }
            Job::Family(family, style) => {
                if let Some(record) = records
                    .iter()
                    .find(|r| r.family.eq_ignore_ascii_case(&family))
                    .cloned()
                {
                    Pending::future(async move {
                        let bytes = store.read(record).await?;
                        fonts::validate_font(&bytes).map_err(|e| e.to_string())?;
                        Ok(Outcome::Face(family, FaceStyle::Regular, bytes))
                    })
                } else {
                    let policy = self.policy;
                    Pending::future(async move {
                        let bytes = load_family(&family, style, policy).await?;
                        Ok(Outcome::Face(family, style, bytes))
                    })
                }
            }
        });
    }
    fn replace_records(&mut self, records: Vec<FontRecord>) {
        for old in &self.registry.records {
            if !records.iter().any(|r| r.id == old.id) {
                fonts::unregister_family(&old.family);
            }
        }
        self.registry.records = records;
        self.revision += 1;
    }
    pub fn sync(&mut self, ctx: &egui::Context, prefs: &mut crate::prefs::Prefs, ui_family: &str) {
        let requests = ctx
            .data_mut(|d| {
                d.remove_temp::<PreviewRequests>(egui::Id::unique("font-preview-requests"))
            })
            .unwrap_or_default();
        for f in requests.0 {
            self.ensure_family(&f, FaceStyle::Regular);
        }
        if self.synced == self.revision {
            return;
        }
        // Enumeration is document-only; deterministic harnesses never walk OS directories.
        if self.system.is_empty() && self.policy == FontPolicy::System {
            self.system = fonts::system_families(&fonts::os_font_dirs(), self.policy);
        }
        let choices = self.registry.choices(&self.system);
        fonts::set_document_choices(choices.clone());
        ctx.data_mut(|d| d.insert_temp(egui::Id::unique(CHOICES_ID), FontChoices(choices)));
        if self.ready() {
            prefs.set(
                METADATA_KEY,
                &serde_json::to_string(&self.registry.records).expect("font metadata"),
            );
        }
        crate::install_fonts(ctx, FontPolicy::Bundled, ui_family);
        self.synced = self.revision;
    }
}
/// The family a face request loads: Calibri is the bundled Carlito.
fn alias(family: &str) -> &str {
    if family.eq_ignore_ascii_case("Calibri") {
        "Carlito"
    } else {
        family
    }
}
pub fn is_font_name(name: &str) -> bool {
    name.rsplit('.').next().is_some_and(|s| {
        ["ttf", "otf", "woff", "woff2"]
            .iter()
            .any(|ext| s.eq_ignore_ascii_case(ext))
    })
}
fn now_ms() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
}
fn new_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    #[cfg(not(target_arch = "wasm32"))]
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    #[cfg(target_arch = "wasm32")]
    let now = (js_sys::Date::now() * 1000.) as u128;
    format!("{now:x}-{:x}", NEXT.fetch_add(1, Ordering::Relaxed))
}
async fn load_family(
    family: &str,
    style: FaceStyle,
    policy: FontPolicy,
) -> Result<Vec<u8>, String> {
    let resolved = fonts::resolve_family(family);
    let path = if resolved.eq_ignore_ascii_case("Carlito") {
        Some(format!(
            "font/carlito/Carlito-{}.ttf",
            match style {
                FaceStyle::Bold | FaceStyle::Semibold => "Bold",
                FaceStyle::Italic => "Italic",
                FaceStyle::BoldItalic => "BoldItalic",
                _ => "Regular",
            }
        ))
    } else if resolved.eq_ignore_ascii_case("Selawik") {
        return Ok(fonts::SELAWIK_REGULAR.to_vec());
    } else if family.eq_ignore_ascii_case("Anton") {
        Some("fonts/anton/Anton-Regular.ttf".into())
    } else if family.eq_ignore_ascii_case("Noto Color Emoji") {
        Some("fonts/noto-color-emoji/NotoColorEmoji.ttf".into())
    } else if let Some((_, file, _)) = fonts::SCRIPT_FALLBACKS
        .iter()
        .find(|(f, ..)| f.eq_ignore_ascii_case(family))
    {
        Some(format!("fonts/{file}"))
    } else {
        fonts::BUNDLED
            .iter()
            .find(|(f, _)| f.eq_ignore_ascii_case(resolved))
            .map(|(_, p)| format!("font/{p}"))
    };
    if let Some(path) = path {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let bytes = std::fs::read(asset_root().join(path)).map_err(|e| e.to_string())?;
            fonts::validate_font(&bytes).map_err(|e| e.to_string())?;
            return Ok(bytes);
        }
        #[cfg(target_arch = "wasm32")]
        {
            return super::font_web::fetch(&path).await;
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(bytes) =
        fonts::document_face_from(&fonts::system_font_files(policy), policy, family, style)
    {
        // R157: an unusable system face is a load failure (a fallback family may follow), not
        // a silent registration miss that leaves its consumer waiting.
        fonts::validate_font(&bytes).map_err(|e| e.to_string())?;
        return Ok(bytes);
    }
    let _ = policy;
    Err(format!("The font {family} is not available"))
}
#[cfg(not(target_arch = "wasm32"))]
fn asset_root() -> std::path::PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("assets")))
        .filter(|p| p.is_dir())
        .unwrap_or_else(|| {
            std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets"))
        })
}
#[cfg(not(target_arch = "wasm32"))]
mod native {
    use super::*;
    use std::{
        io::{Read, Write},
        path::{Path, PathBuf},
    };
    fn safe(path: &Path) -> Result<(), String> {
        if let Ok(m) = std::fs::symlink_metadata(path)
            && (m.file_type().is_symlink() || !m.is_file())
        {
            return Err("Font storage requires contained regular files".into());
        }
        Ok(())
    }
    fn root_ok(root: &Path) -> Result<(), String> {
        if let Ok(m) = std::fs::symlink_metadata(root)
            && (m.file_type().is_symlink() || !m.is_dir())
        {
            return Err("Invalid font storage directory".into());
        }
        Ok(())
    }
    fn path(root: &Path, r: &FontRecord) -> Result<PathBuf, String> {
        if !r.safe_id() {
            return Err("Invalid font id".into());
        }
        let p = root.join(format!("{}.{}", r.id, r.extension()));
        safe(&p)?;
        Ok(p)
    }
    pub fn load(root: &Path) -> Result<Vec<FontRecord>, String> {
        root_ok(root)?;
        let p = root.join("fonts.json");
        safe(&p)?;
        let file = match std::fs::File::open(p) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(e) => return Err(e.to_string()),
        };
        let mut bytes = vec![];
        file.take(8 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err("Font metadata is too large".into());
        }
        let records: Vec<FontRecord> = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Could not read font metadata: {e}"))?;
        valid_records(&records)?;
        Ok(records)
    }
    pub fn read(root: &Path, r: &FontRecord) -> Result<Vec<u8>, String> {
        root_ok(root)?;
        let mut bytes = vec![];
        std::fs::File::open(path(root, r)?)
            .map_err(|e| e.to_string())?
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        fonts::validate_font(&bytes).map_err(|e| e.to_string())?;
        Ok(bytes)
    }
    pub fn write(root: &Path, change: FontChange) -> Result<StoreUpdate, String> {
        root_ok(root)?;
        std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
        let lock_path = root.join("fonts.lock");
        safe(&lock_path)?;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&lock_path)
            .map_err(|e| e.to_string())?;
        // Keep this inode: locking metadata that gets renamed cannot serialize other instances.
        lock.lock().map_err(|e| e.to_string())?;
        let old = load(root)?;
        let update = change.apply(old.clone())?;
        let metadata = root.join("fonts.json");
        safe(&metadata)?;
        let mut new_blob = None;
        if let FontChange::Add(record, bytes) = change {
            let dest = path(root, &record)?;
            let mut temp = crate::platform::native::atomic_temp(&dest)?;
            temp.write_all(&bytes)
                .and_then(|_| temp.as_file().sync_all())
                .map_err(|e| e.to_string())?;
            temp.persist_noclobber(&dest).map_err(|e| e.to_string())?;
            new_blob = Some(dest);
        }
        let result = (|| {
            let mut temp = crate::platform::native::atomic_temp(&metadata)?;
            temp.write_all(&serde_json::to_vec(&update.records).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
            crate::platform::native::atomic_commit(temp, &metadata)
        })();
        if let Err(error) = result {
            if let Some(path) = new_blob {
                let _ = std::fs::remove_file(path);
            }
            return Err(error);
        }
        // Metadata commits ownership first; cleanup cannot leave a live record without its bytes.
        for record in old
            .iter()
            .filter(|r| !update.records.iter().any(|n| n.id == r.id))
        {
            if let Ok(path) = path(root, record) {
                let _ = std::fs::remove_file(path);
            }
        }
        Ok(update)
    }
}

#[cfg(test)]
mod color_retry_tests {
    use super::*;
    fn drain(m: &mut FontManager) {
        for _ in 0..10000 {
            m.poll();
            if !m.busy() {
                return;
            }
            std::thread::yield_now();
        }
        panic!("font job did not finish");
    }
    #[test]
    fn native_text_first_use_is_need_driven_and_renders_palette() {
        use pdn_core::tool::{MouseButton, PointerInput, ToolType, text::TextTool};
        let mut h = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 800.0))
            .build_eframe(|cc| crate::PaintApp::new_with_policy(cc, FontPolicy::Bundled));
        h.run();
        let key = ("noto color emoji".to_string(), FaceStyle::Regular);
        assert!(
            !h.state().fonts.tried.contains(&key),
            "startup does not request color font"
        );
        h.state_mut().app.set_active_tool_from_type(ToolType::Text);
        h.run();
        assert!(
            !h.state().fonts.tried.contains(&key),
            "empty Text does not request color font"
        );
        h.state_mut().app.with_tool(|tool, app| {
            tool.on_mouse_down(
                app,
                40.0,
                40.0,
                MouseButton::Left,
                &PointerInput {
                    pointer_type: pdn_core::tool::PointerType::Mouse,
                    samples: vec![],
                },
            );
            tool.as_any_mut()
                .downcast_mut::<TextTool>()
                .unwrap()
                .insert_text(app, "Plain text 123");
        });
        h.step();
        assert!(
            !h.state().fonts.tried.contains(&key),
            "ordinary Text stays lazy"
        );
        h.state_mut().app.with_tool(|tool, app| {
            tool.as_any_mut()
                .downcast_mut::<TextTool>()
                .unwrap()
                .insert_text(app, "🎂")
        });
        let history: Vec<_> = h
            .state()
            .app
            .active_ws()
            .unwrap()
            .history
            .undo
            .iter()
            .map(|m| m.id)
            .collect();
        // Native Text and thumbnail workers keep requesting frames. Wait for font
        // completion, not global repaint quiescence; registration redraws Text in
        // the same frame, before the shell is rendered.
        for _ in 0..30 {
            h.step();
            if h.state().fonts.tried.contains(&key) && !h.state().fonts.busy() {
                break;
            }
        }
        assert!(
            h.state().fonts.tried.contains(&key) && !h.state().fonts.busy(),
            "native first-use font loading must complete within 30 frames"
        );
        assert_eq!(
            h.state()
                .app
                .active_ws()
                .unwrap()
                .history
                .undo
                .iter()
                .map(|m| m.id)
                .collect::<Vec<_>>(),
            history
        );
        let pixels = pdn_core::testing::layer_bytes(&h.state().app, None);
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 128 && p[0] != p[1] && p[1] != p[2])
                .count()
                > 15,
            "actual native first-use must draw an intrinsic palette"
        );
    }
    #[test]
    fn failed_color_face_retries_in_same_manager_without_frame_storm() {
        let bytes = include_bytes!("../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        let record =
            FontRecord::from_bytes("task35c-retry".into(), "emoji.ttf".into(), bytes, 0).unwrap();
        let family = record.family.clone();
        let store = FontStore::memory();
        let FontStore::Memory(memory) = &store else {
            unreachable!()
        };
        memory.borrow_mut().records.push(record.clone());
        let mut manager = FontManager::new(store.clone(), FontPolicy::Bundled);
        drain(&mut manager);
        manager.ensure_family(&family, FaceStyle::Regular);
        drain(&mut manager);
        assert!(manager.status.contains("missing"));
        let revision = manager.revision;
        for _ in 0..1000 {
            manager.ensure_family(&family, FaceStyle::Regular);
            assert!(!manager.busy(), "failure must not enqueue every frame");
        }
        memory.borrow_mut().bytes.insert(record.id, bytes.to_vec());
        std::thread::sleep(std::time::Duration::from_millis(2100));
        manager.ensure_family(&family, FaceStyle::Regular);
        for _ in 0..1000 {
            manager.ensure_family(&family, FaceStyle::Regular);
        }
        assert_eq!(
            manager.queue.len(),
            1,
            "one retry, including before its first poll"
        );
        drain(&mut manager);
        assert_eq!(manager.revision, revision + 1);
        assert!(fonts::registered_face(&family, FaceStyle::Regular).is_some());
        manager.ensure_family(&family, FaceStyle::Regular);
        assert!(!manager.busy());
    }
}
