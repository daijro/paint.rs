//! The clipboard flows: Copy / Copy Merged (an unmasked bitmap, its masked copy and the clipping
//! geometry), Paste (position, expand, the floating Move Selected Pixels transaction), Paste into
//! New Layer / New Image, and Copy / Paste Selection's JSON text. The platform clipboard is the
//! UI's (`paintrs::platform`); this module is the in-app clipboard and every document change.
//! Port of: js/src/io/DocumentIO.js (`copySelection`, `getClipboardSourceBounds`,
//! `getPastePosition`, `pasteBlob`, `pasteIntoNewLayer`, `pasteIntoNewImage`,
//! `copySelectionOutline`, `pasteSelectionOutline`)
//! Reference: paintdotjs @ b4e6fc5
//! The flows follow Paint.NET; a paste composites with b = 0.
//!
//! Rulings P2: the in-app item keeps the **unmasked** bitmap with the clipping geometry and the
//! paste re-masks with the final selection mask (§6 item 5, inferred; Appendix B B-S3); the system
//! clipboard gets the masked bitmap, and an image there is ours when its bytes are the masked
//! copy's (paintdotjs's digest idea, compared directly).

use crate::app::{AppEvent, AppWorkspace};
use crate::canvas::ImageData;
use crate::document::layer::Layer;
use crate::document::surface::Surface;
use crate::history::{HistoryMemento, MementoKind, functions};
use crate::i18n::{t, tf};
use crate::io::ops::{self, Anchor, CanvasFill};
use crate::pixel::mask::{self, SelectionQuality};
use crate::pixel::{IntRect, ops::apply_alpha_mask};
use crate::selection::matrix::Matrix;
use crate::selection::{CombineMode, path::GraphicsPath};
use crate::tool::ToolType;
use crate::tool::move_tools::MoveTool;
use crate::util::geom::{Point, Rectangle};
use std::sync::Arc;

/// One copy: `bitmap` is the source's integer-bounds crop of the clipping
/// geometry, `masked` the same through the clipping mask (what the system clipboard gets), and
/// `geometry` the clipping geometry in document coordinates; `bounds` places the bitmap. The
/// pixels are shared, so a paste (or a clone of the item) never copies them.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipItem {
    pub bitmap: Arc<Surface>,
    pub masked: Arc<ImageData>,
    pub geometry: GraphicsPath,
    pub bounds: IntRect,
}

impl ClipItem {
    /// §2.11 step 3, the paste bounds: the bounds of the geometry's interior scans (the bitmap's
    /// bounds when it has none).
    pub fn paste_bounds(&self) -> IntRect {
        let scans = mask::scans(&self.geometry.as_polys(), 1.0);
        let (Some(first), Some(last)) = (scans.first(), scans.last()) else {
            return self.bounds;
        };
        let x0 = scans.iter().map(|s| s.1).min().unwrap_or(0);
        let x1 = scans.iter().map(|s| s.2).max().unwrap_or(0);
        IntRect::new(
            x0,
            first.0,
            (x1 - x0).max(0) as u32,
            (last.0 + 1 - first.0) as u32,
        )
    }
}

/// The in-app clipboard (`DocumentIO.internalClipboard`); the selection outline is
/// `AppWorkspace::internal_selection_path`.
#[derive(Clone, Debug, Default)]
pub struct InternalClipboard {
    pub item: Option<ClipItem>,
}

impl InternalClipboard {
    /// §2.11 step 1: copies the active layer (or the flattened document for Copy Merged) inside
    /// the clipping geometry's integer bounds; an empty selection copies the whole layer. Nothing
    /// is copied (the old item stays) when the bounds are smaller than 1×1.
    pub fn copy(&mut self, app: &AppWorkspace, merged: bool) -> Option<&ClipItem> {
        let item = capture(app, merged)?;
        self.item = Some(item);
        self.item.as_ref()
    }

