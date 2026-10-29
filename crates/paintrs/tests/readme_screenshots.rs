//! The README screenshots in `assets/screenshots/`, rendered headless through wgpu (lavapipe works):
//! `cargo test -p paintrs --test readme_screenshots -- --ignored`. Ignored, so the normal suite never
//! writes them. The picture is made in the app: render effects, a gradient and a text layer.
//! The pointer positions and crops are this window size's layout: look at the images after a
//! layout change.
use egui::{Pos2, Rect, pos2};
use egui_kittest::Harness;
use paintrs::PaintApp;
use pdn_core::action::FormId;
use pdn_core::app::AppWorkspace;
use pdn_core::tool::ToolType;
use serde_json::{Value, json};

const OUT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/screenshots");
const SIZE: (f32, f32) = (1280.0, 940.0);

/// The desktop app with the default icon pack and theme on its blank 800 × 600 image.
fn blank() -> Harness<'static, PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(SIZE.0, SIZE.1))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(|cc| {
            let mut app = PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
            app.shell
                .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
            app
        });
    h.run_steps(2);
    // The blank 800 × 600 image lies at `CANVAS`: so the canvas area starts here.
    let (x, y) = screen(&h.state().app, (0.0, 0.0));
    WIDGET.get_or_init(|| CANVAS.min - pos2(x as f32, y as f32));
    h
}

/// Where the canvas area starts in the window ([`screen`] points are from there).
static WIDGET: std::sync::OnceLock<egui::Vec2> = std::sync::OnceLock::new();

/// Image point `p` in the window.
fn window(h: &Harness<'_, PaintApp>, p: (f64, f64)) -> Pos2 {
    let (x, y) = screen(&h.state().app, p);
    pos2(x as f32, y as f32) + *WIDGET.get().unwrap()
}

/// Image rect `r` in the window.
fn window_rect(h: &Harness<'_, PaintApp>, r: Rect) -> Rect {
    let at = |p: Pos2| window(h, (p.x.into(), p.y.into()));
    Rect::from_min_max(at(r.min), at(r.max))
}

/// [`blank`] with the picture.
fn app() -> Harness<'static, PaintApp> {
    let mut h = blank();
    scene(&mut h.state_mut().app);
    settle(&mut h);
    h
}

/// Runs effect `id` on the active layer with its defaults overridden by `values`.
fn effect(app: &mut AppWorkspace, id: &str, values: Value) {
    let def = pdn_core::effect::find(id).unwrap_or_else(|| panic!("{id}"));
    let s = pdn_core::effect::session::EffectSession::begin(app, def).expect("a layer");
    let mut v = s.defaults();
    v.extend(values.as_object().unwrap().clone());
    assert!(s.commit(app, v), "{id}");
}

/// A new layer above the active one, named `name`.
fn layer(app: &mut AppWorkspace, name: &str) {
    pdn_core::action::perform(app, "menu.layers.add.new.layer");
    let ws = app.active_ws_mut().unwrap();
    let i = ws.active_layer;
    ws.document.layers.get_at_mut(i).unwrap().properties.name = name.into();
}

/// The bottom layer, named Sky: a dusk gradient with screened clouds.
fn sky(app: &mut AppWorkspace) {
    let ws = app.active_ws_mut().unwrap();
    ws.document.layers.get_at_mut(0).unwrap().properties.name = "Sky".into();
    effect(
        app,
        "bbGradient",
        json!({"from": "#24305E", "to": "#F6B26B", "shape": "vertical"}),
    );
    effect(
        app,
        "cloudsEffect",
        json!({"color1": "#000000", "color2": "#484848", "scale": 500, "roughness": 0.35,
               "seed": 7, "blendMode": "screen"}),
    );
}

/// The big tree at the right, as `pcTree` draws it on the active layer.
fn tree(app: &mut AppWorkspace) {
    effect(
        app,
        "pcTree",
        json!({"tree": "7", "seed": 3, "at": [0.76, 0.99], "branchesHeight": 80}),
    );
}

/// The active layer as an editable text layer, as the Text tool's "Editable text" commits one:
/// `text` in bold white at `size` px from `origin`.
fn text(app: &mut AppWorkspace, text: &str, size: f64, origin: (f64, f64)) {
    let mut settings = pdn_core::tool::text::TextSettings::from_tool_settings(
        &app.tool_settings,
        pdn_core::fonts::FontPolicy::Bundled,
    );
    (settings.size, settings.bold) = (size, true);
    let spec = pdn_core::document::text_layer::TextSpec {
        text: text.into(),
        settings,
        origin,
        color: [255, 255, 255, 255],
        dpi: 96.0,
    };
    let ws = app.active_ws_mut().unwrap();
    let (w, h) = (ws.document.width, ws.document.height);
    let l = ws.document.layers.get_at_mut(ws.active_layer).unwrap();
    l.surface = pdn_core::document::text_layer::render(&spec, w, h);
    l.text = Some(spec);
    ws.invalidate(None);
}

