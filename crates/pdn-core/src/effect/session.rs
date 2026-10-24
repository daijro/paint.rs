//! One effect invocation: the source layer, the selection mask and the instance seed taken once,
//! previews written into the layer, then OK (a Bitmap memento) or Cancel (the layer and the dirty
//! flag restored). Plus the instant adjustments and Repeat.
//! Port of: js/src/engine/document/history/action/actions/document/effect/BitmapEffectAction.js
//! (`performAction` flow, `rememberedValues`, `lastEffect`, `rememberSuccessfulEffect`),
//! RepeatEffectAction.js (`performAction`).
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's invocation flow, instance seeds, Repeat and remembered tokens.

use std::sync::Arc;

use super::rng::CivilDate;
use super::{
    Category, Control, DEFAULT_PALETTE_PX, EffectDef, EffectEnv, LayerInfo, Values, apply,
    defaults, find,
};
use crate::app::{AppEvent, AppWorkspace};
use crate::canvas::ImageData;
use crate::document::workspace::DocumentWorkspace;
use crate::history::{BitmapMemento, HistoryMemento, MementoKind};
use crate::pixel::mask::{Mask, SelectionQuality};
use crate::pixel::{IntRect, Px};
use crate::util::geom::Rectangle;

/// An effect run over the active layer of one document (§2.2 step 1). Owns copies of its inputs
/// (no borrow of the app) and is `Send`; clones share them, so a preview worker renders from a
/// clone.
#[derive(Clone)]
pub struct EffectSession {
    pub def: EffectDef,
    source: Arc<ImageData>,
    image: Option<Arc<ImageData>>,
    lut: Option<Arc<crate::extra::fx::lut::LutFile>>,
    mask: Arc<Option<Mask>>,
    bounds: IntRect,
    primary: Px,
    secondary: Px,
    instance_seed: u32,
    document_id: u64,
    layer_index: usize,
    dirty: bool,
    /// Every layer's name and pixels, only for an effect with a `Control::LayerSelect`.
    layers: Arc<Vec<LayerInfo>>,
    /// Every layer's name, bottom first.
    layer_names: Arc<Vec<String>>,
    palette: Arc<Vec<Px>>,
    /// The selection outline; empty when nothing is selected.
    selection: Arc<Vec<Vec<[f64; 2]>>>,
    today: CivilDate,
    /// R152 (Q54): the applied numeric culture, captured once; Repeat and reopen recapture it.
    culture: String,
}

impl EffectSession {
    /// Takes the active layer and the selection mask (at the shared rendering quality) once,
    /// draws one instance seed and records the dirty flag. `None` without a document or layer.
    /// Also takes what plugin effects read (BB common.md §5.11): the layer names, the
    /// layers' pixels for a layer drop-down, the palette, the selection outline and the date.
    /// The active tool has already committed: every effect command runs through the
    /// commit-first hook (`action::perform`, painting-tools-2.md §7.6).
    pub fn begin(app: &mut AppWorkspace, def: EffectDef) -> Option<EffectSession> {
        // S§4.5: an effect on a text layer asks to rasterize it first.
        if !crate::document::text_layer::ensure_active_raster(app) {
            return None;
        }
        let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
        let c = |c: crate::util::color::Color| [c.red, c.green, c.blue, c.alpha];
        let (primary, secondary) = (c(app.primary_color), c(app.secondary_color));
        let instance_seed = app.next_instance_seed();
        let ws = app.active_ws()?;
        let surface = &ws.document.layers.get_at(ws.active_layer)?.surface;
        let (w, h) = (surface.width(), surface.height());
        // ponytail: a layer drop-down copies every layer's pixels (20 layers at 4K ≈ 660 MB); share
        // the surfaces or fetch the selected layer lazily if documents that big matter.
        let reads_layers = def
            .controls
            .iter()
            .any(|c| matches!(c, Control::LayerSelect { .. }));
        let layers = ws.document.layers.iter();
        let palette = match &app.current_palette {
            p if p.is_empty() => DEFAULT_PALETTE_PX.to_vec(),
            p => p.clone(),
        };
        let selection = if ws.selection.is_empty() {
            vec![]
        } else {
            ws.selection.clipping_polygons(IntRect::new(0, 0, w, h))
        };
        Some(EffectSession {
            layers: Arc::new(if reads_layers {
                layers
                    .clone()
                    .map(|l| LayerInfo {
                        name: l.properties.name.clone(),
                        pixels: Arc::new(l.surface.get_image_data(0, 0, w, h)),
                    })
                    .collect()
            } else {
                vec![]
            }),
            layer_names: Arc::new(layers.map(|l| l.properties.name.clone()).collect()),
            palette: Arc::new(palette),
            selection: Arc::new(selection),
            today: app.date_override.unwrap_or_else(CivilDate::today),
            culture: crate::i18n::culture_code(),
            source: Arc::new(surface.get_image_data(0, 0, w, h)),
            image: None,
            lut: None,
            mask: Arc::new(super::selection_mask(&ws.selection, quality, w, h)),
            bounds: super::selection_bounds(&ws.selection, w, h),
            primary,
            secondary,
            instance_seed,
            document_id: ws.document.id,
            layer_index: ws.active_layer,
            dirty: ws.dirty,
            def,
        })
    }

