//! Paint.NET's tooltips: the widget the pointer is on
//! registers its tip during the frame ([`item`], [`control`]); [`show`] runs the one state
//! machine at the end of the frame and paints the tip in the tooltip layer.
//! - Item tips (toolstrips, menus): 400 ms after entering the item, motion inside it does not
//!   restart the wait; gone after 5000 ms, on leaving, on a press, or when the window loses focus,
//!   and back only after the pointer enters an item again. No fast re-show.
//! - The Colors window's swap icon (a WinForms control tip): 1000 ms with the pointer resting in
//!   the OS 4 × 4 px hover box, 10000 ms visible, not gated on window focus.
//!
//! The box approximates the OS tooltip (Q2): the 12-DIP shell face, 16-DIP lines, 5/3-DIP padding
//! in a 1-DIP border, 4-DIP corners (Windows 11's DWMWCP_ROUNDSMALL), the popup-menu shadow and
//! colours. No fade (Q6).
use egui::epaint::{Mesh, RectShape, TessellationOptions, Tessellator};
use egui::{Context, Id, LayerId, Order, Pos2, Rect, Response, Shape, Ui, UiBuilder, Vec2};
use pdn_core::tool::types::ToolType;

use super::px;
use super::shadow::{self, BoxShadow};
use super::text::{self, FontSpec};
use crate::theme::{DROP_MENU_SHADOW_BLUR, DROP_MENU_SHADOW_COLOR, DROP_MENU_SHADOW_OFFSET};

/// SPI_GETMOUSEHOVERTIME, the toolstrip hover timer (§4).
const ITEM_DELAY_MS: f64 = 400.0;
/// The strip ToolTip's AutoPopDelay.
const ITEM_VISIBLE_MS: f64 = 5000.0;
/// The swap icon's ToolTip: AutomaticDelay 1000 → InitialDelay 1000, AutoPopDelay 10000.
const CONTROL_DELAY_MS: f64 = 1000.0;
const CONTROL_VISIBLE_MS: f64 = 10000.0;
/// SPI_GETMOUSEHOVERWIDTH / HEIGHT, device px.
const HOVER_BOX_PX: f32 = 4.0;
/// The cursor height below the pointer's hotspot (SM_CYCURSOR at 100 %, the arrow's hotspot at
/// its tip), DIP.
// calibrate: tooltips Q3 — 40 px at 125 % unless the user enlarged the pointer.
const CURSOR_DROP: f32 = 32.0;
/// The tooltip font, NONCLIENTMETRICS' status font: Segoe UI 9 pt (the bundled Selawik, R47).
const SPEC: FontSpec = FontSpec::new(12.0).line_height(LINE);
/// GDI's 12-px line.
const LINE: f32 = 16.0;
/// Inside the 1-DIP border: left/right, top/bottom.
// calibrate: tooltips Q2 — a native 100 % Windows 11 capture of one tip.
const PADDING: Vec2 = Vec2::new(5.0, 3.0);
const BORDER: f32 = 1.0;
const RADIUS: f32 = 4.0;

/// Where an item tip lands (§5.1): `Pdn` = Paint.NET's own buttons and menu items, one device px
/// right of the pointer; `Stock` = WinForms' path, at the pointer's x.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Path {
    Stock,
    Pdn,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Item(Path),
    /// The swap icon's control tip (§9.3).
    Control,
}

impl Kind {
    fn delay(self) -> f64 {
        match self {
            Kind::Item(_) => ITEM_DELAY_MS,
            Kind::Control => CONTROL_DELAY_MS,
        }
    }

    fn visible(self) -> f64 {
        match self {
            Kind::Item(_) => ITEM_VISIBLE_MS,
            Kind::Control => CONTROL_VISIBLE_MS,
        }
    }
}

/// One frame's input.
#[derive(Clone, Copy, Debug)]
struct Frame {
    now: f64,
    pointer: Pos2,
    /// A mouse button went down this frame.
    pressed: bool,
    /// The window has OS focus.
    focused: bool,
    ppp: f32,
}

