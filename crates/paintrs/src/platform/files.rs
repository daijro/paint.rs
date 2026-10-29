//! Port of: paintdotjs js/src/io/DocumentIO.js; js/platform/desktop/{desktop,main}.js
//! Reference: paintdotjs @ b4e6fc5.
//! File service and UI-thread completions. Port: DocumentIO.js, desktop/desktop.js.
//! Native behavior: file-flows-and-palettes.md; save-and-close-flows.md §§2,8 (R113).
use pdn_core::{
    app::AppWorkspace,
    document::layer::LayerProperties,
    io::{codecs, pdn::PdnDocument},
    settings::AppSettings,
};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    path::PathBuf,
    pin::Pin,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll, Waker},
};

/// Owned operation, polled only on the UI thread. Ready targets need no async executor.
pub struct Pending<T>(Pin<Box<dyn Future<Output = T>>>);
impl<T: 'static> Pending<T> {
    pub fn ready(value: T) -> Self {
        Self(Box::pin(std::future::ready(value)))
    }
    pub fn future(future: impl Future<Output = T> + 'static) -> Self {
        Self(Box::pin(future))
    }
    pub fn poll(&mut self) -> Option<T> {
        match self
            .0
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
        {
            Poll::Ready(v) => Some(v),
            Poll::Pending => None,
        }
    }
}
impl<T> std::future::Future for Pending<T> {
    type Output = T;
    fn poll(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<T> {
        self.0.as_mut().poll(cx)
    }
}
type Completion = Box<dyn FnMut(&mut AppWorkspace, &mut AppSettings, &mut bool) -> bool>;
thread_local! {
    static COMPLETIONS:RefCell<Vec<Completion>>=RefCell::new(vec![]);
    static FILES:RefCell<Option<Files>>=const{RefCell::new(None)};
    static PALETTE_IMPORTS:RefCell<Vec<Vec<PickedFile>>>=const{RefCell::new(vec![])};
}
/// Resume with exclusive UI access; no app borrow is held by a platform future.
pub fn then<T: 'static>(
    mut op: Pending<T>,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    done: impl FnOnce(T, &mut AppWorkspace, &mut AppSettings, &mut bool) + 'static,
) {
    if let Some(value) = op.poll() {
        done(value, app, settings, changed);
        return;
    }
    let mut done = Some(done);
    COMPLETIONS.with(|q| {
        q.borrow_mut().push(Box::new(move |app, settings, changed| {
            let Some(value) = op.poll() else {
                return false;
            };
            done.take().expect("completion runs once")(value, app, settings, changed);
            true
        }))
    });
}
pub fn busy() -> bool {
    COMPLETIONS.with(|q| !q.borrow().is_empty())
}
pub fn poll(
    ctx: &egui::Context,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
) {
    let work = COMPLETIONS.with(|q| std::mem::take(&mut *q.borrow_mut()));
    for mut op in work {
        if !op(app, settings, changed) {
            COMPLETIONS.with(|q| q.borrow_mut().push(op));
        }
    }
    if busy() {
        ctx.request_repaint_after(std::time::Duration::from_millis(10));
    }
}

