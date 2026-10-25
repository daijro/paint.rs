//! Image-level document operations: Resize, Canvas Size, Crop to Selection, the Image and Layers
//! flips and rotations, Flatten.
//! Port of: paintdotjs js/src/io/DocumentIO.js `resizeImage`, `changeCanvasSize`,
//! `getCanvasResizeOffset`, `getCanvasFillStyle`, `cropToSelection`, `transformDocument`,
//! `transformActiveLayer`, `drawTransformed`, `flattenDocument`, `replaceDocument`,
//! (flow, history names and icons); `finishActiveTool` is the app's commit-first hook
//! (`AppWorkspace::commit_tool_first`, painting-tools-2.md §7.6). Reference: paintdotjs @ b4e6fc5
//! Every pixel follows Paint.NET, the crop mask and the flatten composite too.

use crate::app::{AppEvent, AppWorkspace};
use crate::canvas::ImageData;
use crate::document::Document;
use crate::document::layer::{Layer, LayerProperties};
use crate::document::surface::Surface;
use crate::history::{BitmapMemento, DocumentStateMemento, HistoryMemento, MementoKind, functions};
use crate::i18n;
use crate::pixel::IntRect;
use crate::pixel::mask::SelectionQuality;
use crate::pixel::ops::apply_alpha_mask;
use crate::resample::{self, Gamma, ResizeMode};
use crate::util::geom::Rectangle;

/// §2.2 step 1: the Resize / Canvas Size dialogs accept 1 ≤ w, h ≤ 262144.
pub const MAX_SIDE: u32 = 262144;

const RESIZE_ICON: &str = "assets/icons/menu_image_resize_icon.png";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpError {
    NoDocument,
    /// A side is 0 or above `MAX_SIDE`, or the buffers do not fit the address space.
    TooLarge,
    /// The memory for the new document could not be reserved.
    OutOfMemory,
}

/// Canvas Size anchors, in paintdotjs's `ImageSizeDialog.js` order (row-major 3×3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Middle,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    const IDS: [(Anchor, &'static str); 9] = [
        (Anchor::TopLeft, "topLeft"),
        (Anchor::Top, "top"),
        (Anchor::TopRight, "topRight"),
        (Anchor::Left, "left"),
        (Anchor::Middle, "middle"),
        (Anchor::Right, "right"),
        (Anchor::BottomLeft, "bottomLeft"),
        (Anchor::Bottom, "bottom"),
        (Anchor::BottomRight, "bottomRight"),
    ];

    pub fn id(self) -> &'static str {
        Self::IDS[self as usize].1
    }

    pub fn from_id(id: &str) -> Option<Anchor> {
        Self::IDS.iter().find(|a| a.1 == id).map(|a| a.0)
    }
}

/// Canvas Size fill choices (§2.3 step 1), with paintdotjs's ids.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasFill {
    Transparent,
    Primary,
    Secondary,
    White,
    Black,
}