/// The picture: a dusk sky with screened clouds and a sun flare, a tree, and an editable title.
fn scene(app: &mut AppWorkspace) {
    sky(app);
    effect(
        app,
        "pcLensFlare",
        json!({"brightness": 130, "center": [-0.5, -0.45]}),
    );
    layer(app, "Tree");
    tree(app);
    layer(app, "Title");
    text(app, "paint.rs", 64.0, (48.0, 470.0));
}

/// Frames with real time between them, so preview workers and thumbnails finish.
fn settle(h: &mut Harness<'_, PaintApp>) {
    for _ in 0..30 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(40));
    }
}

fn hover(h: &mut Harness<'_, PaintApp>, p: Pos2) {
    h.event(egui::Event::PointerMoved(p));
    h.run_steps(3);
}

fn button(h: &mut Harness<'_, PaintApp>, p: Pos2, pressed: bool) {
    h.event(egui::Event::PointerButton {
        pos: p,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
}

fn click(h: &mut Harness<'_, PaintApp>, p: Pos2) {
    hover(h, p);
    button(h, p, true);
    button(h, p, false);
}

/// A left-button drag in `steps` moves.
fn drag(h: &mut Harness<'_, PaintApp>, from: Pos2, to: Pos2, steps: u32) {
    hover(h, from);
    button(h, from, true);
    for i in 1..=steps {
        hover(h, from.lerp(to, i as f32 / steps as f32));
    }
    button(h, to, false);
}

/// Closes the floating windows.
fn hide_forms(h: &mut Harness<'_, PaintApp>) {
    let s = h.state_mut();
    for form in [
        FormId::Tools,
        FormId::Colors,
        FormId::History,
        FormId::Layers,
    ] {
        s.shell.forms.toggle(form, &mut s.app);
    }
}

/// The top dialog beside the canvas: the floating windows closed, the dialog dragged by its
/// caption to the window's right edge, the pointer moved off; the canvas and dialog saved.
fn beside_canvas(h: &mut Harness<'_, PaintApp>, name: &str) {
    beside_canvas_at(h, name, WIDE);
}

/// [`beside_canvas`] grown to `aspect` (width / height).
fn beside_canvas_at(h: &mut Harness<'_, PaintApp>, name: &str, aspect: f32) {
    hide_forms(h);
    let r = dialog_rect(h);
    let from = pos2(r.center().x, r.min.y + 15.0);
    drag(h, from, pos2(from.x + SIZE.0 - 16.0 - r.max.x, from.y), 6);
    hover(h, pos2(SIZE.0 / 2.0, SIZE.1 - 4.0));
    let r = to_aspect(dialog_rect(h).union(CANVAS).expand(8.0), aspect);
    save(h, name, r, 0.0);
}

/// The gallery's tile shapes (width / height): two tiles side by side share one, so no row has
/// a taller tile. `WIDE`: a dialog beside the canvas. `SQUARE`: a menu. `DIALOG`: a dialog with
/// its own preview. Before/after pairs come out at about 2.64.
const WIDE: f32 = 1.70;
const SQUARE: f32 = 1.25;
const DIALOG: f32 = 1.39;

/// `r` grown on its short side to `aspect`, then moved inside the window.
fn to_aspect(r: Rect, aspect: f32) -> Rect {
    let size = if r.width() / r.height() < aspect {
        egui::vec2(r.height() * aspect, r.height())
    } else {
        egui::vec2(r.width(), r.width() / aspect)
    };
    let r = Rect::from_center_size(r.center(), size);
    let shift = egui::vec2(
        (WHOLE.min.x - r.min.x).max(0.0) + (WHOLE.max.x - r.max.x).min(0.0),
        (WHOLE.min.y - r.min.y).max(0.0) + (WHOLE.max.y - r.max.y).min(0.0),
    );
    r.translate(shift)
}

/// Opens effect or adjustment `id`'s dialog as its menu item does, with `values` over its defaults.
/// On the Sky layer: the Title is a text layer, which an effect would ask to rasterize.
fn open_effect(h: &mut Harness<'_, PaintApp>, id: &str, values: Value) {
    open_effect_on(h, 0, id, values);
}

/// [`open_effect`] on layer `layer`.
fn open_effect_on(h: &mut Harness<'_, PaintApp>, layer: usize, id: &str, values: Value) {
    let app = &mut h.state_mut().app;
    app.active_ws_mut().unwrap().active_layer = layer;
    let def = pdn_core::effect::find(id).unwrap_or_else(|| panic!("{id}"));
    let s = pdn_core::effect::session::EffectSession::begin(app, def).expect("a layer");
    let mut v = s.initial_values(app);
    v.extend(values.as_object().unwrap().clone());
    let dialog = paintrs::dialogs::effect_dialog(s, v, |_| {});
    h.state_mut().shell.dialogs.push(dialog);
    // A slow preview (a painterly filter on 800 × 600) takes a few seconds.
    for _ in 0..3 {
        settle(h);
    }
}

fn dialog_rect(h: &Harness<'_, PaintApp>) -> Rect {
    h.state().shell.dialogs.top_rect(&h.ctx).expect("a dialog")
}

fn form_rect(h: &Harness<'_, PaintApp>, form: FormId) -> Rect {
    h.state().shell.forms.window(form).rect()
}

const WHOLE: Rect = Rect {
    min: Pos2::ZERO,
    max: pos2(SIZE.0, SIZE.1),
};

/// The image's 800 × 600 canvas at 100 % in the [`SIZE`] window.
const CANVAS: Rect = Rect {
    min: pos2(232.0, 204.0),
    max: pos2(1032.0, 804.0),
};

/// A gallery tile's largest PNG; the README shows tiles at half width, so a larger one is scaled
/// down until it fits. The full-width main window (`01-main`) keeps its size.
const TILE_BYTES: usize = 350 << 10;

/// Writes `image` as opaque RGB `assets/screenshots/<name>.png` at PNG's best compression.
fn write(image: image::RgbaImage, name: &str) {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    let encode = |rgb: &image::RgbImage| {
        let mut png = vec![];
        let encoder =
            PngEncoder::new_with_quality(&mut png, CompressionType::Best, FilterType::Adaptive);
        rgb.write_with_encoder(encoder).unwrap();
        png
    };
    let mut rgb = image::DynamicImage::ImageRgba8(image).to_rgb8();
    let mut png = encode(&rgb);
    while name != "01-main" && png.len() > TILE_BYTES {
        let k = (TILE_BYTES as f64 / png.len() as f64).sqrt() * 0.97;
        let [w, h] = [rgb.width(), rgb.height()].map(|v| (v as f64 * k).round() as u32);
        rgb = image::imageops::resize(&rgb, w, h, image::imageops::FilterType::Lanczos3);
        png = encode(&rgb);
    }
    std::fs::create_dir_all(OUT).unwrap();
    std::fs::write(format!("{OUT}/{name}.png"), png).unwrap();
}

/// The window cropped to `r` grown by `pad` (kept inside the window), once it has settled.
fn shot(h: &mut Harness<'_, PaintApp>, r: Rect, pad: f32) -> image::RgbaImage {
    settle(h);
    crop(h, r, pad)
}

/// [`shot`] of the last frame, without settling.
fn crop(h: &mut Harness<'_, PaintApp>, r: Rect, pad: f32) -> image::RgbaImage {
    let image = h.render().expect("render");
    let r = r.expand(pad).intersect(WHOLE);
    let [x, y, w, h] = [r.min.x, r.min.y, r.width(), r.height()].map(|v| v.round() as u32);
    image::imageops::crop_imm(&image, x, y, w, h).to_image()
}

/// [`shot`] saved as `name`.
fn save(h: &mut Harness<'_, PaintApp>, name: &str, r: Rect, pad: f32) {
    let image = shot(h, r, pad);
    write(image, name);
}

/// Runs `app`'s frames until its job (a fill, a removal, a heal, a patch) lands.
fn land(app: &mut AppWorkspace) {
    let start = std::time::Instant::now();
    while app.tool_busy() {
        assert!(start.elapsed().as_secs() < 300, "the job never landed");
        std::thread::sleep(std::time::Duration::from_millis(2));
        app.on_frame(app.now_ms + 16.0);
    }
}

/// Image point `p` where the canvas shows it (the tools take canvas points).
fn screen(app: &AppWorkspace, p: (f64, f64)) -> (f64, f64) {
    let view = &app.active_ws().unwrap().view;
    let q = view.to_screen_position(pdn_core::util::geom::Point::new(p.0, p.1));
    (q.x, q.y)
}

/// A drag of `tool` through `points` (image pixels), as the canvas delivers it; a single point
/// is a click. Then its job lands.
fn stroke(app: &mut AppWorkspace, tool: ToolType, points: &[(f64, f64)]) {
    use pdn_core::testing::Ev;
    use pdn_core::tool::MouseButton::Left;
    let points: Vec<_> = points.iter().map(|&p| screen(app, p)).collect();
    let (a, z) = (points[0], points[points.len() - 1]);
    let mut events = vec![Ev::Down(a.0, a.1, Left)];
    events.extend(points[1..].iter().map(|p| Ev::Move(p.0, p.1)));
    events.push(Ev::Up(z.0, z.1, Left));
    pdn_core::testing::drive_tool(app, tool, &events);
    land(app);
}

/// Paintbrush strokes `width` px wide in `rgb`.
fn paint(app: &mut AppWorkspace, strokes: &[&[(f64, f64)]], width: f64, rgb: [u8; 3]) {
    app.update_tool_setting(ToolType::PaintBrush, "width", json!(width));
    let [r, g, b] = rgb;
    let color = pdn_core::util::color::Color::new(r, g, b, 255);
    app.set_colors(color, app.secondary_color);
    for points in strokes {
        stroke(app, ToolType::PaintBrush, points);
    }
}

/// Public-domain and CC0 pictures (the Met's Open Access, Wikimedia Commons) for the README
/// tiles; `SOURCES.md` lists each one's origin.
const SOURCES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/readme-sources");

/// A blank 800 × 600 picture whose one layer is `file` from [`SOURCES`], scaled to fill it and
/// cropped `top` of the way down its overflow (0.5 centres it).
fn source(file: &str, top: f64) -> Harness<'static, PaintApp> {
    let image = image::open(format!("{SOURCES}/{file}"))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
        .to_rgba8();
    let (w, h) = (CANVAS.width() as u32, CANVAS.height() as u32);
    let k = (w as f64 / image.width() as f64).max(h as f64 / image.height() as f64);
    let [sw, sh] = [image.width(), image.height()].map(|v| (v as f64 * k).ceil() as u32);
    let scaled = image::imageops::resize(&image, sw, sh, image::imageops::FilterType::Lanczos3);
    let y = ((sh - h) as f64 * top).round() as u32;
    let fill = image::imageops::crop_imm(&scaled, (sw - w) / 2, y, w, h).to_image();
    let mut harness = blank();
    let ws = harness.state_mut().app.active_ws_mut().unwrap();
    let layer = ws.document.layers.get_at_mut(0).unwrap();
    layer.surface.data_mut().copy_from_slice(fill.as_raw());
    ws.invalidate(None);
    harness
}

/// A `size` picture of `view` (fractions of `file`'s width and height: x0, y0, x1, y1), the
/// floating windows closed: the blank image is resized to `size` (Image ▸ Resize), so the tools'
/// circles, ants and outlines, drawn at screen size, read larger at tile size; then its layer is
/// filled from the source.
fn picture(file: &str, view: [f64; 4], size: (u32, u32)) -> Harness<'static, PaintApp> {
    let image = image::open(format!("{SOURCES}/{file}"))
        .unwrap_or_else(|e| panic!("{file}: {e}"))
        .to_rgba8();
    let (w, h) = (f64::from(image.width()), f64::from(image.height()));
    let [x0, y0, x1, y1] = view;
    let [x, y, cw, ch] = [x0 * w, y0 * h, (x1 - x0) * w, (y1 - y0) * h].map(|v| v.round() as u32);
    let crop = image::imageops::crop_imm(&image, x, y, cw, ch).to_image();
    let fill =
        image::imageops::resize(&crop, size.0, size.1, image::imageops::FilterType::Lanczos3);
    let mut harness = blank();
    hide_forms(&mut harness);
    let app = &mut harness.state_mut().app;
    let dpi = app.active_ws().unwrap().document.resolution;
    use pdn_core::resample::{Gamma, ResizeMode};
    pdn_core::io::ops::resize_image(
        app,
        size.0,
        size.1,
        ResizeMode::Lanczos3,
        Gamma::Linear,
        dpi,
    )
    .expect("resize");
    let ws = app.active_ws_mut().unwrap();
    let layer = ws.document.layers.get_at_mut(0).unwrap();
    layer.surface.data_mut().copy_from_slice(fill.as_raw());
    ws.invalidate(None);
    settle(&mut harness);
    harness
}

