//! Ported from PhotoCraft crates/ui-egui/src/perspective_ui.rs @ e6d6fb45 (MIT OR Apache-2.0, used under MIT). Changes: one plane, so no linked planes, corner snapping, plane count or Remove Planes, and drawing a plane in Layout replaces it; the plane starts on the pixels' bounds (`warp_source`: the layer, or the selection's pixels lifted) and its corners stay a convex quad; the layer itself shows the warp, rendered by the effect's own kernel (the OK path) instead of a GPU-textured mesh; Straighten keeps a quad that would stop being convex; the options are paint.rs's tool options row (`tool_options`) with Paint.NET's Commit and Cancel buttons, and Crop to plane (not upstream) flattens the plane into the whole image; no control channel or `describe`.
//!
//! Layers ▸ Perspective Warp (an Extra) on the canvas, as a mode over the active tool: the
//! plane's four corners on the image at the current zoom. **Layout** (where it starts): drag the
//! corners onto the image's own features (a photographed sheet's corners), or drag elsewhere to
//! draw a new plane; the image stays. **Warp**: drag the corners and the layer follows; Straighten
//! levels and/or plumbs the plane's edges. Enter (or Commit) applies the warp as one history item
//! named "Perspective Warp", or with Crop to plane the plane flattened into an upright image as
//! "Perspective Crop"; Esc (or Cancel) leaves the document as it was. Meanwhile the tool takes no
//! press and no key; a tool or image switch and every commit-first command commit first, an undo
//! cancels (`AppWorkspace`).

use serde_json::{Value, json};

use crate::app::AppWorkspace;
use crate::document::Document;
use crate::document::layer::Layer;
use crate::effect::Values;
use crate::effect::extra::warp::{PERSPECTIVE, warp_source};
use crate::effect::session::EffectSession;
use crate::extra::perspective::{
    PerspectiveMap, Plane, Straighten, convex_quad, crop_size, rectify, straighten,
};
use crate::io::ops::{check_memory, replace_document, try_surface};
use crate::tool::CursorRequest;

/// A press this near a corner (DIP) takes it.
pub const HIT: f64 = 8.0;
/// Shown when there is nothing to warp.
pub const EMPTY: &str = "The active layer has no pixels here.";
pub const CROP_NAME: &str = "Perspective Crop";
const CROP_ICON: &str = "assets/icons/menu_image_crop_icon.png";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Layout,
    Warp,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Drag {
    /// Corner `k` and the pointer's offset from it.
    Corner {
        k: usize,
        offset: [f64; 2],
    },
    NewPlane {
        start: [f64; 2],
        cur: [f64; 2],
    },
}

pub struct PerspectiveWarp {
    /// The active layer and selection as the mode began; Cancel restores it, Commit applies.
    session: EffectSession,
    /// The plane's corners in Layout (the image's features) and in Warp (where they go),
    /// clockwise from top-left, layer pixels.
    pub src: [[f64; 2]; 4],
    pub dst: [[f64; 2]; 4],
    pub mode: Mode,
    /// Crop to plane: Commit flattens the plane into the whole image.
    pub crop: bool,
    drag: Option<Drag>,
    /// The pointer, for the cursor (layer px).
    hover: Option<[f64; 2]>,
    /// The plane the layer shows rendered; `None` while it shows the source.
    shown: Option<Plane>,
}

impl PerspectiveWarp {
    fn plane(&self) -> Plane {
        Plane {
            src: self.src,
            dst: self.dst,
        }
    }

    /// The effect's value: the plane.
    fn values(&self) -> Values {
        let mut v = Values::new();
        v.insert("planes".into(), json!([self.plane()]));
        v
    }

    /// The corners the canvas shows and the pointer drags: the source in Layout, else the
    /// destination.
    pub fn quad(&self) -> [[f64; 2]; 4] {
        match self.mode {
            Mode::Layout => self.src,
            Mode::Warp => self.dst,
        }
    }

    /// The rectangle being drawn in Layout (two opposite corners), if any.
    pub fn new_plane(&self) -> Option<([f64; 2], [f64; 2])> {
        match self.drag {
            Some(Drag::NewPlane { start, cur }) => Some((start, cur)),
            _ => None,
        }
    }

