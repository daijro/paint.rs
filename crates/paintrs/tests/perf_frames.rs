//! Per-frame CPU cost of the desktop app, measured the way eframe drives it (no AccessKit: the
//! AT-SPI bus is off on a normal desktop): `raw_input_hook`, `logic` + `ui` in `run_ui`, then
//! `tessellate`, plus the texture bytes the frame hands the GPU and the repaint the frame asks for.
//! `cargo test --release -p paintrs --test perf_frames -- --ignored --nocapture --test-threads=1`
//! (`test = false`: plain test runs don't build it). Timings keep the native thumbnail worker; the
//! fingerprint scales thumbnails in the frame (tool cursors are OS cursors, outside it).
use std::time::{Duration, Instant};

use eframe::App as _;
use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2, ViewportId, pos2, vec2};
use paintrs::PaintApp;
use pdn_core::tool::ToolType;

/// Logical window size (points) at every scale.
const WINDOW: Vec2 = vec2(1600.0, 1000.0);

struct Rig {
    ctx: egui::Context,
    app: PaintApp,
    frame: eframe::Frame,
    ppp: f32,
    start: Instant,
    /// Real time between frames (the app's clock is real, as in `main`).
    dt: Duration,
    /// Frames run.
    frames: u64,
    /// Thumbnails scale in the frame and the frames are hashed (implies `virtual_time`).
    deterministic: bool,
    /// Input and app time advance 16 ms a frame instead of with the wall clock (a profiler's
    /// slow frames would otherwise end every gesture and let every thumbnail fall due).
    virtual_time: bool,
    /// Running hash of every frame's texture uploads and tessellated meshes.
    hash: u64,
    /// Texture bytes and uploads of `profile_loop`'s measured frames.
    tex_bytes: usize,
    tex_ops: usize,
    /// Every texture's pixels as the uploads left them, and the last frame's meshes: what the
    /// screen shows ([`Rig::screen`]; deterministic rigs).
    mirror: Option<(
        std::collections::HashMap<egui::TextureId, egui::ColorImage>,
        Vec<egui::ClippedPrimitive>,
    )>,
}

#[derive(Default, Clone, Copy)]
struct Sample {
    ui: f64,
    tess: f64,
    tex_bytes: usize,
    tex_ops: usize,
    vertices: usize,
    immediate_repaint: bool,
    /// The root viewport's `repaint_delay` (ms; capped at 1e6).
    repaint_ms: f64,
}

impl Rig {
    fn new(ppp: f32, doc: Option<(u32, u32)>) -> Self {
        Self::with(ppp, doc, false)
    }

    fn with(ppp: f32, doc: Option<(u32, u32)>, deterministic: bool) -> Self {
        Self::with_time(ppp, doc, deterministic, deterministic)
    }

    fn with_time(
        ppp: f32,
        doc: Option<(u32, u32)>,
        deterministic: bool,
        virtual_time: bool,
    ) -> Self {
        let ctx = egui::Context::default();
        let cc = eframe::CreationContext::_new_kittest(ctx.clone());
        let mut app = PaintApp::new_with_policy(&cc, pdn_core::fonts::FontPolicy::Bundled);
        if deterministic {
            // Thumbnails on the frame that needs them (timings keep the native worker).
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
        }
        if virtual_time {
            app.clock = paintrs::Clock::Virtual(0.0);
        }
        let mut rig = Rig {
            ctx,
            app,
            frame: eframe::Frame::_new_kittest(),
            ppp,
            start: Instant::now(),
            dt: Duration::from_millis(0),
            frames: 0,
            deterministic,
            virtual_time,
            hash: 0,
            tex_bytes: 0,
            tex_ops: 0,
            mirror: deterministic.then(Default::default),
        };
        for _ in 0..5 {
            rig.frame(vec![]);
        }
        if let Some((w, h)) = doc {
            rig.app
                .app
                .create_blank_document_in_new_workspace(w, h, 96.0, false);
            for _ in 0..5 {
                rig.frame(vec![]);
            }
        }
        rig
    }

