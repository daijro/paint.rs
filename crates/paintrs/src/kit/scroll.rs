//! Chromium's standard (non-overlay, Fluent) scrollbar as `#view { overflow: scroll }` shows it
//! with `:root { scrollbar-color: var(--color-scrollbar-thumb) var(--color-scrollbar-background) }`
//! (css/component.css). Chromium draws it natively, so its geometry is measured from the
//! `shell-no-forms`, `shell-no-forms-rulers` references (and a zoom-6000% probe for the minimum
//! thumb length), DPR 1:
//! - 15 px thick; 18 px arrow buttons (`NativeThemeFluent::GetVerticalScrollbarButtonSize`,
//!   `kScrollbarButtonSideLength`); the pill thumb (round ends) drawn at the device scale with
//!   exact-along coverage, blitted like Skia's anti-aliased fill over the track color;
//! - the arrows are `NativeThemeBase::PathForArrow` triangles in the thumb color, filled without
//!   anti-aliasing: NativeThemeFluent draws them so when its "Segoe Fluent Icons" glyph font is
//!   missing (ruling R10);
//! - thumb length `round(track * view / content)`, at least 20; position
//!   `trunc(offset * (track - length) / (content - view))` (Blink ScrollbarTheme::ThumbPosition's
//!   `saturated_cast`, a fraction in (0, 1) gives 1); the pill is painted 9 px wide (3 px inset)
//!   over exactly `[pos, pos + length)` — one model for the `#view` bars and the tool-option
//!   popups' bars (six bars, scrolled and unscrolled).
//!
//! Port of: ui/native_theme/native_theme_fluent.cc (`GetArrowRect`, `PaintArrowButton`),
//! ui/native_theme/native_theme_base.cc (`PaintArrow`, `PathForArrow`), Chromium 150
//!
//! Hover/pressed colors are not measured yet (no reference covers them).
//!
//! `ScrollList` ports js/src/interface/item/layout/{ScrollList,data/ScrollSession,
//! data/ScrollOrientation}.js (paintdotjs @ b4e6fc5): the scroll position, wheel, drag-to-reorder
//! and item slide of `.scroll-list`; the lists paint their own items (`.selected-item`,
//! `.dragging-item`, css/component.css).
use std::collections::HashMap;

use egui::{Color32, Id, Painter, Pos2, Rect, Response, Sense, Ui, Vec2};

use super::aa::{blit_aa, fill_spans};
use crate::theme::Theme;

/// Scrollbar thickness (CSS px).
pub const THICKNESS: f32 = 15.0;
/// The arrow buttons' length along the bar (`kScrollbarButtonSideLength`; the track excludes it).
const BUTTON_LENGTH: f32 = 18.0;
/// Blink `MinimumThumbLength` for this theme (probed at zoom 6000%: a 17 px pill = 20 - 3).
const MIN_THUMB: f64 = 20.0;
/// The pill spans `[3, 12)` across the bar.
const THUMB_ACROSS: (f32, f32) = (3.0, 12.0);
/// Chromium: an arrow click scrolls 40 px; a track click pages by 87.5% of the view.
const LINE_STEP: f64 = 40.0;
const PAGE_FRACTION: f64 = 0.875;

/// `NativeThemeFluent::GetArrowRect` for a normal-state button without arrow icons: a
/// `ceil(9 * scale)` square (one larger when the button's short side differs by an odd amount)
/// centred and floored, then moved `round(scale)` px toward the bar's end. Device px.
fn arrow_rect(button: Rect, vertical: bool, start: bool) -> Rect {
    let (min_side, max_side) = (
        button.width().min(button.height()),
        button.width().max(button.height()),
    );
    let scale = max_side / BUTTON_LENGTH;
    let mut side = (9.0 * scale).ceil();
    side += (min_side - side) % 2.0;
    let origin = (button.min + (button.size() - Vec2::splat(side)) / 2.0).floor();
    let offset = (if start { -scale } else { scale }).round();
    let offset = if vertical {
        Vec2::new(0.0, offset)
    } else {
        Vec2::new(offset, 0.0)
    };
    Rect::from_min_size(origin + offset, Vec2::splat(side))
}

