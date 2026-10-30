//! The Layers form: the active document's layers, top first, each with its thumbnail, name and
//! visibility checkbox; drag-to-reorder; and the layer strip.
//! Port of: js/src/interface/window/form/LayerForm.js, form/layer/LayerItem.js, css/window.css
//! (`#layerForm`, "Consistent form density")
//! Reference: paintdotjs @ b4e6fc5
//!
//! Rows are Paint.NET's 62 DIPs with a 54-DIP thumbnail box (rule 4) in paintdotjs's look; the
//! thumbnails are Paint.NET's (raw layer pixels, linear-light Catmull-Rom, Small checker, ring),
//! scaled through the shared `kit::thumbnails` scheduler and recorded as pdn rects.
//! Not built: the 0.5-s thumbnail fade-in (§2.8 Scheduling; static captures equal its end).
use std::collections::HashMap;

use egui::{Color32, ColorImage, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use pdn_core::app::{AppEvent, AppWorkspace};
use pdn_core::document::layer::Layer;
use pdn_core::render::thumbnail::{ThumbFrame, render_framed, ring_extent, scale_thumbnail};

use super::history::{list_area, split_strip, strip};
use crate::kit::button::{self, MenuItemStyle};
use crate::kit::glyph::{dip_pt, dip_px};
use crate::kit::inputs;
use crate::kit::px;
use crate::kit::scroll::ScrollList;
use crate::kit::shadow::{self, BoxShadow, TexturePixels};
use crate::kit::text::{self, FontSpec, record_pdn_rect};
use crate::kit::thumbnails::{self, Scaler, ThumbnailMode};
use crate::shell::canvas_view::DisplayPrefs;
use crate::theme::{self, *};

/// The layer strip (LayerForm.buildContent), in order.
const STRIP: [&str; 7] = [
    "menu.layers.add.new.layer",
    "menu.layers.delete.layer",
    "menu.layers.duplicate.layer",
    "menu.layers.merge.layer.down",
    "menu.layers.move.layer.up",
    "menu.layers.move.layer.down",
    "menu.layers.layer.properties",
];

/// A layer row's framed thumbnail (`ui-drawn-graphics.md §2.8`, row "Layer row"): the raw layer
/// pixels scaled to at most `LAYER_THUMB_MAX` DIPs (opacity, blend mode and visibility ignored),
/// then the ring, checkerboard and backdrop of `frame`. RGBA8, width, height.
pub fn layer_thumbnail(layer: &Layer, ppp: f32, frame: &ThumbFrame) -> (Vec<u8>, u32, u32) {
    let s = &layer.surface;
    let (px, w, h) = scale_thumbnail(s.data(), s.width(), s.height(), max_edge(ppp));
    render_framed(&px, w, h, frame)
}

/// `LAYER_THUMB_MAX` in device pixels.
fn max_edge(ppp: f32) -> u32 {
    dip_px(LAYER_THUMB_MAX, ppp).max(1) as u32
}

/// A scaled thumbnail by (layer pixels' revision, device max edge): RGBA8 and size.
type Scaled = HashMap<(u64, u32), (Vec<u8>, [u32; 2])>;

/// A row's framed texture: what it was made of, the texture and the thumbnail's size.
type Framed = (((u64, u32), ThumbFrame), TextureHandle, [u32; 2]);

#[derive(Default)]
struct RowThumb {
    /// `thumbnails::due`'s state for the row's layer pixels.
    seen: Option<(u64, f64)>,
    /// The scaled thumbnail the row shows (the stale one while a new one waits).
    shown: Option<(u64, u32)>,
    framed: Option<Framed>,
}

#[derive(Default)]
pub struct LayerForm {
    list: ScrollList,
    scaler: Scaler,
    scaled: Scaled,
    /// Per row (document, layer index).
    rows: HashMap<(u64, usize), RowThumb>,
    /// The rows shown, as each one's layer index and rectangle, the window's layer and the pass that showed them: where
    /// a live session's dots for who is on which layer go (`crate::live`).
    pub shown: Vec<(usize, Rect)>,
    pub shown_layer: Option<egui::LayerId>,
    pub shown_pass: u64,
}

impl LayerForm {
    /// Where thumbnails are scaled (the app's default: `ThumbnailMode::default()`).
    pub fn set_thumbnail_mode(&mut self, mode: ThumbnailMode) {
        self.scaler.set_mode(mode);
    }

    /// While the form is hidden: the scaler drops its job and its snapshot buffer.
    pub fn release(&mut self, ctx: &egui::Context) {
        let _ = self.scaler.poll(ctx, |_, _| None);
        self.scaler.trim(0);
    }

    /// Brings every row's scaled thumbnail up to date (§2.8 Scheduling: 200 ms after the layer's
    /// pixels last changed, at once when the scale changed; one job at a time, the stale one
    /// showing meanwhile). Returns the seconds until a waiting one is due.
    fn update_thumbnails(
        &mut self,
        ctx: &egui::Context,
        app: &AppWorkspace,
        now: f64,
    ) -> Option<f64> {
        let Some(ws) = app.active_ws() else {
            self.rows.clear();
            self.scaled.clear();
            self.scaler.trim(0);
            return None;
        };
        // A drag's pixels are lifted out of the layer meanwhile (`render::float`): the thumbnails
        // keep what they show until the drag's end renders them in.
        if ws.float.is_some() {
            return None;
        }
        let edge = max_edge(ctx.pixels_per_point());
        let doc = ws.document.id;
        let layers: Vec<&Layer> = ws.document.layers.iter().collect();
        let wanted: Vec<(u64, u32)> = layers
            .iter()
            .map(|l| (l.surface.revision(), edge))
            .collect();
        // A sliced job reads its layer's pixels while they are unchanged.
        let source = |stamp: u64, _| {
            layers
                .iter()
                .find(|l| l.surface.revision() == stamp)
                .map(|l| l.surface.data())
        };
        if let Some((_, key, (px, w, h))) = self.scaler.poll(ctx, source)
            && wanted.contains(&key)
        {
            self.scaled.insert(key, (px, [w, h]));
        }
        self.rows.retain(|(d, i), _| *d == doc && *i < layers.len());
        let mut pending: Option<f64> = None;
        for (i, layer) in layers.iter().enumerate() {
            let want = wanted[i];
            let row = self.rows.entry((doc, i)).or_default();
            // The row's thumbnail now belongs to another layer (one was inserted or removed
            // below it): this row's layer is new here, so its thumbnail is due at once.
            if row.shown.is_some_and(|s| s != want && wanted.contains(&s)) {
                row.shown = None;
                row.seen = None;
            }
            let due = thumbnails::due(&mut row.seen, want.0, now);
            if self.scaled.contains_key(&want) {
                row.shown = Some(want);
                continue;
            }
            if !due && row.shown.is_some_and(|(_, e)| e == edge) {
                let changed_at = row.seen.map_or(now, |s| s.1);
                let wait = changed_at + thumbnails::THUMBNAIL_LATENCY_S - now;
                pending = Some(pending.map_or(wait, |p| p.min(wait)));
                continue;
            }
            if self.scaler.busy() {
                continue;
            }
            let s = &layer.surface;
            let size = [s.width(), s.height()];
            if let Some((_, key, (px, w, h))) = self.scaler.start(ctx, want.0, want, s.data(), size)
            {
                self.scaled.insert(key, (px, [w, h]));
                row.shown = Some(want);
            }
        }
        let shown: Vec<(u64, u32)> = self.rows.values().filter_map(|r| r.shown).collect();
        self.scaled
            .retain(|k, _| wanted.contains(k) || shown.contains(k));
        let largest = layers.iter().map(|l| l.surface.data().len()).max();
        self.scaler.trim(largest.unwrap_or(0));
        pending
    }

    /// The framed texture of row `row` for `frame`.
    fn framed(
        &mut self,
        ctx: &egui::Context,
        row: (u64, usize),
        frame: ThumbFrame,
    ) -> Option<(TextureHandle, [u32; 2])> {
        let r = self.rows.get_mut(&row)?;
        let key = r.shown?;
        let (px, [w, h]) = self.scaled.get(&key).filter(|s| s.1[0] > 0)?;
        let want = (key, frame);
        if r.framed.as_ref().map(|f| &f.0) != Some(&want) {
            let (rgba, fw, fh) = render_framed(px, *w, *h, &frame);
            let image = ColorImage::from_rgba_unmultiplied([fw as usize, fh as usize], &rgba);
            let tex = ctx.load_texture(
                format!("layers/{}/{}", row.0, row.1),
                image,
                TextureOptions::NEAREST,
            );
            r.framed = Some((want, tex, [*w, *h]));
        }
        r.framed.as_ref().map(|(_, tex, size)| (tex.clone(), *size))
    }

    /// Shows the form in `ui`'s rect at time `now` (s): `display` gives the thumbnails'
    /// checkerboard and blending, `auto_select` is `ui.autoSelectVisibleLayer`, `pixels` the
    /// textures under a dragged row's shadow. Returns an action to run (a strip button, or Layer
    /// Properties from a double click).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        app: &mut AppWorkspace,
        display: DisplayPrefs,
        auto_select: bool,
        now: f64,
        pixels: TexturePixels,
    ) -> Option<&'static str> {
        let ctx = ui.ctx().clone();
        let theme = theme::current(&ctx);
        let ppp = ctx.pixels_per_point();
        if let Some(wait) = self.update_thumbnails(&ctx, app, now) {
            ctx.request_repaint_after_secs(wait.max(0.0) as f32);
        }
        let mut action = None;
        let (list_rect, strip_rect) = split_strip(ui.max_rect());
        // Layers top first (LayerForm.buildContent `addAt(0, …)`).
        let (doc, names, visible, active, texts) = match app.active_ws() {
            Some(ws) => {
                let layers: Vec<&Layer> = ws.document.layers.iter().rev().collect();
                (
                    ws.document.id,
                    layers
                        .iter()
                        .map(|l| l.properties.name.clone())
                        .collect::<Vec<_>>(),
                    layers
                        .iter()
                        .map(|l| l.properties.visible)
                        .collect::<Vec<_>>(),
                    Some(ws.document.layers.size() - 1 - ws.active_layer),
                    layers.iter().map(|l| l.text.is_some()).collect::<Vec<_>>(),
                )
            }
            None => (0, vec![], vec![], None, vec![]),
        };
        let n = names.len();
        let extent = dip_pt(LAYER_ROW, ppp);
        let content = n as f32 * extent;
        let area = list_area(ui, &mut self.list, list_rect, content, "layer-list");
        // A row a notch (`VerticalImageStrip.OnPointerWheelChanged`).
        self.list
            .wheel_notches(ui, area, content, extent, "layer-list");
        let keys = app
            .active_ws()
            .map_or_else(Vec::new, |ws| row_keys(&ws.document.layers));
        let slots = self.list.layout(ui, area, &keys, extent, now);
        self.shown.clear();
        self.shown.extend(
            (slots.iter().enumerate())
                .filter(|(_, slot)| slot.intersects(area))
                .map(|(r, slot)| (n - 1 - r, slot.intersect(area))),
        );
        (self.shown_layer, self.shown_pass) = (Some(ui.layer_id()), ctx.cumulative_pass_nr());
        let dragging = self.list.dragging();
        let painter = ui.painter().with_clip_rect(area.intersect(ui.clip_rect()));
        let spec = FontSpec::new(FORM_FONT_SIZE);
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        let pointer = ui.input(|i| i.pointer.interact_pos());
        // The dragged item paints last (`z-index: 3`).
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by_key(|&r| dragging == Some(keys[r]));
        let mut select = None;
        let mut toggle = None;
        for r in order {
            let slot = slots[r];
            if !slot.intersects(area) {
                continue;
            }
            let RowParts {
                item,
                inner,
                thumb_box,
                name_x,
            } = row_parts(slot, ppp);
            let snapped = px::snap_rect(item, ppp);
            let id = ui.scope_id().with(("layer", keys[r]));
            // `.dragging-item { pointer-events: none }`: the rows under it keep the pointer.
            let dragged = dragging == Some(keys[r]);
            let response = if dragged {
                ui.interact(Rect::NOTHING, id, Sense::hover())
            } else {
                ui.interact(snapped.intersect(area), id, Sense::click())
            };
            // `.dragging-item`: a drop shadow, and the row at 0.92 opacity.
            let (shadow_dy, shadow_blur, shadow_color) = DRAGGING_ITEM_SHADOW;
            let under = dragged.then(|| {
                let reach = snapped
                    .translate(Vec2::new(0.0, shadow_dy))
                    .expand(shadow_blur + 1.0);
                shadow::capture_underlay(ui, reach.union(snapped))
            });
            if dragged {
                let shadow = BoxShadow {
                    offset: Vec2::new(0.0, shadow_dy),
                    blur: shadow_blur,
                    color: shadow_color,
                };
                shadow::paint_box_shadow(ui, id.with("shadow"), snapped, shadow, pixels);
                ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            let check_rect = Rect::from_min_size(
                Pos2::new(
                    inner.max.x - CHECKBOX_MARGIN.1 - CHECKBOX_SIZE,
                    inner.center().y - CHECKBOX_SIZE / 2.0,
                ),
                Vec2::splat(CHECKBOX_SIZE),
            );
            // `:hover` reaches the row through its checkbox (last frame's hover: the checkbox
            // registers after the row it is painted on).
            let check_id = ui.scope_id().with(("checkbox", ("layer-visible", keys[r])));
            let check_hovered = ctx.read_response(check_id).is_some_and(|c| c.hovered());
            let hovered = response.hovered() || check_hovered || dragged;
            let state = MenuItemStyle::of(hovered, false, active == Some(r), true);
            let fill = button::paint_display_item_frame(
                &painter,
                snapped,
                state,
                &theme,
                theme.window_background,
                display.mode,
            );

            let frame = ThumbFrame {
                u: ppp as f64,
                checker_brightness: display.checker_brightness,
                dark: display.dark,
                mode: display.mode,
                checker: true,
                backdrop: [fill.r(), fill.g(), fill.b(), 255],
            };
            let layer_index = n - 1 - r;
            // Paint.NET's pixels (R21 (c)): the box, and the thumbnail with its ring (which can
            // poke out of the box at fractional scales); paintdotjs's `.thumbnail` canvas.
            let mut pdn = thumb_box;
            if let Some((tex, [tw, th])) = self.framed(&ctx, (doc, layer_index), frame) {
                let e = ring_extent(ppp as f64) as f32;
                let left = (thumb_box.center().x * ppp - tw as f32 / 2.0).round();
                let top = (thumb_box.center().y * ppp - th as f32 / 2.0).round();
                let img_rect = Rect::from_min_size(
                    Pos2::new((left - e) / ppp, (top - e) / ppp),
                    Vec2::new((tw as f32 + 2.0 * e) / ppp, (th as f32 + 2.0 * e) / ppp),
                );
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                painter.image(tex.id(), img_rect, uv, Color32::WHITE);
                pdn = pdn.union(img_rect);
            }
            record_pdn_rect(&ctx, pdn.intersect(area));

            let line = spec.resolved_line_height(ppp);
            if texts[r] {
                text_badge(&painter, thumb_box, fill, spec, line, theme.menu_item_text);
            }
            let name_at = Pos2::new(name_x, inner.center().y - line / 2.0);
            text::draw(&painter, name_at, &names[r], spec, theme.menu_item_text);

            let mut checked = visible[r];
            let clip = ui.clip_rect();
            ui.set_clip_rect(area.intersect(clip));
            let check = inputs::checkbox(
                ui,
                ("layer-visible", keys[r]),
                check_rect,
                &mut checked,
                true,
                fill,
            );
            ui.set_clip_rect(clip);
            if active == Some(r) {
                crate::kit::text::record_pdn_rect_over(&ctx, snapped.intersect(area));
            }
            if checked != visible[r] {
                toggle = Some((layer_index, checked));
            }
            // ScrollList: a press selects (and may start a drag); a double click outside the
            // checkbox opens Layer Properties.
            if pressed && response.hovered() && !check.hovered() {
                if active != Some(r) {
                    select = Some(layer_index);
                }
                if let Some(p) = pointer {
                    self.list.press(keys[r], p, slot);
                }
            }
            if response.double_clicked() {
                select = Some(layer_index);
                action = Some("menu.layers.layer.properties");
            }
            match &under {
                Some(under) => {
                    shadow::paint_veil(ui, id, under, snapped, DRAGGING_ITEM_OPACITY, pixels)
                }
                None => {
                    shadow::forget(&ctx, id.with("veil"));
                    shadow::forget(&ctx, id.with("shadow"));
                }
            }
        }
        let swap = self.list.continue_drag(ui, &keys, &slots, area);
        self.list.cover(ui, list_rect, "layer-list");
        // An active-layer change and a layer swap commit the tool first (painting-tools-2.md
        // §7.6); a visibility toggle is a layer-property edit and does not.
        if let Some(i) = select {
            app.set_active_layer(i);
        }
        if let Some((i, visible)) = toggle
            && let Some(next) = app
                .active_ws_mut()
                .and_then(|ws| set_layer_visible(ws, i, visible, auto_select))
        {
            app.set_active_layer(next);
        }
        // LayerForm's itemSwapper: SwapLayerFunction on the two layers.
        if let Some((a, b)) = swap {
            let (a, b) = (n - 1 - a, n - 1 - b);
            app.commit_tool_first(|app| {
                // The commit can delete an emptied text layer (S§4.5): the rows moved.
                if let Some(ws) = app
                    .active_ws_mut()
                    .filter(|ws| ws.document.layers.size() == n)
                {
                    ws.execute_function(|t| pdn_core::history::functions::swap_layer(t, a, b));
                }
            });
        }
        // The Layers form's buttons: the strip actions' ToolTipText (tooltips.md §3.7).
        let items = STRIP.map(|id| {
            let tip = pdn_core::action::get(id).and_then(|a| a.tooltip_key);
            (id, pdn_core::action::is_executable(app, id), tip)
        });
        strip(ui, strip_rect, &items).or(action)
    }
}