    fn raw(&self, events: Vec<Event>) -> RawInput {
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, WINDOW)),
            time: Some(if self.virtual_time {
                self.frames as f64 * 0.016
            } else {
                self.start.elapsed().as_secs_f64()
            }),
            predicted_dt: 1.0 / 60.0,
            max_texture_side: std::env::var("PERF_MAX_TEXTURE")
                .ok()
                .map(|v| v.parse().unwrap()),
            focused: true,
            events,
            ..Default::default()
        };
        let vp = raw.viewports.entry(ViewportId::ROOT).or_default();
        vp.native_pixels_per_point = Some(self.ppp);
        vp.focused = Some(true);
        vp.inner_rect = raw.screen_rect;
        raw
    }

    fn frame(&mut self, events: Vec<Event>) -> Sample {
        if !self.dt.is_zero() {
            std::thread::sleep(self.dt);
        }
        let mut raw = self.raw(events);
        self.frames += 1;
        if self.virtual_time {
            self.app.clock = paintrs::Clock::Virtual(self.frames as f64 * 16.0);
        }
        let t0 = Instant::now();
        self.app.raw_input_hook(&self.ctx, &mut raw);
        let (app, frame) = (&mut self.app, &mut self.frame);
        let mut out = self.ctx.run_ui(raw, |ui| {
            app.logic(ui.ctx(), frame);
            app.ui(ui, frame);
        });
        let t1 = Instant::now();
        let prims = self.ctx.tessellate(out.shapes, out.pixels_per_point);
        let t2 = Instant::now();
        let mut s = Sample {
            ui: (t1 - t0).as_secs_f64() * 1e3,
            tess: (t2 - t1).as_secs_f64() * 1e3,
            ..Default::default()
        };
        for deltas in out.textures_delta.set.values() {
            for delta in deltas {
                s.tex_ops += 1;
                s.tex_bytes += delta.image.width() * delta.image.height() * 4;
            }
        }
        for p in &prims {
            if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                s.vertices += m.vertices.len();
            }
        }
        if self.deterministic {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            self.hash.hash(&mut h);
            // egui hands the deltas over in hash-map order.
            let mut set: Vec<_> = out.textures_delta.set.iter().collect();
            set.sort_by_key(|(id, _)| match id {
                egui::TextureId::Managed(n) => *n,
                egui::TextureId::User(n) => n | 1 << 63,
            });
            for (id, deltas) in set {
                for d in deltas {
                    (id, d.pos, d.image.size()).hash(&mut h);
                    let egui::ImageData::Color(img) = &d.image;
                    for c in &img.pixels {
                        c.to_array().hash(&mut h);
                    }
                }
            }
            for p in &prims {
                if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                    m.texture_id.hash(&mut h);
                    m.indices.hash(&mut h);
                    for v in &m.vertices {
                        (
                            v.pos.x.to_bits(),
                            v.pos.y.to_bits(),
                            v.uv.x.to_bits(),
                            v.uv.y.to_bits(),
                        )
                            .hash(&mut h);
                        v.color.to_array().hash(&mut h);
                    }
                }
            }
            self.hash = h.finish();
        }
        let delay = out
            .viewport_output
            .get(&ViewportId::ROOT)
            .map(|v| v.repaint_delay);
        s.immediate_repaint = delay.is_some_and(|d| d.is_zero());
        s.repaint_ms = delay.map_or(1e6, |d| (d.as_secs_f64() * 1e3).min(1e6));
        if let Some((textures, last)) = &mut self.mirror {
            for (id, deltas) in &out.textures_delta.set {
                for d in deltas {
                    let egui::ImageData::Color(img) = &d.image;
                    let Some([x, y]) = d.pos else {
                        textures.insert(*id, (**img).clone());
                        continue;
                    };
                    let t = textures
                        .get_mut(id)
                        .expect("a partial upload of a new texture");
                    let (w, tw) = (img.width(), t.width());
                    for (row, src) in img.pixels.chunks_exact(w).enumerate() {
                        t.pixels[(y + row) * tw + x..][..w].copy_from_slice(src);
                    }
                }
            }
            for id in &out.textures_delta.free {
                textures.remove(id);
            }
            *last = prims;
        }
        // No renderer applies the uploads (debug builds assert on dropping them).
        out.textures_delta.clear();
        s
    }

    /// A hash of what the screen shows after the last frame: its meshes in order with the pixels
    /// of the textures they sample (not the textures' ids, so an extra texture made meanwhile
    /// changes nothing).
    fn screen(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let (textures, prims) = self.mirror.as_ref().expect("Rig::mirror");
        let mut h = std::collections::hash_map::DefaultHasher::new();
        for p in prims {
            let r = p.clip_rect;
            [r.min.x, r.min.y, r.max.x, r.max.y]
                .map(f32::to_bits)
                .hash(&mut h);
            let egui::epaint::Primitive::Mesh(m) = &p.primitive else {
                continue;
            };
            match textures.get(&m.texture_id) {
                Some(img) => {
                    img.size.hash(&mut h);
                    for c in &img.pixels {
                        c.to_array().hash(&mut h);
                    }
                }
                None => "no texture".hash(&mut h),
            }
            m.indices.hash(&mut h);
            for v in &m.vertices {
                [v.pos.x, v.pos.y, v.uv.x, v.uv.y]
                    .map(f32::to_bits)
                    .hash(&mut h);
                v.color.to_array().hash(&mut h);
            }
        }
        h.finish()
    }

    /// The middle of the window (the canvas, centred in the workspace).
    fn centre(&self) -> Pos2 {
        pos2(WINDOW.x * 0.45, WINDOW.y * 0.5)
    }
}

