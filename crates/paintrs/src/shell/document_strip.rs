//! The document strip: a thumbnail item per open document (dirty star, close button), the scroll
//! buttons, the document list button and its popup, and the document context menu's entries.
//! Port of: js/src/interface/panel/header/documents/{DocumentStrip,DocumentItem,
//! DocumentContextMenu}.js, js/src/interface/item/layout/ScrollList.js (horizontal list, drag
//! reorder, wheel), css/header.css (`#documentStrip`, `.document-strip-button`,
//! `.document-list-popup`, `--document-item-size`, the `.scroll-item .thumbnail` rules),
//! css/component.css (`.scroll-list`, `.selected-item`), js/src/engine/util/ImageUtil.js
//! (`createDirtyStar`)
//! Reference: paintdotjs @ b4e6fc5
//! The strip's bitmaps are drawn from the local icon pack when it has them.
//!
//! The look is paintdotjs's (rule 3: the strip maps 1:1); the thumbnails' pixels are Paint.NET's
//! (`pdn_core::render::thumbnail`), recorded as pdn rects. Not built (Ruling (P2) 6): the 0.5-s
//! thumbnail fade-in.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use egui::{Color32, ColorImage, Id, Pos2, Rect, Sense, TextureHandle, TextureOptions, Ui, Vec2};
use pdn_core::app::AppWorkspace;
use pdn_core::document::workspace::DocumentWorkspace;
use pdn_core::render::display::RenderMode;
use pdn_core::render::thumbnail::{ThumbFrame, render_framed, ring_extent};
use pdn_core::settings::AppSettings;

use crate::assets::Assets;
use crate::kit::glyph::dip_px;
use crate::kit::img;
use crate::kit::menu::MenuEntry;
use crate::kit::px;
use crate::kit::scroll::ScrollList;
use crate::kit::shadow::{self, BoxShadow};
use crate::kit::text::{self, FontSpec, record_pdn_rect};
use crate::kit::thumbnails::{self, Done, Scaler};
use crate::theme::*;

pub use crate::kit::thumbnails::{THUMBNAIL_LATENCY_S, ThumbnailMode};

use super::canvas_view::DisplayPrefs;

/// DocumentStrip's ScrollList: the wheel scrolls 0.75 × its delta.
const WHEEL_SPEED: f32 = 0.75;
/// `css/header.css .window-close-button { transition: opacity 0.2s }`.
const CLOSE_FADE_S: f32 = 0.2;
/// DocumentStrip.createNavigationButton: a held scroll button repeats after 400 ms, every 65 ms.
const NAV_REPEAT_S: (f64, f64) = (0.4, 0.065);

/// What the user asked of the strip this frame (the shell applies it).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum StripAction {
    /// `ScrollList.setSelected` → `app.setActiveDocumentWorkspace`.
    Activate(usize),
    /// The item's close button (`DocumentIO.closeDocumentWorkspace`).
    Close(usize),
    /// The drag swapped two items (`ScrollList.itemSwapper`).
    Reorder { from: usize, to: usize },
    /// A right click on an item at a pointer position (`DocumentContextMenu.show`).
    ContextMenu(usize, Pos2),
}

/// Frame-local strip ownership, captured before either canvas keys or later widgets run.
/// Hit-test the displayed, clipped widget rectangles, including close buttons and overlays.
pub(super) struct CaptureSnapshot {
    captured: bool,
    presses: Vec<(Pos2, bool)>,
}
impl CaptureSnapshot {
    pub fn at(&self, events: &[egui::Event], index: usize) -> bool {
        let mut captured = self.captured;
        for event in &events[..index] {
            match event {
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    if !pressed {
                        captured = false;
                    } else if !captured {
                        captured = self.presses.iter().any(|(p, item)| p == pos && *item);
                    }
                }
                egui::Event::PointerGone | egui::Event::WindowFocused(false) => captured = false,
                _ => {}
            }
        }
        captured
    }
}

pub struct DocumentStrip {
    /// Per `Document::id`.
    thumbs: HashMap<u64, Thumb>,
    /// `#documentsList` (its scroll position, wheel, drag and slides), keyed by document id.
    list: ScrollList,
    /// `documentListOpen`.
    pub list_open: bool,
    /// A held scroll button: (direction, time of its next repeat).
    repeat: Option<(f32, f64)>,
    /// The document `updateActiveDocument` last scrolled to.
    shown_active: Option<u64>,
    scaler: Scaler,
    selection_scroll: Option<(f32, f32, f64)>,
    /// Each tab shown last frame: its document id and rectangle (a live session's dots for who is on it).
    pub tabs: Vec<(u64, Rect)>,
}

impl Default for DocumentStrip {
    fn default() -> Self {
        Self {
            thumbs: HashMap::new(),
            list: ScrollList::horizontal(WHEEL_SPEED),
            list_open: false,
            repeat: None,
            shown_active: None,
            scaler: Scaler::default(),
            selection_scroll: None,
            tabs: Vec::new(),
        }
    }
}

#[derive(Default)]
struct Thumb {
    /// `thumbnail_due`: the last revision seen and when it changed (s).
    seen: Option<(u64, f64)>,
    /// The scaled composite: (revision, device max edge) it was made at, RGBA8, size.
    scaled: Option<Scaled>,
    /// The framed strip and list images: what they were made from, texture, device size.
    strip: Option<Framed>,
    list: Option<Framed>,
}

/// A scaled composite: the (revision, device max edge) it was made at, RGBA8, size (0 × 0 when
/// it cannot be scaled, so it is not retried every frame).
type Scaled = ((u64, u32), Vec<u8>, [u32; 2]);

type Framed = (((u64, u32), ThumbFrame), TextureHandle, [u32; 2]);

/// How an item's frame is drawn (css/component.css `.selected-item`, css/menu.css
/// `.clickable:hover.menu-item`).
#[derive(Clone, Copy, PartialEq)]
enum ItemState {
    Normal,
    Hover,
    Selected,
}

