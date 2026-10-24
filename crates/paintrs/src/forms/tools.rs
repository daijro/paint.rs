//! The Tools form: a button per tool in a wrapping row of 30-px buttons (two to a row in the
//! 64-px window; the Shapes tool's wide icon fills the last row). The Extra tools (S§4.1) join at
//! their places, each with a plus at its button's top-right.
//! Port of: js/src/interface/window/form/ToolForm.js, css/window.css (`#toolForm`, "Consistent
//! form density")
//! Reference: paintdotjs @ b4e6fc5
use egui::{Pos2, Rect, Sense, Ui, Vec2};
use pdn_core::app::AppWorkspace;
use pdn_core::tool::types::ToolType;

use crate::kit::button::{self, MenuItemStyle};
use crate::kit::img::{self, Filter};
use crate::kit::px;
use crate::kit::tooltip::{self, Path};
use crate::theme::{self, *};

/// The button of `tool`: its icon file and size (`.large-icon` for the Shapes tool).
fn icon(tool: ToolType) -> (&'static str, Vec2) {
    if tool == ToolType::Shapes {
        ("shapes_tool_icon.png", TOOL_FORM_LARGE_ICON.into())
    } else {
        (tool.icon(), Vec2::splat(TOOL_FORM_ICON))
    }
}

/// `flex-wrap: wrap` with `flex-grow: 1`: the shown tools' button rects in `content` (each at
/// least 30 px or its icon plus padding, a row's spare width shared out equally).
fn layout(content: Rect, extras: bool) -> Vec<(ToolType, Rect)> {
    let (min_w, h) = TOOL_FORM_BUTTON;
    let base =
        |t: ToolType| min_w.max(icon(t).1.x + 2.0 * (TOOL_FORM_BUTTON_PADDING + MENU_ITEM_BORDER));
    let mut rows: Vec<Vec<(ToolType, f32)>> = vec![];
    let mut used = f32::INFINITY;
    for t in ToolType::shown(extras) {
        let b = base(t);
        if used + b > content.width() {
            rows.push(vec![]);
            used = 0.0;
        }
        rows.last_mut().expect("pushed").push((t, b));
        used += b;
    }
    let mut rects = vec![];
    for (r, row) in rows.iter().enumerate() {
        let spare = (content.width() - row.iter().map(|(_, b)| b).sum::<f32>()) / row.len() as f32;
        let mut x = content.min.x;
        for &(t, b) in row {
            let w = b + spare.max(0.0);
            rects.push((
                t,
                Rect::from_min_size(Pos2::new(x, content.min.y + r as f32 * h), Vec2::new(w, h)),
            ));
            x += w;
        }
    }
    rects
}

/// S§4.1: the plus at an Extra tool button's top-right, drawn by the app (icon packs need
/// nothing): a 6×6 plus of 2-px arms, 2 px in from the button's corner.
fn paint_corner_plus(painter: &egui::Painter, button: Rect, ppp: f32) {
    let plus = Rect::from_min_size(
        Pos2::new(button.max.x - 8.0, button.min.y + 2.0),
        Vec2::splat(6.0),
    );
    let c = plus.center();
    for arm in [
        Rect::from_center_size(c, Vec2::new(6.0, 2.0)),
        Rect::from_center_size(c, Vec2::new(2.0, 6.0)),
    ] {
        painter.rect_filled(px::snap_rect(arm, ppp), 0.0, TOOL_FORM_EXTRA_PLUS);
    }
}