struct Stats {
    name: String,
    n: usize,
    ui_mean: f64,
    ui_p95: f64,
    ui_max: f64,
    tess_mean: f64,
    tex_kb: f64,
    tex_ops: f64,
    vertices: usize,
    immediate: usize,
}

fn stats(name: String, s: &[Sample]) -> Stats {
    let mut ui: Vec<f64> = s.iter().map(|x| x.ui).collect();
    ui.sort_by(f64::total_cmp);
    let n = s.len();
    Stats {
        name,
        n,
        ui_mean: ui.iter().sum::<f64>() / n as f64,
        ui_p95: ui[(n * 95 / 100).min(n - 1)],
        ui_max: ui[n - 1],
        tess_mean: s.iter().map(|x| x.tess).sum::<f64>() / n as f64,
        tex_kb: s.iter().map(|x| x.tex_bytes).sum::<usize>() as f64 / n as f64 / 1024.0,
        tex_ops: s.iter().map(|x| x.tex_ops).sum::<usize>() as f64 / n as f64,
        vertices: s.iter().map(|x| x.vertices).max().unwrap_or(0),
        immediate: s.iter().filter(|x| x.immediate_repaint).count(),
    }
}

fn print(rows: &[Stats]) {
    println!(
        "| scenario | frames | run_ui mean ms | p95 | max | tessellate ms | tex KiB/frame | tex ops/frame | max vertices | frames asking repaint now |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for r in rows {
        println!(
            "| {} | {} | {:.2} | {:.2} | {:.2} | {:.2} | {:.0} | {:.1} | {} | {} |",
            r.name,
            r.n,
            r.ui_mean,
            r.ui_p95,
            r.ui_max,
            r.tess_mean,
            r.tex_kb,
            r.tex_ops,
            r.vertices,
            r.immediate
        );
    }
}

fn press(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn wheel(delta: Vec2, modifiers: Modifiers) -> Event {
    Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        source: egui::MouseWheelSource::Wheel,
        delta,
        phase: egui::TouchPhase::Move,
        modifiers,
    }
}

const N: usize = 120;