static NEXT_SOURCE: AtomicU64 = AtomicU64::new(1);
/// Same-session source identity and retained snapshot. Native rereads use the path; handle
/// rereads use getFile; byte-only drops/snapshots replay these bytes. Names are display text.
#[derive(Clone, Debug)]
pub struct PickedFile {
    pub id: u64,
    pub name: String,
    pub mime: Option<String>,
    pub path: Option<PathBuf>,
    pub bytes: Arc<[u8]>,
    pub dropped: Option<egui::DroppedFileHandle>,
    #[cfg(target_arch = "wasm32")]
    pub handle: Option<web_sys::FileSystemFileHandle>,
}
/// The refusal of a file over a read limit, on every platform path.
pub const FILE_TOO_LARGE: &str = "File is too large to open.";
impl PickedFile {
    pub fn snapshot(name: String, bytes: Vec<u8>) -> Self {
        Self {
            id: NEXT_SOURCE.fetch_add(1, Ordering::Relaxed),
            name,
            mime: None,
            path: None,
            bytes: bytes.into(),
            dropped: None,
            #[cfg(target_arch = "wasm32")]
            handle: None,
        }
    }
    pub fn reread(&self) -> Pending<Result<Self, String>> {
        self.reread_within(codecs::MAX_DECODE_BYTES)
    }
    /// [`Self::reread`] refusing a file over `limit` bytes with [`FILE_TOO_LARGE`], on every path:
    /// a native, dropped or browser-handle file's length is checked before its bytes are read,
    /// and a snapshot already in memory by its length.
    pub fn reread_within(&self, limit: u64) -> Pending<Result<Self, String>> {
        if let Some(dropped) = self.dropped.clone() {
            #[cfg(target_arch = "wasm32")]
            if dropped.web_file().is_some_and(|f| f.size() > limit as f64) {
                return Pending::ready(Err(FILE_TOO_LARGE.into()));
            }
            let mut source = self.clone();
            #[cfg(not(target_arch = "wasm32"))]
            return super::native::background(move || {
                let bytes = if source.path.as_ref().is_some_and(|p| p.is_absolute()) {
                    super::native::read_bytes_within(source.path.as_ref().unwrap(), limit)?
                } else {
                    dropped.bytes()?
                };
                if bytes.len() as u64 > limit {
                    return Err(FILE_TOO_LARGE.into());
                }
                source.bytes = bytes.into();
                Ok(source)
            });
            #[cfg(target_arch = "wasm32")]
            return Pending::future(async move {
                let bytes = dropped.bytes_async().await?;
                if bytes.len() as u64 > limit {
                    return Err(FILE_TOO_LARGE.into());
                }
                source.bytes = bytes.into();
                Ok(source)
            });
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(path) = &self.path {
            let mut source = self.clone();
            let path = path.clone();
            return super::native::background(move || {
                source.bytes = super::native::read_bytes_within(&path, limit)?.into();
                Ok(source)
            });
        }
        #[cfg(target_arch = "wasm32")]
        if self.handle.is_some() {
            return super::web_files::reread(self.clone(), limit);
        }
        if self.bytes.len() as u64 > limit {
            return Pending::ready(Err(FILE_TOO_LARGE.into()));
        }
        Pending::ready(Ok(self.clone()))
    }
}
#[derive(Debug, PartialEq, Eq)]
pub enum DropRoute {
    Shapes,
    IconPacks,
    FontsPage,
    ImportFontsThenAsk(Vec<String>),
    Ask(Vec<String>),
}
fn extension(name: &str) -> String {
    name.rsplit_once('.')
        .map_or("", |(_, e)| e)
        .to_ascii_lowercase()
}
pub fn is_font(name: &str) -> bool {
    matches!(extension(name).as_str(), "ttf" | "otf" | "woff" | "woff2")
}
pub fn route_dropped(names: &[String]) -> DropRoute {
    if names.iter().any(|n| extension(n) == "xaml") {
        return DropRoute::Shapes;
    }
    if names.iter().any(|n| extension(n) == "zip") {
        return DropRoute::IconPacks;
    }
    let images: Vec<_> = names.iter().filter(|n| !is_font(n)).cloned().collect();
    if images.len() != names.len() {
        if images.is_empty() {
            DropRoute::FontsPage
        } else {
            DropRoute::ImportFontsThenAsk(images)
        }
    } else {
        DropRoute::Ask(images)
    }
}
/// Tasks51/53/54 consume these payloads, in order; the image remainder is kept separately.
#[derive(Clone, Debug)]
pub struct PendingImport {
    pub page: &'static str,
    pub files: Vec<PickedFile>,
    pub silent: bool,
}
/// One raw drop, kept intact until its category imports and image choice finish.
#[derive(Clone, Debug)]
pub struct DropBatch {
    pub imports: VecDeque<PendingImport>,
    pub images: Vec<PickedFile>,
    pub active: Option<&'static str>,
    pub started: bool,
}
/// The image choice and its selected Open/Add-layers action share one completion boundary.
#[derive(Clone, Debug)]
pub struct DropImageChoice {
    dialog: crate::dialogs::task::TaskDialogHandle,
    done: Rc<Cell<bool>>,
}
impl DropImageChoice {
    pub fn awaiting_choice(&self) -> bool {
        self.dialog.poll().is_none()
    }
    pub fn finished(&self) -> bool {
        self.dialog.poll().is_some() && self.done.get()
    }
}

type PickQueue = Rc<RefCell<VecDeque<Result<Vec<PickedFile>, String>>>>;
/// An in-memory `save_data` offer: the suggested name and the bytes.
type Offered = (String, Vec<u8>);
/// Test-only observation of in-memory operations: their order, and gates that hold the next ones.
#[cfg(test)]
#[derive(Clone, Default)]
pub(crate) struct Probe {
    pub log: Rc<RefCell<Vec<&'static str>>>,
    pub holds: Rc<RefCell<VecDeque<Rc<Cell<bool>>>>>,
}
#[cfg(test)]
impl Probe {
    /// Holds the next in-memory operation until the returned gate is set.
    pub fn hold(&self) -> Rc<Cell<bool>> {
        let gate = Rc::new(Cell::new(false));
        self.holds.borrow_mut().push_back(gate.clone());
        gate
    }
}
#[derive(Clone, Default)]
pub struct Files {
    pub(crate) system: bool,
    pub(crate) state: Rc<RefCell<super::native_or_web::FileState>>,
    pub(crate) picked: PickQueue,
    printed: Rc<RefCell<Vec<Vec<u8>>>>,
    /// In-memory `save_data` outcomes, queued by tests, and what was offered to them.
    saves: Rc<RefCell<VecDeque<Result<FileSaveOutcome, String>>>>,
    saved: Rc<RefCell<Vec<Offered>>>,
    #[cfg(test)]
    pub(crate) probe: Probe,
}
impl Files {
    pub fn system() -> Self {
        Self {
            system: true,
            ..Default::default()
        }
    }
    pub fn in_memory() -> Self {
        Self::default()
    }
    pub fn queue_pick(&self, value: Result<Vec<PickedFile>, String>) {
        self.picked.borrow_mut().push_back(value);
    }
    pub fn current() -> Self {
        FILES.with(|f| f.borrow().clone().unwrap_or_default())
    }
    pub fn install(self) {
        FILES.with(|f| *f.borrow_mut() = Some(self.clone()));
        crate::dialogs::flows::set_save_target(Some(Rc::new(RefCell::new(self))));
    }
    pub fn pick_open(
        &self,
        multiple: bool,
        accept: &str,
    ) -> Pending<Result<Vec<PickedFile>, String>> {
        if !self.system {
            return Pending::ready(self.picked.borrow_mut().pop_front().unwrap_or(Ok(vec![])));
        }
        super::native_or_web::pick_open(multiple, accept)
    }
    /// An in-memory operation, started when no test hold remains in front of it.
    fn memory_op<T: 'static>(
        &self,
        _op: &'static str,
        start: impl FnOnce() -> Pending<T> + 'static,
    ) -> Pending<T> {
        #[cfg(test)]
        {
            self.probe.log.borrow_mut().push(_op);
            if let Some(gate) = self.probe.holds.borrow_mut().pop_front() {
                return Pending::future(async move {
                    std::future::poll_fn(|_| {
                        if gate.get() {
                            std::task::Poll::Ready(())
                        } else {
                            std::task::Poll::Pending
                        }
                    })
                    .await;
                    start().await
                });
            }
        }
        start()
    }
    /// R156 (BB27): one file from `accept`'s ordered filter, plus All files when `all_files`.
    /// The old `choose_open` callers keep their chooser.
    pub fn choose_open_with(
        &self,
        multiple: bool,
        accept: &str,
        all_files: bool,
    ) -> Pending<Result<Vec<PickedFile>, String>> {
        if !self.system {
            let picked = self.picked.clone();
            return self.memory_op("choose", move || {
                Pending::ready(picked.borrow_mut().pop_front().unwrap_or(Ok(vec![])))
            });
        }
        super::native_or_web::choose_open_with(multiple, accept, all_files)
    }
    /// R156 (BB27): a typed native path, read again on every call; `Ok(None)` when it is absent
    /// or a directory. `admit` judges the existing file's name before its bytes are read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn read_existing(
        &self,
        path: PathBuf,
        admit: fn(&str) -> Result<(), String>,
    ) -> Pending<Result<Option<PickedFile>, String>> {
        let work = move || {
            Ok(super::native::read_existing(&path, admit)?.map(|bytes| {
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                let mut file = PickedFile::snapshot(name.into(), bytes);
                file.path = Some(path);
                file
            }))
        };
        if self.system {
            return super::native::background(work);
        }
        self.memory_op("read", move || Pending::ready(work()))
    }
    /// Reusable actual decode service for file flows and the From File effect (BB27).
    pub fn decode(&self, file: PickedFile) -> Pending<Result<PdnDocument, String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            if self.system {
                return super::native::background(move || decode_snapshot(&file));
            }
            self.memory_op("decode", move || Pending::ready(decode_snapshot(&file)))
        }
        #[cfg(target_arch = "wasm32")]
        {
            let format = codecs::detect_format(&file.name, file.mime.as_deref(), &file.bytes);
            if matches!(format, Some("heic" | "avif" | "jxr")) {
                Pending::future(async move {
                    let format = format.unwrap();
                    let image = super::web::decode_async(format, &file.bytes)
                        .await
                        .map_err(|e| e.to_string())?;
                    let dpi = if format == "jxr" {
                        codecs::jxr_dpi(&file.bytes)
                    } else {
                        None
                    };
                    Ok(single_layer(image, dpi))
                })
            } else {
                Pending::ready(decode_snapshot(&file))
            }
        }
    }
    pub fn adopt_open(&self, id: u64, file: &PickedFile, format: &'static str) {
        super::native_or_web::adopt_open(&self.state, id, file, format);
    }
    pub fn take_prints(&self) -> Vec<Vec<u8>> {
        std::mem::take(&mut *self.printed.borrow_mut())
    }
    pub fn print_png(&self, bytes: Vec<u8>) -> Pending<Result<(), String>> {
        if !self.system {
            self.printed.borrow_mut().push(bytes);
            return Pending::ready(Ok(()));
        }
        super::native_or_web::print_png(bytes)
    }
}
fn single_layer(image: pdn_core::canvas::ImageData, dpi: Option<f64>) -> PdnDocument {
    PdnDocument {
        width: image.width,
        height: image.height,
        resolution: dpi.unwrap_or(96.),
        layers: vec![(
            LayerProperties::new(
                &pdn_core::i18n::t("layer.backgroundLayer.defaultName"),
                true,
                true,
                255,
            ),
            image,
        )],
        texts: Vec::new(),
        header_xml: String::new(),
    }
}
fn decode_snapshot(file: &PickedFile) -> Result<PdnDocument, String> {
    let format = codecs::detect_format(&file.name, file.mime.as_deref(), &file.bytes)
        .or_else(|| (extension(&file.name) == "json").then_some("pdn"))
        .ok_or("Unsupported image format")?;
    if format == "pdn" {
        return pdn_core::io::pdn::decode(&file.bytes).map_err(|e| e.to_string());
    }
    if format == "psd" {
        return pdn_core::io::psd::decode(&file.bytes).map_err(|e| e.to_string());
    }
    let decoded = codecs::decode(format, &file.bytes).map_err(|e| e.to_string())?;
    Ok(single_layer(decoded.image, decoded.dpi))
}
pub fn message(title: &str, text: String) {
    crate::dialogs::task::TaskDialog::show(crate::dialogs::task::TaskDialogSpec {
        title: title.into(),
        message: Some(text),
        choices: vec![crate::dialogs::task::TaskChoice {
            value: "ok",
            title: "OK".into(),
            ..Default::default()
        }],
        cancel_value: "ok",
        ..Default::default()
    });
}

