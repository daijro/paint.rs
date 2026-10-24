//! Ported from PhotoCraft crates/ui-egui/src/vp_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: one plane, made by four clicks in any order (`add_corner`, normalized as upstream) and edited by dragging its corner nodes; the plane drawn in upstream's validity colours with its grid (`grid_steps`, at the default 100-px Grid Size) and nodes, and the corners so far with a rubber band to the pointer; the clipboard image dragged along the plane (`Drag::Paste`) with its Width as an Effect Config range row; the paste previewed by the effect's own kernel into a proxy of the layer instead of a textured mesh; the Effect Config kit's frame, rows and OK / Cancel instead of the canvas mode and its options bar; no Edit / Create Plane tools, edge nodes, extra planes, Delete, Grid Size, Paste or New Layer controls (the clipboard image is always pasted, whole, where upstream pastes its `content_bounds()`), in-dialog undo or control channel; OK hands the plane and placement to Effect Config's final render (`EffectConfigDialog::final_with_image`) instead of running a command, and keeps the dialog open on a red plane as upstream's `commit` does; reopening brings back the last plane and placement while they make a plane on this layer (Ruling R82), where upstream starts empty.
//!
//! Effects ▸ Distort ▸ Vanishing Point (an Extra): a large preview of the layer. Four clicks make
//! a perspective plane; dragging a corner adjusts it. The clipboard image shows pasted onto the
//! plane in perspective, keeping its proportions in the plane's metric; drag it to move it along
//! the plane, and set its Width. OK pastes it at full resolution as one history item named
//! "Vanishing Point" (Effect Config's cancellable "Applying" step); Cancel changes nothing.
//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; S§4.1 (the title's plus).

use std::sync::Arc;

use egui::{
    Color32, ColorImage, Event, Key, Painter, PointerButton, Pos2, Rect, Sense, Stroke,
    TextureHandle, TextureOptions, Ui, Vec2,
};
use pdn_core::canvas::ImageData;
use pdn_core::effect::Values;
use pdn_core::effect::extra::vanishing::paste_of;
use pdn_core::effect::session::EffectSession;
use pdn_core::effect::slider::{RangeRole, SliderKeys};
use pdn_core::extra::fx::vanishing::{
    Homography, PlaneCheck, VpPlane, camera, frame, paste_cached,
};
use pdn_core::extra::geom::Rect as PxRect;
use pdn_core::extra::interrupt::Interrupt;
use pdn_core::render::checker::{checker_colors, checker_is_light};
use pdn_core::resample::{Gamma, ResizeMode, resize};
use serde_json::json;

use super::effect_config::{
    EffectConfigDialog, RangeSpec, SliderState, Track, paint_title_plus, range_line, range_rects,
};
use super::image_size::{footer_pair, rule};
use super::liquify::LiquifyDialog;
use super::{Dialog, DialogCx, DialogResult, DialogState};
use crate::kit::inputs::{self, NumberStyle};
use crate::kit::text::{self, FontSpec};
use crate::kit::window::{Footer, dialog_frame_with};
use crate::platform::{ImageRead, ImageReadRequest};
use crate::theme::{self, *};

/// Width (% of the plane's width): key, label, min, max, default (upstream's field and default).
const WIDTH: (&str, &str, f64, f64, f64) = ("width", "Width", 1.0, 1000.0, 50.0);
/// The preview's longer side: larger layers preview downsampled (as Liquify's); OK still
/// applies at full resolution.
const PROXY_SIDE: u32 = 1600;
/// The clipboard image's longer side in the preview (upstream's paste texture); OK pastes the
/// image itself.
const CLIP_SIDE: u32 = 1024;
/// The options column's width, DIP.
const PANEL_W: f32 = 236.0;
/// A node's side and how near a press must land to take it, DIP.
const NODE: f32 = 7.0;
const NODE_HIT: f32 = 6.0;
/// Upstream's default Grid Size (document px at the plane's nearest edge) and line cap.
const GRID: f64 = 100.0;
const MAX_LINES: usize = 200;
/// The transparency checkerboard's square, proxy pixels.
const CHECKER: u32 = 8;
/// Photoshop's plane colours, softened: valid blue, doubtful yellow, invalid red.
const VALID: Color32 = Color32::from_rgb(86, 140, 230);
const DOUBTFUL: Color32 = Color32::from_rgb(232, 186, 70);
const INVALID: Color32 = Color32::from_rgb(224, 82, 82);
const HINT: &str = "Click four corners of a flat surface to make a plane. Drag a corner to \
    adjust it, and drag the image to move it along the plane.";
const NO_IMAGE: &str = "The clipboard has no image to paste.";
const RED_PLANE: &str = "Fix the red plane first: its corners must form a convex quad.";

/// What a press on the preview drags.
#[derive(Clone, Copy, Debug)]
enum Drag {
    /// A corner of the plane, and the pointer's offset from it (document px).
    Corner { corner: usize, offset: [f64; 2] },
    /// The pasted image, and the pointer's offset from its top-left (plane coordinates).
    Paste { offset: [f64; 2] },
}

/// The clipboard read, as Effect Config's `acquire_external` reads it (R155): the system
/// clipboard's image, else the app's own copy unless the system clipboard said it has none.
enum Read {
    Unstarted,
    Pending {
        request: ImageReadRequest,
        fallback: Option<Arc<ImageData>>,
        authoritative: bool,
    },
    Done,
    /// The read was cancelled: no image, and OK reads again (Effect Config's retired input).
    Retired,
}

pub struct VanishingPointDialog {
    session: Option<EffectSession>,
    title: String,
    icon: &'static str,
    /// Width as its row edits it; OK adds the plane and the placement.
    values: Values,
    /// The corners clicked so far; four are the plane, clockwise from its top-left.
    corners: Vec<[f64; 2]>,
    /// The image's top-left on the plane (plane coordinates).
    at: [f64; 2],
    drag: Option<Drag>,
    read: Read,
    /// The clipboard image OK pastes, and the copy the preview pastes (at most `CLIP_SIDE`).
    image: Option<Arc<ImageData>>,
    preview_image: Option<Arc<ImageData>>,
    /// `preview_image` prefiltered for the last paste, kept while the size it needs holds.
    prefiltered: Option<ImageData>,
    /// The layer's size, and its proxy: the layer itself up to `PROXY_SIDE`, else downsampled.
    doc: [u32; 2],
    base: ImageData,
    /// The proxy with the paste of `rendered` on it.
    out: ImageData,
    rendered: Option<Values>,
    /// The proxy pixels the last paste could reach, put back before the next one.
    pasted: PxRect,
    /// The selection bounds in proxy pixels: the paste's clip, as `env.bounds` is OK's.
    bounds: PxRect,
    tex: Option<TextureHandle>,
    /// Proxy pixels to upload.
    dirty: PxRect,
    slider: SliderState,
}