impl CanvasFill {
    pub fn from_id(id: &str) -> Option<CanvasFill> {
        Some(match id {
            "transparent" => CanvasFill::Transparent,
            "primary" => CanvasFill::Primary,
            "secondary" => CanvasFill::Secondary,
            "white" => CanvasFill::White,
            "black" => CanvasFill::Black,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transform {
    FlipH,
    FlipV,
    Rot90CW,
    Rot90CCW,
    Rot180,
}

impl Transform {
    /// The paintdotjs action key (`menu.image.<key>`, `menu.layers.<key>`) and its icon stem
    /// (the key with each capital turned into `_` + lower case, as `transformDocument` builds it).
    fn ids(self) -> (&'static str, &'static str) {
        match self {
            Transform::FlipH => ("flipHorizontal", "flip_horizontal"),
            Transform::FlipV => ("flipVertical", "flip_vertical"),
            Transform::Rot90CW => ("rotate90CW", "rotate90_c_w"),
            Transform::Rot90CCW => ("rotate90CCW", "rotate90_c_c_w"),
            Transform::Rot180 => ("rotate180", "rotate180"),
        }
    }

    /// The transform of a paintdotjs action key (`flipHorizontal` …).
    pub fn from_key(key: &str) -> Option<Transform> {
        use Transform::*;
        [FlipH, FlipV, Rot90CW, Rot90CCW, Rot180]
            .into_iter()
            .find(|t| t.ids().0 == key)
    }

    fn size(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            Transform::Rot90CW | Transform::Rot90CCW => (h, w),
            _ => (w, h),
        }
    }
}

/// §2.3 step 2: Left/Top 0, Center/Middle Δ/2, Right/Bottom Δ, with truncating division.
pub fn get_canvas_resize_offset(
    old_w: u32,
    old_h: u32,
    new_w: u32,
    new_h: u32,
    anchor: Anchor,
) -> (i32, i32) {
    let pick = |d: i32, i: usize| [0, d / 2, d][i];
    let i = anchor as usize;
    (
        pick(new_w as i32 - old_w as i32, i % 3),
        pick(new_h as i32 - old_h as i32, i / 3),
    )
}

/// §2.4 step 2: ⌊x + ε⌋, ⌊y + ε⌋, ⌈x + w − ε⌉, ⌈y + h − ε⌉ with ε = 1e‑5, ∩ the document.
pub fn crop_bounds(r: &Rectangle, w: u32, h: u32) -> IntRect {
    const EPS: f64 = 1e-5;
    let bounds = Rectangle::absolute(
        (r.x + EPS).floor(),
        (r.y + EPS).floor(),
        (r.x + r.width - EPS).ceil(),
        (r.y + r.height - EPS).ceil(),
    );
    IntRect::bound_of(&bounds).intersect(&IntRect::new(0, 0, w, h))
}

/// §2.5: output pixel (x, y) reads FlipH (W−1−x, y), FlipV (x, H−1−y), Rot90CW (y, H−1−x),
/// Rot90CCW (W−1−y, x), Rot180 (W−1−x, H−1−y). `out` holds the transformed size.
fn transform_into(src: &[u8], w: u32, h: u32, t: Transform, out: &mut [u8]) {
    let (w, h) = (w as usize, h as usize);
    let src = src.as_chunks::<4>().0;
    let (nw, _) = t.size(w as u32, h as u32);
    for (y, row) in out
        .as_chunks_mut::<4>()
        .0
        .chunks_mut(nw as usize)
        .enumerate()
    {
        for (x, o) in row.iter_mut().enumerate() {
            let (sx, sy) = match t {
                Transform::FlipH => (w - 1 - x, y),
                Transform::FlipV => (x, h - 1 - y),
                Transform::Rot90CW => (y, h - 1 - x),
                Transform::Rot90CCW => (w - 1 - y, x),
                Transform::Rot180 => (w - 1 - x, h - 1 - y),
            };
            *o = src[sy * w + sx];
        }
    }
}

/// `transform_into` on an `ImageData`.
pub fn transform_pixels(img: &ImageData, t: Transform) -> ImageData {
    let (width, height) = t.size(img.width, img.height);
    let mut data = vec![0; img.data.len()];
    transform_into(&img.data, img.width, img.height, t, &mut data);
    ImageData {
        width,
        height,
        data,
    }
}

pub(crate) fn check_sides(w: u32, h: u32) -> Result<(), OpError> {
    if w == 0 || h == 0 || w > MAX_SIDE || h > MAX_SIDE {
        return Err(OpError::TooLarge);
    }
    Ok(())
}

/// `n` layers of w·h·4 bytes plus `extra`, or `TooLarge` when that overflows usize.
fn doc_bytes(n: usize, w: u32, h: u32, extra: usize) -> Result<usize, OpError> {
    (w as usize)
        .checked_mul(h as usize)
        .and_then(|p| p.checked_mul(4))
        .and_then(|b| b.checked_mul(n))
        .and_then(|b| b.checked_add(extra))
        .ok_or(OpError::TooLarge)
}

/// `bytes` must be grantable: `OutOfMemory` instead of an abort later.
fn reserve(bytes: usize) -> Result<Vec<u8>, OpError> {
    let mut v = Vec::new();
    v.try_reserve_exact(bytes)
        .map_err(|_| OpError::OutOfMemory)?;
    Ok(v)
}

/// R57: the new document's `layers` plus the workspace's composition and scratch surfaces (which
/// `sync_after_change` reallocates infallibly at the new size) plus `extra` must be grantable
/// before any work starts; the reservation is released at once.
pub fn check_memory(layers: usize, w: u32, h: u32, extra: usize) -> Result<(), OpError> {
    reserve(doc_bytes(layers + 2, w, h, extra)?).map(drop)
}

/// A transparent w×h surface whose memory is reserved fallibly (§2.2 step 1 limits).
pub fn try_surface(w: u32, h: u32) -> Result<Surface, OpError> {
    check_sides(w, h)?;
    let n = doc_bytes(1, w, h, 0)?;
    let mut data = reserve(n)?;
    data.resize(n, 0);
    Ok(Surface::from_data(w, h, data))
}

/// Copies `src` into `dst` with its top-left at (dx, dy), clipped; no blending.
fn copy_at(dst: &mut Surface, src: &Surface, dx: i32, dy: i32) {
    let r = IntRect::new(dx, dy, src.width(), src.height()).intersect(&dst.int_bounds());
    for y in r.y..r.bottom() {
        let s = src.span(y - dy, IntRect { x: r.x - dx, ..r });
        let d = dst.span(y, r);
        dst.data_mut()[d].copy_from_slice(&src.data()[s]);
    }
}

/// paintdotjs `replaceDocument`: installs `doc` with `active` (clamped) and an empty selection;
/// the history entry is a `DocumentState` memento of the old document, active layer, selection
/// and view. Then the view fits the new size.
pub fn replace_document(
    app: &mut AppWorkspace,
    doc: Document,
    active: usize,
    name: &str,
    icon: &str,
) {
    let Some(ws) = app.active_ws_mut() else {
        return;
    };
    let (w, h) = (doc.width, doc.height);
    let v = &ws.view;
    let next = HistoryMemento::new(
        name,
        icon,
        MementoKind::DocumentState(Box::new(DocumentStateMemento {
            active_layer: active.min(doc.layers.size().saturating_sub(1)),
            document: doc,
            selection: Default::default(),
            zoom: v.zoom,
            zoom_to_window: v.zoom_to_window,
            viewport_x: v.viewport_x,
            viewport_y: v.viewport_y,
        })),
    );
    // Applying a memento of the new state returns its inverse: the old state, which is the entry.
    ws.execute_function(|t| Some(next.perform_undo(t)));
    ws.view.fit_viewport();
    ws.view.events.push(AppEvent::LayersChanged);
    ws.view.events.push(AppEvent::UpdateSize { w, h });
}

/// Image ▸ Resize (§2.2 steps 2–3): every layer through Task 17d's scaler, straight RGBA8 in and
/// out, layer properties copied, selection reset. A DPI-only change records only the resolution
/// (§2.2 step 2, R56: Paint.NET records only the metadata; paintdotjs also records an undoable
/// "Resize..." entry), leaving pixels, selection, view and tool alone.
pub fn resize_image(
    app: &mut AppWorkspace,
    w: u32,
    h: u32,
    mode: ResizeMode,
    gamma: Gamma,
    dpi: f64,
) -> Result<(), OpError> {
    let ws = app.active_ws().ok_or(OpError::NoDocument)?;
    check_sides(w, h)?;
    let d = &ws.document;
    if (w, h) == (d.width, d.height) {
        let (old, new) = (d.resolution, Document::clamp_resolution(dpi));
        if new != old {
            let ws = app.active_ws_mut().ok_or(OpError::NoDocument)?;
            let name = i18n::t("menu.image.resize.text");
            ws.execute_function(|t| {
                t.document.resolution = new;
                let kind = MementoKind::Resolution(old);
                Some(HistoryMemento::new(&name, RESIZE_ICON, kind))
            });
        }
        return Ok(());
    }
    // The scalers allocate infallibly: the new layers and the scaler's scratch must fit first.
    let scratch = resample::scratch_bytes(mode, d.height, w).ok_or(OpError::TooLarge)?;
    check_memory(d.layers.size(), w, h, scratch)?;
    app.commit_tool_first(|app| {
        let ws = app.active_ws().ok_or(OpError::NoDocument)?;
        let d = &ws.document;
        let mut doc = Document::new(w, h, dpi);
        for l in d.layers.iter() {
            let data = resample::resize(l.surface.data(), d.width, d.height, w, h, mode, gamma);
            if data.is_empty() {
                return Err(OpError::TooLarge);
            }
            // S§4.5: the image-wide commands rasterize a text layer silently, inside their entry.
            doc.layers.add_layer(Layer {
                id: l.id,
                properties: l.properties.clone(),
                surface: Surface::from_data(w, h, data),
                text: None,
            });
        }
        let active = ws.active_layer;
        let name = i18n::t("menu.image.resize.text");
        replace_document(app, doc, active, &name, RESIZE_ICON);
        Ok(())
    })
}

/// Image ▸ Canvas Size (§2.3): each layer is filled with `fill`, then the old pixels are copied
/// (not blended) at the anchor offset, so old transparent pixels stay transparent.
pub fn change_canvas_size(
    app: &mut AppWorkspace,
    w: u32,
    h: u32,
    anchor: Anchor,
    fill: CanvasFill,
    dpi: f64,
) -> Result<(), OpError> {
    let ws = app.active_ws().ok_or(OpError::NoDocument)?;
    check_sides(w, h)?;
    let d = &ws.document;
    if (w, h, dpi) == (d.width, d.height, d.resolution) {
        return Ok(());
    }
    let rgba = |c: crate::util::color::Color| Some([c.red, c.green, c.blue, c.alpha]);
    let px = match fill {
        CanvasFill::Transparent => None,
        CanvasFill::Primary => rgba(app.primary_color),
        CanvasFill::Secondary => rgba(app.secondary_color),
        CanvasFill::White => Some([255; 4]),
        CanvasFill::Black => Some([0, 0, 0, 255]),
    };
    let (dx, dy) = get_canvas_resize_offset(d.width, d.height, w, h, anchor);
    check_memory(d.layers.size(), w, h, 0)?;
    app.commit_tool_first(|app| {
        let ws = app.active_ws().ok_or(OpError::NoDocument)?;
        let mut doc = Document::new(w, h, dpi);
        for l in ws.document.layers.iter() {
            let mut s = try_surface(w, h)?;
            if let Some(p) = px {
                s.fill_rect(s.int_bounds(), p);
            }
            copy_at(&mut s, &l.surface, dx, dy);
            doc.layers.add_layer(Layer {
                id: l.id,
                properties: l.properties.clone(),
                surface: s,
                text: None,
            });
        }
        let active = ws.active_layer;
        let name = i18n::t("menu.image.canvasSize.text");
        replace_document(
            app,
            doc,
            active,
            &name,
            "assets/icons/menu_image_canvas_size_icon.png",
        );
        Ok(())
    })
}

/// Image ▸ Crop to Selection (§2.4): each layer windowed to `crop_bounds`, every pixel through
/// `apply_alpha_mask` with the selection's clip mask at the shared rendering quality. False (no
/// change) for an empty selection, an empty bounds area or a failed allocation.
pub fn crop_to_selection(app: &mut AppWorkspace) -> bool {
    if app.active_ws().is_none_or(|ws| ws.selection.is_empty()) {
        return false;
    }
    let quality = SelectionQuality::from_value(&app.tool_settings.rendering_quality);
    app.commit_tool_first(|app| {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        let d = &ws.document;
        let b = crop_bounds(&ws.selection.get_bounds(), d.width, d.height);
        if b.is_empty() || check_memory(d.layers.size(), b.w, b.h, 0).is_err() {
            return false;
        }
        let v = &ws.view;
        let (zoom, fit, before) = (v.zoom, v.zoom_to_window, v.get_render_bounds());
        let mask = crate::effect::selection_mask(&ws.selection, quality, d.width, d.height);
        let mut doc = Document::new(b.w, b.h, d.resolution);
        for l in d.layers.iter() {
            let Ok(mut s) = try_surface(b.w, b.h) else {
                return false;
            };
            copy_at(&mut s, &l.surface, -b.x, -b.y);
            if let Some(m) = &mask {
                for (i, p) in s.data_mut().as_chunks_mut::<4>().0.iter_mut().enumerate() {
                    let (x, y) = ((i % b.w as usize) as i32, (i / b.w as usize) as i32);
                    *p = apply_alpha_mask(*p, m.get(b.x + x, b.y + y));
                }
            }
            doc.layers.add_layer(Layer {
                id: l.id,
                properties: l.properties.clone(),
                surface: s,
                text: None,
            });
        }
        let active = ws.active_layer;
        let name = i18n::t("menu.image.crop.text");
        replace_document(
            app,
            doc,
            active,
            &name,
            "assets/icons/menu_image_crop_icon.png",
        );
        // §2.4 step 6: the zoom stays and the canvas offset (the document point at the viewport
        // origin) moves by −(b.x, b.y), so the kept pixels stay put on screen. Fit to Window
        // keeps the fit `replace_document` made.
        if let Some(ws) = app.active_ws_mut()
            && !fit
        {
            let v = &mut ws.view;
            v.zoom = zoom;
            let css = v.css_scale();
            let x = v.view_width - v.get_render_width().min(v.view_width) / 2.0;
            let y = v.view_height - v.get_render_height().min(v.view_height) / 2.0;
            v.set_view_position(
                x - (before.x + b.x as f64 * css),
                y - (before.y + b.y as f64 * css),
            );
        }
        true
    })
}

/// Image ▸ Flip / Rotate (§2.5): every layer permuted, properties kept; `replace_document` drops
/// the selection (Paint.NET deselects first).
pub fn transform_document(app: &mut AppWorkspace, t: Transform) -> Result<(), OpError> {
    app.active_ws().ok_or(OpError::NoDocument)?;
    app.commit_tool_first(|app| {
        let ws = app.active_ws().ok_or(OpError::NoDocument)?;
        let d = &ws.document;
        let (w, h) = t.size(d.width, d.height);
        check_memory(d.layers.size(), w, h, 0)?;
        let mut doc = Document::new(w, h, d.resolution);
        for l in d.layers.iter() {
            let mut s = try_surface(w, h)?;
            transform_into(l.surface.data(), d.width, d.height, t, s.data_mut());
            doc.layers.add_layer(Layer {
                id: l.id,
                properties: l.properties.clone(),
                surface: s,
                text: None,
            });
        }
        let (active, (key, icon)) = (ws.active_layer, t.ids());
        let name = i18n::t(&format!("menu.image.{key}.text"));
        replace_document(
            app,
            doc,
            active,
            &name,
            &format!("assets/icons/menu_image_{icon}_icon.png"),
        );
        Ok(())
    })
}

/// Layers ▸ Flip Horizontal / Flip Vertical / Rotate 180 (§2.5): Paint.NET deselects first
/// (paintdotjs keeps the selection), so the entry is Compound(deselect, whole-layer bitmap).
/// The 90° turns change the size and are not layer operations.
pub fn transform_active_layer(app: &mut AppWorkspace, t: Transform) {
    if matches!(t, Transform::Rot90CW | Transform::Rot90CCW) || app.active_ws().is_none() {
        return;
    }
    // S§4.5: a layer flip edits the active layer's pixels.
    if !crate::document::text_layer::ensure_active_raster(app) {
        return;
    }
    let (key, icon) = t.ids();
    let name = i18n::t(&format!("menu.layers.{key}.text"));
    let icon = format!("assets/icons/menu_layers_{icon}_icon.png");
    app.commit_tool_first(|app| {
        let Some(ws) = app.active_ws_mut() else {
            return;
        };
        ws.execute_function(|target| {
            let index = *target.active_layer;
            let before = BitmapMemento::capture(target.document, index, None);
            let mut parts: Vec<_> = functions::deselect(target).into_iter().collect();
            let d = &mut *target.document;
            let s = &mut d.layers.get_at_mut(index)?.surface;
            let (w, h) = (s.width(), s.height());
            transform_into(&before.chunks.first()?.1.data, w, h, t, s.data_mut());
            d.invalidate(None);
            parts.push(HistoryMemento::new(
                &name,
                &icon,
                MementoKind::Bitmap(before),
            ));
            Some(HistoryMemento::new(
                &name,
                &icon,
                MementoKind::Compound(parts),
            ))
        });
    });
}

/// Image ▸ Flatten (§2.6; compositing.md §2.5): one background layer "Background" holding
/// `Document::composite`, the selection kept. False with a single layer.
pub fn flatten_document(app: &mut AppWorkspace) -> bool {
    if app
        .active_ws()
        .is_none_or(|ws| ws.document.layers.size() < 2)
    {
        return false;
    }
    app.commit_tool_first(|app| {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        let d = &ws.document;
        let mut doc = Document::new(d.width, d.height, d.resolution);
        let name = i18n::t("layer.backgroundLayer.defaultName");
        doc.layers.add_layer(Layer {
            id: Layer::fresh_id(),
            properties: LayerProperties::new(&name, true, true, 255),
            surface: d.composite(),
            text: None,
        });
        let selection = ws.selection.save();
        let name = i18n::t("menu.image.flatten.text");
        replace_document(
            app,
            doc,
            0,
            &name,
            "assets/icons/menu_image_flatten_icon.png",
        );
        if let Some(ws) = app.active_ws_mut() {
            ws.selection.restore(selection);
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::testing::{fresh_doc, layer_bytes};

    /// Each layer's id (bottom first): a live editing session merges layers by it.
    fn layer_ids(app: &AppWorkspace) -> Vec<u64> {
        let ws = app.active_ws().unwrap();
        ws.document.layers.iter().map(|l| l.id).collect()
    }

    /// A live session raises the id counter past the ids it takes in, but never from an id near the end of the range:
    /// the counter must not wrap onto ids in use.
    #[test]
    fn an_id_near_the_end_of_the_range_never_raises_the_layer_id_counter() {
        let before = Layer::fresh_id();
        Layer::ensure_ids_above(u64::MAX - 1);
        Layer::ensure_ids_above(u64::MAX / 2);
        let after = Layer::fresh_id();
        assert!(
            after > before && after < u64::MAX / 2,
            "{before} then {after}"
        );
        Layer::ensure_ids_above(after + 10);
        assert!(Layer::fresh_id() > after + 10);
    }

    /// Image-wide operations rebuild every layer and keep its id (so a live session merges their pixels instead of
    /// deleting and adding layers); Duplicate Layer gives the copy an id of its own.
    #[test]
    fn image_wide_operations_keep_layer_ids_and_a_duplicate_gets_its_own() {
        let mut app = fresh_doc(40, 30, Some(7));
        app.active_ws_mut()
            .unwrap()
            .add_layer(Layer::create_layer(40, 30, "Top"));
        let ids = layer_ids(&app);
        resize_image(
            &mut app,
            20,
            15,
            crate::resample::ResizeMode::Bilinear,
            crate::resample::Gamma::Linear,
            96.0,
        )
        .unwrap();
        assert_eq!(layer_ids(&app), ids, "Resize");
        change_canvas_size(
            &mut app,
            30,
            25,
            Anchor::Middle,
            CanvasFill::Transparent,
            96.0,
        )
        .unwrap();
        assert_eq!(layer_ids(&app), ids, "Canvas Size");
        transform_document(&mut app, Transform::Rot90CW).unwrap();
        assert_eq!(layer_ids(&app), ids, "Rotate");
        crate::testing::drive_tool(
            &mut app,
            crate::tool::ToolType::RectangleSelect,
            &[
                crate::testing::Ev::Down(2.0, 2.0, crate::tool::MouseButton::Left),
                crate::testing::Ev::Move(12.0, 10.0),
                // A selection made faster than this is a click (§2.5).
                crate::testing::Ev::Wait(60.0),
                crate::testing::Ev::Up(12.0, 10.0, crate::tool::MouseButton::Left),
            ],
        );
        assert!(crop_to_selection(&mut app));
        assert_eq!(layer_ids(&app), ids, "Crop to Selection");
        crate::action::perform(&mut app, "menu.layers.duplicate.layer");
        let after = layer_ids(&app);
        assert_eq!(after.len(), 3);
        assert!(
            after
                .iter()
                .all(|id| after.iter().filter(|x| *x == id).count() == 1),
            "{after:?}"
        );
    }

    fn grid(w: u32, h: u32) -> ImageData {
        ImageData {
            width: w,
            height: h,
            data: (0..w * h).flat_map(|i| [i as u8 * 10, 0, 0, 255]).collect(),
        }
    }
    fn ids(img: &ImageData) -> Vec<u8> {
        img.data.chunks(4).map(|p| p[0] / 10).collect()
    }

    /// §2.5: exact pixel permutations. 3×2 = [0 1 2 / 3 4 5].
    #[test]
    fn transforms_permute_pixels() {
        let g = grid(3, 2);
        assert_eq!(
            ids(&transform_pixels(&g, Transform::FlipH)),
            [2, 1, 0, 5, 4, 3]
        );
        assert_eq!(
            ids(&transform_pixels(&g, Transform::FlipV)),
            [3, 4, 5, 0, 1, 2]
        );
        let cw = transform_pixels(&g, Transform::Rot90CW);
        assert_eq!(
            ((cw.width, cw.height), ids(&cw)),
            ((2, 3), vec![3, 0, 4, 1, 5, 2])
        );
        assert_eq!(
            ids(&transform_pixels(&g, Transform::Rot90CCW)),
            [2, 5, 1, 4, 0, 3]
        );
        assert_eq!(
            ids(&transform_pixels(&g, Transform::Rot180)),
            [5, 4, 3, 2, 1, 0]
        );
    }

    /// §5 Task 43 vectors: truncating division (−13/2 = −6, not −7).
    #[test]
    fn canvas_offsets_truncate() {
        use Anchor::*;
        let o = |a| get_canvas_resize_offset(23, 17, 40, 9, a);
        assert_eq!(
            [
                TopLeft,
                Top,
                TopRight,
                Left,
                Middle,
                Right,
                BottomLeft,
                Bottom,
                BottomRight
            ]
            .map(o),
            [
                (0, 0),
                (8, 0),
                (17, 0),
                (0, -4),
                (8, -4),
                (17, -4),
                (0, -8),
                (8, -8),
                (17, -8)
            ]
        );
        assert_eq!(get_canvas_resize_offset(23, 17, 10, 10, Middle), (-6, -3));
        assert_eq!(Anchor::from_id("bottomRight"), Some(BottomRight));
    }

    /// §2.3: fill every layer, then copy (old transparent pixels stay transparent).
    #[test]
    fn canvas_size_copies_over_the_fill() {
        let mut app = fresh_doc(2, 2, None);
        app.active_ws_mut()
            .unwrap()
            .document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .clear(None);
        change_canvas_size(&mut app, 3, 2, Anchor::TopLeft, CanvasFill::White, 96.0).unwrap();
        let px = layer_bytes(&app, Some(0));
        assert_eq!(&px[0..4], [0, 0, 0, 0]);
        assert_eq!(&px[8..12], [255, 255, 255, 255]);
        assert!(app.undo());
        assert_eq!(app.active_ws().unwrap().document.width, 2);
    }

    /// §2.4: ε bounds; A·m rounded; m = 0 → transparent black.
    #[test]
    fn crop_bounds_and_mask() {
        let r = crate::util::geom::Rectangle {
            x: 2.000001,
            y: 2.000001,
            width: 13.000003,
            height: 5.0,
        };
        assert_eq!(crop_bounds(&r, 40, 40), IntRect::new(2, 2, 13, 5));
        let outside = Rectangle::new(-2.0, -2.0, 4.0, 4.0);
        assert_eq!(crop_bounds(&outside, 6, 5), IntRect::new(0, 0, 2, 2));
        use crate::selection::CombineMode;
        use crate::util::geom::Point;
        let mut app = fresh_doc(23, 17, Some(4));
        let src = layer_bytes(&app, Some(0));
        {
            let ws = app.active_ws_mut().unwrap();
            ws.selection.set_continuation_points(
                &[
                    Point::new(2.0, 2.0),
                    Point::new(15.0, 3.0),
                    Point::new(9.0, 14.0),
                ],
                CombineMode::Replace,
            );
            ws.selection.commit_continuation();
        }
        let ws = app.active_ws().unwrap();
        let mask =
            crate::effect::selection_mask(&ws.selection, SelectionQuality::HighQuality, 23, 17)
                .unwrap();
        let b = crop_bounds(&ws.selection.get_bounds(), 23, 17);
        assert!(crop_to_selection(&mut app));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (b.w, b.h));
        let out = layer_bytes(&app, Some(0));
        for y in 0..b.h as i32 {
            for x in 0..b.w as i32 {
                let (sx, sy) = (x + b.x, y + b.y);
                let i = ((sy * 23 + sx) * 4) as usize;
                let want = apply_alpha_mask(
                    [src[i], src[i + 1], src[i + 2], src[i + 3]],
                    mask.get(sx, sy),
                );
                let j = ((y * b.w as i32 + x) * 4) as usize;
                assert_eq!(&out[j..j + 4], want, "({x},{y})");
            }
        }
    }

    /// §2.2 via Task 17d; the history entry undoes; dialog limits (ui-drawn-graphics.md §2.11).
    #[test]
    fn resize_every_layer() {
        let mut app = fresh_doc(23, 17, Some(4));
        let src = layer_bytes(&app, Some(0));
        resize_image(
            &mut app,
            11,
            17,
            ResizeMode::NearestNeighbor,
            Gamma::Linear,
            96.0,
        )
        .unwrap();
        let out = layer_bytes(&app, Some(0));
        for x in 0..11usize {
            assert_eq!(
                &out[x * 4..x * 4 + 4],
                &src[(2 * x + 1) * 4..(2 * x + 1) * 4 + 4],
                "column {x}"
            );
        }
        assert!(matches!(
            resize_image(
                &mut app,
                MAX_SIDE + 1,
                1,
                ResizeMode::Bicubic,
                Gamma::Linear,
                96.0
            ),
            Err(OpError::TooLarge)
        ));
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, Some(0)), src);
    }

    /// §2.5–2.6: layer flips deselect (undo restores the selection); flatten keeps it.
    #[test]
    fn selection_rules() {
        use crate::selection::CombineMode;
        use crate::util::geom::Point;
        let select = |app: &mut crate::app::AppWorkspace| {
            let ws = app.active_ws_mut().unwrap();
            ws.selection.set_continuation_points(
                &[
                    Point::new(1.0, 1.0),
                    Point::new(4.0, 1.0),
                    Point::new(4.0, 3.0),
                ],
                CombineMode::Replace,
            );
            ws.selection.commit_continuation();
        };
        let mut app = fresh_doc(6, 5, Some(4));
        select(&mut app);
        transform_active_layer(&mut app, Transform::FlipH);
        assert!(app.active_ws().unwrap().selection.is_empty());
        assert!(app.undo());
        assert!(!app.active_ws().unwrap().selection.is_empty());
        crate::action::perform(&mut app, "menu.layers.add.new.layer");
        assert!(flatten_document(&mut app));
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.document.layers.size(), 1);
        assert!(!ws.selection.is_empty());
        let l = ws.document.layers.get_at(0).unwrap();
        assert_eq!(
            (l.properties.name.as_str(), l.properties.is_background),
            ("Background", true)
        );
    }

    fn doc(app: &crate::app::AppWorkspace) -> &Document {
        &app.active_ws().unwrap().document
    }

    /// §2.3 step 2–3: a shrink clips the copy (BottomRight on 3×3 → 2×2 keeps [4 5 / 7 8]); a
    /// Middle grow fills the margin with the primary colour, alpha included.
    #[test]
    fn canvas_size_anchors_and_clips() {
        let mut app = fresh_doc(3, 3, None);
        let g = grid(3, 3);
        let ws = app.active_ws_mut().unwrap();
        ws.document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .put_image_data(&g, 0, 0);
        change_canvas_size(&mut app, 2, 2, Anchor::BottomRight, CanvasFill::Black, 72.0).unwrap();
        let img = ImageData {
            width: 2,
            height: 2,
            data: layer_bytes(&app, Some(0)),
        };
        assert_eq!((ids(&img), doc(&app).resolution), (vec![4, 5, 7, 8], 72.0));
        app.primary_color = crate::util::color::Color::new(1, 2, 3, 4);
        change_canvas_size(&mut app, 4, 3, Anchor::Middle, CanvasFill::Primary, 72.0).unwrap();
        let px = layer_bytes(&app, Some(0));
        // Offset (1, 0): column 0 and column 3 are fill, the old 2×2 sits at (1..3, 0..2).
        assert_eq!(
            (&px[0..4], &px[4..8], &px[12..16]),
            (&[1, 2, 3, 4][..], &g.data[16..20], &[1, 2, 3, 4][..])
        );
        assert_eq!(&px[2 * 16 + 4..2 * 16 + 8], [1, 2, 3, 4]);
    }

    /// §2.5: every layer is permuted, properties kept, the selection dropped; undo restores all.
    #[test]
    fn transform_document_rotates_every_layer() {
        use crate::selection::CombineMode;
        let mut app = fresh_doc(3, 2, None);
        let ws = app.active_ws_mut().unwrap();
        ws.document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .put_image_data(&grid(3, 2), 0, 0);
        let mut top = Layer::create_layer(3, 2, "Top");
        top.properties.opacity = 77;
        ws.add_layer(top);
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 1.0, 1.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        transform_document(&mut app, Transform::Rot90CW).unwrap();
        let rotated = ImageData {
            width: 2,
            height: 3,
            data: layer_bytes(&app, Some(0)),
        };
        assert_eq!(ids(&rotated), [3, 0, 4, 1, 5, 2]);
        let ws = app.active_ws().unwrap();
        assert_eq!(
            (
                doc(&app).width,
                doc(&app).layers.get_at(1).unwrap().properties.opacity
            ),
            (2, 77)
        );
        assert_eq!((ws.active_layer, ws.selection.is_empty()), (1, true));
        assert_eq!(ws.history.undo.last().unwrap().name, "Rotate 90° Clockwise");
        assert!(app.undo());
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.selection.is_empty()), (3, false));
        assert_eq!(
            ids(&ImageData {
                width: 3,
                height: 2,
                data: layer_bytes(&app, Some(0))
            }),
            [0, 1, 2, 3, 4, 5]
        );
    }