fn scenarios(ppp: f32, doc: (u32, u32), rows: &mut Vec<Stats>) {
    let tag = format!("{}x{} @{ppp}", doc.0, doc.1);
    let fresh = || Rig::new(ppp, (doc != (800, 600)).then_some(doc));

    // Idle: no input, real 16 ms between frames.
    let mut r = fresh();
    r.dt = Duration::from_millis(16);
    let s: Vec<_> = (0..N).map(|_| r.frame(vec![])).collect();
    rows.push(stats(format!("idle {tag}"), &s));

    // Pointer hovering over the canvas, one move per frame.
    let mut r = fresh();
    let c = r.centre();
    let s: Vec<_> = (0..N)
        .map(|i| {
            let a = i as f32 * 0.1;
            r.frame(vec![Event::PointerMoved(
                c + vec2(a.cos(), a.sin()) * 150.0,
            )])
        })
        .collect();
    rows.push(stats(format!("hover canvas {tag}"), &s));

    // Paintbrush stroke, one pointer move per frame.
    let mut r = fresh();
    r.app.app.set_active_tool_from_type(ToolType::PaintBrush);
    r.frame(vec![]);
    let c = r.centre();
    r.frame(vec![Event::PointerMoved(c)]);
    r.frame(vec![press(c, true)]);
    let s: Vec<_> = (0..N)
        .map(|i| {
            let a = i as f32 * 0.1;
            r.frame(vec![Event::PointerMoved(
                c + vec2(a.cos(), a.sin()) * (60.0 + i as f32),
            )])
        })
        .collect();
    r.frame(vec![press(c, false)]);
    rows.push(stats(format!("brush stroke {tag}"), &s));

    // Panning with the wheel, one notch-ish per frame, back and forth.
    let mut r = fresh();
    let c = r.centre();
    r.frame(vec![Event::PointerMoved(c)]);
    let s: Vec<_> = (0..N)
        .map(|i| {
            let d = if (i / 20).is_multiple_of(2) {
                -20.0
            } else {
                20.0
            };
            r.frame(vec![wheel(vec2(0.0, d), Modifiers::NONE)])
        })
        .collect();
    rows.push(stats(format!("wheel pan {tag}"), &s));

    // Ctrl+wheel zoom, in then out.
    let mut r = fresh();
    let c = r.centre();
    r.frame(vec![Event::PointerMoved(c)]);
    let s: Vec<_> = (0..N)
        .map(|i| {
            let d = if (i / 10).is_multiple_of(2) {
                40.0
            } else {
                -40.0
            };
            r.frame(vec![wheel(vec2(0.0, d), Modifiers::CTRL)])
        })
        .collect();
    rows.push(stats(format!("ctrl+wheel zoom {tag}"), &s));

    // Effects menu: open, then hover down its items.
    let mut r = fresh();
    let effects = pos2(334.0, 46.0);
    r.frame(vec![Event::PointerMoved(effects)]);
    let open = r.frame(vec![press(effects, true), press(effects, false)]);
    let mut s = vec![open];
    s.extend((0..N).map(|i| {
        r.frame(vec![Event::PointerMoved(pos2(
            362.0,
            70.0 + (i % 40) as f32 * 12.0,
        ))])
    }));
    rows.push(stats(format!("effects menu {tag}"), &s));
}

#[test]
#[ignore = "timing; run with --ignored --nocapture"]
fn frame_costs() {
    let mut rows = vec![];
    for ppp in [1.0, 1.25] {
        for doc in [(800, 600), (3000, 2000)] {
            scenarios(ppp, doc, &mut rows);
        }
    }
    print(&rows);
}

/// A press at `from`, then one pointer move per frame along `step`, then the release.
fn drag(r: &mut Rig, from: Pos2, step: Vec2) -> Vec<Sample> {
    r.frame(vec![Event::PointerMoved(from)]);
    r.frame(vec![press(from, true)]);
    let s = (1..=N)
        .map(|i| {
            // Out and back, so the drag stays on screen.
            let k = if i <= N / 2 { i } else { N - i } as f32;
            r.frame(vec![Event::PointerMoved(from + step * k)])
        })
        .collect();
    r.frame(vec![press(from, false)]);
    s
}

