use super::EffectConfigDialog;
use crate::dialogs::DialogCx;
use crate::platform::{Clipboard, DataFile, ImageRead, ImageReadRequest, Pending, PickedFile};
use pdn_core::canvas::ImageData;
use pdn_core::effect::bb::{
    self,
    ui::{External, Preset},
};
use pdn_core::extra::fx::lut;
use pdn_core::io::pdn::PdnDocument;
use serde_json::json;
use std::sync::Arc;
use std::{cell::RefCell, rc::Rc};

/// Acquisition lives on the UI thread; only the resolved Arc enters worker snapshots.
pub(super) enum ExternalInput {
    Unstarted,
    Pending {
        generation: u64,
        request: ImageReadRequest,
        fallback: Option<Arc<ImageData>>,
        authoritative: bool,
    },
    /// BB27: the current value's one read/decode chain.
    Loading {
        generation: u64,
        stage: FileStage,
    },
    Resolved,
    /// BB27 (Q65): the current acquisition failed; `file_error` says why.
    Failed,
    Retired,
}

/// R156: a read (`None`: absent, a directory or empty), then the actual decode service.
pub(super) enum FileStage {
    Read(Pending<Result<Option<PickedFile>, String>>),
    Decode(Pending<Result<PdnDocument, String>>),
}

/// Load Preset...'s chooser, then the chosen file's read.
pub(super) enum PresetLoad {
    Choose(Pending<Result<Vec<PickedFile>, String>>),
    Read(Pending<Result<PickedFile, String>>),
}

/// R156: the last successful From File pick (not a native path, whose text is reread), shared by
/// Shell with the dialogs it opens, for this app session only.
#[derive(Clone, Debug)]
pub struct FileAttachment {
    pub value: String,
    pub file: PickedFile,
}
pub type FileSource = Rc<RefCell<Option<FileAttachment>>>;

impl EffectConfigDialog {
    /// R157: the document faces the current values need. R195: a bundled script fallback that
    /// failed (or has no manager to load it) is dropped, so it draws missing-glyph outlines
    /// instead of blocking the dialog.
    fn required(
        &self,
        fonts: Option<&crate::platform::fonts::FontManager>,
    ) -> Vec<(String, pdn_core::fonts::FaceStyle)> {
        let Some(ui) = self.session.as_ref().and_then(|s| s.def.indirect) else {
            return vec![];
        };
        (ui.fonts)(&self.values)
            .into_iter()
            .filter(|(family, style)| {
                !pdn_core::fonts::is_script_fallback(family)
                    || pdn_core::fonts::registered_face(family, *style).is_some()
                    || fonts.is_some_and(|m| m.family_error(family, *style).is_none())
            })
            .collect()
    }

    /// R157: the registered bytes of every required face; `None` while one is not registered.
    fn required_faces(
        &self,
        fonts: Option<&crate::platform::fonts::FontManager>,
    ) -> Option<Vec<Arc<[u8]>>> {
        self.required(fonts)
            .iter()
            .map(|(family, style)| pdn_core::fonts::registered_face(family, *style))
            .collect()
    }

    /// R157: whether every required face is registered. A missing one is requested through the
    /// app's FontManager (its own jobs, retries and revision); static bundled faces are ready at
    /// once and unrelated font jobs never delay a ready effect. `Err`: a required load failed.
    pub(super) fn fonts_ready(
        &mut self,
        cx: &mut DialogCx,
        ctx: &egui::Context,
    ) -> Result<bool, String> {
        self.font_error = None;
        let ui = self.session.as_ref().and_then(|s| s.def.indirect);
        'requirements: loop {
            let required = self.required(cx.fonts.as_deref());
            // R154: a styled face is requested even while the registry stands its regular face
            // in for it, and awaited while it loads (Calendar's 700 of a 400 face); once its load
            // failed with that stand-in registered, it is not requested again (BB30).
            if let Some(m) = cx.fonts.as_deref_mut() {
                for (family, style) in &required {
                    let stood_in = m.family_error(family, *style).is_some()
                        && pdn_core::fonts::registered_face(family, *style).is_some();
                    if *style != pdn_core::fonts::FaceStyle::Regular && !stood_in {
                        m.ensure_family(family, *style);
                    }
                }
            }
            let loading = cx
                .fonts
                .as_ref()
                .is_some_and(|m| required.iter().any(|(f, s)| m.loading(f, *s)));
            if !loading && let Some(faces) = self.required_faces(cx.fonts.as_deref()) {
                self.faces = (cx.fonts.as_ref().map_or(0, |m| m.revision), faces);
                return Ok(true);
            }
            for (family, style) in required {
                if pdn_core::fonts::registered_face(&family, style).is_some() {
                    continue;
                }
                let Some(m) = cx.fonts.as_deref_mut() else {
                    continue;
                };
                m.ensure_family(&family, style);
                if let Some(error) = m.family_error(&family, style) {
                    // A finished failure (never a pending job) may move the session on to the
                    // effect's fallback family, once; its face is then awaited instead.
                    if ui.is_some_and(|u| (u.font_fallback)(&mut self.values)) {
                        self.rekey();
                        continue 'requirements;
                    }
                    self.font_error = Some(error.into());
                    return Err(error.into());
                }
                if let Some(delay) = m.retry_delay(&family, style) {
                    ctx.request_repaint_after(delay);
                }
            }
            break;
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(10));
        Ok(false)
    }

    /// R157: a font revision that changed a required face's bytes invalidates the generation.
    pub(super) fn check_faces(&mut self, cx: &mut DialogCx) {
        let Some(revision) = cx.fonts.as_ref().map(|m| m.revision) else {
            return;
        };
        if revision == self.faces.0 || self.faces.1.is_empty() {
            return;
        }
        self.faces.0 = revision;
        let changed = self.required_faces(cx.fonts.as_deref()).is_none_or(|now| {
            now.len() != self.faces.1.len()
                || now.iter().zip(&self.faces.1).any(|(a, b)| a[..] != b[..])
        });
        if changed {
            // A face is no settings change (BB33): no fresh external read; render again now.
            self.rekey();
            self.pending_notice = None;
            self.shown = None;
            self.due = Some(cx.app.now_ms);
        }
    }

    /// A new generation for the same values (a face changed, not the external source): a
    /// pending read is re-keyed to it and a resolved image kept.
    fn rekey(&mut self) {
        self.retire_work();
        self.generation += 1;
        if let ExternalInput::Pending { generation, .. } = &mut self.input {
            *generation = self.generation;
        }
    }

    /// R155: whether the current values read an external input.
    pub(super) fn external(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|s| s.def.indirect)
            .is_some_and(|u| (u.external_for)(&self.values) != External::None)
    }

    /// The value key of a file-sourced effect (BB27; Color Lookup's LUT file, I1).
    pub(super) fn file_key(&self) -> Option<&'static str> {
        match (self.session.as_ref()?.def.indirect?.external_for)(&self.values) {
            External::File(key) | External::Lut(key) => Some(key),
            _ => None,
        }
    }

    /// The current file source is a 3D LUT (parsed), not an image (decoded).
    fn lut_source(&self) -> bool {
        self.session
            .as_ref()
            .and_then(|s| s.def.indirect)
            .is_some_and(|u| matches!((u.external_for)(&self.values), External::Lut(_)))
    }

    /// The extension check for the current file source, made before any bytes are read.
    fn admit(&self) -> fn(&str) -> Result<(), String> {
        if self.lut_source() {
            lut::admit
        } else {
            bb::fill::admit_file
        }
    }

    /// R156: Shell's committed attachment. The invocation snapshots its pick when the opening
    /// value still names it; a successful commit publishes the invocation's own source back.
    pub fn attach_file_source(&mut self, source: FileSource) {
        if let Some(key) = self.file_key() {
            self.candidate = source
                .borrow()
                .as_ref()
                .filter(|a| a.value == bb::text(&self.values, key))
                .map(|a| a.file.clone());
        }
        self.file_source = source;
    }

    /// R156: only the original session's successful commit publishes, or clears, the attachment
    /// (`key`: the committed effect's file key, read before its session is consumed).
    pub(super) fn publish_file_source(&mut self, key: Option<&str>) {
        if let Some(key) = key {
            let value = bb::text(&self.values, key).to_string();
            *self.file_source.borrow_mut() = self
                .candidate
                .take()
                .map(|file| FileAttachment { value, file });
        }
    }

    /// One acquisition of the current value (R156): the attached pick, else the typed path. The
    /// extension is admitted before any bytes (fill-from-file.md "Loading the file").
    fn start_file(&mut self, key: &str) {
        #[cfg(test)]
        {
            self.acquisitions += 1;
        }
        let value = bb::text(&self.values, key).to_string();
        let read = match &self.candidate {
            Some(file) => match self.admit()(&file.name) {
                Err(e) => Pending::ready(Err(e)),
                Ok(()) => {
                    let read = file.reread();
                    Pending::future(async move { read.await.map(Some) })
                }
            },
            None if value.is_empty() => Pending::ready(Ok(None)),
            None => self.typed_read(value),
        };
        self.input = ExternalInput::Loading {
            generation: self.generation,
            stage: FileStage::Read(read),
        };
        self.resolve_external();
    }

    fn typed_read(&self, path: String) -> Pending<Result<Option<PickedFile>, String>> {
        #[cfg(not(target_arch = "wasm32"))]
        {
            #[cfg(test)]
            let typed = self.typed_paths;
            #[cfg(not(test))]
            let typed = true;
            if typed {
                return self.files.read_existing(path.into(), self.admit());
            }
        }
        let _ = path;
        // spec-question: Q64 From File typed browser paths
        Pending::ready(Err(bb::conventions::FILE_BROWSER_TYPED_PATH.into()))
    }

    /// Advances the current read/decode chain as far as it is ready. A finished operation is
    /// dropped, never polled again. A LUT file is parsed once read: it has no decode stage.
    fn advance_file(&mut self) {
        let lut = self.lut_source();
        let result = loop {
            let ExternalInput::Loading { generation, stage } = &mut self.input else {
                return;
            };
            if *generation != self.generation || self.session.is_none() {
                self.input = ExternalInput::Retired;
                return;
            }
            match stage {
                FileStage::Read(op) => match op.poll() {
                    None => return,
                    // ponytail: parsed on the UI thread (a 129³ table, the largest, is ~2 M
                    // lines); move it beside the decode service if big tables stutter.
                    Some(Ok(Some(file))) if lut => {
                        break lut::parse(&file.name, &file.bytes)
                            .map(|t| (None, Some(Arc::new(t))))
                            .map_err(|e| e.0);
                    }
                    Some(Ok(Some(file))) => *stage = FileStage::Decode(self.files.decode(file)),
                    Some(other) => break other.map(|_| (None, None)),
                },
                FileStage::Decode(op) => match op.poll() {
                    None => return,
                    // fill-from-file.md "Existing file": flattened, at its native pixel size.
                    Some(Ok(doc)) => {
                        let doc = doc.to_document();
                        let flat = doc.composite();
                        let image = flat.get_image_data(0, 0, doc.width, doc.height);
                        break Ok((Some(Arc::new(image)), None));
                    }
                    Some(Err(e)) => break Err(e),
                },
            }
        };
        let session = self.session.as_mut().unwrap();
        match result {
            Ok((image, table)) => {
                session.set_image(image);
                session.set_lut(table);
                self.file_error = None;
                self.input = ExternalInput::Resolved;
            }
            Err(e) => {
                session.set_image(None);
                session.set_lut(None);
                self.file_error = Some(e);
                self.input = ExternalInput::Failed;
            }
        }
    }

    /// The open chooser's answer. A cancel (an empty pick, or the activation prompt's exact
    /// `Canceled`) keeps the value, source, load and error; an accepted file becomes the value,
    /// and (without a native path) the attached source: one notification, even for a same-name
    /// pick, whose source differs.
    pub(super) fn poll_chooser(&mut self) -> Option<bool> {
        let picked = self.chooser.as_mut()?.poll()?;
        self.chooser = None;
        let key = self.file_key()?;
        match picked {
            Ok(files) => {
                let file = files.into_iter().next()?;
                let value = file
                    .path
                    .as_ref()
                    .map_or_else(|| file.name.clone(), |p| p.to_string_lossy().into_owned());
                self.candidate = file.path.is_none().then_some(file);
                self.values.insert(key.into(), json!(value));
                Some(true)
            }
            Err(e) if e == "Canceled" => None,
            Err(e) => {
                crate::platform::files::message(&self.title, e);
                None
            }
        }
    }

    /// The plugin's preset file (`IndirectUi::preset`), if it has one.
    pub(super) fn preset(&self) -> Option<Preset> {
        self.session.as_ref()?.def.indirect?.preset
    }

    /// Load Preset...: one file through the preset's chooser.
    pub(super) fn choose_preset(&mut self) {
        if let Some(p) = self.preset() {
            let request = DataFile {
                title: "Load Preset",
                filter: p.filter,
                extension: p.extension,
            };
            self.preset_load = Some(PresetLoad::Choose(self.files.choose_data(request)));
        }
    }

    /// Load Preset...'s answer: the chosen file, read within the preset's limit, sets the values
    /// (`Some(true)`: an immediate notification); a refused one says why in a message, a cancel
    /// changes nothing.
    pub(super) fn poll_preset(&mut self) -> Option<bool> {
        let preset = self.preset()?;
        // A chooser answer and an already-read file finish in one frame (Chart's Load Data).
        for _ in 0..2 {
            let result = match self.preset_load.as_mut()? {
                PresetLoad::Choose(op) => match op.poll()? {
                    Ok(files) => {
                        let read = |f: PickedFile| f.reread_within(preset.max_bytes as u64);
                        self.preset_load = files.into_iter().next().map(read).map(PresetLoad::Read);
                        continue;
                    }
                    Err(e) => Err(e),
                },
                PresetLoad::Read(op) => op.poll()?.and_then(|f| {
                    (preset.load)(&f.bytes, &mut self.values)
                        .map_err(|e| format!("{}: {e}", f.name))
                }),
            };
            self.preset_load = None;
            match result {
                Ok(()) => {
                    self.on_change();
                    return Some(true);
                }
                Err(e) if e == "Canceled" => {}
                Err(e) => crate::platform::files::message(&self.title, e),
            }
            break;
        }
        None
    }

    /// Pending file work does not wake the UI: poll it at the file service's interval.
    pub(super) fn repaint_file_work(&self, ctx: &egui::Context) {
        if self.chooser.is_some()
            || self.preset_load.is_some()
            || matches!(self.input, ExternalInput::Loading { .. })
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(10));
        }
    }

    pub(super) fn input_ready(&self) -> bool {
        matches!(self.input, ExternalInput::Resolved)
    }

    /// The shell calls this in the opening command's user gesture; notifications use it too.
    pub fn acquire_external(
        &mut self,
        app: &pdn_core::app::AppWorkspace,
        ctx: &egui::Context,
        clipboard: Option<&mut Clipboard>,
    ) {
        if !self.external() || !matches!(self.input, ExternalInput::Unstarted) {
            return;
        }
        if let Some(key) = self.file_key() {
            return self.start_file(key);
        }
        let fallback = app.clipboard.item.as_ref().map(|item| item.masked.clone());
        if let Some(clipboard) = clipboard {
            let authoritative = clipboard.is_system();
            #[cfg(target_arch = "wasm32")]
            let authoritative = authoritative || clipboard.web_images;
            self.input = ExternalInput::Pending {
                generation: self.generation,
                request: clipboard.request_image(ctx),
                fallback,
                authoritative,
            };
            self.resolve_external();
        } else {
            if let Some(session) = &mut self.session {
                session.set_image(fallback);
            }
            self.input = ExternalInput::Resolved;
        }
    }

    fn resolve_external(&mut self) {
        if matches!(self.input, ExternalInput::Loading { .. }) {
            return self.advance_file();
        }
        let input = std::mem::replace(&mut self.input, ExternalInput::Resolved);
        let ExternalInput::Pending {
            generation,
            mut request,
            fallback,
            authoritative,
        } = input
        else {
            self.input = input;
            return;
        };
        if generation != self.generation || self.session.is_none() {
            self.input = ExternalInput::Retired;
            return;
        }
        match request.poll() {
            ImageRead::Pending => {
                self.input = ExternalInput::Pending {
                    generation,
                    request,
                    fallback,
                    authoritative,
                }
            }
            ImageRead::Ready(result) => {
                let image = match result {
                    Ok(Some(image)) => Some(Arc::new(image)),
                    Ok(None) if authoritative => None,
                    _ => fallback,
                };
                self.session.as_mut().unwrap().set_image(image);
            }
            ImageRead::Canceled => {
                self.retire_work();
                self.shown = None;
                self.due = None;
                self.pending_notice = None;
                self.input = ExternalInput::Retired;
                self.failed = self.finalizing;
            }
        }
    }

    pub(super) fn poll_external(&mut self, app: &mut pdn_core::app::AppWorkspace) {
        if self.session.as_ref().is_none_or(|s| !s.owner_exists(app)) {
            self.input = ExternalInput::Retired;
            self.failed = true;
            return;
        }
        let failed = matches!(self.input, ExternalInput::Failed);
        self.resolve_external();
        // A failure restores the layer once: no fill (fill-from-file.md Expected results 5).
        if matches!(self.input, ExternalInput::Retired)
            || (!failed && matches!(self.input, ExternalInput::Failed))
        {
            self.session.as_ref().unwrap().show(app, None);
        }
    }
}