/// The tip state machine (§9.1, §9.3), one for the whole app (one tip at a time).
#[derive(Clone, Copy, Debug, Default)]
struct Machine {
    /// The item the pointer is on, its kind, and when the wait began (the entry; for the control
    /// tip the last rest).
    on: Option<(Id, Kind, f64)>,
    /// Where the control tip's hover box is centred.
    rest: Pos2,
    /// The pointer when the tip showed, and when.
    shown: Option<(Pos2, f64)>,
    /// Hidden by timeout, press or focus loss, or its wait ran out unfocused: nothing until the
    /// pointer enters an item again.
    spent: bool,
}

impl Machine {
    /// Advances to frame `f`, the pointer on `on` (`None`: on no item, a gap counts). Returns the
    /// ms until the next change without input, if any.
    fn step(&mut self, on: Option<(Id, Kind)>, f: Frame) -> Option<f64> {
        let Some((id, kind)) = on else {
            *self = Self::default();
            return None;
        };
        if self.on.map(|o| o.0) != Some(id) {
            *self = Self {
                on: Some((id, kind, f.now)),
                rest: f.pointer,
                ..Self::default()
            };
        }
        let item = matches!(kind, Kind::Item(_));
        let start = &mut self.on.as_mut().expect("entered above").2;
        if !item && self.shown.is_none() {
            let d = (f.pointer - self.rest).abs() * f.ppp;
            if d.x.max(d.y) > HOVER_BOX_PX / 2.0 {
                self.rest = f.pointer;
                *start = f.now;
            }
        }
        let start = *start;
        let expired = self
            .shown
            .is_some_and(|(_, at)| f.now - at >= kind.visible());
        let unfocused = item && !f.focused;
        // calibrate: tooltips Q4 — on the main-window strips Paint.NET may keep the tip on a click.
        if f.pressed || expired || (unfocused && self.shown.is_some()) {
            self.shown = None;
            self.spent = true;
        }
        if self.shown.is_none() && !self.spent && f.now - start >= kind.delay() {
            // The hover timer fires once: without focus it shows nothing until the next entry.
            if unfocused {
                self.spent = true;
            } else {
                self.shown = Some((f.pointer, f.now));
            }
        }
        match self.shown {
            Some((_, at)) => Some(at + kind.visible() - f.now),
            None if !self.spent => Some(start + kind.delay() - f.now),
            None => None,
        }
    }
}

/// What the pointer is on this frame (the last registration wins).
#[derive(Clone)]
struct Candidate {
    id: Id,
    kind: Kind,
    text: String,
}

#[derive(Clone, Default)]
struct Tips {
    machine: Machine,
    candidate: Option<Candidate>,
    /// The showing tip's text (fixed while it shows, §6.6) and box.
    shown: Option<(String, Rect)>,
}

fn id() -> Id {
    Id::unique("paintrs.kit.tooltip")
}

fn register(response: &Response, kind: Kind, text: impl FnOnce() -> String) {
    if !response.hovered() {
        return;
    }
    // The resx stores CR LF; an empty text has no tip (§2.2). A trailing line break would draw
    // an empty last line.
    let text = text().replace("\r\n", "\n").trim_end().to_owned();
    if text.is_empty() {
        return;
    }
    let candidate = Candidate {
        id: response.id,
        kind,
        text,
    };
    response
        .ctx
        .data_mut(|d| d.get_temp_mut_or_default::<Tips>(id()).candidate = Some(candidate));
}

/// The item tip of `response`'s toolstrip button or menu item, while the pointer is on it. A
/// disabled item still tips, on the stock path (§6.5). `text` runs only then.
pub fn item(response: &Response, path: Path, enabled: bool, text: impl FnOnce() -> String) {
    let path = if enabled { path } else { Path::Stock };
    register(response, Kind::Item(path), text);
}

/// The swap icon's control tip (§9.3).
pub fn control(response: &Response, text: impl FnOnce() -> String) {
    register(response, Kind::Control, text);
}