    /// The remembered token, else the defaults (§2.2 step 3).
    pub fn initial_values(&self, app: &AppWorkspace) -> Values {
        app.effect_tokens
            .get(self.def.id)
            .cloned()
            .unwrap_or_else(|| self.defaults())
    }

    /// The static defaults, then the plugin's dynamic ones for this document.
    pub fn defaults(&self) -> Values {
        super::bb::indirect_defaults(&self.def, &self.env())
    }

    /// Every layer's name, bottom first (a layer drop-down's entries, a current-layer label).
    pub fn layer_names(&self) -> Vec<String> {
        self.layer_names.to_vec()
    }

    /// What the effect sees besides its values.
    pub fn env(&self) -> EffectEnv<'_> {
        EffectEnv {
            image: self.image.as_deref(),
            lut: self.lut.as_deref(),
            render_info: None,
            notice: None,
            cancel: None,
            width: self.source.width,
            height: self.source.height,
            bounds: self.bounds,
            mask: self.mask(),
            primary: self.primary,
            secondary: self.secondary,
            instance_seed: self.instance_seed,
            layers: &self.layers,
            layer_index: self.layer_index,
            palette: &self.palette,
            selection: &self.selection,
            today: self.today,
            culture: &self.culture,
        }
    }

    /// Replaces the invocation's external bitmap; worker clones retain their immutable snapshot.
    pub fn set_image(&mut self, image: Option<Arc<ImageData>>) {
        self.image = image;
    }

    /// Replaces the invocation's LUT file (`External::Lut`), as `set_image` its bitmap.
    pub fn set_lut(&mut self, lut: Option<Arc<crate::extra::fx::lut::LutFile>>) {
        self.lut = lut;
    }

    /// The original document and layer still own this invocation, even in a background tab.
    pub fn owner_exists(&self, app: &AppWorkspace) -> bool {
        app.documents.iter().any(|ws| {
            ws.document.id == self.document_id
                && ws.document.layers.get_at(self.layer_index).is_some()
        })
    }

    /// The layer as it was when the session began.
    pub fn source(&self) -> &ImageData {
        &self.source
    }

    /// The selection's clipping mask; `None` when nothing or everything is selected.
    pub fn mask(&self) -> Option<&Mask> {
        self.mask.as_ref().as_ref()
    }

    /// The effect over the source with `values`; pure, for worker threads.
    pub fn render(&self, values: &Values) -> ImageData {
        self.render_with(values, None).0
    }

    /// A seam-only adapter shares immutable inputs and the same bounded clipping kernel.
    pub fn seam_carver(&self, values: &Values) -> Option<super::bb::seam::Carver> {
        super::bb::seam::orientation(self.def.id).map(|o| {
            super::bb::seam::Carver::new(
                self.source.clone(),
                super::num(values, "amount") as u32,
                o,
            )
            .with_mask(self.mask.clone())
        })
    }

    /// Render with borrowed cancellation and a per-call notice channel, never a UI callback.
    pub fn render_with(
        &self,
        values: &Values,
        cancel: Option<&std::sync::atomic::AtomicBool>,
    ) -> (ImageData, Option<String>) {
        if let Some(mut job) = self.seam_carver(values) {
            use super::bb::seam::{CarveStatus, shortfall_message};
            loop {
                match job.step(4096, cancel) {
                    CarveStatus::Pending => {}
                    CarveStatus::Complete(out, cut) => {
                        return (
                            out,
                            shortfall_message(super::num(values, "amount") as u32, cut),
                        );
                    }
                    CarveStatus::Cancelled => return (self.source.as_ref().clone(), None),
                }
            }
        }
        let notice = std::sync::Mutex::new(None);
        let mut env = self.env();
        env.notice = Some(&notice);
        env.cancel = cancel;
        let out = apply(&self.source, &self.def, values, &env);
        (out, notice.into_inner().unwrap())
    }

    fn workspace<'a>(&self, app: &'a mut AppWorkspace) -> Option<&'a mut DocumentWorkspace> {
        app.documents
            .iter_mut()
            .find(|ws| ws.document.id == self.document_id)
    }

    /// Writes `pixels` (a preview), or the source when `None`, into the layer and invalidates. The
    /// document stays clean (paintdotjs: the references show no dirty mark during a preview).
    pub fn show(&self, app: &mut AppWorkspace, pixels: Option<&ImageData>) {
        let index = self.layer_index;
        let Some(ws) = self.workspace(app) else {
            return;
        };
        if let Some(layer) = ws.document.layers.get_at_mut(index) {
            layer
                .surface
                .put_image_data(pixels.unwrap_or(&self.source), 0, 0);
            ws.invalidate(None);
        }
    }

    /// OK (§2.2 steps 5–7): renders the final, pushes a Bitmap memento named after the effect
    /// with its icon, remembers the token; an Effects-menu effect becomes the one Repeat runs.
    pub fn commit(self, app: &mut AppWorkspace, values: Values) -> bool {
        self.commit_with(app, values, None)
    }

    /// `commit` with the final already rendered (`rendered` must be `render(&values)` of this
    /// session: a finished current preview, byte-identical by the shared instance seed).
    pub fn commit_with(
        self,
        app: &mut AppWorkspace,
        values: Values,
        rendered: Option<ImageData>,
    ) -> bool {
        let out = rendered.unwrap_or_else(|| self.render(&values));
        let mut values = values;
        if let Some(ui) = self.def.indirect {
            for key in ui.transient_keys {
                values.remove(*key);
            }
        }
        let def = &self.def;
        let name = def.display_name();
        let icon = format!("assets/icons/{}", def.icon);
        let Some(ws) = self.workspace(app) else {
            return false;
        };
        let Some(layer) = ws.document.layers.get_at_mut(self.layer_index) else {
            return false;
        };
        layer.surface.put_image_data(&out, 0, 0);
        ws.invalidate(None);
        let source = Arc::unwrap_or_clone(self.source);
        let whole = Rectangle::new(0.0, 0.0, source.width as f64, source.height as f64);
        ws.push_memento(HistoryMemento::new(
            &name,
            &icon,
            MementoKind::Bitmap(BitmapMemento {
                layer_index: self.layer_index,
                chunks: vec![(whole, source)],
            }),
        ));
        if let Some(m) = ws.history.undo.last() {
            ws.fade_source = Some((m.id, name));
        }
        if let Category::Effect(_) = def.category {
            app.last_effect = Some(def.id.to_string());
            app.last_effect_values = Some(values.clone());
        }
        app.effect_tokens.insert(def.id.to_string(), values);
        app.fire(AppEvent::EffectApplied);
        true
    }

    /// Cancel (§2.2 step 5): the source back in the layer, the dirty flag restored.
    pub fn cancel(self, app: &mut AppWorkspace) {
        self.show(app, None);
        if let Some(ws) = self.workspace(app) {
            ws.set_dirty(self.dirty);
        }
    }
}

