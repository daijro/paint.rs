//! Timing of the canvas view's overlay path (`paintrs::shell::canvas_view`), release build:
//! `cargo run --release -p pdn-core --example ants_bench`. Not run by the gate.
//! - Select All with the tint: ants frames at 1080p and 4K (the frame-only span refresh, and the
//!   full overlay-rect refresh every frame took before Task 25p), and a Replace-drag step at 4K.
//! - A noise selection at 4K (a Magic Wand Global click on a noisy photo): salt-and-pepper
//!   stencils 30 % and 50 % set, traced into islands and holes. The mask build in the frame (the
//!   first refresh: every mask, as without workers), the first ants cycle and steady ants frames.
//! - Task 25q, the masks built on the workers (`mask_builds`, as the canvas view does): from a
//!   selection change, a frame every 16 ms until its masks land; the slowest frame and the time
//!   to the frame that shows them (the final ants), and the process's peak memory.
//! - Task 25q fix round 1: a selection moving each frame while its masks rebuild (a Move Selection
//!   drag through `set_interim_transform`, or a scroll), and a Replace-mode drag over a committed
//!   noise selection.
use std::sync::Arc;
use std::time::{Duration, Instant};

use pdn_core::app::AppWorkspace;
use pdn_core::document::workspace::DocumentWorkspace;
use pdn_core::pixel::IntRect;
use pdn_core::render::display::{DisplayTarget, RenderMode, span_bands};
use pdn_core::render::selection::{SelectionRenderer, mask_builds};
use pdn_core::render::view::ViewTransform;
use pdn_core::selection::CombineMode;
use pdn_core::selection::matrix::Matrix;
use pdn_core::selection::path::{GraphicsPath, path_from_stencil};
use pdn_core::tool::types::ToolType;
use pdn_core::tool::{MouseButton, PointerInput, PointerSample, PointerType};
use pdn_core::util::bitvec2d::BitVector2D;
use pdn_core::util::geom::Rectangle;

/// The canvas view's state between frames.
struct View {
    target: DisplayTarget,
    image: Vec<u32>,
    prev: Option<IntRect>,
    key: Option<(u64, u64, u64)>,
    ants: u64,
}

impl View {
    fn new(w: u32, h: u32) -> Self {
        let mut target = DisplayTarget::new(w, h, RenderMode::Hardware);
        target.fill(IntRect::new(0, 0, w, h), [255, 255, 255, 255]);
        View {
            target,
            image: vec![0; (w * h) as usize],
            prev: None,
            key: None,
            ants: 0,
        }
    }

    /// `CanvasView::show`'s overlay refresh (the base unchanged); `full` forces the overlay-rect
    /// path for a frame-only change (every ants frame took it before Task 25p). Partial uploads
    /// are timed as the crops `Tiles` makes.
    fn frame(&mut self, ws: &DocumentWorkspace, t: &ViewTransform, full: bool) {
        self.frame_at(ws, t, full, 0);
    }

    /// `frame` after the mask builds stamp `stamp` (`mask_builds`).
    fn frame_at(&mut self, ws: &DocumentWorkspace, t: &ViewTransform, full: bool, stamp: u64) {
        let all = IntRect::new(0, 0, t.viewport[0], t.viewport[1]);
        let key = (ws.selection.revision(), ws.surface_box.revision(), stamp);
        let ants = ws.surface_box.ants_revision();
        let content = full || self.key != Some(key);
        let frame_only = !content && self.ants != ants;
        let spans = frame_only
            .then(|| ws.surface_box.overlay_spans(ws, t))
            .flatten();
        let paint = ws.surface_box.overlay_painter(ws, t, self.target.mode);
        if let Some(spans) = spans {
            let px = &mut self.image;
            self.target
                .write_rgba8_over(all, Some(&spans), px, u32::from_le_bytes, paint);
            for r in span_bands(&spans, 32) {
                std::hint::black_box(crop(&self.image, all.w, r));
            }
        } else if content || frame_only {
            let drawn = ws
                .surface_box
                .overlay_bounds(ws, t)
                .map(|r| r.intersect(&all));
            let refresh = [self.prev, drawn]
                .into_iter()
                .flatten()
                .fold(IntRect::default(), |a, b| a.union(&b));
            let px = &mut self.image;
            self.target
                .write_rgba8_over(refresh, None, px, u32::from_le_bytes, paint);
            if refresh != all {
                std::hint::black_box(crop(&self.image, all.w, refresh));
            }
            self.prev = drawn;
            self.key = Some(key);
        }
        self.ants = ants;
    }
}

