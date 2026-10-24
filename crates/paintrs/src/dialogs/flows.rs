//! Save/Save As, Save All and close continuations. Paint.NET owns transaction/flatten
//! ordering (save-and-close-flows §§2,7; document-ops-and-formats §2.6); paintdotjs
//! DocumentIO supplies dialog copy. Task50 supplies destination selection and file writing.
use super::{
    save_image::{SaveImageDialog, SaveSelection, find_format},
    task::{TaskChoice, TaskDialog, TaskDialogSpec, TaskImages},
};
use pdn_core::{
    app::AppWorkspace,
    io::{
        encode::{self, SaveOptions},
        pdn,
    },
    settings::AppSettings,
    tool::ToolType,
};
use serde_json::{Value, json};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveDestination {
    pub name: String,
    pub path: Option<String>,
    pub format: &'static str,
}
/// The chooser runs before options/flatten. Existing ordinary targets skip it.
pub struct TargetRequest {
    pub workspace_id: u64,
    pub suggested_name: String,
    pub initial_format: &'static str,
    pub existing_path: Option<String>,
}
/// Actual final encoder output, distinct from the dialog's temporary preview encoding.
pub struct SaveRequest {
    pub workspace_id: u64,
    pub destination: SaveDestination,
    pub bytes: Vec<u8>,
}
/// None from choose means cancellation. A completed write alone makes an image clean.
/// write receives already encoded bytes; its error is after the remember/encode boundary,
/// not native pre-encoding temporary-output initialization (save-and-close-flows §8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteOutcome {
    Written,
    DownloadStarted,
}
pub trait SaveTarget {
    fn choose(&mut self, request: &TargetRequest) -> Result<Option<SaveDestination>, String>;
    fn write(&mut self, request: &SaveRequest) -> Result<(), String>;
    fn choose_pending(
        &mut self,
        request: TargetRequest,
    ) -> crate::platform::Pending<Result<Option<SaveDestination>, String>> {
        crate::platform::Pending::ready(self.choose(&request))
    }
    fn prepare_pending(
        &mut self,
        _id: u64,
        _destination: &SaveDestination,
    ) -> crate::platform::Pending<Result<(), String>> {
        crate::platform::Pending::ready(Ok(()))
    }
    fn write_pending(
        &mut self,
        request: SaveRequest,
    ) -> crate::platform::Pending<Result<WriteOutcome, String>> {
        crate::platform::Pending::ready(self.write(&request).map(|()| WriteOutcome::Written))
    }
    fn existing(&self, _id: u64) -> Option<SaveDestination> {
        None
    }
    fn abandon(&mut self, _id: u64) {}
    fn committed(&mut self, _id: u64, _destination: &SaveDestination) {}
}
thread_local! {static TARGET:RefCell<Option<Rc<RefCell<dyn SaveTarget>>>>=RefCell::new(None);}
/// Installs the UI thread's platform target. The unavailable production default cancels early.
pub fn set_save_target(target: Option<Rc<RefCell<dyn SaveTarget>>>) {
    TARGET.with(|t| *t.borrow_mut() = target);
}
#[derive(Clone, Default)]
pub struct SaveHandle(Rc<Cell<Option<bool>>>);
impl SaveHandle {
    pub fn poll(&self) -> Option<bool> {
        self.0.get()
    }
}
type Done = Box<dyn FnOnce(bool, &mut AppWorkspace, &mut AppSettings, &mut bool)>;
fn index(app: &AppWorkspace, id: u64) -> Option<usize> {
    app.documents.iter().position(|w| w.id == id)
}
fn activate(app: &mut AppWorkspace, id: u64) -> bool {
    let Some(i) = index(app, id) else {
        return false;
    };
    if app.active != Some(i) {
        app.set_active_document_workspace(Some(i));
    }
    true
}
fn restore(app: &mut AppWorkspace, id: Option<u64>) {
    if let Some(id) = id {
        activate(app, id);
    }
}
fn commit_for_save(app: &mut AppWorkspace) {
    // Perspective Warp's preview is in the layer: it commits whatever the tool.
    pdn_core::tool::perspective_warp::end(app, true);
    if app.tool_type != Some(ToolType::MoveSelection) {
        app.commit_tool_first(|_| {});
    }
}
pub(super) fn encode_document(
    app: &AppWorkspace,
    id: u64,
    format: &str,
    options: &SaveOptions,
) -> Result<Vec<u8>, String> {
    let ws = index(app, id)
        .map(|i| &app.documents[i])
        .ok_or("The image is no longer open.")?;
    if format == "pdn" {
        return pdn::encode(&ws.document).map_err(|e| e.to_string());
    }
    let mut options = options.clone();
    options.dpi = ws.document.resolution;
    let image = ws
        .document
        .composite()
        .get_image_data(0, 0, ws.document.width, ws.document.height);
    encode::encode(&image, format, &options).map_err(|e| e.to_string())
}
pub(super) fn option_values(options: &SaveOptions) -> Value {
    let mut value = json!({});
    if let Some(q) = options.quality {
        value["quality"] = json!(q);
    }
    if let Some(c) = &options.chroma {
        value["subsampling"] = json!(c);
    }
    value
}
#[derive(Clone)]
struct FileInfo {
    name: Option<String>,
    path: Option<String>,
    format: Option<String>,
    options: Value,
}
impl FileInfo {
    fn capture(app: &AppWorkspace, id: u64) -> Option<Self> {
        let ws = &app.documents[index(app, id)?];
        Some(Self {
            name: ws.file_name.clone(),
            path: ws.file_path.clone(),
            format: ws.file_format.clone(),
            options: ws.save_options.clone(),
        })
    }
    fn install(self, app: &mut AppWorkspace, id: u64) {
        if let Some(i) = index(app, id) {
            app.documents[i].set_file_info(self.name, self.path, self.format, self.options);
        }
    }
}
struct Plan {
    id: u64,
    destination: SaveDestination,
    old: FileInfo,
    target: Rc<RefCell<dyn SaveTarget>>,
}
fn suggested_name(app: &AppWorkspace, id: u64, format: &str) -> String {
    let name = app.documents[index(app, id).expect("open image")].get_friendly_name();
    let stem = if name == pdn_core::i18n::t("untitled.friendlyName") {
        "Untitled"
    } else {
        name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s)
    };
    format!("{stem}{}", find_format(format).extensions[0])
}
fn target_request(app: &AppWorkspace, id: u64, format: &'static str) -> TargetRequest {
    TargetRequest {
        workspace_id: id,
        suggested_name: suggested_name(app, id, format),
        initial_format: format,
        existing_path: app.documents[index(app, id).expect("open image")]
            .file_path
            .clone(),
    }
}
fn error(message: String) {
    TaskDialog::show(TaskDialogSpec {
        title: "Save Image".into(),
        message: Some(message),
        choices: vec![TaskChoice {
            value: "ok",
            title: "OK".into(),
            icon: Some("menu_file_save_icon.png"),
            ..Default::default()
        }],
        cancel_value: "ok",
        ..Default::default()
    });
}
fn finish_write(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    plan: Plan,
    selection: SaveSelection,
    done: Done,
) {
    let op = plan
        .target
        .borrow_mut()
        .prepare_pending(plan.id, &plan.destination);
    crate::platform::files::then(
        op,
        app,
        settings,
        changed,
        move |result, app, settings, changed| {
            if let Err(e) = result {
                fail(plan, e, app, settings, changed, done);
                return;
            }
            if index(app, plan.id).is_none() {
                fail(
                    plan,
                    "The image is no longer open.".into(),
                    app,
                    settings,
                    changed,
                    done,
                );
                return;
            }
            // §8: preparation succeeded. Remember before final encoding, including late failures.
            let mut remembered = settings.get("fileTypes.saveOptions").clone();
            if !remembered.is_object() {
                remembered = json!({});
            }
            remembered[selection.format] = option_values(&selection.options);
            settings.set("fileTypes.saveOptions", remembered);
            *changed = true;
            let bytes = match encode_document(app, plan.id, selection.format, &selection.options) {
                Ok(b) => b,
                Err(e) => {
                    fail(plan, e, app, settings, changed, done);
                    return;
                }
            };
            let op = plan.target.borrow_mut().write_pending(SaveRequest {
                workspace_id: plan.id,
                destination: plan.destination.clone(),
                bytes,
            });
            crate::platform::files::then(
                op,
                app,
                settings,
                changed,
                move |result, app, settings, changed| match result {
                    Ok(WriteOutcome::Written) if index(app, plan.id).is_some() => {
                        plan.target
                            .borrow_mut()
                            .committed(plan.id, &plan.destination);
                        let i = index(app, plan.id).unwrap();
                        app.documents[i].set_dirty(false);
                        done(true, app, settings, changed);
                    }
                    Ok(WriteOutcome::DownloadStarted) => {
                        plan.old.install(app, plan.id);
                        plan.target.borrow_mut().abandon(plan.id);
                        crate::platform::files::message("Download started","Your browser received the download. It cannot confirm that the file was saved, so the image stays open with its current unsaved state.".into());
                        done(false, app, settings, changed);
                    }
                    Ok(_) => fail(
                        plan,
                        "The image is no longer open.".into(),
                        app,
                        settings,
                        changed,
                        done,
                    ),
                    Err(e) => fail(plan, e, app, settings, changed, done),
                },
            );
        },
    );
}
fn fail(
    plan: Plan,
    message: String,
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    done: Done,
) {
    plan.target.borrow_mut().abandon(plan.id);
    plan.old.install(app, plan.id);
    if message != "Canceled" {
        error(message);
    }
    done(false, app, settings, changed);
}