/// Shows the buttons in `ui`'s rect; the active tool's is `[active]`. Returns the tool clicked
/// (`toolStripChooser.setSelectedId`).
pub fn show(ui: &mut Ui, app: &AppWorkspace) -> Option<ToolType> {
    let theme = theme::current(ui.ctx());
    let ppp = ui.ctx().pixels_per_point();
    let painter = ui.painter().clone();
    let mut picked = None;
    for (tool, rect) in layout(ui.max_rect(), true) {
        let snapped = px::snap_rect(rect, ppp);
        let response = ui.interact(
            snapped,
            ui.scope_id().with(("tool", tool.id())),
            Sense::click(),
        );
        // Stock ToolStripButtons (tooltips.md §3.1).
        tooltip::item(&response, Path::Stock, true, || {
            tooltip::tool_text(tool, true)
        });
        let state = MenuItemStyle::of(response.hovered(), false, app.tool_type == Some(tool), true);
        let fill = button::paint_menu_item_frame(
            &painter,
            snapped,
            state,
            &theme,
            theme.window_background,
        );
        let (name, size) = icon(tool);
        let inner = rect.shrink(MENU_ITEM_BORDER + TOOL_FORM_BUTTON_PADDING);
        // `justify-content: center`; the image fills the content height.
        let at = Pos2::new(inner.center().x - size.x / 2.0, inner.min.y);
        img::paint_icon(
            &painter,
            name,
            px::snap_rect(Rect::from_min_size(at, size), ppp),
            fill,
            Filter::None,
        );
        if tool.is_extra() {
            paint_corner_plus(&painter, snapped, ppp);
        }
        if response.clicked() {
            picked = Some(tool);
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;
    use ToolType::*;

    fn content() -> Rect {
        Rect::from_min_size(Pos2::new(11.0, 138.0), Vec2::new(64.0, 300.0))
    }

    /// The reference DOM: 32×30 buttons from (11, 138), two to a row, the Shapes button 64 wide.
    #[test]
    fn buttons_wrap_two_to_a_row() {
        let rects: Vec<Rect> = layout(content(), false)
            .into_iter()
            .map(|(_, r)| r)
            .collect();
        assert_eq!(rects.len(), 19);
        assert_eq!(
            rects[0],
            Rect::from_min_size(Pos2::new(11.0, 138.0), Vec2::new(32.0, 30.0))
        );
        assert_eq!(
            rects[3],
            Rect::from_min_size(Pos2::new(43.0, 168.0), Vec2::new(32.0, 30.0))
        );
        assert_eq!(
            rects[18],
            Rect::from_min_size(Pos2::new(11.0, 408.0), Vec2::new(64.0, 30.0))
        );
    }

    #[test]
    fn tools_window_has_19_paint_net_buttons_and_25_with_extras() {
        assert_eq!(layout(content(), false).len(), 19);
        assert_eq!(layout(content(), true).len(), 25);
        assert!(layout(content(), false).iter().all(|(t, _)| !t.is_extra()));
    }

    /// R§A.4, the user's rows: the two Extra selection tools under Lasso, the four retouch tools
    /// under Clone Stamp and Recolor, then Text and Line/Curve, Shapes alone at the bottom.
    #[test]
    fn tools_window_rows_match_the_user_layout() {
        let mut rows: Vec<Vec<ToolType>> = vec![];
        let mut top = f32::NAN;
        for (tool, rect) in layout(content(), true) {
            if rect.min.y != top {
                top = rect.min.y;
                rows.push(vec![]);
            }
            rows.last_mut().unwrap().push(tool);
        }
        assert_eq!(rows.len(), 13);
        assert_eq!(rows[0], [RectangleSelect, Move]);
        assert_eq!(rows[1], [LassoSelect, MoveSelection]);
        assert_eq!(rows[2], [MagneticLasso, ObjectSelection]);
        assert_eq!(rows[3], [EllipseSelect, Zoom]);
        assert_eq!(rows[8], [CloneStamp, Recolor]);
        assert_eq!(rows[9], [Remove, Patch]);
        assert_eq!(rows[10], [HealingBrush, SpotHealingBrush]);
        assert_eq!(rows[11], [Text, Line]);
        assert_eq!(rows[12], [Shapes]);
    }

    /// S§4.1: Paint.NET's fixed size for its 19 tools; three button rows taller with the Extras,
    /// so the chrome around the buttons stays the same.
    #[test]
    fn tools_window_grows_by_the_extra_rows() {
        assert_eq!(
            tool_form_size(ToolType::shown(false).count()),
            (64.0, 331.0)
        );
        let (w, h) = tool_form_size(ToolType::shown(true).count());
        assert_eq!((w, h), (64.0, 331.0 + 3.0 * TOOL_FORM_BUTTON.1));
        let height = |extras| {
            let rects = layout(content(), extras);
            rects.last().unwrap().1.max.y - rects[0].1.min.y
        };
        assert_eq!(h - height(true), 331.0 - height(false));
    }

    /// S§4.1: an Extra tool's button carries a 6×6 plus at its top-right, inside the button; the
    /// Paint.NET buttons have none.
    #[test]
    fn extra_tool_buttons_draw_a_corner_plus() {
        let ctx = egui::Context::default();
        let app = AppWorkspace::default();
        let mut output = ctx.run_ui(Default::default(), |ui| {
            show(
                &mut ui.new_child(egui::UiBuilder::new().max_rect(content())),
                &app,
            );
        });
        let mut arms: Vec<Rect> = output
            .shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Rect(r) if r.fill == TOOL_FORM_EXTRA_PLUS => Some(r.rect),
                _ => None,
            })
            .collect();
        output.textures_delta.clear();
        let buttons: Vec<_> = layout(content(), true)
            .into_iter()
            .filter(|(t, _)| t.is_extra())
            .collect();
        assert_eq!(arms.len(), 2 * buttons.len());
        for (tool, button) in buttons {
            let plus = Rect::from_min_size(
                Pos2::new(button.max.x - 8.0, button.min.y + 2.0),
                Vec2::splat(6.0),
            );
            assert!(button.contains_rect(plus), "{tool:?}");
            let (mine, rest): (Vec<Rect>, Vec<Rect>) =
                arms.into_iter().partition(|r| plus.contains_rect(*r));
            arms = rest;
            // A horizontal and a vertical 2-px arm through the middle.
            assert_eq!(mine.len(), 2, "{tool:?}");
            assert_eq!(mine[0].union(mine[1]), plus, "{tool:?}");
            assert!(mine.iter().all(|r| r.width().min(r.height()) == 2.0));
        }
    }
}