/// The header colour under device row `y` (points): the title bar, its menu-coloured bottom
/// border row and the header below it (css/header.css `#titleBar`, css/layout.css `header`).
fn band(theme: &Theme, y: f32, ctx: &egui::Context) -> Color32 {
    let top = if ctx.data_mut(|d| {
        d.get_temp::<bool>(Id::unique("desktop-shell"))
            .unwrap_or(false)
    }) {
        DESKTOP_TITLE_HEIGHT
    } else {
        0.0
    };
    if y < top + TITLE_BAR_BORDER + TITLE_BAR_HEIGHT {
        theme.title_bar
    } else {
        theme.menu_background
    }
}

/// What an item's content sits on at row `y`: its fill over the header.
fn item_backdrop(
    theme: &Theme,
    state: ItemState,
    y: f32,
    mode: RenderMode,
    ctx: &egui::Context,
) -> Color32 {
    match state {
        ItemState::Normal => band(theme, y, ctx),
        ItemState::Hover => theme.menu_item_hover,
        ItemState::Selected => {
            crate::kit::color::display_over(band(theme, y, ctx), theme.menu_item_selected, mode)
        }
    }
}

/// `ImageUtil.createDirtyStar(10)` as its data URL decodes (unpremultiplied), drawn by the
/// Canvas 2D shim.
fn dirty_star() -> &'static Arc<image::RgbaImage> {
    static STAR: LazyLock<Arc<image::RgbaImage>> = LazyLock::new(|| {
        use pdn_core::canvas::{Canvas, FillRule, Style};
        let size = DOCUMENT_DIRTY_SIZE as f64;
        let n = size as u32;
        let s = size / 32.0;
        let (r, c) = (size / 2.0, size / 2.0);
        let mut cx = Canvas::new(n, n);
        cx.save();
        cx.set_image_smoothing(true);
        for (width, color) in [(8.0 * s, DIRTY_STAR_OUTLINE), (3.0 * s, DIRTY_STAR_COLOR)] {
            cx.set_line_width(width);
            cx.set_stroke_style(Style::Color(color));
            for i in 0..8 {
                let rotation = i as f64 * 2.0 * std::f64::consts::PI / 8.0;
                cx.begin_path();
                cx.move_to(c, c);
                cx.line_to(c + r * rotation.sin(), c + r * rotation.cos());
                cx.stroke();
            }
        }
        cx.restore();
        cx.set_fill_style(Style::Color(DIRTY_STAR_OUTLINE));
        cx.begin_path();
        cx.arc(c, c, 3.0 * s, 0.0, 2.0 * std::f64::consts::PI, false);
        cx.fill(FillRule::NonZero);
        let data = cx.get_image_data(0, 0, n, n);
        Arc::new(image::RgbaImage::from_raw(n, n, data.data).expect("star size"))
    });
    &STAR
}

/// `DocumentContextMenu.show`'s entries for document `i` (paintdotjs's, Ruling (P2) 8).
/// Open Containing Folder needs `window.desktopFileActions`, which only the desktop build has
/// (Task 56).
pub fn context_menu_entries(app: &AppWorkspace, i: usize) -> Vec<MenuEntry> {
    let t = pdn_core::i18n::t;
    let Some(ws) = app.documents.get(i) else {
        return vec![];
    };
    let has_path = ws.file_path.is_some();
    let command = |id, key: &str, icon, enabled| MenuEntry::Command {
        id,
        label: t(key),
        icon,
        enabled,
    };
    vec![
        MenuEntry::Title(ws.get_friendly_name()),
        MenuEntry::Separator,
        command(
            "menu.tab.copyPath",
            "menu.tab.copyPath.text",
            "menu_tab_copy_path_icon.png",
            has_path,
        ),
        command(
            "menu.tab.openContainingFolder",
            "menu.tab.openContainingFolder.text",
            "menu_file_open_icon.png",
            false,
        ),
        MenuEntry::Separator,
        command(
            "menu.file.save",
            "menu.file.save.text",
            "menu_file_save_icon.png",
            true,
        ),
        command(
            "menu.file.saveAs",
            "menu.file.saveAs.text",
            "menu_file_save_as_icon.png",
            true,
        ),
        MenuEntry::Separator,
        command(
            "menu.file.close",
            "menu.file.close.text",
            "menu_file_close_icon.png",
            true,
        ),
    ]
}

/// The paintdotjs thumbnail `<canvas>` backing size (DocumentItem.renderThumbnail: fit in 96,
/// `Math.round`, at least 1).
fn css_canvas_size(w: u32, h: u32) -> (f32, f32) {
    let scale = (96.0 / w as f64).min(96.0 / h as f64);
    let r = |v: u32| pdn_core::js::math_round(v as f64 * scale).max(1.0) as f32;
    (r(w), r(h))
}

/// The thumbnails' max edge in device px for the strip `rect` (its height is the item):
/// Paint.NET's preferred image width − 2, the snapped item less rnd(4s) a side less 2 px
/// (`ui-text-and-glyphs.md §8.5`, correcting §7.4; `ui-drawn-graphics.md §2.8` Size), replacing
/// paintdotjs's item − 12 (Ruling (P2) 6). The inset box (item − 2·rnd(4s)) stays the CSS box.
pub fn thumb_max_edge(rect: Rect, ppp: f32) -> u32 {
    let item = px::snap_rect(rect, ppp).height() * ppp;
    (item.round() as i32 - 2 * dip_px(DOCUMENT_THUMB_INSET, ppp) - 2).max(1) as u32
}

impl DocumentStrip {
    /// `ui-drawn-graphics.md §2.8` Scheduling: true once 200 ms have passed since document
    /// `doc_id`'s last revision change (a new change restarts the wait); its first thumbnail is
    /// due at once.
    pub fn thumbnail_due(&mut self, doc_id: u64, revision: u64, now_s: f64) -> bool {
        let t = self.thumbs.entry(doc_id).or_default();
        thumbnails::due(&mut t.seen, revision, now_s)
    }

    /// Where thumbnails are scaled (the app's default: `ThumbnailMode::default()`).
    pub fn set_thumbnail_mode(&mut self, mode: ThumbnailMode) {
        self.scaler.set_mode(mode);
    }

