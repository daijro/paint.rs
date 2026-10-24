//! The Pan tool, which `AppWorkspace` also keeps as the middle-button pan (`AppView.panTool`), and
//! the Zoom tool (Task 31).
//! Port of: js/src/engine/document/tool/tool/{PanTool,ZoomTool}.js
//! Reference: paintdotjs @ b4e6fc5
//! The middle drag pans through the app's pan; the zoom presets (`document::view`, Task 21) and
//! the zoom drag threshold are Paint.NET's.
use super::{CursorRequest, MouseButton, PointerInput, Tool, ToolType};
use crate::app::AppWorkspace;
use crate::pixel::IntRect;
use crate::render::selection::SelectionRenderer;
use crate::render::{Renderer, RendererId};
use crate::selection::path::GraphicsPath;
use crate::util::geom::{Point, Rectangle};

/// Drags the view: the scroll offset plus the pointer stays where it was pressed.
#[derive(Default)]
pub struct PanTool {
    /// The button whose press started the drag: only its release ends it (a second button
    /// pressed and released mid-drag reaches the tool too, `Tool` docs).
    button: Option<MouseButton>,
    /// `mouseClickX/Y`: the scroll position plus the pointer (canvas points) at the press.
    click: (f64, f64),
}

impl PanTool {
    pub fn is_tracking(&self) -> bool {
        self.button.is_some()
    }
}

impl Tool for PanTool {
    fn tool_type(&self) -> ToolType {
        ToolType::Pan
    }

    /// The pointer comes from `app.last_mouse` (screen position), not the document coordinates.
    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        let Some(ws) = app.active_ws() else {
            return false;
        };
        if self.button.is_some() {
            return true;
        }
        // `#view.scrollLeft/scrollTop`.
        let (sx, sy) = ws.view.dom_scroll_position();
        self.button = Some(b);
        self.click = (sx + app.last_mouse.0, sy + app.last_mouse.1);
        true
    }

    fn on_mouse_move(
        &mut self,
        app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        _i: &PointerInput,
    ) -> bool {
        if self.button.is_none() {
            return false;
        }
        let (mx, my) = app.last_mouse;
        // `scrollLeft/Top = click − pointer`: the DOM clamps it, and a Fit to Window view has no
        // scroll range at all.
        if let Some(ws) = app.active_ws_mut()
            && !ws.view.zoom_to_window
        {
            ws.view
                .set_view_position(self.click.0 - mx, self.click.1 - my);
        }
        true
    }

    fn on_mouse_up(
        &mut self,
        _app: &mut AppWorkspace,
        _x: f64,
        _y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        let ends = self.button == Some(b);
        if ends {
            self.button = None;
        }
        ends
    }

    fn cursor(&self) -> CursorRequest {
        CursorRequest::Image(if self.is_tracking() {
            "hand_closed_cursor"
        } else {
            "hand_open_cursor"
        })
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

/// §4 "Zoom drag threshold": 10 screen (device) px.
const ZOOM_DRAG_PX: f64 = 10.0;

/// The Zoom tool (§3.13): a left click zooms in one preset and a right click out, anchored at the
/// pointer; a left drag of 10 screen px or more shows its rectangle (corners ∩ the layer, the
/// integer bound) as marching ants and zooms to it on release; another button pressed meanwhile
/// moves the rectangle's origin with the pointer.
#[derive(Default)]
pub struct ZoomTool {
    /// The button whose press started the drag.
    button: Option<MouseButton>,
    /// The rectangle's corners (document px): its origin and the pointer.
    origin: Point,
    end: Point,
    /// The press point, from which the threshold is measured.
    down: Point,
    /// A second button: it and the pointer when it was last seen.
    second: Option<(MouseButton, Point)>,
    /// The rectangle has shown (the drag went ≥ 10 screen px).
    shown: bool,
    outline: Option<RendererId>,
}

impl ZoomTool {
    /// The corners ∩ the layer bounds.
    fn rect(&self, app: &AppWorkspace) -> Rectangle {
        let (w, h) = app.active_ws().map_or((0.0, 0.0), |ws| {
            (ws.document.width as f64, ws.document.height as f64)
        });
        let (a, b) = (self.origin, self.end);
        let r = Rectangle::absolute(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y));
        Rectangle::intersect_of(&r, &Rectangle::new(0.0, 0.0, w, h))
    }

    /// Device px per document px.
    fn scale(app: &AppWorkspace) -> f64 {
        app.active_ws()
            .map_or(1.0, |ws| ws.view.view_transform().scale)
    }

    /// The outline shows the rectangle's integer bound once the drag passed the threshold.
    fn sync_outline(&self, app: &mut AppWorkspace) {
        let path = (self.shown && self.button.is_some()).then(|| {
            let b = IntRect::bound_of(&self.rect(app));
            let (l, t) = (b.x as f64, b.y as f64);
            let (r, bt) = (l + b.w as f64, t + b.h as f64);
            GraphicsPath::from_points(&[
                Point::new(l, t),
                Point::new(r, t),
                Point::new(r, bt),
                Point::new(l, bt),
            ])
        });
        let (Some(id), Some(ws)) = (self.outline, app.active_ws_mut()) else {
            return;
        };
        if let Some(Renderer::Selection(s)) = ws.surface_box.renderer_mut(id) {
            s.visible = path.is_some();
            s.path = Some(path.unwrap_or_default());
        }
    }
}