/// `NativeThemeBase::PathForArrow`: an up- (or right-) pointing triangle in `r`, mirrored about
/// its centre for down (or left). The JS-like `int` divisions are C++'s.
fn arrow_path(r: Rect, vertical: bool, start: bool) -> [Pos2; 3] {
    let mut p = if vertical {
        let h = r.height().round() as i32 / 2 + 1;
        let y = r.max.y - (h / 2) as f32 + 1.0;
        [
            Pos2::new(r.min.x, y),
            Pos2::new(r.max.x, y),
            Pos2::new(r.min.x + r.width() / 2.0, y - h as f32),
        ]
    } else {
        let w = r.width().round() as i32 / 2 + 1;
        let x = r.min.x + (w / 2) as f32;
        [
            Pos2::new(x, r.min.y),
            Pos2::new(x, r.max.y),
            Pos2::new(x + w as f32, r.max.y - r.height() / 2.0),
        ]
    };
    // Down (vertical, end) and left (horizontal, start) are mirrored.
    if vertical != start {
        let c = r.center();
        for q in &mut p {
            if vertical {
                q.y = 2.0 * c.y - q.y;
            } else {
                q.x = 2.0 * c.x - q.x;
            }
        }
    }
    p
}

/// Thumb `(position, length)` in px along the track; `None` when nothing scrolls.
pub fn thumb_geometry(track: f64, content: f64, view: f64, offset: f64) -> Option<(f64, f64)> {
    if content <= view || track <= 0.0 {
        return None;
    }
    let length = pdn_core::js::math_round(track * view / content)
        .max(MIN_THUMB)
        .min(track);
    let max_offset = content - view;
    let position = offset.clamp(0.0, max_offset) * (track - length) / max_offset;
    let position = if position > 0.0 && position < 1.0 {
        1.0
    } else {
        position.trunc()
    };
    Some((position, length))
}

#[derive(Clone, Copy, Default)]
struct DragState {
    /// Pointer offset from the thumb start when the drag began.
    grab: Option<f32>,
}

pub struct Scrollbar {
    rect: Rect,
    vertical: bool,
    salt: Id,
    /// (track, thumb) from a `scrollbar-color` other than the root's.
    colors: Option<(Color32, Color32)>,
}

impl Scrollbar {
    pub fn vertical(rect: Rect) -> Self {
        Self {
            rect,
            vertical: true,
            salt: Id::NULL,
            colors: None,
        }
    }

    pub fn horizontal(rect: Rect) -> Self {
        Self {
            rect,
            vertical: false,
            salt: Id::NULL,
            colors: None,
        }
    }

    /// Distinguishes several bars of the same orientation within one `Ui` (the widget id is
    /// the `Ui` id, this salt and the orientation).
    pub fn id_salt(mut self, salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        self.salt = Id::NULL.with(salt);
        self
    }

    /// `scrollbar-color: thumb track`.
    pub fn colors(mut self, track: Color32, thumb: Color32) -> Self {
        self.colors = Some((track, thumb));
        self
    }

    fn along(&self, p: Pos2) -> f32 {
        if self.vertical { p.y } else { p.x }
    }

    fn start(&self) -> f32 {
        self.along(self.rect.min)
    }

    fn length(&self) -> f32 {
        if self.vertical {
            self.rect.height()
        } else {
            self.rect.width()
        }
    }