impl VanishingPointDialog {
    /// Vanishing Point over `session`'s layer with `values` (the remembered token or the
    /// defaults): Width as last used, and the last plane and placement (Ruling R82: a second
    /// image goes onto the same plane) while they make a usable plane with a corner on this
    /// layer; otherwise no plane yet and the image at upstream's (0.25, 0.25).
    pub fn new(session: EffectSession, mut values: Values) -> Self {
        let def = &session.def;
        let title = def
            .dialog_title
            .map_or_else(|| def.display_name(), String::from);
        let icon = def.icon;
        let src = session.source();
        let doc = [src.width, src.height];
        let s = (f64::from(doc[0].max(doc[1])) / f64::from(PROXY_SIDE)).max(1.0);
        let side = |n: u32| ((f64::from(n) / s).round() as u32).max(1);
        let (pw, ph) = (side(doc[0]), side(doc[1]));
        let data = resize(
            &src.data,
            doc[0],
            doc[1],
            pw,
            ph,
            ResizeMode::Fant,
            Gamma::Companded,
        );
        let base = ImageData {
            width: pw,
            height: ph,
            data,
        };
        let k = [
            f64::from(pw) / f64::from(doc[0]),
            f64::from(ph) / f64::from(doc[1]),
        ];
        let b = session.env().bounds;
        let bounds = PxRect::new(
            (f64::from(b.x) * k[0]).floor() as i32,
            (f64::from(b.y) * k[1]).floor() as i32,
            (f64::from(b.x + b.w as i32) * k[0]).ceil() as i32,
            (f64::from(b.y + b.h as i32) * k[1]).ceil() as i32,
        )
        .intersect(&PxRect::new(0, 0, pw as i32, ph as i32));
        let on_layer = |c: &[f64; 2]| {
            (0.0..=f64::from(doc[0])).contains(&c[0]) && (0.0..=f64::from(doc[1])).contains(&c[1])
        };
        let (corners, at) = paste_of(&values)
            .filter(|p| frame(&p.plane, doc).is_some() && p.plane.corners.iter().any(on_layer))
            .map_or((vec![], [0.25, 0.25]), |p| (p.plane.corners.to_vec(), p.at));
        values.remove("corners");
        values.remove("at");
        values.entry(WIDTH.0).or_insert(json!(WIDTH.4));
        VanishingPointDialog {
            title,
            icon,
            values,
            corners,
            at,
            drag: None,
            read: Read::Unstarted,
            image: None,
            preview_image: None,
            prefiltered: None,
            doc,
            out: base.clone(),
            base,
            rendered: None,
            pasted: PxRect::EMPTY,
            bounds,
            tex: None,
            dirty: PxRect::EMPTY,
            slider: SliderState::default(),
            session: Some(session),
        }
    }

    /// The effect's value: Width, the plane once it has four corners, and the placement.
    fn effect_values(&self) -> Values {
        let mut v = self.values.clone();
        let corners = if self.corners.len() == 4 {
            json!(self.corners)
        } else {
            json!([])
        };
        v.insert("corners".into(), corners);
        v.insert("at".into(), json!(self.at));
        v
    }

    /// The plane, once it has four corners.
    fn plane(&self) -> Option<VpPlane> {
        let corners = <[[f64; 2]; 4]>::try_from(self.corners.as_slice()).ok()?;
        Some(VpPlane { corners })
    }

    /// The clipboard read, started on the first frame (in the opening command's gesture) and
    /// polled until it answers.
    fn read_clipboard(&mut self, cx: &mut DialogCx, ctx: &egui::Context) {
        match std::mem::replace(&mut self.read, Read::Done) {
            Read::Unstarted => {
                let fallback = cx.app.clipboard.item.as_ref().map(|i| i.masked.clone());
                let Some(clipboard) = cx.clipboard.as_deref_mut() else {
                    return self.set_image(fallback);
                };
                let authoritative = clipboard.is_system();
                #[cfg(target_arch = "wasm32")]
                let authoritative = authoritative || clipboard.web_images;
                self.read = Read::Pending {
                    request: clipboard.request_image(ctx),
                    fallback,
                    authoritative,
                };
                self.read_clipboard(cx, ctx);
            }
            Read::Pending {
                mut request,
                fallback,
                authoritative,
            } => match request.poll() {
                ImageRead::Pending => {
                    self.read = Read::Pending {
                        request,
                        fallback,
                        authoritative,
                    };
                    ctx.request_repaint_after(std::time::Duration::from_millis(10));
                }
                ImageRead::Ready(Ok(Some(image))) => self.set_image(Some(Arc::new(image))),
                ImageRead::Ready(Ok(None)) if authoritative => self.set_image(None),
                ImageRead::Ready(_) => self.set_image(fallback),
                ImageRead::Canceled => {
                    self.set_image(None);
                    self.read = Read::Retired;
                }
            },
            done @ (Read::Done | Read::Retired) => self.read = done,
        }
    }

    /// The clipboard's answer; the preview pastes a copy at most `CLIP_SIDE` on its longer side.
    fn set_image(&mut self, image: Option<Arc<ImageData>>) {
        self.preview_image = image.as_ref().map(|i| {
            let side = i.width.max(i.height);
            if side <= CLIP_SIDE {
                return i.clone();
            }
            let s = f64::from(CLIP_SIDE) / f64::from(side);
            let size = |n: u32| ((f64::from(n) * s).round() as u32).max(1);
            let (w, h) = (size(i.width), size(i.height));
            let data = resize(
                &i.data,
                i.width,
                i.height,
                w,
                h,
                ResizeMode::Bicubic,
                Gamma::Companded,
            );
            Arc::new(ImageData {
                width: w,
                height: h,
                data,
            })
        });
        self.image = image;
        self.prefiltered = None;
        self.rendered = None;
        self.read = Read::Done;
    }

    /// The plane's image → plane map and the pasted image's extent on it; `None` without a
    /// usable plane or an image.
    fn paste_frame(&self) -> Option<(Homography, [f64; 2])> {
        let p = paste_of(&self.effect_values())?;
        let image = self.image.as_ref()?;
        let (h, lu, lv) = frame(&p.plane, self.doc)?;
        Some((h.inverse()?, p.extent(lu, lv, image.width, image.height)))
    }