pub const IMAGE_ACCEPT: &str = ".pdn,.png,.jpg,.jpeg,.jpe,.webp,.gif,.bmp,.tif,.tiff,.jxl,.avif,.heic,.heif,.dds,.tga,.jxr,.wdp,.wmp,.psd,.psb,.json";
impl Files {
    /// Chooses retained sources before reading bytes, so accepted import commits its tool first.
    pub fn choose_open(
        &self,
        multiple: bool,
        accept: &str,
    ) -> Pending<Result<Vec<PickedFile>, String>> {
        if !self.system {
            return self.pick_open(multiple, accept);
        }
        super::native_or_web::choose_open(multiple, accept)
    }
}
pub fn pick_images(
    import: bool,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
) {
    let op = Files::current().choose_open(true, IMAGE_ACCEPT);
    then(
        op,
        app,
        settings,
        changed,
        move |files, app, settings, changed| match files {
            Ok(files) => open_files(files, import, app, settings, changed),
            Err(e) => message("Open Image", e),
        },
    );
}
struct OpenBatch {
    files: VecDeque<PickedFile>,
    import: Option<pdn_core::io::ops::ImportBatch>,
    done: Option<Rc<Cell<bool>>>,
}
pub fn open_files(
    files: Vec<PickedFile>,
    import: bool,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
) {
    open_files_with_completion(files, import, app, settings, changed, None);
}
fn open_files_with_completion(
    files: Vec<PickedFile>,
    import: bool,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    done: Option<Rc<Cell<bool>>>,
) {
    if files.is_empty() {
        if let Some(done) = done {
            done.set(true);
        }
        return;
    }
    let batch = if import && app.active_ws().is_some() {
        pdn_core::io::ops::ImportBatch::begin(app)
    } else {
        None
    };
    next_file(
        OpenBatch {
            files: files.into(),
            import: batch,
            done,
        },
        app,
        settings,
        changed,
    );
}
fn load_error(
    mut batch: OpenBatch,
    error: String,
    app: &mut AppWorkspace,
    _settings: &mut AppSettings,
    _changed: &mut bool,
) {
    use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogSpec};
    let spec = TaskDialogSpec {
        title: "Open Image".into(),
        message: Some(error),
        choices: vec![TaskChoice {
            value: "ok",
            title: "OK".into(),
            ..Default::default()
        }],
        cancel_value: "ok",
        ..Default::default()
    };
    if let Some(import) = batch.import.take() {
        import.finish(app, false);
        TaskDialog::show_then(spec, move |_, _| {
            if let Some(done) = batch.done {
                done.set(true);
            }
        });
    } else {
        TaskDialog::show_then(spec, move |_, cx| {
            next_file(batch, cx.app, cx.settings, &mut cx.settings_changed)
        });
    }
}
fn next_file(
    mut batch: OpenBatch,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
) {
    let Some(source) = batch.files.pop_front() else {
        if let Some(import) = batch.import {
            import.finish(app, true);
        }
        if let Some(done) = batch.done {
            done.set(true);
        }
        return;
    };
    let op = source.reread();
    then(
        op,
        app,
        settings,
        changed,
        move |read, app, settings, changed| {
            let source = match read {
                Ok(s) => s,
                Err(e) => {
                    load_error(batch, e, app, settings, changed);
                    return;
                }
            };
            let op = Files::current().decode(source.clone());
            then(
                op,
                app,
                settings,
                changed,
                move |decoded, app, settings, changed| match decoded {
                    Err(e) => load_error(
                        batch,
                        format!("Could not open \"{}\": {e}", source.name),
                        app,
                        settings,
                        changed,
                    ),
                    Ok(data) => {
                        if let Some(import) = &mut batch.import {
                            if let Err(e) = import.add(app, &source.name, data.layers) {
                                load_error(
                                    batch,
                                    format!("Could not import \"{}\": {e:?}", source.name),
                                    app,
                                    settings,
                                    changed,
                                );
                                return;
                            }
                        } else {
                            let document = data.to_document();
                            let i = app.create_blank_document_in_new_workspace(
                                document.width,
                                document.height,
                                document.resolution,
                                false,
                            );
                            let ws = &mut app.documents[i];
                            ws.set_document_and_active_layer(document, 0);
                            ws.view.fit_viewport();
                            let format = codecs::detect_format(
                                &source.name,
                                source.mime.as_deref(),
                                &source.bytes,
                            )
                            .unwrap_or("pdn");
                            ws.set_file_info(
                                Some(source.name.clone()),
                                source.path.as_ref().map(|p| p.to_string_lossy().into()),
                                Some(format.into()),
                                serde_json::Value::Null,
                            );
                            ws.set_dirty(false);
                            Files::current().adopt_open(ws.id, &source, format);
                        }
                        next_file(batch, app, settings, changed);
                    }
                },
            );
        },
    );
}
pub fn print_active(app: &mut AppWorkspace, settings: &mut AppSettings, changed: &mut bool) {
    let Some(ws) = app.active_ws() else {
        return;
    };
    let image = ws
        .document
        .composite()
        .get_image_data(0, 0, ws.document.width, ws.document.height);
    let options = pdn_core::io::encode::SaveOptions {
        dpi: ws.document.resolution,
        ..pdn_core::io::encode::SaveOptions::default_for("png")
    };
    match pdn_core::io::encode::encode(&image, "png", &options) {
        Ok(bytes) => then(
            Files::current().print_png(bytes),
            app,
            settings,
            changed,
            |r, _, _, _| {
                if let Err(e) = r {
                    message("Print", e);
                }
            },
        ),
        Err(e) => message("Print", e.to_string()),
    }
}
/// Classify a raw drop without opening a later step before earlier imports finish.
pub fn dropped(files: Vec<PickedFile>) -> DropBatch {
    let mut remaining = files;
    let mut imports = VecDeque::new();
    for (page, predicate) in [
        (
            "shapes",
            (|f: &PickedFile| extension(&f.name) == "xaml") as fn(&PickedFile) -> bool,
        ),
        ("assets", |f| {
            extension(&f.name) == "zip"
                || matches!(
                    f.mime.as_deref(),
                    Some("application/zip" | "application/x-zip-compressed")
                )
        }),
        ("plugins", |f| super::plugins::accepted(&f.name)),
        ("fonts", |f| {
            is_font(&f.name)
                || f.mime
                    .as_ref()
                    .is_some_and(|m| m.to_ascii_lowercase().starts_with("font/"))
        }),
    ] {
        let (matches, rest): (Vec<_>, Vec<_>) = remaining.into_iter().partition(predicate);
        remaining = rest;
        if !matches.is_empty() {
            imports.push_back(PendingImport {
                page,
                files: matches,
                silent: page == "fonts" && !remaining.is_empty(),
            });
        }
    }
    DropBatch {
        imports,
        images: remaining,
        active: None,
        started: false,
    }
}