    /// Re-scales `ws`'s composite when it is due (or the device max edge changed), one document at
    /// a time; the stale thumbnail shows meanwhile. Returns the seconds until a pending one is due.
    fn update_scaled(
        &mut self,
        ctx: &egui::Context,
        ws: &DocumentWorkspace,
        max_edge: u32,
        now: f64,
    ) -> Option<f64> {
        // A drag's pixels are lifted out of the layer meanwhile (`render::float`).
        if ws.float.is_some() {
            return None;
        }
        let (id, revision) = (ws.document.id, ws.view.composition_revision);
        let due = self.thumbnail_due(id, revision, now);
        let t = self.thumbs.get_mut(&id).expect("thumbnail_due inserts");
        let want = (revision, max_edge);
        let have = t.scaled.as_ref().map(|s| s.0);
        if have == Some(want) {
            return None;
        }
        if !due && have.is_some_and(|(_, m)| m == max_edge) {
            let changed_at = t.seen.map_or(now, |s| s.1);
            return Some(changed_at + THUMBNAIL_LATENCY_S - now);
        }
        if self.scaler.busy() {
            // One job at a time; its result asks for the next frame.
            return None;
        }
        let c = &ws.view.composition;
        let size = [c.width(), c.height()];
        if let Some(done) = self.scaler.start(ctx, id, want, c.data(), size) {
            self.accept(done, revision, max_edge);
        }
        None
    }

