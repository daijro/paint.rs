//! A floating window: the forms' frame, title bar, close button, dragging with snapping, edge
//! resizing, Paint.NET's snap layout against the canvas area, translucency and its persisted
//! state. And the modal dialogs' frame (`dialog_frame`) with its mover.
//! Port of: js/src/interface/window/window/{AbstractWindow,WebWindow}.js,
//! js/src/interface/window/DialogMover.js, css/window.css (`.window`, `.title-bar`,
//! `.window-close-button`, `.window-resize-*`, "Consistent form density", `.app-dialog*`, the
//! `modal-dialog-attention` keyframes), css/theme.css (`.window-over-image` opacity)
//! Reference: paintdotjs @ b4e6fc5
//! Paint.NET's snap descriptions replace paintdotjs's proportional anchors; inactive captions,
//! opacity and fade times are Paint.NET's.
//!
//! The look is paintdotjs's (spec §0.4 Ruling (P1): Windows draws the caption in Paint.NET); the
//! title is 12 DIP. Positions are
//! the border box's: Paint.NET's visible frame keeps `WINDOW_INSET` from W on every side (§2.4
//! step 4), so the right and bottom placements subtract the border box, not paintdotjs's CSS
//! size (§5.2's formula, which kept its 10-px margin).
//!
//! Paint.NET's dialogs are native forms, so their frames, close button and attention flash keep
//! paintdotjs's look; their text is 12 DIP.
use egui::{Color32, CursorIcon, Id, Pos2, Rect, Sense, Ui, Vec2};

use super::shadow::{self, BoxShadow, TexturePixels};
use super::text::{self, FontSpec};
use super::{img, px};
use crate::theme::{self, *};

/// What a resize handle moves (`.window-resize-<direction>`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Edges {
    n: bool,
    e: bool,
    s: bool,
    w: bool,
}

/// `createResizeHandles`' directions in DOM order (later ones are on top).
const HANDLES: [(&str, Edges); 8] = {
    const fn e(n: bool, e: bool, s: bool, w: bool) -> Edges {
        Edges { n, e, s, w }
    }
    [
        ("n", e(true, false, false, false)),
        ("e", e(false, true, false, false)),
        ("s", e(false, false, true, false)),
        ("w", e(false, false, false, true)),
        ("ne", e(true, true, false, false)),
        ("se", e(false, true, true, false)),
        ("sw", e(false, false, true, true)),
        ("nw", e(true, false, false, true)),
    ]
};

/// `resizeStart`: the pointer and the window's box when a handle was pressed.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Resize {
    edges: Edges,
    pointer: Pos2,
    pos: Pos2,
    size: Vec2,
}

/// Paint.NET's snap description (`floating-windows.md` §2.3, against W only:
/// §2.6 step 4): the anchored horizontal edge (Bottom, else Top) and vertical edge (Right, else
/// Left) and their offsets from W's edges.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Snap {
    pub right: bool,
    pub bottom: bool,
    pub dx: f32,
    pub dy: f32,
}

impl Snap {
    /// A corner with the default offset (§2.4 step 1 "park").
    pub const fn corner(right: bool, bottom: bool) -> Snap {
        Snap {
            right,
            bottom,
            dx: WINDOW_INSET,
            dy: WINDOW_INSET,
        }
    }

    /// §2.3 "Target W (interior)" for a border box of size `outer`: the anchored edges only, with
    /// the min/max clamps that are the layout's only ones.
    pub fn place(self, w: Rect, outer: Vec2) -> Pos2 {
        Pos2::new(
            if self.right {
                w.min.x.max(w.max.x - self.dx) - outer.x
            } else {
                w.max.x.min(w.min.x + self.dx)
            },
            if self.bottom {
                w.min.y.max(w.max.y - self.dy) - outer.y
            } else {
                w.max.y.min(w.min.y + self.dy)
            },
        )
    }

    /// §2.6 pass 2 / §5.3: the description a border box at `r` takes in W — an edge within
    /// `WINDOW_SNAP` of W inset by `WINDOW_INSET` (closed-interval overlap across it; Right before
    /// Left, Bottom before Top) gets the default offset, the free axis keeps its distance from
    /// W's top or left; no close edge: none (the form keeps its absolute position).
    pub fn classify(r: Rect, w: Rect) -> Option<Snap> {
        let wi = w.shrink(WINDOW_INSET);
        let near = |a: f32, b: f32| (a - b).abs() < WINDOW_SNAP;
        let edge = |overlap: bool, far: bool, near_side: bool| {
            overlap
                .then(|| far.then_some(true).or(near_side.then_some(false)))
                .flatten()
        };
        let v = edge(
            r.min.y <= w.max.y && w.min.y <= r.max.y,
            near(r.max.x, wi.max.x),
            near(r.min.x, wi.min.x),
        );
        let h = edge(
            r.min.x <= w.max.x && w.min.x <= r.max.x,
            near(r.max.y, wi.max.y),
            near(r.min.y, wi.min.y),
        );
        (v.is_some() || h.is_some()).then(|| Snap {
            right: v == Some(true),
            bottom: h == Some(true),
            dx: if v.is_some() {
                WINDOW_INSET
            } else {
                r.min.x - w.min.x
            },
            dy: if h.is_some() {
                WINDOW_INSET
            } else {
                r.min.y - w.min.y
            },
        })
    }

    /// `windows.<id>.snap` (§5.4).
    fn to_json(self) -> serde_json::Value {
        serde_json::json!({
            "h": if self.bottom { "bottom" } else { "top" },
            "v": if self.right { "right" } else { "left" },
            "dx": json_number(self.dx),
            "dy": json_number(self.dy),
        })
    }

    fn from_json(v: &serde_json::Value) -> Option<Snap> {
        let side = |k: &str, near: &str, far: &str| match v[k].as_str() {
            Some(s) if s == near => Some(false),
            Some(s) if s == far => Some(true),
            _ => None,
        };
        let offset = |k: &str| v[k].as_f64().filter(|d| d.is_finite()).map(|d| d as f32);
        Some(Snap {
            right: side("v", "left", "right")?,
            bottom: side("h", "top", "bottom")?,
            dx: offset("dx")?,
            dy: offset("dy")?,
        })
    }
}

/// An `f32` as the shortest decimal that reads back to it (5.6, not 5.599999904632568).
fn json_number(v: f32) -> serde_json::Value {
    serde_json::json!(v.to_string().parse::<f64>().unwrap_or(0.0))
}

/// The translucency fade (`ui-drawn-graphics.md` Addendum: to opaque over 0.15 s, to translucent
/// over 0.5 s) on paintdotjs's `ease-out` curve, on egui's input time. Like a CSS transition it
/// restarts from the current opacity when the target flips, so a reversal mid-fade does not jump.
#[derive(Clone, Copy, Debug)]
struct Fade {
    from: f32,
    translucent: bool,
    start: f64,
}

impl Fade {
    const OPAQUE: Fade = Fade {
        from: 1.0,
        translucent: false,
        start: f64::NEG_INFINITY,
    };

    /// The opacity at `now`, and whether the fade has ended.
    fn at(self, now: f64) -> (f32, bool) {
        let (to, time) = if self.translucent {
            (WINDOW_TRANSLUCENT_OPACITY, WINDOW_FADE.1)
        } else {
            (1.0, WINDOW_FADE.0)
        };
        let t = ((now - self.start) / time as f64) as f32;
        if t >= 1.0 {
            (to, true)
        } else {
            (self.from + (to - self.from) * ease_out(t.max(0.0)), false)
        }
    }

    fn update(&mut self, translucent: bool, now: f64) -> (f32, bool) {
        if translucent != self.translucent {
            *self = Fade {
                from: self.at(now).0,
                translucent,
                start: now,
            };
        }
        self.at(now)
    }
}

/// A `WebWindow`. `size` is the CSS `width`/`height` (the content box; the border box is 2 px
/// larger), `snap` Paint.NET's description against W (None: an absolute position), `resizable`
/// the minimum size when the edges resize it.
pub struct FloatingWindow {
    pub id: &'static str,
    pub size: Vec2,
    pub snap: Option<Snap>,
    /// `isOpen()`.
    pub visible: bool,
    pub resizable: Option<Vec2>,
    pub title: String,
    /// Paint.NET's opacity condition this frame (the forms decide it): fade toward translucent.
    pub translucent: bool,
    /// The active image's screen rect (a translucent window over it shows Paint.NET's pixels).
    pub image: Option<Rect>,
    /// Where the canvas overlays draw (selection outline spans, tool handle boxes): Paint.NET's
    /// pixels too, also where they reach past the image (a moved selection).
    pub overlays: Vec<Rect>,
    /// `this.x`, `this.y`: the border box's top-left as shown (overlay coordinates: clamped
    /// into the overlay, on whole device pixels).
    pos: Pos2,
    /// Where an unsnapped form belongs (`floating-windows.md` §2.5 step 4: it keeps its screen
    /// position): set by a drag, a resize and `restore`, never by the overlay clamp; persisted.
    home: Pos2,
    /// A saved paintdotjs anchor, placed once by the first `place` (§5.3 migration).
    legacy_anchor: Option<Vec2>,
    /// `dragStartX/Y`: the pointer minus the position at the title bar press.
    drag: Option<Vec2>,
    resize: Option<Resize>,
    /// W, the size, the overlay and the scale the window was last placed for.
    placed_for: Option<(Rect, Vec2, Rect, f32)>,
    fade: Fade,
    /// The pointer position last moved to (a drag or resize runs on pointer moves).
    last_pointer: Option<Pos2>,
}

/// What happened to a window this frame.
#[derive(Clone, Copy, Debug)]
pub struct WindowResponse {
    /// The border box.
    pub rect: Rect,
    /// The close button was clicked: the window closed itself.
    pub closed: bool,
}