fn selected(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    mut plan: Plan,
    selection: SaveSelection,
    done: Done,
) {
    if !activate(app, plan.id) {
        done(false, app, settings, changed);
        return;
    }
    // This shared paintdotjs dialog can change format after the chooser. Resolve that changed
    // destination before flatten too, so Task50 can validate the extension/overwrite again.
    if selection.format != plan.destination.format {
        let request = target_request(app, plan.id, selection.format);
        let op = plan.target.borrow_mut().choose_pending(request);
        crate::platform::files::then(
            op,
            app,
            settings,
            changed,
            move |chosen, app, settings, changed| match chosen {
                Ok(Some(destination)) if destination.format == selection.format => {
                    plan.destination = destination;
                    selected(app, settings, changed, plan, selection, done);
                }
                Ok(Some(_)) => fail(
                    plan,
                    "The selected file type does not match the save configuration.".into(),
                    app,
                    settings,
                    changed,
                    done,
                ),
                Ok(None) => {
                    plan.target.borrow_mut().abandon(plan.id);
                    done(false, app, settings, changed);
                }
                Err(e) => fail(plan, e, app, settings, changed, done),
            },
        );
        return;
    }

    let valid = super::save_image::FORMATS
        .iter()
        .find(|f| f.id == plan.destination.format)
        .is_some_and(|f| {
            let matches = |name: &str| {
                f.extensions
                    .iter()
                    .any(|e| name.to_ascii_lowercase().ends_with(e))
            };
            matches(&plan.destination.name) && plan.destination.path.as_deref().is_none_or(matches)
        });
    if !valid {
        plan.target.borrow_mut().abandon(plan.id);
        error("The selected file extension does not match the save format.".into());
        done(false, app, settings, changed);
        return;
    }
    let info = FileInfo {
        name: Some(plan.destination.name.clone()),
        path: plan.destination.path.clone(),
        format: Some(selection.format.into()),
        options: option_values(&selection.options),
    };
    info.install(app, plan.id);
    if app.active_ws().expect("open image").document.layers.size() > 1
        && !find_format(selection.format).supports_layers
    {
        let name = find_format(selection.format).name;
        TaskDialog::show_then(
            TaskDialogSpec {
                title: "Flatten Image".into(),
                icon: Some("menu_image_flatten_icon.png"),
                message: Some(format!("{name} cannot preserve layers. Flatten the image after saving?")),
                cancel_value: "cancel",
                choices: vec![
                    TaskChoice {
                        value: "flatten", title: "Flatten and save".into(),
                        description: Some("Saves the composed image. You can undo the flatten operation afterward.".into()),
                        icon: Some("menu_image_flatten_icon.png"),
                    },
                    TaskChoice {
                        value: "cancel", title: "Cancel".into(),
                        description: Some("Returns to the image without saving.".into()),
                        icon: Some("menu_edit_undo_icon.png"),
                    },
                ],
                ..Default::default()
            },
            move |choice, cx| {
                if choice != "flatten" || !activate(cx.app, plan.id) {
                    plan.target.borrow_mut().abandon(plan.id);
                    plan.old.install(cx.app, plan.id);
                    done(false, cx.app, cx.settings, &mut cx.settings_changed);
                    return;
                }
                if !pdn_core::io::ops::flatten_document(cx.app) {
                    plan.target.borrow_mut().abandon(plan.id);
                    plan.old.install(cx.app, plan.id);
                    done(false, cx.app, cx.settings, &mut cx.settings_changed);
                    return;
                }
                finish_write(cx.app, cx.settings, &mut cx.settings_changed, plan, selection, done);
            },
        );
    } else {
        finish_write(app, settings, changed, plan, selection, done);
    }
}
fn begin_save(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    id: u64,
    save_as: bool,
    done: Done,
) {
    if !activate(app, id) {
        done(false, app, settings, changed);
        return;
    }
    commit_for_save(app);
    let Some(target) = TARGET.with(|t| t.borrow().clone()) else {
        done(false, app, settings, changed);
        return;
    };
    let old = FileInfo::capture(app, id).expect("open image");
    let ws = app.active_ws().expect("open image");
    let layered = ws.document.layers.size() > 1;
    let known = old
        .format
        .as_deref()
        .and_then(|id| super::save_image::FORMATS.iter().find(|f| f.id == id))
        .or_else(|| {
            let name = old
                .path
                .as_deref()
                .or(old.name.as_deref())
                .unwrap_or("")
                .to_ascii_lowercase();
            super::save_image::FORMATS
                .iter()
                .find(|f| f.extensions.iter().any(|e| name.ends_with(e)))
        });
    let usable =
        known.filter(|f| !matches!(f.id, "heic" | "jxr") && (!layered || f.supports_layers));
    let existing = target.borrow().existing(id);
    let save_as = save_as || (old.path.is_none() && existing.is_none()) || usable.is_none();
    let format = usable.map_or(if layered { "pdn" } else { "png" }, |f| f.id);
    if save_as {
        let op = target
            .borrow_mut()
            .choose_pending(target_request(app, id, format));
        crate::platform::files::then(
            op,
            app,
            settings,
            changed,
            move |chosen, app, settings, changed| match chosen {
                Ok(Some(destination)) => chosen_destination(
                    app,
                    settings,
                    changed,
                    id,
                    save_as,
                    old,
                    target,
                    destination,
                    done,
                ),
                Ok(None) => {
                    target.borrow_mut().abandon(id);
                    done(false, app, settings, changed);
                }
                Err(e) => {
                    target.borrow_mut().abandon(id);
                    error(e);
                    done(false, app, settings, changed);
                }
            },
        );
    } else {
        let destination = existing
            .filter(|d| d.format == format)
            .unwrap_or(SaveDestination {
                name: old
                    .name
                    .clone()
                    .unwrap_or_else(|| suggested_name(app, id, format)),
                path: old.path.clone(),
                format,
            });
        chosen_destination(
            app,
            settings,
            changed,
            id,
            save_as,
            old,
            target,
            destination,
            done,
        );
    }
}
#[allow(clippy::too_many_arguments)]
fn chosen_destination(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    id: u64,
    save_as: bool,
    old: FileInfo,
    target: Rc<RefCell<dyn SaveTarget>>,
    destination: SaveDestination,
    done: Done,
) {
    if !activate(app, id) {
        target.borrow_mut().abandon(id);
        done(false, app, settings, changed);
        return;
    }
    let initial = if Some(destination.format) == old.format.as_deref() {
        old.options.clone()
    } else {
        Value::Null
    };
    let config = destination.format != "pdn" && (save_as || old.options.is_null());
    let plan = Plan {
        id,
        destination,
        old,
        target,
    };
    if config {
        let format = plan.destination.format;
        super::open(Box::new(SaveImageDialog::new(
            id,
            format,
            initial,
            move |selection, cx| {
                if let Some(selection) = selection {
                    selected(
                        cx.app,
                        cx.settings,
                        &mut cx.settings_changed,
                        plan,
                        selection,
                        done,
                    );
                } else {
                    plan.target.borrow_mut().abandon(plan.id);
                    done(false, cx.app, cx.settings, &mut cx.settings_changed);
                }
            },
        )));
    } else {
        let selection = SaveSelection {
            format: plan.destination.format,
            options: SaveImageDialog::options_from(plan.destination.format, &plan.old.options),
        };
        selected(app, settings, changed, plan, selection, done);
    }
}
pub fn save_active(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    save_as: bool,
) -> SaveHandle {
    let h = SaveHandle::default();
    if crate::platform::files::busy() {
        h.0.set(Some(false));
        return h;
    }
    let result = h.clone();
    if let Some(id) = app.active_ws().map(|w| w.id) {
        begin_save(
            app,
            settings,
            changed,
            id,
            save_as,
            Box::new(move |ok, _, _, _| result.0.set(Some(ok))),
        );
    } else {
        h.0.set(Some(false));
    }
    h
}
fn next_save(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ids: Vec<u64>,
    original: Option<u64>,
    done: Done,
) {
    let Some((&id, rest)) = ids.split_first() else {
        restore(app, original);
        done(true, app, settings, changed);
        return;
    };
    let rest = rest.to_vec();
    if index(app, id).is_none_or(|i| !app.documents[i].dirty) {
        next_save(app, settings, changed, rest, original, done);
        return;
    }
    begin_save(
        app,
        settings,
        changed,
        id,
        false,
        Box::new(move |ok, app, settings, changed| {
            if ok {
                next_save(app, settings, changed, rest, original, done);
            } else {
                done(false, app, settings, changed);
            }
        }),
    );
}
pub fn save_all(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
) -> SaveHandle {
    let ids = app.documents.iter().map(|w| w.id).collect();
    let original = app.active_ws().map(|w| w.id);
    let h = SaveHandle::default();
    if crate::platform::files::busy() {
        h.0.set(Some(false));
        return h;
    }
    let result = h.clone();
    next_save(
        app,
        settings,
        changed,
        ids,
        original,
        Box::new(move |ok, _, _, _| result.0.set(Some(ok))),
    );
    h
}
pub fn close_workspace(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    i: usize,
) {
    close_then(app, settings, changed, i, None);
}
/// Context Close restores its prior image only after success, even after Flatten changes Document.
pub fn close_then(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    i: usize,
    previous: Option<u64>,
) {
    close_with(
        app,
        settings,
        changed,
        i,
        previous,
        Box::new(|_, _, _, _| {}),
    );
}
fn close_with(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    i: usize,
    previous: Option<u64>,
    done: Done,
) {
    let Some(ws) = app.documents.get(i) else {
        done(false, app, settings, changed);
        return;
    };
    let id = ws.id;
    if !ws.dirty {
        app.close_document_workspace(i);
        restore(app, previous);
        done(true, app, settings, changed);
        return;
    }
    activate(app, id);
    let name = app.active_ws().expect("open image").get_friendly_name();
    TaskDialog::show_then(
        TaskDialogSpec {
            title: "Unsaved Changes".into(),
            icon: Some("warning_icon.png"),
            images: Some(TaskImages {
                documents: vec![id],
                explanation: format!("Save changes to \"{name}\" before closing?"),
            }),
            cancel_value: "cancel",
            choices: vec![
                TaskChoice {
                    value: "save",
                    title: "Save".into(),
                    description: Some("Save the image, and then close it.".into()),
                    icon: Some("menu_file_save_icon.png"),
                },
                TaskChoice {
                    value: "discard",
                    title: "Don't Save".into(),
                    description: Some(
                        "Discard the unsaved changes, and then close the image.".into(),
                    ),
                    icon: Some("menu_file_close_icon.png"),
                },
                TaskChoice {
                    value: "cancel",
                    title: "Cancel".into(),
                    description: Some("Keep the image open.".into()),
                    icon: Some("cancel_icon.png"),
                },
            ],
            ..Default::default()
        },
        move |choice, cx| {
            let close: Done = Box::new(move |ok, app, settings, changed| {
                if ok {
                    if let Some(i) = index(app, id) {
                        app.close_document_workspace(i);
                    }
                    restore(app, previous);
                }
                done(ok, app, settings, changed);
            });
            match choice {
                "save" => begin_save(
                    cx.app,
                    cx.settings,
                    &mut cx.settings_changed,
                    id,
                    false,
                    close,
                ),
                "discard" => close(true, cx.app, cx.settings, &mut cx.settings_changed),
                _ => close(false, cx.app, cx.settings, &mut cx.settings_changed),
            }
        },
    );
}
/// Exit uses the multi-image preview and closes each saved image immediately. A later failure
/// preserves earlier closes and restores the original image only if that image remains open.
pub fn exit(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ctx: &egui::Context,
) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        crate::shell::window_close::request(ctx, crate::shell::window_close::Intent::ExitProcess);
        if crate::shell::window_close::begin(ctx) {
            review_close(app, settings, changed, ctx);
        }
    }
    #[cfg(target_arch = "wasm32")]
    exit_impl(app, settings, changed, ctx, false);
}