fn crop(px: &[u32], w: u32, r: IntRect) -> Vec<u32> {
    let rows =
        (r.y..r.bottom()).flat_map(|y| &px[(y as u32 * w + r.x as u32) as usize..][..r.w as usize]);
    rows.copied().collect()
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

/// A `w × h` document at 100 % in a `w × h` view, Rectangle Select active (the tint on), with
/// `path` selected.
fn setup(w: u32, h: u32, path: GraphicsPath) -> (AppWorkspace, ViewTransform) {
    let t = ViewTransform {
        scale: 1.0,
        origin: [0.0, 0.0],
        viewport: [w, h],
        ui_scale: 1.0,
    };
    let mut app = AppWorkspace {
        view_width: w as f64,
        view_height: h as f64,
        ..Default::default()
    };
    app.create_blank_document_in_new_workspace(w, h, 96.0, true);
    app.set_active_tool_from_type(ToolType::RectangleSelect);
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation_path(path, CombineMode::Replace);
    ws.selection.commit_continuation();
    (app, t)
}

/// Mean ms of `n` ants frames from frame `f0` (`full`: the overlay-rect refresh every frame).
fn ants(
    app: &mut AppWorkspace,
    v: &mut View,
    t: &ViewTransform,
    f0: u32,
    n: u32,
    full: bool,
) -> f64 {
    let s = Instant::now();
    for f in f0..f0 + n {
        app.active_ws_mut()
            .unwrap()
            .surface_box
            .set_ants_frame(f % 8);
        v.frame(app.active_ws().unwrap(), t, full);
    }
    ms(s) / n as f64
}

fn select_all(w: u32, h: u32, drag: bool) {
    let rect = Rectangle::new(0.0, 0.0, w as f64, h as f64);
    let (mut app, t) = setup(w, h, GraphicsPath::from_rect(&rect));
    let mut v = View::new(w, h);
    // The selection change (the masks with frame 0), then the first ants frame (the other dashed
    // frames and the spans), then steady frames.
    let build = ants(&mut app, &mut v, &t, 0, 1, false);
    let first = ants(&mut app, &mut v, &t, 1, 1, false);
    ants(&mut app, &mut v, &t, 2, 8, false);
    let span = ants(&mut app, &mut v, &t, 2, 48, false);
    let full = ants(&mut app, &mut v, &t, 2, 48, true);
    print!(
        "Select All {w}x{h}: selection change {build:.1} ms, first ants frame {first:.1} ms; \
         ants frame {span:.2} ms (full refresh {full:.2} ms)"
    );
    if drag {
        // A Replace drag over the whole view: tool move + refresh per step.
        app.now_ms = 1000.0;
        app.on_mouse_down(5.0, 5.0, MouseButton::Left, &input(5.0, 5.0));
        let steps = 24;
        let s = Instant::now();
        for i in 1..=steps {
            let k = i as f64 / steps as f64;
            let (x, y) = (5.0 + k * (w as f64 - 10.0), 5.0 + k * (h as f64 - 10.0));
            app.now_ms += 16.0;
            app.on_mouse_move(x, y, &input(x, y));
            v.frame(app.active_ws().unwrap(), &t, false);
        }
        print!(", drag step {:.2} ms", ms(s) / steps as f64);
    }
    println!();
}

fn noise(w: u32, h: u32, percent: u32) {
    let (path, trace) = noise_path(w, h, percent);
    let polys = path.polygons.len();
    let s = Instant::now();
    let (mut app, t) = setup(w, h, path);
    let commit = ms(s);
    let mut v = View::new(w, h);
    let s = Instant::now();
    app.active_ws_mut().unwrap().surface_box.set_ants_frame(0);
    v.frame(app.active_ws().unwrap(), &t, false);
    let build = ms(s);
    let s = Instant::now();
    let first = (1..8).map(|f| {
        let s = Instant::now();
        app.active_ws_mut().unwrap().surface_box.set_ants_frame(f);
        v.frame(app.active_ws().unwrap(), &t, false);
        ms(s)
    });
    let first: Vec<f64> = first.collect();
    let cycle = ms(s);
    let span = ants(&mut app, &mut v, &t, 0, 16, false);
    let full = ants(&mut app, &mut v, &t, 0, 16, true);
    let ws = app.active_ws().unwrap();
    let spans = ws.surface_box.overlay_spans(ws, &t);
    let spans = spans.map_or("dense: full refresh".into(), |s| {
        let covered: u64 = s.iter().map(|s| s.w as u64).sum();
        format!(
            "{} spans, {:.0} % of the view",
            s.len(),
            covered as f64 * 100.0 / (w * h) as f64
        )
    });
    println!(
        "noise {percent}% {w}x{h}: {polys} polygons, trace {trace:.0} ms, commit {commit:.0} ms; \
         first refresh (mask build) {build:.0} ms; first cycle frames 1-7 {cycle:.0} ms \
         (max {:.0}); ants frame {span:.2} ms (full refresh {full:.2} ms); {spans}",
        first.iter().copied().fold(0.0, f64::max),
    );
}

/// The 4K noise stencil (`percent` % set) traced: the path, the trace ms.
fn noise_path(w: u32, h: u32, percent: u32) -> (GraphicsPath, f64) {
    noise_in(w, h, percent as u64, 100)
}

/// A `w × h` noise stencil with `n` in `of` pixels set, traced: the path, the trace ms.
fn noise_in(w: u32, h: u32, n: u64, of: u64) -> (GraphicsPath, f64) {
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
    let s = Instant::now();
    let path = path_from_stencil(&stencil, &Rectangle::new(0.0, 0.0, w as f64, h as f64));
    (path, ms(s))
}

/// Resets the process's peak resident size (`VmHWM`).
fn reset_peak() {
    let _ = std::fs::write("/proc/self/clear_refs", "5");
}

/// The process's peak resident size since `reset_peak`, in GB.
fn peak_gb() -> f64 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
    let kb = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<f64>().ok());
    kb.unwrap_or(0.0) / 1e6
}