    /// §2.11 step 2: a system-clipboard image that is not ours, with geometry (0, 0, w, h).
    pub fn set_external(&mut self, image: ImageData) {
        let (w, h) = (image.width, image.height);
        self.item = Some(ClipItem {
            geometry: GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, w as f64, h as f64)),
            bounds: IntRect::new(0, 0, w, h),
            bitmap: Arc::new(Surface::from_data(w, h, image.data.clone())),
            masked: Arc::new(image),
        });
    }

    /// `system` (the system clipboard's image) is the masked bitmap of our item, so the item's
    /// geometry and position apply. The test is byte-exact: Linux's PNG round trip is lossless; an
    /// OS reader that changes any byte (a lossy DIB or premultiplied path on macOS or Windows)
    /// makes our own image paste as an external one, at (0, 0) with a rectangle geometry.
    pub fn owned(&self, system: &ImageData) -> bool {
        self.item.as_ref().is_some_and(|i| *i.masked == *system)
    }

    /// §2.11 step 2: the item a paste uses, given the system clipboard's image. Ours keeps the
    /// item (its geometry and place), another app's replaces it, none leaves the in-app item
    /// (paintdotjs's fallback when `navigator.clipboard` is denied).
    pub fn read(&mut self, system: Option<ImageData>) -> Option<&ClipItem> {
        if let Some(image) = system
            && !self.owned(&image)
        {
            self.set_external(image);
        }
        self.item.as_ref()
    }
}

fn capture(app: &AppWorkspace, merged: bool) -> Option<ClipItem> {
    let ws = app.active_ws()?;
    let doc = IntRect::new(0, 0, ws.document.width, ws.document.height);
    let geometry = GraphicsPath {
        polygons: ws
            .selection
            .clipping_polygons(doc)
            .into_iter()
            .map(|p| p.into_iter().map(|[x, y]| Point::new(x, y)).collect())
            .collect(),
    };
    let bounds = IntRect::bound_of(&geometry.get_bounds()).intersect(&doc);
    if bounds.is_empty() {
        return None;
    }
    let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
    let clip = ws.selection.clipping_mask(quality, doc);
    // §7.6 "No effect": the layer (or the composite) holds an open tool's overlay.
    let composite;
    let source = if merged {
        composite = ws.document.composite();
        &composite
    } else {
        &ws.document.layers.get_at(ws.active_layer)?.surface
    };
    let bitmap = source.get_image_data(bounds.x, bounds.y, bounds.w, bounds.h);
    let mut masked = bitmap.clone();
    let bitmap = Surface::from_data(bounds.w, bounds.h, bitmap.data);
    // §2.4 "Copy to clipboard": α′ = round(α·m/255), 0x00000000 where m = 0.
    for (i, p) in masked.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let (x, y) = (i as u32 % bounds.w, i as u32 / bounds.w);
        *p = apply_alpha_mask(*p, clip.get(bounds.x + x as i32, bounds.y + y as i32));
    }
    Some(ClipItem {
        bitmap: Arc::new(bitmap),
        masked: Arc::new(masked),
        geometry,
        bounds,
    })
}

/// §2.11 step 3 "Position": with V the integer inset of `visible` (the rect itself when the inset
/// is empty) and I its integer bounds, per axis a paste left of V moves to I's left, else one past
/// I's right edge right-aligns to it; then x, y ≥ 0; then a paste past the document is shifted
/// back by the overflow, at most by its left/top. A contained paste keeps its position.
pub fn paste_position(visible: &Rectangle, doc: (u32, u32), paste: IntRect) -> (i32, i32) {
    let (l, t) = (visible.left().ceil(), visible.top().ceil());
    let (r, b) = (visible.right().floor(), visible.bottom().floor());
    let v = if r > l && b > t {
        Rectangle::absolute(l, t, r, b)
    } else {
        *visible
    };
    let i = IntRect::bound_of(&v);
    let axis = |pos: i32, len: u32, v_lo: f64, i_lo: i32, i_hi: i32, doc: u32| {
        let mut p = if (pos as f64) < v_lo {
            i_lo
        } else if pos + len as i32 > i_hi {
            i_hi - len as i32
        } else {
            pos
        };
        p = p.max(0);
        let overflow = p as i64 + len as i64 - doc as i64;
        if overflow > 0 {
            p -= overflow.min(p as i64) as i32;
        }
        p
    };
    (
        axis(paste.x, paste.w, v.left(), i.x, i.right(), doc.0),
        axis(paste.y, paste.h, v.top(), i.y, i.bottom(), doc.1),
    )
}

/// §2.11 step 3 "Too large": wider or taller than the document.
pub fn needs_expand(doc: (u32, u32), paste: IntRect) -> bool {
    paste.w > doc.0 || paste.h > doc.1
}

/// The expand question's answer (§2.11 step 3); Cancel never reaches the engine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteExpand {
    Expand,
    Keep,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasteOutcome {
    /// The paste floats in Move Selected Pixels until its commit.
    Floating,
    Failed,
}