pub(crate) fn review_close(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ctx: &egui::Context,
) {
    exit_impl(app, settings, changed, ctx, false);
}

#[cfg(target_arch = "wasm32")]
pub fn exit_web(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ctx: &egui::Context,
) {
    exit_impl(app, settings, changed, ctx, true);
}

fn finish_exit(ctx: &egui::Context, ok: bool) {
    #[cfg(not(target_arch = "wasm32"))]
    crate::shell::window_close::finish(ctx, ok);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = ctx;
        if ok {
            crate::platform::web::close_window();
        }
    }
}

fn exit_impl(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ctx: &egui::Context,
    web_guard: bool,
) {
    let ids: Vec<_> = app
        .documents
        .iter()
        .filter(|w| w.dirty)
        .map(|w| w.id)
        .collect();
    if ids.is_empty() {
        while !app.documents.is_empty() {
            app.close_document_workspace(0);
        }
        finish_exit(ctx, true);
        return;
    }
    let original = app.active_ws().map(|w| w.id);
    let ctx = ctx.clone();
    if ids.len() == 1 && !web_guard {
        let i = index(app, ids[0]).expect("open image");
        close_with(
            app,
            settings,
            changed,
            i,
            None,
            Box::new(move |ok, app, _settings, _changed| {
                if ok {
                    while !app.documents.is_empty() {
                        app.close_document_workspace(0);
                    }
                    finish_exit(&ctx, true);
                } else {
                    restore(app, original);
                    finish_exit(&ctx, false);
                }
            }),
        );
        return;
    }
    TaskDialog::show_then(
        TaskDialogSpec {
            title: "Unsaved Changes".into(),
            icon: Some("warning_icon.png"),
            images: Some(TaskImages {
                documents: ids.clone(),
                explanation: "The following images have changes that have not been saved. Select a thumbnail to show that image in the main window.".into(),
            }),
            cancel_value: "cancel",
            choices: vec![
                TaskChoice { value: "save", title: "Save".into(),
                    description: Some("Save the images listed above, and then exit.".into()),
                    icon: Some("menu_file_save_all_icon.png"), },
                TaskChoice { value: "discard", title: "Don't Save".into(),
                    description: Some(if web_guard { format!("Discard all unsaved changes, and then exit {}.", super::about::PRODUCT) } else { "Discard all unsaved changes, and then exit.".into() }),
                    icon: Some("menu_file_close_icon.png"), },
                TaskChoice { value: "cancel", title: "Cancel".into(),
                    description: Some(format!("Go back to {}.", super::about::PRODUCT)),
                    icon: Some("cancel_icon.png"), },
            ],
            ..Default::default()
        },
        move |choice, cx| match choice {
            "save" => close_saved(cx.app, cx.settings, &mut cx.settings_changed, ids, original, ctx),
            "discard" => {
                while !cx.app.documents.is_empty() { cx.app.close_document_workspace(0); }
                finish_exit(&ctx, true);
            },
            _ => { restore(cx.app, original); finish_exit(&ctx, false); },
        },
    );
}
fn close_saved(
    app: &mut AppWorkspace,
    settings: &mut AppSettings,
    changed: &mut bool,
    ids: Vec<u64>,
    original: Option<u64>,
    ctx: egui::Context,
) {
    let Some((&id, rest)) = ids.split_first() else {
        while !app.documents.is_empty() {
            app.close_document_workspace(0);
        }
        finish_exit(&ctx, true);
        return;
    };
    let rest = rest.to_vec();
    begin_save(
        app,
        settings,
        changed,
        id,
        false,
        Box::new(move |ok, app, settings, changed| {
            if ok {
                if let Some(i) = index(app, id) {
                    app.close_document_workspace(i);
                }
                close_saved(app, settings, changed, rest, original, ctx);
            } else {
                restore(app, original);
                finish_exit(&ctx, false);
            }
        }),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdn_core::testing::{Ev, drive_tool, fresh_doc};
    use pdn_core::tool::MouseButton::Left;
    #[derive(Default)]
    struct MemoryTarget {
        writes: Vec<SaveRequest>,
        choices: Vec<&'static str>,
        cancel: bool,
        fail_at: Option<usize>,
        first_format: Option<&'static str>,
    }
    impl SaveTarget for MemoryTarget {
        fn choose(&mut self, r: &TargetRequest) -> Result<Option<SaveDestination>, String> {
            self.choices.push(r.initial_format);
            let format = self.first_format.take().unwrap_or(r.initial_format);
            let stem = r
                .suggested_name
                .rsplit_once('.')
                .map_or(r.suggested_name.as_str(), |(s, _)| s);
            let name = format!("{stem}{}", find_format(format).extensions[0]);
            Ok((!self.cancel).then(|| SaveDestination {
                name: name.clone(),
                path: Some(format!("/{name}")),
                format,
            }))
        }
        fn write(&mut self, r: &SaveRequest) -> Result<(), String> {
            self.writes.push(SaveRequest {
                workspace_id: r.workspace_id,
                destination: r.destination.clone(),
                bytes: r.bytes.clone(),
            });
            if self.fail_at == Some(self.writes.len()) {
                Err("disk full".into())
            } else {
                Ok(())
            }
        }
    }
    fn compatible(app: &mut AppWorkspace, i: usize) {
        app.documents[i].set_file_info(
            Some(format!("image{i}.png")),
            Some(format!("/image{i}.png")),
            Some("png".into()),
            json!({}),
        );
    }
    #[test]
    fn save_outcome_controls_dirty_file_info_and_actual_bytes() {
        for fail in [false, true] {
            let mut app = fresh_doc(2, 1, None);
            compatible(&mut app, 0);
            app.active_ws_mut()
                .unwrap()
                .document
                .layers
                .get_at_mut(0)
                .unwrap()
                .surface
                .data_mut()
                .copy_from_slice(&[20, 40, 60, 255, 80, 100, 120, 255]);
            app.active_ws_mut().unwrap().set_dirty(true);
            let target = Rc::new(RefCell::new(MemoryTarget {
                fail_at: fail.then_some(1),
                ..Default::default()
            }));
            set_save_target(Some(target.clone()));
            assert_eq!(
                save_active(&mut app, &mut AppSettings::load(None), &mut false, false).poll(),
                Some(!fail)
            );
            let t = target.borrow();
            assert!(t.choices.is_empty());
            assert_eq!(t.writes.len(), 1);
            let decoded = pdn_core::io::codecs::decode("png", &t.writes[0].bytes).unwrap();
            assert_eq!(decoded.image.data, [20, 40, 60, 255, 80, 100, 120, 255]);
            assert_eq!(app.active_ws().unwrap().dirty, fail);
            assert_eq!(
                app.active_ws().unwrap().file_path.as_deref(),
                Some("/image0.png")
            );
        }
        set_save_target(None);
    }
    fn start_text_edit(app: &mut AppWorkspace, text: &str, color: pdn_core::util::color::Color) {
        app.set_colors(color, app.secondary_color);
        // These cover raster text, which Editable text (on by default) would replace.
        app.tool_settings
            .set(ToolType::Text, "editableText", false.into());
        drive_tool(
            app,
            ToolType::Text,
            &[Ev::Down(20.0, 20.0, Left), Ev::Up(20.0, 20.0, Left)],
        );
        pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(app, |t, app| {
            t.insert_text(app, text);
            assert!(t.is_editing());
        });
    }
    fn text_history_identity(
        ws: &pdn_core::document::workspace::DocumentWorkspace,
    ) -> Vec<(u64, String, String)> {
        assert_eq!(ws.history.undo.len(), 2);
        assert!(matches!(
            ws.history.undo[0].kind,
            pdn_core::history::MementoKind::Null
        ));
        ws.history
            .undo
            .iter()
            .map(|m| (m.id, m.name.clone(), m.image.clone()))
            .collect()
    }
    fn text_compound_children(ws: &pdn_core::document::workspace::DocumentWorkspace) -> usize {
        let last = ws.history.undo.last().unwrap();
        assert_eq!(last.name, "Text");
        let pdn_core::history::MementoKind::Compound(children) = &last.kind else {
            panic!("Text must keep its early compound");
        };
        children.len()
    }
    #[test]
    fn text_save_canceled_chooser_keeps_committed_text_pixels_and_history() {
        use pdn_core::{
            action::{ActionOutcome, UiRequest, perform},
            util::color::Color,
        };
        for force_as in [false, true] {
            let mut app = fresh_doc(120, 80, None);
            let blank = app
                .active_ws()
                .unwrap()
                .document
                .composite()
                .data()
                .to_vec();
            start_text_edit(&mut app, "Accepted", Color::new(180, 20, 40, 255));
            let pixels = app
                .active_ws()
                .unwrap()
                .document
                .composite()
                .data()
                .to_vec();
            assert_ne!(pixels, blank);
            let history = text_history_identity(app.active_ws().unwrap());
            assert_eq!(text_compound_children(app.active_ws().unwrap()), 0);
            assert!(app.active_ws().unwrap().dirty);
            let target = Rc::new(RefCell::new(MemoryTarget {
                cancel: true,
                ..Default::default()
            }));
            set_save_target(Some(target.clone()));
            let (command, request) = if force_as {
                ("menu.file.saveAs", UiRequest::SaveAs)
            } else {
                ("menu.file.save", UiRequest::Save)
            };
            assert_eq!(perform(&mut app, command), ActionOutcome::Ui(request));
            // Action dispatch commits before the chooser is even entered.
            pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(&mut app, |t, _| {
                assert!(!t.is_editing())
            });
            assert_eq!(text_compound_children(app.active_ws().unwrap()), 1);
            assert!(target.borrow().choices.is_empty());
            assert_eq!(
                save_active(&mut app, &mut AppSettings::load(None), &mut false, force_as).poll(),
                Some(false)
            );
            let ws = app.active_ws().unwrap();
            assert_eq!(ws.document.composite().data(), pixels);
            assert_eq!(text_history_identity(ws), history);
            assert_eq!(text_compound_children(ws), 1);
            assert!(ws.history.redo.is_empty());
            assert!(ws.dirty);
            assert!(ws.file_path.is_none());
            assert_eq!(target.borrow().choices, ["png"]);
            assert!(target.borrow().writes.is_empty());
            assert!(app.undo());
            assert_eq!(app.active_ws().unwrap().document.composite().data(), blank);
            assert!(app.redo());
            assert_eq!(app.active_ws().unwrap().document.composite().data(), pixels);
            set_save_target(None);
        }
    }
    #[test]
    fn text_save_all_commits_each_parked_owner_and_encodes_its_pixels() {
        use pdn_core::util::color::Color;
        let mut app = fresh_doc(120, 80, None);
        app.documents[0].is_initial_workspace = false;
        compatible(&mut app, 0);
        start_text_edit(&mut app, "A", Color::new(210, 15, 35, 255));
        let a = app.active_ws().unwrap().id;
        app.create_blank_document_in_new_workspace(160, 90, 96.0, false);
        compatible(&mut app, 1);
        // Shared color stays live; vary image/text identity without changing global settings.
        start_text_edit(&mut app, "BB", Color::new(210, 15, 35, 255));
        let b = app.active_ws().unwrap().id;
        assert_ne!(a, b);
        assert!(
            app.documents
                .iter()
                .find(|w| w.id == a)
                .unwrap()
                .parked_tool
                .is_some()
        );
        let expected: Vec<_> = app
            .documents
            .iter()
            .map(|w| {
                assert!(w.dirty);
                assert_eq!(text_compound_children(w), 0);
                let image = w.document.composite();
                (
                    w.id,
                    image.width(),
                    image.height(),
                    image.data().to_vec(),
                    text_history_identity(w),
                )
            })
            .collect();
        assert_ne!(expected[0].3, expected[1].3);
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_all(&mut app, &mut AppSettings::load(None), &mut false).poll(),
            Some(true)
        );
        assert_eq!(
            app.active_ws().unwrap().id,
            b,
            "success restores the original workspace"
        );
        let saved = target.borrow();
        assert!(saved.choices.is_empty());
        assert_eq!(saved.writes.len(), 2);
        for (id, width, height, pixels, history) in expected {
            let writes: Vec<_> = saved
                .writes
                .iter()
                .filter(|r| r.workspace_id == id)
                .collect();
            assert_eq!(writes.len(), 1);
            let decoded = pdn_core::io::codecs::decode("png", &writes[0].bytes)
                .unwrap()
                .image;
            assert_eq!((decoded.width, decoded.height), (width, height));
            assert_eq!(
                decoded.data, pixels,
                "encoded pixels belong to workspace {id}"
            );
            let ws = app.documents.iter().find(|w| w.id == id).unwrap();
            assert_eq!(ws.document.composite().data(), pixels);
            assert_eq!(text_history_identity(ws), history);
            assert_eq!(text_compound_children(ws), 1);
            assert!(!ws.dirty);
            assert!(ws.parked_tool.is_none());
        }
        pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(&mut app, |t, _| {
            assert!(!t.is_editing())
        });
        set_save_target(None);
    }
    #[test]
    fn text_save_dont_save_drops_only_closing_edit_and_resumes_survivor() {
        use pdn_core::util::color::Color;
        let mut driver = DialogDriver::new();
        let mut app = fresh_doc(120, 80, None);
        app.documents[0].is_initial_workspace = false;
        start_text_edit(&mut app, "Closing", Color::new(210, 15, 35, 255));
        let closing = app.active_ws().unwrap().id;
        let closing_pixels = app
            .active_ws()
            .unwrap()
            .document
            .composite()
            .data()
            .to_vec();
        app.create_blank_document_in_new_workspace(120, 80, 96.0, false);
        start_text_edit(&mut app, "Survivor", Color::new(20, 40, 220, 255));
        let survivor = app.active_ws().unwrap().id;
        let pixels = app
            .active_ws()
            .unwrap()
            .document
            .composite()
            .data()
            .to_vec();
        let history = text_history_identity(app.active_ws().unwrap());
        assert_ne!(pixels, closing_pixels);
        assert_eq!(text_compound_children(app.active_ws().unwrap()), 0);
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        set_save_target(Some(target.clone()));
        let closing_index = app.documents.iter().position(|w| w.id == closing).unwrap();
        close_then(
            &mut app,
            &mut driver.settings,
            &mut false,
            closing_index,
            Some(survivor),
        );
        driver.show(&mut app);
        assert!(driver.stack.is_open());
        assert_eq!(app.active_ws().unwrap().id, closing);
        assert_eq!(text_compound_children(app.active_ws().unwrap()), 0);
        let ws = app.documents.iter_mut().find(|w| w.id == survivor).unwrap();
        assert!(
            ws.parked_tool
                .as_mut()
                .unwrap()
                .as_any_mut()
                .downcast_mut::<pdn_core::tool::text::TextTool>()
                .unwrap()
                .is_editing()
        );
        // Close button -> thumbnail -> Save -> Don't Save, through the real dialog controls.
        for _ in 0..3 {
            driver.key(&mut app, egui::Key::Tab);
        }
        driver.key(&mut app, egui::Key::Enter);
        assert!(!driver.stack.is_open());
        assert_eq!(app.documents.len(), 1);
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.id, survivor);
        assert_eq!(ws.document.composite().data(), pixels);
        assert_eq!(text_history_identity(ws), history);
        assert_eq!(
            text_compound_children(ws),
            0,
            "survivor remains an open Text compound"
        );
        assert!(ws.history.redo.is_empty());
        assert!(ws.dirty);
        assert!(target.borrow().choices.is_empty());
        assert!(target.borrow().writes.is_empty());
        pdn_core::testing::with_tool::<pdn_core::tool::text::TextTool>(&mut app, |t, app| {
            assert!(t.is_editing());
            t.insert_text(app, "!");
        });
        assert_eq!(text_history_identity(app.active_ws().unwrap()), history);
        assert_eq!(text_compound_children(app.active_ws().unwrap()), 0);
        assert_ne!(app.active_ws().unwrap().document.composite().data(), pixels);
        set_save_target(None);
    }
    #[test]
    fn save_all_commits_parked_gradients_and_restores_original_on_success() {
        let mut app = fresh_doc(4, 2, None);
        app.documents[0].is_initial_workspace = false;
        app.create_blank_document_in_new_workspace(4, 2, 96.0, false);
        for i in 0..2 {
            compatible(&mut app, i);
            app.set_active_document_workspace(Some(i));
            app.secondary_color = pdn_core::util::color::Color::new(10 + i as u8, 20, 30, 255);
            drive_tool(
                &mut app,
                ToolType::Gradient,
                &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
            );
        }
        let original = app.active_ws().unwrap().id;
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_all(&mut app, &mut AppSettings::load(None), &mut false).poll(),
            Some(true)
        );
        assert_eq!(app.active_ws().unwrap().id, original);
        let t = target.borrow();
        assert_eq!(t.writes.len(), 2);
        for (i, request) in t.writes.iter().enumerate() {
            let image = pdn_core::io::codecs::decode("png", &request.bytes)
                .unwrap()
                .image;
            assert_eq!(&image.data[..4], &[10 + i as u8, 20, 30, 255]);
            app.set_active_document_workspace(Some(i));
            assert_eq!(
                app.active_ws().unwrap().history.undo.last().unwrap().name,
                "Finish Gradient"
            );
            assert!(!app.active_ws().unwrap().dirty);
            assert!(app.undo());
            assert!(app.redo());
        }
        set_save_target(None);
    }
    #[test]
    fn save_all_failure_keeps_failed_image_active_and_later_images_unattempted() {
        let mut app = fresh_doc(2, 1, None);
        app.documents[0].is_initial_workspace = false;
        for _ in 0..2 {
            app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
        }
        for i in 0..3 {
            compatible(&mut app, i);
            app.documents[i].set_dirty(true);
        }
        app.set_active_document_workspace(Some(2));
        let target = Rc::new(RefCell::new(MemoryTarget {
            fail_at: Some(2),
            ..Default::default()
        }));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_all(&mut app, &mut AppSettings::load(None), &mut false).poll(),
            Some(false)
        );
        assert_eq!(app.active, Some(1));
        assert_eq!(
            app.documents.iter().map(|w| w.dirty).collect::<Vec<_>>(),
            [false, true, true]
        );
        assert_eq!(target.borrow().writes.len(), 2);
        set_save_target(None);
    }
    #[test]
    fn chooser_cancel_keeps_prior_tool_commit_but_never_flattens() {
        let mut app = fresh_doc(4, 2, None);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[Ev::Down(1.0, 1.0, Left), Ev::Up(1.0, 1.0, Left)],
        );
        let target = Rc::new(RefCell::new(MemoryTarget {
            cancel: true,
            ..Default::default()
        }));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_active(&mut app, &mut AppSettings::load(None), &mut false, true).poll(),
            Some(false)
        );
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Finish Gradient"
        );
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 1);
        assert!(target.borrow().writes.is_empty());
        assert_eq!(target.borrow().choices, ["png"]);
        set_save_target(None);
    }
    #[test]
    fn unavailable_target_never_claims_save_and_clean_failed_save_stays_clean() {
        let mut app = fresh_doc(2, 1, None);
        set_save_target(None);
        assert_eq!(
            save_active(&mut app, &mut AppSettings::load(None), &mut false, true).poll(),
            Some(false)
        );
        assert!(app.active_ws().unwrap().file_path.is_none());
        compatible(&mut app, 0);
        let target = Rc::new(RefCell::new(MemoryTarget {
            fail_at: Some(1),
            ..Default::default()
        }));
        set_save_target(Some(target));
        assert_eq!(
            save_active(&mut app, &mut AppSettings::load(None), &mut false, false).poll(),
            Some(false)
        );
        assert!(!app.active_ws().unwrap().dirty);
        set_save_target(None);
    }
    struct DialogDriver {
        ctx: egui::Context,
        stack: super::super::DialogStack,
        settings: pdn_core::settings::AppSettings,
        prefs: crate::prefs::Prefs,
        time: f64,
        close_requested: bool,
    }
    impl DialogDriver {
        fn new() -> Self {
            super::super::PENDING.with(|p| p.borrow_mut().clear());
            let ctx = egui::Context::default();
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            Self {
                ctx,
                stack: Default::default(),
                settings: pdn_core::settings::AppSettings::load(None),
                prefs: Default::default(),
                time: 0.0,
                close_requested: false,
            }
        }
        fn frame(&mut self, app: &mut AppWorkspace, events: Vec<egui::Event>) {
            self.time += 0.1;
            app.on_frame(self.time * 1000.0);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280.0, 800.0),
                )),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            let mut output = self.ctx.run_ui(input, |ui| {
                self.stack.show(
                    ui,
                    &mut super::super::DialogCx {
                        app,
                        settings: &mut self.settings,
                        prefs: &mut self.prefs,
                        settings_changed: false,
                        shortcuts: None,
                        clipboard: None,
                        settings_imports: None,
                        fonts: None,
                        asset_library: None,
                        seat_focus: false,
                    },
                );
            });
            self.close_requested |= output.viewport_output.values().any(|viewport| {
                viewport
                    .commands
                    .iter()
                    .any(|command| matches!(command, egui::ViewportCommand::Close))
            });
            output.textures_delta.clear();
        }
        fn key(&mut self, app: &mut AppWorkspace, key: egui::Key) {
            self.frame(
                app,
                vec![
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                    egui::Event::Key {
                        key,
                        physical_key: None,
                        pressed: false,
                        repeat: false,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        fn show(&mut self, app: &mut AppWorkspace) {
            self.frame(app, vec![]);
            self.frame(app, vec![]);
        }
        fn accept_config(&mut self, app: &mut AppWorkspace) {
            self.show(app);
            let at = self.stack.top_rect(&self.ctx).unwrap().max - egui::vec2(177.0, 25.0);
            for pressed in [true, false] {
                self.frame(
                    app,
                    vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
        }
        fn flatten(&mut self, app: &mut AppWorkspace) {
            self.show(app);
            self.key(app, egui::Key::Tab);
            self.key(app, egui::Key::Enter);
        }
    }
    fn two_layers() -> AppWorkspace {
        let mut app = fresh_doc(4, 2, None);
        pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
        app
    }
    fn png_plan(app: &AppWorkspace, target: Rc<RefCell<MemoryTarget>>) -> Plan {
        let id = app.active_ws().unwrap().id;
        Plan {
            id,
            old: FileInfo::capture(app, id).unwrap(),
            destination: SaveDestination {
                name: "new.png".into(),
                path: Some("/new.png".into()),
                format: "png",
            },
            target,
        }
    }
    #[test]
    fn flatten_cancel_restores_old_tuple_without_writing_or_history() {
        let mut driver = DialogDriver::new();
        let mut app = two_layers();
        compatible(&mut app, 0);
        let id = app.active_ws().unwrap().id;
        let history = app.active_ws().unwrap().history.undo.len();
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        let plan = png_plan(&app, target.clone());
        let result = SaveHandle::default();
        let h = result.clone();
        selected(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            plan,
            SaveSelection {
                format: "png",
                options: SaveOptions::default_for("png"),
            },
            Box::new(move |ok, _, _, _| h.0.set(Some(ok))),
        );
        assert_eq!(
            app.active_ws().unwrap().file_path.as_deref(),
            Some("/new.png")
        );
        driver.show(&mut app);
        driver.key(&mut app, egui::Key::Escape);
        assert_eq!(result.poll(), Some(false));
        assert_eq!(app.active_ws().unwrap().id, id);
        assert_eq!(
            app.active_ws().unwrap().file_path.as_deref(),
            Some("/image0.png")
        );
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
        assert!(target.borrow().writes.is_empty());
    }
    #[test]
    fn confirmed_flatten_survives_writer_failure_but_save_tuple_rolls_back() {
        let mut driver = DialogDriver::new();
        let mut app = two_layers();
        compatible(&mut app, 0);
        let id = app.active_ws().unwrap().id;
        let document = app.active_ws().unwrap().document.id;
        let history = app.active_ws().unwrap().history.undo.len();
        let target = Rc::new(RefCell::new(MemoryTarget {
            fail_at: Some(1),
            ..Default::default()
        }));
        let plan = png_plan(&app, target.clone());
        let h = SaveHandle::default();
        let done = h.clone();
        selected(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            plan,
            SaveSelection {
                format: "png",
                options: SaveOptions::default_for("png"),
            },
            Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
        );
        driver.flatten(&mut app);
        assert_eq!(h.poll(), Some(false));
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.id, id);
        assert_ne!(ws.document.id, document);
        assert_eq!(ws.document.layers.size(), 1);
        assert_eq!(ws.history.undo.len(), history + 1);
        assert!(ws.history.redo.is_empty());
        assert!(ws.dirty);
        assert_eq!(ws.file_path.as_deref(), Some("/image0.png"));
        assert_eq!(target.borrow().writes.len(), 1);
        assert!(app.undo());
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
    }
    #[test]
    fn accepted_pdn_target_skips_configuration_and_preserves_layers() {
        let mut driver = DialogDriver::new();
        let mut app = two_layers();
        let id = app.active_ws().unwrap().id;
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_active(&mut app, &mut AppSettings::load(None), &mut false, true).poll(),
            Some(true)
        );
        driver.show(&mut app);
        assert!(!driver.stack.is_open());
        let t = target.borrow();
        assert_eq!(t.choices, ["pdn"]);
        let doc = pdn::decode(&t.writes[0].bytes).unwrap().to_document();
        assert_eq!(doc.layers.size(), 2);
        assert_eq!(t.writes[0].workspace_id, id);
        assert!(!app.active_ws().unwrap().dirty);
        set_save_target(None);
    }
    #[test]
    fn options_cancel_leaves_target_tuple_and_layers_unchanged() {
        let mut driver = DialogDriver::new();
        let mut app = fresh_doc(2, 1, None);
        compatible(&mut app, 0);
        let history = app.active_ws().unwrap().history.undo.len();
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        set_save_target(Some(target.clone()));
        let h = save_active(&mut app, &mut AppSettings::load(None), &mut false, true);
        driver.show(&mut app);
        assert!(driver.stack.is_open());
        driver.key(&mut app, egui::Key::Escape);
        assert_eq!(h.poll(), Some(false));
        assert_eq!(
            app.active_ws().unwrap().file_path.as_deref(),
            Some("/image0.png")
        );
        assert_eq!(app.active_ws().unwrap().history.undo.len(), history);
        assert!(target.borrow().writes.is_empty());
        set_save_target(None);
    }

    #[test]
    fn a_mismatched_destination_never_receives_different_format_bytes() {
        let mut app = fresh_doc(2, 1, None);
        let target = Rc::new(RefCell::new(MemoryTarget::default()));
        let mut plan = png_plan(&app, target.clone());
        plan.destination.name = "wrong.pdn".into();
        plan.destination.path = Some("/wrong.pdn".into());
        let h = SaveHandle::default();
        let done = h.clone();
        selected(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            plan,
            SaveSelection {
                format: "png",
                options: SaveOptions::default_for("png"),
            },
            Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
        );
        assert_eq!(h.poll(), Some(false));
        assert!(target.borrow().writes.is_empty());
        assert!(app.active_ws().unwrap().file_path.is_none());
    }

    #[test]
    fn close_save_keeps_owner_through_flatten_and_restores_context_image() {
        for fail in [false, true] {
            let mut driver = DialogDriver::new();
            let mut app = two_layers();
            compatible(&mut app, 0);
            let closing = app.documents[0].id;
            app.documents[0].is_initial_workspace = false;
            app.create_blank_document_in_new_workspace(4, 2, 96.0, false);
            let survivor = app.documents[1].id;
            let survivor_pixels = app.documents[1].document.composite().data().to_vec();
            let survivor_history = app.documents[1].history.undo.len();
            let target = Rc::new(RefCell::new(MemoryTarget {
                first_format: Some("png"),
                fail_at: fail.then_some(1),
                ..Default::default()
            }));
            set_save_target(Some(target.clone()));
            close_then(
                &mut app,
                &mut AppSettings::load(None),
                &mut false,
                0,
                Some(survivor),
            );
            driver.show(&mut app);
            driver.key(&mut app, egui::Key::Tab);
            driver.key(&mut app, egui::Key::Tab);
            driver.key(&mut app, egui::Key::Enter);
            driver.accept_config(&mut app);
            driver.flatten(&mut app);
            assert_eq!(target.borrow().writes.len(), 1);
            assert_eq!(target.borrow().writes[0].workspace_id, closing);
            if fail {
                assert_eq!(app.documents.len(), 2);
                assert_eq!(app.active_ws().unwrap().id, closing);
                assert_eq!(app.documents[0].document.layers.size(), 1);
                assert!(app.documents[0].dirty);
                assert_eq!(app.documents[0].file_path.as_deref(), Some("/image0.png"));
            } else {
                assert_eq!(app.documents.len(), 1);
                assert_eq!(app.active_ws().unwrap().id, survivor);
            }
            let ws = app.documents.iter().find(|w| w.id == survivor).unwrap();
            assert_eq!(ws.document.composite().data(), survivor_pixels);
            assert_eq!(ws.history.undo.len(), survivor_history);
            set_save_target(None);
        }
    }
    #[test]
    fn save_all_restores_workspace_identity_after_flatten_replaces_document() {
        let mut driver = DialogDriver::new();
        let mut app = two_layers();
        compatible(&mut app, 0);
        app.documents[0].is_initial_workspace = false;
        let original = app.documents[0].id;
        app.create_blank_document_in_new_workspace(4, 2, 96.0, false);
        compatible(&mut app, 1);
        app.documents[1].set_dirty(true);
        app.set_active_document_workspace(Some(0));
        let target = Rc::new(RefCell::new(MemoryTarget {
            first_format: Some("png"),
            ..Default::default()
        }));
        set_save_target(Some(target.clone()));
        let h = save_all(&mut app, &mut AppSettings::load(None), &mut false);
        driver.accept_config(&mut app);
        driver.flatten(&mut app);
        assert_eq!(h.poll(), Some(true));
        assert_eq!(app.active_ws().unwrap().id, original);
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 1);
        assert_eq!(target.borrow().writes.len(), 2);
        set_save_target(None);
    }
    #[test]
    fn save_all_chooser_cancel_keeps_failed_image_active() {
        let mut app = fresh_doc(2, 1, None);
        app.documents[0].is_initial_workspace = false;
        for _ in 0..2 {
            app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
        }
        compatible(&mut app, 0);
        compatible(&mut app, 2);
        for ws in &mut app.documents {
            ws.set_dirty(true);
        }
        app.set_active_document_workspace(Some(2));
        let target = Rc::new(RefCell::new(MemoryTarget {
            cancel: true,
            ..Default::default()
        }));
        set_save_target(Some(target.clone()));
        assert_eq!(
            save_all(&mut app, &mut AppSettings::load(None), &mut false).poll(),
            Some(false)
        );
        assert_eq!(app.active, Some(1));
        assert_eq!(target.borrow().writes.len(), 1);
        assert!(!app.documents[0].dirty);
        assert!(app.documents[1].dirty && app.documents[2].dirty);
        set_save_target(None);
    }
    #[test]
    fn exit_partial_save_closes_successes_and_restores_original_if_still_open() {
        let mut driver = DialogDriver::new();
        let mut app = fresh_doc(2, 1, None);
        app.documents[0].is_initial_workspace = false;
        for _ in 0..2 {
            app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
        }
        for i in 0..3 {
            compatible(&mut app, i);
            app.documents[i].set_dirty(true);
        }
        let original = app.documents[2].id;
        let failed = app.documents[1].id;
        let target = Rc::new(RefCell::new(MemoryTarget {
            fail_at: Some(2),
            ..Default::default()
        }));
        set_save_target(Some(target.clone()));
        exit(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            &driver.ctx,
        );
        driver.show(&mut app);
        for _ in 0..4 {
            driver.key(&mut app, egui::Key::Tab);
        }
        driver.key(&mut app, egui::Key::Enter);
        assert_eq!(target.borrow().writes.len(), 2);
        assert_eq!(
            app.documents.iter().map(|w| w.id).collect::<Vec<_>>(),
            [failed, original]
        );
        assert_eq!(app.active_ws().unwrap().id, original);
        assert!(app.documents.iter().all(|w| w.dirty));
        set_save_target(None);
    }
    #[test]
    fn exit_one_dirty_image_uses_close_path_and_cancel_restores_original() {
        let mut driver = DialogDriver::new();
        let mut app = fresh_doc(2, 1, None);
        app.documents[0].is_initial_workspace = false;
        app.documents[0].set_dirty(true);
        app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
        let original = app.documents[1].id;
        exit(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            &driver.ctx,
        );
        driver.show(&mut app);
        assert_eq!(app.active, Some(0));
        driver.key(&mut app, egui::Key::Escape);
        assert_eq!(app.documents.len(), 2);
        assert_eq!(app.active_ws().unwrap().id, original);
        assert!(!driver.stack.is_open());
    }
    #[test]
    fn exit_multi_image_cancel_restores_original_after_thumbnail_navigation() {
        for cancel in ["choice", "escape", "close"] {
            let mut driver = DialogDriver::new();
            let mut app = fresh_doc(2, 1, None);
            app.documents[0].is_initial_workspace = false;
            app.documents[0].set_dirty(true);
            app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
            app.documents[1].set_dirty(true);
            app.set_active_document_workspace(Some(0));
            let ids = app.documents.iter().map(|ws| ws.id).collect::<Vec<_>>();
            let target = Rc::new(RefCell::new(MemoryTarget::default()));
            set_save_target(Some(target.clone()));
            exit(
                &mut app,
                &mut AppSettings::load(None),
                &mut false,
                &driver.ctx,
            );
            driver.show(&mut app);
            driver.key(&mut app, egui::Key::Tab);
            driver.key(&mut app, egui::Key::Tab);
            driver.key(&mut app, egui::Key::Space);
            assert_eq!(app.active_ws().unwrap().id, ids[1]);
            let rect = driver.stack.top_rect(&driver.ctx).unwrap();
            if cancel == "escape" {
                driver.key(&mut app, egui::Key::Escape);
            } else {
                let at = if cancel == "choice" {
                    egui::pos2(rect.center().x, rect.max.y - 40.0)
                } else {
                    egui::pos2(rect.max.x - 20.0, rect.min.y + 15.0)
                };
                driver.frame(&mut app, vec![egui::Event::PointerMoved(at)]);
                for pressed in [true, false] {
                    driver.frame(
                        &mut app,
                        vec![egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        }],
                    );
                }
            }
            assert!(!driver.stack.is_open(), "{cancel}");
            assert_eq!(app.active_ws().unwrap().id, ids[0], "{cancel}");
            assert_eq!(
                app.documents.iter().map(|ws| ws.id).collect::<Vec<_>>(),
                ids
            );
            assert!(app.documents.iter().all(|ws| ws.dirty));
            assert!(target.borrow().choices.is_empty());
            assert!(target.borrow().writes.is_empty());
            assert!(!driver.close_requested);
            set_save_target(None);
        }
    }
    #[test]
    fn configuration_format_change_reselects_before_flatten_or_write() {
        for cancel in [false, true] {
            let mut driver = DialogDriver::new();
            let mut app = two_layers();
            let target = Rc::new(RefCell::new(MemoryTarget {
                cancel,
                ..Default::default()
            }));
            let mut plan = png_plan(&app, target.clone());
            plan.destination = SaveDestination {
                name: "old.pdn".into(),
                path: Some("/old.pdn".into()),
                format: "pdn",
            };
            let h = SaveHandle::default();
            let done = h.clone();
            selected(
                &mut app,
                &mut AppSettings::load(None),
                &mut false,
                plan,
                SaveSelection {
                    format: "png",
                    options: SaveOptions::default_for("png"),
                },
                Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
            );
            assert_eq!(target.borrow().choices, ["png"]);
            assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
            assert!(target.borrow().writes.is_empty());
            if cancel {
                assert_eq!(h.poll(), Some(false));
                assert!(app.active_ws().unwrap().file_path.is_none());
            } else {
                assert_eq!(
                    app.active_ws().unwrap().file_path.as_deref(),
                    Some("/Untitled.png")
                );
                driver.flatten(&mut app);
                assert_eq!(h.poll(), Some(true));
                let target = target.borrow();
                assert_eq!(target.writes[0].destination.format, "png");
                assert_eq!(target.writes[0].destination.name, "Untitled.png");
                assert!(target.writes[0].bytes.starts_with(b"\x89PNG"));
            }
        }
    }
    #[test]
    fn unsaved_thumbnail_space_selects_on_release() {
        let mut driver = DialogDriver::new();
        let mut app = fresh_doc(2, 1, None);
        app.documents[0].is_initial_workspace = false;
        app.documents[0].set_dirty(true);
        app.create_blank_document_in_new_workspace(2, 1, 96.0, false);
        app.documents[1].set_dirty(true);
        app.set_active_document_workspace(Some(0));
        exit(
            &mut app,
            &mut AppSettings::load(None),
            &mut false,
            &driver.ctx,
        );
        driver.show(&mut app);
        driver.key(&mut app, egui::Key::Tab);
        driver.key(&mut app, egui::Key::Tab);
        let key = |pressed| egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        driver.frame(&mut app, vec![key(true)]);
        assert_eq!(app.active, Some(0));
        driver.frame(&mut app, vec![key(false)]);
        assert_eq!(app.active, Some(1));
        assert!(driver.stack.is_open());
    }

    fn preference_settings() -> pdn_core::settings::AppSettings {
        let mut settings = pdn_core::settings::AppSettings::load(None);
        settings.values["fileTypes"]["saveOptions"] = json!({
            "jpeg":{"quality":70,"subsampling":"4:2:0"},
            "jxr":{"quality":70}, "png":{"keep":true}
        });
        settings
    }
    fn jpeg_plan(
        app: &mut AppWorkspace,
        target: Rc<RefCell<MemoryTarget>>,
        format: &'static str,
    ) -> Plan {
        app.active_ws_mut().unwrap().set_file_info(
            Some("old.jpg".into()),
            Some("/old.jpg".into()),
            Some("jpeg".into()),
            json!({"quality":80,"subsampling":"4:2:2"}),
        );
        let id = app.active_ws().unwrap().id;
        Plan {
            id,
            destination: SaveDestination {
                name: format!("new{}", find_format(format).extensions[0]),
                path: Some(format!("/new{}", find_format(format).extensions[0])),
                format,
            },
            old: FileInfo::capture(app, id).unwrap(),
            target,
        }
    }
    fn accepted_quality(format: &'static str, quality: u8) -> SaveSelection {
        let mut options = SaveOptions::default_for(format);
        options.quality = Some(quality);
        if format == "jpeg" {
            options.chroma = Some("4:4:4".into());
        }
        SaveSelection { format, options }
    }
    #[test]
    fn flatten_cancel_keeps_global_defaults_distinct_from_accepted_and_document_tokens() {
        for quality in [90, 95] {
            let mut driver = DialogDriver::new();
            driver.settings = preference_settings();
            let original = driver.settings.values["fileTypes"].clone();
            let mut app = two_layers();
            let target = Rc::new(RefCell::new(MemoryTarget::default()));
            let plan = jpeg_plan(&mut app, target.clone(), "jpeg");
            let h = SaveHandle::default();
            let done = h.clone();
            selected(
                &mut app,
                &mut driver.settings,
                &mut false,
                plan,
                accepted_quality("jpeg", quality),
                Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
            );
            assert_eq!(app.active_ws().unwrap().save_options["quality"], quality);
            assert_eq!(driver.settings.values["fileTypes"], original);
            driver.show(&mut app);
            driver.key(&mut app, egui::Key::Escape);
            assert_eq!(h.poll(), Some(false));
            assert_eq!(driver.settings.values["fileTypes"], original);
            assert_eq!(app.active_ws().unwrap().save_options["quality"], 80);
            assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
            assert!(target.borrow().writes.is_empty());
        }
    }
    #[test]
    fn target_reselection_cancel_does_not_remember_accepted_options() {
        let mut app = two_layers();
        let mut settings = preference_settings();
        let original = settings.values["fileTypes"].clone();
        let target = Rc::new(RefCell::new(MemoryTarget {
            cancel: true,
            ..Default::default()
        }));
        let plan = jpeg_plan(&mut app, target.clone(), "pdn");
        let h = SaveHandle::default();
        let done = h.clone();
        let mut changed = false;
        selected(
            &mut app,
            &mut settings,
            &mut changed,
            plan,
            accepted_quality("jpeg", 90),
            Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
        );
        assert_eq!(h.poll(), Some(false));
        assert_eq!(settings.values["fileTypes"], original);
        assert_eq!(app.active_ws().unwrap().save_options["quality"], 80);
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
        assert!(!changed);
        assert!(target.borrow().writes.is_empty());
    }
    #[test]
    fn final_attempt_remembers_nested_options_before_encode_and_late_failure_retains_them() {
        for (format, fail_at, success) in [
            ("jpeg", None, true),
            ("jpeg", Some(1), false),
            ("jxr", None, false),
        ] {
            let mut driver = DialogDriver::new();
            driver.settings = preference_settings();
            let mut app = two_layers();
            let target = Rc::new(RefCell::new(MemoryTarget {
                fail_at,
                ..Default::default()
            }));
            let plan = jpeg_plan(&mut app, target.clone(), format);
            let h = SaveHandle::default();
            let done = h.clone();
            selected(
                &mut app,
                &mut driver.settings,
                &mut false,
                plan,
                accepted_quality(format, 90),
                Box::new(move |ok, _, _, _| done.0.set(Some(ok))),
            );
            assert_eq!(
                driver.settings.values["fileTypes"]["saveOptions"][format]["quality"],
                70
            );
            driver.flatten(&mut app);
            assert_eq!(h.poll(), Some(success));
            let values = &driver.settings.values["fileTypes"];
            assert_eq!(values["saveOptions"][format]["quality"], 90);
            assert_eq!(values["saveOptions"]["png"], json!({"keep":true}));
            assert_eq!(
                values.as_object().unwrap().len(),
                1,
                "no accidental dotted property"
            );
            let ws = app.active_ws().unwrap();
            assert_eq!(ws.document.layers.size(), 1);
            assert_eq!(ws.dirty, !success);
            assert_eq!(ws.save_options["quality"], if success { 90 } else { 80 });
            if format == "jxr" {
                assert!(
                    target.borrow().writes.is_empty(),
                    "encoder refusal after remember"
                );
            } else {
                assert!(target.borrow().writes[0].bytes.starts_with(b"\xff\xd8"));
            }
        }
    }
    #[test]
    fn compatible_save_remembers_its_document_token_without_configuration() {
        let mut app = fresh_doc(2, 1, None);
        let mut settings = preference_settings();
        let target = Rc::new(RefCell::new(MemoryTarget {
            fail_at: Some(1),
            ..Default::default()
        }));
        let _ = jpeg_plan(&mut app, target.clone(), "jpeg");
        app.active_ws_mut().unwrap().set_dirty(false);
        set_save_target(Some(target.clone()));
        let mut changed = false;
        assert_eq!(
            save_active(&mut app, &mut settings, &mut changed, false).poll(),
            Some(false)
        );
        assert_eq!(
            settings.values["fileTypes"]["saveOptions"]["jpeg"]["quality"],
            80
        );
        assert!(changed);
        assert_eq!(app.active_ws().unwrap().save_options["quality"], 80);
        assert!(!app.active_ws().unwrap().dirty);
        assert!(target.borrow().choices.is_empty());
        assert_eq!(target.borrow().writes.len(), 1);
        set_save_target(None);
    }
}