/// The worker path from an empty selection to `path`, ants at 30 steps/s: a frame every 16 ms
/// (`mask_builds`, then the refresh) until the overlay shows; (slowest frame ms, ms from the
/// change to that frame, frames).
fn worker_change(
    app: &mut AppWorkspace,
    v: &mut View,
    t: &ViewTransform,
    path: GraphicsPath,
) -> (f64, f64, u32) {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let ws = app.active_ws_mut().unwrap();
    ws.selection.reset();
    v.frame_at(
        app.active_ws().unwrap(),
        t,
        false,
        mask_builds(wake.clone()),
    );
    let s = Instant::now();
    let ws = app.active_ws_mut().unwrap();
    ws.selection
        .set_continuation_path(path, CombineMode::Replace);
    ws.selection.commit_continuation();
    let (mut worst, mut frames) = (0.0f64, 0);
    loop {
        let f = Instant::now();
        let stamp = mask_builds(wake.clone());
        let ws = app.active_ws_mut().unwrap();
        let frame = SelectionRenderer::frame_at(s.elapsed().as_secs_f64(), 8);
        ws.surface_box.set_ants_frame(frame);
        let ws = app.active_ws().unwrap();
        v.frame_at(ws, t, false, stamp);
        worst = worst.max(ms(f));
        frames += 1;
        if ws.surface_box.overlay_bounds(ws, t).is_some() {
            return (worst, ms(s), frames);
        }
        std::thread::sleep(Duration::from_millis(16).saturating_sub(f.elapsed()));
    }
}

/// Task 25q: the masks on the workers. Select All at 4K (built in the frame), then the two noise
/// selections.
fn worker(w: u32, h: u32) {
    let rect = Rectangle::new(0.0, 0.0, w as f64, h as f64);
    let (mut app, t) = setup(w, h, GraphicsPath::default());
    let mut v = View::new(w, h);
    for _ in 0..3 {
        let (worst, to_final, frames) =
            worker_change(&mut app, &mut v, &t, GraphicsPath::from_rect(&rect));
        println!(
            "worker: Select All {w}x{h}: slowest frame {worst:.1} ms, ants shown after {to_final:.1} ms, \
             in frame {frames}"
        );
    }
    for percent in [30, 50] {
        let (path, _) = noise_path(w, h, percent);
        reset_peak();
        let (worst, to_final, frames) = worker_change(&mut app, &mut v, &t, path);
        println!(
            "worker: noise {percent}% {w}x{h}: slowest frame {worst:.1} ms, final ants after \
             {to_final:.0} ms ({frames} frames); peak memory {:.1} GB",
            peak_gb()
        );
    }
}

/// Waits 16 ms from `f` (one 60 Hz frame).
fn next_frame(f: Instant) {
    std::thread::sleep(Duration::from_millis(16).saturating_sub(f.elapsed()));
}