    pub(super) fn capture_snapshot(&self, ui: &Ui, app: &AppWorkspace) -> CaptureSnapshot {
        let items: Vec<_> = app
            .documents
            .iter()
            .map(|w| ui.scope_id().with(("docstrip-item", w.document.id)))
            .collect();
        let positions = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        ..
                    } => Some(*pos),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        let presses = positions
            .into_iter()
            .map(|pos| {
                // Shell widgets share a layer, in paint/hit order. A later close button, menu or
                // palette owns its press rather than starting the underlying ScrollList capture.
                let item = ui.ctx().layer_id_at(pos) == Some(ui.layer_id())
                    && ui.ctx().viewport(|v| {
                        v.prev_pass
                            .widgets
                            .get_layer(ui.layer_id())
                            .filter(|w| {
                                w.enabled && w.sense.senses_click() && w.interact_rect.contains(pos)
                            })
                            .last()
                            .is_some_and(|w| items.contains(&w.id))
                    });
                (pos, item)
            })
            .collect();
        CaptureSnapshot {
            captured: self.list.captured(),
            presses,
        }
    }

    /// Delivered form commands, painting-tools-1 §9. Exact modifiers; egui does not retain
    /// keypad-vs-top-row digit identity on either supported backend (including physical_key).
    pub(crate) fn navigate(
        &mut self,
        app: &mut AppWorkspace,
        key: &str,
        m: egui::Modifiers,
        captured: bool,
    ) -> bool {
        if m.mac_cmd || (m.ctrl && m.alt) {
            return false;
        }
        let n = app.documents.len();
        let selected = app.active.filter(|&i| i < n);
        let target = if !m.shift && (m.ctrl != m.alt) && key.len() == 1 {
            key.as_bytes()
                .first()
                .filter(|&&b| (b'1'..=b'9').contains(&b))
                .map(|b| (b - b'1') as usize)
                .filter(|&i| i < n)
        } else if m.ctrl && !m.alt {
            match (key, m.shift, selected) {
                ("PageDown" | "PageUp", true, Some(i)) => {
                    if captured {
                        return false;
                    }
                    let next = if key == "PageDown" {
                        i.checked_add(1).filter(|&j| j < n)
                    } else {
                        i.checked_sub(1)
                    };
                    if let Some(to) = next {
                        app.documents.swap(i, to);
                        app.active = Some(to);
                        self.shown_active = None;
                    }
                    return true;
                }
                ("Tab" | "PageDown", false, Some(i)) if app.mouse_buttons == 0 => Some((i + 1) % n),
                ("Tab", true, Some(i)) | ("PageUp", false, Some(i)) if app.mouse_buttons == 0 => {
                    Some((i + n - 1) % n)
                }
                _ => None,
            }
        } else {
            None
        };
        let Some(target) = target else {
            return false;
        };
        if selected != Some(target) {
            app.set_active_document_workspace(Some(target));
        }
        true
    }

    /// Keeps a scaled thumbnail made at document `doc`'s current `revision` and `max_edge`; one
    /// for a closed document, an older revision or another scale is dropped.
    fn accept(&mut self, (doc, key, (px, w, h)): Done, revision: u64, max_edge: u32) {
        if key == (revision, max_edge)
            && let Some(t) = self.thumbs.get_mut(&doc)
        {
            t.scaled = Some((key, px, [w, h]));
        }
    }

    /// The framed texture of document `id` for the strip (`list` false) or the list popup.
    fn framed(
        &mut self,
        ctx: &egui::Context,
        id: u64,
        list: bool,
        f: ThumbFrame,
    ) -> Option<(TextureHandle, [u32; 2])> {
        let t = self.thumbs.get_mut(&id)?;
        let (key, px, [w, h]) = t.scaled.as_ref().filter(|s| s.2[0] > 0)?;
        let slot = if list { &mut t.list } else { &mut t.strip };
        let want = (*key, f);
        if slot.as_ref().map(|s| &s.0) != Some(&want) {
            let (rgba, fw, fh) = render_framed(px, *w, *h, &f);
            let image = ColorImage::from_rgba_unmultiplied([fw as usize, fh as usize], &rgba);
            let tex = ctx.load_texture(
                format!("docstrip/{id}/{list}"),
                image,
                TextureOptions::NEAREST,
            );
            *slot = Some((want, tex, [*w, *h]));
        }
        slot.as_ref().map(|(_, tex, size)| (tex.clone(), *size))
    }

    /// Shows the strip in `rect` (the `#documentStrip` box) and, when open, the document list
    /// popup under it. `settings` supplies the thumbnails' checkerboard brightness and display
    /// mode (`canvas.checkerboardBrightness`, `graphics.hardwareAcceleration`).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        app: &mut AppWorkspace,
        theme: &Theme,
        _assets: &Assets,
        settings: &AppSettings,
    ) -> Option<StripAction> {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let now = ui.input(|i| i.time);
        let ids: Vec<u64> = app.documents.iter().map(|ws| ws.document.id).collect();
        self.thumbs.retain(|id, _| ids.contains(id));
        // The scaler releases what closed documents held (an in-flight result, a sliced job, the
        // snapshot buffer) even with no document left.
        let max_edge = thumb_max_edge(rect, ppp);
        // A sliced job reads its document's composite while that is open and unchanged.
        let source = |doc: u64, (revision, _): (u64, u32)| {
            app.documents
                .iter()
                .find(|ws| ws.document.id == doc && ws.view.composition_revision == revision)
                .map(|ws| ws.view.composition.data())
        };
        if let Some(done) = self.scaler.poll(&ctx, source) {
            let revision = app
                .documents
                .iter()
                .find(|ws| ws.document.id == done.0)
                .map(|ws| ws.view.composition_revision);
            if let Some(revision) = revision {
                self.accept(done, revision, max_edge);
            }
        }
        let largest = app
            .documents
            .iter()
            .map(|ws| ws.view.composition.data().len());
        self.scaler.trim(largest.max().unwrap_or(0));
        if ids.is_empty() || rect.width() <= 0.0 {
            // updateNavigation: no documents → no list button, no list.
            self.list_open = false;
            self.tabs.clear();
            return None;
        }
        let item_h = rect.height();
        let item_w = item_h + 2.0 * DOCUMENT_ITEM_BORDER;
        let list = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.max.x - DOCUMENT_STRIP_LIST_BUTTON, rect.max.y),
        );
        let content = ids.len() as f32 * item_w;
        let max_scroll = (content - list.width()).max(0.0);
        let active = app.active;
        // painting-tools-1 §9.5: reveal the selected thumbnail at the nearest bound, over 0.35 s.
        let active_id = active.and_then(|i| ids.get(i).copied());
        if active_id != self.shown_active {
            self.shown_active = active_id;
            if let Some(i) = active {
                let left = i as f32 * item_w;
                let right = left + item_w;
                let target = if left < self.list.scroll {
                    left
                } else {
                    self.list.scroll.max(right - list.width())
                }
                .clamp(0., max_scroll);
                self.selection_scroll =
                    (target != self.list.scroll).then_some((self.list.scroll, target, now));
            }
        }
        if let Some((from, to, start)) = self.selection_scroll {
            let t = ((now - start) / 0.35).clamp(0., 1.) as f32;
            self.list.scroll = from + (to - from) * (1. - (1. - t).powi(3));
            if t < 1. {
                ctx.request_repaint();
            } else {
                self.selection_scroll = None;
            }
        }
        if ui.input(|i| i.smooth_scroll_delta != Vec2::ZERO)
            && list.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or(Pos2::ZERO))
        {
            self.selection_scroll = None;
        }
        self.list.wheel(ui, list, content, now, "docstrip-list");
        self.list.clamp(content, list.width());
        let pointer = ui.input(|i| i.pointer.hover_pos());

        let DisplayPrefs {
            checker_brightness,
            mode,
            dark,
        } = DisplayPrefs::new(settings, theme);
        let mut pending: Option<f64> = None;
        for ws in &app.documents {
            if let Some(wait) = self.update_scaled(&ctx, ws, max_edge, now) {
                pending = Some(pending.map_or(wait, |p| p.min(wait)));
            }
        }
        if let Some(wait) = pending {
            ctx.request_repaint_after_secs(wait.max(0.0) as f32);
        }

        // Item rects: layout x, the reorder slide, the drag translate.
        let dragging = self.list.dragging();
        let rects = self.list.layout(ui, list, &ids, item_w, now);
        self.tabs.clear();
        (self.tabs).extend(
            ids.iter()
                .zip(&rects)
                .map(|(id, r)| (*id, r.intersect(list))),
        );

        let mut action = None;
        // `#documentsList { overflow-x: auto }` clips the items.
        // ponytail: no `.document-overflow-left/right` 30-px fade masks; add them when the
        // multi-document scenarios (Task 44) show an overflowing strip.
        let painter = ui.painter().with_clip_rect(list.intersect(ui.clip_rect()));
        // The dragged item paints last (`z-index: 3`).
        let mut order: Vec<usize> = (0..ids.len()).collect();
        order.sort_by_key(|&i| dragging == Some(ids[i]));
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        for i in order {
            let id = ids[i];
            let snapped = px::snap_rect(rects[i], ppp);
            let item_id = ui.scope_id().with(("docstrip-item", id));
            let item = ui.interact(snapped.intersect(list), item_id, Sense::click());
            ctx.accesskit_node_builder(item.id, |node| {
                node.set_role(egui::accesskit::Role::Tab);
                node.set_label(app.documents[i].get_friendly_name());
                node.set_selected(active == Some(i));
            });
            let pad = rects[i].shrink(DOCUMENT_ITEM_BORDER);
            let (header_h, header_pad) = DOCUMENT_HEADER;
            let close_rect = Rect::from_min_size(
                Pos2::new(
                    pad.max.x - header_pad - DOCUMENT_CLOSE_SIZE,
                    pad.min.y + header_pad + (header_h - DOCUMENT_CLOSE_SIZE) / 2.0,
                ),
                Vec2::splat(DOCUMENT_CLOSE_SIZE),
            );
            let close_snapped = px::snap_rect(close_rect, ppp);
            let close = ui.interact(
                close_snapped.intersect(list),
                item_id.with("close"),
                Sense::click(),
            );
            // `:hover`: the top-most widget under the pointer (a popup over the item takes it).
            let hovered = item.hovered() || close.hovered();
            let state = if active == Some(i) {
                ItemState::Selected
            } else if hovered || dragging == Some(id) {
                ItemState::Hover
            } else {
                ItemState::Normal
            };
            paint_item_frame(&painter, snapped, state, theme, mode);
            // Thumbnail (z-index 0), then the header (z-index 1).
            let ws = &app.documents[i];
            let frame = ThumbFrame {
                u: ppp as f64,
                checker_brightness,
                dark,
                mode,
                checker: true,
                // One colour for the ring's surroundings: the item over the header at the
                // thumbnail's bottom edge. ponytail: the strip also overlaps the title bar and the
                // tool strip there; per-row backdrops would follow them exactly.
                backdrop: {
                    let c =
                        item_backdrop(theme, state, pad.max.y - DOCUMENT_THUMB_BOTTOM, mode, &ctx);
                    [c.r(), c.g(), c.b(), 255]
                },
            };
            let mut thumb_rect = None;
            if let Some((tex, [tw, th])) = self.framed(&ctx, id, false, frame) {
                let e = ring_extent(ppp as f64) as f32;
                let left = (pad.center().x * ppp - tw as f32 / 2.0).round();
                let bottom = ((pad.max.y - DOCUMENT_THUMB_BOTTOM) * ppp).round();
                let img_rect = Rect::from_min_size(
                    Pos2::new((left - e) / ppp, (bottom - th as f32 - e) / ppp),
                    Vec2::new((tw as f32 + 2.0 * e) / ppp, (th as f32 + 2.0 * e) / ppp),
                );
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                painter.image(tex.id(), img_rect, uv, Color32::WHITE);
                // paintdotjs's thumbnail box (`.thumbnail.landscape/.portrait`): its pixels are the
                // same picture, so both boxes are Paint.NET's (R21 (c)). The framed image paint.rs
                // draws is opaque and lies over the tool strip: what is recorded under it is hidden.
                let c = &ws.view.composition;
                let (cw, ch) = css_canvas_size(c.width(), c.height());
                let room = pad.size() - Vec2::splat(2.0 * DOCUMENT_THUMB_INSET);
                let size = if cw >= ch {
                    Vec2::new(room.x, (room.x * ch / cw).min(room.y))
                } else {
                    Vec2::new((room.y * cw / ch).min(room.x), room.y)
                };
                let css_box = Rect::from_center_size(
                    Pos2::new(
                        pad.center().x,
                        pad.max.y - DOCUMENT_THUMB_BOTTOM - size.y / 2.0,
                    ),
                    size,
                );
                text::occlude(&ctx, img_rect.intersect(list));
                thumb_rect = Some(img_rect.union(css_box).intersect(list));
                record_pdn_rect(&ctx, img_rect.union(css_box).intersect(list));
            }
            // `.dirty-indicator { visibility }`: paintdotjs's star.
            if ws.dirty {
                let star = Rect::from_min_size(
                    Pos2::new(
                        pad.min.x + header_pad,
                        pad.min.y + header_pad + (header_h - DOCUMENT_DIRTY_SIZE) / 2.0,
                    ),
                    Vec2::splat(DOCUMENT_DIRTY_SIZE),
                );
                let star = px::snap_rect(star, ppp);
                let backdrop = item_backdrop(theme, state, star.center().y, mode, &ctx);
                img::paint_generated(
                    &painter,
                    "docstrip-dirty-star",
                    dirty_star(),
                    star,
                    backdrop,
                );
            }
            let opacity =
                ctx.animate_bool_with_time(item_id.with("close-fade"), hovered, CLOSE_FADE_S);
            if opacity > 0.0 {
                // The button (z-index 1) lies over the thumbnail's top-right corner in the
                // 54-DIP item (§7.4): opaque, it hides the thumbnail pixels under it; fading, it
                // blends over them (Paint.NET's pixels, its "x" too).
                let over_thumb = thumb_rect.is_some_and(|t| t.intersects(close_snapped));
                if over_thumb && opacity >= 1.0 {
                    text::occlude(&ctx, close_snapped);
                }
                let clipped = painter.with_clip_rect(close_snapped.intersect(painter.clip_rect()));
                paint_close_button(&clipped, close_snapped, &close, opacity, theme);
                if over_thumb && opacity < 1.0 {
                    text::record_pdn_rect_over(&ctx, close_snapped);
                }
            }
            if state == ItemState::Selected {
                text::record_pdn_rect_over(&ctx, snapped.intersect(list));
            }
            // Input: the close button (`stopPropagation`), else a press selects and may drag.
            if close.clicked() {
                action = action.or(Some(StripAction::Close(i)));
            } else if pressed && item.hovered() {
                self.list
                    .press(id, pointer.unwrap_or(rects[i].min), rects[i]);
                if active != Some(i) {
                    action = action.or(Some(StripAction::Activate(i)));
                }
            }
            // The item (or its close button) the right click went up on, at that release.
            if let Some(r) = [&item, &close]
                .into_iter()
                .find(|r| r.clicked_by(egui::PointerButton::Secondary))
            {
                let at = crate::kit::inputs::interact_pos(ui, r).unwrap_or(rects[i].center());
                action = action.or(Some(StripAction::ContextMenu(i, at)));
            }
        }
        if let Some((from, to)) = self.list.continue_drag(ui, &ids, &rects, list) {
            action = action.or(Some(StripAction::Reorder { from, to }));
        }
        self.list.cover(ui, list, "docstrip-list");

        self.nav_buttons(ui, rect, list, max_scroll, now, theme);
        let list_button = Rect::from_min_max(
            Pos2::new(rect.max.x - DOCUMENT_STRIP_LIST_BUTTON, rect.min.y),
            rect.max,
        );
        let toggled = nav_button(
            ui,
            "docstrip-list",
            list_button,
            "tool_bar_image_list_menu_open_button.png",
            DOCUMENT_LIST_IMAGE,
            self.list_open,
            theme,
        )
        .clicked();
        if toggled {
            self.list_open = !self.list_open;
        }
        if self.list_open
            && let Some(a) = self.show_list(ui, rect, app, theme, mode, dark)
        {
            action = action.or(Some(a));
        }
        action
    }
}