    /// A press at document point `p`: a corner of the plane being made, else the corner node
    /// within `tol`, else the image on the plane.
    fn press(&mut self, p: [f64; 2], tol: f64) {
        if self.corners.len() < 4 {
            self.corners.push(p);
            if let Some(plane) = self.plane() {
                self.corners = plane.normalized().corners.to_vec();
            }
            return;
        }
        let near = |c: &[f64; 2]| (c[0] - p[0]).hypot(c[1] - p[1]) <= tol;
        if let Some(corner) = self.corners.iter().position(near) {
            let c = self.corners[corner];
            self.drag = Some(Drag::Corner {
                corner,
                offset: [c[0] - p[0], c[1] - p[1]],
            });
            return;
        }
        if let Some((inv, ext)) = self.paste_frame() {
            let (u, v) = inv.apply(p[0], p[1]);
            let (a, b) = (self.at, [self.at[0] + ext[0], self.at[1] + ext[1]]);
            if (a[0]..=b[0]).contains(&u) && (a[1]..=b[1]).contains(&v) {
                self.drag = Some(Drag::Paste {
                    offset: [a[0] - u, a[1] - v],
                });
            }
        }
    }

    fn drag_to(&mut self, p: [f64; 2]) {
        match self.drag {
            Some(Drag::Corner { corner, offset }) => {
                self.corners[corner] = [p[0] + offset[0], p[1] + offset[1]];
            }
            Some(Drag::Paste { offset }) => {
                if let Some((inv, _)) = self.paste_frame() {
                    let (u, v) = inv.apply(p[0], p[1]);
                    let at = [
                        (u + offset[0]).clamp(-1.0, 2.0),
                        (v + offset[1]).clamp(-1.0, 2.0),
                    ];
                    if at.iter().all(|x| x.is_finite()) {
                        self.at = at;
                    }
                }
            }
            None => {}
        }
    }

    /// The proxy pixels a paste onto `plane` can reach: its corners' box, inside the clip.
    fn reach(&self, plane: &VpPlane) -> PxRect {
        let k = [
            f64::from(self.base.width) / f64::from(self.doc[0]),
            f64::from(self.base.height) / f64::from(self.doc[1]),
        ];
        let (xs, ys) = (
            plane.corners.map(|c| c[0] * k[0]),
            plane.corners.map(|c| c[1] * k[1]),
        );
        let lo = |v: [f64; 4]| v.iter().fold(f64::MAX, |m, x| m.min(*x)).floor() as i32;
        let hi = |v: [f64; 4]| v.iter().fold(f64::MIN, |m, x| m.max(*x)).ceil() as i32;
        PxRect::new(lo(xs), lo(ys), hi(xs), hi(ys)).intersect(&self.bounds)
    }

    /// Re-pastes the preview when the value changed: the last paste's pixels go back to the
    /// layer's, then the effect's own kernel pastes the new value, and both are uploaded.
    fn render(&mut self) {
        let v = self.effect_values();
        if self.rendered.as_ref() == Some(&v) {
            return;
        }
        let w = self.base.width as usize;
        let back = self.pasted;
        for y in back.y0..back.y1 {
            let row =
                (y as usize * w + back.x0 as usize) * 4..(y as usize * w + back.x1 as usize) * 4;
            self.out.data[row.clone()].copy_from_slice(&self.base.data[row]);
        }
        self.pasted = PxRect::EMPTY;
        if let (Some(image), Some(p)) = (&self.preview_image, paste_of(&v)) {
            self.pasted = self.reach(&p.plane);
            let _ = paste_cached(
                &mut self.out,
                self.doc,
                self.bounds,
                &p,
                image,
                &mut self.prefiltered,
                Interrupt::NONE,
            );
        }
        self.dirty = self.dirty.union(&back).union(&self.pasted);
        self.rendered = Some(v);
    }

    /// Re-renders and uploads the dirty proxy pixels (the whole proxy the first time).
    fn upload(&mut self, ctx: &egui::Context, checker: (u8, u8)) {
        self.render();
        let all = PxRect::new(0, 0, self.base.width as i32, self.base.height as i32);
        let r = if self.tex.is_none() {
            all
        } else {
            self.dirty.intersect(&all)
        };
        self.dirty = PxRect::EMPTY;
        if r.is_empty() {
            return;
        }
        let pixels = (r.y0..r.y1)
            .flat_map(|y| (r.x0..r.x1).map(move |x| (x as usize, y as usize)))
            .map(|(x, y)| self.shown(x, y, checker))
            .collect();
        let image = ColorImage::new([r.width() as usize, r.height() as usize], pixels);
        match &mut self.tex {
            Some(t) => t.set_partial(
                [r.x0 as usize, r.y0 as usize],
                image,
                TextureOptions::LINEAR,
            ),
            None => self.tex = Some(ctx.load_texture("vp-preview", image, TextureOptions::LINEAR)),
        }
    }

    /// Proxy pixel (x, y) as shown: the pasted layer kept to the selection as OK's clipping keeps
    /// it, over the checkerboard.
    fn shown(&self, x: usize, y: usize, (light, dark): (u8, u8)) -> Color32 {
        let i = (y * self.base.width as usize + x) * 4;
        let (o, b) = (&self.out.data[i..i + 4], &self.base.data[i..i + 4]);
        let (dx, dy) = (
            (x as f64 + 0.5) * f64::from(self.doc[0]) / f64::from(self.base.width),
            (y as f64 + 0.5) * f64::from(self.doc[1]) / f64::from(self.base.height),
        );
        let selected = self
            .session
            .as_ref()
            .and_then(EffectSession::mask)
            .map_or(1.0, |m| f32::from(m.get(dx as i32, dy as i32)) / 255.0);
        // Premultiplied, blended toward the layer outside the selection.
        let pm = |q: &[u8], c: usize| {
            let a = f32::from(q[3]);
            if c == 3 {
                a
            } else {
                f32::from(q[c]) * a / 255.0
            }
        };
        let p: [f32; 4] =
            std::array::from_fn(|c| pm(o, c) + (pm(b, c) - pm(o, c)) * (1.0 - selected));
        let bg = f32::from(if checker_is_light(x as i32, y as i32, CHECKER) {
            light
        } else {
            dark
        });
        let c: [u8; 3] = std::array::from_fn(|c| (p[c] + bg * (1.0 - p[3] / 255.0) + 0.5) as u8);
        Color32::from_rgb(c[0], c[1], c[2])
    }

    /// The body: the preview, then the options column on the right.
    fn body(&mut self, ui: &mut Ui, r: Rect) {
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let inner = Rect::from_min_max(
            Pos2::new(r.min.x + side, r.min.y + top),
            Pos2::new(r.max.x - side, r.max.y - bottom),
        );
        let preview = Rect::from_min_max(
            inner.min,
            Pos2::new(inner.max.x - PANEL_W - side, inner.max.y),
        );
        self.preview(ui, preview);
        let panel = Rect::from_min_max(Pos2::new(preview.max.x + side, inner.min.y), inner.max);
        self.options(ui, panel);
    }

    /// The layer fitted into `area` (at most ×4): its zoom and screen rect.
    fn fit(&self, area: Rect) -> (f32, Rect) {
        let (w, h) = (self.doc[0] as f32, self.doc[1] as f32);
        let zoom = (area.width() / w).min(area.height() / h).min(4.0);
        (
            zoom,
            Rect::from_center_size(area.center(), Vec2::new(w * zoom, h * zoom)),
        )
    }