    /// Layer flips change only the active layer's pixels, and undo restores them.
    #[test]
    fn layer_flip_is_one_undoable_layer_change() {
        let mut app = fresh_doc(3, 2, Some(3));
        let src = layer_bytes(&app, Some(0));
        transform_active_layer(&mut app, Transform::FlipV);
        let flipped = transform_pixels(
            &ImageData {
                width: 3,
                height: 2,
                data: src.clone(),
            },
            Transform::FlipV,
        );
        assert_eq!(layer_bytes(&app, Some(0)), flipped.data);
        transform_active_layer(&mut app, Transform::Rot90CW);
        assert_eq!(layer_bytes(&app, Some(0)), flipped.data);
        assert!(app.undo());
        assert_eq!(layer_bytes(&app, Some(0)), src);
    }

    /// compositing.md §2.5: the flattened layer is exactly the document composite.
    #[test]
    fn flatten_is_the_composite() {
        let mut app = fresh_doc(5, 4, Some(2));
        let mut top = Layer::create_layer(5, 4, "Top");
        top.surface
            .data_mut()
            .copy_from_slice(&crate::testing::fixture_rgba(5, 4, 7));
        top.properties.blend_mode = crate::document::layer::BlendMode::Multiply;
        app.active_ws_mut().unwrap().add_layer(top);
        let want = doc(&app).composite().data().to_vec();
        assert!(flatten_document(&mut app));
        assert_eq!(layer_bytes(&app, Some(0)), want);
        assert!(!flatten_document(&mut app));
    }