/// `.menu-item` frame states of an item's 74×72 border box (`rect`, snapped).
/// ponytail: a dragged item draws as hovered, without `.dragging-item`'s box shadow and 0.92
/// opacity; add them if the drag look matters.
fn paint_item_frame(
    painter: &egui::Painter,
    rect: Rect,
    state: ItemState,
    theme: &Theme,
    mode: RenderMode,
) {
    let ppp = painter.pixels_per_point();
    let (border, fill) = match state {
        ItemState::Normal => return,
        ItemState::Hover => (theme.menu_item_border, theme.menu_item_hover),
        // Translucent over whatever lies below (the title bar, header and tool strip).
        ItemState::Selected => (theme.menu_item_selected_border, theme.menu_item_selected),
    };
    let bw = px::border_width(DOCUMENT_ITEM_BORDER, ppp);
    let ring = px::border_width(MENU_ITEM_RING, ppp);
    let inner = rect.shrink(bw);
    for side in frame_sides(rect, bw) {
        painter.rect_filled(side, 0.0, border);
    }
    // `.selected-item:before` / `.clickable:hover:before`: a menu-coloured ring inside.
    for side in frame_sides(inner, ring) {
        painter.rect_filled(side, 0.0, theme.menu_background);
    }
    let area = inner.shrink(ring);
    if state == ItemState::Selected {
        let height = 1. / ppp;
        for row in 0..(area.height() * ppp).round() as usize {
            let y = area.min.y + row as f32 / ppp;
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(area.min.x, y), Vec2::new(area.width(), height)),
                0.,
                item_backdrop(theme, state, y + height / 2., mode, painter.ctx()),
            );
        }
    } else {
        painter.rect_filled(area, 0.0, fill);
    }
}