/// S§4.5: a text layer's "T" at the thumbnail box's bottom-left, on the row's fill, with the
/// Extra plus after it.
fn text_badge(
    painter: &egui::Painter,
    thumb_box: Rect,
    fill: Color32,
    spec: FontSpec,
    line: f32,
    color: Color32,
) {
    let ppp = painter.ctx().pixels_per_point();
    let (w, h) = PLUGIN_INDICATOR_SIZE;
    let t = Rect::from_min_size(
        Pos2::new(thumb_box.min.x, thumb_box.max.y - h),
        Vec2::new(w, h),
    );
    painter.rect_filled(px::snap_rect(t, ppp), 0.0, fill);
    let tw = text::measure(painter, "T", spec);
    let at = Pos2::new(t.center().x - tw / 2.0, t.center().y - line / 2.0);
    text::draw(painter, at, "T", spec, color);
    let plus = px::snap_rect(t.translate(Vec2::new(w, 0.0)), ppp);
    crate::kit::img::paint_icon(
        painter,
        "plugin_indicator.png",
        plus,
        fill,
        crate::kit::img::Filter::None,
    );
}

/// A row's parts in its slot (Paint.NET's rnd(62·s)-px row, `ui-drawn-graphics.md §2.0` "Layer
/// list"; R37): the item's border box (inside its 1-DIP margin), its
/// content box, the image box (the slot inset by the rnd(4·s)-px pad, §2.8 Display step 1: 54
/// DIP at 1×) and the name's left edge (after the 50-DIP thumbnail column and its 5-DIP margin).
struct RowParts {
    item: Rect,
    inner: Rect,
    thumb_box: Rect,
    name_x: f32,
}