/// Runs the state machine for this frame at `now_ms` (the app clock) and paints the tip. Call
/// once, after every widget.
pub fn show(ctx: &Context, now_ms: f64) {
    let mut tips = ctx
        .data_mut(|d| d.get_temp::<Tips>(id()))
        .unwrap_or_default();
    let candidate = tips.candidate.take();
    let frame = ctx.input(|i| Frame {
        now: now_ms,
        pointer: i.pointer.hover_pos().unwrap_or_default(),
        pressed: i.pointer.any_pressed(),
        focused: i.focused,
        ppp: i.pixels_per_point,
    });
    let on = candidate.as_ref().map(|c| (c.id, c.kind));
    if let Some(ms) = tips.machine.step(on, frame) {
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(ms.max(0.0) / 1000.0));
    }
    match (tips.machine.shown, candidate) {
        (Some((anchor, _)), Some(c)) => {
            let text = tips.shown.take().map_or(c.text, |(t, _)| t);
            let rect = paint(ctx, &text, anchor, c.kind);
            tips.shown = Some((text, rect));
        }
        _ => {
            if tips.shown.take().is_some() {
                shadow::forget(ctx, id().with("shadow"));
            }
        }
    }
    ctx.data_mut(|d| d.insert_temp(id(), tips));
}

/// The tip showing: its text (`\n` line breaks) and border box. For tests.
pub fn shown(ctx: &Context) -> Option<(String, Rect)> {
    ctx.data(|d| d.get_temp::<Tips>(id())).and_then(|t| t.shown)
}

/// The border box of `lines` lines whose widest is `text_w`: one line is 24 DIP high.
fn box_size(text_w: f32, lines: usize) -> Vec2 {
    Vec2::new(text_w, lines as f32 * LINE) + 2.0 * (PADDING + Vec2::splat(BORDER))
}

/// The tip's top-left for a box of `size` shown with the pointer at `anchor` (§9.2, §9.3), kept
/// inside `screen` (Q1).
fn place(anchor: Pos2, size: Vec2, kind: Kind, screen: Rect, ppp: f32) -> Pos2 {
    let dx = if kind == Kind::Item(Path::Pdn) {
        1.0 / ppp
    } else {
        0.0
    };
    let mut at = anchor + Vec2::new(dx, CURSOR_DROP);
    // calibrate: tooltips Q1 — whether Windows shifts, flips or clips an item tip at the edges.
    if at.y + size.y > screen.max.y {
        at.y = match kind {
            Kind::Control => anchor.y - size.y,
            Kind::Item(_) => screen.max.y - size.y,
        };
    }
    at.x = at.x.min(screen.max.x - size.x);
    at.max(screen.min)
}

/// Paints `text`'s tip for the pointer at `anchor`; returns its border box.
fn paint(ctx: &Context, text: &str, anchor: Pos2, kind: Kind) -> Rect {
    let theme = crate::theme::current(ctx);
    let ppp = ctx.pixels_per_point();
    let screen = ctx.content_rect();
    let ui = Ui::new(
        ctx.clone(),
        id(),
        UiBuilder::new()
            .layer_id(LayerId::new(Order::Tooltip, id()))
            .max_rect(screen),
    );
    let painter = ui.painter();
    // Left-aligned, never wrapped; as wide as the widest line.
    let lines: Vec<&str> = text.split('\n').collect();
    let w = lines
        .iter()
        .map(|l| text::measure(painter, l, SPEC))
        .fold(0.0, f32::max);
    let size = box_size(w, lines.len());
    let rect = px::snap_rect(
        Rect::from_min_size(place(anchor, size, kind, screen, ppp), size),
        ppp,
    );
    let pixels = |t| super::img::texture_pixels(ctx, t);
    let drop = BoxShadow {
        offset: Vec2::splat(DROP_MENU_SHADOW_OFFSET),
        blur: DROP_MENU_SHADOW_BLUR,
        color: DROP_MENU_SHADOW_COLOR,
    };
    shadow::paint_box_shadow(&ui, id().with("shadow"), rect, drop, &pixels);
    // The rounded corners anti-aliased (the chrome's global tessellation has no feathering).
    let b = px::border_width(BORDER, ppp);
    let mut tessellator = Tessellator::new(ppp, TessellationOptions::default(), [1, 1], vec![]);
    let mut mesh = Mesh::default();
    for (r, radius, fill) in [
        (rect, RADIUS, theme.pdn_menu_border),
        (rect.shrink(b), RADIUS - b, theme.pdn_menu_background),
    ] {
        tessellator.tessellate_rect(&RectShape::filled(r, radius, fill), &mut mesh);
    }
    painter.add(Shape::mesh(mesh));
    text::occlude(ctx, rect);
    for (i, line) in lines.iter().enumerate() {
        let at = rect.min + Vec2::splat(BORDER) + PADDING + Vec2::new(0.0, i as f32 * LINE);
        text::draw(painter, at, line, SPEC, theme.pdn_text);
    }
    rect
}