    /// Paints and handles input; `offset` is the scroll offset in px. Returns true when the
    /// user changed it.
    pub fn show(&self, ui: &mut Ui, content_len: f64, view_len: f64, offset: &mut f64) -> bool {
        let theme = crate::theme::current(ui.ctx());
        let id = ui.scope_id().with(("scrollbar", self.salt, self.vertical));
        let response = ui.interact(self.rect, id, Sense::click_and_drag());
        let track_len = (self.length() - 2.0 * BUTTON_LENGTH) as f64;
        let thumb = thumb_geometry(track_len, content_len, view_len, *offset);
        let changed = self.handle_input(ui, &response, id, thumb, content_len, view_len, offset);
        self.paint(
            ui.painter(),
            &theme,
            thumb_geometry(track_len, content_len, view_len, *offset),
        );
        changed
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_input(
        &self,
        ui: &Ui,
        response: &Response,
        id: Id,
        thumb: Option<(f64, f64)>,
        content_len: f64,
        view_len: f64,
        offset: &mut f64,
    ) -> bool {
        let Some((pos, len)) = thumb else {
            return false;
        };
        let before = *offset;
        let max_offset = content_len - view_len;
        let track_start = self.start() + BUTTON_LENGTH;
        let track_len = self.length() - 2.0 * BUTTON_LENGTH;
        let thumb_start = track_start + pos as f32;
        let mut drag = ui.data(|d| d.get_temp::<DragState>(id)).unwrap_or_default();
        if response.drag_started()
            && let Some(p) = response.interact_pointer_pos()
        {
            let a = self.along(p);
            if a >= thumb_start && a < thumb_start + len as f32 {
                drag.grab = Some(a - thumb_start);
            }
        }
        if response.dragged()
            && let (Some(grab), Some(p)) = (drag.grab, response.interact_pointer_pos())
        {
            let free = (track_len as f64 - len).max(1.0);
            let start = (self.along(p) - grab - track_start) as f64;
            *offset = start / free * max_offset;
        }
        if response.drag_stopped() {
            drag.grab = None;
        }
        if response.clicked()
            && let Some(p) = super::inputs::interact_pos(ui, response)
        {
            let a = self.along(p);
            if a < track_start {
                *offset -= LINE_STEP;
            } else if a >= track_start + track_len {
                *offset += LINE_STEP;
            } else if a < thumb_start {
                *offset -= view_len * PAGE_FRACTION;
            } else if a >= thumb_start + len as f32 {
                *offset += view_len * PAGE_FRACTION;
            }
        }
        ui.data_mut(|d| d.insert_temp(id, drag));
        *offset = offset.clamp(0.0, max_offset);
        *offset != before
    }

    fn paint(&self, painter: &Painter, theme: &Theme, thumb: Option<(f64, f64)>) {
        let (track, fg) = self
            .colors
            .unwrap_or((theme.scrollbar_background, theme.scrollbar_thumb));
        painter.rect_filled(self.rect, 0.0, track);
        // `PaintArrow`: the path in device px, filled without anti-aliasing.
        let ppp = painter.ctx().pixels_per_point();
        let (along, across) = (BUTTON_LENGTH, THICKNESS);
        let size = if self.vertical {
            Vec2::new(across, along)
        } else {
            Vec2::new(along, across)
        };
        let buttons = [
            (Rect::from_min_size(self.rect.min, size), true),
            (Rect::from_min_size(self.rect.max - size, size), false),
        ];
        for (button, start) in buttons {
            let device = Rect::from_min_max(button.min * ppp, button.max * ppp);
            let path = arrow_path(
                arrow_rect(device, self.vertical, start),
                self.vertical,
                start,
            );
            for (y, x0, x1) in fill_spans(&path) {
                let span = Rect::from_min_max(
                    Pos2::new(x0 as f32, y as f32) / ppp,
                    Pos2::new(x1 as f32, (y + 1) as f32) / ppp,
                );
                painter.rect_filled(span, 0.0, fg);
            }
        }

        let Some((pos, len)) = thumb else {
            return;
        };
        // The pill in device px: `[a0, a1)` along, `[c0, c1)` across, round ends (R24's DIP
        // geometry scaled; R37 leaves Blink's own fractional-DPR thumb rounding alone).
        let a0 = (self.start() + BUTTON_LENGTH + pos as f32) * ppp;
        let a1 = a0 + len as f32 * ppp;
        let across0 = if self.vertical {
            self.rect.min.x
        } else {
            self.rect.min.y
        };
        let (c0, c1) = (
            (across0 + THUMB_ACROSS.0) * ppp,
            (across0 + THUMB_ACROSS.1) * ppp,
        );
        // (along, across) device px -> screen points.
        let at = |along: f32, across: f32| {
            if self.vertical {
                Pos2::new(across, along) / ppp
            } else {
                Pos2::new(along, across) / ppp
            }
        };
        let r = (c1 - c0) / 2.0;
        // Whole rows clear of both caps are solid.
        let (b0, b1) = ((a0 + r).ceil(), (a1 - r).floor());
        if b1 > b0 {
            painter.rect_filled(Rect::from_two_pos(at(b0, c0), at(b1, c1)), 0.0, fg);
        }
        let caps = (a0.floor() as i32..b0 as i32).chain((b1.max(b0) as i32)..a1.ceil() as i32);
        for row in caps {
            for col in c0.floor() as i32..c1.ceil() as i32 {
                let a = pill_coverage(row as f32, col as f32, [a0, a1], [c0, c1]);
                if a != 0 {
                    let p = at(row as f32, col as f32);
                    let px = Rect::from_min_size(p, Vec2::splat(1.0 / ppp));
                    painter.rect_filled(px, 0.0, blit_aa(fg, a, track));
                }
            }
        }
    }
}

/// The 8-bit coverage of device pixel (`row`, `col`) (along, across) by the thumb pill spanning
/// `along` × `across` with fully round ends: the exact along-extent at 16 sample columns.
/// Chromium's raster of the same pill comes within 45/255 of it at 1× (the tables measured before
/// this replaced them), inside R21's edge rule.
fn pill_coverage(row: f32, col: f32, along: [f32; 2], across: [f32; 2]) -> u8 {
    const N: usize = 16;
    let r = (across[1] - across[0]) / 2.0;
    let mid = across[0] + r;
    let sum: f32 = (0..N)
        .map(|k| {
            let x = col + (k as f32 + 0.5) / N as f32;
            let h = (r * r - (x - mid) * (x - mid)).max(0.0).sqrt();
            if x < across[0] || x > across[1] {
                return 0.0;
            }
            let (top, bottom) = (along[0] + r - h, along[1] - r + h);
            (bottom.min(row + 1.0) - top.max(row)).max(0.0)
        })
        .sum();
    (sum / N as f32 * 255.0).round() as u8
}

/// ScrollList: a drag reorders after 4 px; moved items slide over 140 ms with
/// `cubic-bezier(.2,.8,.2,1)`; the wheel scrolls `scrollSpeed` × its delta at most every 100 ms;
/// a drag auto-scrolls in the last `min(32, size / 4)` px of either end, by up to 14 px a move.
const DRAG_THRESHOLD: f32 = 4.0;
const SLIDE_S: f64 = 0.14;
const SLIDE_EASE: [f32; 4] = [0.2, 0.8, 0.2, 1.0];
const WHEEL_THROTTLE_S: f64 = 0.1;
const AUTO_SCROLL: (f32, f32, f32) = (32.0, 14.0, 0.45); // zone, max step, factor

/// This frame's wheel scrolling in points, egui's sign (Ruling (P2) 9: 120 device px a line).
pub fn wheel_delta(i: &egui::InputState, page: f32) -> Vec2 {
    i.raw.events.iter().fold(Vec2::ZERO, |sum, e| match e {
        egui::Event::MouseWheel { unit, delta, .. } => {
            sum + wheel_points(*unit, *delta, i.pixels_per_point, page)
        }
        _ => sum,
    })
}

/// One wheel event's scrolling in points: pixel deltas as they are, 120 device px a line,
/// `page` points a page.
pub fn wheel_points(unit: egui::MouseWheelUnit, delta: Vec2, ppp: f32, page: f32) -> Vec2 {
    delta
        * match unit {
            egui::MouseWheelUnit::Point => 1.0,
            egui::MouseWheelUnit::Line => 120.0 / ppp,
            egui::MouseWheelUnit::Page => page,
        }
}

/// `cubic-bezier(x1, y1, x2, y2)` at time fraction `x` (bisection on the monotonic x curve).
pub fn cubic_bezier([x1, y1, x2, y2]: [f32; 4], x: f32) -> f32 {
    let b = |a1: f32, a2: f32, s: f32| {
        3.0 * (1.0 - s) * (1.0 - s) * s * a1 + 3.0 * (1.0 - s) * s * s * a2 + s * s * s
    };
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if b(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    b(y1, y2, (lo + hi) / 2.0)
}

/// A pointer drag of one item (ScrollSession's drag fields).
#[derive(Clone, Copy, Debug)]
struct ListDrag {
    key: u64,
    /// `dragGrabOffset`: pointer minus the item's start at the press.
    grab: f32,
    start: f32,
    last: f32,
    passed: bool,
}

/// A `.scroll-list` along one axis of equal item slots, with its `ScrollSession` (the state that
/// outlives the list's rebuilds). Items are identified by a key (`getKey()`).
#[derive(Default)]
pub struct ScrollList {
    horizontal: bool,
    /// `scrollLeft` / `scrollTop`.
    pub scroll: f32,
    /// `setScrollSpeed`: 1 leaves the wheel to the browser.
    speed: f32,
    /// `timeLastScrolled` (s).
    last_wheel: f64,
    drag: Option<ListDrag>,
    /// `itemPositionCache` (layout position along the axis, unscrolled) and each item's slide:
    /// (start offset, start time).
    positions: HashMap<u64, (f32, Option<(f32, f64)>)>,
}

impl ScrollList {
    pub fn horizontal(speed: f32) -> Self {
        Self {
            horizontal: true,
            speed,
            ..Default::default()
        }
    }

    fn along(&self, p: Pos2) -> f32 {
        if self.horizontal { p.x } else { p.y }
    }

    fn start_of(&self, r: Rect) -> f32 {
        self.along(r.min)
    }

    fn len_of(&self, r: Rect) -> f32 {
        if self.horizontal {
            r.width()
        } else {
            r.height()
        }
    }

    /// The list's hover area: the wheel listener hears the wheel only while the list element is
    /// under the pointer with nothing (a popup, another form) over it.
    fn cover_id(ui: &Ui, salt: &str) -> Id {
        ui.scope_id().with(("scroll-list", salt))
    }

    /// Registers the list's hover area `rect` (after its items, which it does not block).
    pub fn cover(&self, ui: &mut Ui, rect: Rect, salt: &str) {
        ui.interact(rect, Self::cover_id(ui, salt), Sense::hover());
    }

    /// The `wheel` listener of the list `salt` (its `cover`) over `viewport` (`content` = the
    /// scroll length): `speed` × the delta, at most every 100 ms, clamped to the range (paintdotjs
    /// dropped a step past an end, so the wheel stopped short of it). ponytail: `behavior: smooth`
    /// scrolls jump.
    pub fn wheel(&mut self, ui: &Ui, viewport: Rect, content: f32, now: f64, salt: &str) {
        if self.speed == 1.0 {
            return;
        }
        let page = self.len_of(viewport);
        let wheel = ui.input(|i| wheel_delta(i, page));
        let hovered = Self::hovered(ui, salt);
        if hovered && wheel != Vec2::ZERO && now - self.last_wheel >= WHEEL_THROTTLE_S {
            self.last_wheel = now;
            // `event.deltaY` (else deltaX) is egui's delta negated.
            let delta = -(if wheel.y != 0.0 { wheel.y } else { wheel.x }) * self.speed;
            self.scroll += delta;
            self.clamp(content, page);
        }
    }

    /// Paint.NET's list wheel (`HistoryControl`, `VerticalImageStrip.OnPointerWheelChanged`) over
    /// the list `salt` (its `cover`): `step` points a notch, every event, clamped to the range. A
    /// notch is a wheel line, or 100 px of a pixel delta (as Ctrl+wheel zoom counts them), so a
    /// touchpad scrolls in proportion. User (2026-10-30): paintdotjs's 0.4 × the delta at most
    /// every 100 ms (`wheel`) scrolled the History and Layers lists too little.
    pub fn wheel_notches(&mut self, ui: &Ui, viewport: Rect, content: f32, step: f32, salt: &str) {
        let notches = ui.input(|i| {
            i.raw.events.iter().fold(Vec2::ZERO, |sum, e| match e {
                egui::Event::MouseWheel { unit, delta, .. } => {
                    sum + match unit {
                        egui::MouseWheelUnit::Point => *delta / 100.0,
                        egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => *delta,
                    }
                }
                _ => sum,
            })
        });
        if Self::hovered(ui, salt) && notches != Vec2::ZERO {
            // egui's delta is up-positive; y, else x (a horizontal wheel).
            let n = if notches.y != 0.0 {
                notches.y
            } else {
                notches.x
            };
            self.scroll -= n * step;
            self.clamp(content, self.len_of(viewport));
        }
    }

    /// Whether the list `salt`'s `cover` is under the pointer with nothing over it.
    fn hovered(ui: &Ui, salt: &str) -> bool {
        ui.ctx()
            .read_response(Self::cover_id(ui, salt))
            .is_some_and(|r| r.hovered())
    }

    /// Clamps the scroll position to `[0, content - view]`.
    pub fn clamp(&mut self, content: f32, view: f32) {
        self.scroll = self.scroll.clamp(0.0, (content - view).max(0.0));
    }

    /// `scrollToItem`: centres item `i` (slot `extent`, `item` its client length) unless it is
    /// wholly in the `view`-long viewport. ponytail: `behavior: smooth` scrolls jump.
    pub fn scroll_to(&mut self, i: usize, extent: f32, item: f32, view: f32) {
        let start = i as f32 * extent;
        if start < self.scroll || start + extent > self.scroll + view {
            self.scroll = start - view / 2.0 + item / 2.0;
        }
    }

    /// The slots of `keys` (`extent` long, `viewport`'s cross size) at their layout position minus
    /// the scroll, plus the reorder slide (`postInitialize`'s animation); the dragged item follows
    /// the pointer. Forgets the positions of keys no longer listed.
    pub fn layout(
        &mut self,
        ui: &Ui,
        viewport: Rect,
        keys: &[u64],
        extent: f32,
        now: f64,
    ) -> Vec<Rect> {
        self.positions.retain(|k, _| keys.contains(k));
        let dragging = self.dragging();
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let mut slots = Vec::with_capacity(keys.len());
        for (i, &key) in keys.iter().enumerate() {
            let layout = i as f32 * extent;
            let (last, slide) = self.positions.entry(key).or_insert((layout, None));
            if (layout - *last).abs() > 1.0 && dragging != Some(key) {
                *slide = Some((*last - layout, now));
            }
            *last = layout;
            let offset = match *slide {
                Some((from, start)) if now - start < SLIDE_S => {
                    ui.ctx().request_repaint();
                    from * (1.0 - cubic_bezier(SLIDE_EASE, ((now - start) / SLIDE_S) as f32))
                }
                _ => {
                    *slide = None;
                    0.0
                }
            };
            let start = match (self.drag, pointer) {
                (Some(d), Some(p)) if dragging == Some(key) => self.along(p) - d.grab,
                _ => self.start_of(viewport) + layout - self.scroll + offset,
            };
            slots.push(if self.horizontal {
                Rect::from_min_size(
                    Pos2::new(start, viewport.min.y),
                    Vec2::new(extent, viewport.height()),
                )
            } else {
                Rect::from_min_size(
                    Pos2::new(viewport.min.x, start),
                    Vec2::new(viewport.width(), extent),
                )
            });
        }
        slots
    }

    /// `startPointerDrag`: a press at `pointer` on the item `key` whose slot is `slot`.
    pub fn press(&mut self, key: u64, pointer: Pos2, slot: Rect) {
        let at = self.along(pointer);
        self.drag = Some(ListDrag {
            key,
            grab: at - self.start_of(slot),
            start: at,
            last: at,
            passed: false,
        });
    }

    /// A list press owns capture even before it crosses the reorder threshold.
    pub fn captured(&self) -> bool {
        self.drag.is_some()
    }

    /// The item being dragged past the threshold (`.dragging-item`, painted last).
    pub fn dragging(&self) -> Option<u64> {
        self.drag.filter(|d| d.passed).map(|d| d.key)
    }

    /// `continuePointerDrag` / `stopPointerDrag`: past 4 px the item follows the pointer, the
    /// list auto-scrolls near its ends, and passing a neighbour's middle asks to swap the two
    /// (`itemSwapper`): the dragged item's index and the neighbour's.
    pub fn continue_drag(
        &mut self,
        ui: &Ui,
        keys: &[u64],
        slots: &[Rect],
        viewport: Rect,
    ) -> Option<(usize, usize)> {
        let (down, pointer) = ui.input(|i| (i.pointer.primary_down(), i.pointer.hover_pos()));
        let (Some(p), true) = (pointer, down) else {
            self.drag = None;
            return None;
        };
        let at = self.along(p);
        let d = self.drag.as_mut()?;
        let Some(index) = keys.iter().position(|&k| k == d.key) else {
            self.drag = None;
            return None;
        };
        let previous = d.last;
        d.last = at;
        if (at - d.start).abs() < DRAG_THRESHOLD {
            return None;
        }
        d.passed = true;
        // autoScrollDuringDrag (on pointer moves only, like the `pointermove` listener).
        let (zone, max_step, factor) = AUTO_SCROLL;
        let (start, end) = (
            self.start_of(viewport),
            self.start_of(viewport) + self.len_of(viewport),
        );
        let edge = (self.len_of(viewport) / 4.0).min(zone);
        let mut amount = 0.0;
        if at < start + edge {
            amount = -max_step.min((start + edge - at) * factor);
        }
        if at > end - edge {
            amount = max_step.min((at - end + edge) * factor);
        }
        self.scroll += amount;
        let middle = |j: usize| self.along(slots[j].center());
        if at < previous && index > 0 && at < middle(index - 1) {
            return Some((index, index - 1));
        }
        if at > previous && index + 1 < keys.len() && at > middle(index + 1) {
            return Some((index, index + 1));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A notch scrolls a step, a pixel delta in proportion, every event; a step past either end
    /// of a list stops at that end instead of being dropped.
    #[test]
    fn list_wheel_steps_by_notch_and_reaches_both_ends() {
        use egui::MouseWheelUnit::{Line, Point};
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(100.0, 218.0));
        let mut list = ScrollList::default();
        let frame = |list: &mut ScrollList, unit, dy: f32| {
            let events = vec![
                egui::Event::PointerMoved(Pos2::new(50.0, 100.0)),
                egui::Event::MouseWheel {
                    unit,
                    delta: Vec2::new(0.0, dy),
                    modifiers: egui::Modifiers::NONE,
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                },
            ];
            let raw = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::splat(400.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                list.wheel_notches(ui, rect, 372.0, 60.0, "t");
                list.cover(ui, rect, "t");
            });
            out.textures_delta.clear();
            list.scroll
        };
        frame(&mut list, Point, 0.0);
        assert_eq!(frame(&mut list, Point, -50.0), 30.0, "half a notch");
        assert_eq!(frame(&mut list, Line, -1.0), 90.0);
        assert_eq!(
            frame(&mut list, Line, -2.0),
            154.0,
            "short of the end: to the end"
        );
        assert_eq!(frame(&mut list, Line, 1.0), 94.0);
        assert_eq!(
            frame(&mut list, Point, 200.0),
            0.0,
            "short of the top: to the top"
        );
    }

    /// The pill's caps against Chromium's 1× raster of a vertical thumb's top cap (the table
    /// measured from `shell-no-forms` before geometry replaced it): within R21 (a) (±1 where the
    /// table's neighbourhood is flat) and at most 45/255 of coverage at edges; at 2× the cap is
    /// twice as long (it used to keep 1×'s rows as 2×2 blocks).
    #[test]
    fn pill_caps_match_dpr1_raster() {
        #[rustfmt::skip]
        const MEASURED: [[u8; 9]; 5] = [
            [0, 8, 141, 219, 247, 217, 142, 8, 0],
            [55, 216, 255, 255, 255, 255, 255, 212, 49],
            [129, 255, 255, 255, 255, 255, 255, 255, 126],
            [216, 255, 255, 255, 255, 255, 255, 255, 200],
            [247, 255, 255, 255, 255, 255, 255, 255, 240],
        ];
        // Beyond the table: the track above and beside, the body (255) below.
        let outside = |x: isize, y: isize| {
            if y >= 5 && (0..9).contains(&x) {
                255
            } else {
                0
            }
        };
        for (row, want) in MEASURED.iter().enumerate() {
            for (col, w) in want.iter().enumerate() {
                let a = pill_coverage(row as f32, col as f32, [0.0, 40.0], [0.0, 9.0]);
                let tol = crate::kit::aa::r21_tolerance(&MEASURED, col, row, outside);
                assert!(
                    a.abs_diff(*w) <= 45.min(tol),
                    "({col}, {row}): {a} vs {w} (±{tol})"
                );
            }
        }
        let edge = |row: f32| pill_coverage(row, 0.0, [0.0, 80.0], [0.0, 18.0]);
        assert!(
            edge(4.0) < 128 && edge(8.0) > 200,
            "{} {}",
            edge(4.0),
            edge(8.0)
        );
    }

    #[test]
    fn slide_easing_is_css_cubic_bezier() {
        assert!(cubic_bezier(SLIDE_EASE, 0.0).abs() < 1e-5);
        assert!((cubic_bezier(SLIDE_EASE, 1.0) - 1.0).abs() < 1e-5);
        // x(s) = 0.6·s(1 − s) + s³ = 0.5 at s ≈ 0.7246, where y(s) ≈ 0.94608.
        assert!((cubic_bezier(SLIDE_EASE, 0.5) - 0.94608).abs() < 1e-4);
    }

    #[test]
    fn thumb_geometry_matches_references() {
        // (bar start, bar length, content, view, offset) -> painted [start, end).
        let painted = |start: f64, len: f64, content: f64, view: f64, offset: f64| {
            let track = len - 2.0 * BUTTON_LENGTH as f64;
            let (pos, length) = thumb_geometry(track, content, view, offset).unwrap();
            let s = start + BUTTON_LENGTH as f64 + pos;
            (s, s + length)
        };
        // shell-no-forms V and H, shell-no-forms-rulers V and H.
        assert_eq!(painted(110.0, 643.0, 1286.0, 643.0, 322.0), (279.0, 583.0));
        assert_eq!(painted(0.0, 1265.0, 2530.0, 1265.0, 633.0), (325.0, 940.0));
        assert_eq!(painted(127.0, 626.0, 1252.0, 626.0, 322.0), (296.0, 591.0));
        assert_eq!(painted(17.0, 1248.0, 2496.0, 1248.0, 633.0), (342.0, 948.0));
        // tooloption-brush-size-presets and -font-family-open popups (unscrolled).
        assert_eq!(painted(109.0, 450.0, 1632.0, 450.0, 0.0), (127.0, 241.0));
        assert_eq!(painted(109.0, 520.0, 783.0, 520.0, 0.0), (127.0, 448.0));
        // zoom 6000% probe: the minimum length.
        assert_eq!(
            thumb_geometry(607.0, 36643.0, 643.0, 18032.0).unwrap().1,
            20.0
        );
        assert_eq!(thumb_geometry(607.0, 643.0, 643.0, 0.0), None);
    }

    #[test]
    fn arrows_are_chromium_path_triangles() {
        // shell-no-forms: the #view bars' buttons (vertical at x 1265 from y 110, 643 long;
        // horizontal at y 753, 1265 long) -> the filled spans (y, x0, x1).
        let spans = |button: Rect, vertical: bool, start: bool| {
            fill_spans(&arrow_path(
                arrow_rect(button, vertical, start),
                vertical,
                start,
            ))
        };
        let v = |y: f32| Rect::from_min_size(Pos2::new(1265.0, y), Vec2::new(15.0, 18.0));
        let h = |x: f32| Rect::from_min_size(Pos2::new(x, 753.0), Vec2::new(18.0, 15.0));
        let up: Vec<_> = (0..5).map(|k| (116 + k, 1272 - k, 1273 + k)).collect();
        assert_eq!(spans(v(110.0), true, true), up);
        let down: Vec<_> = (0..5).map(|k| (741 + k, 1268 + k, 1277 - k)).collect();
        assert_eq!(spans(v(735.0), true, false), down);
        let width = |k: i32| 5 - (k - 4).abs();
        let left: Vec<_> = (0..9).map(|k| (756 + k, 10 - width(k), 10)).collect();
        assert_eq!(spans(h(0.0), false, true), left);
        let right: Vec<_> = (0..9).map(|k| (756 + k, 1254, 1254 + width(k))).collect();
        assert_eq!(spans(h(1247.0), false, false), right);
    }
}