fn row_parts(slot: Rect, ppp: f32) -> RowParts {
    let item = slot.shrink(LAYER_ITEM_MARGIN);
    let pad = dip_pt(LAYER_THUMB_PAD, ppp);
    let thumb_box = Rect::from_min_size(
        slot.min + Vec2::splat(pad),
        Vec2::splat(slot.height() - 2.0 * pad),
    );
    RowParts {
        item,
        inner: item.shrink(MENU_ITEM_BORDER + SCROLL_ITEM_PADDING),
        thumb_box,
        name_x: thumb_box.center().x + LAYER_THUMB_MAX / 2.0 + LAYER_THUMB_GAP,
    }
}

/// The rows' keys, top first (ScrollList `getKey()` = the layer object): each layer's pixel
/// stamp, made unique by how many layers below share it (a duplicate keeps its original's key on
/// the original, as the JS object identity does).
fn row_keys(layers: &pdn_core::document::layer::LayerList) -> Vec<u64> {
    let stamps: Vec<u64> = layers.iter().map(|l| l.surface.revision()).collect();
    let mut keys: Vec<u64> = stamps
        .iter()
        .enumerate()
        .map(
            |(i, &s)| match stamps[..i].iter().filter(|&&t| t == s).count() {
                0 => s,
                k => egui::Id::unique((s, k)).value(),
            },
        )
        .collect();
    keys.reverse();
    keys
}