fn drags(ppp: f32, doc: (u32, u32), rows: &mut Vec<Stats>) {
    let tag = format!("{}x{} @{ppp}", doc.0, doc.1);
    let fresh = || Rig::new(ppp, (doc != (800, 600)).then_some(doc));

    // The History form by its title bar.
    let mut r = fresh();
    let title = r
        .app
        .shell
        .forms
        .window(paintrs::forms::FormId::History)
        .rect()
        .left_top()
        + vec2(40.0, 12.0);
    let s = drag(&mut r, title, vec2(-3.0, 2.0));
    rows.push(stats(format!("form drag {tag}"), &s));

    // Rectangle Select, growing from the canvas centre.
    let mut r = fresh();
    r.app
        .app
        .set_active_tool_from_type(ToolType::RectangleSelect);
    let c = r.centre() - vec2(200.0, 150.0);
    let s = drag(&mut r, c, vec2(4.0, 3.0));
    rows.push(stats(format!("rect select drag {tag}"), &s));

    // Move Selected Pixels with everything selected.
    let mut r = fresh();
    pdn_core::action::perform(&mut r.app.app, "menu.edit.selectAll");
    r.app.app.set_active_tool_from_type(ToolType::Move);
    r.frame(vec![]);
    let c = r.centre();
    let s = drag(&mut r, c, vec2(3.0, 2.0));
    rows.push(stats(format!("move pixels drag {tag}"), &s));

    // A Paintbrush stroke.
    let mut r = fresh();
    r.app.app.set_active_tool_from_type(ToolType::PaintBrush);
    let c = r.centre() - vec2(200.0, 0.0);
    let s = drag(&mut r, c, vec2(6.0, 1.0));
    rows.push(stats(format!("brush drag {tag}"), &s));
}

#[test]
#[ignore = "timing; run with --ignored --nocapture"]
fn drag_costs() {
    let mut rows = vec![];
    for (ppp, doc) in [(1.0, (800, 600)), (1.25, (3000, 2000))] {
        drags(ppp, doc, &mut rows);
    }
    print(&rows);
}

/// Whether an idle app stops asking for frames.
#[test]
#[ignore = "timing; run with --ignored --nocapture"]
fn idle_settles() {
    for ppp in [1.0, 1.25] {
        let mut r = Rig::new(ppp, None);
        r.dt = Duration::from_millis(16);
        let s: Vec<_> = (0..120).map(|_| r.frame(vec![])).collect();
        let immediate = s.iter().filter(|x| x.immediate_repaint).count();
        let soon = s.iter().filter(|x| x.repaint_ms < 100.0).count();
        let delays: Vec<String> = s
            .iter()
            .rev()
            .take(5)
            .map(|x| format!("{:.0}", x.repaint_ms))
            .collect();
        println!(
            "@{ppp}: {immediate}/120 idle frames asked for an immediate repaint, {soon}/120 for one within 100 ms; last delays (ms) {delays:?}"
        );
    }
}