/// A left-button press at `from`, then moves to `to` with the button held.
fn press_and_move(h: &mut Harness<'_, PaintApp>, from: Pos2, to: Pos2) {
    hover(h, from);
    button(h, from, true);
    for i in 1..=8 {
        hover(h, from.lerp(to, i as f32 / 8.0));
    }
}

/// Image pixels `(x0, y0)`–`(x1, y1)`.
fn area(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(pos2(x0, y0), pos2(x1, y1))
}

/// `name`: the image's `region` (image pixels) after `prepare` and then after `act`, side by
/// side, each labelled by a text layer ("Label") above the active one. The pointer leaves the
/// canvas before the after shot.
fn before_after(
    h: &mut Harness<'_, PaintApp>,
    name: &str,
    region: Rect,
    prepare: impl FnOnce(&mut Harness<'_, PaintApp>),
    act: impl FnOnce(&mut Harness<'_, PaintApp>),
) {
    let app = &mut h.state_mut().app;
    let active = app.active_ws().unwrap().active_layer;
    layer(app, "Label");
    let size = (f64::from(region.height()) / 9.0).clamp(28.0, 48.0);
    let at = (
        f64::from(region.min.x) + size * 0.6,
        f64::from(region.min.y) + size * 0.85,
    );
    text(app, "Before", size, at);
    app.active_ws_mut().unwrap().active_layer = active;
    prepare(h);
    let r = window_rect(h, region);
    let before = shot(h, r, 8.0);
    act(h);
    let app = &mut h.state_mut().app;
    land(app);
    // No brush outline or Object Finder highlight in the after shot.
    app.set_active_tool_from_type(ToolType::Pan);
    let ws = app.active_ws_mut().unwrap();
    ws.active_layer = (0..)
        .find(|&i| ws.document.layers.get_at(i).unwrap().properties.name == "Label")
        .unwrap();
    text(app, "After", size, at);
    app.active_ws_mut().unwrap().active_layer = active;
    hover(h, pos2(SIZE.0 / 2.0, SIZE.1 - 4.0));
    let after = shot(h, r, 8.0);
    let mut both = image::RgbaImage::new(before.width() + after.width(), before.height());
    image::imageops::replace(&mut both, &before, 0, 0);
    image::imageops::replace(&mut both, &after, before.width().into(), 0);
    write(both, name);
}

/// The README's main gallery: Paint.NET's features and the plugins.
#[test]
#[ignore = "writes assets/screenshots; run with --ignored"]
fn readme_screenshots() {
    // The main window: the Text tool with "Editable text" on, the title a text layer (T badge).
    let mut h = app();
    let ws = &mut h.state_mut().app;
    ws.set_active_tool_from_type(ToolType::Text);
    ws.tool_settings
        .set(ToolType::Text, "editableText", json!(true));
    save(&mut h, "01-main", WHOLE, 0.0);

    // Effects ▸ Artistic: Paint.NET's own, then BB's and PhotoCraft's, both with the gray plus.
    let mut h = app();
    click(&mut h, pos2(334.0, 46.0));
    hover(&mut h, pos2(362.0, 102.0));
    hover(&mut h, pos2(580.0, 561.0));
    let r = Rect::from_min_max(pos2(300.0, 30.0), pos2(1040.0, 585.0));
    save(&mut h, "02-effects-menu", to_aspect(r, SQUARE), 0.0);

    // A Paint.NET effect's live preview on the canvas: Distort ▸ Twist.
    let mut h = source("12-spare-fruit-banquet.jpg", 0.5);
    open_effect(&mut h, "twistEffect", json!({"amount": 45}));
    beside_canvas(&mut h, "03-effect-preview");

    // Adjustments ▸ Curves, an S-curve per channel.
    let mut h = source("13-spare-river-town.jpg", 0.5);
    open_effect(
        &mut h,
        "curvesEffect",
        json!({"channel": "rgb",
               "redPoints": [[0, 0], [70, 50], [190, 215], [255, 255]],
               "greenPoints": [[0, 0], [128, 138], [255, 255]],
               "bluePoints": [[0, 24], [128, 120], [255, 236]]}),
    );
    beside_canvas(&mut h, "04-curves");

    // The tool chooser: the PhotoCraft tools carry the plus.
    let mut h = app();
    let s = h.state_mut();
    for form in [FormId::Tools, FormId::Colors] {
        s.shell.forms.toggle(form, &mut s.app);
    }
    click(&mut h, pos2(52.0, 101.0));
    hover(&mut h, pos2(80.0, 251.0));
    save(
        &mut h,
        "06-tools",
        to_aspect(
            Rect::from_min_max(pos2(0.0, 88.0), pos2(740.0, 745.0)),
            SQUARE,
        ),
        0.0,
    );

    // BB's Chart or Graph: a 3-D column chart.
    use pdn_core::effect::bb::chart::model::{Chart, ChartType, Point, Series};
    let days = ["Mon", "Tue", "Wed", "Thu", "Fri"];
    let series = |name: &str, color, values: [f64; 5]| Series {
        name: name.into(),
        color: Some(color),
        chart_type: ChartType::Column,
        legend_visible: true,
        value_labels: false,
        points: (0..5)
            .map(|i| Point {
                x: i as f64,
                value: values[i],
                value2: 0.0,
                category: days[i].into(),
                exploded: false,
                color: None,
            })
            .collect(),
    };
    let chart = Chart {
        title: "Sketches".into(),
        x_title: "Day".into(),
        y_title: "Count".into(),
        three_d: true,
        series: vec![
            series("Pencil", 0x7DA7E8, [12.0, 19.0, 15.0, 24.0, 21.0]),
            series("Ink", 0xF2B27A, [8.0, 11.0, 14.0, 13.0, 18.0]),
        ],
    };
    let chart = pdn_core::effect::bb::chart::model::encode(&chart).unwrap();
    let mut h = blank();
    open_effect(&mut h, "bbChartOrGraph", json!({ "chart": chart }));
    beside_canvas(&mut h, "09-chart");

    // BB's Text Fun Factory, its preview on the canvas beside the dialog.
    let mut h = blank();
    open_effect(
        &mut h,
        "bbTextFunFactory",
        json!({"text": "paint.rs", "bold": true, "size": 150, "maintain": true, "tab": 4,
               "fillTab": 1, "gradFrom": "#7DA7E8", "gradTo": "#C58BD8"}),
    );
    beside_canvas(&mut h, "10-text-fun-factory");

    // BB's Bevel Object, with its drop shadow, on the title made a bitmap layer.
    let mut h = app();
    let ws = h.state_mut().app.active_ws_mut().unwrap();
    ws.document.layers.get_at_mut(2).unwrap().text = None;
    open_effect_on(
        &mut h,
        2,
        "bbBevelObject",
        json!({"depth": 12, "strength": 1.4, "dropShadow": true}),
    );
    beside_canvas(&mut h, "12-bevel-object");

    // BB's Render Flames over the flattened picture.
    let mut h = app();
    pdn_core::action::perform(&mut h.state_mut().app, "menu.image.flatten");
    open_effect(&mut h, "bbFlames", json!({}));
    beside_canvas(&mut h, "13-flames");
}

/// The README's From PhotoCraft gallery.
#[test]
#[ignore = "writes assets/screenshots; run with --ignored"]
fn photocraft_screenshots() {
    // Editable text: the Title text layer (T in the Layers window) clicked again with the Text
    // tool, "Editable text" on in the tool bar, the caret between "paint" and ".rs".
    let mut h = app();
    let s = h.state_mut();
    for form in [FormId::Tools, FormId::Colors, FormId::History] {
        s.shell.forms.toggle(form, &mut s.app);
    }
    // The caret shows at once instead of fading in.
    s.settings.set("ui.animations", json!(false));
    s.app.set_active_tool_from_type(ToolType::Text);
    s.app
        .tool_settings
        .set(ToolType::Text, "editableText", json!(true));
    settle(&mut h);
    click(&mut h, pos2(437.0, 680.0));
    hover(&mut h, pos2(SIZE.0 / 2.0, SIZE.1 - 4.0));
    settle(&mut h);
    // A key restarts the caret's blink: it is on for the next 300 ms.
    for key in [egui::Key::ArrowRight, egui::Key::ArrowLeft] {
        for pressed in [true, false] {
            h.event(egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
    }
    h.run_steps(2);
    let r = CANVAS.union(form_rect(&h, FormId::Layers));
    let r = Rect::from_min_max(pos2(r.min.x, 97.0), r.max).expand(8.0);
    let image = crop(&mut h, to_aspect(r, DIALOG), 0.0);
    write(image, "05-editable-text");

    // Liquify on a portrait: two Forward Warp strokes curl the moustache up.
    let mut h = source("08-liquify-portrait.jpg", 0.1);
    open_effect(&mut h, "pcLiquify", json!({"size": 40}));
    let d = dialog_rect(&h);
    // An image point in the preview: its origin in the dialog, scaled 726 / 800.
    let at = |x: f32, y: f32| d.min + egui::vec2(13.0 + x * 0.9075, 80.0 + y * 0.9075);
    drag(&mut h, at(452.0, 281.0), at(478.0, 240.0), 8);
    drag(&mut h, at(344.0, 283.0), at(318.0, 242.0), 8);
    // The brush circle off the picture, over the options.
    hover(&mut h, d.min + egui::vec2(850.0, 400.0));
    let r = dialog_rect(&h);
    save(&mut h, "08-liquify", r, 0.0);

    // Remove: a stroke over a flat rock in open water, red while held with the brush at its
    // end; Enter fills it from the water around it. (The rowing boat touches the shore: its
    // fill copies shore buildings into the water.)
    let mut h = picture(
        "01-remove-boat.jpg",
        [0.385, 0.302, 0.985, 0.878],
        (640, 480),
    );
    let rock = [(108.0, 306.0), (180.0, 306.0), (248.0, 306.0)];
    before_after(
        &mut h,
        "21-remove",
        area(20.0, 150.0, 420.0, 450.0),
        |h| {
            let app = &mut h.state_mut().app;
            app.update_tool_setting(ToolType::Remove, "width", json!(50));
            app.update_tool_setting(ToolType::Remove, "removeEachStroke", json!(false));
            stroke(app, ToolType::Remove, &rock);
            let p = window(h, rock[2]);
            hover(h, p);
        },
        |h| {
            let enter = [pdn_core::testing::Ev::Key("Enter")];
            pdn_core::testing::drive_tool(&mut h.state_mut().app, ToolType::Remove, &enter);
        },
    );

    // Healing Brush: the print's own dark stain. Ctrl+click on the grass to its left sets the
    // source (the Before shows it and the brush over the stain); four strokes, wider than the
    // stain so the blend's border is clean grass, heal it.
    let mut h = picture(
        "02-healing-battlefield.jpg",
        [0.15, 0.405, 0.85, 0.75],
        (640, 480),
    );
    before_after(
        &mut h,
        "23-healing-brush",
        area(100.0, 100.0, 500.0, 400.0),
        |h| {
            use pdn_core::testing::Ev;
            use pdn_core::tool::MouseButton::Left;
            let app = &mut h.state_mut().app;
            app.update_tool_setting(ToolType::HealingBrush, "width", json!(36));
            let (x, y) = screen(app, (150.0, 180.0));
            pdn_core::testing::drive_tool(
                app,
                ToolType::HealingBrush,
                &[Ev::DownMods(x, y, Left, "Ctrl"), Ev::Up(x, y, Left)],
            );
            let p = window(h, (295.0, 245.0));
            hover(h, p);
        },
        |h| {
            let app = &mut h.state_mut().app;
            for x in [258.0, 284.0, 310.0, 336.0] {
                stroke(app, ToolType::HealingBrush, &[(x, 180.0), (x, 312.0)]);
            }
        },
    );

    // Patch: a brown stain added to the print's sky and lassoed, the pointer on it (mid-drag the
    // live preview already shows the stain repaired); the drag onto clean sky patches it.
    let mut h = picture("03-patch-fuji.jpg", [0.0, 0.0327, 1.0, 0.9673], (640, 480));
    let app = &mut h.state_mut().app;
    paint(
        app,
        &[&[(436.0, 88.0), (458.0, 99.0), (445.0, 112.0)]],
        24.0,
        [140, 104, 72],
    );
    let lasso: Vec<(f64, f64)> = (0..=24)
        .map(|i| {
            let a = std::f64::consts::TAU * f64::from(i) / 24.0;
            (446.0 + 35.0 * a.cos(), 100.0 + 35.0 * a.sin())
        })
        .collect();
    stroke(app, ToolType::Patch, &lasso);
    before_after(
        &mut h,
        "24-patch",
        area(264.0, 16.0, 616.0, 280.0),
        |h| {
            let p = window(h, (446.0, 100.0));
            hover(h, p);
        },
        |h| {
            let (from, to) = (window(h, (446.0, 100.0)), window(h, (352.0, 100.0)));
            press_and_move(h, from, to);
            button(h, to, false);
            let app = &mut h.state_mut().app;
            land(app);
            pdn_core::action::perform(app, "menu.edit.deselect");
        },
    );

    // Spot Healing Brush: dust specks on an 1854 print, the brush over one in the Before; one
    // click on each.
    let mut h = picture(
        "04-spot-healing-jerusalem.jpg",
        [0.0, 0.0238, 1.0, 0.9762],
        (640, 480),
    );
    let specks = [
        (88.0, 64.0),
        (144.0, 120.0),
        (208.0, 48.0),
        (264.0, 104.0),
        (320.0, 72.0),
        (352.0, 136.0),
        (120.0, 224.0),
        (336.0, 240.0),
    ];
    let dots: Vec<[(f64, f64); 1]> = specks.iter().map(|&p| [p]).collect();
    let dots: Vec<&[(f64, f64)]> = dots.iter().map(|d| &d[..]).collect();
    paint(&mut h.state_mut().app, &dots, 7.0, [70, 55, 45]);
    before_after(
        &mut h,
        "22-spot-healing-brush",
        area(48.0, 16.0, 400.0, 280.0),
        |h| {
            let app = &mut h.state_mut().app;
            app.set_active_tool_from_type(ToolType::SpotHealingBrush);
            app.update_tool_setting(ToolType::SpotHealingBrush, "width", json!(16));
            let p = window(h, specks[3]);
            hover(h, p);
        },
        |h| {
            let app = &mut h.state_mut().app;
            for p in specks {
                stroke(app, ToolType::SpotHealingBrush, &[p]);
            }
        },
    );

    // Object Selection: a box dragged around the violin, turned on its side with Image ▸ Rotate
    // 90° Clockwise (the Before is mid-drag); the release selects the violin alone, and Invert
    // Selection with Brightness / Contrast darkens the rest. (A box around the teapot missed
    // the lower half of its pale handle.)
    let mut h = picture("16-select-violin.jpg", [0.05, 0.05, 0.95, 0.95], (480, 640));
    let app = &mut h.state_mut().app;
    pdn_core::io::ops::transform_document(app, pdn_core::io::ops::Transform::Rot90CW)
        .expect("rotate");
    settle(&mut h);
    before_after(
        &mut h,
        "25-object-selection",
        area(0.0, 0.0, 640.0, 480.0),
        |h| {
            let app = &mut h.state_mut().app;
            app.set_active_tool_from_type(ToolType::ObjectSelection);
            let (from, to) = (window(h, (16.0, 120.0)), window(h, (624.0, 360.0)));
            press_and_move(h, from, to);
        },
        |h| {
            let to = window(h, (624.0, 360.0));
            button(h, to, false);
            let app = &mut h.state_mut().app;
            land(app);
            pdn_core::action::perform(app, "menu.edit.invertSelection");
            effect(
                app,
                "brightnessAndContrastAdjustment",
                json!({"brightness": -70, "contrast": -20}),
            );
            pdn_core::action::perform(app, "menu.edit.invertSelection");
        },
    );

    // Edit ▸ Fill Selection (Content-Aware) (Content Aware Fill at its defaults): the carriage on
    // the road selected and filled from the road and grass around it.
    let mut h = source("05-content-aware-landscape.jpg", 0.5);
    hide_forms(&mut h);
    let ws = h.state_mut().app.active_ws_mut().unwrap();
    ws.selection.set_continuation(
        &pdn_core::util::geom::Rectangle::new(420.0, 416.0, 54.0, 60.0),
        pdn_core::selection::CombineMode::Replace,
    );
    ws.selection.commit_continuation();
    before_after(
        &mut h,
        "11-content-aware-fill",
        area(250.0, 240.0, 690.0, 570.0),
        |_| {},
        |h| {
            let app = &mut h.state_mut().app;
            effect(app, "contentAwareFill", json!({}));
            pdn_core::action::perform(app, "menu.edit.deselect");
        },
    );

    // Camera Raw on an 1869 panorama: more contrast, clarity, dehaze and vibrance.
    let mut h = source("06-camera-raw-hongkong.jpg", 0.5);
    hide_forms(&mut h);
    let whole = Rect::from_min_size(Pos2::ZERO, CANVAS.size());
    before_after(
        &mut h,
        "15-camera-raw",
        whole,
        |_| {},
        |h| {
            effect(
                &mut h.state_mut().app,
                "pcCameraRaw",
                json!({"contrast": 35, "clarity": 45, "dehaze": 40, "vibrance": 30}),
            );
        },
    );

    // Lighting Effects: its dialog beside the live preview on the canvas.
    let mut h = source("07-lighting-wheatfield.jpg", 0.5);
    open_effect(
        &mut h,
        "pcLightingEffects",
        json!({"lightType": "spot", "texture": "luminance", "height": 60}),
    );
    beside_canvas_at(&mut h, "16-lighting-effects", DIALOG);

    // Tilt-Shift on an 1860s view of Cairo: a sharp band through the mosque.
    let mut h = source("10-tilt-shift-cairo.jpg", 0.5);
    hide_forms(&mut h);
    let whole = Rect::from_min_size(Pos2::ZERO, CANVAS.size());
    before_after(
        &mut h,
        "19-tilt-shift",
        whole,
        |_| {},
        |h| {
            effect(
                &mut h.state_mut().app,
                "pcTiltShift",
                json!({"blur": 25, "center": [0.0, -0.2]}),
            );
        },
    );
}

/// The README's Perspective Warp tile: Layers ▸ Perspective Warp on the canvas puts the paint.rs
/// logo, pasted upright on its own layer, onto a brick wall that recedes to the right. The Before
/// is the logo flat over the wall, the mode in Warp with nothing dragged yet; its corners dragged
/// onto the wall's perspective, Enter, and the layer's blend mode make the After. Its own test so
/// it runs alone: `cargo test -p paintrs --test readme_screenshots perspective -- --ignored`.
#[test]
#[ignore = "writes assets/screenshots; run with --ignored"]
fn perspective_warp_screenshot() {
    // The loading bay's right-hand wall at 1:1 (source px 868–1188 × 114–566).
    let mut h = picture(
        "09-perspective-brick-wall.jpg",
        [
            868.0 / 1600.0,
            114.0 / 1060.0,
            1188.0 / 1600.0,
            566.0 / 1060.0,
        ],
        (320, 452),
    );
    // Where the logo goes, measured on the source: the roof edge and the soldier courses meet at
    // the vanishing point (3277, 690), and along them a course is 6.36 px and a brick 11.94 px
    // tall and long at x = 1100, each shrinking towards it. A rectangle 13.07 bricks long and 28
    // courses tall (a brick is 3 courses) has the logo's 1.40, 4 courses under the roof edge:
    // its sides at x = 945 and 1111, its top and bottom on lines through the vanishing point.
    let wall = [(77.0, 87.6), (243.3, 122.4), (243.3, 299.6), (77.0, 278.4)];
    // The logo cropped to its visible bounds, upright over the middle of that rectangle.
    let logo = image::open(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../assets/app-icon.png"
    ))
    .expect("the logo")
    .crop_imm(18, 160, 988, 705)
    .resize_exact(230, 164, image::imageops::FilterType::Lanczos3)
    .to_rgba8();
    let (x, y) = (45.0, 115.0);
    let rect = [
        (x, y),
        (x + 230.0, y),
        (x + 230.0, y + 164.0),
        (x, y + 164.0),
    ];
    let app = &mut h.state_mut().app;
    layer(app, "Logo");
    let ws = app.active_ws_mut().unwrap();
    let l = ws.document.layers.get_at_mut(ws.active_layer).unwrap();
    let data = pdn_core::canvas::ImageData {
        width: logo.width(),
        height: logo.height(),
        data: logo.into_raw(),
    };
    l.surface.put_image_data(&data, x as i32, y as i32);
    ws.invalidate(None);
    settle(&mut h);
    // Each corner from the logo's onto the wall's: top-left, top-right, bottom-left, then
    // bottom-right (in this order the quad stays convex, which a corner drag needs).
    let corner = |h: &mut Harness<'_, PaintApp>, k: usize| {
        let (from, to) = (window(h, rect[k]), window(h, wall[k]));
        drag(h, from, to, 6);
    };
    before_after(
        &mut h,
        "17-perspective-warp",
        area(16.0, 16.0, 304.0, 436.0),
        |h| {
            pdn_core::action::perform(&mut h.state_mut().app, "menu.layers.perspectiveWarp");
            h.run_steps(3);
            // The options row's Mode: Layout to Warp.
            let r = h.state().shell.tool_menu.anchor("mode").expect("mode");
            click(h, pos2(r.min.x + 8.0, r.center().y));
            let mode = h.state().app.perspective.as_ref().map(|p| p.mode);
            assert_eq!(mode, Some(pdn_core::tool::perspective_warp::Mode::Warp));
            hover(h, pos2(SIZE.0 / 2.0, SIZE.1 - 4.0));
        },
        |h| {
            for k in [0, 1, 3, 2] {
                corner(h, k);
            }
            h.event(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
            h.run_steps(2);
            assert!(h.state().app.perspective.is_none(), "Enter commits");
            // Painted on: the layer in Overlay lets the bricks and mortar through. (Multiply
            // darkened it to a stain and lost the brush, Normal at 78 % read as a sticker.)
            let ws = h.state_mut().app.active_ws_mut().unwrap();
            let l = ws.document.layers.get_at_mut(ws.active_layer).unwrap();
            l.properties.blend_mode = pdn_core::document::layer::BlendMode::Overlay;
            ws.invalidate(None);
        },
    );
}

/// Every screenshot the README shows exists, and every screenshot is shown.
#[test]
fn readme_shows_every_screenshot() {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
    let readme = std::fs::read_to_string(format!("{root}/README.md")).unwrap();
    let mut shown: Vec<&str> = readme
        .split("assets/screenshots/")
        .skip(1)
        .map(|rest| &rest[..rest.find(".png").expect("a .png") + 4])
        .collect();
    shown.sort();
    shown.dedup();
    let mut files: Vec<String> = std::fs::read_dir(OUT)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    assert_eq!(shown, files);
}