    fn corner_at(&self, p: [f64; 2], tol: f64) -> Option<usize> {
        let q = self.quad();
        let d = |k: usize| (q[k][0] - p[0]).hypot(q[k][1] - p[1]);
        (0..4)
            .filter(|&k| d(k) <= tol)
            .min_by(|&a, &b| d(a).total_cmp(&d(b)))
    }

    /// Corner `k` to `p`: in Layout both quads (the image stays), in Warp the destination; a
    /// move that would leave a quad concave stays where it was (upstream's `set_corner`).
    fn set_corner(&mut self, k: usize, p: [f64; 2]) {
        let (mut src, mut dst) = (self.src, self.dst);
        dst[k] = p;
        if self.mode == Mode::Layout {
            src[k] = p;
        }
        if convex_quad(&src) && convex_quad(&dst) {
            (self.src, self.dst) = (src, dst);
        }
    }
}

/// Layers ▸ Perspective Warp: the mode on the active layer, its plane on the pixels' bounds; a
/// tool error when there is nothing to warp. Runs after the commit-first hook.
pub fn begin(app: &mut AppWorkspace) -> bool {
    end(app, true);
    let Some(def) = crate::effect::find(PERSPECTIVE) else {
        return false;
    };
    let Some(session) = EffectSession::begin(app, def) else {
        return false;
    };
    let anchor = warp_source(session.source(), &session.env()).1;
    if anchor.is_empty() {
        app.ui_requests
            .push(crate::action::UiRequest::ToolError(EMPTY.into()));
        return false;
    }
    let (x0, y0, x1, y1) = (anchor.x0, anchor.y0, anchor.x1, anchor.y1);
    let q = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]].map(|p| p.map(f64::from));
    app.perspective = Some(PerspectiveWarp {
        session,
        src: q,
        dst: q,
        mode: Mode::Layout,
        crop: false,
        drag: None,
        hover: None,
        shown: None,
    });
    app.fire(crate::app::AppEvent::ActiveToolUpdated);
    true
}

/// Ends the mode: commits (Enter, Commit, a tool or image switch, a commit-first command) or
/// cancels (Esc, Cancel, an undo). Nothing without the mode.
pub fn end(app: &mut AppWorkspace, commit: bool) {
    let Some(p) = app.perspective.take() else {
        return;
    };
    app.fire(crate::app::AppEvent::ActiveToolUpdated);
    if !commit {
        p.session.cancel(app);
    } else if p.crop {
        crop(app, p);
    } else if PerspectiveMap::is_identity(&[p.plane()]) {
        p.session.cancel(app);
    } else {
        let values = p.values();
        p.session.commit(app, values);
    }
}

/// Crop to plane: every layer's source quad flattened onto the upright rectangle the destination
/// quad sizes ([`crop_size`]), as the document, in one "Perspective Crop" entry (as Image ▸ Crop
/// to Selection replaces it). The selection does not clip it.
fn crop(app: &mut AppWorkspace, p: PerspectiveWarp) {
    let (w, h) = crop_size(&p.dst);
    p.session.cancel(app);
    let Some(ws) = app.active_ws() else {
        return;
    };
    let d = &ws.document;
    if check_memory(d.layers.size(), w, h, 0).is_err() {
        return;
    }
    let mut doc = Document::new(w, h, d.resolution);
    for l in d.layers.iter() {
        let Ok(mut s) = try_surface(w, h) else {
            return;
        };
        let pixels = l.surface.get_image_data(0, 0, d.width, d.height);
        s.put_image_data(&rectify(&pixels, p.src, w, h), 0, 0);
        doc.layers.add_layer(Layer {
            properties: l.properties.clone(),
            surface: s,
            text: None,
        });
    }
    let active = ws.active_layer;
    replace_document(app, doc, active, CROP_NAME, CROP_ICON);
}

