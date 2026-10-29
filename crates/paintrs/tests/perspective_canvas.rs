//! Layers ▸ Perspective Warp on the canvas in the real app (`pdn_core::tool::perspective_warp`):
//! pointer drags on the canvas, the options row's buttons, Enter and Esc. The ignored
//! `perspective_renders` writes the mode's pictures (Layout, Warp after Straighten, the Crop to
//! plane result) to `$PERSP_RENDER_DIR` (default: the target's tmp dir):
//! `PERSP_RENDER_DIR=… cargo test -p paintrs --test perspective_canvas -- --ignored`.
use egui::{Pos2, Rect, pos2};
use egui_kittest::Harness;
use paintrs::PaintApp;
use pdn_core::action::FormId;
use pdn_core::canvas::ImageData;
use pdn_core::extra::perspective::Homography;
use pdn_core::tool::perspective_warp::{self, Mode};
use pdn_core::util::geom::Point;

const SIZE: (f32, f32) = (1280.0, 940.0);
/// The photographed sheet's corners (TL, TR, BR, BL) on the 800 × 600 image.
const PAPER: [[f64; 2]; 4] = [
    [236.0, 128.0],
    [604.0, 186.0],
    [566.0, 520.0],
    [168.0, 446.0],
];
const IMAGE: [[f64; 2]; 4] = [[0.0, 0.0], [800.0, 0.0], [800.0, 600.0], [0.0, 600.0]];

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
    h
}

/// A sheet of paper with a heading and lines of text, photographed at an angle on a desk.
fn photo() -> ImageData {
    let inv = Homography::rect_to_quad([0.0, 0.0, 1.0, 1.0], PAPER)
        .and_then(|m| m.inverse())
        .unwrap();
    let mut data = Vec::with_capacity(800 * 600 * 4);
    for y in 0..600 {
        for x in 0..800 {
            let (u, v) = inv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
            let line = v > 0.24 && v < 0.9 && (v - 0.24) % 0.07 < 0.02;
            let px = if !((0.0..1.0).contains(&u) && (0.0..1.0).contains(&v)) {
                [118, 96, 78, 255]
            } else if (0.1..0.6).contains(&u) && (0.08..0.15).contains(&v) {
                [52, 74, 128, 255]
            } else if (0.1..0.9).contains(&u) && line {
                [150, 150, 150, 255]
            } else {
                [246, 244, 238, 255]
            };
            data.extend(px);
        }
    }
    ImageData {
        width: 800,
        height: 600,
        data,
    }
}

/// [`blank`] with the photo and Layers ▸ Perspective Warp started on it, as its menu item does;
/// the Tools and Colors windows closed (they float over the canvas's left corners).
fn app() -> Harness<'static, PaintApp> {
    let mut h = blank();
    let s = h.state_mut();
    for form in [FormId::Tools, FormId::Colors] {
        s.shell.forms.toggle(form, &mut s.app);
    }
    let ws = h.state_mut().app.active_ws_mut().unwrap();
    ws.document
        .layers
        .get_at_mut(0)
        .unwrap()
        .surface
        .put_image_data(&photo(), 0, 0);
    ws.invalidate(None);
    pdn_core::action::perform(&mut h.state_mut().app, "menu.layers.perspectiveWarp");
    assert!(h.state().app.perspective.is_some());
    h.run_steps(3);
    ORIGIN.get_or_init(|| CANVAS - view_point(&h, [0.0, 0.0]));
    h
}

