//! Extra (paint.rs): the Click points option (`clickPoints`) of Lasso Select and the Patch tool's
//! selecting, after Photoshop's Polygonal Lasso and GIMP's Free Select. A click places a point and the border runs on from it
//! to the pointer with the button up; a drag from a point draws freehand. A click on the first
//! point, a double-click or Enter closes the border and selects it by Lasso Select's release rules
//! ([`select_path`]); Backspace or Delete takes the last point back, Esc cancels. A drag from the
//! first press, before any point is placed, is Paint.NET's lasso: its release selects. While it is
//! drawn the border shows as Lasso Select's selection does (the tinted continuation and the live
//! layer's marching ants).

use super::selection_tools::{
    begin_live, clip_shape, end_live, points_per_px, press_mode, select_path, show_live,
};
use super::{KeyPress, LiveGesture, MouseButton, PointerInput, ToolType};
use crate::app::AppWorkspace;
use crate::render::RendererId;
use crate::selection::path::GraphicsPath;
use crate::selection::{CombineMode, SelectionData};
use crate::util::geom::Point;

/// Windows' double-click time (ms).
const DOUBLE_CLICK_MS: f64 = 500.0;
/// Screen points: a held button that stays this close to its press clicks (and a second press
/// this close to the first double-clicks); a click this close to the first point closes.
const CLICK_REACH: f64 = 4.0;
const CLOSE_RADIUS: f64 = 8.0;

/// A border being placed (document px, floored as Lasso Select's trace).
#[derive(Default)]
pub(super) struct ClickLasso {
    /// The border from the first point; empty when none is drawn.
    points: Vec<Point>,
    /// Indices into `points` of the points clicks placed (freehand stretches end at one).
    anchors: Vec<usize>,
    /// Where the border runs on to.
    pointer: Point,
    /// The held button's press, until its release; whether it went farther than a click.
    press: Option<Point>,
    dragged: bool,
    /// The last click (time, point), for the double-click.
    last_click: Option<(f64, Point)>,
    /// The tool placing it (its combine mode and its history entry's name), the first press's
    /// combine mode, the selection then (the history memento's payload) and its path.
    tool: Option<ToolType>,
    mode: CombineMode,
    before: SelectionData,
    old_path: GraphicsPath,
}

fn floor(x: f64, y: f64) -> Point {
    Point::new(x.floor(), y.floor())
}

fn dist(a: Point, b: Point) -> f64 {
    (a.x - b.x).hypot(a.y - b.y)
}

impl ClickLasso {
    /// A border is being placed.
    pub(super) fn active(&self) -> bool {
        !self.points.is_empty()
    }

    /// A button is held.
    pub(super) fn pressed(&self) -> bool {
        self.press.is_some()
    }

    /// A press of `tool`: the first point (the combine mode from Lasso Select's modifiers), a
    /// point, or the close (on the first point, or a double-click).
    pub(super) fn down(
        &mut self,
        app: &mut AppWorkspace,
        tool: ToolType,
        x: f64,
        y: f64,
        b: MouseButton,
        layer: Option<RendererId>,
    ) {
        let (p, now) = (floor(x, y), app.now_ms);
        if self.active() {
            let double = self.last_click.is_some_and(|(t, q)| {
                now - t <= DOUBLE_CLICK_MS && dist(p, q) * points_per_px(app) <= CLICK_REACH
            });
            if double || self.near_start(app, p) {
                self.close(app, layer);
                return;
            }
            if self.points.last() != Some(&p) {
                self.points.push(p);
                self.anchors.push(self.points.len() - 1);
            }
        } else {
            (self.tool, self.mode) = (Some(tool), press_mode(app, tool, b));
            let Some(ws) = app.active_ws_mut() else {
                return;
            };
            self.before = ws.selection.save();
            self.old_path = ws.selection.create_path(true);
            begin_live(ws, self.mode);
            (self.points, self.anchors) = (vec![p], vec![0]);
        }
        self.last_click = Some((now, p));
        (self.press, self.dragged, self.pointer) = (Some(p), false, p);
        self.show(app, layer);
    }

    /// The pointer moved: the border runs on to it; a held button past a click draws freehand.
    pub(super) fn moved(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        i: &PointerInput,
        layer: Option<RendererId>,
    ) -> bool {
        if !self.active() {
            return false;
        }
        let p = floor(x, y);
        if let Some(q) = self.press {
            self.dragged |= dist(p, q) * points_per_px(app) > CLICK_REACH;
            if self.dragged {
                let samples = i.samples.iter().map(|s| floor(s.x, s.y));
                for s in samples.chain([p]) {
                    if self.points.last() != Some(&s) {
                        self.points.push(s);
                    }
                }
            }
        }
        self.pointer = p;
        self.show(app, layer);
        true
    }