/// The layer shows the warp in Warp mode (rendered again only when the plane changed), else the
/// source.
// ponytail: a full-resolution render per pointer frame; a downsampled or GPU-mesh preview if big
// layers stutter.
fn refresh(app: &mut AppWorkspace) {
    let Some(p) = app.perspective.as_mut() else {
        return;
    };
    let plane = p.plane();
    let want = (p.mode == Mode::Warp && !PerspectiveMap::is_identity(std::slice::from_ref(&plane)))
        .then_some(plane);
    if want == p.shown {
        return;
    }
    let pixels = want.as_ref().map(|_| p.session.render(&p.values()));
    p.shown = want;
    let session = p.session.clone();
    session.show(app, pixels.as_ref());
}

/// Layout or Warp (the options row's Mode).
pub fn set_mode(app: &mut AppWorkspace, mode: Mode) {
    if let Some(p) = app.perspective.as_mut() {
        p.mode = mode;
        p.drag = None;
        // The status bar's hint follows.
        app.fire(crate::app::AppEvent::ActiveToolUpdated);
    }
    refresh(app);
}

/// Warp mode's Straighten buttons; a result that is not a convex quad is not taken.
pub fn straighten_plane(app: &mut AppWorkspace, how: Straighten) {
    let Some(p) = app.perspective.as_mut().filter(|p| p.mode == Mode::Warp) else {
        return;
    };
    let mut planes = [p.plane()];
    straighten(&mut planes, how);
    if convex_quad(&planes[0].dst) {
        p.dst = planes[0].dst;
    }
    refresh(app);
}

/// The doc-px tolerance for [`HIT`] DIP at the current zoom.
fn tolerance(app: &AppWorkspace) -> f64 {
    app.active_ws().map_or(HIT, |ws| {
        HIT * ws.view.ppp / ws.view.view_transform().scale.max(1e-6)
    })
}

/// A primary press at layer point `p`: a corner takes it, else in Layout it starts a new plane.
pub fn on_mouse_down(app: &mut AppWorkspace, p: [f64; 2]) {
    let tol = tolerance(app);
    let Some(s) = app.perspective.as_mut() else {
        return;
    };
    s.drag = match s.corner_at(p, tol) {
        Some(k) => {
            let c = s.quad()[k];
            Some(Drag::Corner {
                k,
                offset: [c[0] - p[0], c[1] - p[1]],
            })
        }
        None if s.mode == Mode::Layout => Some(Drag::NewPlane { start: p, cur: p }),
        None => None,
    };
}

pub fn on_mouse_move(app: &mut AppWorkspace, p: [f64; 2]) {
    let Some(s) = app.perspective.as_mut() else {
        return;
    };
    s.hover = Some(p);
    match s.drag {
        Some(Drag::Corner { k, offset }) => s.set_corner(k, [p[0] + offset[0], p[1] + offset[1]]),
        Some(Drag::NewPlane { start, .. }) => s.drag = Some(Drag::NewPlane { start, cur: p }),
        None => return,
    }
    refresh(app);
}

/// The release: a corner lands; a new plane over 4 × 4 px replaces the plane (upstream's rule).
pub fn on_mouse_up(app: &mut AppWorkspace, p: [f64; 2]) {
    on_mouse_move(app, p);
    let Some(s) = app.perspective.as_mut() else {
        return;
    };
    if let Some(Drag::NewPlane { start, cur }) = s.drag.take() {
        let (x0, y0) = (start[0].min(cur[0]), start[1].min(cur[1]));
        let (x1, y1) = (start[0].max(cur[0]), start[1].max(cur[1]));
        if x1 - x0 > 4.0 && y1 - y0 > 4.0 {
            let q = [[x0, y0], [x1, y0], [x1, y1], [x0, y1]];
            (s.src, s.dst) = (q, q);
        }
    }
    refresh(app);
}

/// The canvas pointer: a move over a corner, a crosshair where Layout draws a plane.
pub fn cursor(app: &AppWorkspace) -> CursorRequest {
    let Some(s) = &app.perspective else {
        return CursorRequest::Css("default");
    };
    let on_corner = s.drag.is_some_and(|d| matches!(d, Drag::Corner { .. }))
        || s.hover
            .is_some_and(|h| s.corner_at(h, tolerance(app)).is_some());
    match (on_corner, s.mode) {
        (true, _) => CursorRequest::Css("move"),
        (false, Mode::Layout) => CursorRequest::Css("crosshair"),
        (false, Mode::Warp) => CursorRequest::Css("default"),
    }
}

