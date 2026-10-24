//! Photocraft phase spec: S§4.5 "Editable text" (PC15): the text-layer model, its renderer, the
//! one rasterize guard and Layers ▸ Rasterize Text Layer.
//!
//! A text layer is an ordinary layer whose `text` spec is set; its surface stays the rendered
//! cache, so compositing, thumbnails, flatten and export never read the spec. Editable text
//! ignores the selection (R75): a text layer re-renders from its spec, which has none.
//!
//! The guard: every pixel edit of a layer asks `ensure_raster` first, the only decision point. A
//! raster layer passes. A text layer queues `UiRequest::RasterizeText` (the "Rasterize text
//! layer?" OK/Cancel box, which the UI shows next frame) and refuses, so the edit does nothing.
//! OK runs `confirm`: `rasterize` (its own history item), then the command that asked again,
//! which now passes; `action::perform` names that command in the request. A refused tool press
//! is not replayed: the user paints again on the raster layer. Cancel drops the request.
//! `DocumentWorkspace::push_memento` debug-asserts that no pixel entry lands on a text layer.
//!
//! Callers: tool presses (`AppWorkspace::on_mouse_down` through `press_edits_pixels`; Move
//! Selected Pixels in its `begin`, which nudges also reach), `EffectSession::begin`, the commands
//! in `action::run` (effects, Repeat, Paste, Fill/Erase Selection, Cut), `io::clipboard::paste`
//! and `io::ops::transform_active_layer`. Merge Down and the image-wide commands (Flatten,
//! Resize, Canvas Size, Crop, Rotate, Flip) rasterize silently inside their own history item.

use serde::{Deserialize, Serialize};

use crate::action::{ActionOutcome, UiRequest};
use crate::app::AppWorkspace;
use crate::canvas::ImageData;
use crate::document::Document;
use crate::document::layer::BlendMode;
use crate::document::surface::Surface;
use crate::history::{HistoryMemento, MementoKind, TextLayerMemento};
use crate::pixel::content::{ContentBlendMode, ContentLayer, render_content};
use crate::pixel::{IntRect, Px};
use crate::tool::text::{TextSettings, layout_text, line_bounds, rasterize_text_content};
use crate::tool::types::ToolSettings;
use crate::tool::{MouseButton, ToolType};
use crate::util::geom::Rectangle;

/// The Text tool's inputs (Task 35) a text layer re-renders from. Its JSON is frozen (R43,
/// `text_spec_json_is_pinned`): missing fields take `Default`, unknown ones are ignored.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TextSpec {
    pub text: String,
    pub settings: TextSettings,
    /// The click the layout starts from (the Text tool's `click`).
    pub origin: (f64, f64),
    pub color: Px,
    /// The resolution the point size is laid out at.
    pub dpi: f64,
}

impl Default for TextSpec {
    fn default() -> Self {
        Self {
            text: String::new(),
            settings: TextSettings::default(),
            origin: (0.0, 0.0),
            color: [0, 0, 0, 255],
            dpi: 96.0,
        }
    }
}

/// The action, its menu name and the history item's name (Extra names are English, S§4.1).
pub const RASTERIZE_ACTION: &str = "menu.layers.rasterizeText";
pub const RASTERIZE_NAME: &str = "Rasterize Text Layer";
const ICON: &str = "assets/icons/text_tool_icon.png";