/// CSS `ease-out` (`cubic-bezier(0, 0, .58, 1)`).
fn ease_out(t: f32) -> f32 {
    super::scroll::cubic_bezier([0.0, 0.0, 0.58, 1.0], t)
}

impl FloatingWindow {
    pub fn new(id: &'static str, size: Vec2, snap: Option<Snap>, resizable: Option<Vec2>) -> Self {
        Self {
            id,
            size,
            snap,
            visible: true,
            resizable,
            title: String::new(),
            translucent: false,
            image: None,
            overlays: vec![],
            pos: Pos2::ZERO,
            home: Pos2::ZERO,
            legacy_anchor: None,
            drag: None,
            resize: None,
            placed_for: None,
            fade: Fade::OPAQUE,
            last_pointer: None,
        }
    }

    /// `restoreState` (`floating-windows.md` §2.4 step 2, §5.4): the saved `windows.<id>`
    /// object's finite, positive sizes (a resizable form never below its minimum, §2.7), its
    /// position and description; a paintdotjs state with only `anchorX`/`anchorY` keeps them for
    /// the first `place` (§5.3; a missing one is this form's default corner). Returns `visible
    /// !== false`.
    pub fn restore(&mut self, state: &serde_json::Value) -> bool {
        if !state.is_object() {
            return true;
        }
        let num = |k: &str| {
            state[k]
                .as_f64()
                .filter(|v| v.is_finite())
                .map(|v| v as f32)
        };
        if let Some(w) = num("width").filter(|&v| v > 0.0) {
            self.size.x = w;
        }
        if let Some(h) = num("height").filter(|&v| v > 0.0) {
            self.size.y = h;
        }
        if let Some(min) = self.resizable {
            self.size = self.size.max(min);
        }
        let pos = num("x").zip(num("y"));
        let snap = Snap::from_json(&state["snap"]);
        if let Some((x, y)) = pos {
            self.home = Pos2::new(x, y);
            self.snap = snap;
        } else if snap.is_some() {
            self.snap = snap;
        } else if num("anchorX").is_some() || num("anchorY").is_some() {
            let corner = self.snap.unwrap_or(Snap::corner(false, false));
            let default = |far: bool| if far { 1.0 } else { 0.0 };
            self.legacy_anchor = Some(Vec2::new(
                num("anchorX").unwrap_or(default(corner.right)),
                num("anchorY").unwrap_or(default(corner.bottom)),
            ));
        }
        state["visible"].as_bool() != Some(false)
    }

    /// `saveState`'s object (§5.4): the position — an unsnapped form's home; a snapped one's is
    /// informational, its description places it — and the description (`null`: absolute).
    pub fn state(&self) -> serde_json::Value {
        let at = if self.snap.is_some() {
            self.pos
        } else {
            self.home
        };
        serde_json::json!({
            "visible": self.visible,
            "width": json_number(self.size.x),
            "height": json_number(self.size.y),
            "x": json_number(at.x),
            "y": json_number(at.y),
            "snap": self.snap.map(Snap::to_json),
        })
    }

    /// The border box.
    pub fn rect(&self) -> Rect {
        Rect::from_min_size(self.pos, self.size + Vec2::splat(2.0 * WINDOW_BORDER))
    }

    /// The border box and the resize handles outside it (what a press can land on).
    pub fn hit_rect(&self) -> Rect {
        let outset = if self.resizable.is_some() {
            WINDOW_RESIZE_OUTSET - WINDOW_BORDER
        } else {
            0.0
        };
        self.rect().expand(outset)
    }

    /// The border box's size.
    fn outer(&self) -> Vec2 {
        self.size + Vec2::splat(2.0 * WINDOW_BORDER)
    }

    /// `setPosition` from a drag, a resize or a migration: `p` becomes the home position, then
    /// shows.
    pub fn set_position(&mut self, p: Pos2, overlay: Rect, ppp: f32) {
        self.home = p;
        self.show_at(p, overlay, ppp);
    }

    /// Where the window shows for `p`: clamped into the overlay (by the CSS size, so the border
    /// may overhang by 2 px — quirk: paintdotjs behaviour no doc analyses; Paint.NET clamps
    /// nothing, §2.3, so the clamp is a platform deviation, §4, and leaves the home position
    /// alone), then on whole device pixels as a Win32 window is.
    fn show_at(&mut self, p: Pos2, overlay: Rect, ppp: f32) {
        let x = p.x.min(overlay.max.x - self.size.x).max(overlay.min.x);
        let y = p.y.min(overlay.max.y - self.size.y).max(overlay.min.y);
        let round = |v: f32| (pdn_core::js::math_round(f64::from(v * ppp)) / f64::from(ppp)) as f32;
        self.pos = Pos2::new(round(x), round(y));
    }

    /// Paint.NET's re-park (`floating-windows.md` §2.3, §2.5, §2.7, §5.2): whenever W, the size,
    /// the overlay or the scale changed — a window resize, rulers, scrollbars, More/Less, a
    /// resize handle — a snapped form is placed by its description and an unsnapped one keeps
    /// its position (both then clamped). Open or not: closed forms keep a position for snapping.
    /// A saved paintdotjs anchor is placed once and classified (§5.3). True when it ran.
    pub fn place(&mut self, w: Rect, overlay: Rect, ppp: f32) -> bool {
        let key = (w, self.size, overlay, ppp);
        if self.placed_for == Some(key) {
            return false;
        }
        self.placed_for = Some(key);
        if let Some(anchor) = self.legacy_anchor.take() {
            // `applyAnchor` within paintdotjs's view bounds.
            let v = w.shrink(WINDOW_VIEW_MARGIN);
            self.set_position(v.min + anchor * (v.max - self.size - v.min), overlay, ppp);
            self.snap = Snap::classify(self.rect(), w);
        }
        let p = self.snap.map_or(self.home, |s| s.place(w, self.outer()));
        self.show_at(p, overlay, ppp);
        true
    }

    /// Gives the window `snap`, placed by the next `place` (a reset, §2.8).
    pub fn park(&mut self, snap: Snap) {
        self.snap = Some(snap);
        self.legacy_anchor = None;
        self.placed_for = None;
    }

    /// A move step (`floating-windows.md` §2.6): the pointer's position, snapped to the outer
    /// edges of `others` (the other forms' border boxes in snap-dictionary order; closed ones
    /// count, §4) where the boxes overlap across the edge, within `WINDOW_SNAP` (twice that once
    /// one snapped), keeping paintdotjs's 7.5-DIP gap (OQ4); then to W (last, so it wins). Only
    /// the W snap is kept as the description (§2.6 step 4).
    pub fn set_position_aligned(
        &mut self,
        p: Pos2,
        w: Rect,
        overlay: Rect,
        others: &[Rect],
        ppp: f32,
    ) {
        let mut r = Rect::from_min_size(p, self.outer());
        let gap = WINDOW_SNAP / 2.0;
        let mut proximity = WINDOW_SNAP;
        for o in others {
            let near = |a: f32, b: f32| (a - b).abs() < proximity;
            let mut hit = false;
            if r.min.y <= o.max.y && o.min.y <= r.max.y {
                let to = if near(r.min.x, o.max.x) {
                    Some(o.max.x + gap)
                } else if near(r.max.x, o.min.x) {
                    Some(o.min.x - gap - r.width())
                } else {
                    None
                };
                if let Some(x) = to {
                    r = r.translate(Vec2::new(x - r.min.x, 0.0));
                    hit = true;
                }
            }
            if r.min.x <= o.max.x && o.min.x <= r.max.x {
                let to = if near(r.min.y, o.max.y) {
                    Some(o.max.y + gap)
                } else if near(r.max.y, o.min.y) {
                    Some(o.min.y - gap - r.height())
                } else {
                    None
                };
                if let Some(y) = to {
                    r = r.translate(Vec2::new(0.0, y - r.min.y));
                    hit = true;
                }
            }
            if hit {
                proximity = 2.0 * WINDOW_SNAP;
            }
        }
        self.snap = Snap::classify(r, w);
        match self.snap {
            Some(s) => self.show_at(s.place(w, self.outer()), overlay, ppp),
            None => self.set_position(r.min, overlay, ppp),
        }
    }

    /// `resizeToPointer` from a handle press: the dragged edges follow the pointer, held to the
    /// minimum size and the overlay, rounded (the next `place` re-parks a snapped form, §2.7).
    fn resize_to(&mut self, r: Resize, pointer: Pos2, overlay: Rect, ppp: f32) {
        let d = pointer - r.pointer;
        let (mut x, mut y, mut w, mut h) = (r.pos.x, r.pos.y, r.size.x, r.size.y);
        if r.edges.e {
            w = r.size.x + d.x;
        }
        if r.edges.s {
            h = r.size.y + d.y;
        }
        if r.edges.w {
            w = r.size.x - d.x;
            x = r.pos.x + d.x;
        }
        if r.edges.n {
            h = r.size.y - d.y;
            y = r.pos.y + d.y;
        }
        let min = self.resizable.unwrap_or(WINDOW_DEFAULT_MIN.into());
        w = w.max(min.x);
        h = h.max(min.y);
        if r.edges.w {
            x = r.pos.x + r.size.x - w;
        }
        if r.edges.n {
            y = r.pos.y + r.size.y - h;
        }
        let (ox, oy) = (x - overlay.min.x, y - overlay.min.y);
        if ox < 0.0 {
            w += ox;
            x = overlay.min.x;
        }
        if oy < 0.0 {
            h += oy;
            y = overlay.min.y;
        }
        // The far-side clamps skip an axis snapped to W's far edge: the re-park keeps that edge
        // inside and moves the near one (§2.7), which the clamp would stop at the overlay.
        // Those axes are held instead by the room before the anchored edge.
        let (far_x, far_y) = self.snap.map_or((false, false), |s| (s.right, s.bottom));
        w = if far_x {
            w.min(r.pos.x + r.size.x - overlay.min.x)
        } else {
            w.min(overlay.max.x - x)
        };
        h = if far_y {
            h.min(r.pos.y + r.size.y - overlay.min.y)
        } else {
            h.min(overlay.max.y - y)
        };
        w = w.max(min.x);
        h = h.max(min.y);
        let round = |v: f32| pdn_core::js::math_round(v as f64) as f32;
        self.size = Vec2::new(round(w), round(h)).max(min);
        self.set_position(Pos2::new(round(x), round(y)), overlay, ppp);
    }