/// `n` frames of `path`, from its landed masks, moving 1 px a frame: right by the Move tools'
/// `set_interim_transform`, or left by a scroll. The slowest `set_interim_transform` and frame,
/// the frames whose ants moved along (the overlay's left edge within 1 px of the moved one), the
/// peak memory.
fn moving(label: &str, (w, h): (u32, u32), path: GraphicsPath, n: i32, scroll: bool) {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let (mut app, t0) = setup(w, h, GraphicsPath::default());
    let mut v = View::new(w, h);
    worker_change(&mut app, &mut v, &t0, path);
    let ws = app.active_ws().unwrap();
    let landed = ws.surface_box.overlay_bounds(ws, &t0).expect("landed");
    reset_peak();
    let (mut set, mut set_all, mut worst, mut along) = (0.0f64, 0.0, 0.0f64, 0);
    let s = Instant::now();
    for i in 1..=n {
        let f = Instant::now();
        let mut t = t0;
        if scroll {
            t.origin[0] -= i as f64;
        } else {
            let mut m = Matrix::identity();
            m.translate(i as f64, 0.0);
            let c = Instant::now();
            app.active_ws_mut()
                .unwrap()
                .selection
                .set_interim_transform(m);
            (set, set_all) = (set.max(ms(c)), set_all + ms(c));
        }
        let stamp = mask_builds(wake.clone());
        let frame = SelectionRenderer::frame_at(s.elapsed().as_secs_f64(), 8);
        app.active_ws_mut()
            .unwrap()
            .surface_box
            .set_ants_frame(frame);
        let ws = app.active_ws().unwrap();
        v.frame_at(ws, &t, false, stamp);
        worst = worst.max(ms(f));
        let x = landed.x + if scroll { -i } else { i };
        let b = ws.surface_box.overlay_bounds(ws, &t);
        along += b.is_some_and(|b| (b.x - x).abs() <= 1) as u32;
        next_frame(f);
    }
    println!(
        "{label}: set_interim_transform slowest {set:.1} ms (all {set_all:.0} ms); slowest frame \
         {worst:.1} ms; ants moved along in {along}/{n} frames; {n} frames in {:.0} ms; peak \
         memory {:.2} GB",
        ms(s),
        peak_gb()
    );
}

/// `n` steps of a Replace rectangle drag over `path` (committed, its masks landed): the mean and
/// slowest step (`set_continuation_path`, then the refresh).
fn replace_drag(label: &str, (w, h): (u32, u32), path: GraphicsPath, n: u32) {
    let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
    let (mut app, t) = setup(w, h, GraphicsPath::default());
    let mut v = View::new(w, h);
    worker_change(&mut app, &mut v, &t, path);
    let (mut worst, s) = (0.0f64, Instant::now());
    for i in 1..=n {
        let f = Instant::now();
        let r = Rectangle::new(100.0, 100.0, 10.0 + 20.0 * i as f64, 10.0 + 10.0 * i as f64);
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation_path(GraphicsPath::from_rect(&r), CombineMode::Replace);
        let stamp = mask_builds(wake.clone());
        v.frame_at(app.active_ws().unwrap(), &t, false, stamp);
        worst = worst.max(ms(f));
        next_frame(f);
    }
    let mean = (ms(s) - 16.0 * n as f64).max(0.0) / n as f64;
    println!(
        "{label}: Replace drag step ~{mean:.1} ms mean (16-ms frames off), slowest {worst:.1} ms"
    );
}

/// A `pw × ph` 30 % noise patch traced and placed at (x, y).
fn patch(x: f64, y: f64, pw: u32, ph: u32) -> GraphicsPath {
    let (p, _) = noise_path(pw, ph, 30);
    let mut m = Matrix::identity();
    m.translate(x, y);
    p.transformed(&m)
}

fn main() {
    let only = std::env::args().nth(1).unwrap_or_default();
    if only.is_empty() || only == "sync" {
        for _ in 0..3 {
            select_all(1920, 1080, false);
            select_all(3840, 2160, true);
        }
        for percent in [30, 50] {
            reset_peak();
            noise(3840, 2160, percent);
            println!("  peak memory {:.1} GB", peak_gb());
        }
    }
    if only.is_empty() || only == "worker" {
        worker(3840, 2160);
    }
    if only.is_empty() || only == "moving" {
        let k = |p: &GraphicsPath| p.polygons.iter().map(Vec::len).sum::<usize>() / 1000;
        let view = (3840, 2160);
        let wand = patch(1600.0, 900.0, 320, 200);
        let label = format!("{}k-vertex patch", k(&wand));
        moving(&format!("move, {label}"), view, wand.clone(), 120, false);
        moving(&format!("scroll, {label}"), view, wand, 120, true);
        let (sparse, _) = noise_in(3840, 2160, 1, 2000);
        let label = format!("{}k-vertex sparse noise over 4K", k(&sparse));
        moving(&format!("move, {label}"), view, sparse, 120, false);
        let (noise, _) = noise_path(3840, 2160, 30);
        replace_drag("4K noise 30 %", view, noise.clone(), 56);
        moving("move, 4K noise 30 %", view, noise, 120, false);
    }
}
