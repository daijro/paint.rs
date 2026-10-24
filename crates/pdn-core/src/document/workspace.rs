//! Port of: js/src/engine/document/DocumentWorkspace.js (plus the view/active-layer parts of
//! history/memento/DocumentStateHistoryMemento.js and the `HistoryStack` event wiring)
//! Reference: paintdotjs @ b4e6fc5
//!
//! JS holds the active layer as a reference and reacts to LayerList events; Rust holds an index
//! and replays `LayerList::log` after every document mutation (`sync_after_change`).

use super::Document;
use super::layer::{Layer, LayerEvent};
use super::surface::Surface;
use super::view::DocumentView;
use crate::app::AppEvent;
use crate::history::{FunctionResult, HistoryMemento, HistoryStack, MementoKind, UndoTarget};
use crate::pixel::IntRect;
use crate::selection::Selection;
use crate::tool::transacted::Step;
use crate::util::geom::Rectangle;

static NEXT_WORKSPACE_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub struct DocumentWorkspace {
    /// Stable across document replacement, for pending save/close continuations.
    pub id: u64,
    pub view: DocumentView,
    /// `workspace.surfaceBox`.
    pub surface_box: crate::render::SurfaceBox,
    /// `DocumentView.gridRenderer` (created hidden).
    pub grid_renderer: crate::render::RendererId,
    /// `selectionRenderer`: the selection layer after the grid (`canvas-rendering.md §2.1`).
    selection_renderer: crate::render::RendererId,
    pub document: Document,
    pub active_layer: usize,
    pub history: HistoryStack,
    /// Extra Edit ▸ Fade: the id and name of the newest effect or adjustment memento (set by
    /// `EffectSession::commit_with`); Fade is available while it is the last undo entry.
    pub fade_source: Option<(u64, String)>,
    pub selection: Selection,
    /// The selection revision last announced by `AppEvent::SelectionChanged` (`AppWorkspace::
    /// drain_events` queues the event when it moves).
    pub(crate) selection_revision: u64,
    pub file_name: Option<String>,
    pub file_path: Option<String>,
    pub file_format: Option<String>,
    pub save_options: serde_json::Value,
    pub dirty: bool,
    pub grid_visible: bool,
    pub rulers_visible: bool,
    pub scratch: Surface,
    pub scratch_borrowed: Option<&'static str>,
    /// The document size when the scratch surface was lent (it must not change meanwhile).
    lent_size: (u32, u32),
    pub is_initial_workspace: bool,
    /// The tool instance whose transaction stays open while another image is active
    /// (painting-tools-2.md §7.6 "Switching to another image tab").
    pub parked_tool: Option<Box<dyn crate::tool::Tool>>,
    /// Id of the document the layer log / composition were last synced with.
    synced_doc_id: u64,
    /// The canvas view's mip levels of `view.composition` (rebuilt per composition revision).
    mips: std::cell::RefCell<crate::render::view::MipChain>,
    /// Move Selected Pixels' pixels while the pointer drags them (`render::float`): lifted out of
    /// the layer, drawn by the canvas until the drag's end renders them in.
    pub float: Option<std::sync::Arc<crate::render::float::Float>>,
}

impl DocumentWorkspace {
    pub fn new(width: u32, height: u32, resolution: f64) -> Self {
        let document = Document::new(width, height, resolution);
        let mut surface_box = crate::render::SurfaceBox::new();
        let grid_renderer = surface_box.add_renderer(crate::render::Renderer::Grid(
            crate::render::grid::GridRenderer { visible: false },
        ));
        let selection_renderer = surface_box.add_renderer(crate::render::Renderer::Selection(
            crate::render::selection::SelectionRenderer::default(),
        ));
        Self {
            id: NEXT_WORKSPACE_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            synced_doc_id: document.id,
            view: DocumentView::new(width, height),
            surface_box,
            grid_renderer,
            selection_renderer,
            document,
            active_layer: 0,
            history: HistoryStack::default(),
            fade_source: None,
            selection: Selection::default(),
            selection_revision: 0,
            file_name: None,
            file_path: None,
            file_format: None,
            save_options: serde_json::Value::Null,
            dirty: false,
            grid_visible: false,
            rulers_visible: false,
            scratch: Surface::create(width, height),
            scratch_borrowed: None,
            lent_size: (0, 0),
            is_initial_workspace: false,
            parked_tool: None,
            mips: Default::default(),
            float: None,
        }
    }