    /// The preview: the pasted layer with the plane over it. A press makes a corner, or takes a
    /// corner node or the image to drag.
    fn preview(&mut self, ui: &mut Ui, area: Rect) {
        let theme = theme::current(ui.ctx());
        let painter = ui.painter().with_clip_rect(area);
        painter.rect_filled(area, 0.0, theme.background);
        let (zoom, image) = self.fit(area);
        if let Some(tex) = &self.tex {
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            painter.image(tex.id(), image, uv, Color32::WHITE);
        }
        let response = ui.interact(
            area,
            ui.scope_id().with("vp-preview"),
            Sense::click_and_drag(),
        );
        let doc = |p: Pos2| {
            [
                f64::from((p.x - image.min.x) / zoom),
                f64::from((p.y - image.min.y) / zoom),
            ]
        };
        // The press itself, so a click released within the same frame still counts.
        let (press, down, at) = ui.input(|i| {
            let press = i.events.iter().find_map(|e| match e {
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed: true,
                    ..
                } if area.contains(*pos) => Some(*pos),
                _ => None,
            });
            (press, i.pointer.primary_down(), i.pointer.interact_pos())
        });
        if let Some(p) = press {
            self.press(doc(p), f64::from(NODE_HIT / zoom));
        } else if let (true, Some(p)) = (down, at) {
            self.drag_to(doc(p));
        }
        if !down {
            self.drag = None;
        }
        if self.drag.is_some() {
            ui.ctx().request_repaint();
        }
        self.overlay(&painter, image, zoom, response.hover_pos());
    }

    /// The plane (outline, grid and corner nodes in its validity colour), or the corners so far
    /// with a rubber band to the pointer.
    fn overlay(&self, painter: &Painter, image: Rect, zoom: f32, hover: Option<Pos2>) {
        let scr = |p: [f64; 2]| image.min + Vec2::new(p[0] as f32, p[1] as f32) * zoom;
        let Some(plane) = self.plane() else {
            let mut pts: Vec<Pos2> = self.corners.iter().map(|c| scr(*c)).collect();
            for c in &pts {
                painter.rect_filled(Rect::from_center_size(*c, Vec2::splat(6.0)), 0.0, VALID);
            }
            if !pts.is_empty() {
                pts.extend(hover);
                painter.add(egui::Shape::line(pts, Stroke::new(1.25, VALID)));
            }
            return;
        };
        let colour = match plane.check(camera(self.doc).0) {
            PlaneCheck::Valid => VALID,
            PlaneCheck::Questionable => DOUBTFUL,
            PlaneCheck::Invalid => INVALID,
        };
        // A red plane has no perspective to grid: only its outline, as in Photoshop.
        if let Some((h, lu, lv)) = frame(&plane, self.doc) {
            let thin = Stroke::new(0.75, colour.gamma_multiply(0.6));
            let at = |u: f64, v: f64| {
                let (x, y) = h.apply(u, v);
                scr([x, y])
            };
            for u in grid_steps(&plane, lu, lv, true) {
                painter.line_segment([at(u, 0.0), at(u, 1.0)], thin);
            }
            for v in grid_steps(&plane, lu, lv, false) {
                painter.line_segment([at(0.0, v), at(1.0, v)], thin);
            }
        }
        let pts: Vec<Pos2> = plane.corners.iter().map(|c| scr(*c)).collect();
        painter.add(egui::Shape::closed_line(
            pts.clone(),
            Stroke::new(2.0, colour),
        ));
        for c in pts {
            let r = Rect::from_center_size(c, Vec2::splat(NODE));
            painter.rect_filled(r, 0.0, Color32::WHITE);
            painter.rect_stroke(r, 0.0, Stroke::new(1.25, colour), egui::StrokeKind::Inside);
        }
    }

    /// The options column: Width as an Effect Config range row, then how to use the preview.
    fn options(&mut self, ui: &mut Ui, r: Rect) {
        let theme = theme::current(ui.ctx());
        let ppp = ui.ctx().pixels_per_point();
        let spec = FontSpec::new(DIALOG_FONT_SIZE);
        let line = spec.resolved_line_height(ppp);
        let (x0, w) = (r.min.x, r.width());
        // `.effect-config-row > label`: the text over a separator.
        text::draw(
            ui.painter(),
            Pos2::new(x0, r.min.y),
            WIDTH.1,
            spec,
            theme.menu_item_text,
        );
        rule(ui, x0, x0 + w, r.min.y + line + EFFECT_LABEL_PADDING);
        let y = r.min.y + line + EFFECT_LABEL_PADDING + 1.0 + EFFECT_ROW_GAP;
        let range_spec = RangeSpec {
            min: WIDTH.2,
            max: WIDTH.3,
            default: WIDTH.4,
            step: 1.0,
            role: RangeRole::Slider,
            keyboard: SliderKeys::INTEGER,
        };
        let (range, number, reset) = range_rects(x0, w, y);
        range_line(
            ui,
            &mut self.slider,
            WIDTH.0,
            range_spec,
            &mut self.values,
            range,
            number,
            Some(reset),
            Track::Progress(0.0),
            NumberStyle::EFFECT,
        );
        let hint = match (&self.read, &self.image) {
            (Read::Done | Read::Retired, None) => NO_IMAGE,
            _ => HINT,
        };
        let runs = [text::Run {
            text: hint,
            spec,
            color: theme.menu_item_text,
            underline: false,
        }];
        let lines = text::wrap_runs(ui.painter(), &runs, w);
        let top = Pos2::new(x0, y + EFFECT_LINE_HEIGHT + EFFECT_CONTENT_GAP);
        text::draw_lines(ui.painter(), top, &runs, &lines, line);
    }

    /// OK: the plane and placement go to Effect Config's final render with the image the preview
    /// pasted, which applies them at full resolution (cancellable) and commits one "Vanishing
    /// Point" history item. Without an answer from the clipboard (still reading, or the read was
    /// cancelled) the final render reads it itself, as Effect Config's OK does, and cancels if
    /// that read is cancelled too. A red plane keeps the dialog open, as upstream's commit does.
    fn ok(&mut self, cx: &mut DialogCx, ctx: &egui::Context) -> DialogState {
        if self
            .plane()
            .is_some_and(|p| p.check(camera(self.doc).0) == PlaneCheck::Invalid)
        {
            crate::platform::files::message(&self.title, RED_PLANE.into());
            return DialogState::Open;
        }
        if let Some(s) = self.session.take() {
            let values = self.effect_values();
            let dialog = if matches!(self.read, Read::Pending { .. } | Read::Retired) {
                let mut d = EffectConfigDialog::final_only(s, values);
                d.acquire_external(cx.app, ctx, cx.clipboard.as_deref_mut());
                d
            } else {
                EffectConfigDialog::final_with_image(s, values, self.image.clone())
            };
            super::open(Box::new(dialog));
        }
        DialogState::Closed(DialogResult::Ok)
    }

    /// Cancel: the layer and the dirty flag as they were; no history.
    fn cancel(&mut self, cx: &mut DialogCx) -> DialogState {
        if let Some(s) = self.session.take() {
            s.cancel(cx.app);
        }
        DialogState::Closed(DialogResult::Cancel)
    }
}