impl Tool for ZoomTool {
    fn tool_type(&self) -> ToolType {
        ToolType::Zoom
    }

    fn on_activate(&mut self, app: &mut AppWorkspace) {
        if let Some(ws) = app.active_ws_mut() {
            self.outline = Some(ws.surface_box.add_renderer(Renderer::Selection(
                SelectionRenderer {
                    visible: false,
                    path: Some(GraphicsPath::default()),
                    ..Default::default()
                },
            )));
        }
    }

    fn on_deactivate(&mut self, app: &mut AppWorkspace) {
        (self.button, self.second, self.shown) = (None, None, false);
        if let (Some(id), Some(ws)) = (self.outline.take(), app.active_ws_mut()) {
            ws.surface_box.remove_renderer(id);
        }
    }

    fn on_mouse_down(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        _i: &PointerInput,
    ) -> bool {
        if !matches!(b, MouseButton::Left | MouseButton::Right) || app.active_ws().is_none() {
            return false;
        }
        let p = Point::new(x, y);
        if self.button.is_some() {
            self.second.get_or_insert((b, p));
            return true;
        }
        (self.button, self.origin, self.end, self.down) = (Some(b), p, p, p);
        self.shown = false;
        true
    }

    fn on_mouse_move(&mut self, app: &mut AppWorkspace, x: f64, y: f64, _i: &PointerInput) -> bool {
        if self.button.is_none() {
            return false;
        }
        let p = Point::new(x, y);
        if let Some((b, last)) = self.second {
            self.origin = Point::new(self.origin.x + p.x - last.x, self.origin.y + p.y - last.y);
            self.second = Some((b, p));
        }
        self.end = p;
        let moved = (p.x - self.down.x).hypot(p.y - self.down.y) * Self::scale(app);
        if self.button == Some(MouseButton::Left) && moved >= ZOOM_DRAG_PX {
            self.shown = true;
        }
        self.sync_outline(app);
        true
    }