    /// The workspace selection layer (outline on, tint off until a selection tool activates).
    pub fn selection_renderer(&self) -> &crate::render::selection::SelectionRenderer {
        match self.surface_box.renderer(self.selection_renderer) {
            Some(crate::render::Renderer::Selection(r)) => r,
            _ => unreachable!("the workspace selection renderer is never removed"),
        }
    }

    pub fn selection_renderer_mut(&mut self) -> &mut crate::render::selection::SelectionRenderer {
        match self.surface_box.renderer_mut(self.selection_renderer) {
            Some(crate::render::Renderer::Selection(r)) => r,
            _ => unreachable!("the workspace selection renderer is never removed"),
        }
    }

    /// The view's `MipChain`; it drops its levels itself when the composition revision changes.
    pub fn view_mips(&self) -> std::cell::RefMut<'_, crate::render::view::MipChain> {
        self.mips.borrow_mut()
    }

    /// `Document.addLayer` through the workspace (runs the LayerList handlers).
    pub fn add_layer(&mut self, layer: Layer) {
        self.document.add_layer(layer);
        self.sync_after_change();
    }

    /// `setDocumentAndActiveLayer`.
    pub fn set_document_and_active_layer(&mut self, document: Document, active_layer: usize) {
        assert!(
            active_layer < document.layers.size(),
            "The active layer must belong to the new document"
        );
        let changed = self.active_layer != active_layer;
        self.document = document;
        self.active_layer = active_layer;
        self.sync_after_change();
        self.view.events.push(AppEvent::DocumentChanged);
        if changed {
            self.view.events.push(AppEvent::ActiveLayerChanged);
        }
    }

    /// Everything the JS handlers did synchronously after a document mutation: the
    /// `onLayerRemoving`/`onLayerInserted` active-layer fixups, composition/scratch resizing
    /// (`setDocument`/`onDocumentChanged`) and the document invalidation that re-composites.
    pub(crate) fn sync_after_change(&mut self) {
        let log = self.document.layers.take_log();
        // A function/memento swapped the document: the incoming document's own log (from
        // building it) is not ours to replay, and it must be re-composited in full.
        let swapped = self.document.id != self.synced_doc_id;
        self.synced_doc_id = self.document.id;
        let layers_changed = !swapped && !log.is_empty();
        if layers_changed {
            self.view.events.push(AppEvent::LayersChanged);
            for event in log {
                match event {
                    LayerEvent::Removing { index, count } => {
                        // Pick a neighbour (computed against the pre-removal list, then
                        // mapped to its post-removal index).
                        let new = if index == 0 { 1 } else { index - 1 };
                        let target = if new < count {
                            if new > index { new - 1 } else { new }
                        } else {
                            0 // last layer / empty list
                        };
                        self.set_active_layer(target);
                    }
                    // Silent, like `this.activeLayer = layer`.
                    LayerEvent::Inserted { index } => self.active_layer = index,
                    LayerEvent::Swapped { a, b } => {
                        if self.active_layer == a {
                            self.active_layer = b;
                        } else if self.active_layer == b {
                            self.active_layer = a;
                        }
                    }
                }
            }
        }
        let (w, h) = (self.document.width, self.document.height);
        let mut resized = false;
        if self.view.composition.width() != w || self.view.composition.height() != h {
            self.view.composition = Surface::create(w, h);
            resized = true;
        }
        // While lent, `scratch` is a placeholder and the borrower holds the real surface (JS lends
        // the object itself, so its size check passes): a transacted pixel tool pushes its entries
        // with the surface out (Task 27). The document cannot change size meanwhile, since every
        // resize commits the tool first (painting-tools-2.md §7.6).
        if let Some(reason) = self.scratch_borrowed {
            debug_assert_eq!(
                (w, h),
                self.lent_size,
                "the document changed size while the scratch surface is lent to {reason}"
            );
        }
        if self.scratch_borrowed.is_none()
            && (self.scratch.width() != w || self.scratch.height() != h)
        {
            self.scratch = Surface::create(w, h);
        }
        if swapped || resized || layers_changed {
            // JS: LayerList `changed` / a new document invalidates everything.
            self.invalidate(None);
        } else {
            // Composite only the region the memento/function invalidated.
            self.composite_update_region();
        }
    }

    /// `document.invalidate(rects)` + the `document:invalidated` handler: synchronously
    /// re-composites and bumps the revision. `None` = the whole document.
    pub fn invalidate(&mut self, rects: Option<Vec<Rectangle>>) {
        let whole = rects.is_none();
        self.document.invalidate(rects);
        self.composite_update_region_as(whole);
    }

    fn composite_update_region(&mut self) {
        self.composite_update_region_as(false);
    }

    fn composite_update_region_as(&mut self, whole: bool) {
        if self.document.update_region.is_empty() {
            return;
        }
        self.document.update(&mut self.view.composition);
        // The view's mips rebuild just the tiles under the composited region.
        let changed = (!whole).then(|| {
            let union = self
                .document
                .update_region
                .iter()
                .copied()
                .reduce(|a, b| Rectangle::union(&a, &b));
            union.map_or_else(IntRect::default, |r| IntRect::bound_of(&r))
        });
        self.mips.get_mut().invalidate(changed);
        self.view.composition_revision += 1;
        let rects = if whole {
            vec![]
        } else {
            std::mem::take(&mut self.document.update_region)
        };
        self.view
            .events
            .push(AppEvent::DocumentInvalidated { rects });
    }

    fn undo_target(&mut self) -> (UndoTarget<'_>, &mut HistoryStack) {
        (
            UndoTarget {
                document: &mut self.document,
                active_layer: &mut self.active_layer,
                selection: &mut self.selection,
            },
            &mut self.history,
        )
    }

    pub fn execute_function(
        &mut self,
        f: impl FnOnce(&mut UndoTarget) -> Option<HistoryMemento>,
    ) -> FunctionResult {
        self.document.update_region.clear();
        match f(&mut self.undo_target().0) {
            Some(memento) => {
                self.debug_assert_raster(&memento);
                self.history.push_new_memento(memento);
                self.after_history_change();
            }
            // No entry (a no-op, e.g. Deselect on an empty selection): the document stays clean
            // (painting-tools-2.md §7.6 note †).
            None => self.sync_after_change(),
        }
        FunctionResult::Success
    }

    /// Pushes a memento a tool/action built itself (`performAction`).
    pub fn push_memento(&mut self, memento: HistoryMemento) {
        self.debug_assert_raster(&memento);
        self.history.push_new_memento(memento);
        self.after_history_change();
    }

    /// The effect Fade would fade: the newest undo entry, when an effect or adjustment made it.
    fn fade_target(&self) -> Option<(&HistoryMemento, &str)> {
        let (id, name) = self.fade_source.as_ref()?;
        let m = self.history.undo.last().filter(|m| m.id == *id)?;
        matches!(m.kind, MementoKind::Bitmap(_)).then_some((m, name))
    }

    /// Edit ▸ Fade is available (the newest history item is an effect or adjustment).
    pub fn can_fade(&self) -> bool {
        self.fade_target().is_some()
    }

    /// Edit ▸ Fade: blends the newest effect's before-pixels with the current ones, as one new
    /// history item "Fade <effect>". `false` when Fade is unavailable. No text-layer guard: the
    /// target is the newest `Bitmap` entry, and `debug_assert_raster` keeps those off text layers.
    pub fn fade(
        &mut self,
        opacity: f64,
        mode: crate::document::layer::BlendMode,
        quality: crate::pixel::mask::SelectionQuality,
    ) -> bool {
        let Some((m, name)) = self.fade_target() else {
            return false;
        };
        let MementoKind::Bitmap(b) = &m.kind else {
            return false;
        };
        let (layer, chunks) = (b.layer_index, b.chunks.clone());
        let name = format!("Fade {name}");
        let image = m.image.clone();
        self.execute_function(|t| {
            crate::history::functions::fade(
                t, layer, &chunks, &name, &image, opacity, mode, quality,
            )
        });
        true
    }

    /// S§4.5: a pixel entry on a text layer is an edit that skipped `text_layer::ensure_raster`.
    fn debug_assert_raster(&self, m: &HistoryMemento) {
        fn edits_text(d: &Document, m: &HistoryMemento) -> bool {
            match &m.kind {
                MementoKind::Bitmap(b) | MementoKind::Stroke { pixels: b, .. } => d
                    .layers
                    .get_at(b.layer_index)
                    .is_some_and(|l| l.text.is_some()),
                MementoKind::Compound(v) => v.iter().any(|m| edits_text(d, m)),
                // A transacted tool's commit holds its pixel entry.
                MementoKind::Tool(e) => {
                    matches!(&e.step, Step::Finish(_, Some(m)) if edits_text(d, m))
                }
                _ => false,
            }
        }
        debug_assert!(
            !edits_text(&self.document, m),
            "{:?} edits a text layer's pixels without text_layer::ensure_raster",
            m.name
        );
    }

    /// Applies a non-tool memento (a tool's snapshot or inner entry) outside the stacks; returns
    /// its inverse.
    pub fn apply_memento(&mut self, m: HistoryMemento) -> HistoryMemento {
        self.document.update_region.clear();
        let inverse = m.perform_undo(&mut self.undo_target().0);
        self.sync_after_change();
        inverse
    }

    pub(crate) fn after_history_change(&mut self) {
        self.sync_after_change();
        // HistoryStack.pop(): executed -> setDirty(true), then document:history_changed.
        self.set_dirty(true);
        self.view.events.push(AppEvent::HistoryChanged);
    }

    pub fn undo(&mut self) -> bool {
        self.step(false)
    }

    pub fn redo(&mut self) -> bool {
        self.step(true)
    }

    /// One `stepForward`/`stepBackward`. `DocumentStateHistoryMemento` also owns zoom and
    /// viewport, which `UndoTarget` cannot reach: the inverse memento gets the CURRENT values
    /// and the restored ones are applied to the view (its `onUndo`).
    fn step(&mut self, forward: bool) -> bool {
        let next = if forward {
            self.history.redo.front()
        } else if self.history.can_undo() {
            self.history.undo.last()
        } else {
            None
        };
        let restored = match next.map(|m| &m.kind) {
            Some(MementoKind::DocumentState(m)) => {
                Some((m.zoom, m.zoom_to_window, m.viewport_x, m.viewport_y))
            }
            _ => None,
        };
        let current = (
            self.view.zoom,
            self.view.zoom_to_window,
            self.view.viewport_x,
            self.view.viewport_y,
        );
        self.document.update_region.clear();
        let (mut target, history) = self.undo_target();
        let ok = if forward {
            history.step_forward(&mut target)
        } else {
            history.step_backward(&mut target)
        };
        if !ok {
            return false;
        }
        if let Some((zoom, zoom_to_window, viewport_x, viewport_y)) = restored {
            let inverse = if forward {
                self.history.undo.last_mut()
            } else {
                self.history.redo.front_mut()
            };
            if let Some(MementoKind::DocumentState(m)) = inverse.map(|m| &mut m.kind) {
                (m.zoom, m.zoom_to_window, m.viewport_x, m.viewport_y) = current;
            }
            self.view.zoom = zoom;
            self.view.zoom_to_window = zoom_to_window;
            self.view.viewport_x = viewport_x;
            self.view.viewport_y = viewport_y;
        }
        self.after_history_change();
        if restored.is_some() {
            // Task 21: the JS memento also deactivates and re-creates the active tool.
            self.view
                .set_view_position(self.view.viewport_x, self.view.viewport_y);
            self.view.events.push(AppEvent::LayersChanged);
            self.view.events.push(AppEvent::UpdateSize {
                w: self.document.width,
                h: self.document.height,
            });
        }
        true
    }

    /// Panics on a double borrow, like the JS throw.
    pub fn borrow_scratch(&mut self, reason: &'static str) -> Surface {
        if let Some(r) = self.scratch_borrowed {
            panic!("scratchSurface is already borrowed: {r}");
        }
        self.scratch_borrowed = Some(reason);
        self.lent_size = (self.document.width, self.document.height);
        std::mem::replace(&mut self.scratch, Surface::create(1, 1))
    }

    pub fn return_scratch(&mut self, surface: Surface) {
        assert!(
            self.scratch_borrowed.is_some(),
            "scratchSurface is not borrowed"
        );
        self.scratch_borrowed = None;
        self.scratch = surface;
    }

    pub fn get_friendly_name(&self) -> String {
        match &self.file_name {
            None => crate::i18n::t("untitled.friendlyName"),
            Some(n) => n.clone(),
        }
    }

    pub fn set_dirty(&mut self, dirty: bool) {
        if self.dirty == dirty {
            return;
        }
        self.dirty = dirty;
        self.view.events.push(AppEvent::DirtyChanged);
    }

    pub fn set_file_info(
        &mut self,
        file_name: Option<String>,
        file_path: Option<String>,
        file_format: Option<String>,
        save_options: serde_json::Value,
    ) {
        self.file_name = file_name;
        self.file_path = file_path;
        self.file_format = file_format;
        self.save_options = save_options;
        self.view.events.push(AppEvent::FileChanged);
    }

    /// `setActiveLayerIndex` / `setActiveLayer`.
    pub fn set_active_layer(&mut self, index: usize) {
        if self.active_layer == index {
            return;
        }
        // `AppWorkspace::set_active_layer` deactivates and re-activates the tool around this.
        self.active_layer = index;
        self.view.events.push(AppEvent::ActiveLayerChanged);
    }

    pub fn set_grid_visible(&mut self, visible: bool) {
        self.grid_visible = visible;
        if let Some(r) = self.surface_box.renderer_mut(self.grid_renderer) {
            r.set_visible(visible);
        }
    }

    pub fn set_rulers_visible(&mut self, visible: bool) {
        self.rulers_visible = visible;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Task 27 M6: entries pushed while the scratch surface is lent leave it alone, but the
    /// document must not change size meanwhile (debug builds assert it).
    #[test]
    #[should_panic(expected = "changed size while the scratch surface is lent")]
    fn lent_scratch_pins_the_document_size() {
        let mut ws = DocumentWorkspace::new(4, 4, 96.0);
        ws.add_layer(Layer::create_background_layer(4, 4));
        let lent = ws.borrow_scratch("test");
        ws.push_memento(HistoryMemento::null("Same size", ""));
        assert_eq!(lent.width(), 4);
        ws.execute_function(|t| {
            let mut d = Document::new(8, 8, 96.0);
            d.add_layer(Layer::create_background_layer(8, 8));
            *t.document = d;
            Some(HistoryMemento::new("Resize", "", MementoKind::Null))
        });
    }
}