/// Continue the image remainder only after the last category import settles.
pub fn show_drop_images(remaining: Vec<PickedFile>) -> DropImageChoice {
    assert!(!remaining.is_empty());
    use crate::dialogs::task::{TaskChoice, TaskDialog, TaskDialogSpec};
    let plural = remaining.len() != 1;
    let done = Rc::new(Cell::new(false));
    let action_done = done.clone();
    let dialog = TaskDialog::show_then(
        TaskDialogSpec {
            title: "Drag and Drop".into(),
            icon: Some("drag_drop_open_or_import_form_icon.png"),
            message: Some(format!(
                "What would you like to do with the file{}?",
                if plural { "s" } else { "" }
            )),
            choices: vec![
                TaskChoice {
                    value: "open",
                    title: "Open".into(),
                    description: Some(
                        if plural {
                            "Opens the images."
                        } else {
                            "Opens the image."
                        }
                        .into(),
                    ),
                    icon: Some("menu_file_open_icon.png"),
                },
                TaskChoice {
                    value: "layer",
                    title: if plural { "Add layers" } else { "Add layer" }.into(),
                    description: Some(
                        "Loads the image and adds it as a new layer in the current image.".into(),
                    ),
                    icon: Some("menu_layers_add_new_layer_icon.png"),
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    description: Some("Cancels the action.".into()),
                    icon: Some("menu_edit_undo_icon.png"),
                },
            ],
            cancel_value: "cancel",
            ..Default::default()
        },
        move |choice, cx| {
            if matches!(choice, "open" | "layer") {
                open_files_with_completion(
                    remaining,
                    choice == "layer",
                    cx.app,
                    cx.settings,
                    &mut cx.settings_changed,
                    Some(action_done),
                );
            } else {
                action_done.set(true);
            }
        },
    );
    DropImageChoice { dialog, done }
}