#[cfg(test)]
pub(super) mod clipboard_tests {
    use super::super::*;
    use super::*;
    use crate::platform::{Clipboard, TestImageReads};
    use pdn_core::testing::{fresh_doc, layer_bytes};
    use std::sync::atomic::Ordering;
    fn with_cx<T>(
        app: &mut pdn_core::app::AppWorkspace,
        clipboard: &mut Clipboard,
        f: impl FnOnce(&mut DialogCx) -> T,
    ) -> T {
        f(&mut DialogCx {
            app,
            settings: &mut pdn_core::settings::AppSettings::load(None),
            prefs: &mut crate::prefs::Prefs::default(),
            settings_changed: false,
            shortcuts: None,
            clipboard: Some(clipboard),
            settings_imports: None,
            fonts: None,
            asset_library: None,
            seat_focus: false,
        })
    }
    fn setup(
        id: &str,
    ) -> (
        pdn_core::app::AppWorkspace,
        EffectConfigDialog,
        Clipboard,
        egui::Context,
    ) {
        let mut app = fresh_doc(3, 2, Some(4));
        app.active_ws_mut()
            .unwrap()
            .document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .data_mut()[3] = 0;
        let def = pdn_core::effect::find(id).unwrap();
        let s = EffectSession::begin(&mut app, def).unwrap();
        let values = s.defaults();
        (
            app,
            EffectConfigDialog::new(s, values),
            Clipboard::in_memory(),
            egui::Context::default(),
        )
    }
    fn white() -> ImageData {
        ImageData {
            width: 1,
            height: 1,
            data: vec![255; 4],
        }
    }
    fn finish(
        app: &mut pdn_core::app::AppWorkspace,
        c: &mut Clipboard,
        d: &mut EffectConfigDialog,
    ) {
        let s = d.session.as_ref().unwrap();
        let result = s.render_with(&d.values, None);
        let g = d.generation;
        d.due = None;
        with_cx(app, c, |cx| {
            assert!(d.accept_result(
                cx,
                JobResult {
                    generation: g,
                    result: Some(result),
                    elapsed: 1.0
                }
            ));
            assert_eq!(
                d.finish_ready(cx),
                Some(DialogState::Closed(DialogResult::Ok))
            );
        });
    }
    fn frame(
        app: &mut pdn_core::app::AppWorkspace,
        c: &mut Clipboard,
        d: &mut EffectConfigDialog,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        seat: bool,
    ) -> (DialogState, Rect) {
        let mut result = (DialogState::Open, Rect::NOTHING);
        let mut out = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280., 800.))),
                events,
                ..Default::default()
            },
            |ui| {
                result.0 = with_cx(app, c, |cx| {
                    cx.seat_focus = seat;
                    d.show(ui, cx)
                });
                result.1 = DialogFrameState::get(ctx, ui.scope_id()).rect;
            },
        );
        out.textures_delta.clear();
        result
    }
    fn key(key: Key) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .to_vec()
    }
    fn click(
        app: &mut pdn_core::app::AppWorkspace,
        c: &mut Clipboard,
        d: &mut EffectConfigDialog,
        ctx: &egui::Context,
        pos: Pos2,
    ) {
        frame(app, c, d, ctx, vec![egui::Event::PointerMoved(pos)], false);
        for pressed in [true, false] {
            frame(
                app,
                c,
                d,
                ctx,
                vec![egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
                false,
            );
        }
    }
    fn focus_initial_number(
        app: &mut pdn_core::app::AppWorkspace,
        c: &mut Clipboard,
        d: &mut EffectConfigDialog,
        ctx: &egui::Context,
    ) {
        let id = ctx.memory(|m| m.focused()).unwrap();
        let pos = ctx.read_response(id).unwrap().rect.center();
        click(app, c, d, ctx, pos);
    }

    type Snapshot = (
        Vec<u8>,
        bool,
        usize,
        std::collections::HashMap<String, Values>,
        Option<String>,
        Option<Values>,
    );
    fn snapshot(app: &pdn_core::app::AppWorkspace) -> Snapshot {
        let w = app.active_ws().unwrap();
        (
            layer_bytes(app, None),
            w.dirty,
            w.history.undo.len(),
            app.effect_tokens.clone(),
            app.last_effect.clone(),
            app.last_effect_values.clone(),
        )
    }
    /// R155: open/idle never reacquire; delayed OK with preview on/off waits for that read and commits once.
    #[test]
    fn bb26_pending_ok_uses_one_open_read() {
        for preview in [false, true] {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
            d.preview = preview;
            let before = snapshot(&app);
            d.acquire_external(&app, &ctx, Some(&mut c));
            assert_eq!(probe.reads.get(), 1);
            assert!(!d.input_ready());
            for _ in 0..3 {
                d.poll_external(&mut app);
                assert_eq!(probe.reads.get(), 1);
            }
            assert_eq!(
                with_cx(&mut app, &mut c, |cx| d.ok(cx, &ctx)),
                DialogState::Open
            );
            assert_eq!(snapshot(&app), before);
            probe.completions.borrow()[0].complete(Ok(Some(white())));
            d.poll_external(&mut app);
            assert!(d.input_ready());
            finish(&mut app, &mut c, &mut d);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            assert_eq!(app.last_effect.as_deref(), Some("bbPasteAlpha"));
            assert!(with_cx(&mut app, &mut c, |cx| d.finish_ready(cx)).is_none());
            assert_eq!(probe.reads.get(), 1);
        }
        TestImageReads::clear();
    }
    /// R155: notification retires even a Ready old handle and old render; only its generation is eligible.
    #[test]
    fn bb26_latest_read_and_render_ownership() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        d.acquire_external(&app, &ctx, Some(&mut c));
        let old = d.generation;
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        with_cx(&mut app, &mut c, |cx| d.notify(cx, &ctx, false));
        assert_eq!(probe.reads.get(), 2);
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        d.poll_external(&mut app);
        assert!(!d.input_ready());
        assert!(d.session.as_ref().unwrap().env().image.is_none());
        with_cx(&mut app, &mut c, |cx| {
            assert!(!d.accept_result(
                cx,
                JobResult {
                    generation: old,
                    result: Some((white(), None)),
                    elapsed: 0.
                }
            ))
        });
        probe.completions.borrow()[1].complete(Ok(None));
        d.poll_external(&mut app);
        assert!(d.input_ready());
        assert!(d.session.as_ref().unwrap().env().image.is_none());
        TestImageReads::clear();
    }
    /// fill-from-clipboard.md Expected results 8; object-paste-alpha.md Expected results 6: real memory image/text/empty transitions.
    #[test]
    fn bb26_authoritative_empty_and_transparent() {
        let probe = TestImageReads::install(false);
        for id in ["bbFillFromClipboard", "bbPasteAlpha"] {
            let (mut app, mut d, mut c, ctx) = setup(id);
            let transparent = ImageData {
                width: 2,
                height: 1,
                data: vec![11, 22, 33, 0, 44, 55, 66, 0],
            };
            c.write_image(&transparent);
            d.values.insert("source".into(), json!("alpha"));
            d.values.insert("nearest".into(), json!(true));
            d.acquire_external(&app, &ctx, Some(&mut c));
            assert_eq!(d.session.as_ref().unwrap().env().image, Some(&transparent));
            let source = d.session.as_ref().unwrap().source();
            let expected: Vec<u8> = source
                .data
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 0])
                .collect();
            let rendered = d.session.as_ref().unwrap().render(&d.values);
            if id == "bbPasteAlpha" {
                assert_eq!(rendered.data, expected);
            } else {
                assert_eq!(rendered, *source);
            }

            for text in [true, false] {
                if text {
                    c.write_text("text");
                } else {
                    c = Clipboard::in_memory();
                }
                with_cx(&mut app, &mut c, |cx| d.notify(cx, &ctx, true));
                assert!(d.input_ready());
                assert!(d.session.as_ref().unwrap().env().image.is_none());
                let source = d.session.as_ref().unwrap().source();
                assert_eq!(d.session.as_ref().unwrap().render(&d.values), *source);
            }
        }
        assert_eq!(probe.reads.get(), 6);
        TestImageReads::clear();
    }
    /// R155: service cancellation restores editable source, never retries on frames; OK reacquires once, final cancellation rolls back.
    #[test]
    fn bb26_service_cancel_retires_or_cancels_final() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        let before = snapshot(&app);
        d.acquire_external(&app, &ctx, Some(&mut c));
        probe.completions.borrow()[0].cancel();
        d.poll_external(&mut app);
        assert!(!d.input_ready());
        for _ in 0..3 {
            d.poll_external(&mut app);
        }
        assert_eq!(probe.reads.get(), 1);
        assert_eq!(
            with_cx(&mut app, &mut c, |cx| d.ok(cx, &ctx)),
            DialogState::Open
        );
        assert_eq!(probe.reads.get(), 2);
        c.write_text("supersedes");
        d.poll_external(&mut app);
        assert_eq!(
            with_cx(&mut app, &mut c, |cx| d.finish_ready(cx)),
            Some(DialogState::Closed(DialogResult::Cancel))
        );
        assert_eq!(snapshot(&app), before);
        probe.completions.borrow()[1].complete(Ok(Some(white())));
        assert_eq!(snapshot(&app), before);
        TestImageReads::clear();
    }
    /// R155: preview off restores immediately, but still reacquires; stale shown pixels cannot be reused.
    #[test]
    fn bb26_preview_toggle_invalidates_and_restores() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        let source = layer_bytes(&app, None);
        d.acquire_external(&app, &ctx, Some(&mut c));
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        d.poll_external(&mut app);
        let g = d.generation;
        let pixels = ImageData {
            width: 3,
            height: 2,
            data: [255, 0, 0, 99].repeat(6),
        };
        with_cx(&mut app, &mut c, |cx| {
            assert!(d.accept_result(
                cx,
                JobResult {
                    generation: g,
                    result: Some((pixels, None)),
                    elapsed: 1.
                }
            ))
        });
        assert_ne!(layer_bytes(&app, None), source);
        d.preview = false;
        with_cx(&mut app, &mut c, |cx| d.notify(cx, &ctx, true));
        assert_eq!(layer_bytes(&app, None), source);
        assert_eq!(probe.reads.get(), 2);
        assert!(d.current_preview().is_none());
        TestImageReads::clear();
    }
    /// R155/R108: ready final is accepted only after this frame's Escape, footer Cancel or title close
    /// (BB33: also Text Fun Factory's clipboard fill).
    #[test]
    fn bb26_same_frame_final_cancel() {
        for (how, text_fun) in ["escape", "footer", "close"]
            .into_iter()
            .flat_map(|how| [(how, false), (how, true)])
        {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = if text_fun {
                tff(json!({}))
            } else {
                setup("bbPasteAlpha")
            };
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            let before = snapshot(&app);
            d.acquire_external(&app, &ctx, Some(&mut c));
            with_cx(&mut app, &mut c, |cx| {
                assert_eq!(d.ok(cx, &ctx), DialogState::Open)
            });
            let (_, rect) = frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
            let events = if how == "escape" {
                key(Key::Escape)
            } else {
                let pos = if how == "footer" {
                    Pos2::new(rect.max.x - 40., rect.max.y - 20.)
                } else {
                    Pos2::new(rect.max.x - 15., rect.min.y + 15.)
                };
                let button = |pressed| egui::Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                frame(
                    &mut app,
                    &mut c,
                    &mut d,
                    &ctx,
                    vec![egui::Event::PointerMoved(pos)],
                    false,
                );
                frame(&mut app, &mut c, &mut d, &ctx, vec![button(true)], false);
                vec![button(false)]
            };
            probe.completions.borrow()[0].complete(Ok(Some(white())));
            d.poll_external(&mut app);
            let (tx, rx) = std::sync::mpsc::channel();
            let result = d.session.as_ref().unwrap().render_with(&d.values, None);
            tx.send(JobResult {
                generation: d.generation,
                result: Some(result),
                elapsed: 1.,
            })
            .unwrap();
            d.job = Some(rx);
            d.job_cancel = Some(Arc::new(AtomicBool::new(false)));
            assert_eq!(
                frame(&mut app, &mut c, &mut d, &ctx, events, false).0,
                DialogState::Closed(DialogResult::Cancel)
            );
            assert_eq!(snapshot(&app), before);
            assert_eq!(probe.reads.get(), 1);
        }
        TestImageReads::clear();
    }
    /// R155: active-tab changes retain the original owner; closing or replacing it rejects final bookkeeping.
    #[test]
    fn bb26_document_ownership() {
        for mode in ["switch", "close", "replace"] {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
            let first = app.active;
            d.acquire_external(&app, &ctx, Some(&mut c));
            with_cx(&mut app, &mut c, |cx| d.ok(cx, &ctx));
            app.create_blank_document_in_new_workspace(3, 2, 96., true);
            let other = layer_bytes(&app, None);
            if mode == "close" {
                app.close_document_workspace(first.unwrap());
            } else if mode == "replace" {
                app.documents[first.unwrap()].document.id += 99999;
            }
            let h = app
                .documents
                .iter()
                .map(|w| w.history.undo.len())
                .collect::<Vec<_>>();
            probe.completions.borrow()[0].complete(Ok(None));
            d.poll_external(&mut app);
            if mode == "switch" {
                finish(&mut app, &mut c, &mut d);
                assert_eq!(
                    app.documents[first.unwrap()].history.undo.len(),
                    h[first.unwrap()] + 1
                );
            } else {
                assert_eq!(
                    with_cx(&mut app, &mut c, |cx| d.finish_ready(cx)),
                    Some(DialogState::Closed(DialogResult::Cancel))
                );
                assert_eq!(
                    app.documents
                        .iter()
                        .map(|w| w.history.undo.len())
                        .collect::<Vec<_>>(),
                    h
                );
                assert!(app.last_effect.is_none());
            }
            assert_eq!(layer_bytes(&app, None), other);
        }
        TestImageReads::clear();
    }
    /// R155: absent reader and browser denial use the captured masked image; authoritative None never falls back.
    #[test]
    fn bb26_captured_fallback_policy() {
        for result in ["error", "empty", "absent", "default"] {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
            let old = ImageData {
                width: 1,
                height: 1,
                data: vec![17, 39, 81, 123],
            };
            app.clipboard.set_external(old.clone());
            if result == "default" {
                c = Clipboard::default();
            }
            d.acquire_external(
                &app,
                &ctx,
                if result == "absent" {
                    None
                } else {
                    Some(&mut c)
                },
            );
            app.clipboard.set_external(white());
            if result != "absent" {
                probe.completions.borrow()[0].complete(if result == "error" {
                    Err("denied".into())
                } else {
                    Ok(None)
                });
                d.poll_external(&mut app);
            }
            assert!(d.input_ready());
            assert_eq!(
                d.session.as_ref().unwrap().env().image,
                if result == "empty" { None } else { Some(&old) }
            );
        }
        TestImageReads::clear();
    }
    /// R155: one real first numeric key after pointer focus reads once; same-frame OK binds the edited values and new acquisition.
    #[test]
    fn bb26_first_key_edit_and_ok() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        d.session
            .as_mut()
            .unwrap()
            .def
            .controls
            .retain(|c| c.key() == "zoom");
        d.acquire_external(&app, &ctx, Some(&mut c));
        frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
        assert_eq!(probe.reads.get(), 1);
        focus_initial_number(&mut app, &mut c, &mut d, &ctx);
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        let mut events = key(Key::ArrowUp);
        events.extend(key(Key::Enter));
        assert_eq!(
            frame(&mut app, &mut c, &mut d, &ctx, events, false).0,
            DialogState::Open
        );
        assert_eq!(d.values["zoom"], json!(1.01));
        assert!(d.finalizing);
        assert!(!d.input_ready());
        assert_eq!(probe.reads.get(), 2);
        probe.completions.borrow()[1].complete(Ok(None));
        d.poll_external(&mut app);
        finish(&mut app, &mut c, &mut d);
        assert_eq!(
            app.last_effect_values.as_ref().unwrap()["zoom"],
            json!(1.01)
        );
        TestImageReads::clear();
    }
    /// R155: each real Pan/range/checkbox/Select key change is one acquisition; idle and Select opening/Escape are local.
    #[test]
    fn bb26_control_notifications_once() {
        for (control, pressed_key, expected) in [
            ("placement", Key::ArrowUp, json!([-0.99, -1.0])),
            ("zoom", Key::ArrowUp, json!(1.01)),
            ("nearest", Key::Space, json!(true)),
            ("method", Key::ArrowDown, json!("minimum")),
        ] {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            d.session
                .as_mut()
                .unwrap()
                .def
                .controls
                .retain(|c| c.key() == control);
            d.acquire_external(&app, &ctx, Some(&mut c));
            frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
            assert_eq!(probe.reads.get(), 1);
            if control == "placement" || control == "zoom" {
                focus_initial_number(&mut app, &mut c, &mut d, &ctx);
            }
            if control == "method" {
                frame(&mut app, &mut c, &mut d, &ctx, key(Key::Enter), false);
                assert_eq!(probe.reads.get(), 1);
            }
            assert_eq!(
                frame(&mut app, &mut c, &mut d, &ctx, key(pressed_key), false).0,
                DialogState::Open
            );
            if control == "method" {
                assert_eq!(probe.reads.get(), 1);
                assert_eq!(d.open_select.unwrap().highlight, 1);
                assert_eq!(
                    frame(&mut app, &mut c, &mut d, &ctx, key(Key::Enter), false).0,
                    DialogState::Open
                );
            }
            assert_eq!(d.values[control], expected, "{control}");
            assert_eq!(probe.reads.get(), 2, "{control}");
            frame(&mut app, &mut c, &mut d, &ctx, vec![], false);
            assert_eq!(probe.reads.get(), 2);
            if control == "method" {
                frame(&mut app, &mut c, &mut d, &ctx, key(Key::Enter), false);
                assert!(d.open_select.is_some());
                assert_eq!(probe.reads.get(), 2);
                assert_eq!(
                    frame(&mut app, &mut c, &mut d, &ctx, key(Key::Escape), false).0,
                    DialogState::Open
                );
                assert_eq!(probe.reads.get(), 2);
            }
        }
        TestImageReads::clear();
    }
    /// R155/BB25: popup open and unchanged Escape read zero; live edit, rollback and reset each acquire once at show's boundary.
    #[test]
    fn bb26_popup_events_once() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let s = d.session.as_mut().unwrap();
        s.def.controls = vec![Control::Color {
            key: "color",
            label: "Color",
            default: pdn_core::effect::ColorDefault::Rgb(0),
        }];
        d.values = s.defaults();
        d.acquire_external(&app, &ctx, Some(&mut c));
        frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
        frame(&mut app, &mut c, &mut d, &ctx, key(Key::Enter), false);
        assert!(d.color_popup.is_some());
        assert_eq!(probe.reads.get(), 1);
        assert_eq!(
            frame(&mut app, &mut c, &mut d, &ctx, key(Key::Escape), false).0,
            DialogState::Open
        );
        assert!(d.color_popup.is_none());
        assert_eq!(probe.reads.get(), 1);
        frame(&mut app, &mut c, &mut d, &ctx, key(Key::Space), false);
        assert!(d.color_popup.is_some());
        frame(
            &mut app,
            &mut c,
            &mut d,
            &ctx,
            vec![egui::Event::Text("00ff00".into())],
            false,
        );
        assert_eq!(d.values["color"], json!("#00ff00"));
        assert_eq!(probe.reads.get(), 2);
        frame(&mut app, &mut c, &mut d, &ctx, key(Key::Escape), false);
        assert_eq!(d.values["color"], json!("#000000"));
        assert_eq!(probe.reads.get(), 3);
        assert!(!d.finalizing);
        TestImageReads::clear();
    }
    /// R155: Drop/replacement/late completions never install; explicit parent Cancel keeps every bookkeeping field.
    #[test]
    fn bb26_drop_and_cancel_late_completion() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        let before = snapshot(&app);
        d.acquire_external(&app, &ctx, Some(&mut c));
        assert_eq!(
            with_cx(&mut app, &mut c, |cx| d.cancel(cx)),
            DialogState::Closed(DialogResult::Cancel)
        );
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        drop(d);
        let def = pdn_core::effect::find("bbPasteAlpha").unwrap();
        let session = EffectSession::begin(&mut app, def).unwrap();
        let values = session.defaults();
        let mut replacement = EffectConfigDialog::new(session, values);
        replacement.acquire_external(&app, &ctx, Some(&mut c));
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        replacement.poll_external(&mut app);
        assert!(!replacement.input_ready());
        assert_eq!(snapshot(&app), before);
        drop(replacement);
        probe.completions.borrow()[1].complete(Ok(Some(white())));
        assert_eq!(snapshot(&app), before);
        TestImageReads::clear();
    }

    /// R155: final-only Repeat freezes values, source, seed and original document before delayed acquisition.
    #[test]
    fn bb26_repeat_frozen_invocation() {
        for has_image in [false, true] {
            let probe = TestImageReads::install(true);
            let (mut app, d, mut c, ctx) = setup("bbPasteAlpha");
            let mut values = d.values.clone();
            values.insert("source".into(), json!("alpha"));
            values.insert("invert".into(), json!(true));
            let session = d.session.as_ref().unwrap().clone();
            let seed = session.env().instance_seed;
            let source = session.source().data.clone();
            let first = app.active.unwrap();
            let history = app.documents[first].history.undo.len();
            let mut d = EffectConfigDialog::final_only(session, values.clone());
            d.acquire_external(&app, &ctx, Some(&mut c));
            app.last_effect = Some("bbFillFromClipboard".into());
            app.last_effect_values = Some(Values::new());
            app.effect_tokens
                .insert("bbPasteAlpha".into(), Values::new());
            app.create_blank_document_in_new_workspace(3, 2, 96., true);
            let other = layer_bytes(&app, None);
            probe.completions.borrow()[0].complete(Ok(has_image.then_some(ImageData {
                width: 1,
                height: 1,
                data: vec![255, 255, 255, 64],
            })));
            d.poll_external(&mut app);
            assert_eq!(d.session.as_ref().unwrap().env().instance_seed, seed);
            finish(&mut app, &mut c, &mut d);
            let expected: Vec<u8> = if has_image {
                source
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .flat_map(|p| [p[0], p[1], p[2], 191])
                    .collect()
            } else {
                source
            };
            assert_eq!(
                app.documents[first]
                    .document
                    .layers
                    .get_at(0)
                    .unwrap()
                    .surface
                    .data(),
                expected
            );
            assert_eq!(layer_bytes(&app, None), other);
            assert_eq!(app.documents[first].history.undo.len(), history + 1);
            assert_eq!(app.effect_tokens["bbPasteAlpha"], values);
            assert_eq!(app.last_effect.as_deref(), Some("bbPasteAlpha"));
            assert_eq!(app.last_effect_values, Some(values));
            assert_eq!(probe.reads.get(), 1);
            TestImageReads::clear();
        }
    }
    /// R155: real reset/Preview pointer notifications acquire once, Help opening acquires none.
    #[test]
    fn bb26_reset_preview_and_help() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        d.session.as_mut().unwrap().def.controls = vec![Control::Color {
            key: "color",
            label: "Color",
            default: pdn_core::effect::ColorDefault::Rgb(0),
        }];
        d.values = d.session.as_ref().unwrap().defaults();
        d.values.insert("color".into(), json!("#123456"));
        d.acquire_external(&app, &ctx, Some(&mut c));
        let (_, rect) = frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
        let id = ctx.memory(|m| m.focused()).unwrap();
        let color = ctx.read_response(id).unwrap().rect;
        let (_, rw, gap) = EFFECT_LINE_COLUMNS;
        click(
            &mut app,
            &mut c,
            &mut d,
            &ctx,
            Pos2::new(color.max.x + gap + rw / 2., color.center().y),
        );
        assert_eq!(d.values["color"], json!("#000000"));
        assert_eq!(probe.reads.get(), 2);
        click(
            &mut app,
            &mut c,
            &mut d,
            &ctx,
            Pos2::new(rect.min.x + 25., rect.max.y - 20.),
        );
        assert!(!d.preview);
        assert_eq!(probe.reads.get(), 3);
        click(
            &mut app,
            &mut c,
            &mut d,
            &ctx,
            Pos2::new(rect.max.x - 69., rect.min.y + 15.),
        );
        assert_eq!(probe.reads.get(), 3);
        assert!(!d.finalizing);
        let pending = crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        assert_eq!(pending.len(), 1, "Help opens a local dialog");
        TestImageReads::clear();
    }
    /// R155: editable canceled service input stays retired through actual idle frames, until explicit OK.
    #[test]
    fn bb26_canceled_frames_never_retry() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let before = snapshot(&app);
        d.acquire_external(&app, &ctx, Some(&mut c));
        frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
        c.write_text("cancel read");
        for _ in 0..3 {
            assert_eq!(
                frame(&mut app, &mut c, &mut d, &ctx, vec![], false).0,
                DialogState::Open
            );
        }
        assert_eq!(probe.reads.get(), 1);
        assert!(matches!(d.input, ExternalInput::Retired));
        assert_eq!(snapshot(&app), before);
        with_cx(&mut app, &mut c, |cx| {
            assert_eq!(d.ok(cx, &ctx), DialogState::Open)
        });
        assert_eq!(probe.reads.get(), 2);
        c.write_text("cancel applying");
        assert_eq!(
            frame(&mut app, &mut c, &mut d, &ctx, vec![], false).0,
            DialogState::Closed(DialogResult::Cancel)
        );
        assert_eq!(snapshot(&app), before);
        TestImageReads::clear();
    }

    /// BB26 review I1: an uninterruptible native worker stays owned until it drains. Edits and a
    /// service cancellation still acquire at once, but start no second worker; then a pending OK
    /// renders only the latest generation, values and image, once, and the stale result never
    /// publishes.
    #[test]
    fn bb26_held_native_worker_defers_replacement() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        let before = snapshot(&app);
        let gray = |v: u8| ImageData {
            width: 1,
            height: 1,
            data: vec![v, v, v, 255],
        };
        d.acquire_external(&app, &ctx, Some(&mut c));
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        d.poll_external(&mut app);
        // The opening generation's worker, still rendering.
        let (held_tx, held_rx) = std::sync::mpsc::channel();
        let held_flag = Arc::new(AtomicBool::new(false));
        let held = d.generation;
        d.job = Some(held_rx);
        d.job_cancel = Some(held_flag.clone());
        d.due = None;
        let held_only = |d: &EffectConfigDialog| {
            d.job.is_some()
                && d.job_cancel
                    .as_ref()
                    .is_some_and(|f| Arc::ptr_eq(f, &held_flag))
        };
        for (i, read) in [Some(40), None, Some(80), Some(120)]
            .into_iter()
            .enumerate()
        {
            d.values.insert("invert".into(), json!(i % 2 == 0));
            with_cx(&mut app, &mut c, |cx| d.notify(cx, &ctx, true));
            assert_eq!(probe.reads.get(), i + 2, "edit {i} acquires at once");
            if i == 3 {
                // The last edit's read is still pending when OK is pressed.
                with_cx(&mut app, &mut c, |cx| {
                    assert_eq!(d.ok(cx, &ctx), DialogState::Open)
                });
                assert!(d.finalizing);
            }
            match read {
                Some(v) => probe.completions.borrow()[i + 1].complete(Ok(Some(gray(v)))),
                None => probe.completions.borrow()[i + 1].cancel(),
            }
            d.poll_external(&mut app);
            assert_eq!(d.input_ready(), read.is_some(), "edit {i}");
            if let Some(v) = read {
                let image = d.session.as_ref().unwrap().env().image.cloned();
                assert_eq!(image, Some(gray(v)), "edit {i} holds its own input");
            }
            with_cx(&mut app, &mut c, |cx| d.run_previews(cx, &ctx));
            assert!(held_flag.load(Ordering::Relaxed));
            assert!(held_only(&d), "edit {i} started a second native worker");
            assert!(d.shown.is_none() && !d.failed);
            assert_eq!(snapshot(&app), before);
        }
        assert_eq!(probe.reads.get(), 5);
        // The held worker exits with its stale result: discarded, then one latest worker starts.
        held_tx
            .send(JobResult {
                generation: held,
                result: Some((
                    ImageData {
                        width: 3,
                        height: 2,
                        data: vec![7; 24],
                    },
                    Some("stale".into()),
                )),
                elapsed: 1.0,
            })
            .unwrap();
        drop(held_tx);
        with_cx(&mut app, &mut c, |cx| d.run_previews(cx, &ctx));
        assert!(d.job.is_some() && !held_only(&d) && d.due.is_none());
        assert!(d.shown.is_none() && d.pending_notice.is_none() && !d.failed);
        assert_eq!(snapshot(&app), before);
        let latest = d.session.as_ref().unwrap().render_with(&d.values, None).0;
        assert_ne!(latest.data, before.0);
        // Wait for that worker without a timing loop, then deliver it through the same receiver.
        let rx = d.job.take().unwrap();
        let result = rx.recv().unwrap();
        assert!(rx.recv().is_err(), "one result per worker");
        assert_eq!(result.generation, d.generation);
        assert_eq!(result.result.as_ref().unwrap().0, latest);
        let (tx, rx) = std::sync::mpsc::channel();
        tx.send(result).unwrap();
        d.job = Some(rx);
        with_cx(&mut app, &mut c, |cx| {
            d.run_previews(cx, &ctx);
            assert!(d.job.is_none() && d.due.is_none(), "rendered once");
            assert_eq!(
                d.finish_ready(cx),
                Some(DialogState::Closed(DialogResult::Ok))
            );
        });
        assert_eq!(layer_bytes(&app, None), latest.data);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), before.2 + 1);
        assert_eq!(probe.reads.get(), 5);
        TestImageReads::clear();
    }

    /// BB26 review I1: a retired worker lost without a result cannot fail a pending final.
    #[test]
    fn bb26_lost_stale_worker_keeps_pending_ok() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = setup("bbPasteAlpha");
        d.acquire_external(&app, &ctx, Some(&mut c));
        probe.completions.borrow()[0].complete(Ok(Some(white())));
        d.poll_external(&mut app);
        let (tx, rx) = std::sync::mpsc::channel::<JobResult>();
        d.job = Some(rx);
        d.job_cancel = Some(Arc::new(AtomicBool::new(false)));
        with_cx(&mut app, &mut c, |cx| {
            d.notify(cx, &ctx, true);
            assert_eq!(d.ok(cx, &ctx), DialogState::Open)
        });
        probe.completions.borrow()[1].complete(Ok(Some(white())));
        d.poll_external(&mut app);
        drop(tx);
        with_cx(&mut app, &mut c, |cx| {
            d.run_previews(cx, &ctx);
            assert!(!d.failed);
            assert!(d.finish_ready(cx).is_none());
        });
        assert!(d.job.is_some(), "the latest generation renders");
        TestImageReads::clear();
    }

    /// BB33: Text Fun Factory over a 40 × 30 white layer: one flat 30 px block over x 0…18 and
    /// y 2.9…39 (fully covering (9, 15)), on Image ▸ "Use Image on Clipboard", then `extra`.
    pub(in crate::dialogs::effect_config) fn tff(
        extra: Value,
    ) -> (
        pdn_core::app::AppWorkspace,
        EffectConfigDialog,
        Clipboard,
        egui::Context,
    ) {
        let mut app = fresh_doc(40, 30, None);
        let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
        let s = EffectSession::begin(&mut app, def).unwrap();
        let mut values = s.defaults();
        for x in [
            json!({"text": "█", "font": "Noto Sans Mono", "size": 30, "offsetX": 0,
                "depth": 0, "outlineSize": 0, "shadowLength": 0, "fillTab": 3,
                "texture": "t49"}),
            extra,
        ] {
            values.extend(x.as_object().unwrap().clone());
        }
        (
            app,
            EffectConfigDialog::new(s, values),
            Clipboard::in_memory(),
            egui::Context::default(),
        )
    }
    /// A 3 × 2 opaque clipboard image of distinct midtones.
    pub(in crate::dialogs::effect_config) fn tile() -> ImageData {
        ImageData {
            width: 3,
            height: 2,
            data: (0..6u8)
                .flat_map(|i| [40 + 30 * i, 200 - 25 * i, 90 + 7 * i, 255])
                .collect(),
        }
    }
    /// The active layer's pixel (x, y) is `want` ±2 per channel.
    #[track_caller]
    pub(super) fn assert_layer_px(
        app: &pdn_core::app::AppWorkspace,
        (x, y): (usize, usize),
        want: [u8; 4],
    ) {
        let bytes = layer_bytes(app, None);
        let w = app.active_ws().unwrap().document.width as usize;
        let got = &bytes[(y * w + x) * 4..][..4];
        assert!(
            got.iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 2),
            "{got:?} want {want:?}"
        );
    }
    /// (9, 15) on `tile()`: texel (0, 1).
    pub(super) const TILE_AT_9_15: [u8; 4] = [130, 125, 111, 255];
    fn edit(
        app: &mut pdn_core::app::AppWorkspace,
        c: &mut Clipboard,
        d: &mut EffectConfigDialog,
        ctx: &egui::Context,
        key: &str,
        v: Value,
    ) {
        d.values.insert(key.into(), v);
        with_cx(app, c, |cx| d.notify(cx, ctx, true));
    }
    fn image(d: &EffectConfigDialog) -> Option<ImageData> {
        d.session.as_ref().unwrap().env().image.cloned()
    }

    /// R155 / BB33: the current values decide the source on every notification. Idle frames and
    /// a remembered clipboard texture on Solid read nothing; switching to Image ▸ clipboard reads
    /// once; switching away retires that read (its late image is ignored, nothing waits);
    /// switching back reads afresh; every change while active (Hue, the outer tab) reads again.
    #[test]
    fn bb33_fill_switches_acquire_and_retire_once() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = tff(json!({"fillTab": 0}));
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        d.acquire_external(&app, &ctx, Some(&mut c));
        frame(&mut app, &mut c, &mut d, &ctx, vec![], true);
        frame(&mut app, &mut c, &mut d, &ctx, vec![], false);
        assert_eq!(probe.reads.get(), 0);
        assert!(d.input_ready());
        edit(&mut app, &mut c, &mut d, &ctx, "fillTab", json!(3));
        assert_eq!(probe.reads.get(), 1);
        assert!(!d.input_ready());
        for _ in 0..3 {
            frame(&mut app, &mut c, &mut d, &ctx, vec![], false);
        }
        assert_eq!(probe.reads.get(), 1);
        edit(&mut app, &mut c, &mut d, &ctx, "texture", json!("t3"));
        assert_eq!(probe.reads.get(), 1);
        assert!(d.input_ready());
        probe.completions.borrow()[0].complete(Ok(Some(tile())));
        d.poll_external(&mut app);
        assert!(d.input_ready());
        assert_eq!(image(&d), None);
        edit(&mut app, &mut c, &mut d, &ctx, "texture", json!("t49"));
        assert_eq!(probe.reads.get(), 2);
        assert!(!d.input_ready());
        probe.completions.borrow()[1].complete(Ok(Some(tile())));
        d.poll_external(&mut app);
        assert_eq!(image(&d), Some(tile()));
        edit(&mut app, &mut c, &mut d, &ctx, "textureHue", json!(10));
        assert_eq!(probe.reads.get(), 3);
        assert_eq!(image(&d), None);
        edit(&mut app, &mut c, &mut d, &ctx, "tab", json!(0));
        assert_eq!(probe.reads.get(), 4);
        edit(&mut app, &mut c, &mut d, &ctx, "fillTab", json!(2));
        assert!(d.input_ready());
        probe.completions.borrow()[3].complete(Ok(Some(tile())));
        d.poll_external(&mut app);
        assert_eq!(image(&d), None);
        for _ in 0..3 {
            frame(&mut app, &mut c, &mut d, &ctx, vec![], false);
        }
        assert_eq!(probe.reads.get(), 4);
        TestImageReads::clear();
    }

    /// BB33 through the real consumer: a completed image renders at its own size; an empty or
    /// rejected read (with no internal image) clears the old image and renders the Solid fill;
    /// a fully transparent image is an image; an image after an empty read recovers.
    #[test]
    fn bb33_completed_reads_render_the_image_or_solid() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = tff(json!({}));
        let solid = {
            let mut v = d.values.clone();
            v.insert("fillTab".into(), json!(0));
            d.session.as_ref().unwrap().render(&v)
        };
        let transparent = ImageData {
            width: 2,
            height: 1,
            data: vec![255, 0, 0, 0, 0, 255, 0, 0],
        };
        d.acquire_external(&app, &ctx, Some(&mut c));
        for (i, result) in [
            Ok(None),
            Ok(Some(tile())),
            Err("denied".to_string()),
            Ok(Some(transparent)),
            Ok(None),
        ]
        .into_iter()
        .enumerate()
        {
            if i > 0 {
                with_cx(&mut app, &mut c, |cx| d.notify(cx, &ctx, true));
            }
            let kind = format!("{result:?}").chars().take(12).collect::<String>();
            probe.completions.borrow()[i].complete(result);
            d.poll_external(&mut app);
            assert!(d.input_ready(), "{kind}");
            let out = d.session.as_ref().unwrap().render(&d.values);
            let p = &out.data[(15 * 40 + 9) * 4..][..4];
            match i {
                1 => assert!(
                    p.iter().zip(TILE_AT_9_15).all(|(g, w)| g.abs_diff(w) <= 2),
                    "{p:?}"
                ),
                3 => assert_eq!(p[3], 0),
                _ => assert_eq!(out, solid, "{kind}"),
            }
        }
        assert_eq!(probe.reads.get(), 5);
        TestImageReads::clear();
    }

    /// R155 / BB33: OK while the read is pending, with Preview on or off, waits for that one
    /// read (pressed again, still one), then commits its image once.
    #[test]
    fn bb33_pending_ok_commits_the_read_image_once() {
        for preview in [false, true] {
            let probe = TestImageReads::install(true);
            let (mut app, mut d, mut c, ctx) = tff(json!({}));
            d.preview = preview;
            let before = snapshot(&app);
            d.acquire_external(&app, &ctx, Some(&mut c));
            for _ in 0..2 {
                assert_eq!(
                    with_cx(&mut app, &mut c, |cx| d.ok(cx, &ctx)),
                    DialogState::Open
                );
            }
            assert_eq!(snapshot(&app), before);
            assert_eq!(probe.reads.get(), 1);
            probe.completions.borrow()[0].complete(Ok(Some(tile())));
            d.poll_external(&mut app);
            finish(&mut app, &mut c, &mut d);
            assert_layer_px(&app, (9, 15), TILE_AT_9_15);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            assert!(with_cx(&mut app, &mut c, |cx| d.finish_ready(cx)).is_none());
            assert_eq!(app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            assert_eq!(
                app.last_effect_values.as_ref().unwrap()["texture"],
                json!("t49")
            );
            assert_eq!(probe.reads.get(), 1);
        }
        TestImageReads::clear();
    }

    /// BB33: switching to a built-in texture while the read is pending unblocks OK at once; the
    /// late completion changes nothing.
    #[test]
    fn bb33_switch_away_while_pending_unblocks_ok() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d, mut c, ctx) = tff(json!({}));
        let before = snapshot(&app);
        d.acquire_external(&app, &ctx, Some(&mut c));
        edit(&mut app, &mut c, &mut d, &ctx, "texture", json!("t3"));
        let want = d.session.as_ref().unwrap().render(&d.values);
        assert_eq!(
            with_cx(&mut app, &mut c, |cx| d.ok(cx, &ctx)),
            DialogState::Closed(DialogResult::Ok)
        );
        let after = snapshot(&app);
        assert_eq!(after.0, want.data);
        assert_eq!(after.2, before.2 + 1);
        probe.completions.borrow()[0].complete(Ok(Some(tile())));
        d.poll_external(&mut app);
        assert_eq!(snapshot(&app), after);
        assert_eq!(probe.reads.get(), 1);
        TestImageReads::clear();
    }

    /// R155 / BB33: final-only Repeat reads afresh only for remembered Image ▸ clipboard values,
    /// commits nothing while that read is pending, then commits its image once; a remembered
    /// clipboard texture on Solid reads nothing and commits the Solid fill.
    #[test]
    fn bb33_repeat_reads_only_for_the_image_fill() {
        for fill_tab in [3, 0] {
            let probe = TestImageReads::install(true);
            let (mut app, d, mut c, ctx) = tff(json!({"fillTab": fill_tab}));
            let (session, values) = (d.session.as_ref().unwrap().clone(), d.values.clone());
            let before = snapshot(&app);
            let mut d = EffectConfigDialog::final_only(session, values.clone());
            d.acquire_external(&app, &ctx, Some(&mut c));
            let image_fill = fill_tab == 3;
            assert_eq!(probe.reads.get(), usize::from(image_fill));
            assert_eq!(d.input_ready(), !image_fill);
            if image_fill {
                assert!(with_cx(&mut app, &mut c, |cx| d.finish_ready(cx)).is_none());
                assert_eq!(snapshot(&app), before);
                probe.completions.borrow()[0].complete(Ok(Some(tile())));
                d.poll_external(&mut app);
            }
            finish(&mut app, &mut c, &mut d);
            let want = if image_fill {
                TILE_AT_9_15
            } else {
                [255, 0, 0, 255]
            };
            assert_layer_px(&app, (9, 15), want);
            assert_eq!(app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            let mut remembered = values.clone();
            remembered.remove("fallbackFont");
            remembered.remove("fallbackFamily");
            assert_eq!(app.last_effect_values, Some(remembered));
            assert_eq!(probe.reads.get(), usize::from(image_fill));
            TestImageReads::clear();
        }
    }
}

