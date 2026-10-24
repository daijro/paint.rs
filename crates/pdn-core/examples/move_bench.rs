//! Timing of the move tools' drag frames (Task 27p), release build:
//! `cargo run --release -p pdn-core --example move_bench [1080p|4k|noise]`. Not run by the gate.
//! A frame is the tool's part (the pointer move and `on_frame`: the drag applied, the layer
//! re-rendered and composited) plus the canvas view's (`mask_builds`, then `CanvasPixels::render`
//! as `CanvasView::show` calls it, without the upload), a frame every 16 ms.
//! - 1080p Select All: a rotate drag (right button) in each resampling mode, an NN translate
//!   drag and an NN nudge burst.
//! - 4K, a 42k-vertex wand patch, and 42k vertices of sparse noise over the whole view: a Move
//!   Selected Pixels and a Move Selection translate drag.
//! - 4K noise 30 % (`noise`): a Move Selection drag while its masks rebuild.
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdn_core::app::{AppEvent, AppWorkspace};
use pdn_core::render::Renderer;
use pdn_core::render::canvas::CanvasPixels;
use pdn_core::render::display::RenderMode;
use pdn_core::render::selection::mask_builds;
use pdn_core::render::view::{CanvasStyle, ViewTransform};
use pdn_core::selection::CombineMode;
use pdn_core::selection::matrix::Matrix;
use pdn_core::selection::path::{GraphicsPath, path_from_stencil};
use pdn_core::tool::types::ToolType;
use pdn_core::tool::{KeyPress, MouseButton, PointerInput, PointerSample, PointerType};
use pdn_core::util::bitvec2d::BitVector2D;
use pdn_core::util::geom::Rectangle;

/// The canvas view's state between frames.
struct View {
    canvas: CanvasPixels,
    image: Vec<u32>,
    style: CanvasStyle,
}

impl View {
    fn new(t: &ViewTransform) -> Self {
        let [w, h] = t.viewport;
        View {
            canvas: CanvasPixels::default(),
            image: vec![0; (w * h) as usize],
            style: CanvasStyle::for_scheme(false, true, None, 1.0, RenderMode::Hardware),
        }
    }

    /// `CanvasView::show` without the upload: the invalidations, then `CanvasPixels::render`.
    fn frame(&mut self, app: &mut AppWorkspace, t: &ViewTransform) {
        let stamp = mask_builds(Arc::new(|| {}));
        for e in app.drain_events() {
            if let AppEvent::DocumentInvalidated { rects } = e {
                self.canvas.invalidate(&rects);
            }
        }
        let ws = app.active_ws().unwrap();
        let image = &mut self.image;
        let out = move || {
            let image = image;
            &mut image[..]
        };
        let _ = self
            .canvas
            .render(ws, t, &self.style, stamp, out, u32::from_le_bytes);
    }
}

fn input(x: f64, y: f64) -> PointerInput {
    PointerInput {
        pointer_type: PointerType::Mouse,
        samples: vec![PointerSample {
            x,
            y,
            pressure: None,
            time_ms: 0.0,
        }],
    }
}

fn ms(s: Instant) -> f64 {
    s.elapsed().as_secs_f64() * 1000.0
}

/// A `w × h` document at 100 % in a `w × h` view, a gradient with alpha on its layer, `path`
/// selected.
fn setup(w: u32, h: u32, path: GraphicsPath) -> (AppWorkspace, ViewTransform) {
    let mut app = AppWorkspace {
        view_width: w as f64,
        view_height: h as f64,
        ..Default::default()
    };
    app.create_blank_document_in_new_workspace(w, h, 96.0, true);
    let ws = app.active_ws_mut().unwrap();
    let px = ws.document.layers.get_at_mut(0).unwrap().surface.data_mut();
    for (i, p) in px.chunks_mut(4).enumerate() {
        let (x, y) = ((i as u32 % w) as u8, (i as u32 / w) as u8);
        p.copy_from_slice(&[x, y, x ^ y, 128 | (x.wrapping_add(y) & 127)]);
    }
    ws.invalidate(None);
    ws.view.ppp = 1.0;
    ws.view.zoom = 1.0;
    ws.view.zoom_to_window = false;
    ws.view.center_view();
    ws.selection
        .set_continuation_path(path, CombineMode::Replace);
    ws.selection.commit_continuation();
    let t = ws.view.view_transform();
    app.drain_events();
    (app, t)
}

/// Mean and slowest (tool, canvas, total) ms over the frames.
#[derive(Default)]
struct Stats {
    tool: Vec<f64>,
    canvas: Vec<f64>,
}

impl Stats {
    fn line(&self, label: &str) {
        let total: Vec<f64> = self
            .tool
            .iter()
            .zip(&self.canvas)
            .map(|(a, b)| a + b)
            .collect();
        let f = |v: &[f64]| {
            let mean = v.iter().sum::<f64>() / v.len().max(1) as f64;
            let mut s = v.to_vec();
            s.sort_by(f64::total_cmp);
            let p95 = s[(s.len() * 95 / 100).min(s.len() - 1)];
            format!("{mean:.1} / p95 {p95:.1} / max {:.1}", s[s.len() - 1])
        };
        println!(
            "{label}: total {} ms (tool {}; canvas {}), {} frames",
            f(&total),
            f(&self.tool),
            f(&self.canvas),
            total.len()
        );
    }
}