    /// The resize handles' rects: 7 px edges and 11 px corners, 4 px outside the padding box.
    fn handle_rects(&self) -> [Rect; 8] {
        resize_handles(self.rect(), WINDOW_BORDER)
    }

    /// Shows the window in the caller's layer (over what is painted there, under what follows):
    /// its shadow, frame, title bar and close button, `body` in the content box, the resize
    /// handles. `w` is W (the canvas area, `floating-windows.md` §2.3), `overlay`
    /// `#windowOverlay`, `others` the other forms' border boxes for snapping, `pixels` the CPU
    /// pixels of textures under the shadow.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        w: Rect,
        overlay: Rect,
        others: &[Rect],
        pixels: TexturePixels,
        body: impl FnOnce(&mut Ui),
    ) -> WindowResponse {
        let ctx = ui.ctx().clone();
        let theme = theme::current(&ctx);
        let ppp = ctx.pixels_per_point();
        let id = Id::unique(("paintrs.window", self.id));
        // The document mousemove/mouseup listeners: a drag or resize follows pointer moves and
        // ends on release.
        let (down, pointer, now, app_active) = ui.input(|i| {
            let p = &i.pointer;
            (p.primary_down(), p.hover_pos(), i.time, i.focused)
        });
        let moved = pointer.filter(|p| self.last_pointer != Some(*p));
        self.last_pointer = pointer;
        if !down {
            self.drag = None;
            self.resize = None;
        }
        if let Some(p) = moved {
            if let Some(r) = self.resize {
                self.resize_to(r, p, overlay, ppp);
            } else if let Some(grab) = self.drag {
                self.set_position_aligned(p - grab, w, overlay, others, ppp);
            }
        }
        // A resize re-parks in the same frame.
        self.place(w, overlay, ppp);

        let outer = px::snap_rect(self.rect(), ppp);
        let (opacity, settled) = if theme::animations_enabled(&ctx) {
            self.fade.update(self.translucent, now)
        } else {
            self.fade = Fade {
                from: if self.translucent {
                    WINDOW_TRANSLUCENT_OPACITY
                } else {
                    1.
                },
                translucent: self.translucent,
                start: f64::NEG_INFINITY,
            };
            self.fade.at(now)
        };
        if !settled {
            ctx.request_repaint();
        }
        let translucent = opacity < 1.0;
        let under = translucent.then(|| shadow::capture_underlay(ui, outer));
        // Translucent, the shadow is part of the group that takes the opacity.
        shadow::paint_group_box_shadow(
            ui,
            id.with("shadow"),
            outer,
            BoxShadow {
                offset: Vec2::ZERO,
                blur: WINDOW_SHADOW_BLUR,
                color: WINDOW_SHADOW_COLOR,
            },
            opacity,
            pixels,
        );
        // The window hides the text and canvas recorded under it, so its pixels compare exactly
        // (when translucent too: the canvas then shows through at 25 %).
        text::occlude(&ctx, outer);
        let painter = ui.painter().clone();
        // `background-color` under the translucent `border`.
        let bw = px::border_width(WINDOW_BORDER, ppp);
        painter.rect_filled(
            outer,
            0.0,
            px::over(theme.window_background, theme.window_border),
        );
        painter.rect_filled(outer.shrink(bw), 0.0, theme.window_background);
        // The window takes the pointer from what lies under it (the canvas).
        ui.interact(outer, id.with("body"), Sense::click_and_drag());

        // .title-bar
        let inner = self.rect().shrink(WINDOW_BORDER);
        let (pad_y, pad_right, pad_left) = WINDOW_TITLE_PADDING;
        let title_bar = Rect::from_min_size(
            inner.min + Vec2::new(0.0, WINDOW_TITLE_MARGIN_TOP),
            Vec2::new(inner.width(), WINDOW_TITLE_HEIGHT + 2.0 * pad_y),
        );
        // The fill sits right inside the whole-pixel border (the layout keeps its DIP inset).
        let title_snapped = px::snap_rect(title_bar, ppp);
        let title_snapped = Rect::from_min_max(
            Pos2::new(outer.min.x + bw, title_snapped.min.y),
            Pos2::new(outer.max.x - bw, title_snapped.max.y),
        );
        painter.rect_filled(title_snapped, 0.0, theme.title_bar);
        let title_response = ui.interact(title_snapped, id.with("title"), Sense::click_and_drag());
        let spec = FontSpec::new(FORM_FONT_SIZE);
        let content_w = inner.width() - pad_left - pad_right;
        let line = spec.resolved_line_height(ppp);
        // Blink's inline size of the caption (`text::draw_ellipsized` decides overflow on it).
        let title_w = text::layout_width(&painter, &self.title, spec)
            .min(content_w - WINDOW_TITLE_RESERVE)
            .max(0.0);
        let title_box = Rect::from_min_size(
            Pos2::new(
                title_bar.min.x + pad_left,
                title_bar.min.y + pad_y + (WINDOW_TITLE_HEIGHT - line) / 2.0,
            ),
            Vec2::new(title_w, line),
        );
        let close_rect = Rect::from_min_size(
            Pos2::new(
                title_bar.max.x - pad_right - WINDOW_CLOSE,
                title_bar.min.y + pad_y + (WINDOW_TITLE_HEIGHT - WINDOW_CLOSE) / 2.0,
            ),
            Vec2::splat(WINDOW_CLOSE),
        );
        let close_snapped = px::snap_rect(close_rect, ppp);
        let close = ui.interact(close_snapped, id.with("close"), Sense::click());
        let (title_color, close_fill, glyph) = caption_colors(&theme, app_active, close.hovered());
        text::draw_ellipsized(&painter, title_box, &self.title, spec, title_color);

        // .window-close-button: right in the bar's content box, centred (it overflows the 20 px).
        if let Some(fill) = close_fill {
            painter.rect_filled(close_snapped, 0.0, fill);
        }
        paint_close_glyph(&painter, close_rect, glyph);

        // .content
        let content = Rect::from_min_max(Pos2::new(inner.min.x, title_bar.max.y), inner.max);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(content).id_salt(self.id));
        child.set_clip_rect(content.intersect(ui.clip_rect()));
        body(&mut child);

        // .window-resize-* (z-index 20: over the content).
        let mut handle_pressed = None;
        if self.resizable.is_some() {
            for ((name, edges), r) in HANDLES.iter().zip(self.handle_rects()) {
                // Clicks too: the 4-px outset takes the pointer from what lies under it.
                let response = ui.interact(r, id.with(("resize", *name)), Sense::click_and_drag());
                if response.hovered() || self.resize.is_some_and(|z| z.edges == *edges) {
                    ctx.set_cursor_icon(cursor(*edges));
                }
                if response.hovered() {
                    handle_pressed = Some(*edges);
                }
            }
        }
        if title_response.hovered() && self.resize.is_none() {
            ctx.set_cursor_icon(CursorIcon::Move);
        }

        // The mousedown listeners: a handle starts a resize, the title bar (and its children) a
        // drag.
        let pressed = ui.input(|i| i.pointer.primary_pressed());
        if let (true, Some(p)) = (pressed, pointer) {
            if let Some(edges) = handle_pressed {
                self.drag = None;
                self.resize = Some(Resize {
                    edges,
                    pointer: p,
                    pos: self.pos,
                    size: self.size,
                });
            } else if title_response.hovered() || close.hovered() {
                self.drag = Some(p - self.pos);
            }
        }
        let closed = close.clicked();
        if closed {
            self.visible = false;
            self.drag = None;
            self.resize = None;
        }
        // The canvas shows through at 25 %: over the image and its drop shadow (the document,
        // checkerboard, edges and shadow are Paint.NET's, canvas-rendering.md §2.5–2.7) the
        // window's pixels, its text too, are Paint.NET's (R21 (c); controller ruling on Task 20
        // concern 3b); the flat surround beyond stays compared.
        if let (true, Some(img)) = (translucent, self.image) {
            let over = img.expand(CANVAS_SHADOW_REACH).intersect(outer);
            text::record_pdn_rect_over(&ctx, over);
        }
        // The same where the overlays draw past the image (Task 27, ruling R67: only the outline
        // spans and handle boxes, so the window's own text and controls stay compared elsewhere).
        if translucent {
            for o in &self.overlays {
                let o = o.intersect(outer);
                if o.is_positive() {
                    text::record_pdn_rect_over(&ctx, o);
                }
            }
        }
        match &under {
            Some(under) => shadow::paint_veil(ui, id, under, outer, opacity, pixels),
            None => shadow::forget(&ctx, id.with("veil")),
        }
        WindowResponse {
            rect: outer,
            closed,
        }
    }
}

/// The caption's title colour, close-box fill (None: the bar shows) and glyph colour:
/// paintdotjs's while the app window has OS focus; without it Paint.NET's inactive caption (every
/// form at once, whichever has focus: `floating-windows.md` §2.9, §5.6) — grey title and glyph,
/// no red box — keeping paintdotjs's hover box (OQ3).
fn caption_colors(
    theme: &theme::Theme,
    app_active: bool,
    close_hovered: bool,
) -> (Color32, Option<Color32>, Color32) {
    let (dark, light) = WINDOW_INACTIVE_TEXT;
    let grey = if theme.scheme == Scheme::Light {
        light
    } else {
        dark
    };
    let title = if app_active {
        theme.menu_item_text
    } else {
        grey
    };
    match (close_hovered, app_active) {
        (true, _) => (
            title,
            Some(theme.close_button_hover),
            theme.close_button_text,
        ),
        (false, true) => (title, Some(theme.close_button), theme.close_button_text),
        (false, false) => (title, None, grey),
    }
}