    /// §3.13 steps 3–4: the release zooms to the rectangle (a left drag whose diagonal is at least
    /// 10 screen px), else one preset in (left) or out (right), anchored at the pointer (the
    /// viewport centre when it lies outside the view).
    fn on_mouse_up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        b: MouseButton,
        i: &PointerInput,
    ) -> bool {
        if self.second.is_some_and(|(s, _)| s == b) {
            self.second = None;
            return true;
        }
        if self.button != Some(b) {
            return self.button.is_some();
        }
        self.on_mouse_move(app, x, y, i);
        let r = self.rect(app);
        let scale = Self::scale(app);
        (self.button, self.second, self.shown) = (None, None, false);
        self.sync_outline(app);
        let Some(ws) = app.active_ws_mut() else {
            return true;
        };
        let v = &mut ws.view;
        let s = v.to_screen_position(Point::new(x, y));
        let inside = s.x >= 0.0 && s.y >= 0.0 && s.x < v.view_width && s.y < v.view_height;
        let anchor = inside.then_some((s.x, s.y));
        let diagonal = r.width.hypot(r.height) * scale;
        match b {
            MouseButton::Left if diagonal >= ZOOM_DRAG_PX => v.zoom_to_rectangle(&r),
            MouseButton::Left => {
                v.set_zoom_to_window(false);
                v.zoom_in(anchor);
            }
            _ => {
                v.set_zoom_to_window(false);
                v.zoom_out(anchor);
            }
        }
        true
    }

    /// paintdotjs's CSS cursors: zoom-out while the right button holds the drag.
    fn cursor(&self) -> CursorRequest {
        CursorRequest::Css(if self.button == Some(MouseButton::Right) {
            "zoom-out"
        } else {
            "zoom-in"
        })
    }

    /// The rectangle's corners are continuous canvas points.
    fn uses_continuous_pointer_coordinates(&self) -> bool {
        true
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::{PointerType, ToolType};

    /// AppView: the middle button pans with the app's PanTool whatever the active tool; the Pan
    /// tool does the same with the left button and shows the open hand when idle.
    #[test]
    fn middle_drag_pans_the_view() {
        let mut app = AppWorkspace {
            view_width: 500.0,
            view_height: 400.0,
            ..Default::default()
        };
        app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
        app.active_ws_mut().unwrap().view.set_zoom_center(1.0);
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_move(100.0, 100.0, &input);
        let (sx, sy) = app.active_ws().unwrap().view.dom_scroll_position();
        assert!(app.on_mouse_down(100.0, 100.0, MouseButton::Middle, &input));
        assert_eq!(app.cursor(), CursorRequest::Image("hand_closed_cursor"));
        app.on_mouse_move(70.0, 80.0, &input);
        let v = &app.active_ws().unwrap().view;
        assert_eq!((v.viewport_x, v.viewport_y), (sx + 30.0, sy + 20.0));
        assert!(app.on_mouse_up(70.0, 80.0, MouseButton::Middle, &input));
        assert_eq!(app.cursor(), CursorRequest::Css("default"));
        app.set_active_tool_from_type(ToolType::Pan);
        assert!(app.tool_active);
        assert_eq!(app.cursor(), CursorRequest::Image("hand_open_cursor"));
        assert!(app.on_mouse_down(70.0, 80.0, MouseButton::Left, &input));
        app.on_mouse_move(90.0, 80.0, &input);
        let v = &app.active_ws().unwrap().view;
        assert_eq!(v.viewport_x, sx + 10.0);
    }

    /// A second button pressed and released mid-drag neither re-anchors nor ends the pan; the
    /// starting button's release does.
    #[test]
    fn a_chord_button_does_not_end_the_pan() {
        let mut app = AppWorkspace {
            view_width: 500.0,
            view_height: 400.0,
            ..Default::default()
        };
        app.create_blank_document_in_new_workspace(800, 600, 96.0, true);
        app.active_ws_mut().unwrap().view.set_zoom_center(1.0);
        app.set_active_tool_from_type(ToolType::Pan);
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        app.on_mouse_move(100.0, 100.0, &input);
        let (sx, _) = app.active_ws().unwrap().view.dom_scroll_position();
        assert!(app.on_mouse_down(100.0, 100.0, MouseButton::Left, &input));
        app.on_mouse_move(90.0, 100.0, &input);
        app.on_mouse_down(90.0, 100.0, MouseButton::Right, &input);
        assert!(!app.on_mouse_up(90.0, 100.0, MouseButton::Right, &input));
        assert_eq!(app.cursor(), CursorRequest::Image("hand_closed_cursor"));
        app.on_mouse_move(80.0, 100.0, &input);
        assert_eq!(app.active_ws().unwrap().view.viewport_x, sx + 20.0);
        assert!(app.on_mouse_up(80.0, 100.0, MouseButton::Left, &input));
        assert_eq!(app.cursor(), CursorRequest::Image("hand_open_cursor"));
        // The middle pan likewise ignores another button's release.
        assert!(app.on_mouse_down(80.0, 100.0, MouseButton::Middle, &input));
        assert!(!app.on_mouse_up(80.0, 100.0, MouseButton::Left, &input));
        assert!(app.pan_tool.is_tracking());
        assert!(app.on_mouse_up(80.0, 100.0, MouseButton::Middle, &input));
        assert!(!app.pan_tool.is_tracking());
    }
}