/// §2.11 step 3: Expand (when too large) runs Canvas Size to the larger dimensions, anchored
/// top-left and transparent; the paste is placed (`paste_position`) and floats in Move Selected
/// Pixels (`MoveTool::begin_paste`, entry "Paste"); a paste not inside the visible rect is
/// scrolled to the view centre.
pub fn paste(app: &mut AppWorkspace, item: &ClipItem, expand: PasteExpand) -> PasteOutcome {
    // S§4.5: a text layer asks first (the web's image paste event reaches here without
    // `action::perform`, which asks before the UI paste).
    if !crate::document::text_layer::ensure_active_raster(app) {
        return PasteOutcome::Failed;
    }
    let Some(ws) = app.active_ws() else {
        return PasteOutcome::Failed;
    };
    let pb = item.paste_bounds();
    let d = &ws.document;
    let (w, h, dpi) = (d.width, d.height, d.resolution);
    if expand == PasteExpand::Expand
        && needs_expand((w, h), pb)
        && ops::change_canvas_size(
            app,
            w.max(pb.w),
            h.max(pb.h),
            Anchor::TopLeft,
            CanvasFill::Transparent,
            dpi,
        )
        .is_err()
    {
        return PasteOutcome::Failed;
    }
    let Some(ws) = app.active_ws() else {
        return PasteOutcome::Failed;
    };
    let visible = ws.view.get_visible_document_rect();
    let (x, y) = paste_position(&visible, (ws.document.width, ws.document.height), pb);
    let at = (item.bounds.x + x - pb.x, item.bounds.y + y - pb.y);
    let mut origin = Matrix::identity();
    origin.translate(-item.bounds.x as f64, -item.bounds.y as f64);
    let geometry = item.geometry.transformed(&origin);
    let bitmap = item.bitmap.clone();
    app.set_active_tool_from_type(ToolType::Move);
    let floating = app
        .with_tool(|t, app| {
            t.as_any_mut()
                .downcast_mut::<MoveTool>()
                .is_some_and(|m| m.begin_paste(app, bitmap, geometry, at))
        })
        .unwrap_or(false);
    if !floating {
        return PasteOutcome::Failed;
    }
    if let Some(ws) = app.active_ws_mut().filter(|ws| !ws.view.zoom_to_window) {
        let p = Rectangle::new(x as f64, y as f64, pb.w as f64, pb.h as f64);
        let inside = p.left() >= visible.left()
            && p.top() >= visible.top()
            && p.right() <= visible.right()
            && p.bottom() <= visible.bottom();
        if !inside {
            let v = &mut ws.view;
            let (b, css) = (v.get_render_bounds(), v.css_scale());
            let cx = b.x + (p.x + p.width / 2.0) * css;
            let cy = b.y + (p.y + p.height / 2.0) * css;
            v.shift_view_position(cx - v.view_width / 2.0, cy - v.view_height / 2.0);
        }
    }
    PasteOutcome::Floating
}

/// §2.11 step 4, first half: Add New Layer ("Layer {N+1}" above the active layer) through the
/// commit-first path. The UI then asks the expand question; a Cancel or a failed paste undoes the
/// layer (`app.undo()`), leaving "New Layer" on the redo stack as Paint.NET does.
pub fn add_paste_layer(app: &mut AppWorkspace) -> bool {
    app.commit_tool_first(|app| {
        app.active_ws_mut()
            .map(|ws| ws.execute_function(functions::add_new_blank_layer))
            .is_some()
    })
}

/// §2.11 step 4: Add New Layer, then Paste; a failed paste undoes the layer.
pub fn paste_into_new_layer(
    app: &mut AppWorkspace,
    item: &ClipItem,
    expand: PasteExpand,
) -> PasteOutcome {
    if !add_paste_layer(app) {
        return PasteOutcome::Failed;
    }
    let out = paste(app, item, expand);
    if out == PasteOutcome::Failed {
        app.undo();
    }
    out
}

