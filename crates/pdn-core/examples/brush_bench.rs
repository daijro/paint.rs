//! Timing of a long Paintbrush stroke on a 4K document (Task 30), release build:
//! `cargo run --release -p pdn-core --example brush_bench`. Not run by the gate.
//! A ~16 000 px zig-zag stroke at 1 point per 3 px, delivered as the UI does (one
//! `on_mouse_move` per 16 ms frame carrying that frame's coalesced samples, here 6 per frame);
//! each frame runs the engine (smoothing, sampling, dabs), Kernel B over the dirty rect and the
//! document composite. Reported per size/hardness: the sprite build (first stroke), the press,
//! the frames (median, 99th percentile, worst), the release (commit) and the whole stroke, and
//! the engine's share (`BrushStroke::add` and the dirty rect's mask, frame by frame).
use std::time::Instant;

use pdn_core::app::AppWorkspace;
use pdn_core::tool::brush::{BrushSettings, BrushStroke, Sprite, StrokeInput};
use pdn_core::tool::types::ToolType;
use pdn_core::tool::{MouseButton, PointerInput, PointerSample, PointerType};

fn input(points: &[(f64, f64)]) -> PointerInput {
    PointerInput {
        pointer_type: PointerType::Mouse,
        samples: points
            .iter()
            .map(|&(x, y)| PointerSample {
                x,
                y,
                pressure: None,
                time_ms: 0.0,
            })
            .collect(),
    }
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn first_press() {
    let mut app = AppWorkspace {
        view_width: 3840.0,
        view_height: 2160.0,
        ..Default::default()
    };
    app.create_blank_document_in_new_workspace(3840, 2160, 96.0, true);
    let ws = app.active_ws_mut().unwrap();
    (ws.view.ppp, ws.view.zoom, ws.view.zoom_to_window) = (1.0, 1.0, false);
    ws.view.center_view();
    let size = if std::env::args().any(|a| a == "--thin") {
        2
    } else {
        2000
    };
    for (k, v) in [("width", size.into()), ("hardness", 0.into())] {
        app.tool_settings.set(ToolType::PaintBrush, k, v);
    }
    app.set_active_tool_from_type(ToolType::PaintBrush);
    let warm = std::env::args().any(|a| a == "--warm");
    let wait = Instant::now();
    if warm {
        #[cfg(target_arch = "wasm32")]
        panic!("native prewarm probe only");
        while !Sprite::prewarm(size as f64, 0.0) {
            assert!(wait.elapsed().as_secs() < 60, "warmup did not complete");
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
    let ready = Sprite::prewarm(size as f64, 0.0);
    println!(
        "requested_warm={warm} ready_at_press={ready} prewarm_wait_ms={:.3}",
        ms(wait)
    );
    let t = Instant::now();
    app.on_mouse_down(100.0, 100.0, MouseButton::Left, &input(&[(100.0, 100.0)]));
    let press = ms(t);
    if size == 2 {
        for end in [
            (3700.0, 2000.0),
            (200.0, 1900.0),
            (3600.0, 200.0),
            (300.0, 1100.0),
        ] {
            app.on_mouse_move(end.0, end.1, &input(&[end]));
        }
    }
    let t = Instant::now();
    app.on_mouse_up(100.0, 100.0, MouseButton::Left, &input(&[]));
    let release = ms(t);
    let ws = app.active_ws().unwrap();
    let pdn_core::history::MementoKind::Stroke { pixels, .. } =
        &ws.history.undo.last().unwrap().kind
    else {
        panic!("stroke");
    };
    let payload: usize = pixels
        .chunks
        .iter()
        .map(|(_, image)| image.data.len())
        .sum();
    println!(
        "first_press size={size} press_ms={press:.3} release_ms={release:.3} chunks={} payload_bytes={payload} full_layer_bytes={}",
        pixels.chunks.len(),
        3840 * 2160 * 4
    );
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status
            .lines()
            .filter(|l| l.starts_with("VmRSS:") || l.starts_with("VmHWM:"))
        {
            println!("{line}");
        }
    }
}

fn main() {
    if std::env::args().any(|a| a == "--first-press") {
        first_press();
        return;
    }
    let (w, h) = (3840u32, 2160u32);
    // A zig-zag of four long diagonals, a point every 3 px.
    let corners = [
        (100.0, 100.0),
        (3700.0, 2000.0),
        (200.0, 1900.0),
        (3600.0, 200.0),
        (300.0, 1100.0),
    ];
    let mut path = vec![corners[0]];
    for pair in corners.windows(2) {
        let ((x0, y0), (x1, y1)): ((f64, f64), (f64, f64)) = (pair[0], pair[1]);
        let n = ((x1 - x0).hypot(y1 - y0) / 3.0).ceil() as usize;
        path.extend((1..=n).map(|i| {
            let t = i as f64 / n as f64;
            ((x0 + (x1 - x0) * t).round(), (y0 + (y1 - y0) * t).round())
        }));
    }
    let length: f64 = path
        .windows(2)
        .map(|p| (p[1].0 - p[0].0).hypot(p[1].1 - p[0].1))
        .sum();
    println!(
        "{w}×{h}, stroke {length:.0} px, {} points, 6 per frame",
        path.len()
    );
    for (size, hardness, antialias) in [
        (2, 75, true),
        (20, 75, true),
        (100, 75, true),
        (300, 0, true),
        (20, 75, false),
    ] {
        let t = Instant::now();
        let _ = Sprite::shared(size as f64, hardness as f64 / 100.0);
        let sprite = ms(t);
        let mut app = AppWorkspace {
            view_width: w as f64,
            view_height: h as f64,
            ..Default::default()
        };
        app.create_blank_document_in_new_workspace(w, h, 96.0, true);
        let ws = app.active_ws_mut().unwrap();
        (ws.view.ppp, ws.view.zoom, ws.view.zoom_to_window) = (1.0, 1.0, false);
        ws.view.center_view();
        for (k, v) in [
            ("width", size.into()),
            ("hardness", hardness.into()),
            ("antialias", antialias.into()),
        ] {
            app.tool_settings.set(ToolType::PaintBrush, k, v);
        }
        app.primary_color = pdn_core::util::color::Color::new(30, 60, 200, 180);
        app.set_active_tool_from_type(ToolType::PaintBrush);
        // The document starts at the viewport origin (canvas points = document px).
        let all = Instant::now();
        let t = Instant::now();
        let (x, y) = path[0];
        app.on_mouse_down(x, y, MouseButton::Left, &input(&path[..1]));
        let press = ms(t);
        let mut frames = vec![];
        for chunk in path[1..].chunks(6) {
            let (x, y) = *chunk.last().unwrap();
            let t = Instant::now();
            app.on_mouse_move(x, y, &input(chunk));
            app.on_frame(0.0);
            frames.push(ms(t));
        }
        let (x, y) = *path.last().unwrap();
        let t = Instant::now();
        app.on_mouse_up(x, y, MouseButton::Left, &input(&[(x, y)]));
        let release = ms(t);
        let total = ms(all);
        // The engine alone over the same frames: add, then the dirty rect's mask.
        let settings = BrushSettings {
            size: size as f64,
            hardness: hardness as f64 / 100.0,
            spacing: 0.15,
            antialias,
            smoothing: true,
        };
        let t = Instant::now();
        let mut stroke = BrushStroke::new(w, h, settings);
        let mut points = StrokeInput::new(1.0, true);
        for chunk in std::iter::once(&path[..1]).chain(path[1..].chunks(6)) {
            chunk.iter().for_each(|&(x, y)| points.push(x, y, None));
            stroke.add(&points);
            std::hint::black_box(stroke.mask(stroke.dirty()));
        }
        let engine = ms(t);
        frames.sort_by(f64::total_cmp);
        let at = |q: f64| frames[((frames.len() - 1) as f64 * q) as usize];
        println!(
            "size {size:>4} h {hardness:>3}% {}: sprite {sprite:7.1} ms | press {press:6.2} ms | \
             {} frames median {:5.2} p99 {:6.2} max {:6.2} ms | release {release:6.2} ms | \
             stroke {total:7.0} ms (engine {engine:6.0} ms)",
            if antialias { "AA" } else { "aliased" },
            frames.len(),
            at(0.5),
            at(0.99),
            frames[frames.len() - 1],
        );
    }
    for (size, hardness) in [(500.0, 0.0), (2000.0, 0.0), (2000.0, 0.75)] {
        let t = Instant::now();
        let s = Sprite::new(size, hardness);
        println!(
            "sprite size {size} hardness {hardness}: edge {}, {:.0} ms",
            s.edge,
            ms(t)
        );
    }
}