/// Runs `step(app, i)` (the tool's input for frame i) then `on_frame` and the canvas frame, a
/// frame every 16 ms; the first `warm` frames are not counted.
fn drive(
    app: &mut AppWorkspace,
    v: &mut View,
    t: &ViewTransform,
    n: usize,
    mut step: impl FnMut(&mut AppWorkspace, usize),
) -> Stats {
    let mut s = Stats::default();
    for i in 0..n {
        let f = Instant::now();
        app.now_ms += 16.0;
        step(app, i);
        let now = app.now_ms;
        app.on_frame(now);
        let tool = ms(f);
        let c = Instant::now();
        v.frame(app, t);
        if i >= 2 {
            s.tool.push(tool);
            s.canvas.push(ms(c));
        }
        std::thread::sleep(Duration::from_millis(16).saturating_sub(f.elapsed()));
    }
    s
}

fn key(app: &mut AppWorkspace, k: &str) {
    app.on_key_press(&KeyPress {
        key: k.into(),
        shift: false,
        ctrl: false,
        alt: false,
        meta: false,
    });
}

/// 1080p Select All: rotate drags in each mode, an NN translate drag, an NN nudge burst.
fn select_all_1080p() {
    let (w, h) = (1920, 1080);
    let all = GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, w as f64, h as f64));
    for mode in [
        "highQualityCubic",
        "anisotropic",
        "linear",
        "multisampleLinear",
    ] {
        let (mut app, t) = setup(w, h, all.clone());
        app.update_tool_setting(ToolType::Move, "resampling", serde_json::json!(mode));
        app.set_active_tool_from_type(ToolType::Move);
        let mut v = View::new(&t);
        v.frame(&mut app, &t);
        // A right-drag from right of the centre around it: 1° a frame.
        let (cx, cy) = (w as f64 / 2.0, h as f64 / 2.0);
        let at = |deg: f64| {
            let a = deg.to_radians();
            (cx + 400.0 * a.cos(), cy + 400.0 * a.sin())
        };
        let (x, y) = at(0.0);
        app.on_mouse_down(x, y, MouseButton::Right, &input(x, y));
        let s = drive(&mut app, &mut v, &t, 62, |app, i| {
            let (x, y) = at(1.0 + i as f64);
            app.on_mouse_move(x, y, &input(x, y));
        });
        s.line(&format!("1080p Select All, rotate drag, {mode}"));
    }
    let (mut app, t) = setup(w, h, all.clone());
    app.set_active_tool_from_type(ToolType::Move);
    let mut v = View::new(&t);
    v.frame(&mut app, &t);
    app.on_mouse_down(500.0, 300.0, MouseButton::Left, &input(500.0, 300.0));
    let s = drive(&mut app, &mut v, &t, 62, |app, i| {
        let (x, y) = (501.0 + i as f64, 300.0 + (i / 2) as f64);
        app.on_mouse_move(x, y, &input(x, y));
    });
    s.line("1080p Select All, NN translate drag");
    app.on_mouse_up(600.0, 300.0, MouseButton::Left, &input(600.0, 300.0));
    let s = drive(&mut app, &mut v, &t, 32, |app, _| key(app, "ArrowRight"));
    s.line("1080p Select All, NN nudge burst");
}

/// A `w × h` noise stencil with `n` in `of` pixels set, traced.
fn noise_in(w: u32, h: u32, n: u64, of: u64) -> GraphicsPath {
    let mut stencil = BitVector2D::new(w as i32, h as i32);
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    for y in 0..h as i32 {
        for x in 0..w as i32 {
            // xorshift64
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            stencil.set(x, y, seed % of < n);
        }
    }
    path_from_stencil(&stencil, &Rectangle::new(0.0, 0.0, w as f64, h as f64))
}

/// The selection's masks landed from the workers.
fn land(app: &mut AppWorkspace, v: &mut View, t: &ViewTransform) {
    let s = Instant::now();
    loop {
        v.frame(app, t);
        let ws = app.active_ws().unwrap();
        let shown = ws.surface_box.renderers.iter().any(|(_, r)| {
            matches!(r, Renderer::Selection(s) if s.path.is_none() && s.bounds(ws, t).is_some())
        });
        if shown {
            println!("  (masks landed after {:.0} ms)", ms(s));
            return;
        }
        std::thread::sleep(Duration::from_millis(16));
    }
}

/// A translate drag of `path` at 4K, 1 px a frame, with `tool`.
fn drag_4k(label: &str, path: &GraphicsPath, tool: ToolType, from: (f64, f64), n: usize) {
    let (mut app, t) = setup(3840, 2160, path.clone());
    app.set_active_tool_from_type(tool);
    let mut v = View::new(&t);
    land(&mut app, &mut v, &t);
    app.on_mouse_down(from.0, from.1, MouseButton::Left, &input(from.0, from.1));
    let s = drive(&mut app, &mut v, &t, n, |app, i| {
        let (x, y) = (from.0 + 1.0 + i as f64, from.1);
        app.on_mouse_move(x, y, &input(x, y));
    });
    s.line(label);
}