impl DocumentStrip {
    /// The previous/next scroll buttons, shown while the list can scroll that way
    /// (updateNavigation); a press scrolls one item and repeats while held.
    fn nav_buttons(
        &mut self,
        ui: &mut Ui,
        strip: Rect,
        list: Rect,
        max_scroll: f32,
        now: f64,
        theme: &Theme,
    ) {
        let w = DOCUMENT_NAV_BUTTON;
        let buttons = [
            (
                -1.0,
                self.list.scroll > 1.0,
                list.min.x,
                "image_strip_scroll_left_arrow.png",
            ),
            (
                1.0,
                max_scroll > 1.0 && self.list.scroll < max_scroll - 1.0,
                list.max.x - w,
                "image_strip_scroll_right_arrow.png",
            ),
        ];
        let mut held = None;
        for (dir, shown, x, image) in buttons {
            if !shown {
                continue;
            }
            let r = Rect::from_min_size(Pos2::new(x, strip.min.y), Vec2::new(w, strip.height()));
            let response = nav_button(ui, image, r, image, DOCUMENT_NAV_IMAGE, false, theme);
            if response.is_pointer_button_down_on() {
                held = Some(dir);
            }
        }
        let item_w = strip.height() + 2.0 * DOCUMENT_ITEM_BORDER;
        match (held, self.repeat) {
            (Some(dir), None) => {
                self.list.scroll += dir * item_w;
                self.repeat = Some((dir, now + NAV_REPEAT_S.0));
            }
            (Some(dir), Some((_, next))) if now >= next => {
                self.list.scroll += dir * item_w;
                self.repeat = Some((dir, next + NAV_REPEAT_S.1));
            }
            (None, _) => self.repeat = None,
            _ => {}
        }
        if let Some((_, next)) = self.repeat {
            ui.ctx()
                .request_repaint_after_secs((next - now).max(0.0) as f32);
        }
        self.list.scroll = self.list.scroll.clamp(0.0, max_scroll);
    }