/// One scenario's steady frames for a profiler: `PERF_SCENARIO` = idle | hover (the pointer over
/// the canvas) | tools (over the Tools window's buttons) | brush | zoom | pan | menu |
/// rect (a Rectangle Select drag) | bigrect (one out to two thirds of the window) | move (a Move
/// Selected Pixels drag) | movepan (one that wheels the view each frame, as auto-scroll does),
/// `PERF_DOC` = WxH, `PERF_PPP`, `PERF_IMAGE` =
/// translucent (the background layer see-through), `PERF_LAYERS` = 2 (a second, active layer over
/// it), `PERF_MAX_TEXTURE` = the renderer's largest texture side (2048 here; WebGL's and WebGPU's
/// 8192 keep a large view in one tile), then `PERF_FRAMES` frames (or `PERF_SECS` seconds), 16 ms
/// of app time apart. Instruction
/// counts: `valgrind --tool=callgrind --toggle-collect=perf_frames::steady_frames <test binary>
/// profile_loop --ignored`.
#[test]
#[ignore = "profiling loop; run with --ignored"]
fn profile_loop() {
    let env = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.into());
    let scenario = env("PERF_SCENARIO", "idle");
    let (w, h) = env("PERF_DOC", "3000x2000")
        .split_once('x')
        .map(|(w, h)| (w.parse().unwrap(), h.parse().unwrap()))
        .unwrap();
    let ppp: f32 = env("PERF_PPP", "1").parse().unwrap();
    let secs: f64 = env("PERF_SECS", "20").parse().unwrap();
    let frames: usize = env("PERF_FRAMES", "0").parse().unwrap();
    let mut r = Rig::with_time(ppp, ((w, h) != (800, 600)).then_some((w, h)), false, true);
    let c = r.centre();
    if let Some(ws) = r.app.app.active_ws_mut() {
        let (w, h) = (ws.document.width, ws.document.height);
        if env("PERF_IMAGE", "") == "translucent" {
            let layer = ws.document.layers.get_at_mut(0).unwrap();
            for (i, px) in layer
                .surface
                .data_mut()
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .enumerate()
            {
                let (x, y) = (i as u32 % w, i as u32 / w);
                px.copy_from_slice(&[
                    (x / 7) as u8,
                    (y / 5) as u8,
                    160,
                    (64 + (x + y) % 128) as u8,
                ]);
            }
        }
        if env("PERF_LAYERS", "1") == "2" {
            let mut top = pdn_core::document::layer::Layer::create_layer(w, h, "Top");
            top.surface.fill_rect(
                pdn_core::pixel::IntRect::new(0, 0, w, h / 2),
                [40, 90, 200, 255],
            );
            ws.add_layer(top);
            ws.active_layer = 1;
        }
        ws.invalidate(None);
    }
    match scenario.as_str() {
        "brush" => {
            r.app.app.set_active_tool_from_type(ToolType::PaintBrush);
            r.frame(vec![Event::PointerMoved(c)]);
            r.frame(vec![press(c, true)]);
        }
        "menu" => {
            let effects = pos2(334.0, 46.0);
            r.frame(vec![Event::PointerMoved(effects)]);
            r.frame(vec![press(effects, true), press(effects, false)]);
        }
        "rect" | "rectpan" | "bigrect" | "move" | "movepan" | "mselpan" | "gridpan" => {
            let c = match scenario.as_str() {
                "bigrect" => pos2(WINDOW.x * 0.2, WINDOW.y * 0.2),
                _ => c,
            };
            if scenario == "gridpan" {
                // 36 × 24 rectangles of 20 × 12 px, 80 px apart (a Magic Wand global pick on a
                // pattern): many separate parts.
                let mut path = pdn_core::selection::path::GraphicsPath::default();
                for (i, j) in (0..36).flat_map(|i| (0..24).map(move |j| (i, j))) {
                    let (x, y) = (40.0 + 80.0 * i as f64, 40.0 + 80.0 * j as f64);
                    let p = |x, y| pdn_core::util::geom::Point::new(x, y);
                    path.polygons.push(vec![
                        p(x, y),
                        p(x + 20.0, y),
                        p(x + 20.0, y + 12.0),
                        p(x, y + 12.0),
                    ]);
                }
                let ws = r.app.app.active_ws_mut().unwrap();
                ws.selection
                    .set_continuation_path(path, pdn_core::selection::CombineMode::Replace);
                ws.selection.commit_continuation();
                r.app.app.set_active_tool_from_type(ToolType::MoveSelection);
            } else if !scenario.contains("rect") {
                pdn_core::action::perform(&mut r.app.app, "menu.edit.selectAll");
                r.app
                    .app
                    .set_active_tool_from_type(match scenario.as_str() {
                        "mselpan" => ToolType::MoveSelection,
                        _ => ToolType::Move,
                    });
            } else {
                r.app
                    .app
                    .set_active_tool_from_type(ToolType::RectangleSelect);
            }
            r.frame(vec![Event::PointerMoved(c)]);
            r.frame(vec![press(c, true)]);
        }
        _ => {}
    }
    // The forms' fade to translucent over the image (0.5 s) ends before the measured frames.
    r.frame(vec![Event::PointerMoved(c)]);
    for _ in 0..40 {
        r.frame(vec![]);
    }
    let start = Instant::now();
    let i = steady_frames(&mut r, &scenario, c, frames, secs);
    let ms = start.elapsed().as_secs_f64() * 1e3 / i.max(1) as f64;
    let kib = r.tex_bytes as f64 / 1024.0 / i.max(1) as f64;
    let ops = r.tex_ops as f64 / i.max(1) as f64;
    println!(
        "{scenario}: {i} frames, {ms:.2} ms a frame, {kib:.0} KiB of textures in {ops:.1} uploads a frame"
    );
}