/// The status bar's hint while the mode runs (upstream's status line, per mode).
pub fn status_text(app: &AppWorkspace) -> Option<&'static str> {
    Some(match app.perspective.as_ref()?.mode {
        Mode::Layout => {
            "Perspective Warp: drag the corners onto the image's corners or edges (or drag elsewhere for a new plane), then pick Warp"
        }
        Mode::Warp => {
            "Perspective Warp: drag the corners and the image follows; Straighten squares the plane; Enter commits, Esc cancels"
        }
    })
}

/// The options row's value of `key` (`tool_options`).
pub fn option_value(app: &AppWorkspace, key: &str) -> Value {
    match (&app.perspective, key) {
        (Some(p), "mode") => json!(if p.mode == Mode::Warp {
            "warp"
        } else {
            "layout"
        }),
        (Some(p), "cropToPlane") => json!(p.crop),
        _ => Value::Null,
    }
}

pub fn set_option(app: &mut AppWorkspace, key: &str, value: &Value) {
    match key {
        "mode" => {
            let warp = value.as_str() == Some("warp");
            set_mode(app, if warp { Mode::Warp } else { Mode::Layout });
        }
        "cropToPlane" => {
            if let Some(p) = app.perspective.as_mut() {
                p.crop = value.as_bool().unwrap_or(false);
            }
        }
        _ => {}
    }
}

/// The options row's buttons.
pub fn action(app: &mut AppWorkspace, key: &str) {
    match key {
        "straightenHorizontal" => straighten_plane(app, Straighten::Horizontal),
        "straightenVertical" => straighten_plane(app, Straighten::Vertical),
        "straightenAuto" => straighten_plane(app, Straighten::Auto),
        "commit" => end(app, true),
        "cancel" => end(app, false),
        _ => {}
    }
}