/// Plane coordinates (0..1, exclusive) of the grid lines across `u` (or `v`) for cells of
/// `GRID` px at the plane's nearest edge, square in the plane's metric.
fn grid_steps(p: &VpPlane, lu: f64, lv: f64, along_u: bool) -> Vec<f64> {
    let q = &p.corners;
    let d = |a: [f64; 2], b: [f64; 2]| (a[0] - b[0]).hypot(a[1] - b[1]);
    // Image px per metric unit at the nearest (longest-looking) edge.
    let k = [
        d(q[0], q[1]) / lu,
        d(q[3], q[2]) / lu,
        d(q[0], q[3]) / lv,
        d(q[1], q[2]) / lv,
    ]
    .into_iter()
    .fold(0.0, f64::max);
    let cell = GRID / k;
    let len = if along_u { lu } else { lv };
    if !cell.is_finite() || cell <= 0.0 || !len.is_finite() {
        return Vec::new();
    }
    let step = cell / len;
    (1..MAX_LINES)
        .map(|j| j as f64 * step)
        .take_while(|t| *t < 1.0 - 1e-9)
        .collect()
}

impl Dialog for VanishingPointDialog {
    fn title(&self) -> String {
        self.title.clone()
    }

    fn show(&mut self, ui: &mut Ui, cx: &mut DialogCx) -> DialogState {
        let ctx = ui.ctx().clone();
        // Plain Esc and Enter only, as in the Effect Config dialog.
        let plain = |key| {
            ui.input(|i| {
                !i.modifiers.ctrl && !i.modifiers.alt && !i.modifiers.mac_cmd && i.key_pressed(key)
            })
        };
        if plain(Key::Escape) {
            return self.cancel(cx);
        }
        let enter = plain(Key::Enter);
        self.read_clipboard(cx, &ctx);
        let display =
            crate::shell::canvas_view::DisplayPrefs::new(cx.settings, &theme::current(&ctx));
        let checker = checker_colors(display.checker_brightness);
        // The texture exists before the body draws it; this frame's edits upload after it.
        self.upload(&ctx, checker);
        let (title, icon) = (self.title.clone(), self.icon);
        let mut action = None;
        let mut body = |ui: &mut Ui, r: Rect| self.body(ui, r);
        let mut footer = |ui: &mut Ui, f: Rect| {
            let (fy, fx) = DIALOG_FOOTER_PADDING;
            let content = Rect::from_min_max(
                Pos2::new(f.min.x + fx, f.min.y + fy),
                Pos2::new(f.max.x - fx, f.max.y - fy),
            );
            action = footer_pair(ui, content, DIALOG_BUTTON_MIN.0, DIALOG_BUTTON_MIN.1, true);
        };
        let frame = dialog_frame_with(
            ui,
            &title,
            Some(icon),
            false,
            (LiquifyDialog::size(&ctx), &mut body),
            Some(&mut footer),
            Footer {
                border: false,
                ..Footer::DEFAULT
            },
        );
        // S§4.1: an Extra effect's title carries the plus after its text.
        let id = self.session.as_ref().map(|s| s.def.id);
        paint_title_plus(ui, id, frame.rect, &title);
        self.upload(&ctx, checker);
        if cx.seat_focus {
            super::seat_no_focus(&ctx);
        }
        match action {
            Some(true) => self.ok(cx, &ctx),
            Some(false) => self.cancel(cx),
            None if frame.closed => self.cancel(cx),
            None if enter && !inputs::enter_taken(&ctx) => self.ok(cx, &ctx),
            // Ruling R65: a press outside only flashes the dialog.
            None => DialogState::Open,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kit::px;
    use crate::kit::window::DialogFrameState;
    use crate::platform::{Clipboard, TestImageReads};
    use pdn_core::app::AppWorkspace;
    use pdn_core::effect::extra::vanishing::ID;
    use pdn_core::testing::{fresh_doc, layer_bytes};

    fn context() -> egui::Context {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        ctx
    }

    fn with_cx<T>(app: &mut AppWorkspace, f: impl FnOnce(&mut DialogCx) -> T) -> T {
        with_clipboard(app, None, f)
    }

    fn with_clipboard<T>(
        app: &mut AppWorkspace,
        clipboard: Option<&mut Clipboard>,
        f: impl FnOnce(&mut DialogCx) -> T,
    ) -> T {
        f(&mut DialogCx {
            app,
            settings: &mut pdn_core::settings::AppSettings::load(None),
            prefs: &mut crate::prefs::Prefs::default(),
            settings_changed: false,
            shortcuts: None,
            clipboard,
            settings_imports: None,
            fonts: None,
            asset_library: None,
            seat_focus: false,
        })
    }

    fn input(events: Vec<Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
            events,
            ..Default::default()
        }
    }

    /// One frame of the dialog on its own: its state and border box.
    fn frame(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut VanishingPointDialog,
        events: Vec<Event>,
    ) -> (DialogState, Rect) {
        frame_with(ctx, app, d, None, events)
    }

    /// `frame` with a system clipboard in the dialog's context.
    fn frame_with(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut VanishingPointDialog,
        mut clipboard: Option<&mut Clipboard>,
        events: Vec<Event>,
    ) -> (DialogState, Rect) {
        let mut result = (DialogState::Open, Rect::NOTHING);
        let mut out = ctx.run_ui(input(events), |ui| {
            result.0 = with_clipboard(app, clipboard.as_deref_mut(), |cx| d.show(ui, cx));
            result.1 = DialogFrameState::get(ctx, ui.scope_id()).rect;
        });
        out.textures_delta.clear();
        result
    }

    /// One frame of a dialog stack.
    fn stack_frame(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        stack: &mut super::super::DialogStack,
    ) {
        let mut out = ctx.run_ui(input(vec![]), |ui| with_cx(app, |cx| stack.show(ui, cx)));
        out.textures_delta.clear();
    }

    const RED: [u8; 4] = [255, 0, 0, 255];
    const GREEN: [u8; 4] = [0, 255, 0, 255];

    /// 64×32, `left` on the left half and blue on the right: larger than the plane, so the
    /// paste is prefiltered.
    fn clip_image(left: [u8; 4]) -> ImageData {
        ImageData {
            width: 64,
            height: 32,
            data: (0..64 * 32)
                .flat_map(|i| if i % 64 < 32 { left } else { [0, 0, 255, 255] })
                .collect(),
        }
    }

    /// The layer pixel in the middle of the pasted image's left half.
    fn left_half(d: &VanishingPointDialog) -> usize {
        let p = paste_of(&d.effect_values()).unwrap();
        let (h, lu, lv) = pdn_core::extra::fx::vanishing::frame(&p.plane, d.doc).unwrap();
        let ext = p.extent(lu, lv, 64, 32);
        let (x, y) = h.apply(p.at[0] + ext[0] / 4.0, p.at[1] + ext[1] / 2.0);
        (y as usize * 64 + x as usize) * 4
    }

    /// Vanishing Point on a 64×48 noise layer, with `clip_image` on the app's clipboard.
    fn session() -> (AppWorkspace, EffectSession) {
        super::super::PENDING.with(|p| p.borrow_mut().clear());
        let mut app = fresh_doc(64, 48, Some(3));
        app.clipboard.set_external(clip_image(RED));
        let def = pdn_core::effect::find(ID).unwrap();
        let s = EffectSession::begin(&mut app, def).unwrap();
        (app, s)
    }

    fn dialog() -> (AppWorkspace, VanishingPointDialog) {
        let (app, s) = session();
        let values = s.initial_values(&app);
        (app, VanishingPointDialog::new(s, values))
    }

    fn button(pos: Pos2, pressed: bool) -> Event {
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        }
    }