/// The measured loop of [`profile_loop`] (a separate symbol for `callgrind --toggle-collect`).
#[inline(never)]
fn steady_frames(r: &mut Rig, scenario: &str, c: Pos2, frames: usize, secs: f64) -> usize {
    let start = Instant::now();
    let mut i = 0usize;
    while if frames > 0 {
        i < frames
    } else {
        start.elapsed().as_secs_f64() < secs
    } {
        let a = i as f32 * 0.1;
        let events = match scenario {
            "brush" => vec![Event::PointerMoved(
                c + vec2(a.cos(), a.sin()) * (60.0 + (i % 200) as f32),
            )],
            "zoom" => {
                let d = if (i / 10).is_multiple_of(2) {
                    40.0
                } else {
                    -40.0
                };
                vec![Event::PointerMoved(c), wheel(vec2(0.0, d), Modifiers::CTRL)]
            }
            "pan" => {
                let d = if (i / 20).is_multiple_of(2) {
                    -20.0
                } else {
                    20.0
                };
                vec![Event::PointerMoved(c), wheel(vec2(0.0, d), Modifiers::NONE)]
            }
            "menu" => vec![Event::PointerMoved(pos2(
                362.0,
                70.0 + (i % 40) as f32 * 12.0,
            ))],
            "hover" => vec![Event::PointerMoved(c + vec2(a.cos(), a.sin()) * 150.0)],
            // Up and down the Tools window's buttons.
            "tools" => {
                let k = if i % 80 < 40 { i % 40 } else { 40 - i % 40 } as f32;
                let x = if (i / 8) % 2 == 0 { 25.0 } else { 55.0 };
                vec![Event::PointerMoved(pos2(x, 130.0 + 9.0 * k))]
            }
            "rect" | "move" => vec![Event::PointerMoved(c + vec2(3.0, 2.0) * (i % 100) as f32)],
            // Out to two thirds of the window and back.
            "bigrect" => {
                let k = if i % 120 < 60 { i % 60 } else { 60 - i % 60 } as f32 / 100.0;
                vec![Event::PointerMoved(pos2(
                    WINDOW.x * (0.2 + k),
                    WINDOW.y * (0.2 + k),
                ))]
            }
            // A drag while the wheel scrolls the view (as auto-scroll does).
            "movepan" | "rectpan" | "mselpan" | "gridpan" => {
                let d = if (i / 20).is_multiple_of(2) {
                    -20.0
                } else {
                    20.0
                };
                let at = c + vec2(3.0, 2.0) * (i % 20) as f32;
                vec![
                    Event::PointerMoved(at),
                    wheel(vec2(0.0, d), Modifiers::NONE),
                ]
            }
            _ => vec![],
        };
        let s = r.frame(events);
        r.tex_bytes += s.tex_bytes;
        r.tex_ops += s.tex_ops;
        i += 1;
    }
    i
}

/// A hash of everything the frames hand the renderer (texture uploads, meshes) over fixed
/// scenarios on a virtual clock: equal before and after a change = the same pixels.
#[test]
#[ignore = "output fingerprint; run with --ignored --nocapture"]
fn output_fingerprint() {
    for (ppp, doc) in [(1.0, (800, 600)), (1.25, (3000, 2000)), (1.5, (1200, 900))] {
        let mut r = Rig::with(ppp, (doc != (800, 600)).then_some(doc), true);
        let c = r.centre();
        for i in 0..30 {
            let a = i as f32 * 0.2;
            r.frame(vec![Event::PointerMoved(
                c + vec2(a.cos(), a.sin()) * 300.0,
            )]);
        }
        // Over a form (opaque again) and back.
        r.frame(vec![Event::PointerMoved(pos2(
            WINDOW.x - 60.0,
            WINDOW.y - 100.0,
        ))]);
        for _ in 0..40 {
            r.frame(vec![]);
        }
        r.frame(vec![Event::PointerMoved(c)]);
        for _ in 0..40 {
            r.frame(vec![]);
        }
        r.app.app.set_active_tool_from_type(ToolType::PaintBrush);
        r.frame(vec![press(c, true)]);
        for i in 0..20 {
            r.frame(vec![Event::PointerMoved(
                c + vec2(i as f32 * 7.0, i as f32 * 3.0),
            )]);
        }
        r.frame(vec![press(c + vec2(140.0, 60.0), false)]);
        for i in 0..20 {
            let d = if i < 10 { 40.0 } else { -40.0 };
            r.frame(vec![wheel(vec2(0.0, d), Modifiers::CTRL)]);
        }
        let effects = pos2(334.0, 46.0);
        r.frame(vec![Event::PointerMoved(effects)]);
        r.frame(vec![press(effects, true), press(effects, false)]);
        for i in 0..30 {
            r.frame(vec![Event::PointerMoved(pos2(
                362.0,
                70.0 + i as f32 * 12.0,
            ))]);
        }
        println!("{doc:?} @{ppp}: {:016x}", r.hash);
    }
}