fn resize_handles(rect: Rect, border: f32) -> [Rect; 8] {
    let p = rect.shrink(border);
    let (o, e, c) = (
        WINDOW_RESIZE_OUTSET,
        WINDOW_RESIZE_EDGE,
        WINDOW_RESIZE_CORNER,
    );
    let r = |x: f32, y: f32, w: f32, h: f32| Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h));
    [
        r(p.min.x + e, p.min.y - o, p.width() - 2.0 * e, e),
        r(p.max.x + o - e, p.min.y + e, e, p.height() - 2.0 * e),
        r(p.min.x + e, p.max.y + o - e, p.width() - 2.0 * e, e),
        r(p.min.x - o, p.min.y + e, e, p.height() - 2.0 * e),
        r(p.max.x + o - c, p.min.y - o, c, c),
        r(p.max.x + o - c, p.max.y + o - c, c, c),
        r(p.min.x - o, p.max.y + o - c, c, c),
        r(p.min.x - o, p.min.y - o, c, c),
    ]
}

fn cursor(e: Edges) -> CursorIcon {
    match (e.n || e.s, e.e || e.w) {
        (true, false) => CursorIcon::ResizeVertical,
        (false, true) => CursorIcon::ResizeHorizontal,
        _ if (e.n && e.e) || (e.s && e.w) => CursorIcon::ResizeNeSw,
        _ => CursorIcon::ResizeNwSe,
    }
}

/// The close button's "x": css/window.css `font: bold 14px 'Arial'` (Noto Sans here, ruling (P2)
/// Task 19), centred, `padding-bottom: 1px` lifting the line box.
/// ponytail: Chromium synthesizes the bold from Noto Sans Regular; this draws the Regular face.
fn paint_close_glyph(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let spec = crate::kit::fonts::face_spec(DOCUMENT_CLOSE_FAMILY, WINDOW_CLOSE_FONT);
    let w = text::measure(painter, "x", spec);
    let ppp = painter.pixels_per_point();
    let line = spec.resolved_line_height(ppp);
    // Blink centres the line box in device px (its layout runs at the device scale).
    let at = Pos2::new(
        rect.center().x - w / 2.0,
        rect.min.y + (((rect.height() - 1.0 - line) / 2.0) * ppp).ceil() / ppp,
    );
    text::draw(painter, at, "x", spec, color);
}

// ---- Modal dialog frame (`.app-dialog`, DialogMover) ----

/// A dialog frame's state between frames, in egui memory under the dialog's `Ui` scope id (each
/// dialog of `dialogs::DialogStack` shows in its own child `Ui`). The stack seeds `pos` from
/// `dialogs.<persistence id>`, reads `rect` and `dropped`, and sets `attention`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DialogFrameState {
    /// DialogMover's `position: fixed` top-left (the border box); `None` while centred.
    pub pos: Option<Pos2>,
    /// The pointer minus the box's top-left at the title bar press (`offsetX/Y`).
    grab: Option<Vec2>,
    /// The border box last shown.
    pub rect: Rect,
    /// DialogMover.stop ran this frame after a drag (the position persists).
    pub dropped: bool,
    /// The attention flash's keyframe amount this frame: 0 = the normal look, 1 = the 50 %
    /// keyframe (no shadow, #606060 title).
    pub attention: f32,
    /// A popup of the dialog outside its box (an open select's list): presses on it are the
    /// dialog's, not the backdrop's.
    pub popup: Rect,
    /// Opt-in border-box minimum and current size. Ordinary dialogs stay fixed-size.
    pub min_size: Option<Vec2>,
    pub size: Option<Vec2>,
    resize: Option<Resize>,
}

impl Default for DialogFrameState {
    fn default() -> Self {
        Self {
            pos: None,
            grab: None,
            rect: Rect::NOTHING,
            dropped: false,
            attention: 0.0,
            popup: Rect::NOTHING,
            min_size: None,
            size: None,
            resize: None,
        }
    }
}

impl DialogFrameState {
    fn id(ui_id: Id) -> Id {
        ui_id.with("paintrs.dialog_frame")
    }

    /// The state of the dialog shown in the `Ui` with id `ui_id`.
    pub fn get(ctx: &egui::Context, ui_id: Id) -> DialogFrameState {
        ctx.data(|d| d.get_temp(Self::id(ui_id)))
            .unwrap_or_default()
    }

    pub fn set(self, ctx: &egui::Context, ui_id: Id) {
        ctx.data_mut(|d| d.insert_temp(Self::id(ui_id), self));
    }

    /// Drops the state and the shadow texture of a dialog that closed.
    pub fn forget(ctx: &egui::Context, ui_id: Id) {
        ctx.data_mut(|d| d.remove::<DialogFrameState>(Self::id(ui_id)));
        shadow::forget(ctx, Self::id(ui_id).with("shadow"));
    }
}

/// What a dialog frame did this frame.
#[derive(Clone, Copy, Debug)]
pub struct DialogFrameResponse {
    /// The border box.
    pub rect: Rect,
    /// The content box between the title bar and the footer.
    pub body: Rect,
    /// The title bar's close button was clicked (or activated from the keyboard).
    pub closed: bool,
    /// The close button's widget id (a dialog may give it the keyboard focus).
    pub close_id: Id,
    /// The optional title-bar help button was activated.
    pub help_clicked: bool,
}

/// DialogMover.setPosition: the box held inside the backdrop (its left/top edge wins when it is
/// larger).
fn clamp_dialog(p: Pos2, size: Vec2, bounds: Rect) -> Pos2 {
    let max = (bounds.max - size).max(bounds.min);
    Pos2::new(
        p.x.min(max.x).max(bounds.min.x),
        p.y.min(max.y).max(bounds.min.y),
    )
}

/// CSS `ease-in-out` (`cubic-bezier(.42, 0, .58, 1)`).
fn ease_in_out(t: f32) -> f32 {
    super::scroll::cubic_bezier([0.42, 0.0, 0.58, 1.0], t)
}

/// The `modal-dialog-attention-*` keyframe amount `t` ms into the flash: each 160-ms run goes
/// 0 → 1 → 0 with `ease-in-out` per half; 0 once the five runs are over.
pub fn attention_amount(t: f64) -> f32 {
    let (period, runs, ..) = DIALOG_ATTENTION_MS;
    if !(0.0..period * runs as f64).contains(&t) {
        return 0.0;
    }
    let p = ((t % period) / period) as f32;
    if p < 0.5 {
        ease_in_out(p * 2.0)
    } else {
        1.0 - ease_in_out(p * 2.0 - 1.0)
    }
}

/// `color` faded toward `to` by `k` (CSS colour interpolation of opaque colours).
fn lerp_color(color: Color32, to: Color32, k: f32) -> Color32 {
    let m = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * k).round() as u8;
    Color32::from_rgb(
        m(color.r(), to.r()),
        m(color.g(), to.g()),
        m(color.b(), to.b()),
    )
}

/// A glyph of `spec` centred in `rect` whose bottom padding is `pad_bottom`, on a `line-height`
/// of the font size (`.app-dialog-close`: `line-height: 1; padding-bottom: 2px`).
fn paint_centered_glyph(
    painter: &egui::Painter,
    rect: Rect,
    glyph: &str,
    spec: FontSpec,
    pad_bottom: f32,
    color: Color32,
) {
    let spec = spec.line_height(spec.size);
    let w = text::measure(painter, glyph, spec);
    let at = Pos2::new(
        rect.center().x - w / 2.0,
        rect.min.y + (rect.height() - pad_bottom - spec.size) / 2.0,
    );
    text::draw(painter, at, glyph, spec, color);
}

/// Paints a dialog part into its box (the content box, the footer box).
pub type DialogPainter<'a> = &'a mut dyn FnMut(&mut Ui, Rect);

/// The `.app-dialog` frame (css/window.css) in its transparent full-window backdrop, which takes
/// every press and drag beneath it (ModalDialogController blocks the app under the dialog; the
/// dialog stack withholds the shell's raw input). `title` and the 20-DIP `icon` (an
/// `assets/icons` file) in the 31-DIP title bar with its 46×30 close button; `body` = (the border
/// box width, the content height) and the content's painter, given the content box; `footer`
/// paints into the 45-DIP `.app-dialog-footer` box (inside its top border). DialogMover: a drag
/// from the title bar (not its close button) moves the box, held inside the backdrop. State
/// lives under `ui.scope_id()` (`DialogFrameState`).
pub fn dialog_frame(
    ui: &mut Ui,
    title: &str,
    icon: Option<&str>,
    body: (Vec2, DialogPainter),
    footer: Option<DialogPainter>,
) -> DialogFrameResponse {
    dialog_frame_with(ui, title, icon, false, body, footer, Footer::DEFAULT)
}

/// A dialog's `.app-dialog-footer` box: its height and whether it has its top border
/// (`.effect-config-dialog .effect-config-footer { border-top: 0 }`, `.layer-properties-dialog
/// .app-dialog-footer { min-height: 51px; border-top: 0 }`).
#[derive(Clone, Copy, Debug)]
pub struct Footer {
    pub height: f32,
    pub border: bool,
}

impl Footer {
    pub const DEFAULT: Footer = Footer {
        height: DIALOG_FOOTER_HEIGHT,
        border: true,
    };
}