    /// §2.2 step 2 (R11): a DPI-only Resize sets the DPI without a history entry; the side and
    /// memory checks fail before anything changes.
    #[test]
    fn resize_dpi_only_and_errors() {
        let mut app = fresh_doc(4, 4, None);
        let entries = app.active_ws().unwrap().history.undo.len();
        let layer = layer_bytes(&app, Some(0));
        resize_image(&mut app, 4, 4, ResizeMode::Bicubic, Gamma::Linear, 96.0).unwrap();
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.history.undo.len(), ws.dirty), (entries, false));
        resize_image(&mut app, 4, 4, ResizeMode::Bicubic, Gamma::Linear, 300.0).unwrap();
        let ws = app.active_ws().unwrap();
        let top = ws.history.undo.last().unwrap();
        assert_eq!(
            (
                doc(&app).resolution,
                ws.history.undo.len(),
                top.name.as_str()
            ),
            (300.0, entries + 1, "Resize...")
        );
        assert_eq!(top.image, "assets/icons/menu_image_resize_icon.png");
        assert!(app.undo());
        assert_eq!(
            (doc(&app).resolution, layer_bytes(&app, Some(0))),
            (96.0, layer)
        );
        assert!(app.redo());
        assert_eq!(doc(&app).resolution, 300.0);
        assert_eq!(
            resize_image(&mut app, 0, 4, ResizeMode::Fant, Gamma::Linear, 96.0),
            Err(OpError::TooLarge)
        );
        assert_eq!(try_surface(MAX_SIDE + 1, 1).err(), Some(OpError::TooLarge));
        assert_eq!(reserve(usize::MAX).err(), Some(OpError::OutOfMemory));
        let mut none = crate::app::AppWorkspace::default();
        assert_eq!(
            change_canvas_size(&mut none, 2, 2, Anchor::TopLeft, CanvasFill::White, 96.0),
            Err(OpError::NoDocument)
        );
    }

    /// The commit-first hook (painting-tools-2.md §7.6): the active tool is deactivated around
    /// the operation and active again after it.
    #[test]
    fn active_tool_is_finished_and_reactivated() {
        use crate::testing::{Ev, drive_tool};
        use crate::tool::{MouseButton::Left, ToolType};
        let mut app = fresh_doc(20, 20, Some(2));
        drive_tool(
            &mut app,
            ToolType::RectangleSelect,
            &[
                Ev::Down(2.0, 3.0, Left),
                Ev::Move(9.0, 8.0),
                Ev::Wait(60.0),
                Ev::Up(9.0, 8.0, Left),
            ],
        );
        let b = crop_bounds(&app.active_ws().unwrap().selection.get_bounds(), 20, 20);
        assert!(crop_to_selection(&mut app));
        assert_eq!(
            (doc(&app).width, doc(&app).height, b.is_empty()),
            (b.w, b.h, false)
        );
        assert!(app.tool_active && app.tool_type == Some(ToolType::RectangleSelect));
    }

    /// §2.2 step 3: every layer is scaled with its properties, the active layer and new DPI kept.
    #[test]
    fn resize_keeps_layers_and_properties() {
        let mut app = fresh_doc(8, 6, Some(1));
        let mut top = Layer::create_layer(8, 6, "Top");
        top.properties.opacity = 77;
        app.active_ws_mut().unwrap().add_layer(top);
        resize_image(&mut app, 3, 5, ResizeMode::Fant, Gamma::Companded, 150.0).unwrap();
        let ws = app.active_ws().unwrap();
        let p: Vec<_> = ws
            .document
            .layers
            .iter()
            .map(|l| {
                (
                    l.properties.name.clone(),
                    l.properties.opacity,
                    l.surface.width(),
                )
            })
            .collect();
        assert_eq!(p, [("Background".into(), 255, 3), ("Top".into(), 77, 3)]);
        assert_eq!(
            (
                ws.active_layer,
                ws.document.resolution,
                ws.history.undo.last().unwrap().name.as_str()
            ),
            (1, 150.0, "Resize...")
        );
    }

    /// §2.4 step 4: the mask follows the shared rendering quality (here Aliased).
    #[test]
    fn crop_uses_the_rendering_quality() {
        use crate::selection::CombineMode;
        use crate::util::geom::Point;
        let mut app = fresh_doc(23, 17, Some(4));
        app.tool_settings.rendering_quality = "aliased".into();
        let src = layer_bytes(&app, Some(0));
        let ws = app.active_ws_mut().unwrap();
        let tri = [
            Point::new(2.0, 2.0),
            Point::new(15.0, 3.0),
            Point::new(9.0, 14.0),
        ];
        ws.selection
            .set_continuation_points(&tri, CombineMode::Replace);
        ws.selection.commit_continuation();
        let sel = &app.active_ws().unwrap().selection;
        let mask = crate::effect::selection_mask(sel, SelectionQuality::Aliased, 23, 17).unwrap();
        let b = crop_bounds(&sel.get_bounds(), 23, 17);
        assert!(crop_to_selection(&mut app));
        let out = layer_bytes(&app, Some(0));
        for (j, p) in out.chunks(4).enumerate() {
            let (sx, sy) = ((j as u32 % b.w) as i32 + b.x, (j as u32 / b.w) as i32 + b.y);
            let i = (sy * 23 + sx) as usize * 4;
            let want = apply_alpha_mask(src[i..i + 4].try_into().unwrap(), mask.get(sx, sy));
            assert_eq!(p, want, "({sx},{sy})");
        }
    }

    /// §2.4 step 6: Crop keeps the zoom and moves the canvas offset by −(b.x, b.y), so the kept
    /// pixels stay where they were on screen; undo restores the view.
    #[test]
    fn crop_keeps_zoom_and_shifts_the_viewport() {
        use crate::selection::CombineMode;
        let mut app = fresh_doc(40, 30, Some(4));
        let ws = app.active_ws_mut().unwrap();
        (ws.view.view_width, ws.view.view_height) = (300.0, 200.0);
        ws.view.set_zoom_center(2.0);
        ws.view.shift_view_position(-7.0, 5.0);
        let sel = Rectangle::new(4.0, 3.0, 30.0, 20.0);
        ws.selection.set_continuation(&sel, CombineMode::Replace);
        ws.selection.commit_continuation();
        let v = &ws.view;
        let (zoom, before, viewport) =
            (v.zoom, v.get_render_bounds(), (v.viewport_x, v.viewport_y));
        assert!(crop_to_selection(&mut app));
        let v = &app.active_ws().unwrap().view;
        let after = v.get_render_bounds();
        assert_eq!((v.zoom, after.width, after.height), (zoom, 60.0, 40.0));
        assert!(
            (after.x - (before.x + 8.0)).abs() < 1e-9,
            "{after:?} {before:?}"
        );
        assert!(
            (after.y - (before.y + 6.0)).abs() < 1e-9,
            "{after:?} {before:?}"
        );
        assert!(app.undo());
        let v = &app.active_ws().unwrap().view;
        assert_eq!(
            (v.zoom, v.viewport_x, v.viewport_y),
            (zoom, viewport.0, viewport.1)
        );
    }
}