/// LayerItem's visibility checkbox: `layer.setVisible` (the layer invalidates), then
/// `document:layer_properties_changed`. No history entry. Returns the layer
/// `ui.autoSelectVisibleLayer` moves the selection to off a hidden active layer: the nearest
/// visible one (below first).
fn set_layer_visible(
    ws: &mut pdn_core::document::workspace::DocumentWorkspace,
    index: usize,
    visible: bool,
    auto_select: bool,
) -> Option<usize> {
    let layer = ws.document.layers.get_at_mut(index)?;
    layer.properties.visible = visible;
    let bounds = layer.get_bounds();
    ws.invalidate(Some(vec![bounds]));
    ws.view.events.push(AppEvent::LayerPropertiesChanged);
    if visible || !auto_select || ws.active_layer != index {
        return None;
    }
    let layers = &ws.document.layers;
    let shown = |i: Option<usize>| i.filter(|&i| layers.get_at(i).is_some_and(|l| l.is_visible()));
    (1..layers.size()).find_map(|d| shown(index.checked_sub(d)).or_else(|| shown(Some(index + d))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdn_core::pixel::IntRect;
    use pdn_core::render::display::RenderMode;
    use pdn_core::render::thumbnail::ThumbFrame;

    fn frame() -> ThumbFrame {
        ThumbFrame {
            u: 1.0,
            checker_brightness: 0.75,
            dark: true,
            mode: RenderMode::Hardware,
            checker: true,
            backdrop: [0x28, 0x28, 0x28, 255],
        }
    }

    /// ui-drawn-graphics.md §3 "Thumb, M = 50": 800×600 → 50×37 (75×56 at ppp 1.5); the frame adds e.
    #[test]
    fn layer_thumbnail_sizes_follow_paint_net() {
        let layer = Layer::create_layer(800, 600, "L");
        let (_, w, h) = layer_thumbnail(&layer, 1.0, &frame());
        assert_eq!((w, h), (52, 39));
        let (_, w, h) = layer_thumbnail(&layer, 1.5, &ThumbFrame { u: 1.5, ..frame() });
        assert_eq!((w, h), (75 + 4, 56 + 4));
    }

    /// §2.8: the thumbnail shows raw layer pixels — a hidden 50 %-opacity layer is not dimmed.
    #[test]
    fn layer_thumbnail_ignores_opacity_and_visibility() {
        let mut layer = Layer::create_layer(4, 4, "L");
        layer
            .surface
            .fill_rect(IntRect::new(0, 0, 4, 4), [10, 200, 30, 255]);
        let (a, _, _) = layer_thumbnail(&layer, 1.0, &frame());
        layer.properties.opacity = 128;
        layer.properties.visible = false;
        let (b, w, _) = layer_thumbnail(&layer, 1.0, &frame());
        assert_eq!(a, b);
        let at = ((w + 1) * 4) as usize;
        assert_eq!(&b[at..at + 4], [10, 200, 30, 255]);
    }

    /// A duplicate layer shares its original's pixel stamp: the rows' keys stay unique, top
    /// first, and the original (the lower layer) keeps the plain stamp.
    #[test]
    fn row_keys_are_unique_for_duplicates() {
        let mut layers = pdn_core::document::layer::LayerList::new();
        let base = Layer::create_layer(4, 4, "A");
        layers.add_layer(base.clone());
        layers.add_layer(base.clone());
        layers.add_layer(Layer::create_layer(4, 4, "B"));
        let keys = row_keys(&layers);
        assert_eq!(keys.len(), 3);
        assert_eq!(keys[2], base.surface.revision());
        assert!(keys[0] != keys[1] && keys[1] != keys[2] && keys[0] != keys[2]);
    }

    /// Controller ruling (Task 19 concern 3; ui-drawn-graphics.md §2.0, §2.8 Display step 1): a
    /// row is 62 DIPs in all, its 54-DIP image box the row inset by 4; the patched reference DOM
    /// (forms-layers-two-layers): items 198×60 at (1057, 500) and (1057, 562), the thumbnail
    /// canvas from x 1062 (centred in the box), the name at x 1117.
    #[test]
    fn layer_rows_are_62_dips_with_a_54_box() {
        assert_eq!(LAYER_ROW, 62.0);
        let slot = |r: f32| {
            Rect::from_min_size(
                Pos2::new(1056.0, 499.0 + r * LAYER_ROW),
                Vec2::new(200.0, LAYER_ROW),
            )
        };
        let first = row_parts(slot(0.0), 1.0);
        let second = row_parts(slot(1.0), 1.0);
        assert_eq!(
            first.item,
            Rect::from_min_size(Pos2::new(1057.0, 500.0), Vec2::new(198.0, 60.0))
        );
        assert_eq!(second.item.min, Pos2::new(1057.0, 562.0));
        assert_eq!(
            first.thumb_box,
            Rect::from_min_size(Pos2::new(1060.0, 503.0), Vec2::splat(54.0))
        );
        assert_eq!(first.inner, first.thumb_box.with_max_x(1252.0));
        assert_eq!(first.thumb_box.center().x - LAYER_THUMB_MAX / 2.0, 1062.0);
        assert_eq!(first.name_x, 1117.0);
    }

    /// Task 19 review: Add New Layer above a lower active layer inserts the new layer at a row
    /// whose old thumbnail is now another layer's; the new layer's thumbnail is due at once, not
    /// 200 ms later with the displaced one showing meanwhile.
    #[test]
    fn a_new_layer_does_not_show_the_displaced_layers_thumbnail() {
        let ctx = egui::Context::default();
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(64, 48, 96.0, true);
        pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
        let mut form = LayerForm::default();
        form.set_thumbnail_mode(ThumbnailMode::Synchronous);
        form.update_thumbnails(&ctx, &app, 10.0);
        let ws = app.active_ws_mut().unwrap();
        ws.set_active_layer(0);
        let doc = ws.document.id;
        pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
        let ws = app.active_ws().unwrap();
        let stamp = |i: usize| ws.document.layers.get_at(i).unwrap().surface.revision();
        assert_eq!(ws.document.layers.size(), 3);
        form.update_thumbnails(&ctx, &app, 10.05);
        let edge = max_edge(1.0);
        for i in 0..3 {
            assert_eq!(
                form.rows[&(doc, i)].shown,
                Some((stamp(i), edge)),
                "row {i}"
            );
        }
    }

    /// Task 19 review (carry 3): a hidden form's scaler holds no job and no buffer.
    #[test]
    fn a_hidden_form_releases_its_scaler() {
        let ctx = egui::Context::default();
        let mut app = AppWorkspace::default();
        app.create_blank_document_in_new_workspace(640, 480, 96.0, true);
        for mode in [ThumbnailMode::Sliced, ThumbnailMode::Worker] {
            let mut form = LayerForm::default();
            form.set_thumbnail_mode(mode);
            form.update_thumbnails(&ctx, &app, 0.0);
            assert!(form.scaler.busy(), "{mode:?}");
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            while form.scaler.busy() || form.scaler.spare_capacity() > 0 {
                form.release(&ctx);
                assert!(std::time::Instant::now() < deadline, "{mode:?}");
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    /// R37 (Task 23): at fractional scales every row is Paint.NET's rnd(62·s) device px (78 at
    /// 1.25, 93 at 1.5, not alternating 77/78) and its image box rnd(62·s) − 2·rnd(4·s) (68, 81)
    /// inset rnd(4·s) (5, 6), from any fractional list origin.
    #[test]
    fn rows_take_paint_nets_device_rounding() {
        for (s, pitch, inset, side) in [(1.25f32, 78.0, 5.0, 68.0), (1.5, 93.0, 6.0, 81.0)] {
            let e = dip_pt(LAYER_ROW, s);
            let top = 499.3;
            let device = |v: f32| (v * s).round();
            for r in 0..6 {
                let slot =
                    Rect::from_min_size(Pos2::new(1056.0, top + r as f32 * e), Vec2::new(200.0, e));
                assert_eq!(
                    device(slot.max.y) - device(slot.min.y),
                    pitch,
                    "@{s} row {r}"
                );
                let b = row_parts(slot, s).thumb_box;
                assert_eq!(((b.min.y - slot.min.y) * s).round(), inset, "@{s}");
                assert_eq!((b.height() * s).round(), side, "@{s}");
            }
        }
    }

    /// R162: exercise the list and rendered row hit boxes, including fractional list origins.
    /// Expectations use the written 62-DIP grid, never slots synthesized by the test.
    #[test]
    fn three_layer_rows_keep_the_absolute_fractional_grid() {
        for (s, pitch) in [(1.0f32, 62.0), (1.25, 78.0), (1.5, 93.0)] {
            for top in [501.0, 499.3, 501.7] {
                let ctx = egui::Context::default();
                crate::seed_native_scale(&ctx, s);
                crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
                let mut app = AppWorkspace::default();
                app.create_blank_document_in_new_workspace(64, 48, 96.0, true);
                for _ in 0..2 {
                    pdn_core::action::perform(&mut app, "menu.layers.add.new.layer");
                }
                let keys = row_keys(&app.active_ws().unwrap().document.layers);
                let mut form = LayerForm::default();
                form.set_thumbnail_mode(ThumbnailMode::Synchronous);
                let mut input = egui::RawInput::default();
                input
                    .viewports
                    .get_mut(&egui::ViewportId::ROOT)
                    .unwrap()
                    .native_pixels_per_point = Some(s);
                let mut output = ctx.run_ui(input, |root_ui| {
                    let ctx = root_ui.ctx();
                    assert_eq!(ctx.pixels_per_point(), s);
                    let mut ui = Ui::new(
                        ctx.clone(),
                        egui::Id::unique("three-layer-grid"),
                        egui::UiBuilder::new().max_rect(Rect::from_min_size(
                            Pos2::new(1056.3, top),
                            Vec2::new(200.0, 250.0),
                        )),
                    );
                    let (list_rect, _) = split_strip(ui.max_rect());
                    let extent = dip_pt(LAYER_ROW, s);
                    let area = list_area(
                        &mut ui,
                        &mut form.list,
                        list_rect,
                        3.0 * extent,
                        "layer-list",
                    );
                    let slots = form.list.layout(&ui, area, &keys, extent, 1.0);
                    assert_eq!(slots.len(), 3);
                    assert_eq!(form.list.scroll, 0.0);
                    assert_eq!(form.list.dragging(), None);
                    // The absolute anchor is the supplied content origin, before row offsets.
                    assert!(((area.min.y - top) * s).abs() < 0.001);
                    assert!(((slots[0].min.y - top) * s).abs() <= 2.01);
                    for (i, slot) in slots.iter().enumerate() {
                        let want = top * s + i as f32 * pitch;
                        assert!((slot.min.y * s - want).abs() < 0.001, "@{s} row {i}");
                        assert!(((slot.min.x - 1056.3) * s).abs() < 0.001);
                        assert!((slot.height() * s - pitch).abs() < 0.001);
                    }
                    for adjacent in slots.windows(2) {
                        let actual = (adjacent[1].min.y - adjacent[0].min.y) * s;
                        assert!((actual - pitch).abs() < 0.001, "@{s}: pitch {actual}");
                    }
                    form.show(
                        &mut ui,
                        &mut app,
                        DisplayPrefs {
                            checker_brightness: 0.75,
                            mode: RenderMode::Hardware,
                            dark: true,
                        },
                        false,
                        1.0,
                        &|_| None,
                    );
                    // Observe the actual show path too: a wrong extent there must fail here.
                    for (i, key) in keys.iter().enumerate() {
                        let rect = ctx
                            .read_response(ui.scope_id().with(("layer", key)))
                            .expect("rendered layer row")
                            .rect;
                        let want = (top * s + i as f32 * pitch + s).round();
                        assert_eq!((rect.min.y * s).round(), want, "@{s} row {i}");
                    }
                });
                output.textures_delta.clear();
            }
        }
    }

    /// ui-text-and-glyphs.md §5 Task 19: History rows rnd(20·s) → 20, 25, 30, 40, the pitch the
    /// History list lays its rows out at.
    #[test]
    fn history_row_heights_scale_per_dip() {
        assert_eq!(
            [1.0, 1.25, 1.5, 2.0].map(|s| (dip_pt(HISTORY_ROW, s) * s).round()),
            [20.0, 25.0, 30.0, 40.0]
        );
    }
}