/// Dialog chrome keeps actual focus after pointer acceptance, while Chromium's UA outline
/// follows `:focus-visible`. Task-dialog choices explicitly use that pseudo-class too
/// (window.css:1761). Native frame decoration remains the documented Task59 calibration limit.
pub(crate) fn dialog_focus_visible(ctx: &egui::Context) -> bool {
    let id = Id::NULL.with("dialog-focus-visible");
    let mut visible = ctx.data_mut(|d| d.get_temp::<bool>(id).unwrap_or(true));
    ctx.input(|i| {
        for event in &i.events {
            match event {
                egui::Event::Key { pressed: true, .. } => visible = true,
                egui::Event::PointerButton { pressed: true, .. } => visible = false,
                _ => {}
            }
        }
    });
    ctx.data_mut(|d| d.insert_temp(id, visible));
    visible
}

// Modality determines newly acquired focus. A canvas/title drag that keeps the same
// focused control also keeps its ring; a pointer press on the close button hides it.
fn dialog_close_focus_visible(ctx: &egui::Context, close: &egui::Response, modality: bool) -> bool {
    let id = close.id.with("focus-visible");
    let mut visible = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(modality);
    if close.has_focus() {
        ctx.input(|i| {
            for event in &i.events {
                match event {
                    egui::Event::Key { pressed: true, .. } => visible = true,
                    egui::Event::PointerButton {
                        pos, pressed: true, ..
                    } if close.rect.contains(*pos) => visible = false,
                    _ => {}
                }
            }
        });
    }
    ctx.data_mut(|d| d.insert_temp(id, visible));
    visible
}

/// `dialog_frame` with its footer's `look` and optional help button.
pub fn dialog_frame_with(
    ui: &mut Ui,
    title: &str,
    icon: Option<&str>,
    help: bool,
    body: (Vec2, DialogPainter),
    footer: Option<DialogPainter>,
    look: Footer,
) -> DialogFrameResponse {
    dialog_frame_with_title(
        ui,
        title,
        icon,
        help,
        body,
        footer,
        look,
        (DIALOG_TITLE_BAR, DIALOG_CLOSE.1),
    )
}

/// Settings keeps the shared modal frame and drag behavior with its taller title bar.
#[allow(clippy::too_many_arguments)]
pub fn dialog_frame_with_title(
    ui: &mut Ui,
    title: &str,
    icon: Option<&str>,
    help: bool,
    body: (Vec2, DialogPainter),
    footer: Option<DialogPainter>,
    look: Footer,
    title_size: (f32, f32),
) -> DialogFrameResponse {
    let ctx = ui.ctx().clone();
    let focus_visible = dialog_focus_visible(&ctx);
    let theme = theme::current(&ctx);
    let ppp = ctx.pixels_per_point();
    let ui_id = ui.scope_id();
    let id = DialogFrameState::id(ui_id);
    let mut state = DialogFrameState::get(&ctx, ui_id);
    // .app-dialog-backdrop (`inset: 0`): transparent, over everything painted before it.
    let mut backdrop = ctx.content_rect();
    if ctx.data_mut(|d| {
        d.get_temp::<bool>(egui::Id::unique("desktop-shell"))
            .unwrap_or(false)
    }) {
        backdrop.min.y += crate::theme::DESKTOP_TITLE_HEIGHT;
    }
    // Not focusable: Tab moves between the dialog's controls only.
    ui.interact(backdrop, id.with("backdrop"), Sense::CLICK | Sense::DRAG);
    let focused = ctx.memory(|m| m.focused());

    let ((width, body_h), paint_body) = (body.0.into(), body.1);
    let footer_h = if footer.is_some() { look.height } else { 0.0 };
    let mut size = Vec2::new(
        width,
        title_size.0 + body_h + footer_h + 2.0 * DIALOG_BORDER,
    );
    if let Some(min) = state.min_size {
        size = state.size.unwrap_or(size).max(min);
    }
    // DialogMover: `mousemove` follows the pointer while the button is held; `mouseup` stops.
    let (down, pointer) = ui.input(|i| (i.pointer.primary_down(), i.pointer.hover_pos()));
    state.dropped = false;
    if let Some(resize) = state.resize {
        if !down {
            state.resize = None;
        } else if let (Some(pointer), Some(min)) = (pointer, state.min_size) {
            let delta = pointer - resize.pointer;
            let mut bounds = Rect::from_min_size(resize.pos, resize.size);
            if resize.edges.w {
                bounds.min.x = (resize.pos.x + delta.x).clamp(backdrop.min.x, bounds.max.x - min.x);
            }
            if resize.edges.n {
                bounds.min.y = (resize.pos.y + delta.y).clamp(backdrop.min.y, bounds.max.y - min.y);
            }
            if resize.edges.e {
                bounds.max.x = (bounds.max.x + delta.x)
                    .min(backdrop.max.x)
                    .max(bounds.min.x + min.x);
            }
            if resize.edges.s {
                bounds.max.y = (bounds.max.y + delta.y)
                    .min(backdrop.max.y)
                    .max(bounds.min.y + min.y);
            }
            size = bounds.size();
            state.pos = Some(bounds.min);
        }
    }
    if state.min_size.is_some() {
        state.size = Some(size);
    }
    let body_h = if state.min_size.is_some() {
        size.y - title_size.0 - footer_h - 2.0 * DIALOG_BORDER
    } else {
        body_h
    };
    if let Some(grab) = state.grab {
        if !down {
            state.grab = None;
            state.dropped = true;
        } else if let Some(p) = pointer {
            state.pos = Some(p - grab);
        }
    }
    // Centred by the backdrop's flexbox until moved; a moved box stays clamped (`clampToBounds`
    // on resize).
    let pos = match state.pos {
        Some(p) => clamp_dialog(p, size, backdrop),
        // Clamped too, so a dialog taller than the window keeps its title bar in it.
        None => clamp_dialog(
            backdrop.min + (backdrop.size() - size) / 2.0,
            size,
            backdrop,
        ),
    };
    if state.pos.is_some() {
        state.pos = Some(pos);
    }
    let rect = Rect::from_min_size(pos, size);
    let outer = px::snap_rect(rect, ppp);
    let k = state.attention;

    // box-shadow (animated to `none` by the attention keyframes).
    let (dy, blur, color) = DIALOG_SHADOW;
    let alpha = (color.a() as f32 * (1.0 - k)).round() as u8;
    if alpha > 0 {
        let none: TexturePixels = &|tid| super::img::texture_pixels(&ctx, tid);
        shadow::paint_box_shadow(
            ui,
            id.with("shadow"),
            outer,
            BoxShadow {
                offset: Vec2::new(0.0, dy * (1.0 - k)),
                blur: blur * (1.0 - k),
                color: Color32::from_black_alpha(alpha),
            },
            none,
        );
    } else {
        shadow::forget(&ctx, id.with("shadow"));
    }
    // The dialog is opaque: what was recorded under it is hidden.
    text::occlude(&ctx, outer);
    let painter = ui.painter().clone();
    let bw = px::border_width(DIALOG_BORDER, ppp);
    painter.rect_filled(outer, 0.0, theme.title_bar_border);
    painter.rect_filled(outer.shrink(bw), 0.0, theme.window_background);
    let inner = rect.shrink(DIALOG_BORDER);
    let snap_x = |r: Rect| {
        let s = px::snap_rect(r, ppp);
        Rect::from_min_max(
            Pos2::new(outer.min.x + bw, s.min.y),
            Pos2::new(outer.max.x - bw, s.max.y),
        )
    };

    // .app-dialog-title-bar: the fill, its 1-px bottom border.
    let bar = Rect::from_min_size(inner.min, Vec2::new(inner.width(), title_size.0));
    let bar_px = snap_x(bar);
    let line = |r: Rect, top: bool, fill: Color32| {
        let y = if top { r.min.y } else { r.max.y - bw };
        painter.rect_filled(
            Rect::from_min_size(Pos2::new(r.min.x, y), Vec2::new(r.width(), bw)),
            0.0,
            fill,
        );
    };
    painter.rect_filled(bar_px, 0.0, theme.title_bar);
    line(
        bar_px,
        false,
        px::over(theme.title_bar, theme.window_border),
    );
    // The close button fills the bar's content box at its right.
    let (cw, ch) = (DIALOG_CLOSE.0, title_size.1);
    let close_rect = Rect::from_min_size(Pos2::new(bar.max.x - cw, bar.min.y), Vec2::new(cw, ch));
    // The handle first: its close button lies over it.
    let title_response = ui.interact(bar_px, id.with("title"), Sense::CLICK | Sense::DRAG);
    let close = ui.interact(
        px::snap_rect(close_rect, ppp),
        id.with("close"),
        Sense::click(),
    );
    let help_rect = close_rect.translate(Vec2::new(-cw, 0.0));
    let help_response = help.then(|| {
        let response = ui.interact(
            px::snap_rect(help_rect, ppp),
            id.with("help"),
            Sense::click(),
        );
        ctx.accesskit_node_builder(response.id, |node| {
            node.set_role(egui::accesskit::Role::Button);
            node.set_label("Help");
        });
        super::inputs::take_enter(ui, &response);
        if response.hovered() {
            painter.rect_filled(px::snap_rect(help_rect, ppp), 0.0, theme.menu_item_selected);
        }
        paint_centered_glyph(
            &painter,
            help_rect,
            "?",
            FontSpec::new(DIALOG_FONT_SIZE),
            DIALOG_CLOSE_PADDING_BOTTOM,
            theme.menu_item_text,
        );
        if response.has_focus() && dialog_close_focus_visible(&ctx, &response, focus_visible) {
            let line = px::over(theme.title_bar, theme.window_border);
            focus_ring(&painter, px::snap_rect(help_rect, ppp), bar_px, |y| {
                if y >= bar_px.max.y - bw {
                    line
                } else {
                    theme.title_bar
                }
            });
        }
        response
    });
    // .app-dialog-title: icon, gap, the ellipsized title, centred in the bar's content box.
    let content_h = title_size.0 - DIALOG_BORDER;
    let mut x = bar.min.x + DIALOG_TITLE_PADDING_LEFT;
    if let Some(icon) = icon {
        let r = Rect::from_min_size(
            Pos2::new(x, bar.min.y + (content_h - DIALOG_TITLE_ICON) / 2.0),
            Vec2::splat(DIALOG_TITLE_ICON),
        );
        img::paint_icon(
            &painter,
            icon,
            px::snap_rect(r, ppp),
            theme.title_bar,
            img::Filter::None,
        );
        x = r.max.x + DIALOG_TITLE_GAP;
    }
    let spec = FontSpec::new(DIALOG_FONT_SIZE);
    let line_h = spec.resolved_line_height(ppp);
    let title_box = Rect::from_min_max(
        Pos2::new(x, bar.min.y + (content_h - line_h) / 2.0),
        Pos2::new(
            if help {
                help_rect.min.x
            } else {
                close_rect.min.x
            },
            bar.min.y + (content_h + line_h) / 2.0,
        ),
    );
    let title_color = lerp_color(theme.menu_item_text, DIALOG_ATTENTION_TITLE, k);
    text::draw_ellipsized(&painter, title_box, title, spec, title_color);
    let (glyph_color, close_backdrop) = if close.hovered() {
        painter.rect_filled(
            px::snap_rect(close_rect, ppp),
            0.0,
            theme.close_button_hover,
        );
        (theme.close_button_text, theme.close_button_hover)
    } else {
        (theme.menu_item_text, theme.title_bar)
    };
    super::aa::paint_close_x(
        &painter,
        close_rect.center(),
        ppp,
        glyph_color,
        close_backdrop,
    );
    let close_focus_visible = dialog_close_focus_visible(&ctx, &close, focus_visible);
    if close.has_focus() && close_focus_visible {
        // `:focus-visible` with `outline: auto` (Chromium's ring: 2 px of white straddling the
        // edge on a 1-px #101010 one), inside the dialog's border; the content box paints over
        // what passes the title bar (measured from the effect-dialog references).
        let line = px::over(theme.title_bar, theme.window_border);
        let clip = Rect::from_min_max(snap_x(bar).min, Pos2::new(outer.max.x - bw, bar_px.max.y));
        focus_ring(&painter, px::snap_rect(close_rect, ppp), clip, |y| {
            if y >= bar_px.max.y - bw {
                line
            } else {
                theme.title_bar
            }
        });
    }
    if close.hovered() || help_response.as_ref().is_some_and(|r| r.hovered()) {
        ctx.set_cursor_icon(CursorIcon::PointingHand);
    } else if title_response.hovered() || state.grab.is_some() {
        ctx.set_cursor_icon(CursorIcon::Move);
    }

    // The content.
    let body_rect = Rect::from_min_size(
        Pos2::new(inner.min.x, bar.max.y),
        Vec2::new(inner.width(), body_h),
    );
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body_rect).id_salt("body"));
    child.set_clip_rect(snap_x(body_rect).intersect(ui.clip_rect()));
    paint_body(&mut child, body_rect);

    // .app-dialog-footer: the fill, its 1-px top border.
    if let Some(paint_footer) = footer {
        let f = Rect::from_min_size(
            Pos2::new(inner.min.x, body_rect.max.y),
            Vec2::new(inner.width(), footer_h),
        );
        let f_px = snap_x(f);
        painter.rect_filled(f_px, 0.0, theme.menu_background);
        let top = if look.border {
            line(
                f_px,
                true,
                px::over(theme.menu_background, theme.window_border),
            );
            DIALOG_BORDER
        } else {
            0.0
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(f).id_salt("footer"));
        paint_footer(
            &mut child,
            Rect::from_min_max(Pos2::new(f.min.x, f.min.y + top), f.max),
        );
    }

    let mut resize_edge = None;
    if state.min_size.is_some() {
        for ((name, edges), handle) in HANDLES.iter().zip(resize_handles(rect, DIALOG_BORDER)) {
            // Window decoration takes pointer input without joining the Tab order.
            let response = ui.interact(
                handle,
                id.with(("resize", *name)),
                Sense::CLICK | Sense::DRAG,
            );
            if response.hovered() || state.resize.is_some_and(|r| r.edges == *edges) {
                ctx.set_cursor_icon(cursor(*edges));
            }
            if response.hovered() {
                resize_edge = Some(*edges);
            }
        }
        state.popup = rect.expand(WINDOW_RESIZE_OUTSET - DIALOG_BORDER);
    }
    // DialogMover.start: a primary press on the handle, not on its buttons.
    let pressed = ui.input(|i| i.pointer.any_pressed());
    let on_handle = title_response.hovered()
        && !close.hovered()
        && !help_response.as_ref().is_some_and(|r| r.hovered())
        && resize_edge.is_none();
    if let (true, Some(p)) = (pressed, pointer) {
        if let Some(edges) = resize_edge {
            state.grab = None;
            state.resize = Some(Resize {
                edges,
                pointer: p,
                pos,
                size,
            });
        }
        if on_handle && ui.input(|i| i.pointer.primary_pressed()) {
            state.grab = Some(p - pos);
        }
        // The handle's `preventDefault` and `blockBackgroundEvent` keep the focus where it was
        // (egui surrenders it on a press elsewhere).
        if (on_handle || !rect.contains(p))
            && let Some(f) = focused
        {
            ctx.memory_mut(|m| m.request_focus(f));
        }
    }
    state.rect = rect;
    state.set(&ctx, ui_id);
    DialogFrameResponse {
        rect,
        body: body_rect,
        closed: super::inputs::button_activated(ui, &close),
        close_id: close.id,
        help_clicked: help_response
            .as_ref()
            .is_some_and(|r| super::inputs::button_activated(ui, r)),
    }
}