/// R115: external choosers put the default first; in-app FORMATS remains canonical.
pub fn chooser_formats(initial: &str) -> Vec<&'static crate::dialogs::save_image::FormatInfo> {
    let formats = crate::dialogs::save_image::FORMATS;
    formats
        .iter()
        .filter(|f| f.id == initial && !matches!(f.id, "heic" | "jxr"))
        .chain(
            formats
                .iter()
                .filter(|f| f.id != initial && !matches!(f.id, "heic" | "jxr")),
        )
        .collect()
}
pub fn accepted_format(name: &str) -> Result<&'static str, String> {
    crate::dialogs::save_image::FORMATS
        .iter()
        .find(|f| {
            f.extensions
                .iter()
                .any(|e| name.to_ascii_lowercase().ends_with(e))
        })
        .filter(|f| !matches!(f.id, "heic" | "jxr"))
        .map(|f| f.id)
        .ok_or_else(|| "The selected extension cannot be saved.".into())
}

pub fn take_palette_imports() -> Vec<Vec<PickedFile>> {
    PALETTE_IMPORTS.with(|q| std::mem::take(&mut *q.borrow_mut()))
}
#[cfg(target_arch = "wasm32")]
pub fn pick_palette_folder(app: &mut AppWorkspace, settings: &mut AppSettings, changed: &mut bool) {
    let op = super::web_files::palette_files();
    then(op, app, settings, changed, |files, _, _, _| match files {
        Ok(files) => PALETTE_IMPORTS.with(|q| q.borrow_mut().push(files)),
        Err(e) if e == "Canceled" => {}
        Err(e) => message("Open Palettes Folder", e),
    });
}