/// `spec` on a transparent `w × h` layer: the Text tool's own glyph raster and content kernel
/// (Normal blend, no selection), so a text layer matches what typing on an empty layer commits.
pub fn render(spec: &TextSpec, w: u32, h: u32) -> Surface {
    let mut surface = Surface::create(w, h);
    let layout = layout_text(
        &spec.text,
        &spec.settings,
        [spec.origin.0, spec.origin.1],
        spec.dpi,
    );
    let Some(bounds) = layout
        .lines
        .iter()
        .map(line_bounds)
        .reduce(|a, b| Rectangle::union(&a, &b))
    else {
        return surface;
    };
    let r = IntRect::bound_of(&bounds).intersect(&surface.int_bounds());
    if r.is_empty() {
        return surface;
    }
    let srgb = crate::effect::surface::Space::Srgb;
    let raster = rasterize_text_content(&layout, &spec.settings, r, spec.color, srgb);
    let content = [ContentLayer {
        content: &raster.content,
        mask: Some(&raster.coverage),
    }];
    let normal = ContentBlendMode::Layer(BlendMode::Normal);
    let data = render_content(surface.data(), w, r, &content, None, normal);
    let img = ImageData {
        width: r.w,
        height: r.h,
        data,
    };
    surface.put_image_data(&img, r.x, r.y);
    surface
}

/// Layer `layer` of the active document carries a text spec.
pub fn is_text(app: &AppWorkspace, layer: usize) -> bool {
    app.active_ws()
        .and_then(|ws| ws.document.layers.get_at(layer))
        .is_some_and(|l| l.text.is_some())
}

/// The guard (module doc): true for a raster layer; for a text layer, queues the prompt once and
/// returns false.
pub fn ensure_raster(app: &mut AppWorkspace, layer: usize) -> bool {
    if !is_text(app, layer) {
        return true;
    }
    let queued = app
        .ui_requests
        .iter()
        .any(|r| matches!(r, UiRequest::RasterizeText { layer: l, .. } if *l == layer));
    if !queued {
        app.ui_requests
            .push(UiRequest::RasterizeText { layer, then: None });
    }
    false
}

/// `ensure_raster` of the active layer (true without a document).
pub fn ensure_active_raster(app: &mut AppWorkspace) -> bool {
    let active = app.active_ws().map(|ws| ws.active_layer);
    active.is_none_or(|layer| ensure_raster(app, layer))
}

/// A press of `b` with `tool` starts a pixel edit of the active layer. A Clone Stamp or Healing
/// Brush Ctrl+press only sets its source; the Text tool starts on the left button, unless Editable
/// text is on (its text goes to a text layer: a new one, or the one clicked); Move Selected Pixels
/// guards in its own `begin`, which its nudges reach too, and Patch when a patch drag starts.
pub fn press_edits_pixels(tool: ToolType, b: MouseButton, ctrl: bool, s: &ToolSettings) -> bool {
    use ToolType::*;
    match tool {
        PaintBucket | Gradient | PaintBrush | Eraser | Pencil | Recolor | Line | Shapes => true,
        Remove | SpotHealingBrush => true,
        // R65: Patch's press draws a selection or grabs the patch; it asks when a patch drag
        // starts (`tool::patch`), as Move's `begin` does.
        Patch => false,
        CloneStamp | HealingBrush => !ctrl,
        Text => b == MouseButton::Left && !crate::tool::text::editable_text(s),
        RectangleSelect | Move | LassoSelect | MoveSelection | EllipseSelect | Zoom | MagicWand
        | Pan | ColorPicker => false,
        MagneticLasso | ObjectSelection => false,
    }
}

/// Clears layer `layer`'s spec, keeping its pixels: the "Rasterize Text Layer" entry, or `None`
/// for a raster layer. Merge Down puts it inside its own entry.
pub(crate) fn rasterize_memento(doc: &mut Document, layer: usize) -> Option<HistoryMemento> {
    let old = doc.layers.get_at_mut(layer)?.text.take()?;
    let kind = MementoKind::TextLayer(TextLayerMemento {
        layer,
        old: Some(old),
        new: None,
        old_surface: None,
    });
    Some(HistoryMemento::new(RASTERIZE_NAME, ICON, kind))
}