#[cfg(test)]
mod pending_target_tests {
    use super::*;
    use crate::platform::Pending;
    struct Target {
        prepare_fails: bool,
        export: bool,
        writes: Rc<Cell<usize>>,
    }
    impl SaveTarget for Target {
        fn choose(&mut self, r: &TargetRequest) -> Result<Option<SaveDestination>, String> {
            Ok(Some(SaveDestination {
                name: r.suggested_name.clone(),
                path: Some(r.suggested_name.clone()),
                format: r.initial_format,
            }))
        }
        fn write(&mut self, _: &SaveRequest) -> Result<(), String> {
            self.writes.set(self.writes.get() + 1);
            Ok(())
        }
        fn prepare_pending(&mut self, _: u64, _: &SaveDestination) -> Pending<Result<(), String>> {
            Pending::ready(if self.prepare_fails {
                Err("prepare failed".into())
            } else {
                Ok(())
            })
        }
        fn write_pending(&mut self, r: SaveRequest) -> Pending<Result<WriteOutcome, String>> {
            let _ = self.write(&r);
            Pending::ready(Ok(if self.export {
                WriteOutcome::DownloadStarted
            } else {
                WriteOutcome::Written
            }))
        }
    }
    #[test]
    fn prepare_failure_does_not_remember_or_write() {
        let mut app = pdn_core::testing::fresh_doc(2, 2, Some(4));
        app.active_ws_mut().unwrap().set_file_info(
            Some("a.png".into()),
            Some("a.png".into()),
            Some("png".into()),
            json!({"quality":17}),
        );
        let mut settings = AppSettings::load(None);
        let mut changed = false;
        settings.set("fileTypes.saveOptions", json!({"png":{"quality":81}}));
        let writes = Rc::new(Cell::new(0));
        set_save_target(Some(Rc::new(RefCell::new(Target {
            prepare_fails: true,
            export: false,
            writes: writes.clone(),
        }))));
        let h = save_active(&mut app, &mut settings, &mut changed, false);
        assert_eq!(h.poll(), Some(false));
        assert_eq!(writes.get(), 0);
        assert_eq!(settings.get("fileTypes.saveOptions")["png"]["quality"], 81);
        set_save_target(None);
    }
    #[test]
    fn export_handoff_keeps_dirty_and_is_unsuccessful_for_close() {
        let mut app = pdn_core::testing::fresh_doc(2, 2, Some(4));
        app.active_ws_mut().unwrap().set_file_info(
            Some("a.png".into()),
            Some("a.png".into()),
            Some("png".into()),
            json!({"quality":17}),
        );
        app.active_ws_mut().unwrap().set_dirty(true);
        let mut settings = AppSettings::load(None);
        let mut changed = false;
        set_save_target(Some(Rc::new(RefCell::new(Target {
            prepare_fails: false,
            export: true,
            writes: Rc::new(Cell::new(0)),
        }))));
        let h = save_active(&mut app, &mut settings, &mut changed, false);
        assert_eq!(h.poll(), Some(false));
        assert!(app.active_ws().unwrap().dirty);
        set_save_target(None);
    }
}