/// One accepted Import From File batch. The UI may load/decode each source asynchronously.
/// Native contract: file-flows-and-palettes.md, including handled-failure rollback.
pub struct ImportBatch {
    workspace: u64,
    previous_tool: Option<crate::tool::ToolType>,
    parts: Vec<HistoryMemento>,
    redo: std::collections::VecDeque<HistoryMemento>,
    last_bounds: Option<Rectangle>,
}
impl ImportBatch {
    /// Call only after a nonempty chooser result. Deactivation commits outside the compound.
    pub fn begin(app: &mut AppWorkspace) -> Option<Self> {
        let previous_tool = app.tool_type;
        app.deactivate_tool();
        app.tool = None;
        app.tool_type = None;
        let ws = app.active_ws_mut()?;
        Some(Self {
            workspace: ws.id,
            previous_tool,
            parts: vec![],
            redo: std::mem::take(&mut ws.history.redo),
            last_bounds: None,
        })
    }
    fn activate(&self, app: &mut AppWorkspace) -> bool {
        let Some(i) = app.documents.iter().position(|w| w.id == self.workspace) else {
            return false;
        };
        if app.active != Some(i) {
            app.set_active_document_workspace(Some(i));
        }
        true
    }
    pub fn add(
        &mut self,
        app: &mut AppWorkspace,
        file: &str,
        layers: Vec<(LayerProperties, ImageData)>,
    ) -> Result<(), OpError> {
        if !self.activate(app) || layers.is_empty() {
            return Err(OpError::NoDocument);
        }
        let stem = std::path::Path::new(file)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(file);
        for (mut properties, image) in layers {
            check_sides(image.width, image.height)?;
            if (image.width as u64)
                .checked_mul(image.height as u64)
                .and_then(|n| n.checked_mul(4))
                != Some(image.data.len() as u64)
            {
                return Err(OpError::TooLarge);
            }
            let ws = app.active_ws_mut().ok_or(OpError::NoDocument)?;
            if !ws.selection.is_empty() {
                self.parts.push(HistoryMemento::new(
                    "Import From File",
                    "",
                    MementoKind::Selection(ws.selection.save()),
                ));
                ws.selection.reset();
            }
            let (w, h, dpi) = (
                ws.document.width.max(image.width),
                ws.document.height.max(image.height),
                ws.document.resolution,
            );
            check_memory(ws.document.layers.size() + 1, w, h, 0)?;
            if (w, h) != (ws.document.width, ws.document.height) {
                change_canvas_size(app, w, h, Anchor::TopLeft, CanvasFill::Transparent, dpi)?;
                self.parts.push(
                    app.active_ws_mut()
                        .unwrap()
                        .history
                        .undo
                        .pop()
                        .expect("canvas growth memento"),
                );
            }
            let mut surface = try_surface(w, h)?;
            surface.fill_rect(surface.int_bounds(), [255, 255, 255, 0]);
            for y in 0..image.height as usize {
                let source = y * image.width as usize * 4;
                let target = y * w as usize * 4;
                surface.data_mut()[target..target + image.width as usize * 4]
                    .copy_from_slice(&image.data[source..source + image.width as usize * 4]);
            }
            properties.name = format!("{stem}: {}", properties.name);
            properties.is_background = false;
            let ws = app.active_ws_mut().unwrap();
            let index = ws.document.layers.size();
            ws.document.add_layer(Layer {
                id: Layer::fresh_id(),
                properties,
                surface,
                text: None,
            });
            ws.set_active_layer(index);
            self.parts.push(HistoryMemento::new(
                "Import From File",
                "",
                MementoKind::NewLayer { index },
            ));
            self.last_bounds = Some(Rectangle::new(
                0.,
                0.,
                image.width as f64,
                image.height as f64,
            ));
        }
        Ok(())
    }
    pub fn finish(mut self, app: &mut AppWorkspace, success: bool) -> bool {
        if !self.activate(app) {
            return false;
        }
        let success = success && self.last_bounds.is_some();
        let ws = app.active_ws_mut().unwrap();
        if success {
            self.parts.push(HistoryMemento::new(
                "Import From File",
                "",
                MementoKind::Selection(ws.selection.save()),
            ));
            ws.selection.set_continuation(
                &self.last_bounds.unwrap(),
                crate::selection::CombineMode::Replace,
            );
            ws.selection.commit_continuation();
            ws.push_memento(HistoryMemento::new(
                &i18n::t("menu.layers.importFromFile.text"),
                "assets/icons/menu_layers_import_from_file_icon.png",
                MementoKind::Compound(self.parts),
            ));
        } else {
            if !self.parts.is_empty() {
                ws.apply_memento(HistoryMemento::new(
                    "",
                    "",
                    MementoKind::Compound(self.parts),
                ));
            }
            ws.history.redo = self.redo;
        }
        if let Some(tool) = if success {
            Some(crate::tool::ToolType::Move)
        } else {
            self.previous_tool
        } {
            app.set_active_tool_from_type(tool);
        }
        success
    }
}
/// Native single-file entry point, sharing the batch implementation. Appendix B B-F11 calibrates
/// its history and canvas growth.
pub fn import_layers(
    app: &mut AppWorkspace,
    file_name: &str,
    layers: Vec<(LayerProperties, ImageData)>,
) -> bool {
    let Some(mut batch) = ImportBatch::begin(app) else {
        return false;
    };
    let success = batch.add(app, file_name, layers).is_ok();
    batch.finish(app, success)
}
/// Synchronous batch adapter. The UI uses ImportBatch for asynchronous platform decoding.
pub fn import_files<'a>(
    app: &mut AppWorkspace,
    files: impl IntoIterator<Item = &'a str>,
    mut load: impl FnMut(&'a str) -> Result<Vec<(LayerProperties, ImageData)>, String>,
) -> bool {
    let mut files = files.into_iter().peekable();
    if files.peek().is_none() {
        return false;
    }
    let Some(mut batch) = ImportBatch::begin(app) else {
        return false;
    };
    for name in files {
        let Ok(layers) = load(name) else {
            return batch.finish(app, false);
        };
        if batch.add(app, name, layers).is_err() {
            return batch.finish(app, false);
        }
    }
    batch.finish(app, true)
}