/// A non-configurable effect (§2.2 step 2): straight to the final render with the defaults.
pub fn run_instant(app: &mut AppWorkspace, def: EffectDef) -> bool {
    let values = defaults(&def);
    EffectSession::begin(app, def).is_some_and(|s| s.commit(app, values))
}

/// Effects ▸ Repeat (§2.7): the last OK'd effect and values on a fresh instance, no dialog; a
/// no-op without a document, a layer or a last effect.
pub fn repeat_last(app: &mut AppWorkspace) -> bool {
    let Some(def) = app.last_effect.as_deref().and_then(find) else {
        return false;
    };
    let values = app.last_effect_values.clone().unwrap_or_default();
    EffectSession::begin(app, def).is_some_and(|s| s.commit(app, values))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{fresh_doc, layer_bytes};
    use serde_json::json;

    fn app() -> crate::app::AppWorkspace {
        let mut app = fresh_doc(8, 6, Some(4));
        app.effect_seed = Some(0);
        app
    }

    /// §2.2 step 5: Cancel restores the layer and the dirty flag; no history entry.
    #[test]
    fn preview_then_cancel_restores() {
        let mut app = app();
        let before = layer_bytes(&app, None);
        let history = app.active_ws().unwrap().history.undo.len();
        let s = EffectSession::begin(&mut app, crate::effect::find("invertColorsEffect").unwrap())
            .unwrap();
        let out = s.render(&crate::effect::Values::new());
        s.show(&mut app, Some(&out));
        assert_ne!(layer_bytes(&app, None), before);
        s.cancel(&mut app);
        assert_eq!(layer_bytes(&app, None), before);
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.history.undo.len(), ws.dirty), (history, false));
    }

    /// §2.2 steps 6–7, §2.7: OK pushes a memento, remembers the token; only Effects set Repeat.
    #[test]
    fn commit_records_tokens_and_last_effect() {
        let mut app = app();
        let s =
            EffectSession::begin(&mut app, crate::effect::find("sepiaEffect").unwrap()).unwrap();
        let v = crate::effect::with(
            &crate::effect::find("sepiaEffect").unwrap(),
            &json!({"intensity": 70}),
        );
        assert!(s.commit(&mut app, v.clone()));
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().image,
            "assets/icons/sepia_effect.png"
        );
        assert_eq!(app.effect_tokens.get("sepiaEffect"), Some(&v));
        assert!(app.last_effect.is_none(), "adjustments never set Repeat");
        let s = EffectSession::begin(&mut app, crate::effect::rotate_zoom()).unwrap();
        s.commit(
            &mut app,
            crate::effect::defaults(&crate::effect::rotate_zoom()),
        );
        assert!(
            app.last_effect.is_none(),
            "Rotate/Zoom never becomes the Repeat target"
        );
        let s =
            EffectSession::begin(&mut app, crate::effect::find("addNoiseEffect").unwrap()).unwrap();
        s.commit(
            &mut app,
            crate::effect::defaults(&crate::effect::find("addNoiseEffect").unwrap()),
        );
        assert_eq!(app.last_effect.as_deref(), Some("addNoiseEffect"));
    }

    /// §2.6 step 5: preview and final share a seed; Repeat creates a new instance (a new pattern).
    #[test]
    fn repeat_uses_a_fresh_instance_seed() {
        let mut app = app();
        let def = crate::effect::find("addNoiseEffect").unwrap();
        let s = EffectSession::begin(&mut app, def.clone()).unwrap();
        let preview = s.render(&crate::effect::defaults(&def));
        s.commit(&mut app, crate::effect::defaults(&def));
        let first = layer_bytes(&app, None);
        app.undo();
        let s2 = EffectSession::begin(&mut app, def.clone()).unwrap();
        assert_ne!(s2.render(&crate::effect::defaults(&def)).data, preview.data);
        s2.cancel(&mut app);
        assert!(repeat_last(&mut app));
        assert_ne!(layer_bytes(&app, None), first);
        assert_eq!(app.last_effect_values, Some(crate::effect::defaults(&def)));
    }

    /// Plan BB1: a plugin's history entry carries its untranslated manifest name; the built-ins
    /// keep their translated one.
    #[test]
    fn commit_names_the_memento_by_display_name() {
        let mut app = app();
        let invert = crate::effect::find("invertColorsEffect").unwrap();
        let def = EffectDef {
            id: "bbDream",
            ..invert.clone()
        };
        for (def, name) in [(def, "Dream"), (invert, "Invert Colors")] {
            let s = EffectSession::begin(&mut app, def).unwrap();
            s.commit(&mut app, crate::effect::Values::new());
            let ws = app.active_ws().unwrap();
            assert_eq!(ws.history.undo.last().unwrap().name, name);
        }
    }

    #[test]
    fn instant_adjustment_and_undo() {
        let mut app = app();
        let before = layer_bytes(&app, None);
        assert!(run_instant(
            &mut app,
            crate::effect::find("invertColorsEffect").unwrap()
        ));
        assert_ne!(layer_bytes(&app, None), before);
        app.undo();
        assert_eq!(layer_bytes(&app, None), before);
    }
    /// BB26/R155: worker clones borrow their own immutable image snapshot; None explicitly clears.
    #[test]
    fn clipboard_image_snapshot_and_original_owner() {
        let mut app = app();
        let def = crate::effect::find("bbPasteAlpha").unwrap();
        let mut s = EffectSession::begin(&mut app, def).unwrap();
        assert!(s.env().image.is_none());
        let image = Arc::new(ImageData {
            width: 1,
            height: 1,
            data: vec![12, 34, 56, 255],
        });
        s.set_image(Some(image.clone()));
        let worker = s.clone();
        assert!(std::ptr::eq(s.env().image.unwrap(), image.as_ref()));
        s.set_image(None);
        assert!(s.env().image.is_none());
        assert_eq!(worker.env().image, Some(image.as_ref()));
        let original = app.active.unwrap();
        let history = app.documents[original].history.undo.len();
        app.create_blank_document_in_new_workspace(8, 6, 96., true);
        let other = layer_bytes(&app, None);
        assert!(worker.owner_exists(&app));
        let v = worker.defaults();
        assert!(worker.commit(&mut app, v));
        assert_eq!(layer_bytes(&app, None), other);
        assert_eq!(app.documents[original].history.undo.len(), history + 1);
        app.documents[original].document.id += 100000;
        assert!(!s.owner_exists(&app));
        let h = app.effect_tokens.clone();
        assert!(!s.commit(&mut app, Values::new()));
        assert_eq!(app.effect_tokens, h);
    }
}