/// The layer's FNV-1a after every frame of 1080p drags (`dump`: compared across versions): Select
/// All and a 64-gon, each quality, resampling mode and gamma; a rotate, a scale past 1/8 and an
/// NN translate.
fn dump() {
    let (w, h) = (1920, 1080);
    let gon: Vec<_> = (0..64)
        .map(|i| {
            let a = i as f64 * std::f64::consts::TAU / 64.0;
            pdn_core::util::geom::Point::new(900.0 + 700.0 * a.cos(), 520.0 + 430.0 * a.sin())
        })
        .collect();
    let shapes = [
        GraphicsPath::from_rect(&Rectangle::new(0.0, 0.0, w as f64, h as f64)),
        GraphicsPath::from_points(&gon),
    ];
    let fnv = |b: &[u8]| {
        b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| {
            (h ^ b as u64).wrapping_mul(0x100_0000_01b3)
        })
    };
    for (s, shape) in shapes.iter().enumerate() {
        let b = shape.get_bounds();
        for quality in ["high", "aliased"] {
            for mode in ["nearest", "linear", "multisampleLinear", "highQualityCubic"] {
                for gamma in [true, false] {
                    let label = format!("shape {s} {quality} {mode} gamma {gamma}");
                    let (mut app, _) = setup(w, h, shape.clone());
                    app.tool_settings.rendering_quality = quality.into();
                    app.update_tool_setting(ToolType::Move, "resampling", serde_json::json!(mode));
                    app.update_tool_setting(
                        ToolType::Move,
                        "gammaCorrected",
                        serde_json::json!(gamma),
                    );
                    app.set_active_tool_from_type(ToolType::Move);
                    let east = (b.right(), b.y + b.height / 2.0);
                    // Each drag: the button, the press, then a frame per point.
                    let drags = [
                        (
                            MouseButton::Right,
                            vec![
                                (1500.0, 500.0),
                                (1490.0, 560.0),
                                (1400.0, 800.0),
                                (1300.5, 900.2),
                            ],
                        ),
                        (
                            MouseButton::Left,
                            vec![east, (b.x + 60.0, east.1), (b.x + 300.3, east.1 + 9.0)],
                        ),
                        (
                            MouseButton::Left,
                            vec![
                                (b.x + 200.0, b.y + 150.0),
                                (b.x + 203.0, b.y + 151.0),
                                (b.x + 190.4, b.y + 140.6),
                            ],
                        ),
                    ];
                    for (button, points) in drags {
                        let (from, to) = (points[0], &points[1..]);
                        app.on_mouse_down(from.0, from.1, button, &input(from.0, from.1));
                        for &(x, y) in to {
                            app.now_ms += 16.0;
                            app.on_mouse_move(x, y, &input(x, y));
                            let now = app.now_ms;
                            app.on_frame(now);
                            let ws = app.active_ws().unwrap();
                            let px = ws.document.layers.get_at(0).unwrap().surface.data();
                            println!("{label}: {:016x}", fnv(px));
                        }
                        let (x, y) = to[to.len() - 1];
                        app.on_mouse_up(x, y, button, &input(x, y));
                    }
                    app.with_tool(|t, app| t.commit_pending(app));
                    let ws = app.active_ws().unwrap();
                    let px = ws.document.layers.get_at(0).unwrap().surface.data();
                    println!("{label}: commit {:016x}", fnv(px));
                }
            }
        }
    }
}

fn main() {
    let only = std::env::args().nth(1).unwrap_or_default();
    if only == "dump" {
        return dump();
    }
    if only.is_empty() || only == "1080p" {
        select_all_1080p();
    }
    if only.is_empty() || only == "4k" {
        let mut patch = noise_in(320, 200, 30, 100);
        let mut m = Matrix::identity();
        m.translate(1600.0, 900.0);
        patch = patch.transformed(&m);
        let k = patch.polygons.iter().map(Vec::len).sum::<usize>() / 1000;
        for tool in [ToolType::Move, ToolType::MoveSelection] {
            let label = format!("4K, {k}k-vertex wand patch, {tool:?} translate drag");
            drag_4k(&label, &patch, tool, (1650.0, 950.0), 122);
        }
        // The same vertex count spread over the whole view (a wand click on a sparse texture).
        let sparse = noise_in(3840, 2160, 1, 790);
        let k = sparse.polygons.iter().map(Vec::len).sum::<usize>() / 1000;
        for tool in [ToolType::Move, ToolType::MoveSelection] {
            let label = format!("4K, {k}k-vertex sparse wand selection, {tool:?} translate drag");
            drag_4k(&label, &sparse, tool, (1001.0, 1001.0), 122);
        }
    }
    if only == "noise" {
        let noise = noise_in(3840, 2160, 30, 100);
        let label = "4K noise 30 %, MoveSelection translate drag";
        drag_4k(label, &noise, ToolType::MoveSelection, (101.0, 101.0), 122);
    }
}