/// Layers ▸ Rasterize Text Layer, without asking: one history item; nothing on a raster layer.
pub fn rasterize(app: &mut AppWorkspace, layer: usize) {
    app.commit_tool_first(|app| {
        if let Some(ws) = app.active_ws_mut()
            && let Some(m) = rasterize_memento(&mut ws.document, layer)
        {
            ws.push_memento(m);
        }
    });
}

/// The prompt's OK: rasterize, then run `then` (the command that asked) again; a dialog it opens
/// is queued on `ui_requests`.
pub fn confirm(app: &mut AppWorkspace, layer: usize, then: Option<&str>) {
    rasterize(app, layer);
    if let Some(id) = then
        && let ActionOutcome::Ui(request) = crate::action::perform(app, id)
    {
        app.ui_requests.push(request);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{PasteKind, perform};
    use crate::fonts::FontPolicy;
    use crate::history::{BitmapMemento, functions};
    use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes};

    const W: u32 = 40;
    const H: u32 = 30;

    fn spec(app: &AppWorkspace, text: &str) -> TextSpec {
        TextSpec {
            text: text.into(),
            settings: TextSettings::from_tool_settings(&app.tool_settings, FontPolicy::Bundled),
            origin: (4.0, 6.0),
            color: [10, 20, 30, 255],
            dpi: 96.0,
        }
    }

    /// A white background and, active above it, layer 1: "Hi" as a text layer.
    fn text_app() -> AppWorkspace {
        let mut app = fresh_doc(W, H, None);
        let spec = spec(&app, "Hi");
        let ws = app.active_ws_mut().unwrap();
        ws.execute_function(functions::add_new_blank_layer);
        let layer = ws.document.layers.get_at_mut(1).unwrap();
        layer.surface = render(&spec, W, H);
        assert!(layer.surface.data().chunks(4).any(|p| p[3] > 0), "no ink");
        layer.text = Some(spec);
        ws.invalidate(None);
        assert_eq!(ws.active_layer, 1);
        // The raster Text tool unless a test turns Editable text on.
        app.tool_settings
            .set(ToolType::Text, "editableText", false.into());
        app
    }

    fn history(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    fn text(app: &AppWorkspace, i: usize) -> Option<TextSpec> {
        app.active_ws()
            .unwrap()
            .document
            .layers
            .get_at(i)?
            .text
            .clone()
    }

    fn asked(layer: usize, then: Option<&'static str>) -> Vec<UiRequest> {
        vec![UiRequest::RasterizeText { layer, then }]
    }

    fn stroke() -> [Ev; 3] {
        [
            Ev::Down(5.0, 5.0, MouseButton::Left),
            Ev::Move(30.0, 20.0),
            Ev::Up(30.0, 20.0, MouseButton::Left),
        ]
    }

    #[test]
    fn painting_on_a_text_layer_asks_to_rasterize() {
        use ToolType::*;
        for tool in [
            PaintBrush,
            Pencil,
            Eraser,
            Recolor,
            CloneStamp,
            PaintBucket,
            Gradient,
            Line,
            Shapes,
            Text,
            Move,
        ] {
            let mut app = text_app();
            let (pixels, items) = (layer_bytes(&app, None), history(&app));
            drive_tool(&mut app, tool, &stroke());
            app.set_active_tool_from_type(ToolType::Zoom);
            assert_eq!(app.ui_requests, asked(1, None), "{tool:?}");
            // Cancel drops the request: nothing changed and no history.
            app.ui_requests.clear();
            assert_eq!(layer_bytes(&app, None), pixels, "{tool:?}");
            assert_eq!(history(&app), items, "{tool:?}");
            assert!(text(&app, 1).is_some(), "{tool:?}");
        }
        // A Move Selected Pixels nudge asks too.
        let mut app = text_app();
        drive_tool(&mut app, Move, &[Ev::Key("ArrowRight")]);
        assert_eq!(app.ui_requests, asked(1, None));
        // Selecting (a drag held past the 50-ms click) and setting a clone source edit no pixels.
        let mut app = text_app();
        let [down, drag, up] = stroke();
        drive_tool(&mut app, RectangleSelect, &[down, drag, Ev::Wait(60.0), up]);
        let ctrl = Ev::DownMods(5.0, 5.0, MouseButton::Left, "Ctrl");
        drive_tool(
            &mut app,
            CloneStamp,
            &[ctrl, Ev::Up(5.0, 5.0, MouseButton::Left)],
        );
        assert!(app.ui_requests.is_empty());
        assert!(!app.active_ws().unwrap().selection.is_empty());
        assert!(!app.clone_anchors.is_empty());
    }

    /// R65: drawing a Patch selection on a text layer edits no pixels, so it asks nothing; a patch
    /// drag inside that selection asks first (as Move's begin) and changes nothing.
    #[test]
    fn patch_asks_to_rasterize_only_when_patching() {
        let mut app = text_app();
        let lasso = [
            Ev::Down(5.0, 5.0, MouseButton::Left),
            Ev::Move(30.0, 5.0),
            Ev::Move(30.0, 20.0),
            Ev::Move(5.0, 20.0),
            Ev::Wait(60.0),
            Ev::Up(5.0, 20.0, MouseButton::Left),
        ];
        drive_tool(&mut app, ToolType::Patch, &lasso);
        assert!(app.ui_requests.is_empty());
        assert!(!app.active_ws().unwrap().selection.is_empty());
        let (pixels, items) = (layer_bytes(&app, None), history(&app));
        let patch = [
            Ev::Down(15.0, 12.0, MouseButton::Left),
            Ev::Move(25.0, 15.0),
            Ev::Up(25.0, 15.0, MouseButton::Left),
        ];
        drive_tool(&mut app, ToolType::Patch, &patch);
        assert_eq!(app.ui_requests, asked(1, None));
        assert!(!app.tool_busy());
        assert_eq!(layer_bytes(&app, None), pixels);
        assert_eq!(history(&app), items);
        assert!(text(&app, 1).is_some());
    }

    #[test]
    fn ok_rasterizes_then_paints_as_two_items() {
        let mut app = text_app();
        let (pixels, items) = (layer_bytes(&app, None), history(&app));
        drive_tool(&mut app, ToolType::PaintBrush, &stroke());
        let Some(UiRequest::RasterizeText { layer, then }) = app.ui_requests.pop() else {
            panic!("no prompt");
        };
        confirm(&mut app, layer, then);
        assert_eq!(history(&app)[items.len()..], [RASTERIZE_NAME]);
        assert_eq!(text(&app, 1), None);
        assert_eq!(layer_bytes(&app, None), pixels);
        drive_tool(&mut app, ToolType::PaintBrush, &stroke());
        app.set_active_tool_from_type(ToolType::Zoom);
        assert!(app.ui_requests.is_empty());
        assert_ne!(layer_bytes(&app, None), pixels);
        assert_eq!(history(&app).len(), items.len() + 2);
    }

    #[test]
    fn effects_and_paste_go_through_the_guard() {
        for id in [
            "menu.effects.blurEffect",
            "adjustment.invertColorsEffect",
            "menu.edit.paste",
            "menu.edit.fillSelection",
            "menu.edit.eraseSelection",
            "menu.edit.cut",
            "menu.layers.flipHorizontal",
            "menu.layers.rotateZoom",
        ] {
            let mut app = text_app();
            perform(&mut app, "menu.edit.selectAll");
            let (pixels, items) = (layer_bytes(&app, None), history(&app));
            assert_eq!(perform(&mut app, id), ActionOutcome::Done, "{id}");
            assert_eq!(app.ui_requests, asked(1, Some(id)), "{id}");
            app.ui_requests.clear();
            assert_eq!(layer_bytes(&app, None), pixels, "{id}");
            assert_eq!(history(&app), items, "{id}");
        }
        // Repeat, and a session begun directly.
        let mut app = text_app();
        app.last_effect = Some("invertColorsEffect".into());
        perform(&mut app, "menu.effects.repeat");
        assert_eq!(app.ui_requests, asked(1, Some("menu.effects.repeat")));
        app.ui_requests.clear();
        let invert = crate::effect::find("invertColorsEffect").unwrap();
        assert!(crate::effect::session::EffectSession::begin(&mut app, invert).is_none());
        assert_eq!(app.ui_requests, asked(1, None));
        // OK rasterizes and runs the command again: an instant effect applies, Paste reaches the UI.
        let mut app = text_app();
        let (pixels, items) = (layer_bytes(&app, None), history(&app));
        perform(&mut app, "adjustment.invertColorsEffect");
        let Some(UiRequest::RasterizeText { layer, then }) = app.ui_requests.pop() else {
            panic!("no prompt");
        };
        confirm(&mut app, layer, then);
        assert_eq!(history(&app).len(), items.len() + 2);
        assert_eq!(history(&app)[items.len()], RASTERIZE_NAME);
        assert_ne!(layer_bytes(&app, None), pixels);
        let mut app = text_app();
        confirm(&mut app, 1, Some("menu.edit.paste"));
        assert_eq!(app.ui_requests, [UiRequest::Paste(PasteKind::Paste)]);
        // The paste itself refuses too: the web's paste event reaches it without `perform`.
        use crate::io::clipboard::{PasteExpand, PasteOutcome, paste};
        let mut app = text_app();
        perform(&mut app, "menu.edit.copy");
        let item = app.clipboard.item.clone().unwrap();
        let pixels = layer_bytes(&app, None);
        assert_eq!(
            paste(&mut app, &item, PasteExpand::Keep),
            PasteOutcome::Failed
        );
        assert_eq!(app.ui_requests, asked(1, None));
        assert_eq!(layer_bytes(&app, None), pixels);
    }

    /// Ruling R32: Content-Aware Fill writes pixels outside the tool path, so it asks first; OK
    /// rasterizes, then the fill runs to its own entry.
    #[test]
    fn content_aware_fill_asks_to_rasterize_first() {
        let id = "menu.edit.fillSelectionContentAware";
        let mut app = text_app();
        let ws = app.active_ws_mut().unwrap();
        let r = Rectangle::new(8.0, 8.0, 4.0, 4.0);
        ws.selection
            .set_continuation(&r, crate::selection::CombineMode::Replace);
        ws.selection.commit_continuation();
        let items = history(&app);
        assert_eq!(perform(&mut app, id), ActionOutcome::Done);
        assert_eq!(app.ui_requests, asked(1, Some(id)));
        assert!(app.content_fill.is_none());
        assert_eq!(history(&app), items);
        app.ui_requests.clear();
        confirm(&mut app, 1, Some(id));
        assert_eq!(text(&app, 1), None);
        let start = std::time::Instant::now();
        while app.tool_busy() {
            assert!(start.elapsed().as_secs() < 120, "the fill never finished");
            std::thread::sleep(std::time::Duration::from_millis(2));
            app.on_frame(app.now_ms + 16.0);
        }
        assert!(app.ui_requests.is_empty(), "{:?}", app.ui_requests);
        assert_eq!(
            history(&app)[items.len()..],
            [RASTERIZE_NAME, "Fill Selection (Content-Aware)"]
        );
    }

    /// PC16: with Editable text on, a Text press on a text layer edits the text, so it never asks;
    /// with the toggle off the press asks again, as in Paint.NET. A Healing Brush Ctrl+press only
    /// sets its source.
    #[test]
    fn editable_text_press_does_not_ask_until_toggled_off() {
        let press = [
            Ev::Down(30.0, 25.0, MouseButton::Left),
            Ev::Up(30.0, 25.0, MouseButton::Left),
        ];
        let mut app = text_app();
        app.tool_settings
            .set(ToolType::Text, "editableText", true.into());
        drive_tool(&mut app, ToolType::Text, &press);
        app.set_active_tool_from_type(ToolType::Zoom);
        assert!(app.ui_requests.is_empty());
        app.tool_settings
            .set(ToolType::Text, "editableText", false.into());
        assert!(!crate::tool::text::editable_text(&app.tool_settings));
        drive_tool(&mut app, ToolType::Text, &press);
        assert_eq!(app.ui_requests, asked(1, None));
        let s = &app.tool_settings;
        let heal = |ctrl| press_edits_pixels(ToolType::HealingBrush, MouseButton::Left, ctrl, s);
        assert!(heal(false) && !heal(true));
    }

    #[test]
    fn flatten_rasterizes_silently() {
        for id in [
            "menu.image.flatten",
            "menu.image.rotate180",
            "menu.image.flipVertical",
            "menu.image.crop",
        ] {
            let mut app = text_app();
            perform(&mut app, "menu.edit.selectAll");
            let items = history(&app).len();
            perform(&mut app, id);
            assert!(app.ui_requests.is_empty(), "{id}");
            assert_eq!(history(&app).len(), items + 1, "{id}");
            let ws = app.active_ws().unwrap();
            assert!(ws.document.layers.iter().all(|l| l.text.is_none()), "{id}");
            // One undo brings the editable layer back.
            assert!(app.undo());
            assert_eq!(text(&app, 1), Some(spec(&app, "Hi")), "{id}");
        }
        // Merge Down onto a text layer: one item, the text gone, back on undo.
        let mut app = text_app();
        perform(&mut app, "menu.layers.add.new.layer");
        let items = history(&app).len();
        perform(&mut app, "menu.layers.merge.layer.down");
        assert!(app.ui_requests.is_empty());
        assert_eq!(history(&app).len(), items + 1);
        assert_eq!(text(&app, 1), None);
        assert!(app.undo());
        assert_eq!(text(&app, 1), Some(spec(&app, "Hi")));
        assert!(app.redo());
        assert_eq!(text(&app, 1), None);
    }

    #[test]
    fn text_edit_and_rasterize_undo() {
        let mut app = text_app();
        let old = text(&app, 1);
        let old_pixels = layer_bytes(&app, Some(1));
        let new = spec(&app, "Hello");
        // A text edit as PC16 records it: the specs and the pixels before.
        let ws = app.active_ws_mut().unwrap();
        let layer = ws.document.layers.get_at_mut(1).unwrap();
        let old_surface = std::mem::replace(&mut layer.surface, render(&new, W, H));
        layer.text = Some(new.clone());
        let kind = MementoKind::TextLayer(TextLayerMemento {
            layer: 1,
            old: old.clone(),
            new: Some(new.clone()),
            old_surface: Some(old_surface),
        });
        ws.push_memento(HistoryMemento::new("Text", "", kind));
        let new_pixels = layer_bytes(&app, Some(1));
        assert_ne!(new_pixels, old_pixels);
        assert!(app.undo());
        assert_eq!(
            (text(&app, 1), layer_bytes(&app, Some(1))),
            (old.clone(), old_pixels)
        );
        assert!(app.redo());
        let edited = (text(&app, 1), layer_bytes(&app, Some(1)));
        assert_eq!(edited, (Some(new.clone()), new_pixels.clone()));
        // Rasterize: one item that keeps the pixels; undo brings the spec back.
        let items = history(&app).len();
        perform(&mut app, RASTERIZE_ACTION);
        assert_eq!(history(&app).len(), items + 1);
        assert_eq!(
            (text(&app, 1), layer_bytes(&app, Some(1))),
            (None, new_pixels.clone())
        );
        assert!(app.undo());
        assert_eq!(
            (text(&app, 1), layer_bytes(&app, Some(1))),
            (Some(new), new_pixels)
        );
        assert!(app.redo());
        assert_eq!(text(&app, 1), None);
    }

    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "ensure_raster")]
    fn bitmap_memento_on_text_layer_without_guard_panics_in_debug() {
        let mut app = text_app();
        let ws = app.active_ws_mut().unwrap();
        let m = BitmapMemento::capture(&ws.document, 1, None);
        ws.push_memento(HistoryMemento::new("Draw", "", MementoKind::Bitmap(m)));
    }

    /// A transacted tool's commit (a Finish entry holding the pixels) is checked too.
    #[test]
    #[cfg(debug_assertions)]
    #[should_panic(expected = "ensure_raster")]
    fn tool_finish_on_text_layer_without_guard_panics_in_debug() {
        use crate::tool::transacted::{Step, ToolEntry};
        let mut app = text_app();
        let ws = app.active_ws_mut().unwrap();
        let pixels = MementoKind::Bitmap(BitmapMemento::capture(&ws.document, 1, None));
        let inner = HistoryMemento::new("Paint Bucket", "", pixels);
        let entry = ToolEntry {
            tool: ToolType::PaintBucket,
            layer: 1,
            step: Step::Finish(Box::new(()), Some(Box::new(inner))),
        };
        ws.push_memento(HistoryMemento::new("Finish", "", MementoKind::Tool(entry)));
    }

    /// R43: the spec's JSON is frozen (PC17 stores it in the .pdn). The font policy is the
    /// environment's, not stored; missing fields take the Text tool's defaults and unknown ones
    /// are ignored, so older and newer files load.
    #[test]
    fn text_spec_json_is_pinned() {
        use crate::tool::text::{FontSizeMetric, TextAlign, TextRenderingMode};
        let settings = TextSettings {
            family: "Calibri".into(),
            size: 12.0,
            metric: FontSizeMetric::Fixed96Dpi,
            bold: true,
            italic: false,
            underline: false,
            strikeout: false,
            mode: TextRenderingMode::Classic,
            align: TextAlign::Center,
            antialias: true,
            policy: FontPolicy::System,
        };
        let s = TextSpec {
            text: "Hi\nthere".into(),
            settings,
            origin: (4.0, 6.5),
            color: [10, 20, 30, 255],
            dpi: 96.0,
        };
        let json = r#"{"text":"Hi\nthere","settings":{"family":"Calibri","size":12.0,"metric":"pixels","bold":true,"italic":false,"underline":false,"strikeout":false,"mode":"aliased","align":"center","antialias":true},"origin":[4.0,6.5],"color":[10,20,30,255],"dpi":96.0}"#;
        assert_eq!(serde_json::to_string(&s).unwrap(), json);
        let mut loaded: TextSpec = serde_json::from_str(json).unwrap();
        assert_eq!(loaded.settings.policy, TextSettings::default().policy);
        loaded.settings.policy = FontPolicy::System;
        assert_eq!(loaded, s);
        for (other, metric, mode, align) in [
            ("points", FontSizeMetric::Points, "smooth", TextAlign::Left),
            ("points", FontSizeMetric::Points, "sharp", TextAlign::Right),
        ] {
            let json = format!(
                r#"{{"settings":{{"metric":"{other}","mode":"{mode}","align":"{}"}}}}"#,
                if align == TextAlign::Left {
                    "left"
                } else {
                    "right"
                }
            );
            let t: TextSpec = serde_json::from_str(&json).unwrap();
            assert_eq!((t.settings.metric, t.settings.align), (metric, align));
        }
        let old: TextSpec = serde_json::from_str(r#"{"text":"A","future":1}"#).unwrap();
        assert_eq!(
            old,
            TextSpec {
                text: "A".into(),
                ..TextSpec::default()
            }
        );
        let app = fresh_doc(4, 4, None);
        assert_eq!(TextSpec::default().settings, spec(&app, "").settings);
    }
}