    /// The `.document-list-popup` under the strip's right edge: an entry per document with its
    /// thumbnail (Paint.NET's, no checkerboard, over the content background, §2.8 Display step 6)
    /// and name.
    fn show_list(
        &mut self,
        ui: &mut Ui,
        strip: Rect,
        app: &AppWorkspace,
        theme: &Theme,
        mode: RenderMode,
        dark: bool,
    ) -> Option<StripAction> {
        let ctx = ui.ctx().clone();
        let ppp = ctx.pixels_per_point();
        let spec = FontSpec::new(DOCUMENT_LIST_FONT);
        let names: Vec<String> = app
            .documents
            .iter()
            .map(|w| w.get_friendly_name())
            .collect();
        let longest = names
            .iter()
            .map(|n| text::css_width(n, DOCUMENT_LIST_FONT))
            .fold(0.0, f32::max);
        let (min_w, max_w, extra) = DOCUMENT_LIST_WIDTH;
        let width = (extra + longest).ceil().clamp(min_w, max_w);
        let chrome = 2.0 * (DROP_MENU_BORDER + DOCUMENT_LIST_PADDING);
        // `max-height: calc(100vh - 76px)`. ponytail: no `overflow-y: auto` scrolling; entries
        // past the height are clipped (about eight documents at 800 px).
        let height = (chrome + names.len() as f32 * DOCUMENT_LIST_ENTRY_HEIGHT)
            .min(ctx.content_rect().height() - 76.0);
        let popup = Rect::from_min_size(
            Pos2::new(strip.max.x - width, strip.max.y - 1.0),
            Vec2::new(width, height),
        );
        let snapped = px::snap_rect(popup, ppp);
        let (dy, blur) = DOCUMENT_LIST_SHADOW;
        let pixels = |id| img::texture_pixels(&ctx, id);
        shadow::paint_box_shadow(
            ui,
            Id::unique("docstrip-list-shadow"),
            snapped,
            BoxShadow {
                offset: Vec2::new(0.0, dy),
                blur,
                color: DOCUMENT_LIST_SHADOW_COLOR,
            },
            &pixels,
        );
        text::occlude(&ctx, snapped);
        ui.interact(
            snapped,
            ui.scope_id().with("docstrip-list-popup"),
            Sense::click(),
        );
        let painter = ui
            .painter()
            .with_clip_rect(snapped.intersect(ui.clip_rect()));
        let bw = px::border_width(DROP_MENU_BORDER, ppp);
        painter.rect_filled(snapped, 0.0, theme.menu_item_selected_border);
        painter.rect_filled(snapped.shrink(bw), 0.0, theme.menu_background);

        let content_back = if dark {
            CONTENT_BACK_DARK
        } else {
            CONTENT_BACK_LIGHT
        };
        let inner = popup.shrink(DROP_MENU_BORDER + DOCUMENT_LIST_PADDING);
        let mut action = None;
        for (i, name) in names.iter().enumerate() {
            let entry = Rect::from_min_size(
                Pos2::new(
                    inner.min.x,
                    inner.min.y + i as f32 * DOCUMENT_LIST_ENTRY_HEIGHT,
                ),
                Vec2::new(inner.width(), DOCUMENT_LIST_ENTRY_HEIGHT),
            );
            let entry_snapped = px::snap_rect(entry, ppp);
            let response = ui.interact(
                entry_snapped,
                ui.scope_id().with(("docstrip-list-entry", i)),
                Sense::click(),
            );
            let selected = app.active == Some(i);
            let (border, fill) = if selected {
                (
                    theme.menu_item_selected_border,
                    px::over(theme.menu_background, theme.menu_item_selected),
                )
            } else if response.hovered() {
                (theme.menu_item_border, theme.menu_item_hover)
            } else {
                (Color32::TRANSPARENT, theme.menu_background)
            };
            if border != Color32::TRANSPARENT {
                let bw = px::border_width(MENU_ITEM_BORDER, ppp);
                painter.rect_filled(entry_snapped, 0.0, border);
                painter.rect_filled(entry_snapped.shrink(bw), 0.0, fill);
            }
            let (pad_y, _, pad_left) = DOCUMENT_LIST_ENTRY_PADDING;
            let content = Rect::from_min_max(
                entry.min + Vec2::new(MENU_ITEM_BORDER + pad_left, MENU_ITEM_BORDER + pad_y),
                entry.max
                    - Vec2::new(
                        MENU_ITEM_BORDER + DOCUMENT_LIST_ENTRY_PADDING.1,
                        MENU_ITEM_BORDER + pad_y,
                    ),
            );
            let preview = Rect::from_min_size(content.min, DOCUMENT_LIST_PREVIEW.into());
            let ws = &app.documents[i];
            let frame = ThumbFrame {
                u: ppp as f64,
                checker_brightness: 1.0,
                dark,
                mode,
                checker: false,
                backdrop: [content_back.r(), content_back.g(), content_back.b(), 255],
            };
            if let Some((tex, [tw, th])) = self.framed(&ctx, ws.document.id, true, frame) {
                let e = ring_extent(ppp as f64) as f32;
                let left = (preview.center().x * ppp - tw as f32 / 2.0).round();
                let top = (preview.center().y * ppp - th as f32 / 2.0).round();
                let img_rect = Rect::from_min_size(
                    Pos2::new((left - e) / ppp, (top - e) / ppp),
                    Vec2::new((tw as f32 + 2.0 * e) / ppp, (th as f32 + 2.0 * e) / ppp),
                );
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                painter.image(tex.id(), img_rect, uv, Color32::WHITE);
                // copyThumbnail's box (the strip canvas fitted in 88×64, centred) and its
                // `box-shadow: 0 2px 5px` (≈ 2.4σ reach): paintdotjs's frame of the same picture.
                let c = &ws.view.composition;
                let (cw, ch) = css_canvas_size(c.width(), c.height());
                let (mw, mh) = DOCUMENT_LIST_THUMB_MAX;
                let scale = (mw / cw).min(mh / ch);
                let size = Vec2::new((cw * scale).round(), (ch * scale).round());
                let css_box = Rect::from_center_size(preview.center(), size);
                let (shadow_dy, shadow_blur) = DOCUMENT_LIST_THUMB_SHADOW;
                let reach = css_box
                    .translate(Vec2::new(0.0, shadow_dy))
                    .expand(shadow_blur + 1.0);
                record_pdn_rect(
                    &ctx,
                    img_rect.union(css_box).union(reach).intersect(snapped),
                );
            }
            // `.document-list-name`: one line, ellipsis, centred in the grid row.
            let line = spec.resolved_line_height(ppp);
            let name_box = Rect::from_min_max(
                Pos2::new(
                    preview.max.x + DOCUMENT_LIST_GAP,
                    content.center().y - line / 2.0,
                ),
                Pos2::new(content.max.x, content.center().y + line / 2.0),
            );
            text::draw_ellipsized_aligned(
                &painter,
                name_box,
                name,
                spec,
                theme.pdn_text,
                text::Alignment::Left,
            );
            if response.clicked() {
                // entry.onclick: activate, hide the list.
                self.list_open = false;
                action = Some(StripAction::Activate(i));
            }
        }
        // The document `pointerdown` (capture) and `keydown` listeners: a press outside the strip
        // element or Escape hides the list.
        let press = crate::kit::inputs::press_pos(ui.ctx(), None);
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if escape || press.is_some_and(|p| !strip.contains(p) && !snapped.contains(p)) {
            self.list_open = false;
        }
        action
    }
}

/// `rect`'s four border strips of width `w` (a frame without overdraw, for translucent fills).
fn frame_sides(rect: Rect, w: f32) -> [Rect; 4] {
    [
        Rect::from_min_max(rect.min, Pos2::new(rect.max.x, rect.min.y + w)),
        Rect::from_min_max(Pos2::new(rect.min.x, rect.max.y - w), rect.max),
        Rect::from_min_max(
            Pos2::new(rect.min.x, rect.min.y + w),
            Pos2::new(rect.min.x + w, rect.max.y - w),
        ),
        Rect::from_min_max(
            Pos2::new(rect.max.x - w, rect.min.y + w),
            Pos2::new(rect.max.x, rect.max.y - w),
        ),
    ]
}