    /// The held button's release is a last move. The first press's drag selects (Paint.NET's
    /// lasso); a later drag's end is a point; a click leaves the border open.
    pub(super) fn up(
        &mut self,
        app: &mut AppWorkspace,
        x: f64,
        y: f64,
        i: &PointerInput,
        layer: Option<RendererId>,
    ) -> bool {
        if self.press.is_none() {
            return false;
        }
        self.moved(app, x, y, i, layer);
        self.press = None;
        if std::mem::take(&mut self.dragged) {
            if self.anchors.len() == 1 {
                self.close(app, layer);
                return true;
            }
            let last = self.points.len() - 1;
            if self.anchors.last() != Some(&last) {
                self.anchors.push(last);
            }
            // A drag's release is no first click of a double-click.
            self.last_click = None;
        }
        self.show(app, layer);
        true
    }

    /// Enter closes the border, Esc cancels it, Backspace and Delete take the last point back.
    pub(super) fn key(
        &mut self,
        app: &mut AppWorkspace,
        key: &KeyPress,
        layer: Option<RendererId>,
    ) -> bool {
        if !self.active() || key.ctrl || key.meta {
            return false;
        }
        match key.key.as_str() {
            "Enter" => self.close(app, layer),
            "Escape" => self.cancel(app, layer),
            "Backspace" | "Delete" => self.remove_last(app, layer),
            _ => return false,
        }
        true
    }

    /// The border so far, on to the pointer, for the live gesture readouts.
    pub(super) fn live_gesture(&self, app: &AppWorkspace) -> Option<LiveGesture> {
        let shape = self.polygon(app);
        (shape.len() >= 2).then(|| LiveGesture::Lasso(shape.iter().map(|p| [p.x, p.y]).collect()))
    }

    /// `p` is on the first point of a border with something to close.
    fn near_start(&self, app: &AppWorkspace, p: Point) -> bool {
        self.points.len() >= 3 && dist(self.points[0], p) * points_per_px(app) <= CLOSE_RADIUS
    }

    /// Selects the border (without the stretch to the pointer) in the first press's mode, from
    /// the selection it began with.
    pub(super) fn close(&mut self, app: &mut AppWorkspace, layer: Option<RendererId>) {
        self.pointer = self.points.last().copied().unwrap_or_default();
        let polygon = self.polygon(app);
        let (mode, tool) = (self.mode, self.tool.unwrap_or(ToolType::LassoSelect));
        self.cancel(app, layer);
        if let Some(ws) = app.active_ws_mut() {
            let path = GraphicsPath::from_points(&polygon);
            select_path(ws, path, mode, tool);
        }
    }

    /// Drops the border: the selection is back as it began and the live layer empties.
    pub(super) fn cancel(&mut self, app: &mut AppWorkspace, layer: Option<RendererId>) {
        if !self.active() {
            return;
        }
        let old = std::mem::take(self);
        if let Some(ws) = app.active_ws_mut() {
            ws.selection.restore(old.before);
            end_live(ws, layer);
        }
    }

    /// Takes the last point (and the freehand stretch to it) back; with only the first left,
    /// cancels.
    fn remove_last(&mut self, app: &mut AppWorkspace, layer: Option<RendererId>) {
        if self.anchors.len() <= 1 {
            self.cancel(app, layer);
            return;
        }
        self.anchors.pop();
        let keep = self.anchors.last().map_or(1, |&a| a + 1);
        self.points.truncate(keep);
        self.show(app, layer);
    }

    /// The border on to the pointer, clipped as Lasso Select's polygon is.
    fn polygon(&self, app: &AppWorkspace) -> Vec<Point> {
        let mut shape = self.points.clone();
        if shape.last().is_some_and(|&l| l != self.pointer) {
            shape.push(self.pointer);
        }
        let doc = app
            .active_ws()
            .map_or((0, 0), |ws| (ws.document.width, ws.document.height));
        clip_shape(shape, self.mode, doc)
    }