#[cfg(test)]
mod file_tests {
    //! BB27 (fill-from-file.md, R156): the real File source consumer over in-memory Files.
    use super::super::*;
    use super::*;
    use crate::platform::{Clipboard, Files, PickedFile};
    use pdn_core::app::AppWorkspace;
    use pdn_core::testing::{fresh_doc, layer_bytes};
    use std::path::Path;

    const R: [u8; 4] = [255, 0, 0, 255];
    const G: [u8; 4] = [0, 255, 0, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const K: [u8; 4] = [0, 0, 0, 255];
    const W: [u8; 4] = [255; 4];
    const TITLE: &str = "Fill From File";
    /// fill-from-file.md Expected results 2, the layout case 6 must match.
    const CASE_2: [&str; 8] = [
        "WWWWWWWW", "WWWWWWWW", "WWWWWWWW", "WWWRGWWW", "WWWBKWWW", "WWWWWWWW", "WWWWWWWW",
        "WWWWWWWW",
    ];

    fn map(rows: &[&str]) -> Vec<u8> {
        rows.iter()
            .flat_map(|r| r.chars())
            .flat_map(|c| match c {
                'R' => R,
                'G' => G,
                'B' => B,
                'K' => K,
                'W' => W,
                _ => panic!("colour"),
            })
            .collect()
    }
    fn image(w: u32, h: u32, px: &[[u8; 4]]) -> ImageData {
        ImageData {
            width: w,
            height: h,
            data: px.concat(),
        }
    }
    /// The spec's 2 × 2 image: red, green / blue, black.
    fn quad() -> ImageData {
        image(2, 2, &[R, G, B, K])
    }
    fn encoded(img: &ImageData, format: &str) -> Vec<u8> {
        use pdn_core::io::encode::{SaveOptions, encode};
        encode(img, format, &SaveOptions::default_for(format)).unwrap()
    }
    fn write(dir: &Path, name: &str, bytes: &[u8]) -> String {
        let p = dir.join(name);
        std::fs::write(&p, bytes).unwrap();
        p.to_string_lossy().into_owned()
    }
    fn picked(name: &str, img: &ImageData) -> PickedFile {
        PickedFile::snapshot(name.into(), encoded(img, "png"))
    }
    type Snapshot = (
        Vec<u8>,
        bool,
        usize,
        std::collections::HashMap<String, Values>,
        Option<String>,
        Option<Values>,
    );

    /// An 8 × 8 opaque white layer with From File open (nearest on), its Files and attachment.
    struct Fx {
        id: &'static str,
        app: AppWorkspace,
        d: EffectConfigDialog,
        c: Clipboard,
        ctx: egui::Context,
        files: Files,
        source: FileSource,
        dir: tempfile::TempDir,
    }
    impl Fx {
        fn new() -> Self {
            Self::open("bbFillFromFile", |v| {
                v.insert("nearest".into(), json!(true));
            })
        }
        /// `id`'s dialog over the layer, its defaults edited by `edit`.
        fn open(id: &'static str, edit: impl FnOnce(&mut Values)) -> Self {
            let mut app = fresh_doc(8, 8, None);
            let def = pdn_core::effect::find(id).unwrap();
            let s = EffectSession::begin(&mut app, def).unwrap();
            let mut values = s.defaults();
            edit(&mut values);
            let files = Files::in_memory();
            let source = FileSource::default();
            let mut d = EffectConfigDialog::new(s, values);
            d.files = files.clone();
            d.attach_file_source(source.clone());
            let ctx = egui::Context::default();
            crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
            Fx {
                id,
                app,
                d,
                c: Clipboard::in_memory(),
                ctx,
                files,
                source,
                dir: tempfile::tempdir().unwrap(),
            }
        }
        /// A fresh invocation from the app's remembered token (`repeat`: last values, final only),
        /// capturing the same attachment, as Shell does.
        fn reopen(&mut self, repeat: bool) {
            let def = pdn_core::effect::find(self.id).unwrap();
            let s = EffectSession::begin(&mut self.app, def).unwrap();
            let mut d = if repeat {
                let v = self.app.last_effect_values.clone().unwrap_or_default();
                EffectConfigDialog::final_only(s, v)
            } else {
                let v = s.initial_values(&self.app);
                EffectConfigDialog::new(s, v)
            };
            d.files = self.files.clone();
            d.attach_file_source(self.source.clone());
            self.d = d;
        }
        fn path(&self, name: &str) -> String {
            self.dir.path().join(name).to_string_lossy().into_owned()
        }
        fn write(&self, name: &str, bytes: &[u8]) -> String {
            write(self.dir.path(), name, bytes)
        }
        fn cx<T>(
            &mut self,
            f: impl FnOnce(&mut EffectConfigDialog, &mut DialogCx, &egui::Context) -> T,
        ) -> T {
            let Fx { app, d, c, ctx, .. } = self;
            f(
                d,
                &mut DialogCx {
                    app,
                    settings: &mut pdn_core::settings::AppSettings::load(None),
                    prefs: &mut crate::prefs::Prefs::default(),
                    settings_changed: false,
                    shortcuts: None,
                    clipboard: Some(c),
                    settings_imports: None,
                    fonts: None,
                    asset_library: None,
                    seat_focus: false,
                },
                ctx,
            )
        }
        fn acquire(&mut self) {
            let Fx { app, d, c, ctx, .. } = self;
            d.acquire_external(app, ctx, Some(c));
        }
        /// An accepted value event: `key` = `v`, then the dialog's own notification.
        fn edit(&mut self, key: &str, v: Value) {
            self.d.values.insert(key.into(), v);
            self.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        }
        fn poll(&mut self) {
            self.d.poll_external(&mut self.app);
        }
        fn image(&self) -> Option<ImageData> {
            self.d.session.as_ref().unwrap().env().image.cloned()
        }
        fn render(&self) -> Vec<u8> {
            self.d.session.as_ref().unwrap().render(&self.d.values).data
        }
        fn ok(&mut self) -> DialogState {
            self.cx(|d, cx, ctx| d.ok(cx, ctx))
        }
        /// Publishes the current generation's render as its preview.
        fn finish_preview(&mut self) {
            let result = self
                .d
                .session
                .as_ref()
                .unwrap()
                .render_with(&self.d.values, None);
            let generation = self.d.generation;
            self.d.due = None;
            self.cx(|d, cx, _| {
                assert!(d.accept_result(
                    cx,
                    JobResult {
                        generation,
                        result: Some(result),
                        elapsed: 1.0
                    }
                ))
            });
        }
        /// The current preview, then a pending final's acceptance of it, else OK reusing it.
        fn finish(&mut self) -> Option<DialogState> {
            self.finish_preview();
            if self.d.finalizing {
                return self.cx(|d, cx, _| d.finish_ready(cx));
            }
            Some(self.ok())
        }
        fn frame(&mut self, events: Vec<egui::Event>, seat: bool) -> DialogState {
            self.frame_at(events, seat).0
        }
        /// One real frame; also the dialog's frame rect.
        fn frame_at(&mut self, events: Vec<egui::Event>, seat: bool) -> (DialogState, Rect) {
            let ctx = self.ctx.clone();
            let mut result = (DialogState::Open, Rect::NOTHING);
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280., 800.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    result.0 = self.cx(|d, cx, _| {
                        cx.seat_focus = seat;
                        d.show(ui, cx)
                    });
                    result.1 = DialogFrameState::get(&ctx, ui.scope_id()).rect;
                },
            );
            out.textures_delta.clear();
            result
        }
        /// A ready worker result for the current generation, as a finished final render.
        fn ready_final(&mut self, pixels: ImageData) {
            let (tx, rx) = std::sync::mpsc::channel();
            tx.send(JobResult {
                generation: self.d.generation,
                result: Some((pixels, None)),
                elapsed: 1.0,
            })
            .unwrap();
            self.d.job = Some(rx);
            self.d.job_cancel = Some(Arc::new(AtomicBool::new(false)));
        }
        fn snapshot(&self) -> Snapshot {
            let w = self.app.active_ws().unwrap();
            (
                layer_bytes(&self.app, None),
                w.dirty,
                w.history.undo.len(),
                self.app.effect_tokens.clone(),
                self.app.last_effect.clone(),
                self.app.last_effect_values.clone(),
            )
        }
        fn attached(&self) -> Option<(String, u64)> {
            self.source
                .borrow()
                .as_ref()
                .map(|a| (a.value.clone(), a.file.id))
        }
        fn log(&self) -> Vec<&'static str> {
            self.files.probe.log.borrow().clone()
        }
    }
    fn key(key: Key) -> Vec<egui::Event> {
        [true, false]
            .map(|pressed| egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            })
            .to_vec()
    }

    /// fill-from-file.md Expected results 1: an empty and a missing path each resolve, silently,
    /// to no image; the whole white layer stays, and OK is the ordinary no-op success.
    #[test]
    fn bb27_expected_1_empty_and_missing() {
        for missing in [false, true] {
            let mut f = Fx::new();
            assert_eq!(layer_bytes(&f.app, None), map(&["WWWWWWWW"; 8]));
            if missing {
                let path = f.path("missing.png");
                f.d.values.insert("file".into(), json!(path));
            }
            f.acquire();
            assert!(f.d.input_ready());
            assert_eq!((f.image(), f.d.file_error.clone()), (None, None));
            assert_eq!(f.render(), map(&["WWWWWWWW"; 8]));
            assert_eq!(f.log(), if missing { vec!["read"] } else { vec![] });
            let before = f.snapshot();
            assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
            assert_eq!(layer_bytes(&f.app, None), before.0);
            assert_eq!(f.app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            assert_eq!(f.attached(), None);
        }
    }
    /// fill-from-file.md Behaviour "Path empty, nonexistent, or a folder": a directory and a
    /// path under a file are also silent no-image results.
    #[test]
    fn bb27_directory_is_silent_none() {
        let mut f = Fx::new();
        let file = f.write("a.png", &encoded(&quad(), "png"));
        for path in [
            f.dir.path().to_string_lossy().into_owned(),
            format!("{file}/x.png"),
        ] {
            f.edit("file", json!(path));
            assert!(f.d.input_ready());
            assert_eq!((f.image(), f.d.file_error.clone()), (None, None));
        }
    }
    /// fill-from-file.md Expected results 5: an existing notes.txt raises ".txt is not supported."
    /// inline, clears the previous image and applies no fill; OK writes nothing; the next
    /// supported file recovers and clears the message.
    #[test]
    fn bb27_expected_5_unsupported_and_recovery() {
        let mut f = Fx::new();
        let png = f.write("a.png", &encoded(&quad(), "png"));
        let notes = f.write("notes.txt", b"hi");
        f.d.values.insert("file".into(), json!(png));
        f.acquire();
        assert_eq!(f.image(), Some(quad()));
        f.finish_preview();
        assert_eq!(layer_bytes(&f.app, None), map(&CASE_2));
        let before = f.snapshot();
        f.edit("file", json!(notes));
        assert!(matches!(f.d.input, ExternalInput::Failed));
        assert_eq!(f.d.file_error.as_deref(), Some(".txt is not supported."));
        assert_eq!(f.image(), None);
        assert_eq!(layer_bytes(&f.app, None), map(&["WWWWWWWW"; 8]));
        assert_eq!(f.ok(), DialogState::Open);
        assert!(!f.d.finalizing);
        assert_eq!(f.snapshot().2, before.2);
        assert_eq!(f.app.effect_tokens.get("bbFillFromFile"), None);
        assert_eq!(f.attached(), None);
        // The message is laid out under the row, so the rows below it move down.
        let painter = f.ctx.layer_painter(egui::LayerId::background());
        let ind = f.d.session.as_ref().unwrap().def.indirect.unwrap();
        let controls = f.d.controls();
        let with = f.d.laid_rows(&painter, ind, &controls, 300.0);
        f.edit("file", json!(png));
        assert_eq!((f.d.file_error.clone(), f.image()), (None, Some(quad())));
        let without = f.d.laid_rows(&painter, ind, &controls, 300.0);
        let line = FontSpec::new(DIALOG_FONT_SIZE).resolved_line_height(painter.pixels_per_point());
        assert_eq!(with[0].height, without[0].height + EFFECT_ROW_GAP + line);
        assert_eq!(with[1].height, without[1].height);
        assert_eq!(f.render(), map(&CASE_2));
    }
    /// fill-from-file.md Expected results 6: the same image as .bmp, and as a typed .tga outside
    /// the chooser filter, renders case 2 on every pixel.
    #[test]
    fn bb27_expected_6_other_built_in_formats() {
        for format in ["bmp", "tga"] {
            let mut f = Fx::new();
            let path = f.write(&format!("a.{format}"), &encoded(&quad(), format));
            f.d.values.insert("file".into(), json!(path));
            f.acquire();
            assert_eq!(f.image(), Some(quad()), "{format}");
            assert_eq!(f.render(), map(&CASE_2), "{format}");
        }
    }
    /// R156: the final component's extension alias is checked before content; valid PNG bytes
    /// under .txt, .TXT, .json or no extension fail with the extension as written and are never
    /// decoded; a corrupt supported file reports the decoder's own error.
    #[test]
    fn bb27_extension_admission_before_content() {
        let mut f = Fx::new();
        let png = encoded(&quad(), "png");
        std::fs::create_dir(f.dir.path().join("dir.png")).unwrap();
        for (name, error) in [
            ("notes.txt", ".txt"),
            ("NOTES.TXT", ".TXT"),
            ("data.json", ".json"),
            ("noext", ""),
            ("dir.png/noext", ""),
        ] {
            let path = f.write(name, &png);
            f.edit("file", json!(path));
            assert_eq!(
                f.d.file_error.clone(),
                Some(format!("{error} is not supported.")),
                "{name}"
            );
            assert_eq!(f.log().last(), Some(&"read"), "{name} was decoded");
        }
        let upper = f.write("UPPER.PNG", &png);
        f.edit("file", json!(upper));
        assert_eq!((f.image(), f.d.file_error.clone()), (Some(quad()), None));
        let bad = f.write("bad.png", b"not a png");
        f.edit("file", json!(bad));
        let error = f.d.file_error.clone().unwrap();
        assert!(
            !error.is_empty() && !error.ends_with("is not supported."),
            "{error}"
        );
        assert_eq!(f.image(), None);
    }
    /// fill-from-file.md Behaviour "Existing file": a .pdn is flattened the way Paint.NET flattens
    /// (hidden layer skipped, opacity and blend mode applied) at its native size, whatever its DPI.
    #[test]
    fn bb27_pdn_flattened_at_native_size() {
        use pdn_core::document::{Document, layer::BlendMode, layer::Layer};
        let mut doc = Document::new(3, 2, 300.0);
        for (i, (c, visible, opacity, mode)) in [
            ([200, 100, 50, 255], true, 255, BlendMode::Normal),
            ([0, 255, 0, 255], false, 255, BlendMode::Normal),
            ([40, 80, 255, 255], true, 128, BlendMode::Multiply),
        ]
        .into_iter()
        .enumerate()
        {
            let mut layer = Layer::create_layer(3, 2, &format!("L{i}"));
            for p in layer.surface.data_mut().chunks_mut(4) {
                p.copy_from_slice(&c);
            }
            layer.properties.visible = visible;
            layer.properties.opacity = opacity;
            layer.properties.blend_mode = mode;
            doc.add_layer(layer);
        }
        let flat = doc.composite().get_image_data(0, 0, 3, 2);
        let mut f = Fx::new();
        let path = f.write("layers.pdn", &pdn_core::io::pdn::encode(&doc).unwrap());
        f.d.values.insert("file".into(), json!(path));
        f.acquire();
        let image = f.image().unwrap();
        assert_eq!((image.width, image.height), (3, 2));
        assert_eq!(image, flat);
        assert_ne!(&image.data[..4], &[200, 100, 50, 255]);
        assert_ne!(&image.data[..4], &[40, 80, 255, 255]);
    }
    /// fill-from-file.md Behaviour "loaded from disk again on every settings change" and Notes
    /// "Repeat reloads the same path": a replaced file shows on the next change and on Repeat,
    /// one read per acquisition and never one per idle frame.
    #[test]
    fn bb27_native_disk_replacement_and_repeat() {
        let mut f = Fx::new();
        let path = f.write("a.png", &encoded(&quad(), "png"));
        f.d.values.insert("file".into(), json!(path));
        f.acquire();
        assert_eq!(f.image(), Some(quad()));
        std::fs::write(&path, encoded(&image(1, 1, &[R]), "png")).unwrap();
        for _ in 0..3 {
            f.frame(vec![], false);
        }
        assert_eq!(f.d.acquisitions, 1);
        assert_eq!(f.image(), Some(quad()));
        f.edit("zoom", json!(2));
        assert_eq!(f.d.acquisitions, 2);
        assert_eq!(f.image(), Some(image(1, 1, &[R])));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(f.log(), ["read", "decode", "read", "decode"]);
        std::fs::write(&path, encoded(&image(1, 1, &[B]), "png")).unwrap();
        let before = f.snapshot();
        f.reopen(true);
        f.acquire();
        assert_eq!(f.image(), Some(image(1, 1, &[B])));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        let mut want = map(&["WWWWWWWW"; 8]);
        // Zoom 2 about c = (4, 4): the 1 × 1 copy covers x, y 4..6.
        for (x, y) in [(4, 4), (5, 4), (4, 5), (5, 5)] {
            want[(y * 8 + x) * 4..][..4].copy_from_slice(&B);
        }
        assert_eq!(layer_bytes(&f.app, None), want);
        assert_eq!(f.app.active_ws().unwrap().history.undo.len(), before.2 + 1);
        assert!(!f.log().contains(&"choose"), "Repeat never opens a chooser");
    }
    /// R156: Tab reaches the accessible Browse button, whose Space starts one chooser in its
    /// activation; same-name picks are distinct sources, each notified once; a successful OK
    /// publishes its pick, which a reopened dialog and Repeat reuse, and a canceled pick does not.
    #[test]
    fn bb27_same_name_picks_and_attachment() {
        let mut f = Fx::new();
        let (a, b) = (
            picked("same.png", &quad()),
            picked("same.png", &image(1, 1, &[R])),
        );
        assert_ne!(a.id, b.id);
        f.files.queue_pick(Ok(vec![a.clone()]));
        f.files.queue_pick(Ok(vec![b.clone()]));
        f.acquire();
        f.frame(vec![], true);
        let field = f.ctx.memory(|m| m.focused()).unwrap();
        f.frame(key(Key::Tab), false);
        let browse = f.ctx.memory(|m| m.focused()).unwrap();
        assert_ne!(field, browse);
        assert_eq!(f.d.acquisitions, 1);
        f.frame(key(Key::Space), false);
        assert_eq!(f.d.values["file"], json!("same.png"));
        assert_eq!(f.d.candidate.as_ref().map(|c| c.id), Some(a.id));
        assert_eq!((f.d.acquisitions, f.image()), (2, Some(quad())));
        f.frame(key(Key::Space), false);
        assert_eq!(f.d.values["file"], json!("same.png"));
        assert_eq!(f.d.candidate.as_ref().map(|c| c.id), Some(b.id));
        assert_eq!((f.d.acquisitions, f.image()), (3, Some(image(1, 1, &[R]))));
        assert_eq!(f.log(), ["choose", "decode", "choose", "decode"]);
        assert_eq!(f.attached(), None, "only a successful commit publishes");
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(f.attached(), Some(("same.png".into(), b.id)));
        f.reopen(false);
        f.acquire();
        assert_eq!(f.image(), Some(image(1, 1, &[R])));
        let c = picked("same.png", &image(1, 1, &[G]));
        f.files.queue_pick(Ok(vec![c]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), Some(true));
        f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        assert_eq!(f.image(), Some(image(1, 1, &[G])));
        f.cx(|d, cx, _| d.cancel(cx));
        assert_eq!(f.attached(), Some(("same.png".into(), b.id)));
        f.reopen(true);
        f.acquire();
        assert_eq!(f.image(), Some(image(1, 1, &[R])));
    }
    /// I1: Color Lookup's File source reads a `.cube` picked through its control and applies it
    /// (the white layer inverted to black). A bad file shows the parser's error under the row and
    /// leaves the layer as it was, and OK waits; another type is refused by its extension; a
    /// typed path to a good file recovers.
    #[test]
    fn color_lookup_file_source_reads_a_cube() {
        const INVERSE: &str =
            "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";
        let mut f = Fx::open("pcColorLookup", |v| {
            v.insert("source".into(), json!("file"));
        });
        let cube = PickedFile::snapshot("inverse.cube".into(), INVERSE.into());
        f.files.queue_pick(Ok(vec![cube]));
        f.acquire();
        assert_eq!(f.render(), map(&["WWWWWWWW"; 8]), "nothing read yet");
        f.d.chooser = Some(f.files.choose_open_with(false, ".cube,.3dl,.look", false));
        assert_eq!(f.d.poll_chooser(), Some(true));
        f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        assert!(matches!(f.d.input, ExternalInput::Resolved));
        assert_eq!(f.render(), map(&["KKKKKKKK"; 8]));
        // Typed paths from here: a typed edit detaches the pick (R156).
        f.d.candidate = None;
        let bad = f.write("bad.cube", b"LUT_3D_SIZE 2\n0 0 0\n");
        f.edit("file", json!(bad));
        assert!(matches!(f.d.input, ExternalInput::Failed));
        assert_eq!(
            f.d.file_error.as_deref(),
            Some("expected 8 entries, found 1")
        );
        assert_eq!(f.render(), map(&["WWWWWWWW"; 8]));
        assert_eq!(layer_bytes(&f.app, None), map(&["WWWWWWWW"; 8]));
        assert_eq!(f.ok(), DialogState::Open);
        let notes = f.write("notes.txt", b"LUT_3D_SIZE 2");
        f.edit("file", json!(notes));
        assert_eq!(f.d.file_error.as_deref(), Some(".txt is not supported."));
        let good = f.write("inverse.cube", INVERSE.as_bytes());
        f.edit("file", json!(good));
        assert_eq!(f.d.file_error, None);
        assert_eq!(f.render(), map(&["KKKKKKKK"; 8]));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(layer_bytes(&f.app, None), map(&["KKKKKKKK"; 8]));
    }

    /// N2: a picked file over 128 MiB shows the size error under the row, unparsed.
    #[test]
    fn color_lookup_refuses_an_oversized_file() {
        let mut f = Fx::open("pcColorLookup", |v| {
            v.insert("source".into(), json!("file"));
        });
        let huge = vec![b'0'; lut::MAX_FILE_BYTES + 1];
        f.files
            .queue_pick(Ok(vec![PickedFile::snapshot("huge.cube".into(), huge)]));
        f.acquire();
        f.d.chooser = Some(f.files.choose_open_with(false, ".cube,.3dl,.look", false));
        assert_eq!(f.d.poll_chooser(), Some(true));
        f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        assert!(matches!(f.d.input, ExternalInput::Failed));
        assert_eq!(f.d.file_error.as_deref(), Some(lut::TOO_BIG));
        assert_eq!(f.render(), map(&["WWWWWWWW"; 8]));
    }

    /// N4: Color Lookup is an adjustment, so as in Paint.NET it never becomes Effects ▸ Repeat
    /// (Repeat stays unavailable and changes nothing); reopened from its remembered values with
    /// Source: File it reads the file again: the file now holds an all-red table, so the black
    /// layer turns red. (Its File values do select the external route Repeat would take,
    /// `External::Lut`; see `tone`'s tests.)
    #[test]
    fn color_lookup_reopen_rereads_the_file() {
        use pdn_core::action::{ActionOutcome, is_executable, perform};
        const INVERSE: &str =
            "LUT_3D_SIZE 2\n1 1 1\n0 1 1\n1 0 1\n0 0 1\n1 1 0\n0 1 0\n1 0 0\n0 0 0\n";
        let mut f = Fx::open("pcColorLookup", |v| {
            v.insert("source".into(), json!("file"));
        });
        let path = f.write("look.cube", INVERSE.as_bytes());
        f.edit("file", json!(path));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(layer_bytes(&f.app, None), map(&["KKKKKKKK"; 8]));
        std::fs::write(&path, format!("LUT_3D_SIZE 2\n{}", "1 0 0\n".repeat(8))).unwrap();
        assert!(f.app.last_effect.is_none());
        assert!(!is_executable(&f.app, "menu.effects.repeat"));
        assert_eq!(
            perform(&mut f.app, "menu.effects.repeat"),
            ActionOutcome::Done
        );
        assert_eq!(layer_bytes(&f.app, None), map(&["KKKKKKKK"; 8]));
        f.reopen(false);
        let reads = f.log().iter().filter(|op| **op == "read").count();
        f.acquire();
        assert_eq!(
            f.log().iter().filter(|op| **op == "read").count(),
            reads + 1
        );
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(layer_bytes(&f.app, None), map(&["RRRRRRRR"; 8]));
    }
    /// R156: the path box takes the first input focus and stays editable; an actual edit is one
    /// acquisition and detaches the pick, and the same name typed back cannot recover it.
    #[test]
    fn bb27_text_edit_detaches_pick() {
        let mut f = Fx::new();
        f.d.typed_paths = false;
        let a = picked("pick.png", &quad());
        f.files.queue_pick(Ok(vec![a]));
        f.acquire();
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), Some(true));
        f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        assert_eq!(f.image(), Some(quad()));
        f.frame(vec![], true);
        let n = f.d.acquisitions;
        f.frame(vec![egui::Event::Text("x".into())], false);
        assert_ne!(f.d.values["file"], json!("pick.png"));
        assert!(f.d.candidate.is_none());
        assert_eq!(f.d.acquisitions, n + 1);
        for _ in 0..2 {
            f.frame(vec![], false);
        }
        assert_eq!(f.d.acquisitions, n + 1, "idle frames acquire nothing");
        f.edit("file", json!("pick.png"));
        assert_eq!(f.image(), None, "a name never reattaches the pick");
        assert_eq!(
            f.d.file_error.as_deref(),
            Some("Use Browse to choose a file in this browser.")
        );
    }
    /// R156: a canceled chooser (an empty pick, or the activation prompt's exact "Canceled")
    /// keeps the value, source, pending load and error; a chooser failure is reported and keeps
    /// them too.
    #[test]
    fn bb27_picker_cancel_retains_everything() {
        let mut f = Fx::new();
        let notes = f.write("notes.txt", b"hi");
        let png = f.write("a.png", &encoded(&quad(), "png"));
        f.d.values.insert("file".into(), json!(notes));
        f.acquire();
        let error = Some(".txt is not supported.".to_string());
        assert_eq!(f.d.file_error, error);
        for result in [Ok(vec![]), Err("Canceled".to_string())] {
            f.files.queue_pick(result);
            f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
            assert_eq!(f.d.poll_chooser(), None);
            assert!(f.d.chooser.is_none());
            assert_eq!(
                (f.d.acquisitions, f.d.file_error.clone()),
                (1, error.clone())
            );
            assert_eq!(f.d.values["file"], json!(notes));
        }
        // A load still pending when the chooser is canceled resolves under its generation.
        let gate = f.files.probe.hold();
        f.edit("file", json!(png));
        assert!(matches!(f.d.input, ExternalInput::Loading { .. }));
        f.files.queue_pick(Ok(vec![]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), None);
        gate.set(true);
        f.poll();
        assert_eq!((f.image(), f.d.file_error.clone()), (Some(quad()), None));
        crate::dialogs::PENDING.with(|p| p.borrow_mut().clear());
        f.files.queue_pick(Err("boom".into()));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), None);
        let shown = crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        assert_eq!(shown.len(), 1);
        assert_eq!(
            (shown[0].title(), shown[0].message()),
            (TITLE.into(), Some("boom".into()))
        );
        assert_eq!(f.image(), Some(quad()));
    }
    /// R156: a newer value retires both a slower load and an open chooser; their late
    /// completions change nothing.
    #[test]
    fn bb27_newer_values_retire_stale_operations() {
        let mut f = Fx::new();
        let a = f.write("a.png", &encoded(&quad(), "png"));
        let b = f.write("b.png", &encoded(&image(1, 1, &[G]), "png"));
        let gate_a = f.files.probe.hold();
        f.d.values.insert("file".into(), json!(a));
        f.acquire();
        let gate_choose = f.files.probe.hold();
        f.files
            .queue_pick(Ok(vec![picked("late.png", &image(1, 1, &[K]))]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), None);
        assert!(f.d.chooser.is_some());
        f.edit("file", json!(b));
        assert!(f.d.chooser.is_none());
        assert_eq!(f.image(), Some(image(1, 1, &[G])));
        gate_a.set(true);
        gate_choose.set(true);
        for _ in 0..2 {
            f.poll();
            assert_eq!(f.d.poll_chooser(), None);
        }
        assert_eq!(f.image(), Some(image(1, 1, &[G])));
        assert_eq!(f.d.values["file"], json!(b));
        assert_eq!(f.log(), ["read", "choose", "read", "decode"]);
    }
    /// R155/R156: OK while the load is pending waits for it (Preview on or off), commits the
    /// current source once with full bookkeeping; parent Cancel during the wait wins.
    #[test]
    fn bb27_pending_ok_and_cancel() {
        for (preview, cancel) in [(true, false), (false, false), (true, true)] {
            let mut f = Fx::new();
            f.d.preview = preview;
            let path = f.write("a.png", &encoded(&quad(), "png"));
            f.d.values.insert("file".into(), json!(path));
            let before = f.snapshot();
            let gate = f.files.probe.hold();
            f.acquire();
            assert_eq!(f.ok(), DialogState::Open);
            assert!(f.d.finalizing);
            assert_eq!(f.snapshot(), before);
            if cancel {
                f.cx(|d, cx, _| d.cancel(cx));
                gate.set(true);
                assert_eq!(f.snapshot(), before);
                continue;
            }
            gate.set(true);
            f.poll();
            assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
            assert_eq!(layer_bytes(&f.app, None), map(&CASE_2));
            assert_eq!(f.app.active_ws().unwrap().history.undo.len(), before.2 + 1);
            assert_eq!(f.app.effect_tokens["bbFillFromFile"]["file"], json!(path));
            assert_eq!(f.app.last_effect.as_deref(), Some("bbFillFromFile"));
            assert_eq!(f.d.acquisitions, 1);
        }
    }
    /// R156: an open chooser blocks final acceptance; its accepted pick is notified and loaded,
    /// then committed and published.
    #[test]
    fn bb27_pending_chooser_blocks_ok() {
        let mut f = Fx::new();
        f.acquire();
        let gate = f.files.probe.hold();
        let a = picked("pick.png", &quad());
        f.files.queue_pick(Ok(vec![a.clone()]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), None);
        assert_eq!(f.ok(), DialogState::Open);
        assert!(f.d.finalizing);
        assert!(f.cx(|d, cx, _| d.finish_ready(cx)).is_none());
        gate.set(true);
        assert_eq!(f.frame(vec![], false), DialogState::Open);
        assert_eq!(f.image(), Some(quad()));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(layer_bytes(&f.app, None), map(&CASE_2));
        assert_eq!(f.attached(), Some(("pick.png".into(), a.id)));
    }
    /// Q64: in a browser a typed path without its attached pick is an inline error (no read);
    /// empty is no image; an attached pick still loads.
    #[test]
    fn bb27_q64_browser_typed_path() {
        let mut f = Fx::new();
        f.d.typed_paths = false;
        let path = f.write("a.png", &encoded(&quad(), "png"));
        f.d.values.insert("file".into(), json!(path));
        f.acquire();
        assert_eq!(
            f.d.file_error.as_deref(),
            Some("Use Browse to choose a file in this browser.")
        );
        assert!(f.log().is_empty());
        f.edit("file", json!(""));
        assert_eq!((f.image(), f.d.file_error.clone()), (None, None));
        f.files.queue_pick(Ok(vec![picked("a.png", &quad())]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), Some(true));
        f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
        assert_eq!((f.image(), f.d.file_error.clone()), (Some(quad()), None));
    }
    /// Q65: a failure that arrives after OK returns to the editable dialog with its message and
    /// writes nothing; Repeat's failure cancels and reports the exact error under the effect's
    /// title, keeping the last success; a parent Cancel first suppresses the message.
    #[test]
    fn bb27_q65_failure_transactions() {
        let mut f = Fx::new();
        let notes = f.write("notes.txt", b"hi");
        f.d.values.insert("file".into(), json!(notes));
        let before = f.snapshot();
        let gate = f.files.probe.hold();
        f.acquire();
        assert_eq!(f.ok(), DialogState::Open);
        assert!(f.d.finalizing);
        gate.set(true);
        f.poll();
        assert!(f.cx(|d, cx, _| d.finish_ready(cx)).is_none());
        assert!(!f.d.finalizing);
        assert_eq!(f.d.file_error.as_deref(), Some(".txt is not supported."));
        assert_eq!(f.snapshot(), before);
        assert_eq!(f.frame(vec![], false), DialogState::Open);
        // A successful pick, then Repeat with a failing remembered path.
        let a = picked("pick.png", &quad());
        *f.source.borrow_mut() = Some(FileAttachment {
            value: "pick.png".into(),
            file: a.clone(),
        });
        f.app.last_effect = Some("bbFillFromFile".into());
        let mut values = f.d.values.clone();
        values.insert("file".into(), json!(notes));
        f.app.last_effect_values = Some(values);
        for cancel_first in [false, true] {
            crate::dialogs::PENDING.with(|p| p.borrow_mut().clear());
            let before = f.snapshot();
            let gate = f.files.probe.hold();
            f.reopen(true);
            f.acquire();
            if cancel_first {
                assert_eq!(
                    f.frame(key(Key::Escape), false),
                    DialogState::Closed(DialogResult::Cancel)
                );
                gate.set(true);
            } else {
                gate.set(true);
                assert_eq!(
                    f.frame(vec![], false),
                    DialogState::Closed(DialogResult::Cancel)
                );
            }
            let shown = crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
            if cancel_first {
                assert!(shown.is_empty());
            } else {
                assert_eq!(shown.len(), 1);
                assert_eq!(
                    (shown[0].title(), shown[0].message()),
                    (TITLE.into(), Some(".txt is not supported.".into()))
                );
            }
            assert_eq!(f.snapshot(), before);
            assert_eq!(f.attached(), Some(("pick.png".into(), a.id)));
        }
    }
    /// R155/R156: a closed or replaced original document rejects a delayed load's commit; a
    /// background-tab original still completes.
    #[test]
    fn bb27_original_document_owns_delayed_work() {
        for mode in ["switch", "close"] {
            let mut f = Fx::new();
            let path = f.write("a.png", &encoded(&quad(), "png"));
            f.d.values.insert("file".into(), json!(path));
            let first = f.app.active.unwrap();
            let history = f.app.documents[first].history.undo.len();
            let gate = f.files.probe.hold();
            f.acquire();
            assert_eq!(f.ok(), DialogState::Open);
            f.app
                .create_blank_document_in_new_workspace(8, 8, 96., true);
            if mode == "close" {
                f.app.close_document_workspace(first);
            }
            gate.set(true);
            f.poll();
            if mode == "switch" {
                assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
                assert_eq!(f.app.documents[first].history.undo.len(), history + 1);
            } else {
                assert_eq!(
                    f.cx(|d, cx, _| d.finish_ready(cx)),
                    Some(DialogState::Closed(DialogResult::Cancel))
                );
                assert!(f.app.last_effect.is_none());
            }
            assert_eq!(f.attached(), None);
        }
    }

    fn pending_messages() -> Vec<Box<dyn Dialog>> {
        crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()))
    }

    /// R156 same-frame ordering: a slower load that becomes ready in the frame which also edits
    /// the path and presses Enter is retired unpolled; only the edited value acquires, and OK
    /// waits for that acquisition.
    #[test]
    fn bb27_same_frame_edit_and_enter_outrank_ready_load() {
        let mut f = Fx::new();
        let a = f.write("a.png", &encoded(&quad(), "png"));
        let b = f.write("b.png", &encoded(&image(1, 1, &[G]), "png"));
        f.d.values.insert("file".into(), json!(a));
        let gate_a = f.files.probe.hold();
        f.acquire();
        f.frame(vec![], true);
        assert!(matches!(f.d.input, ExternalInput::Loading { .. }));
        let before = f.snapshot();
        let gate_b = f.files.probe.hold();
        gate_a.set(true);
        let mut events = vec![egui::Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::COMMAND,
        }];
        events.push(egui::Event::Text(b.clone()));
        events.extend(key(Key::Enter));
        assert_eq!(f.frame(events, false), DialogState::Open);
        assert_eq!(f.d.values["file"], json!(b));
        assert!(f.d.finalizing, "OK waits for the edited value");
        assert_eq!(f.snapshot(), before);
        assert_eq!(f.log(), ["read", "read"], "A was never decoded");
        assert_eq!(f.d.acquisitions, 2);
        gate_b.set(true);
        f.poll();
        assert_eq!(f.image(), Some(image(1, 1, &[G])));
        assert_eq!(f.finish(), Some(DialogState::Closed(DialogResult::Ok)));
        assert_eq!(f.app.effect_tokens["bbFillFromFile"]["file"], json!(b));
        assert_eq!(f.d.acquisitions, 2);
    }
    /// R156/Q65 same-frame ordering: Escape in the frame where Repeat's held load becomes ready
    /// cancels first, whether that load would succeed (with its final render ready) or fail: no
    /// history, token, attachment change or message.
    #[test]
    fn bb27_same_frame_escape_outranks_ready_repeat() {
        for succeed in [true, false] {
            let mut f = Fx::new();
            let notes = f.write("notes.txt", b"hi");
            let pick = picked("pick.png", &quad());
            *f.source.borrow_mut() = Some(FileAttachment {
                value: "pick.png".into(),
                file: pick.clone(),
            });
            let mut values = f.d.values.clone();
            let remembered = if succeed {
                json!("pick.png")
            } else {
                json!(notes)
            };
            values.insert("file".into(), remembered);
            f.app.last_effect = Some("bbFillFromFile".into());
            f.app.last_effect_values = Some(values);
            pending_messages();
            let before = f.snapshot();
            let gate = f.files.probe.hold();
            f.reopen(true);
            f.acquire();
            assert!(matches!(f.d.input, ExternalInput::Loading { .. }));
            f.ready_final(image(8, 8, &[R; 64]));
            gate.set(true);
            assert_eq!(
                f.frame(key(Key::Escape), false),
                DialogState::Closed(DialogResult::Cancel),
                "succeed={succeed}"
            );
            assert!(pending_messages().is_empty(), "succeed={succeed}");
            assert_eq!(f.snapshot(), before, "succeed={succeed}");
            assert_eq!(f.attached(), Some(("pick.png".into(), pick.id)));
        }
    }
    /// R156 same-frame ordering: while a pending OK applies, the Applying footer Cancel or title
    /// close in the frame where its load and final render become ready wins before
    /// `finish_ready`: nothing is committed or published.
    #[test]
    fn bb27_same_frame_applying_cancel_outranks_ready_final() {
        for how in ["footer", "close"] {
            let mut f = Fx::new();
            f.acquire();
            let pick = picked("pick.png", &quad());
            f.files.queue_pick(Ok(vec![pick]));
            f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
            assert_eq!(f.d.poll_chooser(), Some(true));
            let gate = f.files.probe.hold();
            f.cx(|d, cx, ctx| d.notify(cx, ctx, true));
            assert!(matches!(f.d.input, ExternalInput::Loading { .. }));
            let before = f.snapshot();
            assert_eq!(f.ok(), DialogState::Open);
            assert!(f.d.finalizing);
            let (_, rect) = f.frame_at(vec![], true);
            let pos = if how == "footer" {
                Pos2::new(rect.max.x - 40., rect.max.y - 20.)
            } else {
                Pos2::new(rect.max.x - 15., rect.min.y + 15.)
            };
            let button = |pressed| egui::Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            };
            f.frame(vec![egui::Event::PointerMoved(pos)], false);
            f.frame(vec![button(true)], false);
            f.ready_final(image(8, 8, &[R; 64]));
            gate.set(true);
            assert_eq!(
                f.frame(vec![button(false)], false),
                DialogState::Closed(DialogResult::Cancel),
                "{how}"
            );
            assert_eq!(f.snapshot(), before, "{how}");
            assert_eq!(f.attached(), None, "{how}");
        }
    }
    /// R156 same-frame ordering: a chooser answering in the frame of a path edit is retired; the
    /// edit wins, the pick is dropped, and there is exactly one acquisition.
    #[test]
    fn bb27_same_frame_edit_outranks_ready_chooser() {
        let mut f = Fx::new();
        f.acquire();
        f.frame(vec![], true);
        let gate = f.files.probe.hold();
        f.files.queue_pick(Ok(vec![picked("pick.png", &quad())]));
        f.d.chooser = Some(f.files.choose_open_with(false, ".png", true));
        assert_eq!(f.d.poll_chooser(), None);
        let n = f.d.acquisitions;
        gate.set(true);
        assert_eq!(
            f.frame(vec![egui::Event::Text("x".into())], false),
            DialogState::Open
        );
        assert_eq!(f.d.values["file"], json!("x"));
        assert!(f.d.chooser.is_none() && f.d.candidate.is_none());
        assert_eq!(f.d.acquisitions, n + 1);
        assert_eq!(f.log(), ["choose", "read"], "the pick was never decoded");
        assert_eq!(f.image(), None);
    }

    /// Camera Raw's Load Preset... button: the `.xmp` develop preset its chooser picks sets the
    /// dialog's controls and previews at once; a cancel changes nothing, and a file that is not a
    /// preset changes nothing and says why in a message.
    #[test]
    fn camera_raw_load_preset_sets_the_controls() {
        let mut f = Fx::open("pcCameraRaw", |_| {});
        let (_, rect) = f.frame_at(vec![], true);
        // The button after the footer's Preview check box.
        let at = Pos2::new(rect.min.x + 140., rect.max.y - 20.);
        let click = |f: &mut Fx| {
            f.frame(vec![egui::Event::PointerMoved(at)], false);
            for pressed in [true, false] {
                let button = egui::Event::PointerButton {
                    pos: at,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                };
                f.frame(vec![button], false);
            }
        };
        let pending = || crate::dialogs::PENDING.with(|p| std::mem::take(&mut *p.borrow_mut()));
        let before = f.d.values.clone();
        f.files.queue_pick(Ok(vec![]));
        click(&mut f);
        assert_eq!(f.log(), ["choose"]);
        assert_eq!((&f.d.values, pending().len()), (&before, 0));
        let preset = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
            <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+0.35"
            crs:Clarity="+8" crs:SplitToningShadowHue="215" crs:SplitToningShadowSaturation="18"/></rdf:RDF></x:xmpmeta>"#;
        let file = PickedFile::snapshot("look.xmp".into(), preset.to_vec());
        f.files.queue_pick(Ok(vec![file]));
        let generation = f.d.generation;
        click(&mut f);
        assert_eq!(f.log(), ["choose", "choose"]);
        let v = |f: &Fx, k: &str| f.d.values[k].as_f64().unwrap() as f32;
        assert_eq!(
            [v(&f, "exposure"), v(&f, "clarity"), v(&f, "gradeShadowHue")],
            [0.35, 8.0, 215.0]
        );
        assert_eq!(v(&f, "gradeShadowSat"), 18.0);
        assert!(f.d.generation > generation, "the preset previews");
        assert_eq!(pending().len(), 0);
        let loaded = f.d.values.clone();
        let file = PickedFile::snapshot("photo.xmp".into(), b"\x89PNG\r\n\x1a\n".to_vec());
        f.files.queue_pick(Ok(vec![file]));
        click(&mut f);
        assert_eq!(f.d.values, loaded);
        assert_eq!(pending().len(), 1, "the refusal is a message");
    }
}