/// A non-image data file's chooser (BB36 Chart Load/Save Data): its title, one filter's label
/// and extension (no dot). Browsers show neither the title nor, on open, the label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataFile {
    pub title: &'static str,
    pub filter: &'static str,
    pub extension: &'static str,
}

impl Files {
    /// One file through `request`'s chooser; cancel is an empty list. In memory: the next
    /// queued pick, as `choose_open_with`.
    pub fn choose_data(&self, request: DataFile) -> Pending<Result<Vec<PickedFile>, String>> {
        if !self.system {
            let picked = self.picked.clone();
            return self.memory_op("choose", move || {
                Pending::ready(picked.borrow_mut().pop_front().unwrap_or(Ok(vec![])))
            });
        }
        super::native_or_web::choose_data(request)
    }

    /// Writes `bytes` to a file chosen with `request`'s title and filter, `name` suggested; the
    /// filter's extension is added to a chosen name without one. In memory: records the offer
    /// and answers the next queued outcome, else Cancelled.
    pub fn save_data(
        &self,
        request: DataFile,
        name: &str,
        bytes: Vec<u8>,
    ) -> Pending<Result<FileSaveOutcome, String>> {
        if !self.system {
            self.saved.borrow_mut().push((name.into(), bytes));
            let saves = self.saves.clone();
            return self.memory_op("save", move || {
                let next = saves.borrow_mut().pop_front();
                Pending::ready(next.unwrap_or(Ok(FileSaveOutcome::Cancelled)))
            });
        }
        super::native_or_web::save_data(request, name, bytes)
    }

    pub fn queue_save(&self, outcome: Result<FileSaveOutcome, String>) {
        self.saves.borrow_mut().push_back(outcome);
    }

    pub fn take_saved(&self) -> Vec<Offered> {
        std::mem::take(&mut *self.saved.borrow_mut())
    }
}

/// Reusable destination for non-document file consumers such as Chart data (BB36).
#[derive(Clone, Debug)]
pub struct FileTarget {
    pub name: String,
    pub path: Option<PathBuf>,
    #[cfg(target_arch = "wasm32")]
    pub handle: Option<web_sys::FileSystemFileHandle>,
}
#[derive(Debug)]
pub enum FileSaveOutcome {
    Written(FileTarget),
    DownloadStarted,
    Cancelled,
}
impl Files {
    /// Writes supplied bytes; it never changes document dirty state. Export is explicit.
    pub fn save(
        &self,
        name: &str,
        bytes: Vec<u8>,
        existing: Option<FileTarget>,
    ) -> Pending<Result<FileSaveOutcome, String>> {
        if !self.system {
            return Pending::ready(Ok(FileSaveOutcome::Cancelled));
        }
        super::native_or_web::save_bytes(name, bytes, existing)
    }
}