/// §2.11 step 5: a new document of the paste bounds' size holding one transparent "Layer 1", the
/// paste committed into it, the selection reset, the history cleared to one entry named by the
/// menu text, and the previous tool back.
pub fn paste_into_new_image(app: &mut AppWorkspace, item: &ClipItem) -> PasteOutcome {
    let pb = item.paste_bounds();
    if ops::check_sides(pb.w, pb.h).is_err() || ops::check_memory(1, pb.w, pb.h, 0).is_err() {
        return PasteOutcome::Failed;
    }
    let previous = app.tool_type;
    let i = app.create_blank_document_in_new_workspace(pb.w, pb.h, 96.0, false);
    let ws = &mut app.documents[i];
    let name = tf("addNewBlankLayer.layerName.format", &["1"]);
    ws.document
        .layers
        .set_layer_at(0, Layer::create_layer(pb.w, pb.h, &name));
    ws.invalidate(None);
    let out = paste(app, item, PasteExpand::Keep);
    // The tool change commits the paste.
    match previous {
        Some(tool) => app.set_active_tool_from_type(tool),
        None => app.commit_tool_first(|_| ()),
    }
    app.edit_selection(|ws| ws.selection.reset());
    if let Some(ws) = app.active_ws_mut() {
        ws.history.clear_all();
        ws.history.push_new_memento(HistoryMemento::null(
            &t("menu.edit.pasteInToNewImage.text"),
            "assets/icons/menu_edit_paste_in_to_new_image_icon.png",
        ));
        ws.set_dirty(true);
        ws.view.events.push(AppEvent::HistoryChanged);
    }
    out
}

/// §2.11 step 6: `geometry` becomes the continuation in `mode`, then a commit: one selection
/// entry named
/// "Paste Selection: …" by mode.
pub fn paste_selection(app: &mut AppWorkspace, geometry: GraphicsPath, mode: CombineMode) {
    let name = t(&format!(
        "pasteSelectionFromClipboardAction.historyMemento.{}.text",
        mode.value()
    ));
    app.edit_selection(|ws| {
        ws.execute_function(|target| {
            let before = MementoKind::Selection(target.selection.save());
            target.selection.set_continuation_path(geometry, mode);
            target.selection.commit_continuation();
            Some(HistoryMemento::new(
                &name,
                "assets/icons/menu_edit_paste_selection_replace_icon.png",
                before,
            ))
        })
    });
}

/// .NET's `ToString("G17", InvariantCulture)`: 17 significant digits, trailing zeros dropped,
/// fixed notation for decimal exponents −5 < e < 17, else `d.dddE±dd`.
pub fn g17(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    if v == 0.0 {
        return if v.is_sign_negative() { "-0" } else { "0" }.into();
    }
    let s = format!("{:.16e}", v.abs());
    let (mantissa, exp) = s.split_once('e').expect("exponent form");
    let e: i32 = exp.parse().expect("exponent");
    let digits = mantissa.replace('.', "");
    let digits = digits.trim_end_matches('0');
    let sign = if v < 0.0 { "-" } else { "" };
    let body = if (-4..17).contains(&e) {
        if e >= 0 {
            let int_len = e as usize + 1;
            let int = format!("{digits:0<int_len$}");
            let (int, frac) = int.split_at(int_len);
            if frac.is_empty() {
                int.to_string()
            } else {
                format!("{int}.{frac}")
            }
        } else {
            format!("0.{}{digits}", "0".repeat((-e - 1) as usize))
        }
    } else {
        let (first, rest) = digits.split_at(1);
        let rest = if rest.is_empty() {
            String::new()
        } else {
            format!(".{rest}")
        };
        format!(
            "{first}{rest}E{}{:02}",
            if e < 0 { '-' } else { '+' },
            e.abs()
        )
    };
    format!("{sign}{body}")
}

/// §2.11 step 6, the selection's clipboard text: `{"polygonList":["x,y,x,y,…",…]}` with G17
/// numbers.
pub fn selection_to_json(path: &GraphicsPath) -> String {
    let list: Vec<String> = path
        .polygons
        .iter()
        .map(|p| {
            p.iter()
                .flat_map(|v| [g17(v.x), g17(v.y)])
                .collect::<Vec<_>>()
                .join(",")
        })
        .collect();
    serde_json::json!({ "polygonList": list }).to_string()
}

/// The inverse of `selection_to_json`; `None` for anything but a list of finite x,y pairs.
pub fn selection_from_json(text: &str) -> Option<GraphicsPath> {
    let v: serde_json::Value = serde_json::from_str(text).ok()?;
    let polygons = v
        .get("polygonList")?
        .as_array()?
        .iter()
        .map(|p| {
            let n: Vec<f64> = p
                .as_str()?
                .split(',')
                .map(|s| s.trim().parse::<f64>().ok().filter(|n| n.is_finite()))
                .collect::<Option<_>>()?;
            n.len()
                .is_multiple_of(2)
                .then(|| n.chunks(2).map(|c| Point::new(c[0], c[1])).collect())
        })
        .collect::<Option<_>>()?;
    Some(GraphicsPath { polygons })
}