/// The tool's shortcut chord (§3.1): `tool`'s hotkey upper-cased, once per tool sharing it from
/// the start of `tools` up to and including `tool`, joined by ", " (the presses that reach it).
pub fn chord(tools: &[ToolType], tool: ToolType) -> String {
    let end = tools.iter().position(|&t| t == tool).map_or(0, |i| i + 1);
    let n = tools[..end]
        .iter()
        .filter(|t| t.hot_key() == tool.hot_key())
        .count();
    vec![tool.hot_key().to_uppercase().to_string(); n].join(", ")
}

/// The Tools window's (and tool chooser's) tip: the tool tip format string applied to the name
/// and the chord in the Tools window order (`extras`: with the Extra tools). An Extra tool's tip
/// ends with "(from PhotoCraft)" (S§4.1 "Tooltips").
pub fn tool_text(tool: ToolType, extras: bool) -> String {
    let name = pdn_core::i18n::t(&format!("{}.name", tool.id()));
    let shown: Vec<_> = ToolType::shown(extras).collect();
    let text = pdn_core::i18n::tf(
        "toolsControl.toolToolTip.format",
        &[&name, &chord(&shown, tool)],
    );
    if tool.is_extra() {
        text + "\r\n" + crate::kit::menu::FROM_PHOTOCRAFT
    } else {
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ToolType::*;

    const NONE: Option<(Id, Kind)> = None;

    fn on(name: &str, kind: Kind) -> Option<(Id, Kind)> {
        Some((Id::unique(name), kind))
    }

    fn at(now: f64) -> Frame {
        Frame {
            now,
            pointer: Pos2::new(10.0, 10.0),
            pressed: false,
            focused: true,
            ppp: 1.0,
        }
    }

    const ITEM: Kind = Kind::Item(Path::Stock);

    /// A machine whose pointer entered item "a" at 0 ms.
    fn entered() -> Machine {
        let mut m = Machine::default();
        assert_eq!(m.step(on("a", ITEM), at(0.0)), Some(400.0));
        m
    }

    #[test]
    fn an_item_tip_shows_at_400_ms_not_399() {
        let mut m = entered();
        assert_eq!(m.step(on("a", ITEM), at(399.0)), Some(1.0));
        assert_eq!(m.shown, None);
        assert_eq!(m.step(on("a", ITEM), at(400.0)), Some(5000.0));
        assert_eq!(m.shown, Some((Pos2::new(10.0, 10.0), 400.0)));
    }

    #[test]
    fn moving_inside_the_item_keeps_the_wait_and_the_anchor() {
        let mut m = entered();
        let moved = |now, x| Frame {
            pointer: Pos2::new(x, 12.0),
            ..at(now)
        };
        m.step(on("a", ITEM), moved(200.0, 20.0));
        m.step(on("a", ITEM), moved(400.0, 25.0));
        // Shown where the pointer was at 400 ms, and it stays there.
        assert_eq!(m.shown, Some((Pos2::new(25.0, 12.0), 400.0)));
        m.step(on("a", ITEM), moved(900.0, 5.0));
        assert_eq!(m.shown, Some((Pos2::new(25.0, 12.0), 400.0)));
    }

    #[test]
    fn leaving_pressing_or_5000_ms_hide_it() {
        // Leave: onto no item (a gap).
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        assert_eq!(m.step(NONE, at(410.0)), None);
        assert_eq!(m.shown, None);
        // A press.
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        let press = Frame {
            pressed: true,
            ..at(450.0)
        };
        assert_eq!(m.step(on("a", ITEM), press), None);
        assert_eq!(m.shown, None);
        // 5000 ms after showing, not before.
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        assert_eq!(m.step(on("a", ITEM), at(5399.0)), Some(1.0));
        assert!(m.shown.is_some());
        assert_eq!(m.step(on("a", ITEM), at(5400.0)), None);
        assert_eq!(m.shown, None);
    }

    #[test]
    fn a_hidden_tip_waits_for_the_next_entry() {
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        m.step(on("a", ITEM), at(5400.0));
        // Still on the item: nothing, however long.
        assert_eq!(m.step(on("a", ITEM), at(60_000.0)), None);
        assert_eq!(m.shown, None);
        // A press before the 400 ms spends it too.
        let mut m = entered();
        let press = Frame {
            pressed: true,
            ..at(100.0)
        };
        m.step(on("a", ITEM), press);
        assert_eq!(m.step(on("a", ITEM), at(1000.0)), None);
        // Leave and re-enter: a full 400 ms again.
        m.step(NONE, at(1100.0));
        assert_eq!(m.step(on("a", ITEM), at(1200.0)), Some(400.0));
        m.step(on("a", ITEM), at(1600.0));
        assert!(m.shown.is_some());
    }

    /// One tip at a time, no fast re-show: the neighbour hides the old tip at once and waits the
    /// full 400 ms itself.
    #[test]
    fn the_next_item_hides_the_tip_and_waits_400_ms() {
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        assert_eq!(m.step(on("b", ITEM), at(500.0)), Some(400.0));
        assert_eq!(m.shown, None);
        m.step(on("b", ITEM), at(899.0));
        assert_eq!(m.shown, None);
        m.step(on("b", ITEM), at(900.0));
        assert_eq!(m.shown.map(|s| s.1), Some(900.0));
    }

    #[test]
    fn item_tips_need_the_window_focus() {
        let blurred = |now| Frame {
            focused: false,
            ..at(now)
        };
        // Focus lost while shown: hidden, and not back with the focus.
        let mut m = entered();
        m.step(on("a", ITEM), at(400.0));
        assert_eq!(m.step(on("a", ITEM), blurred(500.0)), None);
        assert_eq!(m.shown, None);
        m.step(on("a", ITEM), at(600.0));
        assert_eq!(m.shown, None);
        // The wait ran out unfocused: the timer fired once.
        let mut m = entered();
        m.step(on("a", ITEM), blurred(400.0));
        m.step(on("a", ITEM), at(500.0));
        assert_eq!(m.shown, None);
    }

    #[test]
    fn the_swap_icon_tip_rests_1000_ms_and_shows_10000_ms_unfocused() {
        const C: Kind = Kind::Control;
        let frame = |now, x: f32, ppp| Frame {
            pointer: Pos2::new(x, 10.0),
            focused: false,
            ppp,
            ..at(now)
        };
        let mut m = Machine::default();
        m.step(on("swap", C), frame(0.0, 10.0, 1.0));
        // Within the 4 x 4 px box (2 px either way) the rest goes on.
        m.step(on("swap", C), frame(500.0, 12.0, 1.0));
        m.step(on("swap", C), frame(999.0, 12.0, 1.0));
        assert_eq!(m.shown, None);
        assert_eq!(
            m.step(on("swap", C), frame(1000.0, 12.0, 1.0)),
            Some(10000.0)
        );
        assert!(m.shown.is_some());
        assert_eq!(m.step(on("swap", C), frame(10999.0, 12.0, 1.0)), Some(1.0));
        assert_eq!(m.step(on("swap", C), frame(11000.0, 12.0, 1.0)), None);
        // Beyond the box (3 device px at 1x; 1.5 points = 3 device px at 2x) the wait restarts.
        let mut m = Machine::default();
        m.step(on("swap", C), frame(0.0, 10.0, 1.0));
        m.step(on("swap", C), frame(600.0, 13.0, 1.0));
        m.step(on("swap", C), frame(1000.0, 13.0, 1.0));
        assert_eq!(m.shown, None);
        m.step(on("swap", C), frame(1600.0, 13.0, 1.0));
        assert!(m.shown.is_some());
        let mut m = Machine::default();
        m.step(on("swap", C), frame(0.0, 10.0, 2.0));
        m.step(on("swap", C), frame(600.0, 11.5, 2.0));
        m.step(on("swap", C), frame(1000.0, 11.5, 2.0));
        assert!(m.shown.is_none());
    }

    #[test]
    fn placement_drops_32_dip_and_stays_in_the_viewport() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let size = Vec2::new(100.0, 24.0);
        let p = Pos2::new(50.0, 60.0);
        let pdn = Kind::Item(Path::Pdn);
        assert_eq!(place(p, size, ITEM, screen, 1.25), Pos2::new(50.0, 92.0));
        // One device px right of the pointer.
        assert_eq!(place(p, size, pdn, screen, 1.25), Pos2::new(50.8, 92.0));
        // Right and bottom overflow shift the item tip in; the control tip goes above.
        let corner = Pos2::new(780.0, 590.0);
        assert_eq!(
            place(corner, size, ITEM, screen, 1.0),
            Pos2::new(700.0, 576.0)
        );
        assert_eq!(
            place(corner, size, Kind::Control, screen, 1.0),
            Pos2::new(700.0, 566.0)
        );
        // A one-line box is 24 DIP high; two lines 40.
        assert_eq!(box_size(30.0, 1), Vec2::new(42.0, 24.0));
        assert_eq!(box_size(30.0, 2).y, 40.0);
    }

    /// Disabled items tip too, on the stock path.
    #[test]
    fn disabled_items_tip_on_the_stock_path() {
        let ctx = Context::default();
        let rect = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(20.0));
        let mut kinds = vec![];
        for _ in 0..3 {
            let mut input = egui::RawInput::default();
            input
                .events
                .push(egui::Event::PointerMoved(Pos2::new(20.0, 20.0)));
            let mut out = ctx.run_ui(input, |ui| {
                let r = ui.interact(rect, Id::unique("button"), egui::Sense::click());
                item(&r, Path::Pdn, false, || "Undo".into());
            });
            out.textures_delta.clear();
            kinds.push(
                ctx.data_mut(|d| d.get_temp_mut_or_default::<Tips>(id()).candidate.take())
                    .map(|c| (c.kind, c.text)),
            );
        }
        assert_eq!(
            kinds.last().cloned().flatten(),
            Some((Kind::Item(Path::Stock), "Undo".into()))
        );
    }

    /// tooltips.md §9.4.
    #[test]
    fn chords_follow_the_tools_window_order() {
        let paint_net: Vec<_> = ToolType::shown(false).collect();
        for (tool, c) in [
            (RectangleSelect, "S"),
            (LassoSelect, "S, S"),
            (EllipseSelect, "S, S, S"),
            (MagicWand, "S, S, S, S"),
            (Move, "M"),
            (MoveSelection, "M, M"),
            (Line, "O"),
            (Shapes, "O, O"),
            (Zoom, "Z"),
        ] {
            assert_eq!(chord(&paint_net, tool), c, "{tool:?}");
        }
        // Extras sit between Paint.NET tools in the strip, but never count while hidden.
        for (tool, c) in [(Shapes, "O, O"), (MagicWand, "S, S, S, S")] {
            assert_eq!(tool_text(tool, false).rsplit(": ").next(), Some(c));
        }
        assert_eq!(tool_text(Zoom, false), "Zoom\r\nShortcut key: Z");
        assert_eq!(tool_text(Shapes, false), "Shapes\r\nShortcut key: O, O");
        assert_eq!(
            tool_text(EllipseSelect, false),
            "Ellipse Select\r\nShortcut key: S, S, S"
        );
    }

    /// S§4.1 "Tooltips": with the Extra tools shown the chords count them, and an Extra tool's
    /// tip ends with "(from PhotoCraft)".
    #[test]
    fn extra_tool_tips_count_the_extras_and_say_so() {
        let all: Vec<_> = ToolType::shown(true).collect();
        for (tool, c) in [
            (MagneticLasso, "S, S, S"),
            (ObjectSelection, "S, S, S, S"),
            (EllipseSelect, "S, S, S, S, S"),
            (MagicWand, "S, S, S, S, S, S"),
            (Remove, "J"),
            (SpotHealingBrush, "J, J, J, J"),
        ] {
            assert_eq!(chord(&all, tool), c, "{tool:?}");
        }
        assert_eq!(
            tool_text(Patch, true),
            "Patch\r\nShortcut key: J, J\r\n(from PhotoCraft)"
        );
        assert_eq!(
            tool_text(EllipseSelect, true),
            "Ellipse Select\r\nShortcut key: S, S, S, S, S"
        );
    }
}