    fn key(key: Key) -> Vec<Event> {
        vec![Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]
    }

    /// Where the preview shows document point (x, y), from the layout constants rather than the
    /// dialog's own mapping: the 64×48 layer at ×4 (the cap), centred in the preview, which is
    /// the body (below the title bar, inside the border) less its padding and the options column.
    fn screen_of(ctx: &egui::Context, rect: Rect, x: f64, y: f64) -> Pos2 {
        let body = Rect::from_min_size(
            rect.min + Vec2::new(DIALOG_BORDER, DIALOG_BORDER + DIALOG_TITLE_BAR),
            Vec2::new(
                rect.width() - 2.0 * DIALOG_BORDER,
                LiquifyDialog::size(ctx).y,
            ),
        );
        let (top, side, bottom) = EFFECT_CONTENT_PADDING;
        let area = Rect::from_min_max(
            body.min + Vec2::new(side, top),
            Pos2::new(body.max.x - side - PANEL_W - side, body.max.y - bottom),
        );
        area.center() + Vec2::new(4.0 * (x as f32 - 32.0), 4.0 * (y as f32 - 24.0))
    }

    /// A press and release at `p`, a frame each.
    fn click(ctx: &egui::Context, app: &mut AppWorkspace, d: &mut VanishingPointDialog, p: Pos2) {
        frame(ctx, app, d, vec![Event::PointerMoved(p), button(p, true)]);
        frame(ctx, app, d, vec![button(p, false)]);
    }

    /// A press at `from`, a move to `to` and the release there.
    fn drag(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut VanishingPointDialog,
        from: Pos2,
        to: Pos2,
    ) {
        frame(
            ctx,
            app,
            d,
            vec![Event::PointerMoved(from), button(from, true)],
        );
        frame(ctx, app, d, vec![Event::PointerMoved(to)]);
        frame(ctx, app, d, vec![button(to, false)]);
    }

    /// The trapezoid the tests make, clockwise from its top-left (document px).
    const TRAPEZOID: [[f64; 2]; 4] = [[16.0, 10.0], [48.0, 10.0], [60.0, 42.0], [4.0, 42.0]];

    /// Clicks the trapezoid's corners counter-clockwise from its bottom-left; the border box.
    fn make_plane(
        ctx: &egui::Context,
        app: &mut AppWorkspace,
        d: &mut VanishingPointDialog,
    ) -> Rect {
        let (_, rect) = frame(ctx, app, d, vec![]);
        for i in [3, 2, 1, 0] {
            let [x, y] = TRAPEZOID[i];
            click(ctx, app, d, screen_of(ctx, rect, x, y));
        }
        rect
    }