/// What the screen shows once each gesture has settled (a pan, a zoom, Move Selected Pixels
/// drags: translate and rotate), on a virtual clock: equal before and after a change that draws
/// only the frames inside a gesture differently. The image is a pattern, opaque in its top half
/// and translucent below (the moved pixels' interior and the checkerboard show).
#[test]
#[ignore = "output fingerprint; run with --ignored --nocapture"]
fn settled_fingerprint() {
    for (ppp, doc) in [(1.0, (800, 600)), (1.25, (3000, 2000)), (1.5, (1200, 900))] {
        let mut r = Rig::with(ppp, (doc != (800, 600)).then_some(doc), true);
        let ws = r.app.app.active_ws_mut().unwrap();
        let (w, h) = (ws.document.width, ws.document.height);
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        for (i, px) in layer
            .surface
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            let (x, y) = (i as u32 % w, i as u32 / w);
            let a = if y < h / 2 {
                255
            } else {
                (40 + (x * 3 + y) % 200) as u8
            };
            px.copy_from_slice(&[
                (x * 7 % 256) as u8,
                (y * 5 % 256) as u8,
                ((x ^ y) % 256) as u8,
                a,
            ]);
        }
        ws.invalidate(None);
        let mut marks = vec![];
        let settle = |r: &mut Rig, marks: &mut Vec<u64>| {
            for _ in 0..20 {
                r.frame(vec![]);
            }
            marks.push(r.screen());
        };
        let c = r.centre();
        r.frame(vec![Event::PointerMoved(c)]);
        settle(&mut r, &mut marks);
        for i in 0..20 {
            let d = if i < 13 { 40.0 } else { -40.0 };
            r.frame(vec![wheel(vec2(0.0, d), Modifiers::CTRL)]);
        }
        settle(&mut r, &mut marks);
        for i in 0..30 {
            let d = if i < 15 { -20.0 } else { 15.0 };
            let dx = if i % 3 == 0 { 7.0 } else { 0.0 };
            r.frame(vec![wheel(vec2(dx, d), Modifiers::NONE)]);
        }
        settle(&mut r, &mut marks);
        pdn_core::action::perform(&mut r.app.app, "menu.edit.selectAll");
        r.app.app.set_active_tool_from_type(ToolType::Move);
        r.frame(vec![]);
        r.frame(vec![press(c, true)]);
        for i in 1..=25 {
            r.frame(vec![Event::PointerMoved(c + vec2(3.0, 2.0) * i as f32)]);
        }
        r.frame(vec![press(c + vec2(75.0, 50.0), false)]);
        settle(&mut r, &mut marks);
        // A rotation (the right button) of the moved pixels.
        let at = c + vec2(75.0, 50.0);
        let right = |pressed| Event::PointerButton {
            pos: at,
            button: PointerButton::Secondary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        r.frame(vec![Event::PointerMoved(at)]);
        r.frame(vec![right(true)]);
        for i in 1..=15 {
            r.frame(vec![Event::PointerMoved(at + vec2(4.0, 6.0) * i as f32)]);
        }
        r.frame(vec![right(false)]);
        settle(&mut r, &mut marks);
        println!("{doc:?} @{ppp}: {marks:016x?}");
    }
}