fn hover(h: &mut Harness<'_, PaintApp>, p: Pos2) {
    h.event(egui::Event::PointerMoved(p));
    h.run_steps(2);
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

/// The 800 × 600 image's canvas at 100 % in the [`SIZE`] window as it opens (the README shots'
/// layout).
const CANVAS: Pos2 = pos2(232.0, 204.0);
/// The canvas view's top-left in the window, from [`CANVAS`] before anything scrolled.
static ORIGIN: std::sync::OnceLock<Pos2> = std::sync::OnceLock::new();

/// Image point `p` from the canvas view's top-left.
fn view_point(h: &Harness<'_, PaintApp>, p: [f64; 2]) -> egui::Vec2 {
    let ws = h.state().app.active_ws().unwrap();
    let s = ws.view.to_screen_position(Point::new(p[0], p[1]));
    egui::vec2(s.x as f32, s.y as f32)
}

/// Where the canvas shows image point `p`.
fn screen(h: &Harness<'_, PaintApp>, p: [f64; 2]) -> Pos2 {
    ORIGIN.get().copied().unwrap() + view_point(h, p)
}

/// A left drag on the canvas from image point `from` to `to`.
fn drag(h: &mut Harness<'_, PaintApp>, from: [f64; 2], to: [f64; 2]) {
    let (a, b) = (screen(h, from), screen(h, to));
    hover(h, a);
    button(h, a, true);
    for i in 1..=4 {
        hover(h, a.lerp(b, i as f32 / 4.0));
    }
    button(h, b, false);
}

/// Clicks the options row's control `key` (its main part for a split toggle).
fn option(h: &mut Harness<'_, PaintApp>, key: &str) {
    let r: Rect = h.state().shell.tool_menu.anchor(key).expect(key);
    click(h, pos2(r.min.x + 8.0, r.center().y));
}

fn key(h: &mut Harness<'_, PaintApp>, key: egui::Key) {
    h.event(egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run_steps(2);
}

fn layer(h: &Harness<'_, PaintApp>) -> Vec<u8> {
    let ws = h.state().app.active_ws().unwrap();
    ws.document
        .layers
        .get_at(0)
        .unwrap()
        .surface
        .data()
        .to_vec()
}

fn history(h: &Harness<'_, PaintApp>) -> Vec<String> {
    let ws = h.state().app.active_ws().unwrap();
    ws.history.undo.iter().map(|m| m.name.clone()).collect()
}

fn mode<'a>(h: &'a Harness<'_, PaintApp>) -> &'a perspective_warp::PerspectiveWarp {
    h.state().app.perspective.as_ref().expect("the mode")
}

/// Layout: the corners onto the paper.
/// Layout (the mode starts in Warp), then the plane's corners onto the paper.
fn place(h: &mut Harness<'_, PaintApp>) {
    if mode(h).mode == Mode::Warp {
        option(h, "mode");
    }
    for k in 0..4 {
        drag(h, IMAGE[k], PAPER[k]);
    }
}

fn near(a: [[f64; 2]; 4], b: [[f64; 2]; 4]) -> bool {
    a.iter()
        .zip(b)
        .all(|(p, q)| (p[0] - q[0]).abs() <= 1.0 && (p[1] - q[1]).abs() <= 1.0)
}

/// The mode starts in Warp; canvas drags move the plane in Layout (the image stays) and warp the
/// image in Warp; the options row switches the mode and straightens; Enter commits one history
/// item, Esc cancels.
#[test]
fn canvas_drags_warp_and_the_options_row_drives_the_mode() {
    let mut h = app();
    let before = layer(&h);
    assert_eq!(mode(&h).mode, Mode::Warp);
    place(&mut h);
    assert_eq!(mode(&h).mode, Mode::Layout);
    assert!(near(mode(&h).src, PAPER), "{:?}", mode(&h).src);
    assert_eq!(mode(&h).dst, mode(&h).src);
    assert_eq!(layer(&h), before, "Layout leaves the image");
    option(&mut h, "mode");
    assert_eq!(mode(&h).mode, Mode::Warp);
    let src = mode(&h).src;
    drag(&mut h, src[1], [640.0, 150.0]);
    assert_eq!(mode(&h).src, src);
    assert_ne!(layer(&h), before, "the image follows the corner");
    option(&mut h, "straightenAuto");
    let d = mode(&h).dst;
    assert!(d[0][1] == d[1][1] && d[1][0] == d[2][0] && d[2][1] == d[3][1] && d[3][0] == d[0][0]);
    key(&mut h, egui::Key::Enter);
    assert!(h.state().app.perspective.is_none());
    assert_eq!(history(&h), ["New Image", "Perspective Warp"]);

    let mut h = app();
    option(&mut h, "mode");
    drag(&mut h, IMAGE[0], PAPER[0]);
    option(&mut h, "mode");
    drag(&mut h, PAPER[0], [200.0, 60.0]);
    assert_ne!(layer(&h), before);
    key(&mut h, egui::Key::Escape);
    assert!(h.state().app.perspective.is_none());
    assert_eq!(layer(&h), before);
    assert_eq!(history(&h), ["New Image"]);
    // The Commit button, untouched: nothing to apply.
    pdn_core::action::perform(&mut h.state_mut().app, "menu.layers.perspectiveWarp");
    h.run_steps(2);
    option(&mut h, "commit");
    assert!(h.state().app.perspective.is_none());
    assert_eq!(history(&h), ["New Image"]);
}

/// Crop to plane from the row: the paper alone becomes the image.
#[test]
fn crop_to_plane_from_the_options_row() {
    let mut h = app();
    place(&mut h);
    option(&mut h, "cropToPlane");
    assert!(mode(&h).crop);
    let size = pdn_core::extra::perspective::crop_size(&mode(&h).dst);
    option(&mut h, "commit");
    let ws = h.state().app.active_ws().unwrap();
    assert_eq!((ws.document.width, ws.document.height), size);
    assert_eq!(history(&h), ["New Image", "Perspective Crop"]);
}

fn save(h: &mut Harness<'_, PaintApp>, name: &str) {
    hover(h, pos2(SIZE.0 / 2.0, SIZE.1 - 4.0));
    for _ in 0..10 {
        h.run_steps(1);
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    let dir =
        std::env::var("PERSP_RENDER_DIR").unwrap_or_else(|_| env!("CARGO_TARGET_TMPDIR").into());
    std::fs::create_dir_all(&dir).unwrap();
    let path = format!("{dir}/{name}.png");
    h.render().expect("render").save(&path).unwrap();
    eprintln!("wrote {path}");
}

#[test]
#[ignore = "writes PNGs; run with --ignored"]
fn perspective_renders() {
    let mut h = app();
    place(&mut h);
    save(&mut h, "persp-layout");
    option(&mut h, "mode");
    option(&mut h, "straightenAuto");
    save(&mut h, "persp-warp-straightened");
    let mut h = app();
    place(&mut h);
    option(&mut h, "cropToPlane");
    save(&mut h, "persp-crop-armed");
    option(&mut h, "commit");
    save(&mut h, "persp-crop-result");
}