    fn close(a: [f64; 2], b: [f64; 2]) -> bool {
        (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
    }

    /// S§4.4: Effects ▸ Distort ▸ Vanishing Point opens this dialog, not the Effect Config one.
    #[test]
    fn effect_dialog_opens_vanishing_point_for_its_effect() {
        let (mut app, s) = session();
        let values = s.initial_values(&app);
        let ctx = context();
        let mut stack = super::super::DialogStack::default();
        stack.push(super::super::effect_dialog(s, values, |_| {}));
        stack_frame(&ctx, &mut app, &mut stack);
        let rect = stack.top_rect(&ctx).unwrap();
        assert!(stack.top_title_is("Vanishing Point"));
        assert_eq!(
            rect.width(),
            LiquifyDialog::size(&ctx).x,
            "the large preview"
        );
    }

    /// S§4.1: the dialog's title carries the plus after its text.
    #[test]
    fn vanishing_point_dialog_title_has_plus() {
        let (mut app, mut d) = dialog();
        assert_eq!(d.title, "Vanishing Point");
        let ctx = context();
        let image = image::RgbaImage::from_pixel(12, 16, image::Rgba([20, 200, 70, 255]));
        let mut png = std::io::Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        crate::assets::Assets::set_icon_override(&ctx, "plugin_indicator.png", png.into_inner());
        let mut expected = Rect::NOTHING;
        let mut output = ctx.run_ui(input(vec![]), |ui| {
            with_cx(&mut app, |cx| d.show(ui, cx));
            let frame = DialogFrameState::get(ui.ctx(), ui.scope_id()).rect;
            expected = px::snap_rect(
                super::super::effect_config::title_plus_rect(ui.painter(), frame, &d.title),
                1.0,
            );
        });
        let meshes: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Mesh(m) => Some(m.calc_bounds()),
                _ => None,
            })
            .collect();
        output.textures_delta.clear();
        assert!(meshes.contains(&expected), "{expected:?} in {meshes:?}");
    }

    /// S§4.1: the menu item is under Distort.
    #[test]
    fn vanishing_point_menu_item_is_under_distort() {
        use crate::kit::menu::MenuEntry;
        use crate::shell::main_menu::{ITEMS, entries};
        fn distort_has_it(entries: &[MenuEntry]) -> bool {
            entries.iter().any(|e| match e {
                MenuEntry::Submenu { key, entries } => {
                    *key == "effects.distort.submenu"
                        && entries.contains(&MenuEntry::Action("menu.effects.pcVanishingPoint"))
                }
                _ => false,
            })
        }
        let effects = ITEMS.iter().position(|i| *i == "menu.effects").unwrap();
        assert!(distort_has_it(&entries(effects, &AppWorkspace::default())));
    }

    /// Four clicks on the preview, in any order, make the plane at the clicked document points,
    /// clockwise from its top-left, and its unit square's corners land on them. The expected
    /// points come from the layout, not from the dialog's pointer mapping.
    #[test]
    fn plane_from_four_points_maps_corners() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let (_, rect) = frame(&ctx, &mut app, &mut d, vec![]);
        for (i, [x, y]) in TRAPEZOID.into_iter().enumerate().rev() {
            assert!(d.plane().is_none(), "{} corners", 3 - i);
            click(&ctx, &mut app, &mut d, screen_of(&ctx, rect, x, y));
        }
        let plane = d.plane().unwrap();
        assert!(
            plane
                .corners
                .iter()
                .zip(TRAPEZOID)
                .all(|(a, b)| close(*a, b)),
            "{:?}",
            plane.corners
        );
        let h = plane.homography().unwrap();
        for (uv, want) in [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]
            .into_iter()
            .zip(TRAPEZOID)
        {
            let (x, y) = h.apply(uv[0], uv[1]);
            assert!(close([x, y], want), "{uv:?} → ({x}, {y})");
        }
        // A fifth click on the plane (off the nodes and the image) changes nothing.
        let before = d.corners.clone();
        click(&ctx, &mut app, &mut d, screen_of(&ctx, rect, 32.0, 40.0));
        assert_eq!(d.corners, before);
    }

    /// Effect Config's final render on a fresh stack until it closes.
    fn finish(ctx: &egui::Context, app: &mut AppWorkspace) {
        let mut stack = super::super::DialogStack::default();
        for _ in 0..1000 {
            stack_frame(ctx, app, &mut stack);
            if !stack.is_open() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!stack.is_open());
    }

    /// Making the plane edits only the preview; OK pastes the clipboard onto it at full
    /// resolution as one history item named "Vanishing Point", and undo restores the layer bit
    /// for bit.
    #[test]
    fn paste_onto_plane_is_one_history_item() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        make_plane(&ctx, &mut app, &mut d);
        assert_ne!(d.out, d.base, "the preview shows the paste");
        assert_eq!(layer_bytes(&app, None), before, "the layer waits for OK");
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Enter));
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        finish(&ctx, &mut app);
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo.len(), undo + 1);
        assert_eq!(ws.history.undo.last().unwrap().name, "Vanishing Point");
        let after = layer_bytes(&app, None);
        assert_ne!(after, before);
        // The middle of the image's red (left) half is red on the layer.
        let i = left_half(&d);
        assert_eq!(&after[i..i + 4], &RED);
        assert_eq!(app.last_effect.as_deref(), Some(ID));
        let values = app.last_effect_values.as_ref().unwrap();
        let stored: Vec<[f64; 2]> = serde_json::from_value(values["corners"].clone()).unwrap();
        assert!(
            stored.iter().zip(TRAPEZOID).all(|(a, b)| close(*a, b)),
            "{stored:?}"
        );
        app.undo();
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// The preview is what OK applies: after the plane, a drag of the image along it and a drag
    /// of a corner, the previewed layer is the effect rendered at full resolution from the value
    /// OK stores, with the same image.
    #[test]
    fn the_preview_is_the_full_resolution_render() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let rect = make_plane(&ctx, &mut app, &mut d);
        // Drag the image by its middle, 3 px right and 2 down.
        let (inv, ext) = d.paste_frame().unwrap();
        let h = inv.inverse().unwrap();
        let (mx, my) = h.apply(d.at[0] + ext[0] / 2.0, d.at[1] + ext[1] / 2.0);
        let at = d.at;
        drag(
            &ctx,
            &mut app,
            &mut d,
            screen_of(&ctx, rect, mx, my),
            screen_of(&ctx, rect, mx + 3.0, my + 2.0),
        );
        assert!(d.at[0] > at[0] && d.at[1] > at[1], "{:?} from {at:?}", d.at);
        // Drag the top-right corner out.
        drag(
            &ctx,
            &mut app,
            &mut d,
            screen_of(&ctx, rect, 48.0, 10.0),
            screen_of(&ctx, rect, 52.0, 6.0),
        );
        assert!(close(d.corners[1], [52.0, 6.0]), "{:?}", d.corners);
        let mut s = d.session.clone().unwrap();
        s.set_image(d.image.clone());
        let full = s.render(&d.effect_values());
        assert_ne!(full, *s.source());
        assert_eq!(d.out, full);
    }

    /// With a selection the preview shows the paste only inside it, as OK's clipping does.
    #[test]
    fn the_preview_keeps_to_the_selection() {
        use pdn_core::selection::CombineMode;
        use pdn_core::util::geom::Rectangle;
        let (mut app, _) = session();
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(0.0, 0.0, 32.0, 48.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let def = pdn_core::effect::find(ID).unwrap();
        let s = EffectSession::begin(&mut app, def).unwrap();
        let mut d = VanishingPointDialog::new(s, Values::new());
        let ctx = context();
        make_plane(&ctx, &mut app, &mut d);
        d.at = [-0.5, -0.5];
        d.values.insert(WIDTH.0.into(), json!(300));
        d.render();
        let checker = (255, 191);
        let plain = |x: usize, y: usize| {
            let mut still = VanishingPointDialog::new(d.session.clone().unwrap(), Values::new());
            still.render();
            still.shown(x, y, checker)
        };
        // Right of the selection the layer as it is; inside it the paste.
        assert_eq!(d.shown(40, 30, checker), plain(40, 30));
        assert_ne!(d.shown(24, 30, checker), plain(24, 30));
    }

    /// Cancel after making a plane: the layer, history, dirty flag and Repeat as they were.
    #[test]
    fn vanishing_point_cancel_changes_nothing() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        make_plane(&ctx, &mut app, &mut d);
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Escape));
        assert_eq!(state, DialogState::Closed(DialogResult::Cancel));
        assert!(super::super::PENDING.with(|p| p.borrow().is_empty()));
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.history.undo.len(), ws.dirty), (undo, false));
        assert_eq!(layer_bytes(&app, None), before);
        assert!(app.last_effect.is_none());
    }

    /// OK on a red (bow-tie) plane keeps the dialog open with a message, as upstream's commit
    /// does; nothing is applied.
    #[test]
    fn ok_on_a_red_plane_keeps_the_dialog_open() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let rect = make_plane(&ctx, &mut app, &mut d);
        // The bottom-right corner dragged past the bottom-left one twists the quad.
        drag(
            &ctx,
            &mut app,
            &mut d,
            screen_of(&ctx, rect, 60.0, 42.0),
            screen_of(&ctx, rect, 0.0, 30.0),
        );
        assert_eq!(
            d.plane().unwrap().check(camera(d.doc).0),
            PlaneCheck::Invalid
        );
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Enter));
        assert_eq!(state, DialogState::Open);
        assert_eq!(
            super::super::PENDING.with(|p| p.borrow().len()),
            1,
            "the message"
        );
        assert!(d.session.is_some());
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// Ctrl, Alt or Cmd with Esc or Enter does neither (Effect Config's rule).
    #[test]
    fn modified_escape_and_enter_do_nothing() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        frame(&ctx, &mut app, &mut d, vec![]);
        for modifiers in [
            egui::Modifiers::CTRL,
            egui::Modifiers::ALT,
            egui::Modifiers::MAC_CMD,
        ] {
            for k in [Key::Escape, Key::Enter] {
                let raw = input(vec![
                    Event::ModifiersChanged(modifiers),
                    Event::Key {
                        key: k,
                        physical_key: None,
                        pressed: true,
                        repeat: false,
                        modifiers,
                    },
                ]);
                let mut state = DialogState::Open;
                let mut out = ctx.run_ui(raw, |ui| {
                    state = with_cx(&mut app, |cx| d.show(ui, cx));
                });
                out.textures_delta.clear();
                assert_eq!(state, DialogState::Open, "{modifiers:?} {k:?}");
            }
        }
        assert!(super::super::PENDING.with(|p| p.borrow().is_empty()));
    }

    /// Without an image on the clipboard the dialog says so, and still makes planes.
    #[test]
    fn without_a_clipboard_image_nothing_is_pasted() {
        let (mut app, s) = session();
        app.clipboard.item = None;
        let mut d = VanishingPointDialog::new(s, Values::new());
        let ctx = context();
        make_plane(&ctx, &mut app, &mut d);
        assert!(matches!(d.read, Read::Done) && d.image.is_none());
        assert!(d.plane().is_some());
        assert_eq!(d.out, d.base);
    }

    /// Ruling R82: after OK, reopening brings back the plane, the placement and Width, so a
    /// second image goes onto the same plane; a remembered red plane, or one with no corner on
    /// the layer, is dropped (Width stays).
    #[test]
    fn reopening_keeps_the_remembered_plane() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        make_plane(&ctx, &mut app, &mut d);
        d.at = [0.3, 0.35];
        d.values.insert(WIDTH.0.into(), json!(70));
        frame(&ctx, &mut app, &mut d, key(Key::Enter));
        finish(&ctx, &mut app);
        let def = pdn_core::effect::find(ID).unwrap();
        let s = EffectSession::begin(&mut app, def.clone()).unwrap();
        let remembered = s.initial_values(&app);
        let mut again = VanishingPointDialog::new(s, remembered.clone());
        assert_eq!(again.corners, d.corners);
        assert_eq!(again.at, [0.3, 0.35]);
        assert_eq!(again.values[WIDTH.0], json!(70));
        // A second image onto the same plane: one more history item.
        app.clipboard.set_external(clip_image(GREEN));
        let undo = app.active_ws().unwrap().history.undo.len();
        frame(&ctx, &mut app, &mut again, vec![]);
        let (state, _) = frame(&ctx, &mut app, &mut again, key(Key::Enter));
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        finish(&ctx, &mut app);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo + 1);
        let i = left_half(&again);
        assert_eq!(&layer_bytes(&app, None)[i..i + 4], &GREEN);
        let bowtie = json!([[16, 10], [60, 42], [48, 10], [4, 42]]);
        let off_layer = json!([[100, 100], [140, 100], [150, 140], [90, 140]]);
        for corners in [bowtie, off_layer] {
            let mut v = remembered.clone();
            v.insert("corners".into(), corners.clone());
            let s = EffectSession::begin(&mut app, def.clone()).unwrap();
            let d = VanishingPointDialog::new(s, v);
            assert!(d.corners.is_empty(), "{corners}");
            assert_eq!(d.at, [0.25, 0.25]);
            assert_eq!(d.values[WIDTH.0], json!(70));
        }
    }

    /// OK pastes the image the preview read, though the clipboard changed since: the reason for
    /// `final_with_image`.
    #[test]
    fn ok_pastes_the_image_the_preview_read() {
        let (mut app, mut d) = dialog();
        let ctx = context();
        make_plane(&ctx, &mut app, &mut d);
        app.clipboard.set_external(clip_image(GREEN));
        let (state, _) = frame(&ctx, &mut app, &mut d, key(Key::Enter));
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        finish(&ctx, &mut app);
        let i = left_half(&d);
        assert_eq!(&layer_bytes(&app, None)[i..i + 4], &RED);
    }

    /// OK while the system clipboard is still reading: the final render reads it itself (a
    /// second read) and pastes what that read answers.
    #[test]
    fn ok_while_the_clipboard_is_reading_reads_it_in_the_final_render() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d) = dialog();
        let mut clipboard = Clipboard::in_memory();
        let ctx = context();
        frame_with(&ctx, &mut app, &mut d, Some(&mut clipboard), vec![]);
        assert!(matches!(d.read, Read::Pending { .. }));
        assert_eq!(probe.reads.get(), 1);
        make_plane(&ctx, &mut app, &mut d);
        let undo = app.active_ws().unwrap().history.undo.len();
        let (state, _) = frame_with(
            &ctx,
            &mut app,
            &mut d,
            Some(&mut clipboard),
            key(Key::Enter),
        );
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        assert_eq!(probe.reads.get(), 2, "the final render's own read");
        probe.completions.borrow()[1].complete(Ok(Some(clip_image(GREEN))));
        finish(&ctx, &mut app);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo + 1);
        let i = left_half(&d);
        assert_eq!(&layer_bytes(&app, None)[i..i + 4], &GREEN);
        TestImageReads::clear();
    }

    /// A cancelled read retires the input (no image, the no-image hint); OK reads again, as
    /// Effect Config's OK does, and when that read is cancelled too the final step cancels
    /// instead of committing a no-op.
    #[test]
    fn a_cancelled_read_retires_and_ok_cancels_on_a_second_cancel() {
        let probe = TestImageReads::install(true);
        let (mut app, mut d) = dialog();
        let mut clipboard = Clipboard::in_memory();
        let ctx = context();
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        frame_with(&ctx, &mut app, &mut d, Some(&mut clipboard), vec![]);
        probe.completions.borrow()[0].cancel();
        make_plane(&ctx, &mut app, &mut d);
        assert!(matches!(d.read, Read::Retired) && d.image.is_none());
        assert_eq!(d.out, d.base);
        let (state, _) = frame_with(
            &ctx,
            &mut app,
            &mut d,
            Some(&mut clipboard),
            key(Key::Enter),
        );
        assert_eq!(state, DialogState::Closed(DialogResult::Ok));
        assert_eq!(probe.reads.get(), 2);
        probe.completions.borrow()[1].cancel();
        finish(&ctx, &mut app);
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo);
        assert_eq!(layer_bytes(&app, None), before);
        assert!(app.last_effect.is_none());
        TestImageReads::clear();
    }
}