/// Chromium's `outline: auto` focus ring around the snapped `b`: a white ring from 1 px outside
/// to 1 px inside its edge (radii 3 and 2) on a #101010 one 1 px further out (radius 4), clipped
/// to `clip`, over the flat colour `under(y)` of each row. 4×4 supersampled coverage.
fn focus_ring(painter: &egui::Painter, b: Rect, clip: Rect, under: impl Fn(f32) -> Color32) {
    let ppp = painter.pixels_per_point();
    let d = |r: Rect| Rect::from_min_max(r.min * ppp, r.max * ppp);
    let (b, clip) = (d(b), d(clip));
    let inside = |r: Rect, radius: f32, x: f32, y: f32| {
        if !r.contains(Pos2::new(x, y)) {
            return false;
        }
        let cx = x.clamp(r.min.x + radius, r.max.x - radius);
        let cy = y.clamp(r.min.y + radius, r.max.y - radius);
        (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    };
    let (white_out, white_in, dark_out) = (b.expand(ppp), b.shrink(ppp), b.expand(2.0 * ppp));
    let area = dark_out.intersect(clip);
    for py in area.min.y.round() as i32..area.max.y.round() as i32 {
        for px_ in area.min.x.round() as i32..area.max.x.round() as i32 {
            let (mut white, mut dark) = (0, 0);
            for sy in 0..4 {
                for sx in 0..4 {
                    let (x, y) = (
                        px_ as f32 + (sx as f32 + 0.5) / 4.0,
                        py as f32 + (sy as f32 + 0.5) / 4.0,
                    );
                    if inside(white_out, 3.0 * ppp, x, y) {
                        white += !inside(white_in, 2.0 * ppp, x, y) as u32;
                    } else {
                        dark += inside(dark_out, 4.0 * ppp, x, y) as u32;
                    }
                }
            }
            if white + dark == 0 {
                continue;
            }
            let base = under(py as f32 / ppp);
            let mix = |c: Color32, k: u32, to: Color32| {
                let m = |a: u8, b: u8| {
                    (a as f32 + (b as f32 - a as f32) * k as f32 / 16.0).round() as u8
                };
                Color32::from_rgb(m(c.r(), to.r()), m(c.g(), to.g()), m(c.b(), to.b()))
            };
            let c = mix(
                mix(base, dark, Color32::from_gray(0x10)),
                white,
                Color32::WHITE,
            );
            let at = Pos2::new(px_ as f32, py as f32) / ppp;
            painter.rect_filled(Rect::from_min_size(at, Vec2::splat(1.0 / ppp)), 0.0, c);
        }
    }
}

/// A `.app-dialog-footer button` label's border box width: its text, `padding: 3px 12px` and
/// the 1-px border, at least `min` wide.
pub fn dialog_button_width(painter: &egui::Painter, label: &str, min: f32) -> f32 {
    (text::layout_width(painter, label, FontSpec::new(DIALOG_FONT_SIZE))
        + 2.0 * (DIALOG_BUTTON_PADDING_X + 1.0))
        .max(min)
}

/// A footer button (`.app-dialog-footer button`, the global `button` look) at `rect`, with the
/// `:focus-visible` outline of the dialog's primary button while it has the keyboard focus;
/// `request_focus` gives it the focus (a dialog's first frame: `button.focus()`).
pub fn dialog_button(
    ui: &mut Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    rect: Rect,
    label: &str,
    request_focus: bool,
) -> egui::Response {
    let response = super::inputs::push_button(ui, id_salt, rect, label, true, false);
    if request_focus {
        response.request_focus();
    }
    if request_focus || response.has_focus() {
        let ppp = ui.ctx().pixels_per_point();
        let theme = theme::current(ui.ctx());
        let (w, offset) = DIALOG_FOCUS_RING;
        let outer = px::snap_rect(rect.expand(offset + w), ppp);
        let inner = px::snap_rect(rect.expand(offset), ppp);
        let painter = ui.painter();
        let c = theme.menu_item_selected_border;
        for r in [
            Rect::from_min_max(outer.min, Pos2::new(outer.max.x, inner.min.y)),
            Rect::from_min_max(Pos2::new(outer.min.x, inner.max.y), outer.max),
            Rect::from_min_max(
                Pos2::new(outer.min.x, inner.min.y),
                Pos2::new(inner.min.x, inner.max.y),
            ),
            Rect::from_min_max(
                Pos2::new(inner.max.x, inner.min.y),
                Pos2::new(outer.max.x, inner.max.y),
            ),
        ] {
            painter.rect_filled(r, 0.0, c);
        }
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, vec2};
    use serde_json::json;

    #[test]
    fn dialog_pointer_focus_hides_ring_until_keyboard_without_losing_activation() {
        let ctx = egui::Context::default();
        crate::install_fonts(&ctx, pdn_core::fonts::FontPolicy::Bundled, "Selawik");
        let frame = |events: Vec<egui::Event>| {
            let mut result = (false, false);
            let mut points = (Pos2::ZERO, Pos2::ZERO);
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 800.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let mut body = |ui: &mut Ui, r: Rect| {
                        let focused = ui.memory(|m| m.focused());
                        let canvas = ui.interact(
                            r.shrink(10.0),
                            ui.scope_id().with("canvas"),
                            Sense::click_and_drag(),
                        );
                        if canvas.hovered()
                            && ui.input(|i| i.pointer.primary_pressed())
                            && let Some(f) = focused
                        {
                            ui.memory_mut(|m| m.request_focus(f));
                        }
                    };
                    let f = dialog_frame(
                        ui,
                        "Focus",
                        None,
                        (Vec2::new(300.0, 100.0), &mut body),
                        None,
                    );
                    result = (ctx.memory(|m| m.has_focus(f.close_id)), f.closed);
                    points = (
                        pos2(
                            f.rect.max.x - DIALOG_BORDER - DIALOG_CLOSE.0 / 2.0,
                            f.rect.min.y + DIALOG_BORDER + DIALOG_CLOSE.1 / 2.0,
                        ),
                        f.body.center(),
                    );
                    ctx.memory_mut(|m| m.request_focus(f.close_id));
                },
            );
            out.textures_delta.clear();
            (result, out.shapes.len(), points)
        };
        let key = |key, pressed| egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        frame(vec![key(egui::Key::A, true), key(egui::Key::A, false)]);
        let keyboard = frame(vec![]);
        assert!(keyboard.0.0);
        frame(vec![egui::Event::PointerMoved(keyboard.2.1)]);
        frame(vec![egui::Event::PointerButton {
            pos: keyboard.2.1,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        frame(vec![egui::Event::PointerMoved(
            keyboard.2.1 + vec2(10.0, 5.0),
        )]);
        frame(vec![egui::Event::PointerButton {
            pos: keyboard.2.1 + vec2(10.0, 5.0),
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        let canvas = frame(vec![]);
        assert!(canvas.0.0);
        assert_eq!(
            keyboard.1, canvas.1,
            "a nonfocusable canvas press retains the focused close button's ring"
        );
        let close_pos = keyboard.2.0;
        frame(vec![egui::Event::PointerMoved(close_pos)]);
        frame(vec![egui::Event::PointerButton {
            pos: close_pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        }]);
        let pointer = frame(vec![egui::Event::PointerButton {
            pos: close_pos,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(pointer.0.0);
        assert!(pointer.0.1, "pointer close still activates");
        assert!(keyboard.1 > pointer.1, "pointer close hides its ring");
        frame(vec![egui::Event::PointerMoved(keyboard.2.1)]);
        let enter = frame(vec![
            key(egui::Key::Enter, true),
            key(egui::Key::Enter, false),
        ]);
        assert_eq!(enter.0, (true, true));
        assert_eq!(enter.1, keyboard.1, "keyboard restores the ring");
        assert!(!frame(vec![key(egui::Key::Space, true)]).0.1);
        assert!(frame(vec![key(egui::Key::Space, false)]).0.1);
    }

    fn r(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, y), Vec2::new(w, h))
    }

    /// `floating-windows.md` §5's layouts on today's shell (the editor starts at y 85, Task 19b):
    /// (W, overlay). A = 1280×800: the 1265×668 client; B = 1024×632: 1009×500.
    fn a() -> (Rect, Rect) {
        (r(0.0, 85.0, 1265.0, 668.0), r(0.0, 0.0, 1280.0, 800.0))
    }
    fn b() -> (Rect, Rect) {
        (r(0.0, 85.0, 1009.0, 500.0), r(0.0, 0.0, 1024.0, 632.0))
    }

    const TOOLS: (f32, f32) = (64.0, 331.0);
    const HISTORY: (f32, f32) = (200.0, 230.0);
    const LAYERS: (f32, f32) = (200.0, 276.0);
    const COLORS: (f32, f32) = (270.0, 315.0);

    fn form(size: (f32, f32), right: bool, bottom: bool) -> FloatingWindow {
        FloatingWindow::new("t", size.into(), Some(Snap::corner(right, bottom)), None)
    }

    fn placed(mut w: FloatingWindow, (wr, overlay): (Rect, Rect)) -> Pos2 {
        w.place(wr, overlay, 1.0);
        w.pos
    }

    /// The four default corners, `floating-windows.md` §2.4 step 1 / §5.2 with the 5.6-DIP
    /// inset (§2.4 step 4; controller ruling: §5's vectors used paintdotjs's 10): the border box
    /// 5.6 inside W on each side, on whole pixels. P1 = A; P2 = B, where History (y 91–323) and
    /// Layers (y 301–579) overlap.
    #[test]
    fn p1_p2_forms_park_in_their_corners() {
        let corners = |layout: fn() -> (Rect, Rect)| {
            [
                placed(form(TOOLS, false, false), layout()),
                placed(form(HISTORY, true, false), layout()),
                placed(form(LAYERS, true, true), layout()),
                placed(form(COLORS, false, true), layout()),
            ]
        };
        // History: 1265 − 5.6 − 202 = 1057.4; Layers: 753 − 5.6 − 278 = 469.4.
        assert_eq!(
            corners(a),
            [
                pos2(6.0, 91.0),
                pos2(1057.0, 91.0),
                pos2(1057.0, 469.0),
                pos2(6.0, 430.0)
            ]
        );
        assert_eq!(
            corners(b),
            [
                pos2(6.0, 91.0),
                pos2(801.0, 91.0),
                pos2(801.0, 301.0),
                pos2(6.0, 262.0)
            ]
        );
        // At 125 % the 5.6-DIP inset is 7 device px.
        let mut w = form(TOOLS, false, false);
        w.place(a().0, a().1, 1.25);
        assert_eq!(w.pos * 1.25, pos2(7.0, 113.0));
    }

    /// P3: a 640×400 window (W = [0, 625]×[85, 353]): Layers' top lies above W (no clamp to W);
    /// Tools is held in the overlay (its CSS height). P4: a 4-DIP W: top-anchored forms at W's
    /// bottom (the interior min), bottom-anchored ones above W, then clamped into the overlay.
    #[test]
    fn p3_p4_small_canvas_areas() {
        let small = || (r(0.0, 85.0, 625.0, 268.0), r(0.0, 0.0, 640.0, 400.0));
        assert_eq!(
            placed(form(HISTORY, true, false), small()),
            pos2(417.0, 91.0)
        );
        assert_eq!(placed(form(LAYERS, true, true), small()), pos2(417.0, 69.0));
        assert_eq!(placed(form(COLORS, false, true), small()), pos2(6.0, 30.0));
        assert_eq!(placed(form(TOOLS, false, false), small()), pos2(6.0, 69.0));
        let flat = || (r(0.0, 85.0, 1009.0, 4.0), b().1);
        assert_eq!(placed(form(TOOLS, false, false), flat()), pos2(6.0, 89.0));
        assert_eq!(placed(form(LAYERS, true, true), flat()), pos2(801.0, 0.0));
    }

    /// D1–D4 (`floating-windows.md` §2.6 step 4, §5.3): released in A, then laid out in B. Away
    /// from every edge: absolute. Near W's right edge: Right, and the free axis keeps its distance
    /// from W's top. Near the bottom: Bottom, and x stays. 14.4 from W's inset right edge snaps,
    /// 15.4 does not.
    #[test]
    fn d1_d4_released_forms_follow_their_snapped_edges() {
        let drop = |size, at: Pos2| {
            let mut w = form(size, true, false);
            let (wr, overlay) = a();
            w.place(wr, overlay, 1.0);
            w.set_position_aligned(at, wr, overlay, &[], 1.0);
            let released = (w.pos, w.snap);
            let (wr, overlay) = b();
            w.place(wr, overlay, 1.0);
            (released, w.pos)
        };
        // D1
        assert_eq!(
            drop(HISTORY, pos2(500.0, 300.0)),
            ((pos2(500.0, 300.0), None), pos2(500.0, 300.0))
        );
        // D2: 1009 − 5.6 − 202 = 801.4.
        let right = Snap {
            dy: 215.0,
            ..Snap::corner(true, false)
        };
        assert_eq!(
            drop(HISTORY, pos2(1057.0, 300.0)),
            ((pos2(1057.0, 300.0), Some(right)), pos2(801.0, 300.0))
        );
        // D3: 585 − 5.6 − 317 = 262.4.
        let bottom = Snap {
            dx: 300.0,
            ..Snap::corner(false, true)
        };
        assert_eq!(
            drop(COLORS, pos2(300.0, 430.0)),
            ((pos2(300.0, 430.0), Some(bottom)), pos2(300.0, 262.0))
        );
        // D4: the border box's right edge 14.4 from 1259.4 snaps (live), 15.4 does not.
        assert_eq!(
            drop(HISTORY, pos2(1043.0, 300.0)).0,
            (pos2(1057.0, 300.0), Some(right))
        );
        assert_eq!(
            drop(HISTORY, pos2(1042.0, 300.0)).0,
            (pos2(1042.0, 300.0), None)
        );
    }

    /// §2.6 pass 1: a form snaps to another's outer edge only where they overlap across it,
    /// within 15 DIP — 30 once one snapped in the same pass — keeping the 7.5 gap; the snap moves
    /// the window but is not stored.
    #[test]
    fn forms_snap_to_overlapping_neighbours_with_doubled_proximity() {
        let (wr, overlay) = a();
        // History's bottom (532) 8 above `below`'s top; `beside`'s left edge 18 past its right.
        let below = r(500.0, 540.0, 202.0, 100.0);
        let beside = r(720.0, 300.0, 100.0, 100.0);
        let drop = |others: &[Rect]| {
            let mut w = form(HISTORY, true, false);
            w.set_position_aligned(pos2(500.0, 300.0), wr, overlay, others, 1.0);
            (w.pos, w.snap)
        };
        // 540 − 7.5 − 232 = 300.5; then 720 − 7.5 − 202 = 510.5, 18 < 30.
        assert_eq!(drop(&[below, beside]), (pos2(511.0, 301.0), None));
        // `beside` first: 18 ≥ 15, no snap; then `below`.
        assert_eq!(drop(&[beside, below]), (pos2(500.0, 301.0), None));
        // No overlap across the edge: `below` moved right of History's span.
        assert_eq!(
            drop(&[below.translate(vec2(210.0, 0.0))]),
            (pos2(500.0, 300.0), None)
        );
    }

    /// §2.7: the minimum is the default size, and a resize re-parks: the west edge of a
    /// Right/Bottom form grows it leftward down to the minimum; its east edge grows it leftward
    /// too (the anchored right edge stays).
    #[test]
    fn resizing_keeps_the_anchored_edges() {
        let (wr, overlay) = a();
        let mut w = FloatingWindow::new(
            "t",
            LAYERS.into(),
            Some(Snap::corner(true, true)),
            Some(LAYERS.into()),
        );
        w.place(wr, overlay, 1.0);
        let start = |edges| Resize {
            edges,
            pointer: pos2(1055.0, 600.0),
            pos: w.pos,
            size: w.size,
        };
        let west = start(HANDLES[3].1);
        let east = start(HANDLES[1].1);
        let mut resized = |r: Resize, to: Pos2| {
            w.resize_to(r, to, overlay, 1.0);
            w.place(wr, overlay, 1.0);
            (w.pos.x, w.size.x)
        };
        assert_eq!(resized(west, pos2(1015.0, 600.0)), (1017.0, 240.0));
        assert_eq!(resized(west, pos2(1131.0, 600.0)), (1057.0, 200.0));
        assert_eq!(resized(east, pos2(1095.0, 600.0)), (1017.0, 240.0));
        // Far past the overlay: held by the room left of the anchored right edge (1057 + 200).
        assert_eq!(resized(east, pos2(3000.0, 600.0)), (0.0, 1257.0));
    }

    /// §2.5 step 4: an unsnapped form keeps its screen position; the overlay clamp (a platform
    /// deviation) only moves where it shows, so it comes back, and the home is what persists.
    #[test]
    fn the_overlay_clamp_leaves_an_unsnapped_home_alone() {
        let (wr, overlay) = a();
        let mut w = form(HISTORY, true, false);
        w.place(wr, overlay, 1.0);
        w.set_position_aligned(pos2(1000.0, 300.5), wr, overlay, &[], 1.0);
        assert_eq!((w.pos, w.snap), (pos2(1000.0, 301.0), None));
        let small = (r(0.0, 85.0, 625.0, 268.0), r(0.0, 0.0, 640.0, 400.0));
        assert!(w.place(small.0, small.1, 1.0));
        assert_eq!(w.pos, pos2(440.0, 170.0));
        assert_eq!(
            (&w.state()["x"], &w.state()["y"]),
            (&json!(1000.0), &json!(300.5))
        );
        w.place(wr, overlay, 1.0);
        assert_eq!(w.pos, pos2(1000.0, 301.0));
    }

    /// `ui-drawn-graphics.md` Addendum: 0.5 s to translucent, 0.15 s to opaque, ease-out; a
    /// reversal mid-fade continues from the opacity it reached (Task 20 review: no jump).
    #[test]
    fn fades_take_their_direction_s_time_and_reverse_smoothly() {
        let mut f = Fade::OPAQUE;
        assert_eq!(f.update(false, 0.0), (1.0, true));
        assert_eq!(f.update(true, 1.0), (1.0, false));
        let mid = f.update(true, 1.25).0;
        assert!(0.75 < mid && mid < 0.875, "{mid}");
        assert_eq!(f.update(true, 1.5), (0.75, true));
        f.update(false, 2.0);
        assert!(!f.update(false, 2.1).1);
        assert_eq!(f.update(false, 2.2), (1.0, true));
        f.update(true, 3.0);
        let reached = f.update(true, 3.1).0;
        assert_eq!(f.update(false, 3.1).0, reached);
        assert!(f.update(false, 3.15).0 > reached);
    }

    /// §5.4: `windows.<id>` stores the position and the description; a resizable form's saved
    /// size never drops below its minimum; an unsnapped form comes back where it was.
    #[test]
    fn state_roundtrips_through_settings() {
        let mut w = FloatingWindow::new(
            "t",
            HISTORY.into(),
            Some(Snap::corner(true, false)),
            Some(HISTORY.into()),
        );
        let saved = json!({"visible": false, "width": 150, "height": 260, "x": 500, "y": 300.5,
            "snap": null});
        assert!(!w.restore(&saved));
        assert_eq!((w.size, w.snap), (vec2(200.0, 260.0), None));
        let (wr, overlay) = b();
        w.place(wr, overlay, 1.0);
        assert_eq!(w.pos, pos2(500.0, 301.0));
        assert_eq!(
            w.state(),
            json!({"visible": true, "width": 200.0, "height": 260.0, "x": 500.0, "y": 300.5,
                "snap": null})
        );
        let mut v = form(HISTORY, false, false);
        let snap = json!({"h": "top", "v": "right", "dx": 5.6, "dy": 215});
        v.restore(&json!({"x": 1, "y": 2, "snap": snap}));
        v.place(wr, overlay, 1.0);
        assert_eq!(v.pos, pos2(801.0, 300.0));
        assert_eq!(
            v.state()["snap"],
            json!({"h": "top", "v": "right", "dx": 5.6, "dy": 215.0})
        );
        assert!(w.restore(&serde_json::Value::Null));
    }

    /// §5.3: a paintdotjs state (anchors only) is placed once by its anchors in the 10-px view
    /// bounds, then classified like a release.
    #[test]
    fn saved_anchors_migrate_to_descriptions() {
        let migrated = |state| {
            let mut w = form(HISTORY, true, false);
            w.restore(&state);
            let (wr, overlay) = a();
            w.place(wr, overlay, 1.0);
            (w.pos, w.snap)
        };
        assert_eq!(
            migrated(json!({"anchorX": 1, "anchorY": 0})),
            (pos2(1057.0, 91.0), Some(Snap::corner(true, false)))
        );
        // 10 + 0.5 × 1045 = 532.5; 95 + 0.47 × 418 = 291.46.
        assert_eq!(
            migrated(json!({"anchorX": 0.5, "anchorY": 0.47})),
            (pos2(533.0, 291.0), None)
        );
        // The missing anchorY is the default corner's (top): 95 is 4.4 from W's inset top.
        let top = Snap {
            dx: 533.0,
            ..Snap::corner(false, false)
        };
        assert_eq!(
            migrated(json!({"anchorX": 0.5})),
            (pos2(533.0, 91.0), Some(top))
        );
    }

    /// DialogMover.setPosition: the box stays inside the backdrop; one larger than the backdrop
    /// keeps its top-left edge in.
    #[test]
    fn dialog_mover_clamps_to_the_backdrop() {
        let b = r(0.0, 0.0, 1280.0, 800.0);
        let about = vec2(625.0, 578.0 + 47.0 / 64.0);
        // about-dragged: (387, 127) → (187, 67) with the grab at (59.5, 16.375).
        assert_eq!(
            clamp_dialog(pos2(127.5, 50.625), about, b),
            pos2(127.5, 50.625)
        );
        assert_eq!(
            clamp_dialog(pos2(-50.0, 900.0), about, b),
            pos2(0.0, 221.0 + 17.0 / 64.0)
        );
        assert_eq!(
            clamp_dialog(pos2(100.0, 100.0), vec2(2000.0, 900.0), b),
            pos2(0.0, 0.0)
        );
    }

    /// `@keyframes modal-dialog-attention-*` at `160ms ease-in-out 5`: 0 → 1 → 0 per run, eased
    /// per keyframe pair; over after five runs.
    #[test]
    fn attention_keyframes_run_five_times() {
        assert!(attention_amount(0.0).abs() < 1e-6);
        assert!((attention_amount(80.0) - 1.0).abs() < 1e-3);
        assert!((attention_amount(40.0) - 0.5).abs() < 1e-3);
        assert!((attention_amount(4.0 * 160.0 + 80.0) - 1.0).abs() < 1e-3);
        assert_eq!(attention_amount(800.0), 0.0);
        assert_eq!(attention_amount(-1.0), 0.0);
    }

    /// I1 (`floating-windows.md` §2.9 step 4, §5.6; calibrate B-W3).
    #[test]
    fn inactive_captions_are_grey_without_a_close_box() {
        let rgb = |c: u32| Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8);
        let dark = theme::Theme::for_scheme(Scheme::Dark);
        let white = rgb(0xFFFFFF);
        assert_eq!(
            caption_colors(&dark, true, false),
            (white, Some(rgb(0xC75050)), white)
        );
        assert_eq!(
            caption_colors(&dark, false, false),
            (rgb(0x7F7F7F), None, rgb(0x7F7F7F))
        );
        assert_eq!(
            caption_colors(&dark, false, true),
            (rgb(0x7F7F7F), Some(rgb(0xE04343)), white)
        );
        let light = theme::Theme::for_scheme(Scheme::Light);
        assert_eq!(
            caption_colors(&light, false, false),
            (rgb(0x999999), None, rgb(0x999999))
        );
    }
}