/// A `.document-strip-button` (border box `rect`) with its `image` (`assets/images`) centred;
/// `pushed` = `.pushed`.
/// ponytail: the 140-ms `background-color`/`border-color` transition is not animated (static
/// captures equal its end state).
fn nav_button(
    ui: &mut Ui,
    id: &str,
    rect: Rect,
    image: &str,
    image_size: (f32, f32),
    pushed: bool,
    theme: &Theme,
) -> egui::Response {
    let ppp = ui.ctx().pixels_per_point();
    let snapped = px::snap_rect(rect, ppp);
    let response = ui.interact(snapped, ui.scope_id().with(id), Sense::click());
    let hover = Color32::from_rgba_unmultiplied(
        theme.menu_item_hover.r(),
        theme.menu_item_hover.g(),
        theme.menu_item_hover.b(),
        px::css_alpha(DOCUMENT_NAV_HOVER_ALPHA),
    );
    let style = if pushed || response.is_pointer_button_down_on() {
        Some((theme.menu_item_selected, theme.menu_item_selected_border))
    } else if response.hovered() {
        Some((hover, theme.menu_item_border))
    } else {
        None
    };
    let painter = ui.painter();
    if let Some((fill, border)) = style {
        let bw = px::border_width(DOCUMENT_ITEM_BORDER, ppp);
        for side in frame_sides(snapped, bw) {
            painter.rect_filled(side, 0.0, border);
        }
        painter.rect_filled(snapped.shrink(bw), 0.0, fill);
    }
    let icon = px::snap_rect(
        Rect::from_center_size(rect.center(), image_size.into()),
        ppp,
    );
    // The image's backdrop per device row: the button fill over the header bands.
    let (w, h) = (
        (icon.width() * ppp).round() as usize,
        (icon.height() * ppp).round() as usize,
    );
    let backdrop: Vec<Color32> = (0..h)
        .flat_map(|r| {
            let y = icon.min.y + (r as f32 + 0.5) / ppp;
            let base = band(theme, y, ui.ctx());
            let c = style.map_or(base, |(fill, _)| px::over(base, fill));
            std::iter::repeat_n(c, w)
        })
        .collect();
    img::paint_icon_over(painter, image, icon, &backdrop);
    response
}

/// The item's close button at `opacity`: paintdotjs's `.window-close-button` "x".
fn paint_close_button(
    painter: &egui::Painter,
    rect: Rect,
    response: &egui::Response,
    opacity: f32,
    theme: &Theme,
) {
    let bg = if response.hovered() {
        theme.close_button_hover
    } else {
        theme.close_button
    };
    painter.rect_filled(rect, 0.0, bg.gamma_multiply(opacity));
    // Centred; `padding-bottom: 1px` lifts the line box.
    // ponytail: Chromium synthesizes the bold; this draws the Regular face.
    let spec = crate::kit::fonts::face_spec(DOCUMENT_CLOSE_FAMILY, DOCUMENT_CLOSE_FONT);
    let w = text::measure(painter, "x", spec);
    let ppp = painter.pixels_per_point();
    let line = spec.resolved_line_height(ppp);
    // Blink centres the line box in device px (its layout runs at the device scale).
    let at = Pos2::new(
        rect.center().x - w / 2.0,
        rect.min.y + (((rect.height() - 1.0 - line) / 2.0) * ppp).ceil() / ppp,
    );
    text::draw(
        painter,
        at,
        "x",
        spec,
        theme.close_button_text.gamma_multiply(opacity),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ui-drawn-graphics.md §2.8 Scheduling.
    #[test]
    fn thumbnails_wait_200_ms_after_the_last_change() {
        let mut s = DocumentStrip::default();
        assert!(
            s.thumbnail_due(7, 3, 10.0),
            "the first thumbnail is due at once"
        );
        assert!(s.thumbnail_due(7, 3, 10.01));
        assert!(!s.thumbnail_due(7, 4, 11.0), "a change waits");
        assert!(!s.thumbnail_due(7, 4, 11.19));
        assert!(
            !s.thumbnail_due(7, 5, 11.15),
            "a new change restarts the wait"
        );
        assert!(!s.thumbnail_due(7, 5, 11.3));
        assert!(s.thumbnail_due(7, 5, 11.36));
        assert!(s.thumbnail_due(8, 1, 11.36), "per document");
    }

    /// Task 19 review (carry 3): with no document left, the scaler drops its job (sliced or on
    /// the worker) and releases its snapshot buffer.
    #[test]
    fn closing_the_last_document_releases_the_scaler() {
        for mode in [ThumbnailMode::Sliced, ThumbnailMode::Worker] {
            let ctx = egui::Context::default();
            let mut app = AppWorkspace::default();
            app.create_blank_document_in_new_workspace(2000, 1500, 96.0, true);
            let settings = AppSettings::load_with(None, true).0;
            let mut strip = DocumentStrip::default();
            strip.set_thumbnail_mode(mode);
            let rect = Rect::from_min_size(Pos2::new(387.0, 3.0), Vec2::new(93.0, 72.0));
            let frame = |app: &mut AppWorkspace, strip: &mut DocumentStrip| {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                    let theme = crate::theme::current(ui.ctx());
                    strip.show(ui, rect, app, &theme, &Assets, &settings);
                });
                out.textures_delta.clear();
            };
            frame(&mut app, &mut strip);
            assert!(strip.scaler.busy(), "{mode:?}: a job in flight");
            assert!(app.close_document_workspace(0));
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
            loop {
                frame(&mut app, &mut strip);
                if !strip.scaler.busy() && strip.scaler.spare_capacity() == 0 {
                    break;
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "{mode:?} kept its buffers"
                );
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }
    }

    /// Results for an older revision, another scale or a closed document are dropped.
    #[test]
    fn stale_results_are_discarded() {
        let mut s = DocumentStrip::default();
        assert!(s.thumbnail_due(7, 5, 0.0));
        let done = |key| (7, key, (vec![1, 2, 3, 4], 1, 1));
        s.accept(done((4, 60)), 5, 60);
        s.accept(done((5, 75)), 5, 60);
        s.accept((8, (5, 60), (vec![], 0, 0)), 5, 60);
        assert!(s.thumbs[&7].scaled.is_none() && !s.thumbs.contains_key(&8));
        s.accept(done((5, 60)), 5, 60);
        assert_eq!(s.thumbs[&7].scaled.as_ref().map(|t| t.0), Some((5, 60)));
    }

    /// DocumentItem.renderThumbnail's canvas: fit in 96, rounded, at least 1.
    #[test]
    fn css_canvas_fits_96() {
        assert_eq!(css_canvas_size(800, 600), (96.0, 72.0));
        assert_eq!(css_canvas_size(300, 1000), (29.0, 96.0));
        assert_eq!(css_canvas_size(1000, 1), (96.0, 1.0));
    }
}