    fn show(&self, app: &mut AppWorkspace, layer: Option<RendererId>) {
        let polygon = self.polygon(app);
        if let Some(ws) = app.active_ws_mut() {
            show_live(ws, layer, &polygon, self.mode, &self.old_path);
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use crate::app::AppWorkspace;
    use crate::testing::{Ev, drive_tool, fresh_doc};
    use crate::tool::{MouseButton::Left, ToolType};
    use crate::util::geom::Rectangle;

    fn app() -> AppWorkspace {
        let mut app = fresh_doc(100, 80, None);
        app.tool_settings
            .set(ToolType::LassoSelect, "clickPoints", true.into());
        app
    }
    fn click(x: f64, y: f64) -> [Ev; 3] {
        [Ev::Down(x, y, Left), Ev::Up(x, y, Left), Ev::Wait(600.0)]
    }
    fn clicks(points: &[(f64, f64)]) -> Vec<Ev> {
        points.iter().flat_map(|&(x, y)| click(x, y)).collect()
    }
    fn run(app: &mut AppWorkspace, events: &[Ev]) {
        drive_tool(app, ToolType::LassoSelect, events);
    }
    fn bounds(app: &AppWorkspace) -> Rectangle {
        app.active_ws()
            .unwrap()
            .selection
            .create_path(true)
            .get_bounds()
    }
    fn history(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }
    const SQUARE: [(f64, f64); 4] = [(10.0, 10.0), (60.0, 10.0), (60.0, 50.0), (10.0, 50.0)];

    /// Clicks place points and select nothing yet; a click on the first point selects them, one
    /// Lasso Select item.
    #[test]
    fn clicks_place_points_and_the_first_point_closes() {
        let mut app = app();
        let n = history(&app).len();
        run(&mut app, &clicks(&SQUARE));
        assert_eq!(history(&app).len(), n, "nothing selected yet");
        run(&mut app, &click(11.0, 11.0));
        assert_eq!(history(&app)[n..], ["Lasso Select"]);
        assert_eq!(bounds(&app), Rectangle::new(10.0, 10.0, 50.0, 40.0));
    }

    /// Enter and a double-click close too; Backspace takes the last point back; Esc cancels,
    /// leaving the selection as it was and no history.
    #[test]
    fn enter_double_click_backspace_and_escape() {
        let mut app = app();
        let mut events = clicks(&SQUARE);
        events.extend(click(90.0, 70.0));
        events.push(Ev::Key("Backspace"));
        events.push(Ev::Key("Enter"));
        run(&mut app, &events);
        assert_eq!(bounds(&app), Rectangle::new(10.0, 10.0, 50.0, 40.0));
        let mut app = self::app();
        let mut events = clicks(&SQUARE[..3]);
        events.extend([Ev::Down(10.0, 50.0, Left), Ev::Up(10.0, 50.0, Left)]);
        events.extend([Ev::Down(10.0, 50.0, Left), Ev::Up(10.0, 50.0, Left)]);
        run(&mut app, &events);
        assert_eq!(bounds(&app), Rectangle::new(10.0, 10.0, 50.0, 40.0));
        let (before, n) = (bounds(&app), history(&app).len());
        let mut events = clicks(&[(70.0, 5.0), (95.0, 5.0), (95.0, 30.0)]);
        events.push(Ev::Key("Escape"));
        run(&mut app, &events);
        assert_eq!((bounds(&app), history(&app).len()), (before, n));
    }

    /// The first press's drag is Paint.NET's lasso (its release selects); after a click, a drag
    /// draws freehand and the border stays open.
    #[test]
    fn drags_draw_freehand() {
        let mut app = app();
        let lasso = [
            Ev::Down(10.0, 10.0, Left),
            Ev::Move(60.0, 10.0),
            Ev::Move(60.0, 50.0),
            Ev::Wait(100.0),
            Ev::Up(10.0, 50.0, Left),
        ];
        run(&mut app, &lasso);
        assert_eq!(bounds(&app), Rectangle::new(10.0, 10.0, 50.0, 40.0));
        let mut app = self::app();
        let n = history(&app).len();
        let mut events = clicks(&[(10.0, 10.0)]);
        events.extend([
            Ev::Down(60.0, 10.0, Left),
            Ev::Move(70.0, 30.0),
            Ev::Up(60.0, 50.0, Left),
        ]);
        run(&mut app, &events);
        assert_eq!(
            history(&app).len(),
            n,
            "a later drag leaves the border open"
        );
        run(&mut app, &[Ev::Key("Enter")]);
        assert_eq!(bounds(&app), Rectangle::new(10.0, 10.0, 60.0, 40.0));
    }

    /// Off, Lasso Select is Paint.NET's: a click selects nothing and leaves no border open.
    #[test]
    fn off_is_paint_net() {
        let mut app = app();
        app.tool_settings
            .set(ToolType::LassoSelect, "clickPoints", false.into());
        let n = history(&app).len();
        run(&mut app, &clicks(&SQUARE));
        run(&mut app, &[Ev::Key("Enter")]);
        assert_eq!(history(&app).len(), n);
        assert!(app.active_ws().unwrap().selection.is_empty());
    }
}