/// The options row while the mode runs (`option_definitions` stands in for the tool's).
pub fn tool_options(app: &AppWorkspace) -> Vec<crate::tool::types::OptionDef> {
    let warp = app
        .perspective
        .as_ref()
        .is_some_and(|p| p.mode == Mode::Warp);
    crate::tool::types::perspective_warp_options(warp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::{ActionOutcome, UiRequest, perform};
    use crate::canvas::ImageData;
    use crate::extra::perspective::Homography;
    use crate::testing::{fresh_doc, layer_bytes};
    use crate::tool::{KeyPress, MouseButton, PointerInput, PointerType, ToolType};
    use crate::util::geom::Point;

    const W: u32 = 120;
    const H: u32 = 90;
    /// A sheet of paper photographed at an angle: its corners (TL, TR, BR, BL).
    const PAPER: [[f64; 2]; 4] = [[30.0, 20.0], [100.0, 30.0], [92.0, 78.0], [22.0, 66.0]];
    const DESK: [u8; 4] = [60, 60, 60, 255];
    const QUADRANTS: [[u8; 4]; 4] = [
        [230, 40, 40, 255],
        [40, 200, 60, 255],
        [40, 70, 230, 255],
        [240, 220, 40, 255],
    ];

    fn quadrant(u: f64, v: f64) -> [u8; 4] {
        QUADRANTS[usize::from(u >= 0.5) + 2 * usize::from(v >= 0.5)]
    }

    /// The paper's pixel at plane coordinates of `q`, or the desk.
    fn picture(q: [[f64; 2]; 4], w: u32, h: u32) -> ImageData {
        let inv = Homography::rect_to_quad([0.0, 0.0, 1.0, 1.0], q)
            .and_then(|m| m.inverse())
            .unwrap();
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let (u, v) = inv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
                let on = (0.0..1.0).contains(&u) && (0.0..1.0).contains(&v);
                data.extend(if on { quadrant(u, v) } else { DESK });
            }
        }
        ImageData {
            width: w,
            height: h,
            data,
        }
    }

    /// A 120 × 90 photo of the paper on a desk and the Paintbrush, Layers ▸ Perspective Warp
    /// started on it.
    fn photo() -> AppWorkspace {
        let mut app = fresh_doc(W, H, None);
        app.set_active_tool_from_type(ToolType::PaintBrush);
        let ws = app.active_ws_mut().unwrap();
        let l = ws.document.layers.get_at_mut(0).unwrap();
        l.surface.put_image_data(&picture(PAPER, W, H), 0, 0);
        ws.invalidate(None);
        assert_eq!(
            perform(&mut app, "menu.layers.perspectiveWarp"),
            ActionOutcome::Done
        );
        app
    }

    fn mode(app: &AppWorkspace) -> &PerspectiveWarp {
        app.perspective.as_ref().expect("the mode")
    }

    /// A left drag through the app's canvas routing (canvas points from layer points).
    fn drag(app: &mut AppWorkspace, from: [f64; 2], to: [f64; 2]) {
        let at = |app: &AppWorkspace, p: [f64; 2]| {
            let s = app
                .active_ws()
                .unwrap()
                .view
                .to_screen_position(Point::new(p[0], p[1]));
            (s.x, s.y)
        };
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        let (x, y) = at(app, from);
        app.on_mouse_down(x, y, MouseButton::Left, &input);
        for t in [0.3, 0.6, 1.0] {
            let p = [
                from[0] + (to[0] - from[0]) * t,
                from[1] + (to[1] - from[1]) * t,
            ];
            let (x, y) = at(app, p);
            app.on_mouse_move(x, y, &input);
        }
        let (x, y) = at(app, to);
        app.on_mouse_up(x, y, MouseButton::Left, &input);
    }

    fn key(app: &mut AppWorkspace, k: &str) -> bool {
        app.on_key_press(&KeyPress {
            key: k.into(),
            shift: false,
            ctrl: false,
            alt: false,
            meta: false,
        })
    }

    fn history(app: &AppWorkspace) -> Vec<String> {
        let ws = app.active_ws().unwrap();
        ws.history.undo.iter().map(|m| m.name.clone()).collect()
    }

    /// Layout: the plane's corners onto the paper's (the image stays).
    fn place_on_paper(app: &mut AppWorkspace) {
        let start = mode(app).src;
        for k in 0..4 {
            drag(app, start[k], PAPER[k]);
        }
        assert_eq!(mode(app).src, PAPER);
    }

    /// The desk's colour at any coverage (the warp's edge fades it).
    fn is_desk(p: &[u8]) -> bool {
        p[..3]
            .iter()
            .zip(DESK)
            .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 24)
    }

    /// The menu item starts the mode in Layout on the layer's bounds; Layout drags move the
    /// plane, not the image, and the tool sees none of them; drawing elsewhere replaces the plane.
    #[test]
    fn layout_drags_move_only_the_plane() {
        let mut app = photo();
        let before = layer_bytes(&app, None);
        let bounds = [[0.0, 0.0], [120.0, 0.0], [120.0, 90.0], [0.0, 90.0]];
        assert_eq!(
            (mode(&app).mode, mode(&app).src, mode(&app).dst),
            (Mode::Layout, bounds, bounds)
        );
        assert_eq!(
            app.tool_type,
            Some(ToolType::PaintBrush),
            "the brush would paint"
        );
        drag(&mut app, [0.0, 0.0], [30.0, 20.0]);
        let m = mode(&app);
        assert_eq!(m.src[0], [30.0, 20.0]);
        assert_eq!(m.dst, m.src, "the plane is still the identity");
        assert_eq!(
            layer_bytes(&app, None),
            before,
            "the image does not move in Layout"
        );
        assert_eq!(history(&app), ["New Image"]);
        // A drag away from the corners draws a new plane in its place.
        drag(&mut app, [50.0, 40.0], [80.0, 70.0]);
        let q = [[50.0, 40.0], [80.0, 40.0], [80.0, 70.0], [50.0, 70.0]];
        assert_eq!((mode(&app).src, mode(&app).dst), (q, q));
        assert_eq!(layer_bytes(&app, None), before);
        // Straighten waits for Warp.
        let defs = tool_options(&app);
        assert!(
            defs.iter()
                .filter(|d| d.key.starts_with("straighten"))
                .all(|d| d.disabled)
        );
    }

    /// Warp: the drags move the destination, the source stays, and the layer shows exactly what
    /// Commit applies; a corner never folds the quad.
    #[test]
    fn warp_drags_move_the_image() {
        let mut app = photo();
        let before = layer_bytes(&app, None);
        place_on_paper(&mut app);
        set_option(&mut app, "mode", &json!("warp"));
        assert_eq!(option_value(&app, "mode"), json!("warp"));
        assert!(tool_options(&app).iter().all(|d| !d.disabled));
        drag(&mut app, PAPER[0], [20.0, 10.0]);
        let m = mode(&app);
        assert_eq!((m.src, m.dst[0]), (PAPER, [20.0, 10.0]));
        let shown = layer_bytes(&app, None);
        assert_ne!(shown, before, "the image follows the corners");
        let render = m.session.render(&m.values());
        assert_eq!(shown, render.data, "the preview is the Commit kernel");
        // Across the opposite corner: it stops at the last convex position.
        drag(&mut app, [20.0, 10.0], [110.0, 85.0]);
        assert!(convex_quad(&mode(&app).dst), "{:?}", mode(&app).dst);
        // Back to Layout: the image as it was, the warp kept for Commit.
        set_mode(&mut app, Mode::Layout);
        assert_eq!(layer_bytes(&app, None), before);
        assert_ne!(mode(&app).dst, PAPER);
    }

    /// Enter commits one "Perspective Warp" history item (undo restores the layer); Esc leaves
    /// the layer, the history and the dirty flag as they were. Crop to plane off.
    #[test]
    fn enter_commits_and_esc_cancels() {
        let mut app = photo();
        let before = layer_bytes(&app, None);
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, [120.0, 0.0], [100.0, 12.0]);
        let warped = layer_bytes(&app, None);
        assert!(key(&mut app, "Enter"));
        assert!(app.perspective.is_none());
        assert_eq!(history(&app), ["New Image", "Perspective Warp"]);
        assert_eq!(layer_bytes(&app, None), warped);
        assert_eq!(app.active_ws().unwrap().document.width, W, "no crop");
        app.undo();
        assert_eq!(layer_bytes(&app, None), before);

        let mut app = photo();
        let dirty = app.active_ws().unwrap().dirty;
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, [120.0, 0.0], [100.0, 12.0]);
        assert!(key(&mut app, "Escape"));
        assert!(app.perspective.is_none());
        assert_eq!(history(&app), ["New Image"]);
        assert_eq!(
            (layer_bytes(&app, None), app.active_ws().unwrap().dirty),
            (before, dirty)
        );
        // An untouched plane commits nothing.
        perform(&mut app, "menu.layers.perspectiveWarp");
        key(&mut app, "Enter");
        assert_eq!(history(&app), ["New Image"]);
    }

    /// Layout onto the slanted paper, Warp, Straighten Auto, Commit: the paper comes out an
    /// upright rectangle, its quadrants where they belong.
    #[test]
    fn straighten_auto_squares_the_paper() {
        let mut app = photo();
        place_on_paper(&mut app);
        action(&mut app, "straightenAuto");
        assert_eq!(mode(&app).dst, PAPER, "only in Warp");
        set_mode(&mut app, Mode::Warp);
        action(&mut app, "straightenAuto");
        let d = mode(&app).dst;
        assert_eq!(d, [[26.0, 25.0], [96.0, 25.0], [96.0, 72.0], [26.0, 72.0]]);
        action(&mut app, "commit");
        assert_eq!(history(&app), ["New Image", "Perspective Warp"]);
        let out = layer_bytes(&app, None);
        let (mut inside, mut outside) = (0, 0);
        for y in 0..H {
            for x in 0..W {
                let (px, py) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
                let (u, v) = ((px - 26.0) / 70.0, (py - 25.0) / 47.0);
                let p = &out[((y * W + x) * 4) as usize..][..4];
                // Two pixels clear of the rectangle's edges and the quadrants' borders.
                let margin =
                    |t: f64, n: f64| [t, (t - 0.5).abs(), 1.0 - t].iter().all(|d| d * n > 2.0);
                if margin(u, 70.0) && margin(v, 47.0) {
                    let want = quadrant(u, v);
                    assert!(
                        p.iter()
                            .zip(want)
                            .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 12),
                        "({x}, {y}) {p:?} ≠ {want:?}"
                    );
                    inside += 1;
                } else if !(-2.0 / 70.0..=1.0 + 2.0 / 70.0).contains(&u)
                    || !(-2.0 / 47.0..=1.0 + 2.0 / 47.0).contains(&v)
                {
                    assert!(
                        p[3] == 0 || is_desk(p),
                        "({x}, {y}) {p:?}: paper outside the rectangle"
                    );
                    outside += 1;
                }
            }
        }
        assert!(inside > 2000 && outside > 4000, "{inside} {outside}");
    }

    /// Crop to plane: Commit flattens the paper into the whole image at the mean edge lengths,
    /// one "Perspective Crop" item; undo restores the size and the pixels.
    #[test]
    fn crop_to_plane_keeps_just_the_paper() {
        let mut app = photo();
        let before = layer_bytes(&app, None);
        place_on_paper(&mut app);
        set_option(&mut app, "cropToPlane", &json!(true));
        assert_eq!(option_value(&app, "cropToPlane"), json!(true));
        let (w, h) = crop_size(&PAPER);
        assert_eq!((w, h), (71, 48));
        key(&mut app, "Enter");
        assert_eq!(history(&app), ["New Image", CROP_NAME]);
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (w, h));
        let out = layer_bytes(&app, None);
        let mut checked = 0;
        for y in 0..h {
            for x in 0..w {
                let (u, v) = (
                    (f64::from(x) + 0.5) / f64::from(w),
                    (f64::from(y) + 0.5) / f64::from(h),
                );
                let margin = |t: f64, n: u32| {
                    [t, (t - 0.5).abs(), 1.0 - t]
                        .iter()
                        .all(|d| d * f64::from(n) > 2.0)
                };
                if margin(u, w) && margin(v, h) {
                    let p = &out[((y * w + x) * 4) as usize..][..4];
                    let want = quadrant(u, v);
                    assert!(
                        p.iter()
                            .zip(want)
                            .all(|(a, b)| (i32::from(*a) - i32::from(b)).abs() <= 12),
                        "({x}, {y}) {p:?} ≠ {want:?}"
                    );
                    checked += 1;
                }
            }
        }
        assert!(checked > 1500, "{checked}");
        app.undo();
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), (W, H));
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// The crop takes every layer, sized from the warped plane when it was moved in Warp.
    #[test]
    fn crop_takes_every_layer_and_the_warped_size() {
        let mut app = photo();
        perform(&mut app, "menu.layers.add.new.layer");
        assert!(
            app.perspective.is_none(),
            "a commit-first command ended the mode"
        );
        assert_eq!(app.active_ws().unwrap().document.layers.size(), 2);
        perform(&mut app, "menu.layers.perspectiveWarp");
        assert!(app.perspective.is_none(), "the new layer is empty");
        assert!(matches!(app.ui_requests.last(), Some(UiRequest::ToolError(e)) if e == EMPTY));
        app.active_ws_mut().unwrap().active_layer = 0;
        perform(&mut app, "menu.layers.perspectiveWarp");
        place_on_paper(&mut app);
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, PAPER[1], [110.0, 30.0]);
        set_option(&mut app, "cropToPlane", &json!(true));
        let size = crop_size(&mode(&app).dst);
        action(&mut app, "commit");
        let ws = app.active_ws().unwrap();
        assert_eq!((ws.document.width, ws.document.height), size);
        assert_eq!(ws.document.layers.size(), 2);
        assert_eq!(history(&app).last().map(String::as_str), Some(CROP_NAME));
    }

    /// The middle button still pans and View ▸ Zoom In zooms, the mode running on (its hint in the
    /// status bar); switching images commits it in its own image.
    #[test]
    fn pan_and_zoom_keep_the_mode_and_an_image_switch_commits() {
        let mut app = photo();
        assert!(status_text(&app).is_some_and(|t| t.contains("pick Warp")));
        set_mode(&mut app, Mode::Warp);
        assert!(status_text(&app).is_some_and(|t| t.contains("Enter commits")));
        let at = |app: &AppWorkspace| {
            app.active_ws()
                .unwrap()
                .view
                .to_screen_position(Point::new(0.0, 0.0))
        };
        perform(&mut app, "menu.view.zoomIn");
        assert!(app.perspective.is_some());
        let input = PointerInput {
            pointer_type: PointerType::Mouse,
            samples: vec![],
        };
        let before = at(&app);
        app.on_mouse_down(60.0, 40.0, MouseButton::Middle, &input);
        app.on_mouse_move(40.0, 30.0, &input);
        app.on_mouse_up(40.0, 30.0, MouseButton::Middle, &input);
        assert_ne!(at(&app), before, "panned");
        assert!(app.perspective.is_some());
        // Corners still land under the pointer after the zoom and the pan.
        drag(&mut app, [120.0, 90.0], [110.0, 80.0]);
        assert_eq!(mode(&app).dst[2], [110.0, 80.0]);
        let first = app.active;
        app.create_blank_document_in_new_workspace(40, 30, 96.0, true);
        assert!(app.perspective.is_none());
        let ws = &app.documents[first.unwrap()];
        let names: Vec<_> = ws.history.undo.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["New Image", "Perspective Warp"]);
    }

    /// A tool switch commits; an undo cancels and steps nothing else; a selection's pixels are
    /// lifted (the plane on the selection's bounds, nothing outside it changes).
    #[test]
    fn switches_commit_undo_cancels_and_selections_lift() {
        let mut app = photo();
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, [0.0, 0.0], [10.0, 6.0]);
        app.set_active_tool_from_type(ToolType::RectangleSelect);
        assert!(app.perspective.is_none());
        assert_eq!(history(&app), ["New Image", "Perspective Warp"]);
        let committed = layer_bytes(&app, None);
        perform(&mut app, "menu.layers.perspectiveWarp");
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, [0.0, 0.0], [10.0, 6.0]);
        assert!(app.undo());
        assert!(app.perspective.is_none());
        assert_eq!(
            history(&app),
            ["New Image", "Perspective Warp"],
            "nothing else undone"
        );
        assert_eq!(layer_bytes(&app, None), committed);

        // Save commits it even under Move Selected Pixels, whose transaction a save keeps.
        let mut app = photo();
        key(&mut app, "Escape");
        app.set_active_tool_from_type(ToolType::MoveSelection);
        perform(&mut app, "menu.layers.perspectiveWarp");
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, [0.0, 0.0], [10.0, 6.0]);
        assert_eq!(
            perform(&mut app, "menu.file.save"),
            ActionOutcome::Ui(UiRequest::Save)
        );
        assert!(app.perspective.is_none());
        assert_eq!(history(&app), ["New Image", "Perspective Warp"]);

        use crate::selection::CombineMode;
        use crate::util::geom::Rectangle;
        let mut app = photo();
        key(&mut app, "Escape");
        let before = layer_bytes(&app, None);
        let ws = app.active_ws_mut().unwrap();
        ws.selection.set_continuation(
            &Rectangle::new(20.0, 10.0, 60.0, 50.0),
            CombineMode::Replace,
        );
        ws.selection.commit_continuation();
        perform(&mut app, "menu.layers.perspectiveWarp");
        let q = [[20.0, 10.0], [80.0, 10.0], [80.0, 60.0], [20.0, 60.0]];
        assert_eq!(mode(&app).src, q);
        set_mode(&mut app, Mode::Warp);
        drag(&mut app, q[2], [70.0, 50.0]);
        key(&mut app, "Enter");
        let out = layer_bytes(&app, None);
        let inside = |x: u32, y: u32| (20..80).contains(&x) && (10..60).contains(&y);
        let mut vacated = false;
        for y in 0..H {
            for x in 0..W {
                let i = ((y * W + x) * 4) as usize;
                if !inside(x, y) {
                    assert_eq!(out[i..i + 4], before[i..i + 4], "({x}, {y})");
                } else {
                    vacated |= out[i + 3] == 0;
                }
            }
        }
        assert!(vacated, "the lifted pixels leave a hole");
    }
}