#[cfg(test)]
mod import_tests {
    use super::*;
    use crate::{selection::CombineMode, testing::fresh_doc};
    fn image(w: u32, h: u32) -> ImageData {
        ImageData {
            width: w,
            height: h,
            data: [9, 8, 7, 255].repeat((w * h) as usize),
        }
    }
    fn layers(w: u32, h: u32) -> Vec<(LayerProperties, ImageData)> {
        vec![(
            LayerProperties::new("Background", true, true, 255),
            image(w, h),
        )]
    }
    /// document-ops-and-formats §2.7; file-flows-and-palettes batch ordering.
    #[test]
    fn import_from_file_grow_padding_selection_undo_redo() {
        let mut app = fresh_doc(4, 3, Some(4));
        let n = app.active_ws().unwrap().history.undo.len();
        assert!(import_layers(&mut app, "photo.png", layers(6, 2)));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (6, 3));
        let top = ws
            .document
            .layers
            .get_at(ws.document.layers.size() - 1)
            .unwrap();
        assert_eq!(
            (top.properties.name.as_str(), top.properties.is_background),
            ("photo: Background", false)
        );
        assert_eq!(top.surface.pixel(0, 2), [255, 255, 255, 0]);
        assert_eq!(
            ws.document.layers.get_at(0).unwrap().surface.pixel(5, 0),
            [0; 4]
        );
        assert_eq!(ws.selection.get_bounds(), Rectangle::new(0., 0., 6., 2.));
        assert_eq!(ws.history.undo.len(), n + 1);
        assert!(matches!(
            ws.history.undo.last().unwrap().kind,
            MementoKind::Compound(_)
        ));
        assert!(app.undo());
        assert_eq!(app.active_ws().unwrap().document.width, 4);
        assert!(app.redo());
        assert_eq!(app.active_ws().unwrap().document.width, 6);
    }
    /// file-flows-and-palettes: A then failed B rolls A back and never loads C.
    #[test]
    fn import_batch_rolls_back_stops_and_restores_selection() {
        let mut app = fresh_doc(4, 3, Some(4));
        app.active_ws_mut()
            .unwrap()
            .selection
            .set_continuation(&Rectangle::new(1., 1., 2., 1.), CombineMode::Replace);
        app.active_ws_mut().unwrap().selection.commit_continuation();
        let n = app.active_ws().unwrap().history.undo.len();
        let mut called = vec![];
        assert!(!import_files(
            &mut app,
            ["A.png", "B.png", "C.png"],
            |name| {
                called.push(name);
                if name == "B.png" {
                    Err("failed".into())
                } else {
                    Ok(layers(6, 2))
                }
            }
        ));
        assert_eq!(called, ["A.png", "B.png"]);
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (4, 3));
        assert_eq!(ws.history.undo.len(), n);
        assert_eq!(ws.selection.get_bounds(), Rectangle::new(1., 1., 2., 1.));
    }
    #[test]
    fn import_batch_one_entry_preserves_destination_dpi_and_last_bounds() {
        let mut app = fresh_doc(4, 3, Some(4));
        app.active_ws_mut().unwrap().document.resolution = 300.;
        let n = app.active_ws().unwrap().history.undo.len();
        assert!(import_files(&mut app, ["A.png", "B.png"], |name| Ok(
            if name == "A.png" {
                layers(8, 2)
            } else {
                layers(2, 7)
            }
        )));
        let ws = app.active_ws().unwrap();
        assert_eq!(
            (
                ws.document.width,
                ws.document.height,
                ws.document.resolution
            ),
            (8, 7, 300.)
        );
        assert_eq!(ws.selection.get_bounds(), Rectangle::new(0., 0., 2., 7.));
        assert_eq!(ws.history.undo.len(), n + 1);
        assert!(app.undo());
        assert_eq!(app.active_ws().unwrap().document.width, 4);
        assert!(app.redo());
        assert_eq!(app.active_ws().unwrap().document.height, 7);
    }
    #[test]
    fn import_cancel_keeps_pending_gradient_and_accepted_failure_keeps_its_commit() {
        use crate::testing::{Ev, drive_tool};
        use crate::tool::{MouseButton::Left, ToolType};
        let mut app = fresh_doc(4, 2, None);
        drive_tool(
            &mut app,
            ToolType::Gradient,
            &[Ev::Down(1., 1., Left), Ev::Up(1., 1., Left)],
        );
        let n = app.active_ws().unwrap().history.undo.len();
        let generation = app.tool_generation;
        assert!(!import_files(&mut app, [], |_| unreachable!()));
        assert_eq!(app.tool_generation, generation);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        assert!(!import_files(&mut app, ["bad.png"], |_| Err(
            "failed".into()
        )));
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n + 1);
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Finish Gradient"
        );
        assert_eq!(app.tool_type, Some(ToolType::Gradient));
        assert!(app.undo());
    }
    #[test]
    fn import_rollback_can_select_original_top_and_reverses_partial_multilayer() {
        let mut app = fresh_doc(4, 3, Some(4));
        for i in 0..2 {
            app.active_ws_mut().unwrap().add_layer(Layer::create_layer(
                4,
                3,
                &format!("Original {i}"),
            ));
        }
        app.active_ws_mut().unwrap().set_active_layer(0);
        let n = app.active_ws().unwrap().history.undo.len();
        assert!(!import_files(&mut app, ["ok.png", "bad.png"], |name| {
            if name == "ok.png" {
                Ok(layers(4, 3))
            } else {
                Err("bad".into())
            }
        }));
        assert_eq!(app.active_ws().unwrap().active_layer, 2);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), n);
        let mut bad = layers(8, 2);
        bad.push((
            LayerProperties::new("Bad", true, false, 255),
            ImageData {
                width: 3,
                height: 2,
                data: vec![0],
            },
        ));
        assert!(!import_layers(&mut app, "multi.pdn", bad));
        assert_eq!(app.active_ws().unwrap().document.width, 4);
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 3);
    }
}
