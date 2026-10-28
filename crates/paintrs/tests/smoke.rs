use egui_kittest::Harness;

#[test]
fn app_shows_the_initial_document_on_the_background() {
    let mut harness = app_harness();
    let image = harness.render().expect("render");
    assert_eq!(image.dimensions(), (1280, 800));
    // Paint.NET's dark surround around the canvas (canvas-rendering.md §2.7 step 1); the white
    // 800x600 document centred.
    assert_eq!(image.get_pixel(100, 400).0, [0x30, 0x30, 0x30, 0xFF]);
    assert_eq!(image.get_pixel(640, 400).0, [255, 255, 255, 255]);
}

/// The existing paint.js web-chrome coordinate/pixel contract, on the native backend.
/// Keep its web origins explicit: native caption geometry and interaction are covered by
/// desktop.rs, settings.rs and settings_keyboard.rs. Thumbnails render synchronously.
fn new_web_chrome_app(cc: &mut eframe::CreationContext<'_>) -> paintrs::PaintApp {
    let mut app = paintrs::PaintApp::new_with_policy(cc, pdn_core::fonts::FontPolicy::Bundled);
    app.shell.desktop = false;
    app.shell
        .set_thumbnail_mode(paintrs::kit::thumbnails::ThumbnailMode::Synchronous);
    app
}

/// Every harness here lets `run` take 16 frames to settle (egui_kittest's default is 4): a
/// background job (an effect preview, a thumbnail) that lands late on a loaded machine repaints a
/// few frames after the input.
fn app_harness() -> Harness<'static, paintrs::PaintApp> {
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    harness.run();
    harness
}

fn click(harness: &mut Harness<'_, paintrs::PaintApp>, x: f32, y: f32) {
    let pos = egui::pos2(x, y);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
}

/// Task 21 review: a press on the canvas owns the pointer until its release; when the window
/// loses focus mid-press the release goes elsewhere, so the capture ends then (MouseUp at the last
/// canvas position).
#[test]
fn focus_loss_releases_the_canvas_press() {
    let mut harness = app_harness();
    let pos = egui::pos2(640.0, 400.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    assert!(harness.state().shell.canvas_view.captured().is_some());
    harness.event(egui::Event::WindowFocused(false));
    harness.run();
    assert_eq!(harness.state().shell.canvas_view.captured(), None);
}

/// The text runs (text-kind rects with their text) of a fresh app at `ppp` whose settings JSON is
/// `settings`, after `act`.
fn text_runs(
    ppp: f32,
    settings: &str,
    act: impl Fn(&mut Harness<'_, paintrs::PaintApp>),
) -> Vec<(egui::Rect, String)> {
    let mut storage = serde_json::Map::new();
    storage.insert("paintdotjs.welcome.dismissed".into(), "true".into());
    storage.insert("paintdotjs.settings.v1".into(), settings.into());
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(ppp)
        .with_context_setup(move |ctx| paintrs::seed_native_scale(ctx, ppp))
        .build_eframe(move |cc| paintrs::PaintApp::new_for_test(&cc.egui_ctx, storage));
    harness.run_steps(3);
    act(&mut harness);
    harness.run_steps(2);
    paintrs::kit::text::take_text_runs(&harness.ctx)
        .into_iter()
        .filter(|(_, kind, _)| *kind == paintrs::kit::text::RectKind::Text)
        .filter_map(|(r, _, text)| Some((r, text?)))
        .collect()
}

/// The runs recorded at both scales, paired by text and occurrence.
fn paired<'a>(
    a: &'a [(egui::Rect, String)],
    b: &'a [(egui::Rect, String)],
) -> Vec<(egui::Rect, egui::Rect, &'a str)> {
    let nth = |runs: &'a [(egui::Rect, String)], text: &str, n: usize| {
        runs.iter()
            .filter(|(_, t)| t == text)
            .nth(n)
            .map(|(r, _)| *r)
    };
    a.iter()
        .enumerate()
        .filter_map(|(i, (ra, t))| {
            let n = a[..i].iter().filter(|(_, u)| u == t).count();
            Some((*ra, nth(b, t, n)?, t.as_str()))
        })
        .collect()
}

/// Every run's origin at `s` is its 1× origin × `s` within `tol` device px.
fn assert_dip_scaled(
    base: &[(egui::Rect, String)],
    scaled: &[(egui::Rect, String)],
    s: f32,
    tol: f32,
) {
    for (a, b, text) in paired(base, scaled) {
        let (dx, dy) = ((b.min.x - a.min.x) * s, (b.min.y - a.min.y) * s);
        assert!(
            dx.abs() <= tol && dy.abs() <= tol,
            "@{s} {text:?}: {a:?} -> {b:?} (±{tol})"
        );
    }
}

/// R40 (Task 23): Paint.NET lays the chrome out at DIP × ppp (R24), so at 1.25 and 1.5 every text
/// run R40 un-gates sits where 1×'s does, scaled (ink box = line-box top, one DIP left of the
/// glyph origin): the main menu, tool options and status bar within one device px; the Effects
/// drop menu and the forms (captions, Layers/History names, Colors labels) within two, the
/// popup's or form's device position (≤ ½ px), egui's whole-pixel glyph placement in a centred
/// run (≤ ½ px) and the baseline snap (≤ 1 px) adding up; the forms' rows also keep Paint.NET's
/// rnd(62·s) / rnd(20·s) device pitch. Blink's floored borders drift the references' runs up to 4
/// device px (3.3 by the main menu's Effects at 1.25); this pins that paint.rs does not
/// (`ui-text-and-glyphs.md §2.3`, §6.1).
#[test]
fn text_origins_scale_by_dip_at_fractional_scales() {
    const NO_FORMS: &str = r#"{"ui":{"fontFamily":"Selawik"},"windows":{"mainToolBarForm":{"visible":false},"colorsForm":{"visible":false},"historyForm":{"visible":false},"layerForm":{"visible":false}}}"#;
    // Opaque forms: at 1:1 device px the image's size, so which form would turn translucent over
    // it (its text then Paint.NET's pixels), changes with the scale.
    const FORMS: &str = r#"{"ui":{"fontFamily":"Selawik","translucentWindows":false}}"#;
    let none = |_: &mut Harness<'_, paintrs::PaintApp>| {};
    // The Effects menu item's centre (DIPs, the same at every scale).
    let effects = |h: &mut Harness<'_, paintrs::PaintApp>| click(h, 335.0, 16.0);
    let add_layer = |h: &mut Harness<'_, paintrs::PaintApp>| {
        // R22 selected rows are PDN regions. Leave Layer2/Background in the top two unselected
        // slots and the first NewLayer unselected; all origin/pitch assertions stay unchanged.
        for _ in 0..2 {
            h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
            h.run_steps(3);
        }
        for _ in 0..2 {
            pdn_core::action::perform(&mut h.state_mut().app, "menu.layers.move.layer.down");
            h.run_steps(3);
        }
    };
    let row_pitch = |runs: &[(egui::Rect, String)], top: &str, next: &str, s: f32| {
        let y = |t: &str| {
            runs.iter()
                .find(|(_, u)| u == t)
                .map(|(r, _)| r.min.y)
                .unwrap()
        };
        ((y(next) - y(top)) * s).round()
    };
    // The brush size field shows Paint.NET's DPI-scaled default, trunc(2·s) (R19).
    let brush = |s: f32| ((2.0 * s).trunc() as i64).to_string();
    for (settings, act, at_least, tol) in [
        (
            NO_FORMS,
            &none as &dyn Fn(&mut Harness<'_, paintrs::PaintApp>),
            20,
            1.01,
        ),
        (NO_FORMS, &effects, 28, 2.01),
        (FORMS, &add_layer, 30, 2.01),
    ] {
        let base = text_runs(1.0, settings, act);
        assert!(base.len() >= at_least, "{} runs at 1×", base.len());
        for s in [1.25f32, 1.5] {
            let scaled = text_runs(s, settings, act);
            // Every run is recorded at both scales but the brush size where its value changes.
            let scaled_value = |(_, t): &&(egui::Rect, String)| *t == brush(1.0) && brush(s) != *t;
            let want = base.len() - base.iter().filter(scaled_value).count();
            assert_eq!(
                paired(&base, &scaled).len(),
                want,
                "@{s}: runs at both scales"
            );
            assert_dip_scaled(&base, &scaled, s, tol);
            if settings == FORMS {
                for t in ["Tools", "History", "Layers", "Colors", "More >>"] {
                    assert!(scaled.iter().any(|(_, u)| u == t), "@{s}: no {t:?}");
                }
                let dip = |d: f32| paintrs::kit::glyph::dip_px(d, s) as f32;
                assert_eq!(
                    row_pitch(&scaled, "Layer 2", "Background", s),
                    dip(62.0),
                    "@{s}"
                );
                assert_eq!(
                    row_pitch(&scaled, "New Image", "New Layer", s),
                    dip(20.0),
                    "@{s}"
                );
            }
        }
    }
}

/// Three real rows with the newly added Layer 3 still selected in slot 0 (R22/R162).
fn three_layer_text_runs(ppp: f32) -> Vec<(egui::Rect, String)> {
    use paintrs::kit::text::RectKind;
    let mut storage = serde_json::Map::new();
    storage.insert("paintdotjs.welcome.dismissed".into(), "true".into());
    storage.insert(
        "paintdotjs.settings.v1".into(),
        r#"{"ui":{"fontFamily":"Selawik","translucentWindows":false}}"#.into(),
    );
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(ppp)
        .with_context_setup(move |ctx| paintrs::seed_native_scale(ctx, ppp))
        .build_eframe(move |cc| paintrs::PaintApp::new_for_test(&cc.egui_ctx, storage));
    h.run_steps(3);
    for n in 2..=3 {
        h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
        h.run_steps(3);
        let ws = h.state().app.active_ws().unwrap();
        assert_eq!(ws.document.layers.size(), n);
        assert_eq!(ws.active_layer, n - 1);
    }
    h.run_steps(2);
    let ws = h.state().app.active_ws().unwrap();
    let rows: Vec<_> = ws
        .document
        .layers
        .iter()
        .rev()
        .enumerate()
        .map(|(i, layer)| (layer.properties.name.as_str(), i, ws.active_layer == 2 - i))
        .collect();
    assert_eq!(
        rows,
        [
            ("Layer 3", 0, true),
            ("Layer 2", 1, false),
            ("Background", 2, false)
        ]
    );
    let runs = paintrs::kit::text::take_text_runs(&h.ctx);
    assert!(
        !runs
            .iter()
            .any(|(_, _, text)| text.as_deref() == Some("Layer 3"))
    );
    // R22's selected-row PDN box owns the label band. No mask is added by this test.
    let label_band = egui::Rect::from_min_max(egui::pos2(1117.0, 521.0), egui::pos2(1180.0, 546.0));
    let selected: Vec<_> = runs
        .iter()
        .filter(|(r, kind, _)| *kind == RectKind::Pdn && r.contains_rect(label_band))
        .collect();
    assert_eq!(selected.len(), 1, "@{ppp}: selected slot 0 PDN ownership");
    // Default anchor: round(1280-15-5.6-202)+1 = 1058,
    // round(800-32-15-5.6-278)+32 = 501, then the 1-DIP item margin.
    // Keep absolute placement inside R40's 2.01 budget, independent of measured row pitch.
    let row0 = selected[0].0;
    assert!(
        ((row0.min.x - 1059.0) * ppp).abs() <= 2.01,
        "@{ppp}: row 0 x"
    );
    assert!(
        ((row0.min.y - 502.0) * ppp).abs() <= 2.01,
        "@{ppp}: row 0 y"
    );
    let text: Vec<_> = runs
        .into_iter()
        .filter(|(_, kind, _)| *kind == RectKind::Text)
        .filter_map(|(r, _, text)| Some((r, text?)))
        .collect();
    for name in ["Layer 2", "Background"] {
        assert_eq!(
            text.iter().filter(|(_, t)| t == name).count(),
            1,
            "@{ppp}: {name}"
        );
    }
    text
}

/// R162 reconciles R40 with integer row pitch for this additional case only. The original-slot
/// fixture above stays unchanged. Only i*(round(62*s)-62*s) is removed from vertical label drift;
/// form placement, half-row centering, line-height rounding and any extra shift still cost 2.01.
#[test]
fn three_layer_text_origins_follow_the_fractional_row_grid() {
    let base = three_layer_text_runs(1.0);
    assert!(base.len() >= 30, "{} runs at 1×", base.len());
    let brush = |s: f32| ((2.0 * s).trunc() as i64).to_string();
    for (s, pitch, history_pitch) in [(1.25f32, 78.0, 25.0), (1.5, 93.0, 30.0)] {
        let scaled = three_layer_text_runs(s);
        let scaled_value = |(_, t): &&(egui::Rect, String)| *t == brush(1.0) && brush(s) != *t;
        let want = base.len() - base.iter().filter(scaled_value).count();
        assert_eq!(
            paired(&base, &scaled).len(),
            want,
            "@{s}: runs at both scales"
        );
        for (a, b, text) in paired(&base, &scaled) {
            let index = match text {
                "Layer 2" => 1.0,
                "Background" => 2.0,
                _ => 0.0,
            };
            let dx = (b.min.x - a.min.x) * s;
            let dy = (b.min.y - a.min.y) * s;
            let row_term = index * ((62.0 * s).round() - 62.0 * s);
            assert!(
                dx.abs() <= 2.01 && (dy - row_term).abs() <= 2.01,
                "@{s} {text:?}: {a:?} -> {b:?}, dx={dx}, dy={dy}, row term={row_term} (±2.01)"
            );
        }
        for t in ["Tools", "History", "Layers", "Colors", "More >>"] {
            assert!(scaled.iter().any(|(_, u)| u == t), "@{s}: no {t:?}");
        }
        let y = |text: &str| scaled.iter().find(|(_, t)| t == text).unwrap().0.min.y;
        assert_eq!(
            ((y("Background") - y("Layer 2")) * s).round(),
            pitch,
            "@{s}"
        );
        assert_eq!(
            ((y("New Layer") - y("New Image")) * s).round(),
            history_pitch,
            "@{s}"
        );
    }
}

/// Task 23 review: at 2560×1440 @2× a near-full-height Layers form's shadow (and the canvas) is
/// taller and wider than egui's 2048-px `max_texture_side`; the textures tile instead of panicking
/// in `load_texture`.
#[test]
fn big_shadows_and_canvases_tile_at_2x() {
    let mut storage = serde_json::Map::new();
    storage.insert("paintdotjs.welcome.dismissed".into(), "true".into());
    storage.insert(
        "paintdotjs.settings.v1".into(),
        r#"{"ui":{"fontFamily":"Selawik"},"windows":{"layerForm":{"x":2200,"y":100,"width":300,"height":1250}}}"#
            .into(),
    );
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(2560.0, 1440.0))
        .with_pixels_per_point(2.0)
        .with_context_setup(|ctx| paintrs::seed_native_scale(ctx, 2.0))
        .build_eframe(move |cc| paintrs::PaintApp::new_for_test(&cc.egui_ctx, storage));
    harness.run_steps(3);
    let layers = paintrs::forms::FormId::Layers;
    assert!(form_rect(&harness, layers).height() * 2.0 > 2048.0);
}

/// The shadow cache (`kit::shadow`) keys a veil on the versions of the textures under it: a
/// translucent form over the image shows the canvas changed in the frame before, not the cached
/// veil of the old canvas.
#[test]
fn translucent_form_veils_follow_the_canvas() {
    let mut h = app_harness();
    // The image under every form at 100 %: they turn translucent (0.75).
    let app = &mut h.state_mut().app;
    app.create_blank_document_in_new_workspace(3000, 2000, 96.0, false);
    pdn_core::action::perform(app, "menu.view.actualSize");
    h.run_steps(8);
    // The Tools form: nothing on it changes with the image (History and Layers do).
    let tools = h.state().shell.forms.window(paintrs::forms::FormId::Tools);
    assert!(tools.translucent);
    let p = tools.rect().center();
    let pixel = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.render()
            .expect("render")
            .get_pixel(p.x as u32, p.y as u32)
            .0
    };
    let white = pixel(&mut h);
    // Steady frames reuse the veil.
    h.run_steps(2);
    assert_eq!(pixel(&mut h), white);
    // White to black under the form.
    let app = &mut h.state_mut().app;
    let def = pdn_core::effect::find("invertColorsEffect").unwrap();
    let s = pdn_core::effect::session::EffectSession::begin(app, def).unwrap();
    let values = s.defaults();
    assert!(s.commit(app, values));
    h.run_steps(2);
    let black = pixel(&mut h);
    // A quarter of the canvas shows through: 64 levels of the white are gone.
    for c in 0..3 {
        assert!(white[c] >= black[c] + 50, "{white:?} -> {black:?}");
    }
    // A partial change (the canvas uploads only the rect it redraws): the image's left half, the
    // Tools form over it, white again.
    let app = &mut h.state_mut().app;
    app.edit_selection(|ws| {
        let half = pdn_core::util::geom::Rectangle::new(0., 0., 1500., 2000.);
        ws.selection
            .set_continuation(&half, pdn_core::selection::CombineMode::Replace);
        ws.selection.commit_continuation();
    });
    pdn_core::action::perform(app, "menu.edit.fillSelectionSecondary");
    h.run_steps(2);
    assert_eq!(pixel(&mut h), white);
}

#[test]
fn shortcuts_dispatch_actions() {
    let mut harness = app_harness();
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    assert_eq!(layers(&harness), 1);
    // Ctrl+Shift+N: Add New Layer; Ctrl+Z undoes it.
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    assert_eq!(layers(&harness), 2);
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.run();
    assert_eq!(layers(&harness), 1);
}

/// On a Mac Cmd is Ctrl (`pdn_modifiers`): Cmd+Shift+N adds a layer and Cmd+Z undoes it. A
/// trackpad pinch (`Event::Zoom`) over the canvas zooms about the pointer.
#[test]
fn mac_command_shortcuts_and_pinch_zoom() {
    let cmd = egui::Modifiers {
        mac_cmd: true,
        command: true,
        ..Default::default()
    };
    let pdn = paintrs::pdn_modifiers(cmd);
    assert!(pdn.ctrl && !pdn.mac_cmd);
    let windows = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    assert_eq!(paintrs::pdn_modifiers(windows), windows);
    let mut harness = app_harness();
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    harness.key_press_modifiers(cmd | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    assert_eq!(layers(&harness), 2);
    harness.key_press_modifiers(cmd, egui::Key::Z);
    harness.run();
    assert_eq!(layers(&harness), 1);
    let zoom = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.active_ws().unwrap().view.zoom;
    let before = zoom(&harness);
    let middle = harness.ctx.content_rect().center();
    harness.event(egui::Event::PointerMoved(middle));
    harness.run();
    harness.event(egui::Event::Zoom(2.0));
    harness.run();
    assert!(
        (zoom(&harness) - before * 2.0).abs() < 1e-6,
        "{before} → {}",
        zoom(&harness)
    );
}

fn colors_control_point(h: &Harness<'_, paintrs::PaintApp>, x: f32, y: f32) -> egui::Pos2 {
    use paintrs::theme::{
        WINDOW_BORDER, WINDOW_TITLE_HEIGHT, WINDOW_TITLE_MARGIN_TOP, WINDOW_TITLE_PADDING,
    };
    let rect = h
        .state()
        .shell
        .forms
        .window(paintrs::forms::FormId::Colors)
        .rect();
    rect.min
        + egui::vec2(
            WINDOW_BORDER + x,
            WINDOW_BORDER
                + WINDOW_TITLE_MARGIN_TOP
                + WINDOW_TITLE_HEIGHT
                + 2.0 * WINDOW_TITLE_PADDING.0
                + y,
        )
}

fn color_add_key(h: &mut Harness<'_, paintrs::PaintApp>) {
    h.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::ALT, egui::Key::F12);
    h.run();
}

#[test]
fn color_add_action_toggles_and_preserves_document() {
    use paintrs::theme::COLORS_PALETTE;
    use pdn_core::util::{color::Color, shortcut::ShortcutKey};
    let mut h = app_harness();
    h.state_mut()
        .shortcuts
        .set_shortcut("colorAdd", Some(ShortcutKey::from_combo("Ctrl+Alt+F12")));
    let snapshot = |h: &Harness<'_, paintrs::PaintApp>| {
        let ws = h.state().app.active_ws().unwrap();
        (
            ws.document
                .layers
                .get_at(0)
                .unwrap()
                .surface
                .data()
                .to_vec(),
            ws.history
                .undo
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>(),
            ws.history
                .redo
                .iter()
                .map(|m| m.name.clone())
                .collect::<Vec<_>>(),
            ws.dirty,
        )
    };
    let before = snapshot(&h);
    let (x, y, _, cell) = COLORS_PALETTE;
    let slot = colors_control_point(&h, x + cell / 2., y + cell / 2.);
    let added = Color::new(9, 8, 7, 6);
    h.state_mut().app.primary_color = added;
    color_add_key(&mut h);
    assert_eq!(snapshot(&h), before);
    click(&mut h, slot.x, slot.y);
    assert_eq!(h.state().shell.forms.colors.palette[0], added);
    // Add mode ends after one slot; the next click selects red instead of overwriting it.
    click(&mut h, slot.x + 2. * cell, slot.y);
    assert_eq!(h.state().app.primary_color, Color::new(255, 0, 0, 255));
    let palette = h.state().shell.forms.colors.palette.clone();
    color_add_key(&mut h);
    color_add_key(&mut h);
    assert_eq!(snapshot(&h), before);
    click(&mut h, slot.x + 3. * cell, slot.y);
    assert_eq!(h.state().app.primary_color, palette[3]);
    assert_eq!(h.state().shell.forms.colors.palette, palette);
    h.state_mut().shell.forms.colors.select_secondary(true);
    h.state_mut().app.secondary_color = added;
    color_add_key(&mut h);
    assert_eq!(snapshot(&h), before);
    click_with(
        &mut h,
        slot.x + cell,
        slot.y,
        egui::PointerButton::Secondary,
    );
    assert_eq!(h.state().shell.forms.colors.palette[1], added);
    // The selected secondary stays selected; an ordinary right click sets primary.
    assert!(h.state().shell.forms.colors.secondary_selected());
    click_with(
        &mut h,
        slot.x + 2. * cell,
        slot.y,
        egui::PointerButton::Secondary,
    );
    assert_eq!(h.state().app.primary_color, Color::new(255, 0, 0, 255));
    assert_eq!(h.state().app.secondary_color, added);
    assert_eq!(snapshot(&h), before);
}

#[test]
fn color_add_pointer_toggles_exactly_once() {
    use paintrs::theme::{COLORS_ADD_ITEM, COLORS_PALETTE, COLORS_STRIP};
    use pdn_core::util::color::Color;
    let mut h = app_harness();
    let add = colors_control_point(
        &h,
        COLORS_STRIP.0 + COLORS_ADD_ITEM / 2.,
        COLORS_STRIP.1 + COLORS_ADD_ITEM / 2.,
    );
    let (x, y, _, cell) = COLORS_PALETTE;
    let slot = colors_control_point(&h, x + cell / 2., y + cell / 2.);
    let added = Color::new(1, 2, 3, 4);
    h.state_mut().app.primary_color = added;
    click(&mut h, add.x, add.y);
    click(&mut h, slot.x, slot.y);
    assert_eq!(h.state().shell.forms.colors.palette[0], added);
    click(&mut h, slot.x + 2. * cell, slot.y);
    assert_eq!(h.state().app.primary_color, Color::new(255, 0, 0, 255));
    click(&mut h, add.x, add.y);
    click(&mut h, add.x, add.y);
    let palette = h.state().shell.forms.colors.palette.clone();
    click(&mut h, slot.x + 3. * cell, slot.y);
    assert_eq!(h.state().shell.forms.colors.palette, palette);
    assert_eq!(h.state().app.primary_color, palette[3]);
}

#[test]
fn color_add_action_without_document_preserves_disabled_pointer() {
    use paintrs::theme::{COLORS_ADD_ITEM, COLORS_PALETTE, COLORS_STRIP};
    use pdn_core::util::{color::Color, shortcut::ShortcutKey};
    for toggles in [1, 2] {
        let mut h = app_harness();
        h.state_mut()
            .shortcuts
            .set_shortcut("colorAdd", Some(ShortcutKey::from_combo("Ctrl+Alt+F12")));
        assert!(h.state_mut().app.close_document_workspace(0));
        h.run();
        let add = colors_control_point(
            &h,
            COLORS_STRIP.0 + COLORS_ADD_ITEM / 2.,
            COLORS_STRIP.1 + COLORS_ADD_ITEM / 2.,
        );
        let added = Color::new(4, 3, 2, 1);
        h.state_mut().app.primary_color = added;
        for _ in 0..toggles {
            color_add_key(&mut h);
        }
        // A disabled pointer click must not cancel or enter the mode selected by the action.
        click(&mut h, add.x, add.y);
        assert!(h.state().app.active_ws().is_none());
        h.state_mut()
            .app
            .create_blank_document_in_new_workspace(3, 2, 96.0, true);
        h.run();
        let (x, y, _, cell) = COLORS_PALETTE;
        let slot = colors_control_point(&h, x + cell / 2., y + cell / 2.);
        let previous = h.state().shell.forms.colors.palette[0];
        click(&mut h, slot.x, slot.y);
        if toggles == 1 {
            assert_eq!(h.state().shell.forms.colors.palette[0], added);
            assert_eq!(h.state().app.primary_color, added);
        } else {
            assert_eq!(h.state().shell.forms.colors.palette[0], previous);
            assert_eq!(h.state().app.primary_color, previous);
        }
    }
}

/// Plan BB1 (BB common.md §5.11): the shell syncs the Colors window's palette into the app
/// every frame, for the plugin effects.
#[test]
fn colors_palette_reaches_the_effects() {
    let mut harness = app_harness();
    let palette = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.current_palette.clone();
    assert_eq!(
        (palette(&harness).len(), palette(&harness)[2]),
        (96, [255, 0, 0, 255])
    );
    harness.state_mut().shell.forms.colors.palette[2] =
        pdn_core::util::color::Color::new(1, 2, 3, 4);
    harness.run();
    assert_eq!(palette(&harness)[2], [1, 2, 3, 4]);
}

#[test]
fn view_menu_toggles_rulers_and_persists_it() {
    let mut harness = app_harness();
    assert!(!harness.state().app.rulers_visible);
    // View (third #mainMenu item), then its Rulers entry (eighth row).
    click(&mut harness, 100.0, 21.0);
    click(&mut harness, 150.0, 207.0);
    let app = harness.state();
    assert!(app.app.rulers_visible);
    let stored = app.prefs.get(pdn_core::settings::STORAGE_KEY).unwrap();
    assert!(stored.contains(r#""showRulers":true"#), "{stored}");
}

/// Task 28 fix I3: natively the clipboard shortcuts are plain keys (egui-winit's translation is
/// off), so they reach the action map even with an empty clipboard; a focused text field gets
/// them back as egui's clipboard events, the paste text from the system clipboard.
#[test]
fn clipboard_shortcuts_are_keys_and_text_fields_keep_them() {
    use egui::{Event, Key, Modifiers};
    let mut harness = app_harness();
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::N);
    harness.run();
    assert_eq!(layers(&harness), 2);
    // Ctrl+Shift+Delete (Windows' Cut command when translated) is Delete Layer.
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::Delete);
    harness.run();
    assert_eq!(layers(&harness), 1);
    // Ctrl+V with an empty system clipboard: the "no image" message.
    harness.state_mut().shell.clipboard = paintrs::platform::Clipboard::in_memory();
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    harness.key_press(Key::Escape);
    harness.run();
    // Outside a text field the hook leaves keys alone.
    let ctx = harness.ctx.clone();
    let ctrl_v = || egui::RawInput {
        events: vec![Event::Key {
            key: Key::V,
            physical_key: None,
            pressed: true,
            repeat: false,
            // What egui-winit reports for Ctrl off macOS.
            modifiers: Modifiers::CTRL | Modifiers::COMMAND,
        }],
        ..Default::default()
    };
    let mut raw = ctrl_v();
    harness.state_mut().text_clipboard_input(&ctx, &mut raw);
    assert!(matches!(raw.events[..], [Event::Key { key: Key::V, .. }]));
    // In the status bar's zoom field Ctrl+V pastes the clipboard's text; with no text (an image
    // on the clipboard) it sends nothing, so a selection in the field is not cleared.
    click(&mut harness, 1022.0, 784.0);
    assert!(harness.state().shell.text_input_focused());
    let image = pdn_core::canvas::ImageData::new(1, 1);
    harness.state_mut().shell.clipboard.write_image(&image);
    let mut raw = ctrl_v();
    harness.state_mut().text_clipboard_input(&ctx, &mut raw);
    assert!(raw.events.is_empty());
    harness.state_mut().shell.clipboard.write_text("300");
    let mut raw = ctrl_v();
    harness.state_mut().text_clipboard_input(&ctx, &mut raw);
    assert_eq!(raw.events, [Event::Paste("300".into())]);
    for e in raw.events {
        harness.event(e);
    }
    harness.key_press(Key::Enter);
    harness.run();
    let zoom = harness.state().app.active_ws().unwrap().view.zoom;
    assert_eq!(zoom, 3.0);
}

/// Task 28 fix I1 (selection-and-move.md §2.11 steps 2–6): with a system clipboard its answer is
/// final — our own image pastes with its geometry, another app's text leaves no stale in-app
/// image or outline to paste, another app's image replaces the in-app item.
#[test]
fn system_clipboard_answers_are_final() {
    use egui::{Key, Modifiers};
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    harness.state_mut().shell.clipboard = paintrs::platform::Clipboard::in_memory();
    let ctrl_alt_shift = Modifiers::CTRL | Modifiers::ALT | Modifiers::SHIFT;
    let selection_empty = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().selection.is_empty()
    };
    harness.key_press_modifiers(Modifiers::CTRL, Key::A);
    harness.key_press_modifiers(Modifiers::CTRL, Key::C);
    harness.key_press_modifiers(ctrl_alt_shift, Key::C);
    harness.run();
    // Copy Selection replaced the image with the outline's JSON; Copy puts the image back.
    harness.key_press_modifiers(Modifiers::CTRL, Key::C);
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    let app = &harness.state().app;
    assert_eq!(app.tool_type, Some(ToolType::Move));
    let names: Vec<_> = app
        .active_ws()
        .unwrap()
        .history
        .undo
        .iter()
        .map(|m| m.name.clone())
        .collect();
    assert_eq!(names.last().unwrap(), "Paste");
    harness.key_press(Key::Enter);
    harness.run();
    assert!(selection_empty(&harness));
    // Another app's text: Paste and Paste Selection report, nothing stale is used.
    harness.state_mut().shell.clipboard.write_text("unrelated");
    harness.key_press_modifiers(ctrl_alt_shift, Key::V);
    harness.run();
    assert!(selection_empty(&harness));
    assert!(harness.state().shell.dialogs.is_open());
    harness.key_press(Key::Escape);
    harness.run();
    let n = harness.state().app.active_ws().unwrap().history.undo.len();
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert_eq!(
        harness.state().app.active_ws().unwrap().history.undo.len(),
        n
    );
    harness.key_press(Key::Escape);
    harness.run();
    // Another app's image replaces the in-app item and pastes at (0, 0).
    let red = pdn_core::canvas::ImageData {
        width: 3,
        height: 2,
        data: [255, 0, 0, 255].repeat(6),
    };
    harness.state_mut().shell.clipboard.write_image(&red);
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    let item = harness.state().app.clipboard.item.clone().unwrap();
    assert_eq!(*item.masked, red);
    assert_eq!(item.bounds, pdn_core::pixel::IntRect::new(0, 0, 3, 2));
    assert!(!selection_empty(&harness));
}

#[test]
fn tool_chooser_and_options_update_and_persist_tool_settings() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    // ToolSelector's default tool.
    assert_eq!(harness.state().app.tool_type, Some(ToolType::PaintBrush));
    // The selector (left of the strip), then the chooser's first entry: Rectangle Select.
    click(&mut harness, 37.0, 71.0);
    click(&mut harness, 100.0, 97.0);
    assert_eq!(
        harness.state().app.tool_type,
        Some(ToolType::RectangleSelect)
    );
    // The split selection-mode dropdown's main part cycles normal -> fixedSize, which persists.
    click(&mut harness, 240.0, 71.0);
    let app = harness.state();
    assert_eq!(
        app.app
            .tool_settings
            .get(ToolType::RectangleSelect, "selectionMode"),
        "fixedSize"
    );
    let stored = app
        .prefs
        .get("paintdotjs.toolSettings.rectangleSelectTool")
        .unwrap();
    assert!(
        stored.contains(r#""selectionMode":"fixedSize""#),
        "{stored}"
    );
}

/// User (2026-10-24): an on/off tool option is a checkbox. A click on its name flips the setting
/// (the same JSON bool), a click on its box flips it back.
#[test]
fn a_tool_option_checkbox_flips_on_each_click() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    let tool = ToolType::CloneStamp;
    harness.state_mut().app.set_active_tool_from_type(tool);
    harness.run();
    let aligned =
        |h: &Harness<'_, paintrs::PaintApp>| h.state().app.tool_settings.get(tool, "aligned");
    assert_eq!(aligned(&harness), true);
    let r = harness
        .state()
        .shell
        .tool_menu
        .anchor("aligned")
        .expect("Aligned");
    click(&mut harness, r.max.x - 8.0, r.center().y);
    assert_eq!(aligned(&harness), false);
    click(&mut harness, r.min.x + 10.0, r.center().y);
    assert_eq!(aligned(&harness), true);
}

#[test]
fn tool_chooser_picks_without_a_document() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    assert!(harness.state_mut().app.close_document_workspace(0));
    harness.run();
    assert!(harness.state().app.active_ws().is_none());
    click(&mut harness, 37.0, 71.0);
    click(&mut harness, 100.0, 97.0);
    assert_eq!(
        harness.state().app.tool_type,
        Some(ToolType::RectangleSelect)
    );
}

#[test]
fn font_picker_typeahead_owns_the_keyboard() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    harness.key_press(egui::Key::T);
    harness.run();
    assert_eq!(harness.state().app.tool_type, Some(ToolType::Text));
    // Open the font dropdown, type "mo" (M and O are tool hotkeys), then Enter.
    click(&mut harness, 190.0, 71.0);
    harness.event(egui::Event::Text("mo".into()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    let app = &harness.state().app;
    assert_eq!(app.tool_type, Some(ToolType::Text));
    assert_eq!(
        app.tool_settings.get(ToolType::Text, "fontFamily"),
        "Montserrat"
    );
}

#[test]
fn size_combo_input_edits_steps_and_opens_presets() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    let width = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .tool_settings
            .get(ToolType::PaintBrush, "width")
            .as_f64()
    };
    assert_eq!(width(&harness), Some(2.0));
    // Focus the Brush size input, type "5" after the "2" (`oninput` applies 25), then Enter.
    click(&mut harness, 185.0, 71.0);
    harness.event(egui::Event::Text("5".into()));
    harness.run();
    assert_eq!(width(&harness), Some(25.0));
    harness.key_press(egui::Key::Enter);
    harness.run();
    // ArrowUp steps from the entry; Alt+ArrowDown opens the presets, Escape closes them.
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(width(&harness), Some(26.0));
    harness.key_press_modifiers(egui::Modifiers::ALT, egui::Key::ArrowDown);
    harness.run();
    assert!(harness.state().shell.tool_menu.popup.is_some());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().shell.tool_menu.popup.is_none());
    assert_eq!(harness.state().app.tool_type, Some(ToolType::PaintBrush));
}

/// Task 19b: Rectangle Select's fixed-size Width and Height are Paint.NET's text boxes (no spin
/// buttons; ArrowUp still steps the focused one) round the swap button.
#[test]
fn fixed_size_boxes_step_and_swap() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    // Rectangle Select (Tools form), selection mode cycled to fixed size.
    click(&mut harness, 27.0, 143.0);
    click(&mut harness, 240.0, 71.0);
    let get = |h: &Harness<'_, paintrs::PaintApp>, key| {
        h.state()
            .app
            .tool_settings
            .get(ToolType::RectangleSelect, key)
            .as_f64()
            .unwrap()
    };
    let (w0, h0) = (
        get(&harness, "selectionWidth"),
        get(&harness, "selectionHeight"),
    );
    // The Width box (50×24 at x 341): focus it, ArrowUp steps once.
    click(&mut harness, 366.0, 71.0);
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(get(&harness, "selectionWidth"), w0 + 1.0);
    // The swap button (23×23 at x 391) exchanges them.
    click(&mut harness, 402.0, 71.0);
    assert_eq!(get(&harness, "selectionWidth"), h0);
    assert_eq!(get(&harness, "selectionHeight"), w0 + 1.0);
}

#[test]
fn font_picker_lets_modified_keys_reach_shortcuts() {
    let mut harness = app_harness();
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    assert_eq!(layers(&harness), 2);
    harness.key_press(egui::Key::T);
    harness.run();
    click(&mut harness, 190.0, 71.0);
    assert!(harness.state().shell.tool_menu.popup.is_some());
    // Ctrl+Z is not the picker's: Undo runs.
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.run();
    assert_eq!(layers(&harness), 1);
}

/// Review Focus 1: a running app moved to another scale renders exactly like one started there.
#[test]
fn scale_change_matches_fresh_render() {
    let build = |ppp: f32| {
        Harness::builder()
            .with_max_steps(16)
            .with_size(egui::vec2(1280.0, 800.0))
            .with_pixels_per_point(ppp)
            .with_render_cursor(false)
            .wgpu()
            .build_eframe(new_web_chrome_app)
    };
    let mut moved = build(1.0);
    moved.run_steps(3);
    // A monitor change arrives as a new native scale (eframe/winit), the window keeping its size in
    // points; kittest's `set_pixels_per_point` would set egui's zoom factor instead.
    moved
        .input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(1.5);
    moved.run_steps(4);
    let mut fresh = build(1.5);
    fresh.run_steps(4);
    let (a, b) = (
        moved.render().expect("render"),
        fresh.render().expect("render"),
    );
    assert_eq!(a.dimensions(), (1920, 1200));
    // egui keeps the old scale's glyphs in its font atlas, so the moved app's glyphs sit at other
    // atlas positions and a glyph-edge texel can sample its neighbour one level differently; a
    // stale cache (wrong size, position or scale) differs by far more.
    let differing: Vec<_> = a
        .enumerate_pixels()
        .zip(b.pixels())
        .filter(|((_, _, p), q)| p.0.iter().zip(q.0).any(|(c, d)| c.abs_diff(d) > 1))
        .map(|((x, y, p), q)| (x, y, p.0, q.0))
        .collect();
    assert_eq!(
        differing.len(),
        0,
        "stale caches after a scale change: {differing:?}"
    );
}

/// R17: a brush width set with the Paintbrush is the Eraser's too, and both tools' JSON persist it.
#[test]
fn brush_width_is_shared_between_tools() {
    let mut harness = app_harness();
    let app = &mut harness.state_mut().app;
    app.tool_settings.set(
        pdn_core::tool::types::ToolType::PaintBrush,
        "width",
        serde_json::json!(13),
    );
    harness.run();
    let prefs = &harness.state().prefs;
    for id in ["paintBrushTool", "eraserTool", "shapesTool"] {
        let json: serde_json::Value =
            serde_json::from_str(prefs.get(&format!("paintdotjs.toolSettings.{id}")).unwrap())
                .unwrap();
        assert_eq!(json["width"], 13, "{id}");
    }
}

/// Task 17h ruling: the first load runs the shared-settings migration and sets its marker.
#[test]
fn shared_migration_marker_is_set_after_first_load() {
    let harness = app_harness();
    assert_eq!(
        harness
            .state()
            .prefs
            .get(pdn_core::tool::types::SHARED_MIGRATION_KEY),
        Some("1")
    );
}

/// Task 17h ruling: after the one-time migration an explicit width of 2 survives a restart at
/// ppp 1.5; rule B alone would read it as paintdotjs's untouched default and load trunc(2·1.5).
#[test]
fn explicit_width_survives_a_restart_at_ppp_1_5() {
    use pdn_core::tool::types::{SHARED_MIGRATION_KEY, ToolType};
    let mut harness = app_harness();
    let tools = &mut harness.state_mut().app.tool_settings;
    tools.set(ToolType::PaintBrush, "width", serde_json::json!(2));
    harness.run();
    let prefs = &harness.state().prefs.map;
    let restart = |keep_marker: bool| {
        let seed = prefs
            .iter()
            .filter(|(k, _)| keep_marker || k.as_str() != SHARED_MIGRATION_KEY)
            .map(|(k, v)| (k.clone(), serde_json::json!(v)))
            .collect();
        let ctx = egui::Context::default();
        ctx.input_mut(|i| i.pixels_per_point = 1.5);
        let app = paintrs::PaintApp::new_for_test(&ctx, seed);
        app.app.tool_settings.get(ToolType::Eraser, "width")
    };
    assert_eq!(restart(true), serde_json::json!(2));
    assert_eq!(restart(false), serde_json::json!(3));
}

/// Task 17h review: eframe's web runner can `save()` before the first `ui`, so the migrated
/// values must already be in prefs when the marker is.
#[test]
fn migrated_values_reach_prefs_before_the_first_frame() {
    let seed = serde_json::json!({"paintdotjs.toolSettings.recolorTool": r#"{"tolerance":25}"#});
    let app = paintrs::PaintApp::new_for_test(
        &egui::Context::default(),
        seed.as_object().unwrap().clone(),
    );
    let recolor: serde_json::Value = serde_json::from_str(
        app.prefs
            .get("paintdotjs.toolSettings.recolorTool")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(recolor["tolerance"], 50);
    assert_eq!(
        app.prefs.get(pdn_core::tool::types::SHARED_MIGRATION_KEY),
        Some("1")
    );
}

/// Review fix (Task 17e): two same-size documents at the same revision and view are different
/// pictures; switching between them must re-render (the render key carries the document id).
#[test]
fn switching_documents_shows_the_other_document() {
    let mut harness = app_harness();
    {
        let app = &mut harness.state_mut().app;
        // The untouched initial document closes when the first one opens.
        let a = app.create_blank_document_in_new_workspace(800, 600, 96.0, false);
        let b = app.create_blank_document_in_new_workspace(800, 600, 96.0, false);
        let red = pdn_core::pixel::IntRect::new(0, 0, 800, 600);
        let ws = &mut app.documents[b];
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        layer.surface.fill_rect(red, [255, 0, 0, 255]);
        ws.invalidate(None);
        app.documents[a].invalidate(None);
        let (wa, wb) = (&app.documents[a].view, &app.documents[b].view);
        assert_eq!(wa.composition_revision, wb.composition_revision);
        assert_eq!(wa.view_transform(), wb.view_transform());
        app.set_active_document_workspace(Some(a));
    }
    let centre = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.run();
        h.render().expect("render").get_pixel(640, 400).0
    };
    assert_eq!(centre(&mut harness), [255, 255, 255, 255]);
    let b = harness.state().app.documents.len() - 1;
    harness
        .state_mut()
        .app
        .set_active_document_workspace(Some(b));
    assert_eq!(centre(&mut harness), [255, 0, 0, 255]);
    harness
        .state_mut()
        .app
        .set_active_document_workspace(Some(b - 1));
    assert_eq!(centre(&mut harness), [255, 255, 255, 255]);
}

/// The document strip (web layout: the item's border box is 387..461 × 3..75, the list button
/// 461..480; the context menu opens at the pointer, its Close entry at y 178..202 as in the
/// docstrip-context-menu reference).
#[test]
fn document_strip_closes_clean_documents_and_toggles_the_list() {
    // A dirty document stays open from its close button: Task 45 asks first.
    let mut harness = app_harness();
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    // The 54-DIP item's close button (Task 19b), 14×14 at (426, 6).
    click(&mut harness, 433.0, 13.0);
    assert_eq!(harness.state().app.documents.len(), 1);
    // A clean one closes from its close button and from its context menu.
    let mut harness = app_harness();
    click(&mut harness, 433.0, 13.0);
    assert!(harness.state().app.documents.is_empty());
    let mut harness = app_harness();
    let pos = egui::pos2(424.0, 39.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
    click(&mut harness, 520.0, 190.0);
    assert!(harness.state().app.documents.is_empty());
    // The list button opens the document list; Escape hides it.
    let mut harness = app_harness();
    click(&mut harness, 452.0, 30.0);
    assert!(harness.state().shell.document_strip.list_open);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.document_strip.list_open);
}

/// A primary-button drag from `from` to `to` in `steps` pointer moves.
fn drag(
    harness: &mut Harness<'_, paintrs::PaintApp>,
    from: egui::Pos2,
    to: egui::Pos2,
    steps: u32,
) {
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(from));
    harness.run();
    harness.event(button(from, true));
    harness.run();
    for i in 1..=steps {
        let p = from + (to - from) * (i as f32 / steps as f32);
        harness.event(egui::Event::PointerMoved(p));
        harness.run();
    }
    harness.event(button(to, false));
    harness.run();
}

/// Task 19: F5 (the Tools toggle) closes the Tools form and saves `windows.mainToolBarForm`;
/// a second F5 reopens it; the close button closes the History form. With the Extra tools shown
/// (S§4.1) the form is three button rows taller than Paint.NET's 331.
#[test]
fn forms_toggle_close_and_persist() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let open = |h: &Harness<'_, paintrs::PaintApp>, f| h.state().shell.forms.window(f).visible;
    assert!(open(&harness, FormId::Tools) && open(&harness, FormId::History));
    harness.key_press(egui::Key::F5);
    harness.run();
    assert!(!open(&harness, FormId::Tools));
    let stored = harness
        .state()
        .prefs
        .get(pdn_core::settings::STORAGE_KEY)
        .unwrap()
        .to_string();
    assert!(stored.contains(r#""mainToolBarForm":{"height":421.0,"snap":{"dx":5.6,"dy":5.6,"h":"top","v":"left"},"visible":false,"width":64.0,"x":6.0,"y":91.0}"#), "{stored}");
    harness.key_press(egui::Key::F5);
    harness.run();
    assert!(open(&harness, FormId::Tools));
    // The History form's close button (24×24 at 1233, 96).
    click(&mut harness, 1245.0, 108.0);
    assert!(!open(&harness, FormId::History));
}

/// Task 19: a Tools form button picks its tool; a History row steps the history there.
#[test]
fn tools_and_history_forms_act() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    click(&mut harness, 27.0, 143.0);
    assert_eq!(
        harness.state().app.tool_type,
        Some(ToolType::RectangleSelect)
    );
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    assert_eq!(layers(&harness), 2);
    // "New Image" (first row): undo to it; then the redo row "New Layer": redo.
    click(&mut harness, 1156.0, 138.0);
    assert_eq!(layers(&harness), 1);
    click(&mut harness, 1156.0, 158.0);
    assert_eq!(layers(&harness), 2);
}

/// Task 19: a Layers row selects its layer, its checkbox hides it (no history entry), and a drag
/// swaps two layers (SwapLayerFunction, one history entry).
#[test]
fn layers_form_selects_hides_and_reorders() {
    let mut harness = app_harness();
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    let ws = |h: &Harness<'_, paintrs::PaintApp>| {
        let ws = h.state().app.active_ws().unwrap();
        let names: Vec<String> = ws
            .document
            .layers
            .iter()
            .map(|l| l.properties.name.clone())
            .collect();
        (
            ws.active_layer,
            names,
            ws.document.layers.get_at(0).unwrap().properties.visible,
            ws.history.undo.len(),
        )
    };
    assert_eq!(ws(&harness).0, 1);
    // Rows top first, 62 px apart from y 499: Layer 2, then Background.
    click(&mut harness, 1156.0, 592.0);
    assert_eq!(ws(&harness).0, 0);
    click(&mut harness, 1243.0, 592.0);
    assert!(!ws(&harness).2, "the Background is hidden");
    assert_eq!(ws(&harness).3, 2, "no history entry");
    drag(
        &mut harness,
        egui::pos2(1156.0, 592.0),
        egui::pos2(1156.0, 520.0),
        8,
    );
    let (active, names, _, undo) = ws(&harness);
    assert_eq!(names, ["Layer 2", "Background"]);
    assert_eq!((active, undo), (1, 3), "the dragged layer stays active");
}

/// Task 19 review: a press-and-move on a drop-menu entry over a form's title bar drags nothing
/// (the popup body takes drags as well as clicks).
#[test]
fn a_drag_on_an_open_menu_does_not_move_the_form_below() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let tools =
        |h: &Harness<'_, paintrs::PaintApp>| h.state().shell.forms.window(FormId::Tools).rect();
    let before = tools(&harness);
    // File, then a press on its popup over the Tools title bar (y 93..123), moved 10 px.
    click(&mut harness, 20.0, 21.0);
    drag(
        &mut harness,
        egui::pos2(40.0, 113.0),
        egui::pos2(50.0, 113.0),
        2,
    );
    assert_eq!(tools(&harness), before);
}

/// Pre-merge review: Escape closes an open drop menu, as it closes the document context menu
/// (File, and the tool chooser).
#[test]
fn escape_closes_an_open_menu() {
    let mut harness = app_harness();
    for (x, y) in [(20.0, 21.0), (37.0, 71.0)] {
        click(&mut harness, x, y);
        assert!(harness.state().shell.menu_open());
        harness.key_press(egui::Key::Escape);
        harness.run();
        assert!(!harness.state().shell.menu_open());
    }
}

/// The Escape that closes a menu does not also reach the tool: a text edit survives it (the next
/// letter still types), and the next Escape ends the edit (a letter then types nothing).
#[test]
fn escape_on_a_menu_stays_off_the_tool() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    let app = &mut harness.state_mut().app;
    app.tool_settings
        .set(ToolType::Text, "editableText", false.into());
    app.set_active_tool_from_type(ToolType::Text);
    harness.run();
    click(&mut harness, 640.0, 400.0);
    let typed = |h: &mut Harness<'_, paintrs::PaintApp>, key, text: &str| {
        let before = canvas_bytes(h);
        h.event(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        h.event(egui::Event::Text(text.into()));
        h.run();
        canvas_bytes(h) != before
    };
    assert!(typed(&mut harness, egui::Key::A, "a"), "the edit starts");
    click(&mut harness, 20.0, 21.0);
    assert!(harness.state().shell.menu_open());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.menu_open());
    assert!(
        typed(&mut harness, egui::Key::B, "b"),
        "the menu's Escape reached the Text tool"
    );
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(
        !typed(&mut harness, egui::Key::C, "c"),
        "Escape ends the edit"
    );
}

/// User (2026-10-24): a click on the open menu's own item closes it, and a press anywhere off
/// the menu closes it at once, any button (the title bar's press never becomes a click).
#[test]
fn a_menu_closes_on_its_item_and_on_any_press_off_it() {
    let mut harness = app_harness();
    let open = |h: &Harness<'_, paintrs::PaintApp>| h.state().shell.menu_open();
    // A held press on the canvas keeps a stroke repainting: step frames instead of settling.
    let button = |h: &mut Harness<'_, paintrs::PaintApp>, button, pressed| {
        h.event(egui::Event::PointerButton {
            pos: egui::pos2(640.0, 400.0),
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run_steps(2);
    };
    // File, then File again.
    click(&mut harness, 20.0, 21.0);
    assert!(open(&harness));
    click(&mut harness, 20.0, 21.0);
    assert!(!open(&harness));
    // A press on the canvas, either button, closes it before the release.
    for b in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
        click(&mut harness, 20.0, 21.0);
        assert!(open(&harness));
        button(&mut harness, b, true);
        assert!(!open(&harness));
        button(&mut harness, b, false);
        harness.run();
    }
    // With File open, moving onto View opens View; a click on it then closes it.
    click(&mut harness, 20.0, 21.0);
    harness.event(egui::Event::PointerMoved(egui::pos2(100.0, 21.0)));
    harness.run();
    assert!(open(&harness));
    click(&mut harness, 100.0, 21.0);
    assert!(!open(&harness));
}

/// Review M1: a touch moves and presses in one frame and releases in the next. With File open,
/// a tap on View opens View (its press closes File, its click opens View), not nothing.
#[test]
fn a_tap_on_another_main_menu_opens_it() {
    let mut harness = app_harness();
    // `Harness::event` runs a frame per event: the move and the press go in one frame's input.
    let tap = |h: &mut Harness<'_, paintrs::PaintApp>, pos| {
        let button = |pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        h.input_mut()
            .events
            .extend([egui::Event::PointerMoved(pos), button(true)]);
        h.run();
        h.event(button(false));
        h.run();
    };
    click(&mut harness, 20.0, 21.0);
    assert!(harness.state().shell.menu_open());
    tap(&mut harness, egui::pos2(100.0, 21.0));
    assert!(harness.state().shell.menu_open());
    // A tap on View, the open one, closes it.
    tap(&mut harness, egui::pos2(100.0, 21.0));
    assert!(!harness.state().shell.menu_open());
}

/// Task 19 review: the Layers list scrolls 0.4 × the wheel delta while the pointer is over it (6
/// rows of 62 overflow the 218-px list): 120 px down puts the second row under y 530.
#[test]
fn the_layers_list_scrolls_under_the_wheel() {
    let mut harness = app_harness();
    for _ in 0..5 {
        harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
        harness.run();
    }
    let at = egui::pos2(1156.0, 530.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.run();
    harness.event(egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Point,
        source: egui::MouseWheelSource::Wheel,
        delta: egui::vec2(0.0, -120.0),
        phase: egui::TouchPhase::Move,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
    click(&mut harness, at.x, at.y);
    assert_eq!(harness.state().app.active_ws().unwrap().active_layer, 4);
}

/// Task 19 review: a form drag saves `windows.<id>` when it ends (the document `mouseup`), not
/// while it moves; Task 20b: as its position and snap description (`floating-windows.md` §5.4).
#[test]
fn a_form_drag_saves_its_state_on_release() {
    let mut harness = app_harness();
    let stored = |h: &Harness<'_, paintrs::PaintApp>| {
        let s = h
            .state()
            .prefs
            .get(pdn_core::settings::STORAGE_KEY)
            .unwrap();
        serde_json::from_str::<serde_json::Value>(s).unwrap()["windows"]["historyForm"].clone()
    };
    let before = stored(&harness);
    assert_eq!(
        before["snap"],
        serde_json::json!({"h": "top", "v": "right", "dx": 5.6, "dy": 5.6})
    );
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let (from, to) = (egui::pos2(1156.0, 113.0), egui::pos2(856.0, 243.0));
    harness.event(egui::Event::PointerMoved(from));
    harness.run();
    harness.event(button(from, true));
    harness.run();
    harness.event(egui::Event::PointerMoved(to));
    harness.run();
    assert_eq!(stored(&harness), before, "not while dragging");
    harness.event(button(to, false));
    harness.run();
    // Moved by (−300, 130) to (757, 221): no W edge near, and the Layers form below spans other
    // columns (no overlap, no snap): an absolute position.
    let after = stored(&harness);
    assert_eq!((&after["x"], &after["y"]), (&757.0.into(), &221.0.into()));
    assert_eq!(after["snap"], serde_json::Value::Null);
    assert_eq!(after.get("anchorX"), None);
}

/// Task 20: More/Less resizes the Colors form (270×315 ↔ 492×371) keeping its bottom edge (the
/// bottom-left re-park, `floating-windows.md` §2.7) and
/// saves `windows.colorsForm.expanded`; the palette selector opens Paint.NET's palette menu,
/// whose Reset to Default Palette restores the palette.
#[test]
fn colors_form_more_less_and_palette_menu() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let rect =
        |h: &Harness<'_, paintrs::PaintApp>| h.state().shell.forms.window(FormId::Colors).rect();
    let expanded = |h: &Harness<'_, paintrs::PaintApp>| {
        let s = h
            .state()
            .prefs
            .get(pdn_core::settings::STORAGE_KEY)
            .unwrap();
        serde_json::from_str::<serde_json::Value>(s).unwrap()["windows"]["colorsForm"]["expanded"]
            .clone()
    };
    assert_eq!(
        rect(&harness),
        egui::Rect::from_min_size(egui::pos2(6.0, 430.0), egui::vec2(272.0, 317.0))
    );
    // `#moreLessButton` (92×27 at 167, 477).
    click(&mut harness, 213.0, 490.0);
    assert!(harness.state().shell.forms.colors.expanded);
    assert_eq!(
        rect(&harness),
        egui::Rect::from_min_size(egui::pos2(6.0, 374.0), egui::vec2(494.0, 373.0))
    );
    assert_eq!(expanded(&harness), true);
    click(&mut harness, 213.0, 434.0);
    assert!(!harness.state().shell.forms.colors.expanded);
    assert_eq!(rect(&harness).max.y, 747.0);
    assert_eq!(expanded(&harness), false);
    // The palette selector (44×26 at 41, 667), then Reset to Default Palette (its third entry).
    harness.state_mut().shell.forms.colors.palette[0] = pdn_core::util::color::Color::WHITE;
    click(&mut harness, 63.0, 680.0);
    click(&mut harness, 145.0, 762.0);
    assert_eq!(
        harness.state().shell.forms.colors.palette,
        paintrs::forms::colors::default_palette()
    );
}

/// A press and release of `button` at (x, y).
fn click_with(
    harness: &mut Harness<'_, paintrs::PaintApp>,
    x: f32,
    y: f32,
    button: egui::PointerButton,
) {
    let pos = egui::pos2(x, y);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run();
    }
}

/// Task 29 through the pointer (`ui-drawn-graphics.md §2.3`, §2.6, §2.7): the secondary preview
/// selects the secondary; the swap icon keeps that selection; a palette left click sets the
/// selected colour and a right click the other; a left press on the wheel picks the selected
/// colour as integer HSV with V = 0 → 100, a right press the other one.
#[test]
fn colors_form_picks_through_the_pointer() {
    use egui::PointerButton::{Primary, Secondary};
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    let colors = |h: &Harness<'_, paintrs::PaintApp>| {
        let app = &h.state().app;
        (app.primary_color, app.secondary_color)
    };
    // The form's content starts at (7, 462): the secondary preview's box is (38, 501) 43×43
    // under the primary's (15, 478).
    click(&mut harness, 70.0, 535.0);
    assert!(harness.state().shell.forms.colors.secondary_selected());
    // The swap icon (65, 478) 16×16.
    click(&mut harness, 73.0, 486.0);
    assert_eq!(colors(&harness), (Color::WHITE, Color::BLACK));
    assert!(harness.state().shell.forms.colors.secondary_selected());
    // Palette swatch 2 (red) at (45, 701) and 4 (FFD800) at (75, 701), 15 px each.
    click(&mut harness, 52.0, 708.0);
    assert_eq!(colors(&harness).1, Color::new(255, 0, 0, 255));
    click_with(&mut harness, 82.0, 708.0, Secondary);
    assert_eq!(colors(&harness).0, Color::new(255, 0xD8, 0, 255));
    // §2.6 step 4: a press on swatch 0 released over swatch 1 picks nothing.
    let press = |h: &mut Harness<'_, paintrs::PaintApp>, x: f32, pressed: bool| {
        let pos = egui::pos2(x, 708.0);
        h.event(egui::Event::PointerMoved(pos));
        h.run();
        h.event(egui::Event::PointerButton {
            pos,
            button: Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        h.run();
    };
    press(&mut harness, 22.0, true);
    press(&mut harness, 37.0, false);
    assert_eq!(colors(&harness).1, Color::new(255, 0, 0, 255));
    // The wheel's box is (79, 510), L = 180: c = 89, R = 84. Back to the primary; black first.
    click(&mut harness, 30.0, 490.0);
    click(&mut harness, 23.0, 536.0);
    assert_eq!(colors(&harness), (Color::BLACK, Color::WHITE));
    // px (131, 89): H 0, S trunc(0.5 + 100·0.5^1.4) = 38; V 0 → 100.
    click_with(&mut harness, 210.0, 599.0, Primary);
    assert_eq!(colors(&harness).0, Color::new(255, 158, 158, 255));
    // A right press edits the other colour: px (89, 47) → H 270, S 38 on white's V 100.
    click_with(&mut harness, 168.0, 557.0, Secondary);
    assert_eq!(
        colors(&harness),
        (
            Color::new(255, 158, 158, 255),
            Color::new(206, 158, 255, 255)
        )
    );
}

/// §2.7 "Hex": typing applies only parsable text; leaving the box with unparsable text gives 0
/// and clears it.
#[test]
fn colors_form_hex_box_applies_on_edit_and_leave() {
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    // More (167, 477); the expanded form's content starts at (7, 406): the hex box is
    // (415, 531) 68×25.
    click(&mut harness, 213.0, 490.0);
    click(&mut harness, 449.0, 543.0);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("3399cc".into()));
    harness.run();
    assert_eq!(
        harness.state().app.primary_color,
        Color::new(0x33, 0x99, 0xCC, 255)
    );
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("zz".into()));
    harness.run();
    assert_eq!(
        harness.state().app.primary_color,
        Color::new(0x33, 0x99, 0xCC, 255)
    );
    // A press on the form's background leaves the box.
    click(&mut harness, 300.0, 700.0);
    assert_eq!(harness.state().app.primary_color, Color::BLACK);
}

/// A press and release of `button` at (x, y) arriving in one frame (a touchpad tap).
fn tap(harness: &mut Harness<'_, paintrs::PaintApp>, x: f32, y: f32, button: egui::PointerButton) {
    let pos = egui::pos2(x, y);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    // `Harness::event` runs a frame per event: both go into the next frame's raw input.
    for pressed in [true, false] {
        harness.input_mut().events.push(egui::Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
    }
    harness.run();
}

/// Task 29 review round 1, Important 2: a press and release in one frame still picks — on the
/// palette (left and right), the wheel (left and right) and a slider.
#[test]
fn colors_form_takes_taps() {
    use egui::PointerButton::{Primary, Secondary};
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    let colors = |h: &Harness<'_, paintrs::PaintApp>| {
        let app = &h.state().app;
        (app.primary_color, app.secondary_color)
    };
    tap(&mut harness, 52.0, 708.0, Primary);
    assert_eq!(colors(&harness).0, Color::new(255, 0, 0, 255));
    tap(&mut harness, 82.0, 708.0, Secondary);
    assert_eq!(colors(&harness).1, Color::new(255, 0xD8, 0, 255));
    tap(&mut harness, 210.0, 599.0, Primary);
    assert_eq!(colors(&harness).0, Color::new(255, 158, 158, 255));
    tap(&mut harness, 168.0, 557.0, Secondary);
    assert_eq!(colors(&harness).1, Color::new(206, 158, 255, 255));
    // Expanded, the R slider's strip is (320, 450) 85 px wide: column 42 → 255 − (42·255 ÷ 84)
    // = 128.
    click(&mut harness, 213.0, 490.0);
    tap(&mut harness, 362.0, 455.0, Primary);
    assert_eq!(colors(&harness).0, Color::new(128, 158, 158, 255));
}

/// Task 29 review round 1, Important 1: a palette click that takes the hex box's focus sets the
/// colour in one click (leaving the box does not re-apply its old text).
#[test]
fn a_palette_click_out_of_the_hex_box_wins() {
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    // Expanded: the hex box (415, 531), the palette's swatch 2 at (45, 645).
    click(&mut harness, 213.0, 490.0);
    click(&mut harness, 449.0, 543.0);
    click(&mut harness, 52.0, 652.0);
    assert_eq!(
        harness.state().app.primary_color,
        Color::new(255, 0, 0, 255)
    );
}

/// Task 29 review round 1, Minor 1: with no document open (the form disabled) the hex box takes
/// no keyboard.
#[test]
fn the_hex_box_is_inert_without_a_document() {
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    // The document's close button (Task 19b).
    click(&mut harness, 433.0, 13.0);
    assert!(harness.state().app.documents.is_empty());
    click(&mut harness, 213.0, 490.0);
    click(&mut harness, 449.0, 543.0);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("FF0000".into()));
    harness.run();
    assert!(!harness.ctx.text_edit_focused());
    assert_eq!(harness.state().app.primary_color, Color::BLACK);
}

/// Task 29 review round 1, Minor 3: Tab walks the expanded form's inputs as the DOM does — R, G,
/// B, Hex, H, S, V, A — passing over the sliders, wheel, palette and previews.
#[test]
fn tab_walks_the_colors_inputs() {
    use pdn_core::util::color::Color;
    let mut harness = app_harness();
    let primary = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.primary_color;
    let tab_type = |h: &mut Harness<'_, paintrs::PaintApp>, text: &str| {
        h.key_press(egui::Key::Tab);
        h.event(egui::Event::Text(text.into()));
        h.run();
    };
    click(&mut harness, 213.0, 490.0);
    // The R field (the expanded form's (408, 37) 68×25 input, less its 15-px spin column).
    click(&mut harness, 440.0, 455.0);
    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::A);
    harness.event(egui::Event::Text("10".into()));
    harness.run();
    assert_eq!(primary(&harness), Color::new(10, 0, 0, 255));
    tab_type(&mut harness, "20");
    assert_eq!(primary(&harness), Color::new(10, 20, 0, 255));
    tab_type(&mut harness, "30");
    assert_eq!(primary(&harness), Color::new(10, 20, 30, 255));
    tab_type(&mut harness, "FF0000");
    assert_eq!(primary(&harness), Color::new(255, 0, 0, 255));
    tab_type(&mut harness, "120");
    assert_eq!(primary(&harness), Color::new(0, 255, 0, 255));
    tab_type(&mut harness, "50");
    assert_eq!(primary(&harness), Color::new(127, 255, 127, 255));
    tab_type(&mut harness, "50");
    assert_eq!(primary(&harness), Color::new(63, 127, 63, 255));
    tab_type(&mut harness, "100");
    assert_eq!(primary(&harness), Color::new(63, 127, 63, 100));
}

/// Task 20b (`ui-drawn-graphics.md` Addendum, controller 2026-10-19): a form over the image is
/// 0.75 opaque unless the pointer is over it (focus plays no part). Under the pointer it turns
/// opaque within 0.15 s; once the pointer leaves it fades back over 0.5 s (kittest steps 0.25 s a
/// frame; a fade starts in the frame that sees the change).
#[test]
fn a_translucent_form_turns_opaque_under_the_pointer() {
    let mut harness = app_harness();
    // The Colors form's background over the white image (the form ends at x 278, the image
    // starts at 232).
    let shade = |h: &mut Harness<'_, paintrs::PaintApp>| h.render().unwrap().get_pixel(275, 600)[0];
    let translucent = shade(&mut harness);
    // ⌊40·191/255⌋ + ⌊255·64/255 − ½⌋ ± 1.
    assert!((92..=94).contains(&translucent), "{translucent}");
    // Over the form's plain background, no press.
    harness.event(egui::Event::PointerMoved(egui::pos2(270.0, 600.0)));
    harness.step();
    harness.step();
    assert_eq!(
        shade(&mut harness),
        40,
        "opaque 0.25 s after the pointer arrived"
    );
    // Onto the canvas surround: a quarter second into the half-second fade, then translucent.
    harness.event(egui::Event::PointerMoved(egui::pos2(600.0, 760.0)));
    harness.step();
    harness.step();
    let between = shade(&mut harness);
    assert!(40 < between && between < translucent, "{between}");
    harness.run();
    assert_eq!(shade(&mut harness), translucent);
}

/// The window's border box, by form id.
fn form_rect(h: &Harness<'_, paintrs::PaintApp>, form: paintrs::forms::FormId) -> egui::Rect {
    h.state().shell.forms.window(form).rect()
}

fn pixel(image: &image::RgbaImage, x: f32, y: f32) -> [u8; 3] {
    let p = image.get_pixel(x as u32, y as u32).0;
    [p[0], p[1], p[2]]
}

/// Task 20b I1–I2 (`floating-windows.md` §2.9, §5.6): while the app window lacks OS focus every
/// caption draws inactive — the title grey (#7F7F7F in the dark scheme), no red box behind the
/// "x" — and focus back restores them; a press in a form changes nothing.
#[test]
fn captions_draw_inactive_while_the_app_is_unfocused() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    // Opaque forms (the Colors form lies over the image).
    harness
        .state_mut()
        .settings
        .set("ui.translucentWindows", false.into());
    harness.run();
    // Near the close box's top-left corner (clear of the glyph), and the title's ink.
    let probe = |h: &mut Harness<'_, paintrs::PaintApp>, form| {
        let r = form_rect(h, form);
        let image = h.render().unwrap();
        let close = pixel(&image, r.max.x - 23.0, r.min.y + 8.0);
        let mut ink = 0;
        for y in (r.min.y + 7.0) as u32..(r.min.y + 27.0) as u32 {
            for x in (r.min.x + 6.0) as u32..(r.min.x + 40.0) as u32 {
                ink = ink.max(image.get_pixel(x, y).0[0]);
            }
        }
        (close, ink)
    };
    let forms = [
        FormId::Tools,
        FormId::History,
        FormId::Layers,
        FormId::Colors,
    ];
    let active = |(close, ink): ([u8; 3], u8)| close == [0xC7, 0x50, 0x50] && ink >= 0xE0;
    for form in forms {
        let p = probe(&mut harness, form);
        assert!(active(p), "{form:?} {p:?}");
    }
    // I2: a press in the Layers form (its title) leaves every caption active.
    let layers = form_rect(&harness, FormId::Layers);
    click(&mut harness, layers.min.x + 60.0, layers.min.y + 16.0);
    for form in forms {
        assert_eq!(probe(&mut harness, form).0, [0xC7, 0x50, 0x50], "{form:?}");
    }
    // I1: focus lost.
    harness.input_mut().focused = false;
    harness.run();
    for form in forms {
        let (close, ink) = probe(&mut harness, form);
        assert_eq!(close, [0x0D, 0x0D, 0x0D], "{form:?}");
        assert!((0x60..=0x7F).contains(&ink), "{form:?} {ink:#x}");
    }
    harness.input_mut().focused = true;
    harness.run();
    for form in forms {
        let p = probe(&mut harness, form);
        assert!(active(p), "{form:?} {p:?}");
    }
}

/// A 1024×632 app (P2: the History and Layers forms overlap).
fn small_harness() -> Harness<'static, paintrs::PaintApp> {
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1024.0, 632.0))
        .with_pixels_per_point(1.0)
        .with_render_cursor(false)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    harness.run();
    harness
}

/// Task 20b Z1–Z3 (`floating-windows.md` §2.2): the last-shown form is on top (Layers over
/// History in a small window); a press anywhere in a form raises it, a press on the canvas
/// changes nothing.
#[test]
fn a_press_raises_a_form() {
    use paintrs::forms::FormId;
    let mut harness = small_harness();
    // Keep the image away from the forms, which turn translucent over it (§2.4).
    for _ in 0..3 {
        harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Minus);
        harness.run();
    }
    let history = form_rect(&harness, FormId::History);
    let layers = form_rect(&harness, FormId::Layers);
    assert!(history.intersects(layers), "{history:?} {layers:?}");
    // In the overlap, on Layers' title bar.
    let probe = egui::pos2(layers.min.x + 60.0, layers.min.y + 10.0);
    assert!(history.contains(probe));
    let title_bar = [0x0D, 0x0D, 0x0D];
    let at = |h: &mut Harness<'_, paintrs::PaintApp>| pixel(&h.render().unwrap(), probe.x, probe.y);
    assert_eq!(at(&mut harness), title_bar, "Layers over History");
    // History's title: History comes to the top.
    click(&mut harness, history.min.x + 60.0, history.min.y + 16.0);
    assert_ne!(at(&mut harness), title_bar, "History over Layers");
    // A press in the overlap goes to the form on top there: History stays on top.
    click(&mut harness, probe.x, probe.y);
    assert_ne!(
        at(&mut harness),
        title_bar,
        "the overlap press raised Layers"
    );
    // The canvas surround: no change.
    click(&mut harness, 150.0, 150.0);
    assert_ne!(at(&mut harness), title_bar);
    // A press in Layers' content raises it again.
    click(&mut harness, layers.min.x + 60.0, layers.max.y - 40.0);
    assert_eq!(at(&mut harness), title_bar);
    // Now the overlap press goes to Layers: it stays on top.
    click(&mut harness, probe.x, probe.y);
    assert_eq!(
        at(&mut harness),
        title_bar,
        "the overlap press raised History"
    );
}

/// Task 20b (`floating-windows.md` §2.5, §2.6, §5.4): a form dragged away from every edge keeps
/// its absolute position, through a restart in a smaller window too (paintdotjs's proportional
/// anchors would move it).
#[test]
fn a_dragged_form_keeps_its_position_across_a_restart() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let history = form_rect(&harness, FormId::History);
    let grab = history.min + egui::vec2(100.0, 16.0);
    drag(&mut harness, grab, grab + egui::vec2(-500.0, 150.0), 4);
    let dropped = form_rect(&harness, FormId::History);
    assert_eq!(dropped.min, history.min + egui::vec2(-500.0, 150.0));
    let seed: serde_json::Map<String, serde_json::Value> = harness
        .state()
        .prefs
        .map
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::json!(v)))
        .collect();
    let mut restarted = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1024.0, 632.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(move |cc| paintrs::PaintApp::new_for_test(&cc.egui_ctx, seed.clone()));
    restarted.run();
    assert_eq!(form_rect(&restarted, FormId::History), dropped);
}

/// `windows.historyForm` as stored.
fn stored_history(h: &Harness<'_, paintrs::PaintApp>) -> serde_json::Value {
    let s = h
        .state()
        .prefs
        .get(pdn_core::settings::STORAGE_KEY)
        .unwrap();
    serde_json::from_str::<serde_json::Value>(s).unwrap()["windows"]["historyForm"].clone()
}

/// Task 20b review (`floating-windows.md` §2.5 step 4): an unsnapped form clamped into a smaller
/// app window comes back when the window grows again, and the stored position stays its home.
#[test]
fn a_clamped_unsnapped_form_comes_back() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let history = form_rect(&harness, FormId::History);
    let grab = history.min + egui::vec2(100.0, 16.0);
    drag(&mut harness, grab, grab + egui::vec2(-500.0, 150.0), 4);
    let home = form_rect(&harness, FormId::History);
    assert_eq!(home.min, egui::pos2(557.0, 241.0));
    // 640×400: held in the overlay by its CSS size (640 − 200, 400 − 230).
    harness.set_size(egui::vec2(640.0, 400.0));
    harness.run();
    let clamped = form_rect(&harness, FormId::History);
    assert_eq!(clamped.min, egui::pos2(440.0, 170.0));
    let saved = stored_history(&harness);
    assert_eq!((&saved["x"], &saved["y"]), (&557.0.into(), &241.0.into()));
    assert_eq!(saved["snap"], serde_json::Value::Null);
    harness.set_size(egui::vec2(1280.0, 800.0));
    harness.run();
    assert_eq!(form_rect(&harness, FormId::History), home);
}

/// Task 20b review (`floating-windows.md` §2.3 step 4, §2.5): the forms re-park whenever the
/// canvas area changes, not only on a window resize: rulers move W's top-left (Tools follows:
/// 17 + 5.6, 102 + 5.6), Fit to Window drops the scrollbars (Layers follows W's bottom-right:
/// 1280 − 5.6 − 202, 768 − 5.6 − 278).
#[test]
fn forms_follow_rulers_and_scrollbars() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    assert_eq!(
        form_rect(&harness, FormId::Tools).min,
        egui::pos2(6.0, 91.0)
    );
    assert_eq!(
        form_rect(&harness, FormId::Layers).min,
        egui::pos2(1057.0, 469.0)
    );
    let perform = |h: &mut Harness<'_, paintrs::PaintApp>, id: &str| {
        pdn_core::action::perform(&mut h.state_mut().app, id);
        h.run();
    };
    perform(&mut harness, "menu.view.rulers");
    assert!(harness.state().app.rulers_visible);
    assert_eq!(
        form_rect(&harness, FormId::Tools).min,
        egui::pos2(23.0, 108.0)
    );
    assert_eq!(
        form_rect(&harness, FormId::Layers).min,
        egui::pos2(1057.0, 469.0)
    );
    perform(&mut harness, "menu.view.zoomToWindow");
    assert_eq!(
        form_rect(&harness, FormId::Layers).min,
        egui::pos2(1072.0, 484.0)
    );
    assert_eq!(
        form_rect(&harness, FormId::Tools).min,
        egui::pos2(23.0, 108.0)
    );
}

/// Task 20b review (`floating-windows.md` §2.8): Ctrl+Shift+click on History's aux-menu toggle
/// resets the form (default size and corner, shown); a plain click toggles it.
#[test]
fn ctrl_shift_click_on_a_toggle_resets_its_form() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let home = form_rect(&harness, FormId::History);
    let visible =
        |h: &Harness<'_, paintrs::PaintApp>| h.state().shell.forms.window(FormId::History).visible;
    harness
        .state_mut()
        .shell
        .forms
        .windows
        .iter_mut()
        .find(|w| w.id == FormId::History.id())
        .unwrap()
        .size = egui::vec2(260.0, 300.0);
    harness.run();
    assert_ne!(form_rect(&harness, FormId::History), home);
    // The History toggle (22×22 at 1149, 4: the fourth icon from the right of `#settingsMenu`).
    let at = egui::pos2(1160.0, 15.0);
    let reset_click = |h: &mut Harness<'_, paintrs::PaintApp>| {
        let m = egui::Modifiers::CTRL | egui::Modifiers::SHIFT;
        h.event(egui::Event::PointerMoved(at));
        h.run();
        for pressed in [true, false] {
            let button = egui::PointerButton::Primary;
            let event = egui::Event::PointerButton {
                pos: at,
                button,
                pressed,
                modifiers: m,
            };
            h.event_modifiers(event, m);
            h.run();
        }
    };
    reset_click(&mut harness);
    assert!(visible(&harness));
    assert_eq!(form_rect(&harness, FormId::History), home);
    click(&mut harness, at.x, at.y);
    assert!(!visible(&harness), "a plain click toggles");
    reset_click(&mut harness);
    assert!(visible(&harness));
    assert_eq!(form_rect(&harness, FormId::History), home);
}

/// Task 20b Z4–Z5 (`floating-windows.md` §2.8): Ctrl+Shift+F6 returns History to its default
/// size and corner and leaves the stack alone; with History hidden it shows it (on top).
#[test]
fn ctrl_shift_f6_resets_the_history_form() {
    use paintrs::forms::FormId;
    let mut harness = app_harness();
    let home = form_rect(&harness, FormId::History);
    harness
        .state_mut()
        .shell
        .forms
        .windows
        .iter_mut()
        .find(|w| w.id == FormId::History.id())
        .unwrap()
        .size = egui::vec2(260.0, 300.0);
    harness.run();
    let reset = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.key_press_modifiers(
            egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
            egui::Key::F6,
        );
        h.run();
    };
    reset(&mut harness);
    assert_eq!(form_rect(&harness, FormId::History), home);
    harness.key_press(egui::Key::F6);
    harness.run();
    assert!(!harness.state().shell.forms.window(FormId::History).visible);
    reset(&mut harness);
    assert!(harness.state().shell.forms.window(FormId::History).visible);
    assert_eq!(form_rect(&harness, FormId::History), home);
}

/// `.horizontal-separator`'s colour (`--color-title-bar-border` at opacity .5) over `backdrop`.
fn px_blend(backdrop: egui::Color32, color: egui::Color32) -> egui::Color32 {
    paintrs::kit::px::blend(backdrop, color, paintrs::kit::px::css_alpha(0.5))
}

/// Task 19b requirement 4 (and review Important #1): every chrome border is whole device pixels
/// (Chromium and Paint.NET/WinForms both draw them so) at 125, 175 and 200 %: along each probed
/// edge the border is `border_width(1)` lines of the border colour (1 px below 200 %, 2 at it) and
/// the lines either side are the colours inside and outside it — never a partial-coverage blend,
/// an extra row or a missing one. Probes the main-menu tab, the strip item, the Brush size combo,
/// the Hardness meter, the Fill combo, the History and Layers frames and the strip and toolbar
/// separators.
#[test]
fn chrome_borders_are_whole_pixels() {
    let failures: Vec<String> = [1.25, 1.75, 2.0]
        .into_iter()
        .flat_map(chrome_border_failures)
        .collect();
    // Per probe: "ppp <s> <name>" and its failing pixel count.
    let mut groups: Vec<(String, usize)> = vec![];
    for f in &failures {
        let key = f.split(" @").next().unwrap_or(f).to_string();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, n)) => *n += 1,
            None => groups.push((key, 1)),
        }
    }
    assert!(
        failures.is_empty(),
        "{} partial or misplaced border pixels: {groups:?}\nfirst: {}",
        failures.len(),
        failures[..failures.len().min(10)].join("\n")
    );
}

fn chrome_border_failures(ppp: f32) -> Vec<String> {
    use egui::{Color32, Pos2, Rect, Vec2};
    use paintrs::shell::{ShellLayout, tool_menu};
    use paintrs::theme::{self, Scheme, Theme};
    use pdn_core::tool::types::{ToolType, option_definitions};
    // 1020 DIP wide: the Paintbrush row still reaches its Fill combo.
    let size = egui::vec2(1020.0, 800.0);
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(size)
        .with_pixels_per_point(ppp)
        .with_render_cursor(false)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    // Opaque forms (the narrower window puts them over the image); the History form's shadow would
    // lie over the Fill combo here, so the Colors form stands in for it.
    harness
        .state_mut()
        .settings
        .set("ui.translucentWindows", false.into());
    for w in harness.state_mut().shell.forms.windows.iter_mut() {
        w.visible &= w.id != "historyForm";
    }
    harness.run_steps(4);
    let image = harness.render().expect("render");
    let t = Theme::for_scheme(Scheme::Dark);
    let dev = |v: f32| (v * ppp).round() as i32;
    let snap = |r: Rect| [dev(r.min.x), dev(r.min.y), dev(r.max.x), dev(r.max.y)];
    // A CSS 1-px border and a 1-DIP line, in device px.
    let bw = (ppp.floor() as i32).max(1);
    let thin = (ppp.round() as i32).max(1);
    let mut failures = vec![];
    // One edge: `line` is the border's outermost row (`horizontal`) or column, `out` the side
    // (-1 or +1) its outside lies on, `width` its lines, `span` the probed range along it;
    // `outside` None skips that side.
    let mut edge = |name: &str,
                    horizontal: bool,
                    line: i32,
                    out: i32,
                    width: i32,
                    span: std::ops::Range<i32>,
                    [border, inside]: [Color32; 2],
                    outside: Option<Color32>| {
        for a in span.step_by(3) {
            let at = |d: i32| {
                let (x, y) = if horizontal {
                    (a, line + d)
                } else {
                    (line + d, a)
                };
                let (x, y) = (x.clamp(0, image.width() as i32 - 1), y.max(0));
                let p = image.get_pixel(x as u32, y as u32).0;
                Color32::from_rgb(p[0], p[1], p[2])
            };
            let mut want: Vec<(i32, Color32)> = (0..width).map(|k| (-out * k, border)).collect();
            want.push((-out * width, inside));
            want.extend(outside.map(|c| (out, c)));
            for (d, c) in want {
                if at(d) != c {
                    failures.push(format!(
                        "ppp {ppp} {name} @{a} line {}: {:?} != {c:?}",
                        line + d,
                        at(d)
                    ));
                }
            }
        }
    };
    let screen = Rect::from_min_size(Pos2::ZERO, size);
    // The tab fits its text as the app measures it at this scale.
    let painter = egui::Painter::new(
        harness.ctx.clone(),
        egui::LayerId::background(),
        Rect::EVERYTHING,
    );
    let menu_w = paintrs::shell::main_menu::width(&painter);
    let l = ShellLayout::compute(screen, false, menu_w, 1);
    let title = t.title_bar;
    // Main-menu tab: top, left and right borders over the title bar.
    let [x0, y0, x1, y1] = snap(l.main_menu);
    let tab = [t.title_bar_border, t.menu_background];
    edge(
        "tab top",
        true,
        y0,
        -1,
        bw,
        x0 + bw..x1 - bw,
        tab,
        Some(title),
    );
    edge("tab left", false, x0, -1, bw, y0 + bw..y1, tab, Some(title));
    edge(
        "tab right",
        false,
        x1 - 1,
        1,
        bw,
        y0 + bw..y1,
        tab,
        Some(title),
    );
    // The selected strip item (`.selected-item` border, then the menu-coloured ring).
    let item = Rect::from_min_size(
        l.document_strip.min,
        Vec2::new(l.document_strip.height() + 2.0, l.document_strip.height()),
    );
    let [x0, y0, x1, y1] = snap(item);
    let ring = [t.menu_item_selected_border, t.menu_background];
    let title_bottom = dev(l.title_bar.max.y) - bw;
    edge(
        "item top",
        true,
        y0,
        -1,
        bw,
        x0 + bw..x1 - bw,
        ring,
        Some(title),
    );
    edge(
        "item left",
        false,
        x0,
        -1,
        bw,
        y0 + bw..title_bottom,
        ring,
        Some(title),
    );
    let header = Some(t.menu_background);
    edge(
        "item bottom",
        true,
        y1 - 1,
        1,
        bw,
        x0 + bw..x1 - bw,
        ring,
        header,
    );
    // The Paintbrush's tool options, laid out as `ToolMenu::show` lays them out.
    let tool = harness.state().app.tool_type.expect("a tool");
    assert_eq!(tool, ToolType::PaintBrush);
    let settings = &harness.state().app.tool_settings;
    let defs = option_definitions(tool, settings);
    let content = Rect::from_min_max(
        Pos2::new(l.tool_menu.min.x, l.tool_menu.min.y + 1.0),
        l.tool_menu.max,
    );
    // Options start after the 68-DIP selector (1-DIP margins) and its 11-DIP separator.
    let x_options = l.tool_menu.min.x + 1.0 + 68.0 + 1.0 + 11.0;
    let row = tool_menu::layout_row(&defs, |k| settings.get(tool, k), content, x_options, ppp);
    let control = |key: &str| row.options[defs.iter().position(|d| d.key == key).unwrap()].control;
    let bar = t.pdn_toolbar_background;
    let field = [t.pdn_control_border, t.pdn_control_background];
    let b = paintrs::kit::px::border_width(1.0, ppp);
    let parts = paintrs::kit::tool_option::size_parts(control("width"), 75.0, b);
    let [x0, y0, _, y1] = snap(parts[1]);
    let arrow = snap(parts[3])[0];
    edge(
        "size combo top",
        true,
        y0,
        -1,
        bw,
        x0 + bw..arrow - 1,
        field,
        Some(bar),
    );
    edge(
        "size combo bottom",
        true,
        y1 - 1,
        1,
        bw,
        x0 + bw..arrow - 1,
        field,
        Some(bar),
    );
    edge(
        "size combo left",
        false,
        x0,
        -1,
        bw,
        y0 + bw..y1 - bw,
        field,
        Some(bar),
    );
    let hardness = control("hardness");
    let meter = Rect::from_min_max(
        hardness.min + Vec2::new(theme::TOOL_BUTTON, 0.0),
        hardness.max - Vec2::new(theme::TOOL_BUTTON, 0.0),
    );
    let [_, y0, x1, y1] = snap(meter);
    let empty = [t.pdn_control_border, t.pdn_slider_background];
    let near = dev(24.0);
    edge(
        "meter top",
        true,
        y0,
        -1,
        bw,
        x1 - near..x1 - bw,
        empty,
        Some(bar),
    );
    edge(
        "meter right",
        false,
        x1 - 1,
        1,
        bw,
        y0 + bw..y1 - bw,
        empty,
        Some(bar),
    );
    let [x0, y0, x1, y1] = snap(control("fillStyle"));
    edge(
        "fill top",
        true,
        y0,
        -1,
        bw,
        x0 + bw..x1 - dev(30.0),
        field,
        Some(bar),
    );
    edge(
        "fill left",
        false,
        x0,
        -1,
        bw,
        y0 + bw..y1 - bw,
        field,
        Some(bar),
    );
    // The tool-strip separators: 1-device-px lines in 11-DIP items (§8.2) — the chooser's and
    // each section's — and the toolbar row's first (a 1-DIP line, `px::snap_line`).
    let mid = dev(content.center().y);
    let chooser = px_blend(bar, t.title_bar_border);
    let sep_x = x_options - 6.0;
    edge(
        "chooser separator",
        false,
        dev(sep_x),
        -1,
        1,
        mid - 4..mid + 4,
        [chooser, bar],
        Some(bar),
    );
    let visible = |x: f32| x < l.tool_menu.max.x - 2.0;
    for p in row
        .options
        .iter()
        .filter_map(|p| p.section_line)
        .filter(|p| visible(p.min.x))
    {
        let name = format!("section separator at {}", p.min.x);
        let line = [t.pdn_separator, bar];
        edge(
            &name,
            false,
            dev(p.min.x),
            -1,
            1,
            mid - 4..mid + 4,
            line,
            Some(bar),
        );
    }
    let common = l.common_menu.min.x + 3.0 * 22.0 + 5.0;
    let mid = dev(l.common_menu.center().y);
    let rule = px_blend(t.menu_background, t.title_bar_border);
    let bg = t.menu_background;
    edge(
        "toolbar separator",
        false,
        dev(common),
        -1,
        thin,
        mid - 3..mid + 3,
        [rule, bg],
        Some(bg),
    );
    // Floating-form frames: the translucent border over the window background, then the 1-px
    // title-bar margin; the left border beside the title bar.
    for id in ["layerForm", "colorsForm"] {
        let w = harness
            .state()
            .shell
            .forms
            .windows
            .iter()
            .find(|w| w.id == id)
            .unwrap();
        let [x0, y0, x1, _] = snap(w.rect());
        let border = paintrs::kit::px::over(t.window_background, t.window_border);
        let top = [border, t.window_background];
        edge(id, true, y0, -1, bw, x0 + bw..x1 - bw, top, None);
        let title_rows = y0 + dev(3.0)..y0 + dev(20.0);
        edge(id, false, x0, -1, bw, title_rows, [border, title], None);
    }
    failures
}

/// Task 21 carry: a new view's first Fit to Window uses the real scale (not ppp 1).
#[test]
fn the_first_fit_uses_the_real_scale() {
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(800.0, 500.0))
        .with_pixels_per_point(1.25)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    harness.run();
    let app = &harness.state().app;
    let s = 1.25;
    let expected =
        pdn_core::document::view::fit_zoom(app.view_width * s, app.view_height * s, 800, 600, s);
    assert!(expected < 1.0);
    assert_eq!(app.active_ws().unwrap().view.zoom, expected);
}

fn zoom_of(h: &Harness<'_, paintrs::PaintApp>) -> f64 {
    h.state().app.active_ws().unwrap().view.zoom
}

/// Task 21 (`ui-drawn-graphics.md §2.10`): the status bar's zoom buttons and slider step the
/// presets, the zoom field applies a typed zoom, the unit selector cycles left of its separator.
#[test]
fn status_bar_zoom_and_units() {
    let mut harness = app_harness();
    // The slider's 200-ms repeat runs on the app clock: stop it for the clicks.
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    assert_eq!(zoom_of(&harness), 1.0);
    // Zoom in (1229..1259) and out (1075..1105) in the status bar (y 768..800).
    click(&mut harness, 1244.0, 784.0);
    assert_eq!(zoom_of(&harness), 1.5);
    click(&mut harness, 1090.0, 784.0);
    click(&mut harness, 1090.0, 784.0);
    assert_eq!(zoom_of(&harness), 2.0 / 3.0);
    // The slider (input 1107..1227): a press left of the thumb steps one preset (§2.10 step 3).
    click(&mut harness, 1112.0, 784.0);
    assert_eq!(zoom_of(&harness), 0.5);
    // Dragging the thumb (index 20 → left 1157) picks the preset under the pointer.
    drag(
        &mut harness,
        egui::pos2(1162.0, 784.0),
        egui::pos2(1222.0, 784.0),
        4,
    );
    assert_eq!(zoom_of(&harness), 100.0);
    // The zoom field: open it from the zoom text, type, Enter.
    click(&mut harness, 1022.0, 784.0);
    assert!(harness.state().shell.text_input_focused());
    harness.event(egui::Event::Text("250".into()));
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.text_input_focused());
    assert_eq!(zoom_of(&harness), 2.5);
    // Held left of the thumb (index 25), the step repeats every 200 ms: 2 → 1.5.
    let at = egui::pos2(1112.0, 784.0);
    let button = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    harness.event(egui::Event::PointerMoved(at));
    harness.run();
    harness.event(button(true));
    harness.run();
    assert_eq!(zoom_of(&harness), 2.0);
    harness.state_mut().advance_clock(200);
    harness.step();
    assert_eq!(zoom_of(&harness), 1.5);
    harness.event(button(false));
    harness.run();
    assert_eq!(zoom_of(&harness), 1.5);
    // The unit selector: its text part cycles px → in.
    click(&mut harness, 965.0, 784.0);
    assert_eq!(
        harness.state().app.measurement_unit,
        pdn_core::app::MeasurementUnit::Inch
    );
}

/// Frames slower than the drafts' settle time (a large view on the web, a busy CPU) still zoom in
/// drafts: a view change in the frame after another is a gesture however long the frame took, and
/// the view settles once a frame changes nothing. A pan by whole device px is exact: no draft.
#[test]
fn slow_frames_zoom_in_drafts() {
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    let pos = egui::pos2(640.0, 400.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    let wheel = |dy: f32, modifiers: egui::Modifiers| egui::Event::MouseWheel {
        unit: egui::MouseWheelUnit::Line,
        source: egui::MouseWheelSource::Wheel,
        delta: egui::vec2(0.0, dy),
        phase: egui::TouchPhase::Move,
        modifiers,
    };
    // Zoomed in, so the view scrolls.
    for _ in 0..4 {
        harness.event_modifiers(wheel(1.0, egui::Modifiers::CTRL), egui::Modifiers::CTRL);
        harness.run();
    }
    let frame = |h: &mut Harness<'_, paintrs::PaintApp>, dy: f32, m: egui::Modifiers| {
        if dy != 0.0 {
            h.event(wheel(dy, m));
        }
        h.state_mut().advance_clock(400);
        h.step();
        h.state().shell.canvas_view.drafted()
    };
    let none = egui::Modifiers::NONE;
    assert!(!frame(&mut harness, 0.0, none));
    let y = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.active_ws().unwrap().view.viewport_y;
    let y0 = y(&harness);
    assert!(!frame(&mut harness, -1.0, none));
    assert_eq!(y(&harness), y0 + 120.0);
    assert!(
        !frame(&mut harness, -1.0, none),
        "a whole-pixel pan is exact"
    );
    assert!(!frame(&mut harness, 0.0, none));
    let ctrl = egui::Modifiers::CTRL;
    assert!(
        !frame(&mut harness, 1.0, ctrl),
        "a gesture's first step renders exactly"
    );
    assert!(frame(&mut harness, 1.0, ctrl));
    assert!(frame(&mut harness, -1.0, ctrl));
    // (The view rounds its position in the frame after a zoom: a change too.)
    frame(&mut harness, 0.0, none);
    assert!(!frame(&mut harness, 0.0, none), "settled");
}

/// Task 21 (`ui-drawn-graphics.md §2.10` "Wheel"): a wheel line scrolls 120 device px, Ctrl+wheel
/// zooms ×1.12 per notch about the pointer; the middle button drags the view.
#[test]
fn wheel_and_middle_drag_move_the_view() {
    let mut harness = app_harness();
    let pos = egui::pos2(640.0, 400.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    let wheel = |h: &mut Harness<'_, paintrs::PaintApp>, dy: f32, m: egui::Modifiers| {
        h.event_modifiers(
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                source: egui::MouseWheelSource::Wheel,
                delta: egui::vec2(0.0, dy),
                phase: egui::TouchPhase::Move,
                modifiers: m,
            },
            m,
        );
        h.run();
    };
    let doc_at = |h: &Harness<'_, paintrs::PaintApp>| {
        let v = &h.state().app.active_ws().unwrap().view;
        let b = v.get_render_bounds();
        (
            (640.0 - b.x) / v.css_scale(),
            (400.0 - 85.0 - b.y) / v.css_scale(),
        )
    };
    let before = doc_at(&harness);
    wheel(&mut harness, 1.0, egui::Modifiers::CTRL);
    assert!((zoom_of(&harness) - 1.12).abs() < 1e-12);
    let after = doc_at(&harness);
    assert!((after.0 - before.0).abs() < 1.0 && (after.1 - before.1).abs() < 1.0);
    let y = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.active_ws().unwrap().view.viewport_y;
    let y0 = y(&harness);
    wheel(&mut harness, -1.0, egui::Modifiers::NONE);
    assert_eq!(y(&harness), y0 + 120.0);
    let x = |h: &Harness<'_, paintrs::PaintApp>| h.state().app.active_ws().unwrap().view.viewport_x;
    let (x0, y0) = (x(&harness), y(&harness));
    drag_with(
        &mut harness,
        egui::PointerButton::Middle,
        (640.0, 400.0),
        (600.0, 350.0),
    );
    assert_eq!((x(&harness), y(&harness)), (x0 + 40.0, y0 + 50.0));
}

fn drag_with(
    harness: &mut Harness<'_, paintrs::PaintApp>,
    button: egui::PointerButton,
    from: (f32, f32),
    to: (f32, f32),
) {
    let ev = |pos: egui::Pos2, pressed| egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let (a, z) = (egui::pos2(from.0, from.1), egui::pos2(to.0, to.1));
    harness.event(egui::Event::PointerMoved(a));
    harness.run();
    harness.event(ev(a, true));
    harness.run();
    harness.event(egui::Event::PointerMoved(z));
    harness.run();
    harness.event(ev(z, false));
    harness.run();
}

/// Task 22 (ModalDialogController): under an open dialog the app is inert — a Tools form button,
/// a form's title-bar drag, the canvas (wheel, Ctrl+wheel, middle drag), a main-menu item and
/// the shortcuts (Ctrl+Shift+N, the forms' Ctrl+Shift+F6 reset) do nothing; Escape closes the
/// dialog and the app answers again.
#[test]
fn a_modal_dialog_blocks_the_app_beneath_it() {
    use paintrs::forms::FormId;
    use pdn_core::tool::types::ToolType;
    // Each blocked press flashes the dialog for 850 ms: let `run` wait it out.
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(120)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    harness.run();
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    harness
        .state_mut()
        .shell
        .dialogs
        .push(Box::new(paintrs::dialogs::about::AboutDialog::new(false)));
    harness.run();
    let tool = harness.state().app.tool_type;
    click(&mut harness, 27.0, 143.0);
    assert_eq!(harness.state().app.tool_type, tool);
    let history = form_rect(&harness, FormId::History);
    let grab = history.min + egui::vec2(60.0, 16.0);
    drag(&mut harness, grab, grab + egui::vec2(-300.0, 200.0), 4);
    assert_eq!(form_rect(&harness, FormId::History), history);
    harness.key_press_modifiers(egui::Modifiers::CTRL | egui::Modifiers::SHIFT, egui::Key::N);
    harness.run();
    assert_eq!(layers(&harness), 1);
    harness.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::F6,
    );
    harness.run();
    assert_eq!(form_rect(&harness, FormId::History), history);
    // The canvas outside the dialog: the wheel, Ctrl+wheel and a middle-button drag leave the
    // view alone.
    let view = |h: &Harness<'_, paintrs::PaintApp>| {
        let v = &h.state().app.active_ws().unwrap().view;
        (v.zoom, v.viewport_x, v.viewport_y)
    };
    let before = view(&harness);
    let at = egui::pos2(1000.0, 730.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.run();
    for m in [egui::Modifiers::NONE, egui::Modifiers::CTRL] {
        harness.event_modifiers(
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Line,
                source: egui::MouseWheelSource::Wheel,
                delta: egui::vec2(0.0, -1.0),
                phase: egui::TouchPhase::Move,
                modifiers: m,
            },
            m,
        );
        harness.run();
    }
    drag_with(
        &mut harness,
        egui::PointerButton::Middle,
        (1000.0, 730.0),
        (960.0, 700.0),
    );
    assert_eq!(view(&harness), before);
    // File: its drop menu would cover the canvas surround at (150, 300).
    let surround = pixel(&harness.render().expect("render"), 150.0, 300.0);
    click(&mut harness, 22.0, 16.0);
    let after = pixel(&harness.render().expect("render"), 150.0, 300.0);
    assert_eq!(after, surround);
    assert!(harness.state().shell.dialogs.is_open());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    click(&mut harness, 27.0, 143.0);
    assert_eq!(
        harness.state().app.tool_type,
        Some(ToolType::RectangleSelect)
    );
}

/// Task 22: Help ▸ About (`UiRequest::About`) opens the About dialog, 625 wide and centred; its
/// Close button (104 × 26, 10 from the right and centred in the 45-DIP footer) closes it.
#[test]
fn help_about_opens_and_closes_the_about_dialog() {
    let mut harness = app_harness();
    harness
        .state_mut()
        .app
        .ui_requests
        .push(pdn_core::action::UiRequest::About);
    harness.run();
    let dialogs = &harness.state().shell.dialogs;
    let rect = dialogs.top_rect(&harness.ctx).expect("the About dialog");
    assert_eq!((rect.min.x, rect.width()), (327.5, 625.0));
    assert!((rect.center().y - 400.0).abs() < 1e-3);
    click(&mut harness, rect.max.x - 11.0 - 52.0, rect.max.y - 23.0);
    assert!(!harness.state().shell.dialogs.is_open());
}

/// Task 25: a selection change re-uploads the old overlay rect as well as the new one (with the
/// base change), so no ants or tint are left where the selection was.
#[test]
fn a_moved_selection_leaves_no_ghost_ants() {
    use pdn_core::util::geom::Rectangle;
    let mut harness = app_harness();
    let mut select = |r: Option<Rectangle>| {
        let ws = harness.state_mut().app.active_ws_mut().unwrap();
        ws.selection.reset();
        if let Some(r) = r {
            ws.selection
                .set_continuation(&r, pdn_core::selection::CombineMode::Replace);
            ws.selection.commit_continuation();
        }
        harness.run_steps(2);
        harness.render().expect("render")
    };
    let base = select(None);
    let a = select(Some(Rectangle::new(100.0, 100.0, 150.0, 100.0)));
    // A's ants (and tint) over the white image, above the status bar (its text changes too).
    let canvas_bottom = 800 - paintrs::theme::FOOTER_HEIGHT as u32;
    let drawn: Vec<(u32, u32)> = a
        .enumerate_pixels()
        .filter(|&(x, y, p)| {
            y < canvas_bottom && base.get_pixel(x, y).0 == [255; 4] && p.0 != [255; 4]
        })
        .map(|(x, y, _)| (x, y))
        .collect();
    assert!(drawn.len() > 300, "{}", drawn.len());
    let b = select(Some(Rectangle::new(500.0, 400.0, 150.0, 100.0)));
    let ghosts: Vec<_> = drawn
        .iter()
        .filter(|&&(x, y)| b.get_pixel(x, y) != base.get_pixel(x, y))
        .collect();
    assert!(ghosts.is_empty(), "{} ghost pixels", ghosts.len());
}

/// Task 25q: a complex selection's masks (over 4096 vertices: a Magic Wand-like noise patch) build
/// on the workers. The frame that changes the selection keeps drawing the old ants; once the
/// masks land the canvas shows the new ones with nothing of the old left, as a forced full refresh
/// would.
#[test]
fn complex_selection_masks_land_from_the_workers() {
    use pdn_core::selection::CombineMode;
    use pdn_core::selection::path::GraphicsPath;
    use pdn_core::util::geom::{Point, Rectangle};
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    let canvas_bottom = 800 - paintrs::theme::FOOTER_HEIGHT as u32;
    let canvas = |h: &mut Harness<'_, paintrs::PaintApp>| {
        let image = h.render().expect("render");
        let rows = image.rows().take(canvas_bottom as usize);
        rows.flat_map(|r| r.map(|p| p.0)).collect::<Vec<_>>()
    };
    let select = |h: &mut Harness<'_, paintrs::PaintApp>, p: GraphicsPath| {
        let ws = h.state_mut().app.active_ws_mut().unwrap();
        ws.selection.set_continuation_path(p, CombineMode::Replace);
        ws.selection.commit_continuation();
    };
    let base = canvas(&mut harness);
    select(
        &mut harness,
        GraphicsPath::from_rect(&Rectangle::new(100.0, 100.0, 150.0, 100.0)),
    );
    harness.run_steps(2);
    let old = canvas(&mut harness);
    assert!(old != base, "the rectangle's ants");
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut noise = GraphicsPath::default();
    for j in 0..48 {
        for i in 0..64 {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            if seed.is_multiple_of(3) {
                let (x, y) = (400.0 + i as f64, 300.0 + j as f64);
                let square = [
                    (x, y),
                    (x + 1.0, y),
                    (x + 1.0, y + 1.0),
                    (x, y + 1.0),
                    (x, y),
                ];
                noise
                    .polygons
                    .push(square.iter().map(|&(x, y)| Point::new(x, y)).collect());
            }
        }
    }
    select(&mut harness, noise);
    harness.run_steps(1);
    assert!(canvas(&mut harness) == old, "the old ants meanwhile");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    let new = loop {
        std::thread::sleep(std::time::Duration::from_millis(5));
        harness.run_steps(1);
        let now = canvas(&mut harness);
        if now != old {
            break now;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the masks never landed"
        );
    };
    // The rectangle's ants are gone (its box over the white image is white again); the patch's
    // show.
    let changed = |a: &[[u8; 4]], b: &[[u8; 4]]| (0..a.len()).filter(|&i| a[i] != b[i]).count();
    let rect: Vec<(usize, usize)> = (0..old.len())
        .filter(|&i| old[i] != base[i] && base[i] == [255; 4])
        .map(|i| (i % 1280, i / 1280))
        .collect();
    let (xs, ys) = (rect.iter().map(|p| p.0), rect.iter().map(|p| p.1));
    let (x0, x1) = (xs.clone().min().unwrap(), xs.max().unwrap());
    let (y0, y1) = (ys.clone().min().unwrap(), ys.max().unwrap());
    let ghosts = (y0..=y1).flat_map(|y| (x0..=x1).map(move |x| y * 1280 + x));
    let ghosts = ghosts.filter(|&i| base[i] == [255; 4] && new[i] != base[i]);
    assert_eq!(ghosts.count(), 0, "ghosts");
    assert!(changed(&new, &base) > 500, "the patch's ants");
    // A forced full refresh changes nothing.
    harness.state_mut().shell.canvas_view.refresh_all();
    harness.run_steps(1);
    assert_eq!(changed(&canvas(&mut harness), &new), 0, "stale pixels");
}

/// Task 25p: an ants frame alone rewrites and uploads only the outline spans (a large selection),
/// or the whole overlay rect when they would cost about as much (a small one). After several
/// phases the canvas matches a forced full overlay refresh at the same phase byte for byte (no
/// stale dashes).
#[test]
fn ants_frames_match_a_full_refresh() {
    use pdn_core::selection::CombineMode;
    use pdn_core::util::geom::{Point, Rectangle};
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    let lasso = [
        (380.2, 60.1),
        (700.7, 420.3),
        (420.4, 480.0),
        (650.1, 90.7),
        (360.0, 400.5),
    ];
    let lasso: Vec<Point> = lasso.iter().map(|&(x, y)| Point::new(x, y)).collect();
    let small = Rectangle::new(200.5, 300.5, 7.0, 5.0);
    for large in [true, false] {
        {
            let ws = harness.state_mut().app.active_ws_mut().unwrap();
            ws.selection.reset();
            if large {
                let r = Rectangle::new(40.0, 30.0, 300.0, 200.0);
                ws.selection.set_continuation(&r, CombineMode::Replace);
                ws.selection.commit_continuation();
                ws.selection
                    .set_continuation_points(&lasso, CombineMode::Union);
            } else {
                ws.selection.set_continuation(&small, CombineMode::Replace);
            }
            ws.selection.commit_continuation();
        }
        harness.run_steps(2);
        let first = harness.render().expect("render");
        let revisions = |h: &Harness<'_, paintrs::PaintApp>| {
            let ws = h.state().app.active_ws().unwrap();
            let sb = &ws.surface_box;
            ((ws.selection.revision(), sb.revision()), sb.ants_revision())
        };
        let before = revisions(&harness);
        for _ in 0..5 {
            // One 1/30-s counter step each.
            harness.state_mut().advance_clock(34);
            harness.run_steps(1);
        }
        let spans = harness.render().expect("render");
        assert!(spans != first, "the ants moved");
        // Frame-only changes: the overlay key stays, so these frames took the ants path.
        let after = revisions(&harness);
        assert_eq!(after.0, before.0, "an ants frame bumped a content revision");
        assert_ne!(after.1, before.1);
        // A full refresh, same phase.
        harness.state_mut().shell.canvas_view.refresh_all();
        harness.run_steps(1);
        let full = harness.render().expect("render");
        let stale = spans
            .pixels()
            .zip(full.pixels())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(stale, 0, "stale pixels (large: {large})");
    }
}

/// Task 25p on Task 26: the canvas view paints the overlays on the way to the texture, handles
/// included. After a Magic Wand click its pending nub is on screen at the click: removing the
/// handle layer changes the pixels around it (the origin square and the compass) and no others.
#[test]
fn the_canvas_view_draws_the_wand_handles() {
    use pdn_core::render::Renderer;
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    harness
        .state_mut()
        .app
        .set_active_tool_from_type(ToolType::MagicWand);
    harness.run();
    click(&mut harness, 500.0, 400.0);
    harness.run_steps(2);
    let shown = harness.render().expect("render");
    {
        let ws = harness.state_mut().app.active_ws_mut().unwrap();
        let id = ws
            .surface_box
            .renderers
            .iter()
            .find(|(_, r)| matches!(r, Renderer::Handles(h) if !h.is_empty()))
            .map(|(id, _)| *id)
            .expect("the pending nub");
        // Removed, not emptied: the wand writes its list back every frame.
        ws.surface_box.remove_renderer(id);
    }
    harness.run_steps(1);
    let hidden = harness.render().expect("render");
    let changed: Vec<(u32, u32)> = shown
        .enumerate_pixels()
        .filter(|&(x, y, p)| hidden.get_pixel(x, y) != p)
        .map(|(x, y, _)| (x, y))
        .collect();
    assert!(changed.len() > 20, "{} nub pixels", changed.len());
    let far = changed
        .iter()
        .filter(|&&(x, y)| !(490..530).contains(&x) || !(390..430).contains(&y));
    assert_eq!(far.count(), 0);
}

/// Task 26t (`tooltips.md` §9): resting on a Tools-window button shows its tip 400 ms after the
/// pointer entered it (not 399), 32 DIP below the pointer at its x (the stock path), laid out as
/// two lines; the status bar's zoom buttons never tip.
#[test]
fn tools_window_buttons_tip_after_400_ms_and_the_status_bar_never() {
    let mut storage = serde_json::Map::new();
    storage.insert("paintdotjs.welcome.dismissed".into(), "true".into());
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .build_eframe(move |cc| paintrs::PaintApp::new_for_test(&cc.egui_ctx, storage));
    harness.run_steps(3);
    let tip = |h: &Harness<'_, paintrs::PaintApp>| paintrs::kit::tooltip::shown(&h.ctx);
    let rest = |h: &mut Harness<'_, paintrs::PaintApp>, x: f32, y: f32, ms: u32| {
        h.event(egui::Event::PointerMoved(egui::pos2(x, y)));
        h.run();
        h.state_mut().advance_clock(ms);
        h.run();
    };
    // The Zoom button (the 8th, after the two Extra selection tools; the buttons from (7, 123):
    // 39..71 × 213..243), entered at (59, 228), then a move inside it.
    rest(&mut harness, 59.0, 228.0, 300);
    rest(&mut harness, 62.0, 231.0, 99);
    assert_eq!(tip(&harness), None);
    harness.state_mut().advance_clock(1);
    harness.run();
    let (text, rect) = tip(&harness).expect("the Zoom tip");
    assert_eq!(text, "Zoom\nShortcut key: Z");
    assert_eq!(rect.min, egui::pos2(62.0, 263.0));
    assert_eq!(rect.height(), 40.0);
    let runs = paintrs::kit::text::take_text_runs(&harness.ctx);
    for line in ["Zoom", "Shortcut key: Z"] {
        assert!(
            runs.iter()
                .any(|(r, _, t)| t.as_deref() == Some(line) && rect.contains_rect(*r)),
            "{line:?} in {rect:?}"
        );
    }
    // The status bar's Zoom in (1229..1259 × 768..800): nothing, however long.
    rest(&mut harness, 1244.0, 784.0, 1000);
    assert_eq!(tip(&harness), None);
    rest(&mut harness, 1245.0, 785.0, 10_000);
    assert_eq!(tip(&harness), None);
}

/// Task 42 (adjustments-and-effect-framework.md §2.2 steps 3–5): the dialog previews into the
/// layer; Escape restores it with no history entry; Enter (OK) pushes the effect's memento and
/// makes it the one Repeat runs.
#[test]
fn effect_dialog_previews_cancels_and_commits() {
    let mut harness = app_harness();
    harness.state_mut().app.effect_seed = Some(0);
    let layer = |h: &Harness<'_, paintrs::PaintApp>| {
        let ws = h.state().app.active_ws().unwrap();
        ws.document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .to_vec()
    };
    let history =
        |h: &Harness<'_, paintrs::PaintApp>| h.state().app.active_ws().unwrap().history.undo.len();
    let white = layer(&harness);
    let undo = history(&harness);
    let open = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.state_mut()
            .app
            .ui_requests
            .push(pdn_core::action::UiRequest::EffectDialog(
                "addNoiseEffect".into(),
            ));
        h.run();
        assert!(h.state().shell.dialogs.is_open());
        // The preview renders on a worker thread.
        for _ in 0..500 {
            if layer(h) != white {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            h.run();
        }
        assert_ne!(layer(h), white, "the preview reached the layer");
    };
    open(&mut harness);
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(layer(&harness), white);
    assert_eq!(history(&harness), undo);
    assert!(!harness.state().app.active_ws().unwrap().dirty);
    open(&mut harness);
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_ne!(layer(&harness), white);
    assert_eq!(history(&harness), undo + 1);
    assert_eq!(
        harness.state().app.last_effect.as_deref(),
        Some("addNoiseEffect")
    );
}

/// Opens effect `id`'s dialog through the shell (`UiRequest::EffectDialog`).
fn open_effect(harness: &mut Harness<'_, paintrs::PaintApp>, id: &str) {
    harness
        .state_mut()
        .app
        .ui_requests
        .push(pdn_core::action::UiRequest::EffectDialog(id.into()));
    harness.run();
    harness.run();
    assert!(harness.state().shell.dialogs.is_open(), "{id}");
}

fn undo_len(harness: &Harness<'_, paintrs::PaintApp>) -> usize {
    harness.state().app.active_ws().unwrap().history.undo.len()
}

/// R229: Enter is OK as soon as an effect dialog opens, never a click on the title bar's close
/// button (which held the initial focus, a paint.js quirk): a configurable effect (its first
/// editor focused) and the Extra dialogs with their own Enter (Puppet Warp, Liquify, Vanishing
/// Point) each apply, as one history item.
#[test]
fn enter_applies_an_opened_effect_dialog() {
    use pdn_core::effect::extra::{liquify, vanishing, warp};
    for id in ["blurEffect", warp::PUPPET, liquify::ID, vanishing::ID] {
        let mut h = app_harness();
        h.state_mut()
            .app
            .create_blank_document_in_new_workspace(64, 48, 96.0, true);
        h.run_steps(2);
        let before = undo_len(&h);
        open_effect(&mut h, id);
        h.key_press(egui::Key::Enter);
        for _ in 0..500 {
            h.run();
            if !h.state().shell.dialogs.is_open() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(!h.state().shell.dialogs.is_open(), "{id}");
        assert_eq!(undo_len(&h), before + 1, "{id}");
    }
}

/// BB36 (R143): Chart or Graph through the real app: its registered dialog, the type list and
/// Key, OK committing once with the stored chart; Repeat draws that chart again.
#[test]
fn chart_or_graph_dialog_flow() {
    use egui_kittest::kittest::Queryable;
    let mut h = app_harness();
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(160, 120, 96.0, true);
    h.run_steps(2);
    let before = undo_len(&h);
    open_effect(&mut h, "bbChartOrGraph");
    assert!(h.state().shell.dialogs.top_title_is("Chart or Graph"));
    h.get_by_label("Type").click();
    h.run_steps(3);
    h.get_by_label("Column").click();
    h.run_steps(3);
    h.get_by_label("Key").click();
    h.run_steps(3);
    h.get_by_label("OK").click();
    for _ in 0..500 {
        h.run();
        if !h.state().shell.dialogs.is_open() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), before + 1);
    let token = h.state().app.effect_tokens["bbChartOrGraph"]["chart"].clone();
    let chart = pdn_core::effect::bb::chart::model::decode(token.as_str().unwrap()).unwrap();
    assert!(chart.series.iter().all(|s| s.chart_type
        == pdn_core::effect::bb::chart::model::ChartType::Column
        && s.legend_visible));
    let picture = pdn_core::effect::bb::chart::render(&chart, 160, 120, "en").unwrap();
    let layer = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .active_ws()
            .unwrap()
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .to_vec()
    };
    assert_eq!(layer(&h), picture.data);
    pdn_core::action::perform(&mut h.state_mut().app, "menu.effects.repeat");
    h.run_steps(2);
    assert_eq!(undo_len(&h), before + 2);
    assert_eq!(layer(&h), picture.data);
}

/// Task 42 review round 1: an open select's list takes the pointer — a row over OK picks the row
/// and leaves the dialog open (Pixelate's Scale up), Escape then closes only the list, and Enter
/// picks the highlighted row.
#[test]
fn select_lists_take_the_pointer_and_their_keys() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    let undo = undo_len(&harness);
    open_effect(&mut harness, "pixelateEffect");
    harness.get_by_label("Scale up").click();
    harness.run();
    let row = harness.get_by_label("Bilinear").rect();
    assert!(
        row.intersects(harness.get_by_label("OK").rect()),
        "the probed layout: the row lies over OK"
    );
    // The list's border takes the press too: here it lies over Cancel.
    let border = egui::pos2(row.max.x + 0.5, row.center().y);
    assert!(harness.get_by_label("Cancel").rect().contains(border));
    click(&mut harness, border.x, border.y);
    assert!(harness.state().shell.dialogs.is_open());
    harness.get_by_label("Bilinear").click();
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&harness), undo);
    assert_eq!(
        harness.get_by_label("Scale up").value().as_deref(),
        Some("Bilinear")
    );
    // Escape with the list open closes the list only.
    harness.get_by_label("Scale up").click();
    harness.run();
    assert!(harness.query_by_label("Nearest Neighbor").is_some());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert!(harness.query_by_label("Nearest Neighbor").is_none());
    // Enter with the list open picks the highlighted row.
    harness.get_by_label("Scale up").click();
    harness.run();
    harness.key_press(egui::Key::ArrowDown);
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert_eq!(
        harness.get_by_label("Scale up").value().as_deref(),
        Some("Nearest Neighbor")
    );
    assert_eq!(undo_len(&harness), undo);
}

/// Task 42 review round 1: Curves' "RGB" row over the editor switches the mode without adding a
/// Luminosity point; Rotate/Zoom's "Repeat" row over the Sampling select leaves Sampling alone.
#[test]
fn select_rows_over_other_controls_do_not_reach_them() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    open_effect(&mut harness, "curvesEffect");
    harness.get_by_label("Transfer Map").click();
    harness.run();
    assert!(
        harness
            .get_by_label("RGB")
            .rect()
            .intersects(harness.get_by_label("Curve editor").rect())
    );
    harness.get_by_label("RGB").click();
    harness.run();
    assert_eq!(
        harness.get_by_label("Transfer Map").value().as_deref(),
        Some("RGB")
    );
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    let token = &harness.state().app.effect_tokens["curvesEffect"];
    assert_eq!(token["channel"], serde_json::json!("rgb"));
    assert_eq!(token["points"], serde_json::json!([[0, 0], [255, 255]]));
    open_effect(&mut harness, "rotateZoomEffect");
    harness.get_by_label("Tiling Mode:").click();
    harness.run();
    let row = harness.get_by_label("Repeat").rect();
    assert!(row.intersects(harness.get_by_label("Sampling:").rect()));
    harness.get_by_label("Repeat").click();
    harness.run();
    assert_eq!(
        harness.get_by_label("Tiling Mode:").value().as_deref(),
        Some("Repeat")
    );
    assert_eq!(
        harness.get_by_label("Sampling:").value().as_deref(),
        Some("Bilinear")
    );
    assert!(harness.query_by_label("Nearest Neighbor").is_none());
}

/// Task 42 review round 1: Enter is OK while the title bar's close button holds the initial
/// focus, but a Tab-focused footer button takes it (Cancel cancels).
#[test]
fn enter_activates_a_focused_footer_button() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    let undo = undo_len(&harness);
    open_effect(&mut harness, "addNoiseEffect");
    harness.get_by_label("Cancel").focus();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&harness), undo);
    open_effect(&mut harness, "addNoiseEffect");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&harness), undo + 1);
}

/// Task 42 review round 2: as a native drop-down, a press outside an open list only closes it —
/// over Cancel the dialog stays open, on the backdrop it is not cancelled — and Enter on a focused
/// closed select opens its list instead of OK.
#[test]
fn a_press_outside_an_open_list_only_closes_it() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    let undo = undo_len(&harness);
    open_effect(&mut harness, "pixelateEffect");
    harness.get_by_label("Scale up").click();
    harness.run();
    let rows = ["Bicubic", "Bilinear", "Nearest Neighbor"]
        .map(|l| harness.get_by_label(l).rect().expand(1.0));
    let list = rows.iter().fold(egui::Rect::NOTHING, |a, r| a.union(*r));
    let cancel = harness.get_by_label("Cancel").rect().shrink(1.0);
    let spot = (0..=8)
        .flat_map(|j| (0..=8).map(move |i| (i, j)))
        .map(|(i, j)| cancel.min + cancel.size() * egui::vec2(i as f32 / 8.0, j as f32 / 8.0))
        .find(|p| !list.contains(*p))
        .expect("a part of Cancel outside the list");
    click(&mut harness, spot.x, spot.y);
    harness.run();
    assert!(
        harness.query_by_label("Nearest Neighbor").is_none(),
        "closed"
    );
    assert!(harness.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&harness), undo);
    harness.get_by_label("Scale up").click();
    harness.run();
    click(&mut harness, 5.0, 400.0);
    harness.run();
    assert!(harness.query_by_label("Nearest Neighbor").is_none());
    assert!(
        harness.state().shell.dialogs.is_open(),
        "the backdrop press only closed the list"
    );
    assert!(
        !harness.state().shell.dialogs.attention_active(&harness.ctx),
        "the list's press: no flash"
    );
    // Enter on the focused, closed select.
    harness.get_by_label("Scale up").focus();
    harness.run();
    assert!(harness.get_by_label("Scale up").is_focused());
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert!(
        harness.query_by_label("Nearest Neighbor").is_some(),
        "the list opened"
    );
    assert_eq!(undo_len(&harness), undo);
}

/// Ruling R65: a click on the backdrop never cancels an effect dialog (Paint.NET's modal windows;
/// paintdotjs's `backdrop.onclick` dropped): the dialog flashes for attention and stays, its
/// settings kept.
#[test]
fn a_backdrop_click_flashes_and_keeps_the_effect_dialog() {
    let mut harness = app_harness();
    let undo = undo_len(&harness);
    open_effect(&mut harness, "pixelateEffect");
    assert!(!harness.state().shell.dialogs.attention_active(&harness.ctx));
    let pos = egui::pos2(5.0, 400.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run_steps(2);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        harness.run_steps(2);
    }
    assert!(harness.state().shell.dialogs.is_open(), "the dialog stays");
    assert!(
        harness.state().shell.dialogs.attention_active(&harness.ctx),
        "the backdrop press flashed it"
    );
    assert_eq!(undo_len(&harness), undo);
}

fn doc_size(h: &Harness<'_, paintrs::PaintApp>) -> (u32, u32) {
    let d = &h.state().app.active_ws().unwrap().document;
    (d.width, d.height)
}

fn last_entry(h: &Harness<'_, paintrs::PaintApp>) -> String {
    let ws = h.state().app.active_ws().unwrap();
    ws.history
        .undo
        .last()
        .map(|m| m.name.clone())
        .unwrap_or_default()
}

/// Opens a dialog by its shortcut and lets its initial focus land (`input.select()`).
fn open_by_key(
    harness: &mut Harness<'_, paintrs::PaintApp>,
    mods: egui::Modifiers,
    key: egui::Key,
) {
    harness.key_press_modifiers(mods, key);
    harness.run_steps(4);
    assert!(harness.state().shell.dialogs.is_open(), "{key:?}");
}

/// Task 44: Resize opens with its width selected; typing replaces it, the aspect lock (Paint.NET's
/// default) carries the height in doubles (1601 on 800×600 → 1201), Enter resizes. Canvas Size
/// (Top Left by default) anchors where its chooser is clicked and fills the new area transparent.
#[test]
fn size_dialogs_resize_and_change_the_canvas() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    open_by_key(&mut harness, egui::Modifiers::CTRL, egui::Key::R);
    harness.event(egui::Event::Text("1601".into()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(doc_size(&harness), (1601, 1201));
    assert_eq!(last_entry(&harness), "Resize...");
    open_by_key(
        &mut harness,
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::R,
    );
    harness.event(egui::Event::Text("1611".into()));
    harness.run();
    harness.get_by_label("Center").click();
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(doc_size(&harness), (1611, 1201));
    assert_eq!(last_entry(&harness), "Canvas Size...");
    let ws = harness.state().app.active_ws().unwrap();
    let data = ws.document.layers.get_at(0).unwrap().surface.data();
    // dx = (1611 − 1601) / 2: five transparent columns, then the old white.
    assert_eq!(
        (data[4 * 4 + 3], data[5 * 4..5 * 4 + 4].to_vec()),
        (0, vec![255; 4])
    );
}

/// Shared Select arrows must not leak into Resize's close/accept controls.
#[test]
fn resize_select_arrows_pick_before_independent_parent_ok_or_cancel() {
    use egui::{Key, Modifiers};
    use egui_kittest::kittest::{NodeT, Queryable};
    for accept in [false, true] {
        let mut h = app_harness();
        let size = doc_size(&h);
        let history = undo_len(&h);
        open_by_key(&mut h, Modifiers::CTRL, Key::R);
        h.event(egui::Event::Text("400".into()));
        h.run();
        h.get_by_label("Resampling").focus();
        h.run_steps(2);
        let field = h.get_by_label("Resampling").accesskit_node().id();
        for key in [Key::Enter, Key::End, Key::ArrowUp, Key::Enter] {
            h.key_press(key);
            h.run_steps(3);
        }
        assert!(h.state().shell.dialogs.is_open());
        assert_eq!(h.get_by(|n| n.is_focused()).accesskit_node().id(), field);
        assert!(h.query_by_label("Bicubic").is_none());
        assert_eq!(
            h.get_by_label("Resampling").value().as_deref(),
            Some("Fant")
        );
        assert_eq!(doc_size(&h), size);
        assert_eq!(undo_len(&h), history);
        h.get_by_label("Reset").focus();
        h.run_steps(2);
        h.key_press(Key::Enter);
        h.run_steps(3);
        assert_eq!(
            h.get_by_label("Resampling").value().as_deref(),
            Some("Bicubic")
        );
        assert!(h.state().shell.dialogs.is_open());
        h.get_by_label("Resampling").focus();
        h.run_steps(2);
        for key in [Key::Enter, Key::End, Key::ArrowUp, Key::Enter] {
            h.key_press(key);
            h.run_steps(3);
        }
        assert!(h.get_by_label("Resampling").is_focused());
        assert_eq!(
            h.get_by_label("Resampling").value().as_deref(),
            Some("Fant")
        );
        assert_eq!(doc_size(&h), size);
        assert_eq!(undo_len(&h), history);
        h.get_by_label(if accept { "OK" } else { "Cancel" }).focus();
        h.run_steps(2);
        h.key_press(Key::Enter);
        h.run_steps(3);
        assert!(!h.state().shell.dialogs.is_open());
        assert_eq!(doc_size(&h), if accept { (400, 300) } else { size });
        assert_eq!(undo_len(&h), history + usize::from(accept));
    }
}

/// Task 44: Layer Properties previews on the layer; Escape puts it back with no entry; OK makes
/// one "Layer Properties" entry; an emptied name keeps the old one (no change, no entry).
#[test]
fn layer_properties_preview_cancel_and_commit() {
    let mut harness = app_harness();
    let name = |h: &Harness<'_, paintrs::PaintApp>| {
        let ws = h.state().app.active_ws().unwrap();
        ws.document
            .layers
            .get_at(0)
            .unwrap()
            .properties
            .name
            .clone()
    };
    let undo = undo_len(&harness);
    open_by_key(&mut harness, egui::Modifiers::NONE, egui::Key::F4);
    harness.event(egui::Event::Text("Sky".into()));
    harness.run();
    assert_eq!(name(&harness), "Sky", "the preview");
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(
        (name(&harness), undo_len(&harness)),
        ("Background".into(), undo)
    );
    open_by_key(&mut harness, egui::Modifiers::NONE, egui::Key::F4);
    harness.event(egui::Event::Text("Sky".into()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(
        (name(&harness), undo_len(&harness)),
        ("Sky".into(), undo + 1)
    );
    assert_eq!(last_entry(&harness), "Layer Properties");
    assert!(harness.state().app.active_ws().unwrap().dirty);
    open_by_key(&mut harness, egui::Modifiers::NONE, egui::Key::F4);
    harness.key_press(egui::Key::Backspace);
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(
        (name(&harness), undo_len(&harness)),
        ("Sky".into(), undo + 1)
    );
}

/// Task 44: New opens with the active image's size, its width selected; Tab selects the height;
/// OK opens the image (replacing the untouched initial one). Maintain aspect ratio carries the
/// height from the width and is remembered (`workspace.newFileMaintainAspectRatio`).
#[test]
fn new_file_dialog_opens_the_new_image() {
    let mut harness = app_harness();
    open_by_key(&mut harness, egui::Modifiers::CTRL, egui::Key::N);
    harness.event(egui::Event::Text("300".into()));
    harness.run();
    harness.key_press(egui::Key::Tab);
    harness.run_steps(2);
    harness.event(egui::Event::Text("200".into()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!(harness.state().app.documents.len(), 1);
    assert_eq!(doc_size(&harness), (300, 200));
    let aspect = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .settings
            .get("workspace.newFileMaintainAspectRatio")
            .clone()
    };
    assert_eq!(aspect(&harness), serde_json::json!(false));
    // The dialog (340 × 351) is centred: its aspect checkbox at (482, 294), the width at (590,
    // 339.5) in a 1280 × 800 window.
    open_by_key(&mut harness, egui::Modifiers::CTRL, egui::Key::N);
    click(&mut harness, 488.0, 300.0);
    click(&mut harness, 620.0, 352.0);
    harness.key_press_modifiers(
        egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
        egui::Key::A,
    );
    harness.run();
    harness.event(egui::Event::Text("600".into()));
    harness.run();
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(harness.state().app.documents.len(), 2);
    assert_eq!(doc_size(&harness), (600, 400));
    assert_eq!(aspect(&harness), serde_json::json!(true));
}

/// The keyboard-focused accessible node's label and value.
fn focused_node(h: &Harness<'_, paintrs::PaintApp>) -> (String, String) {
    use egui_kittest::kittest::{NodeT, Queryable};
    let n = h.get_by(|n| n.is_focused());
    let n = n.accesskit_node();
    (n.label().unwrap_or_default(), n.value().unwrap_or_default())
}

/// Task 44 review round 1: Shift+Tab leaves a number field for the control before it (it was
/// trapped: the field's Tab stop handed the focus straight back to its editor) — Resize's Height
/// to Width, Add Noise's Intensity number to its slider (as at 80b739e).
#[test]
fn shift_tab_leaves_number_fields() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    open_by_key(&mut harness, egui::Modifiers::CTRL, egui::Key::R);
    harness.key_press(egui::Key::Tab);
    harness.run_steps(3);
    assert_eq!(focused_node(&harness), ("Height:".into(), "600".into()));
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    harness.run_steps(3);
    assert_eq!(focused_node(&harness), ("Width:".into(), "800".into()));
    harness.key_press(egui::Key::Escape);
    harness.run();
    open_effect(&mut harness, "addNoiseEffect");
    // Native first selection is the scalar numeric editor (§7.6, R94).
    assert_eq!(focused_node(&harness).0, "intensity");
    harness.key_press_modifiers(egui::Modifiers::SHIFT, egui::Key::Tab);
    harness.run_steps(3);
    assert!(
        harness
            .get_by_role_and_label(egui::accesskit::Role::Slider, "intensity")
            .is_focused()
    );
}

/// Task 44 review round 1: tabbing through a field the user did not edit changes nothing — its
/// shown text is rounded (a 300-dpi image's print width 2.67 in; 96 dpi shown per cm as 37.80),
/// and committing it on blur resampled the image or moved its DPI.
#[test]
fn tabbing_through_unedited_fields_changes_nothing() {
    use egui_kittest::kittest::Queryable;
    for (dpi, unit, tabs) in [(300.0, "inch", 6), (96.0, "centimeter", 4)] {
        let mut harness = app_harness();
        harness
            .state_mut()
            .app
            .active_ws_mut()
            .unwrap()
            .document
            .resolution = dpi;
        harness.state_mut().prefs.set(
            "paintdotjs.imageSize.v1",
            &serde_json::json!({ "resolutionUnit": unit }).to_string(),
        );
        let undo = undo_len(&harness);
        open_by_key(&mut harness, egui::Modifiers::CTRL, egui::Key::R);
        // Width → Height → Resolution → its unit → print Width (→ its unit → print Height).
        for _ in 0..tabs {
            harness.key_press(egui::Key::Tab);
            harness.run_steps(3);
        }
        harness.get_by_label("OK").click();
        harness.run_steps(2);
        assert!(!harness.state().shell.dialogs.is_open(), "{unit}");
        let ws = harness.state().app.active_ws().unwrap();
        assert_eq!(
            (
                doc_size(&harness),
                ws.document.resolution,
                undo_len(&harness),
                ws.dirty
            ),
            ((800, 600), dpi, undo, false),
            "{unit}"
        );
    }
}

/// Task 44 review round 1: on a focused checkbox Space toggles and Enter submits the dialog
/// without toggling it (egui clicks focused widgets on Enter too).
#[test]
fn enter_on_a_checkbox_submits_without_toggling() {
    let mut harness = app_harness();
    let visible = |h: &Harness<'_, paintrs::PaintApp>| {
        let ws = h.state().app.active_ws().unwrap();
        ws.document.layers.get_at(0).unwrap().properties.visible
    };
    let undo = undo_len(&harness);
    // Name → the opacity slider → its number → Blend Mode → Visible.
    let to_visible = |h: &mut Harness<'_, paintrs::PaintApp>| {
        open_by_key(h, egui::Modifiers::NONE, egui::Key::F4);
        for _ in 0..4 {
            h.key_press(egui::Key::Tab);
            h.run_steps(3);
        }
    };
    to_visible(&mut harness);
    harness.key_press(egui::Key::Space);
    harness.run();
    assert!(!visible(&harness), "Space toggles (previewed)");
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!((visible(&harness), undo_len(&harness)), (false, undo + 1));
    to_visible(&mut harness);
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    assert_eq!((visible(&harness), undo_len(&harness)), (false, undo + 1));
}

/// Task 44 review round 1: Enter on a focused push button inside the form (an anchor cell, the
/// reset button) clicks it only; the dialog stays open until OK.
#[test]
fn enter_on_an_anchor_cell_picks_it_only() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    open_by_key(
        &mut harness,
        egui::Modifiers::CTRL | egui::Modifiers::SHIFT,
        egui::Key::R,
    );
    harness.event(egui::Event::Text("810".into()));
    harness.run();
    harness
        .get_by_role_and_label(egui::accesskit::Role::RadioButton, "Top")
        .focus();
    harness.run_steps(2);
    harness.key_press(egui::Key::Enter);
    harness.run_steps(2);
    assert!(
        harness.state().shell.dialogs.is_open(),
        "Enter picked the cell only"
    );
    harness.get_by_label("OK").click();
    harness.run_steps(2);
    assert_eq!(doc_size(&harness), (810, 600));
    let ws = harness.state().app.active_ws().unwrap();
    let data = ws.document.layers.get_at(0).unwrap().surface.data();
    // Top: centred across, dx = 10 / 2.
    assert_eq!((data[4 * 4 + 3], data[5 * 4 + 3]), (0, 255));
}

/// Task 28 (selection-and-move.md §2.11 step 3): with no image anywhere Paste shows a message; a
/// paste larger than the canvas asks Expand / Keep / Cancel, and Expand grows the canvas before
/// the paste floats in Move Selected Pixels.
#[test]
fn oversized_paste_asks_to_expand_the_canvas() {
    use egui::{Key, Modifiers};
    let mut harness = app_harness();
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    harness.key_press(Key::Escape);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
    harness
        .state_mut()
        .app
        .clipboard
        .set_external(pdn_core::canvas::ImageData::new(900, 700));
    harness.key_press_modifiers(Modifiers::CTRL, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    // The close button holds the focus; Tab reaches "Expand canvas", Enter picks it.
    harness.key_press(Key::Tab);
    harness.run();
    harness.key_press(Key::Enter);
    harness.run();
    let app = &harness.state().app;
    let doc = &app.active_ws().unwrap().document;
    assert_eq!((doc.width, doc.height), (900, 700));
    assert_eq!(app.tool_type, Some(pdn_core::tool::types::ToolType::Move));
    // Fix M1: Paste into New Layer adds the layer, then asks; Cancel undoes the layer, which
    // stays on the redo stack.
    harness.key_press(Key::Enter);
    harness
        .state_mut()
        .app
        .clipboard
        .set_external(pdn_core::canvas::ImageData::new(1000, 10));
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::SHIFT, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    let layers = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state().app.active_ws().unwrap().document.layers.size()
    };
    assert_eq!(layers(&harness), 2);
    harness.key_press(Key::Escape);
    harness.run();
    assert_eq!(layers(&harness), 1);
    let redo = &harness.state().app.active_ws().unwrap().history.redo;
    assert_eq!(
        redo.iter().map(|m| m.name.as_str()).collect::<Vec<_>>(),
        ["New Layer"]
    );
    // Fix M2: Paste into New Image with nothing to paste reports and opens no document.
    harness.state_mut().app.clipboard.item = None;
    harness.key_press_modifiers(Modifiers::CTRL | Modifiers::ALT, Key::V);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    assert_eq!(harness.state().app.documents.len(), 1);
}

/// Task 30: a Paintbrush drag (the startup tool) paints the stroke and shows it to its end — the
/// last frame's pixels too, though they change after the frame's event drain — and pushes one
/// "Paintbrush" entry; the outline follows the pointer.
#[test]
fn paintbrush_stroke_shows_to_its_end() {
    let mut harness = app_harness();
    drag(
        &mut harness,
        egui::pos2(350.0, 300.0),
        egui::pos2(700.0, 520.0),
        16,
    );
    harness.run();
    let app = &harness.state().app;
    let names: Vec<&str> = app
        .active_ws()
        .unwrap()
        .history
        .undo
        .iter()
        .map(|m| m.name.as_str())
        .collect();
    assert_eq!(names.last(), Some(&"Paintbrush"));
    let image = harness.render().expect("render");
    // The last segment runs from (678, 506) to (700, 520): its middle, then the stroke's middle.
    let darkest = |x0: u32, y0: u32| {
        (x0..x0 + 4)
            .flat_map(|x| (y0..y0 + 4).map(move |y| (x, y)))
            .map(|(x, y)| image.get_pixel(x, y)[0])
            .min()
            .unwrap()
    };
    assert!(darkest(689, 512) < 100, "{}", darkest(689, 512));
    assert!(darkest(524, 408) < 100, "{}", darkest(524, 408));
    assert!(darkest(520, 300) > 200);
    let outline = harness
        .state()
        .app
        .active_ws()
        .unwrap()
        .surface_box
        .renderers
        .iter()
        .find_map(|(_, r)| match r {
            pdn_core::render::Renderer::BrushPreview(p) => Some(*p),
            _ => None,
        })
        .expect("the brush outline");
    assert_eq!((outline.radius, outline.opacity > 0.0), (1.0, true));
}

/// painting-tools-1.md §2 "Brush size controls": the brush size + button steps 1, 5 with Ctrl
/// (its tip says so); − likewise.
#[test]
fn brush_size_buttons_step_five_with_ctrl() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    let width = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .tool_settings
            .get(ToolType::PaintBrush, "width")
    };
    let press = |h: &mut Harness<'_, paintrs::PaintApp>, x: f32, m: egui::Modifiers| {
        let pos = egui::pos2(x, 71.0);
        h.event_modifiers(egui::Event::PointerMoved(pos), m);
        h.run();
        for pressed in [true, false] {
            h.event_modifiers(
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: m,
                },
                m,
            );
            h.run();
        }
    };
    press(&mut harness, 257.0, egui::Modifiers::CTRL);
    assert_eq!(width(&harness), serde_json::json!(7));
    press(&mut harness, 257.0, egui::Modifiers::NONE);
    assert_eq!(width(&harness), serde_json::json!(8));
    press(&mut harness, 157.0, egui::Modifiers::CTRL);
    assert_eq!(width(&harness), serde_json::json!(3));
}

/// painting-tools-1.md §2 "Slider steps": the hardness, spacing and tolerance meters' − / +
/// step 1 %, or 5 % with Ctrl, the result rounded half away from zero to 0.01 (a whole %).
#[test]
fn slider_buttons_step_five_with_ctrl_and_round() {
    use pdn_core::tool::types::ToolType;
    let brush = ToolType::PaintBrush;
    let mut h = harness_at(1.0);
    let get =
        |h: &Harness<'_, paintrs::PaintApp>, k: &str| h.state().app.tool_settings.get(brush, k);
    let press = |h: &mut Harness<'_, paintrs::PaintApp>, pos: egui::Pos2, m: egui::Modifiers| {
        h.event_modifiers(egui::Event::PointerMoved(pos), m);
        h.run();
        for pressed in [true, false] {
            h.event_modifiers(
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: m,
                },
                m,
            );
            h.run();
        }
    };
    let buttons = |h: &Harness<'_, paintrs::PaintApp>, k: &str| {
        let c = tool_option_control(h, brush, k, 1.0);
        let y = c.center().y;
        (egui::pos2(c.min.x + 11.0, y), egui::pos2(c.max.x - 11.0, y))
    };
    let (minus, plus) = buttons(&h, "spacing");
    assert_eq!(get(&h, "spacing"), serde_json::json!(15));
    press(&mut h, plus, egui::Modifiers::CTRL);
    assert_eq!(get(&h, "spacing"), serde_json::json!(20));
    press(&mut h, minus, egui::Modifiers::NONE);
    assert_eq!(get(&h, "spacing"), serde_json::json!(19));
    press(&mut h, minus, egui::Modifiers::CTRL);
    assert_eq!(get(&h, "spacing"), serde_json::json!(14));
    h.state_mut()
        .app
        .tool_settings
        .set(brush, "hardness", serde_json::json!(50.5));
    h.run();
    let (_, plus) = buttons(&h, "hardness");
    press(&mut h, plus, egui::Modifiers::NONE);
    assert_eq!(
        get(&h, "hardness"),
        serde_json::json!(52),
        "51.5 rounds away from 0"
    );
}

/// The active document's history names.
fn history_names(harness: &Harness<'_, paintrs::PaintApp>) -> Vec<String> {
    let ws = harness.state().app.active_ws().unwrap();
    ws.history.undo.iter().map(|m| m.name.clone()).collect()
}

fn press(harness: &mut Harness<'_, paintrs::PaintApp>, pos: egui::Pos2, pressed: bool) {
    harness.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    });
    harness.run();
}

/// Task 31 (painting-tools-1.md §8.2, §8.6, R81): Ctrl+Z mid-stroke reaches Undo, which does
/// nothing while the canvas holds the button; losing the window focus then commits the stroke
/// as one entry (the synthesized release), never cancels it.
#[test]
fn ctrl_z_mid_stroke_is_ignored_and_focus_loss_commits() {
    let mut harness = app_harness();
    let n = history_names(&harness).len();
    // A first stroke, so that an undo would have something to step.
    drag(
        &mut harness,
        egui::pos2(400.0, 200.0),
        egui::pos2(600.0, 200.0),
        4,
    );
    assert_eq!(history_names(&harness)[n..], ["Paintbrush"]);
    let n = n + 1;
    let (a, b) = (egui::pos2(400.0, 300.0), egui::pos2(600.0, 400.0));
    harness.event(egui::Event::PointerMoved(a));
    harness.run();
    press(&mut harness, a, true);
    harness.event(egui::Event::PointerMoved(b));
    harness.run();
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    harness.run();
    assert_eq!(history_names(&harness).len(), n);
    assert!(harness.state().app.canvas_captured());
    harness.event(egui::Event::WindowFocused(false));
    harness.run();
    assert_eq!(history_names(&harness)[n..], ["Paintbrush"]);
}

/// §8.5/§8.11 "Dialog first": a modal dialog opened mid-stroke (Layer Properties, F4; Resize,
/// Ctrl+R, which keeps the tool active) takes the capture, and the stroke ends as a release (one
/// entry); the later real release does nothing.
#[test]
fn a_dialog_opened_mid_stroke_commits_it() {
    for (mods, key) in [
        (egui::Modifiers::NONE, egui::Key::F4),
        (egui::Modifiers::CTRL, egui::Key::R),
    ] {
        let mut harness = app_harness();
        let n = history_names(&harness).len();
        let (a, b) = (egui::pos2(400.0, 300.0), egui::pos2(600.0, 400.0));
        harness.event(egui::Event::PointerMoved(a));
        harness.run();
        press(&mut harness, a, true);
        harness.event(egui::Event::PointerMoved(b));
        harness.run();
        harness.key_press_modifiers(mods, key);
        harness.run();
        harness.run();
        assert!(harness.state().shell.dialogs.is_open(), "{key:?}");
        assert_eq!(history_names(&harness)[n..], ["Paintbrush"], "{key:?}");
        assert_eq!(harness.state().shell.canvas_view.captured(), None);
        press(&mut harness, b, false);
        assert_eq!(history_names(&harness)[n..], ["Paintbrush"], "{key:?}");
    }
}

/// §3.10 step 2: a Clone Stamp click with no anchor opens the error box after the release;
/// Escape closes it.
#[test]
fn clone_stamp_without_an_anchor_shows_the_error_box() {
    let mut harness = app_harness();
    harness
        .state_mut()
        .app
        .set_active_tool_from_type(pdn_core::tool::ToolType::CloneStamp);
    harness.run();
    let pos = egui::pos2(500.0, 350.0);
    harness.event(egui::Event::PointerMoved(pos));
    harness.run();
    press(&mut harness, pos, true);
    assert!(!harness.state().shell.dialogs.is_open());
    press(&mut harness, pos, false);
    harness.run();
    assert!(harness.state().shell.dialogs.is_open());
    harness.key_press(egui::Key::Escape);
    harness.run();
    assert!(!harness.state().shell.dialogs.is_open());
}

/// The canvas of every frame of a Move Selected Pixels drag over a sparse selection on a noise
/// image (a press, six moves, a release elsewhere), refreshing all of it before each frame
/// (`full`) or not.
fn sparse_move_frames(full: bool) -> Vec<Vec<[u8; 4]>> {
    use pdn_core::selection::CombineMode;
    use pdn_core::selection::path::GraphicsPath;
    use pdn_core::tool::types::ToolType;
    use pdn_core::util::geom::Point;
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    {
        let app = &mut harness.state_mut().app;
        let ws = app.active_ws_mut().unwrap();
        let (w, h) = (ws.document.width, ws.document.height);
        let layer = ws.document.layers.get_at_mut(0).unwrap();
        for (i, p) in layer.surface.data_mut().chunks_mut(4).enumerate() {
            let v = (i as u32).wrapping_mul(2_654_435_761);
            p.copy_from_slice(&[v as u8, (v >> 8) as u8, (v >> 16) as u8, 255]);
        }
        ws.invalidate(None);
        let mut cells = GraphicsPath::default();
        for k in 0..(w * h / 400) {
            let (x, y) = ((k * 7919 % w) as f64, (k * 104_729 % h) as f64);
            let square = [(x, y), (x + 1.0, y), (x + 1.0, y + 1.0), (x, y + 1.0)];
            cells
                .polygons
                .push(square.map(|(x, y)| Point::new(x, y)).to_vec());
        }
        ws.selection
            .set_continuation_path(cells, CombineMode::Replace);
        ws.selection.commit_continuation();
        app.set_active_tool_from_type(ToolType::Move);
    }
    harness.run_steps(2);
    let canvas_bottom = 800 - paintrs::theme::FOOTER_HEIGHT as u32;
    let mut shots = vec![];
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let at = |i: usize| egui::pos2(500.0 + 3.0 * i as f32, 350.0 + i as f32);
    let mut events = vec![
        vec![egui::Event::PointerMoved(at(0))],
        vec![button(at(0), true)],
    ];
    events.extend((1..=6).map(|i| vec![egui::Event::PointerMoved(at(i))]));
    events.push(vec![egui::Event::PointerMoved(at(8)), button(at(8), false)]);
    for frame in events {
        frame.into_iter().for_each(|e| harness.event(e));
        if full {
            harness.state_mut().shell.canvas_view.refresh_all();
        }
        harness.run_steps(1);
        let image = harness.render().expect("render");
        let rows = image.rows().take(canvas_bottom as usize);
        shots.push(rows.flat_map(|r| r.map(|p| p.0)).collect());
    }
    shots
}

/// Task 27r: a Move drag redraws only what each frame changed (`CanvasPixels`); every frame,
/// the release's included (it shows the last move in its own frame), equals a full refresh byte
/// for byte.
#[test]
fn move_drag_frames_match_a_full_refresh() {
    let (frames, full) = (sparse_move_frames(false), sparse_move_frames(true));
    assert_eq!(frames.len(), full.len());
    for (i, (a, b)) in frames.iter().zip(&full).enumerate() {
        let stale = a.iter().zip(b).filter(|(a, b)| a != b).count();
        assert_eq!(stale, 0, "frame {i}: stale pixels");
    }
    let moved = frames.windows(2).filter(|w| w[0] != w[1]).count();
    assert!(moved >= 6, "the pixels moved in {moved} frames");
}

/// BB plan BB5: a plugin dialog (common.md §3) with a tab strip, a wrapping hue slider
/// without its reset arrow, a slider with a document-sized default, a check box gated by another,
/// a colour "with alpha", single- and multi-line text boxes, a captioned layer drop-down, a
/// captioned drop-down, a pan control and a current-layer label with a footnote.
mod bb5_probe {
    use pdn_core::effect::bb::ui::{ControlUi, IndirectUi, Note, TRACK_HUE, Tab, TabStrip};
    use pdn_core::effect::{ColorDefault, Control, EffectDef, LabelText, PanThumb, Values};

    fn gated_by_flag(v: &Values, key: &str) -> bool {
        key != "gated" || pdn_core::effect::flag(v, "flag")
    }

    /// artistic-dream.md "Default Radius": a default from the document's size (800 / 8).
    fn sized(v: &mut Values, env: &pdn_core::effect::EffectEnv<'_>) {
        v.insert("radius".into(), serde_json::json!(env.width / 8));
    }

    static UI: IndirectUi = IndirectUi {
        width: 1.1,
        tabs: &[TabStrip {
            key: "tab",
            tabs: &[
                Tab {
                    caption: "First",
                    keys: &["amount", "radius", "flag", "gated", "tint"],
                },
                Tab {
                    caption: "Second",
                    keys: &["words", "story", "layer", "mode", "spot"],
                },
                Tab {
                    caption: "Third",
                    keys: &[],
                },
            ],
        }],
        controls: &[
            (
                "amount",
                ControlUi {
                    track: Some(&TRACK_HUE),
                    wrap: true,
                    reset: false,
                    ..ControlUi::NO_HEADER
                },
            ),
            (
                "tint",
                ControlUi {
                    alpha: true,
                    reset: false,
                    ..ControlUi::DEFAULT
                },
            ),
            (
                "layer",
                ControlUi {
                    caption: "Layer  ",
                    ..ControlUi::DEFAULT
                },
            ),
            (
                "mode",
                ControlUi {
                    caption: "Mode",
                    ..ControlUi::DEFAULT
                },
            ),
            (
                "info",
                ControlUi {
                    note: Note::Text("A footnote."),
                    ..ControlUi::DEFAULT
                },
            ),
        ],
        enabled: gated_by_flag,
        defaults: sized,
        ..IndirectUi::DEFAULT
    };

    pub fn def() -> EffectDef {
        EffectDef {
            id: "bbProbe",
            dialog_title: Some("Probe"),
            controls: vec![
                Control::Value {
                    key: "tab",
                    default: || serde_json::json!(0),
                },
                pdn_core::effect::range("amount", "Amount", 0.0, 360.0, 360.0),
                pdn_core::effect::range("radius", "Radius", 0.0, 200.0, 7.0),
                Control::Checkbox {
                    key: "flag",
                    label: "Flag",
                    default: false,
                },
                Control::Checkbox {
                    key: "gated",
                    label: "Gated",
                    default: false,
                },
                Control::Color {
                    key: "tint",
                    label: "Tint",
                    default: ColorDefault::Rgb(0),
                },
                Control::Text {
                    key: "words",
                    label: "Words",
                    default: "hi",
                    max_len: 5,
                    multiline: false,
                },
                Control::Text {
                    key: "story",
                    label: "Story",
                    default: "",
                    max_len: 100,
                    multiline: true,
                },
                Control::LayerSelect {
                    key: "layer",
                    label: "",
                },
                Control::Select {
                    key: "mode",
                    label: "",
                    default: "a",
                    choices: &[("a", "Alpha"), ("b", "Beta")],
                },
                Control::Pan {
                    key: "spot",
                    label: "Spot",
                    min: -1.0,
                    max: 1.0,
                    default: [0.0, 0.0],
                    step: 0.25,
                    thumb: PanThumb::Source,
                },
                Control::Label {
                    key: "info",
                    label: "",
                    text: LabelText::CurrentLayer("Current layer: "),
                },
            ],
            indirect: Some(&UI),
            ..pdn_core::effect::find("invertColorsEffect").unwrap()
        }
    }
}

/// Control assertions need queued input and its layout frame, not preview-worker quiescence.
fn probe_frames(harness: &mut Harness<'_, paintrs::PaintApp>) {
    harness.run_steps(2);
    harness.check_accessibility();
}

fn probe_click(harness: &mut Harness<'_, paintrs::PaintApp>, x: f32, y: f32) {
    let pos = egui::pos2(x, y);
    harness.event(egui::Event::PointerMoved(pos));
    probe_frames(harness);
    for pressed in [true, false] {
        harness.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        });
        probe_frames(harness);
    }
}

/// Opens the BB5 probe's dialog over the active layer.
fn open_probe(harness: &mut Harness<'_, paintrs::PaintApp>) {
    let state = harness.state_mut();
    let s = pdn_core::effect::session::EffectSession::begin(&mut state.app, bb5_probe::def())
        .expect("a document");
    let values = s.initial_values(&state.app);
    state.shell.dialogs.push(Box::new(
        paintrs::dialogs::effect_config::EffectConfigDialog::new(s, values),
    ));
    probe_frames(harness);
    assert!(harness.state().shell.dialogs.is_open());
}

/// OK, then the probe's remembered values.
fn probe_ok(harness: &mut Harness<'_, paintrs::PaintApp>) -> pdn_core::effect::Values {
    harness.key_press(egui::Key::Enter);
    probe_frames(harness);
    assert!(!harness.state().shell.dialogs.is_open(), "OK closed it");
    harness.state().app.effect_tokens["bbProbe"].clone()
}

/// The probe's tab `caption` is the selected one.
fn selected(h: &Harness<'_, paintrs::PaintApp>, caption: &str) -> bool {
    use egui_kittest::kittest::Queryable;
    h.query_by(|n| {
        n.role() == egui::accesskit::Role::Tab
            && n.label().as_deref() == Some(caption)
            && n.is_selected() == Some(true)
    })
    .is_some()
}

/// common.md §3 "Width" (×1.1), "Tabs" (a click picks the page, stored as a setting; ruling R83:
/// the page area keeps the tallest page's height, so neither the dialog nor the strip moves),
/// "Enable rules" (a disabled control ignores the pointer and keeps its value).
#[test]
fn plugin_dialog_tabs_rules_and_width() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    let mut harness = app_harness();
    open_probe(&mut harness);
    let rect = harness
        .state()
        .shell
        .dialogs
        .top_rect(&harness.ctx)
        .unwrap();
    assert!((rect.width() - 429.0).abs() < 0.5, "{}", rect.width());
    let strip = harness.get_by_label("Second").rect();
    assert!(selected(&harness, "First") && !selected(&harness, "Second"));
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "amount")
            .is_some()
    );
    assert!(harness.query_by_label("Words").is_none());
    // Gated is disabled until Flag is checked.
    harness.get_by_label("Gated").click();
    probe_frames(&mut harness);
    harness.get_by_label("Flag").click();
    probe_frames(&mut harness);
    // The check box's AccessKit node carries its state.
    let checked = |h: &Harness<'_, paintrs::PaintApp>, label: &str| {
        h.get_by_role_and_label(Role::CheckBox, label)
            .accesskit_node()
            .toggled()
    };
    assert_eq!(
        (checked(&harness, "Flag"), checked(&harness, "Gated")),
        (
            Some(egui::accesskit::Toggled::True),
            Some(egui::accesskit::Toggled::False)
        )
    );
    harness.get_by_label("Gated").click();
    probe_frames(&mut harness);
    harness.get_by_label("Second").click();
    probe_frames(&mut harness);
    assert!(selected(&harness, "Second"));
    assert!(
        harness
            .query_by_role_and_label(Role::Slider, "amount")
            .is_none()
    );
    assert!(harness.query_by_label("Words").is_some());
    assert_eq!(
        harness.state().shell.dialogs.top_rect(&harness.ctx),
        Some(rect),
        "the dialog keeps its size and place"
    );
    assert_eq!(
        harness.get_by_label("Second").rect(),
        strip,
        "the strip stays"
    );
    // Enter on the focused tab is the dialog's OK.
    harness.get_by_label("Second").focus();
    probe_frames(&mut harness);
    assert!(harness.get_by_label("Second").is_focused());
    let token = probe_ok(&mut harness);
    assert_eq!(
        (&token["tab"], &token["flag"], &token["gated"]),
        (
            &serde_json::json!(1),
            &serde_json::json!(true),
            &serde_json::json!(true)
        )
    );
}

/// common.md §3 "Text box" (capped at its length; a multi-line one takes Enter as a line break and
/// scrolls past its height),
/// "Static label", the layer drop-down (bottom layer first, its caption beside it) and "Pan" (a
/// press sets the value, clamped; the X / Y boxes step by the control's step).
#[test]
fn plugin_text_layer_and_pan_rows() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    pdn_core::action::perform(&mut harness.state_mut().app, "menu.layers.add.new.layer");
    probe_frames(&mut harness);
    let top_name = {
        let ws = harness.state().app.active_ws().unwrap();
        assert_eq!(ws.active_layer, 1);
        ws.document
            .layers
            .get_at(1)
            .unwrap()
            .properties
            .name
            .clone()
    };
    open_probe(&mut harness);
    harness.get_by_label("Second").click();
    probe_frames(&mut harness);
    harness.get_by_label("Words").click();
    probe_frames(&mut harness);
    harness.get_by_label("Words").type_text("abcdefgh");
    probe_frames(&mut harness);
    harness.get_by_label("Story").click();
    probe_frames(&mut harness);
    harness.get_by_label("Story").type_text("a");
    probe_frames(&mut harness);
    harness.key_press(egui::Key::Enter);
    probe_frames(&mut harness);
    assert!(
        harness.state().shell.dialogs.is_open(),
        "Enter was a line break"
    );
    harness.get_by_label("Story").type_text("b");
    probe_frames(&mut harness);
    // Past its 4 lines the box scrolls: the text's last line (the caret's) stays inside it.
    for l in ["c", "d", "e", "f"] {
        harness.key_press(egui::Key::Enter);
        probe_frames(&mut harness);
        harness.get_by_label("Story").type_text(l);
        probe_frames(&mut harness);
    }
    let below = harness.get_by_label("Layer").rect().min.y - 10.0;
    let text = harness.get_by_label("Story").rect();
    assert!(
        text.max.y <= below + 0.5 && text.height() > 60.0,
        "scrolled to its end: {text:?}, the box ends at {below}"
    );
    harness.get_by_label("Layer").click();
    probe_frames(&mut harness);
    let (bottom, top) = (
        harness.get_by_label("Background").rect(),
        harness.get_by_label(&top_name).rect(),
    );
    assert!(bottom.min.y < top.min.y, "bottom layer first");
    harness.get_by_label(&top_name).click();
    probe_frames(&mut harness);
    let spot = harness.get_by_label("Spot").rect();
    assert!(
        harness.get_by_label("OK").rect().min.y > spot.max.y + 30.0,
        "the dialog fits its rows (the label and its footnote below the box)"
    );
    probe_click(&mut harness, spot.min.x + 1.0, spot.min.y + 1.0);
    // The X box (right of the box, after its label), stepped by the control's 0.25.
    let x_box = egui::pos2(
        spot.max.x + 1.0 + 10.0 + 16.0 + 36.0,
        spot.min.y - 1.0 + 9.0 + 12.5,
    );
    probe_click(&mut harness, x_box.x, x_box.y);
    harness.key_press(egui::Key::ArrowUp);
    probe_frames(&mut harness);
    let token = probe_ok(&mut harness);
    assert_eq!(token["words"], serde_json::json!("hiabc"));
    assert_eq!(token["story"], serde_json::json!("a\nb\nc\nd\ne\nf"));
    assert_eq!(token["layer"], serde_json::json!(1));
    assert_eq!(token["spot"], serde_json::json!([-0.75, -1.0]));
}

/// common.md §3 "Slider" (a range that wraps steps past its end to the other end; "no reset
/// button"; the reset arrow restores the session's default, artistic-dream.md "Default Radius")
/// and "Colour wheel … with alpha" (the pick keeps the colour's alpha).
#[test]
fn plugin_slider_wraps_without_reset_and_colour_keeps_alpha() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    harness.state_mut().app.primary_color =
        pdn_core::util::color::Color::new(0x10, 0x20, 0x30, 0x80);
    open_probe(&mut harness);
    let slider = harness.get_by_role_and_label(Role::Slider, "amount").rect();
    // `range_rects`: the number box 8 px right of the slider's box (its 2-px margin, the 10-px
    // gap), 72 wide; the reset arrow 10 px after it, 24 wide.
    let number = egui::pos2(slider.max.x + 8.0 + 36.0, slider.center().y);
    probe_click(&mut harness, number.x, number.y);
    harness.key_press(egui::Key::ArrowUp);
    probe_frames(&mut harness);
    let reset = egui::pos2(slider.max.x + 8.0 + 72.0 + 10.0 + 12.0, slider.center().y);
    probe_click(&mut harness, reset.x, reset.y);
    let radius = harness.get_by_role_and_label(Role::Slider, "radius").rect();
    probe_click(&mut harness, radius.max.x + 8.0 + 36.0, radius.center().y);
    harness.key_press(egui::Key::ArrowUp);
    probe_frames(&mut harness);
    let reset = egui::pos2(radius.max.x + 8.0 + 72.0 + 10.0 + 12.0, radius.center().y);
    probe_click(&mut harness, reset.x, reset.y);
    harness.get_by_label("Tint").click();
    probe_frames(&mut harness);
    harness.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::A);
    harness.event(egui::Event::Text("10203080".into()));
    probe_frames(&mut harness);
    harness.key_press(egui::Key::Enter);
    probe_frames(&mut harness);
    harness.get_by_label("OK").click();
    probe_frames(&mut harness);
    assert!(!harness.state().shell.dialogs.is_open());
    let token = harness.state().app.effect_tokens["bbProbe"].clone();
    assert_eq!(token["amount"], serde_json::json!(0.0));
    assert_eq!(token["radius"], serde_json::json!(100.0));
    assert_eq!(token["tint"], serde_json::json!("#10203080"));
}

/// BB plan BB5 (paint.rs's own look): a disabled row's control is faded, a track's colours
/// are drawn, a row without its header line has no rule, and a colour with alpha shows checks.
#[test]
fn plugin_dialog_pixels() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    harness.state_mut().app.primary_color =
        pdn_core::util::color::Color::new(0x10, 0x80, 0x30, 0x80);
    open_probe(&mut harness);
    harness.state_mut().app.secondary_color = harness.state().app.primary_color;
    let at = harness.get_by_label("Tint").rect().center();
    tap(&mut harness, at.x, at.y, egui::PointerButton::Secondary);
    probe_frames(&mut harness);
    let theme = paintrs::theme::current(&harness.ctx);
    let image = harness.render().expect("render");
    let px = |p: egui::Pos2| pixel(&image, p.x, p.y);
    let (flag, gated) = (
        harness.get_by_label("Flag").rect(),
        harness.get_by_label("Gated").rect(),
    );
    let box_centre = |r: egui::Rect| egui::pos2(r.min.x + 8.0, r.center().y);
    assert_ne!(
        px(box_centre(flag)),
        px(box_centre(gated)),
        "Gated is faded"
    );
    let slider = harness.get_by_role_and_label(Role::Slider, "amount").rect();
    let [r, g, b] = px(egui::pos2(slider.min.x + 3.0, slider.center().y));
    assert!(
        r > 200 && g < 60 && b < 60,
        "the hue track starts red: {r} {g} {b}"
    );
    let separator = [
        theme.pdn_separator.r(),
        theme.pdn_separator.g(),
        theme.pdn_separator.b(),
    ];
    let strip = harness.get_by_label("First").rect();
    for y in strip.max.y as u32 + 2..slider.min.y as u32 {
        assert_ne!(
            px(egui::pos2(slider.center().x, y as f32)),
            separator,
            "no header line under Amount (y {y})"
        );
    }
    let tint = harness.get_by_label("Tint").rect();
    assert_ne!(
        px(tint.min + egui::vec2(8.0, 9.0)),
        px(tint.min + egui::vec2(12.0, 9.0)),
        "the alpha swatch shows checks"
    );
    // Ruling R84: Tint's header line runs right of its label, not under it.
    let rule_at = |x: f32| {
        (tint.min.y as u32 - 25..tint.min.y as u32)
            .any(|y| px(egui::pos2(x, y as f32)) == separator)
    };
    assert!(rule_at(tint.min.x + 200.0), "the rule right of \"Tint\"");
    assert!(!rule_at(tint.min.x + 2.0), "no rule under \"Tint\"");
}

/// BB5 review: a WinForms tab strip on the keyboard. Only the selected tab is a Tab stop; there
/// ArrowRight / ArrowLeft pick the next / previous tab (stopping at the ends) and keep the focus on
/// it; Ctrl+Tab / Ctrl+Shift+Tab cycle forwards / backwards from anywhere in the dialog.
#[test]
fn plugin_tabs_take_the_keyboard() {
    use egui_kittest::kittest::Queryable;
    let mut harness = app_harness();
    open_probe(&mut harness);
    let rect = harness
        .state()
        .shell
        .dialogs
        .top_rect(&harness.ctx)
        .unwrap();
    harness.get_by_label("Second").focus();
    probe_frames(&mut harness);
    assert!(
        !harness.get_by_label("Second").is_focused(),
        "not a Tab stop"
    );
    harness.get_by_label("First").focus();
    probe_frames(&mut harness);
    assert!(harness.get_by_label("First").is_focused());
    harness.key_press(egui::Key::ArrowRight);
    probe_frames(&mut harness);
    assert!(selected(&harness, "Second"));
    assert!(
        harness.get_by_label("Second").is_focused(),
        "the focus follows"
    );
    for _ in 0..2 {
        harness.key_press(egui::Key::ArrowRight);
        probe_frames(&mut harness);
    }
    assert!(selected(&harness, "Third"), "the last tab stays");
    assert_eq!(
        harness.state().shell.dialogs.top_rect(&harness.ctx),
        Some(rect),
        "an empty page keeps the page area too"
    );
    for _ in 0..2 {
        harness.key_press(egui::Key::ArrowLeft);
        probe_frames(&mut harness);
    }
    assert!(selected(&harness, "First"));
    // Ctrl+Tab from a control on the page (eframe sets `command` with Ctrl off macOS).
    let ctrl = egui::Modifiers::CTRL | egui::Modifiers::COMMAND;
    harness.get_by_label("Flag").click();
    probe_frames(&mut harness);
    harness.key_press_modifiers(ctrl | egui::Modifiers::SHIFT, egui::Key::Tab);
    probe_frames(&mut harness);
    assert!(selected(&harness, "Third"), "Ctrl+Shift+Tab cycles back");
    harness.key_press_modifiers(ctrl, egui::Key::Tab);
    probe_frames(&mut harness);
    assert!(selected(&harness, "First"), "Ctrl+Tab cycles");
    harness.key_press_modifiers(ctrl, egui::Key::Tab);
    probe_frames(&mut harness);
    assert!(selected(&harness, "Second"));
    let token = probe_ok(&mut harness);
    assert_eq!(
        (&token["tab"], &token["flag"]),
        (&serde_json::json!(1), &serde_json::json!(true))
    );
}

/// Model finite late worker repaints while real probe interactions remain observable.
#[test]
fn plugin_probe_controls_with_worker_repaints() {
    use egui_kittest::kittest::{NodeT, Queryable};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let mut harness = app_harness();
    open_probe(&mut harness);
    let remaining = Arc::new(AtomicUsize::new(32));
    let worker_frames = remaining.clone();
    harness.ctx.on_end_pass(
        "probe worker repaint",
        Arc::new(move |ui| {
            let left = worker_frames.load(Ordering::Relaxed);
            if left > 0 {
                worker_frames.store(left - 1, Ordering::Relaxed);
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || ctx.request_repaint())
                    .join()
                    .unwrap();
            }
        }),
    );
    harness.get_by_label("Flag").click();
    probe_frames(&mut harness);
    assert_eq!(
        harness.get_by_label("Flag").accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    harness.get_by_label("First").focus();
    probe_frames(&mut harness);
    assert!(harness.get_by_label("First").is_focused());
    harness.key_press(egui::Key::ArrowRight);
    probe_frames(&mut harness);
    assert!(selected(&harness, "Second"));
    assert!(harness.get_by_label("Second").is_focused());
    let token = probe_ok(&mut harness);
    assert_eq!(token["tab"], serde_json::json!(1));
    assert_eq!(token["flag"], serde_json::json!(true));
    assert!(
        (1..32).contains(&remaining.load(Ordering::Relaxed)),
        "worker repaints remained active through the assertions"
    );
}

// Five-button input cases use document pixels and history, without a GPU screenshot.
fn buttons_harness() -> Harness<'static, paintrs::PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .build_eframe(new_web_chrome_app);
    h.run();
    h.state_mut().clock = paintrs::Clock::Virtual(0.0);
    let brush = pdn_core::tool::ToolType::PaintBrush;
    for (key, value) in [
        ("width", serde_json::json!(4)),
        ("hardness", serde_json::json!(100)),
        ("smoothing", serde_json::json!(false)),
        ("antialias", serde_json::json!(false)),
    ] {
        h.state_mut().app.tool_settings.set(brush, key, value);
    }
    h
}

fn button_event(pos: egui::Pos2, button: egui::PointerButton, pressed: bool) -> egui::Event {
    egui::Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn canvas_button(
    h: &mut Harness<'_, paintrs::PaintApp>,
    pos: egui::Pos2,
    button: egui::PointerButton,
    pressed: bool,
) {
    h.event(button_event(pos, button, pressed));
    h.run();
}

fn canvas_move(h: &mut Harness<'_, paintrs::PaintApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run();
}

fn canvas_bytes(h: &Harness<'_, paintrs::PaintApp>) -> Vec<u8> {
    h.state()
        .app
        .active_ws()
        .unwrap()
        .document
        .layers
        .get_at(0)
        .unwrap()
        .surface
        .data()
        .to_vec()
}

fn ink_count(h: &Harness<'_, paintrs::PaintApp>) -> usize {
    canvas_bytes(h).chunks(4).filter(|p| *p != [255; 4]).count()
}

/// painting-tools-1 §8.7: Middle/X1/X2 pressed during painting do not steal the gesture;
/// their release commits one entry, and later motion with Left held paints no more pixels.
#[test]
fn canvas_buttons_chord_release_stops_painting() {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary};
    for chord in [Middle, Extra1, Extra2] {
        let mut h = buttons_harness();
        h.state_mut()
            .app
            .active_ws_mut()
            .unwrap()
            .view
            .set_zoom_center(1.0);
        h.run();
        let n = history_names(&h).len();
        let (a, b, c, d) = (
            egui::pos2(400.0, 300.0),
            egui::pos2(500.0, 300.0),
            egui::pos2(600.0, 300.0),
            egui::pos2(700.0, 400.0),
        );
        canvas_move(&mut h, a);
        canvas_button(&mut h, a, Primary, true);
        canvas_move(&mut h, b);
        let before_chord = ink_count(&h);
        let v = &h.state().app.active_ws().unwrap().view;
        let viewport = (v.viewport_x, v.viewport_y);
        canvas_button(&mut h, b, chord, true);
        canvas_move(&mut h, c);
        assert!(
            ink_count(&h) > before_chord,
            "{chord:?} must not stop the stroke at press"
        );
        let v = &h.state().app.active_ws().unwrap().view;
        assert_eq!((v.viewport_x, v.viewport_y), viewport, "no Middle takeover");
        canvas_button(&mut h, c, chord, false);
        assert_eq!(&history_names(&h)[n..], ["Paintbrush"], "{chord:?}");
        let stopped = canvas_bytes(&h);
        canvas_move(&mut h, d);
        canvas_button(&mut h, d, Primary, false);
        assert!(
            canvas_bytes(&h) == stopped,
            "{chord:?} release must end drawing"
        );
        assert_eq!(&history_names(&h)[n..], ["Paintbrush"]);
        assert!(!h.state().app.canvas_captured());
    }
}

/// §8.7: all three buttons remain independently captured. Releasing the original Left
/// ends drawing; X1 and X2 still block Undo and tool letters until each is physically up.
#[test]
fn canvas_buttons_three_buttons_keep_guards_until_last_release() {
    use egui::PointerButton::{Extra1, Extra2, Primary};
    use pdn_core::tool::ToolType;
    let mut h = buttons_harness();
    let n = history_names(&h).len();
    let (a, b) = (egui::pos2(400.0, 300.0), egui::pos2(550.0, 300.0));
    canvas_move(&mut h, a);
    canvas_button(&mut h, a, Primary, true);
    canvas_move(&mut h, b);
    canvas_button(&mut h, b, Extra1, true);
    canvas_button(&mut h, b, Extra2, true);
    canvas_button(&mut h, b, Primary, false);
    let stopped = canvas_bytes(&h);
    for button in [Extra1, Extra2] {
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
        h.key_press(egui::Key::P);
        h.run();
        assert_eq!(h.state().app.tool_type, Some(ToolType::PaintBrush));
        assert_eq!(&history_names(&h)[n..], ["Paintbrush"]);
        assert!(h.state().app.canvas_captured());
        canvas_move(&mut h, egui::pos2(650.0, 400.0));
        assert!(canvas_bytes(&h) == stopped);
        canvas_button(&mut h, b, button, false);
    }
    assert!(!h.state().app.canvas_captured());
    h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
    h.run();
    assert_eq!(
        ink_count(&h),
        0,
        "Undo works after the last physical release"
    );
    assert_eq!(history_names(&h).len(), n);
    h.key_press(egui::Key::P);
    h.run();
    assert_eq!(h.state().app.tool_type, Some(ToolType::Pencil));
}

/// §8.7: a first X1 or X2 paints with Primary. Its down, moves, and up may all be in one
/// egui frame; the initial dab and complete line still become one undoable stroke.
#[test]
fn canvas_buttons_extra_first_paints_primary_in_one_frame() {
    use egui::PointerButton::{Extra1, Extra2};
    use pdn_core::util::color::Color;
    for first in [Extra1, Extra2] {
        let mut h = buttons_harness();
        h.state_mut()
            .app
            .set_colors(Color::new(12, 34, 56, 255), Color::new(200, 0, 0, 255));
        let n = history_names(&h).len();
        let (a, b) = (egui::pos2(400.0, 300.0), egui::pos2(600.0, 300.0));
        canvas_move(&mut h, a);
        h.event(button_event(a, first, true));
        h.event(egui::Event::PointerMoved(b));
        h.event(button_event(b, first, false));
        h.run();
        let pixels = canvas_bytes(&h);
        assert!(
            pixels.chunks(4).filter(|p| *p == [12, 34, 56, 255]).count() > 100,
            "{first:?} must paint Primary"
        );
        assert!(!pixels.chunks(4).any(|p| p == [200, 0, 0, 255]));
        assert_eq!(&history_names(&h)[n..], ["Paintbrush"]);
        assert!(!h.state().app.canvas_captured());
        h.key_press_modifiers(egui::Modifiers::CTRL, egui::Key::Z);
        h.run();
        assert_eq!(ink_count(&h), 0);
    }
}

/// §8.7: a chord down/up in the same frame must end the stroke before later moves in that
/// frame, even though egui's final physical-button state has only Left down.
#[test]
fn canvas_buttons_same_frame_chord_release_stops_before_later_motion() {
    let mut h = buttons_harness();
    let (a, b, c) = (
        egui::pos2(400.0, 300.0),
        egui::pos2(500.0, 300.0),
        egui::pos2(700.0, 400.0),
    );
    canvas_move(&mut h, a);
    canvas_button(&mut h, a, egui::PointerButton::Primary, true);
    canvas_move(&mut h, b);
    let n = history_names(&h).len();
    let drawn = canvas_bytes(&h);
    h.event(button_event(b, egui::PointerButton::Extra2, true));
    h.event(button_event(b, egui::PointerButton::Extra2, false));
    h.event(egui::Event::PointerMoved(c));
    h.run();
    assert_eq!(&history_names(&h)[n..], ["Paintbrush"]);
    assert!(
        canvas_bytes(&h) == drawn,
        "a later same-frame move must not draw"
    );
    canvas_button(&mut h, c, egui::PointerButton::Primary, false);
}

/// §8.6/§8.11: loss releases all five held buttons, commits once and makes Undo available.
/// PointerGone is the capture-loss signal when a release happens outside the window.
#[test]
fn canvas_buttons_capture_loss_releases_all_five() {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary, Secondary};
    for loss in [egui::Event::WindowFocused(false), egui::Event::PointerGone] {
        let mut h = buttons_harness();
        let n = history_names(&h).len();
        let (a, b) = (egui::pos2(400.0, 300.0), egui::pos2(550.0, 300.0));
        canvas_move(&mut h, a);
        canvas_button(&mut h, a, Primary, true);
        canvas_move(&mut h, b);
        for button in [Secondary, Middle, Extra1, Extra2] {
            canvas_button(&mut h, b, button, true);
        }
        h.event(loss.clone());
        h.run();
        assert_eq!(&history_names(&h)[n..], ["Paintbrush"], "{loss:?}");
        assert!(!h.state().app.canvas_captured(), "{loss:?}");
        assert_eq!(h.state().shell.canvas_view.captured(), None);
        assert!(ink_count(&h) > 100);
        assert!(h.state_mut().app.undo());
        assert_eq!(ink_count(&h), 0);
    }
}

/// R82: image switches end an X-started stroke in its own image; both held extra buttons
/// keep capture in the new image until their actual releases, with no paint leaking there.
#[test]
fn canvas_buttons_image_switch_ends_extra_stroke_in_old_image() {
    use egui::PointerButton::{Extra1, Extra2};
    let mut h = buttons_harness();
    h.state_mut().app.documents[0].is_initial_workspace = false;
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(800, 600, 96.0, false);
    h.state_mut().app.set_active_document_workspace(Some(0));
    h.run();
    let n = history_names(&h).len();
    let (a, b, c) = (
        egui::pos2(400.0, 300.0),
        egui::pos2(550.0, 300.0),
        egui::pos2(650.0, 400.0),
    );
    canvas_move(&mut h, a);
    canvas_button(&mut h, a, Extra1, true);
    canvas_move(&mut h, b);
    canvas_button(&mut h, b, Extra2, true);
    h.state_mut().app.set_active_document_workspace(Some(1));
    h.run();
    assert_eq!(h.state().app.documents[0].history.undo.len(), n + 1);
    assert_eq!(
        h.state().app.documents[0].history.undo.last().unwrap().name,
        "Paintbrush"
    );
    assert!(!h.state_mut().app.undo());
    canvas_move(&mut h, c);
    canvas_button(&mut h, c, Extra1, false);
    assert!(!h.state_mut().app.undo());
    canvas_button(&mut h, c, Extra2, false);
    assert!(!h.state().app.canvas_captured());
    assert_eq!(ink_count(&h), 0, "the new image stays white");
    h.state_mut().app.set_active_document_workspace(Some(0));
    assert!(ink_count(&h) > 100);
    assert!(h.state_mut().app.undo());
    assert_eq!(ink_count(&h), 0);
}

/// §8.7 Color Picker: a second Right picks Secondary, Middle/X press nothing, and any first
/// release stops picking even while Left is still held. Picking adds no history.
#[test]
fn canvas_buttons_picker_chords_and_first_extra_do_not_misroute_colors() {
    use egui::PointerButton::{Extra1, Extra2, Middle, Primary, Secondary};
    use pdn_core::{tool::ToolType, util::color::Color};
    for chord in [Middle, Extra1, Extra2, Secondary] {
        let mut h = buttons_harness();
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::ColorPicker);
        let primary = Color::new(12, 34, 56, 255);
        let secondary = Color::new(200, 0, 0, 255);
        h.state_mut().app.set_colors(primary, secondary);
        let n = history_names(&h).len();
        let p = egui::pos2(500.0, 300.0);
        canvas_move(&mut h, p);
        canvas_button(&mut h, p, Primary, true);
        assert_eq!(h.state().app.primary_color, Color::WHITE);
        canvas_button(&mut h, p, chord, true);
        assert_eq!(
            h.state().app.secondary_color,
            if chord == Secondary {
                Color::WHITE
            } else {
                secondary
            }
        );
        canvas_button(&mut h, p, chord, false);
        h.state_mut().app.set_colors(primary, secondary);
        canvas_move(&mut h, egui::pos2(600.0, 300.0));
        assert_eq!(
            (h.state().app.primary_color, h.state().app.secondary_color),
            (primary, secondary)
        );
        assert!(!h.state_mut().app.undo());
        canvas_button(&mut h, p, Primary, false);
        assert_eq!(history_names(&h).len(), n);
    }
    for first in [Extra1, Extra2] {
        let mut h = buttons_harness();
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::ColorPicker);
        let colors = (Color::new(12, 34, 56, 255), Color::new(200, 0, 0, 255));
        h.state_mut().app.set_colors(colors.0, colors.1);
        let p = egui::pos2(500.0, 300.0);
        canvas_move(&mut h, p);
        canvas_button(&mut h, p, first, true);
        canvas_move(&mut h, egui::pos2(600.0, 300.0));
        assert_eq!(
            (h.state().app.primary_color, h.state().app.secondary_color),
            colors
        );
        h.state_mut()
            .app
            .tool_settings
            .set(ToolType::ColorPicker, "afterClick", "pencil".into());
        canvas_button(&mut h, p, first, false);
        assert_eq!(h.state().app.tool_type, Some(ToolType::Pencil));
    }
}

/// §8.7: first Middle pans and a later Left cannot begin painting inside that pan.
#[test]
fn canvas_buttons_first_middle_keeps_panning_without_paint() {
    use egui::PointerButton::{Middle, Primary};
    let mut h = buttons_harness();
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .view
        .set_zoom_center(2.0);
    h.run();
    let n = history_names(&h).len();
    let (a, b, c) = (
        egui::pos2(600.0, 400.0),
        egui::pos2(580.0, 390.0),
        egui::pos2(560.0, 380.0),
    );
    canvas_move(&mut h, a);
    let v = &h.state().app.active_ws().unwrap().view;
    let origin = (v.viewport_x, v.viewport_y);
    canvas_button(&mut h, a, Middle, true);
    canvas_move(&mut h, b);
    canvas_button(&mut h, b, Primary, true);
    canvas_move(&mut h, c);
    canvas_button(&mut h, c, Primary, false);
    canvas_button(&mut h, c, Middle, false);
    let v = &h.state().app.active_ws().unwrap().view;
    assert_eq!(
        (v.viewport_x, v.viewport_y),
        (origin.0 + 40.0, origin.1 + 20.0)
    );
    assert_eq!(
        ink_count(&h),
        0,
        "a second press during pan must not start painting"
    );
    assert_eq!(history_names(&h).len(), n);
}

/// §8.7/§8.9: the first X release fixes a Clone Stamp anchor, so later Left-held motion
/// cannot move it. Anchors do not add history or change document pixels.
#[test]
fn canvas_buttons_clone_anchor_stops_at_extra_release() {
    use egui::PointerButton::{Extra1, Extra2, Primary};
    use pdn_core::tool::ToolType;
    for chord in [Extra1, Extra2] {
        let mut h = buttons_harness();
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::CloneStamp);
        let n = history_names(&h).len();
        let (a, b, c) = (
            egui::pos2(400.0, 300.0),
            egui::pos2(500.0, 300.0),
            egui::pos2(650.0, 400.0),
        );
        canvas_move(&mut h, a);
        h.event_modifiers(
            egui::Event::PointerButton {
                pos: a,
                button: Primary,
                pressed: true,
                modifiers: egui::Modifiers::CTRL,
            },
            egui::Modifiers::CTRL,
        );
        h.run();
        canvas_move(&mut h, b);
        canvas_button(&mut h, b, chord, true);
        canvas_button(&mut h, b, chord, false);
        let id = h.state().app.active_ws().unwrap().document.id;
        let fixed = h.state().app.clone_anchors[&(id, ToolType::CloneStamp)]
            .absolute
            .unwrap();
        canvas_move(&mut h, c);
        canvas_button(&mut h, c, Primary, false);
        assert_eq!(
            h.state().app.clone_anchors[&(id, ToolType::CloneStamp)].absolute,
            Some(fixed),
            "{chord:?}"
        );
        assert_eq!(ink_count(&h), 0);
        assert_eq!(history_names(&h).len(), n);
    }
}

/// selection-and-move §2.5 step 3: either Left/Right order moves the selection's origin,
/// then releasing the chord resumes extension until the original button is released.
#[test]
fn canvas_buttons_left_right_selection_origin_keeps_its_release_rule() {
    use egui::PointerButton::{Primary, Secondary};
    use pdn_core::tool::ToolType;
    for (first, chord) in [(Primary, Secondary), (Secondary, Primary)] {
        let mut h = buttons_harness();
        h.state_mut().clock = paintrs::Clock::Virtual(0.0);
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::RectangleSelect);
        let (a, b, c, d) = (
            egui::pos2(400.0, 300.0),
            egui::pos2(450.0, 350.0),
            egui::pos2(500.0, 370.0),
            egui::pos2(550.0, 420.0),
        );
        canvas_move(&mut h, a);
        canvas_button(&mut h, a, first, true);
        canvas_move(&mut h, b);
        let bounds = |h: &Harness<'_, paintrs::PaintApp>| {
            h.state()
                .app
                .active_ws()
                .unwrap()
                .selection
                .create_path(true)
                .get_bounds()
        };
        canvas_button(&mut h, b, chord, true);
        canvas_move(&mut h, c);
        // The live trace starts at a+(50,20), stays 50×50 while origin mode is held.
        canvas_button(&mut h, c, chord, false);
        assert!(h.state().app.canvas_captured());
        canvas_move(&mut h, d);
        h.state_mut().advance_clock(60);
        canvas_button(&mut h, d, first, false);
        let selected = bounds(&h);
        assert_eq!((selected.width, selected.height), (101.0, 101.0));
        assert!(!h.state().app.canvas_captured());
        assert_eq!(history_names(&h).last().unwrap(), "Rectangle Select");
        assert_eq!(ink_count(&h), 0);
    }
}

/// painting-tools-2 §7.4 step 5: an X button held outside the canvas keeps the Gradient's
/// settings window open past 500 ms. Releasing it ends that edit and pushes Change Color.
#[test]
fn canvas_buttons_extra_outside_canvas_keeps_settings_edit_open() {
    use pdn_core::{tool::ToolType, util::color::Color};
    for button in [egui::PointerButton::Extra1, egui::PointerButton::Extra2] {
        let mut h = buttons_harness();
        h.state_mut().clock = paintrs::Clock::Virtual(0.0);
        h.state_mut()
            .app
            .set_active_tool_from_type(ToolType::Gradient);
        drag_with(
            &mut h,
            egui::PointerButton::Primary,
            (400.0, 300.0),
            (600.0, 300.0),
        );
        let n = history_names(&h).len();
        let p = egui::pos2(10.0, 10.0);
        canvas_move(&mut h, p);
        canvas_button(&mut h, p, button, true);
        h.state_mut()
            .app
            .set_colors(Color::new(200, 0, 0, 255), Color::WHITE);
        h.run();
        h.state_mut().advance_clock(1000);
        h.run();
        assert_eq!(
            history_names(&h).len(),
            n,
            "{button:?} keeps the settings edit open"
        );
        canvas_button(&mut h, p, button, false);
        h.state_mut().advance_clock(100);
        h.run();
        assert_eq!(&history_names(&h)[n..], ["Change Color"]);
    }
}

/// Task 33: the tool can consume C before the UI toggles the active palette slot.
#[test]
fn shapes_active_color_key_respects_gesture_and_global_shortcuts() {
    use egui::{Key, PointerButton};
    use pdn_core::tool::ToolType;
    let mut h = buttons_harness();
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::Shapes);
    h.run();
    assert!(!h.state().shell.forms.colors.secondary_selected());
    h.key_press(Key::C);
    h.run();
    assert!(h.state().shell.forms.colors.secondary_selected());
    let a = egui::pos2(420.0, 300.0);
    let b = egui::pos2(640.0, 460.0);
    canvas_move(&mut h, a);
    canvas_button(&mut h, a, PointerButton::Primary, true);
    canvas_move(&mut h, b);
    h.key_press(Key::C);
    h.run();
    assert!(h.state().shell.forms.colors.secondary_selected());
    canvas_button(&mut h, b, PointerButton::Primary, false);
    h.key_press(Key::C);
    h.run();
    assert!(!h.state().shell.forms.colors.secondary_selected());
    h.key_press_modifiers(egui::Modifiers::CTRL, Key::A);
    h.run();
    assert!(!h.state().app.active_ws().unwrap().selection.is_empty());
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Shapes, "shape"),
        "rectangle"
    );
}

/// R95: the canvas relay preserves a complete stationary click delivered in one UI frame.
#[test]
fn shapes_same_frame_idle_click_draws_but_line_click_cancels() {
    use pdn_core::tool::ToolType;
    for tool in [ToolType::Shapes, ToolType::Line] {
        let mut h = buttons_harness();
        h.state_mut().app.set_active_tool_from_type(tool);
        h.run();
        let p = egui::pos2(500.0, 400.0);
        canvas_move(&mut h, p);
        let n = history_names(&h).len();
        let blank = canvas_bytes(&h);
        h.event(button_event(p, egui::PointerButton::Primary, true));
        h.event(button_event(p, egui::PointerButton::Primary, false));
        h.run();
        assert!(!h.state().app.canvas_captured());
        assert_eq!(h.state().app.canvas_buttons, 0);
        if tool == ToolType::Shapes {
            assert!(ink_count(&h) > 0);
            assert_eq!(&history_names(&h)[n..], ["Rectangle Shape"]);
            assert!(h.state_mut().app.undo());
            assert_eq!(canvas_bytes(&h), blank);
        } else {
            assert_eq!(canvas_bytes(&h), blank);
            assert_eq!(history_names(&h).len(), n);
        }
    }
}

#[test]
fn shapes_fix1_alt_characters_and_actual_global_shortcuts() {
    use egui::{Key, Modifiers};
    use pdn_core::{tool::ToolType, util::shortcut::ShortcutKey};
    let mut h = buttons_harness();
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::Shapes);
    h.run();
    h.key_press_modifiers(Modifiers::ALT, Key::A);
    h.run();
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Shapes, "shape"),
        "roundedRectangle"
    );
    h.key_press_modifiers(Modifiers::ALT, Key::C);
    h.run();
    assert!(h.state().shell.forms.colors.secondary_selected());
    h.key_press_modifiers(Modifiers::CTRL, Key::A);
    h.run();
    assert!(!h.state().app.active_ws().unwrap().selection.is_empty());
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Shapes, "shape"),
        "roundedRectangle"
    );
    h.key_press_modifiers(Modifiers::CTRL, Key::C);
    h.run();
    assert!(h.state().app.clipboard.item.is_some());
    assert!(h.state().shell.forms.colors.secondary_selected());
    let colors = [h.state().app.primary_color, h.state().app.secondary_color];
    h.key_press_modifiers(Modifiers::CTRL, Key::X);
    h.run();
    assert_eq!(
        [h.state().app.primary_color, h.state().app.secondary_color],
        colors
    );
    assert_eq!(history_names(&h).last().unwrap(), "Cut");
    // A real registered global action owns Alt+A; merely holding Alt does not.
    h.state_mut().shortcuts.set_shortcut(
        "menu.edit.selectAll",
        Some(ShortcutKey::from_combo("Alt+A")),
    );
    h.key_press_modifiers(Modifiers::ALT, Key::A);
    h.run();
    assert!(!h.state().app.active_ws().unwrap().selection.is_empty());
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Shapes, "shape"),
        "roundedRectangle"
    );
    let p = egui::pos2(500.0, 400.0);
    canvas_move(&mut h, p);
    canvas_button(&mut h, p, egui::PointerButton::Primary, true);
    h.key_press_modifiers(Modifiers::ALT, Key::C);
    h.run();
    assert!(h.state().shell.forms.colors.secondary_selected());
    canvas_button(&mut h, p, egui::PointerButton::Primary, false);
}

#[test]
fn shapes_fix1_reset_same_frame_ui_click_is_one_anchor_edit() {
    use pdn_core::tool::{ToolType, shapes::ShapesTool};
    let mut h = buttons_harness();
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::Shapes);
    h.run();
    drag_with(
        &mut h,
        egui::PointerButton::Primary,
        (420.0, 300.0),
        (640.0, 460.0),
    );
    drag_with(
        &mut h,
        egui::PointerButton::Primary,
        (530.0, 380.0),
        (530.0, 500.0),
    );
    let pixels = canvas_bytes(&h);
    let n = history_names(&h).len();
    let p = egui::pos2(530.0, 533.0);
    canvas_move(&mut h, p);
    h.event(button_event(p, egui::PointerButton::Secondary, true));
    h.event(button_event(p, egui::PointerButton::Secondary, false));
    h.run();
    h.state_mut().app.with_tool(|t, _| {
        let t = t.as_any_mut().downcast_mut::<ShapesTool>().unwrap();
        assert_eq!(t.context().unwrap().anchor, None);
    });
    assert_eq!(&history_names(&h)[n..], ["Move Rotation Anchor"]);
    assert_eq!(canvas_bytes(&h), pixels);
    assert!(!h.state().app.canvas_captured());
}

#[test]
fn shapes_fix1_toolbar_key_translation_is_single_and_keeps_shifted_punctuation() {
    use egui::{Key, Modifiers};
    use pdn_core::tool::ToolType;
    let mut h = buttons_harness();
    h.state_mut().app.set_active_tool_from_type(ToolType::Line);
    h.run();
    h.key_press_modifiers(Modifiers::ALT, Key::Period);
    h.event(egui::Event::Text(".".into()));
    h.run();
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Line, "dash"),
        "dash"
    );
    h.key_press_modifiers(Modifiers::SHIFT, Key::Period);
    h.run();
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Line, "dash"),
        "dash"
    );
    h.key_press_modifiers(Modifiers::ALT, Key::CloseBracket);
    h.run();
    assert_eq!(h.state().app.tool_settings.get(ToolType::Line, "width"), 5);
    h.key_press_modifiers(Modifiers::CTRL, Key::CloseBracket);
    h.run();
    assert_eq!(h.state().app.tool_settings.get(ToolType::Line, "width"), 10);
    h.key_press_modifiers(Modifiers::SHIFT, Key::CloseBracket);
    h.run();
    assert_eq!(h.state().app.tool_settings.get(ToolType::Line, "width"), 10);
}

#[path = "support/effect_keyboard.rs"]
mod effect_keyboard;

/// Task35: grouping a frame must not move Enter ahead of character input or insert key text twice.
#[test]
fn text_canvas_keeps_mixed_event_order_and_single_character_insertion() {
    fn setup() -> Harness<'static, paintrs::PaintApp> {
        let mut h = app_harness();
        h.state_mut()
            .app
            .set_active_tool_from_type(pdn_core::tool::ToolType::Text);
        h.run();
        click(&mut h, 450.0, 320.0);
        h
    }
    let events = [
        egui::Event::Key {
            key: egui::Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        },
        egui::Event::Text("a".into()),
        egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        },
        egui::Event::Text("b".into()),
    ];
    let mut batch = setup();
    for e in &events {
        batch.event(e.clone());
    }
    batch.run_steps(1);
    let mut separate = setup();
    for e in &events {
        separate.event(e.clone());
        separate.run_steps(1);
    }
    assert_eq!(canvas_bytes(&batch), canvas_bytes(&separate));
    assert_eq!(batch.state().app.active_ws().unwrap().history.undo.len(), 2);
    assert!(batch.output().platform_output.ime.is_some());
    batch.event(egui::Event::Ime(egui::ImeEvent::Preedit {
        text: "draft".into(),
        active_range_chars: Some(1..3),
    }));
    batch.run_steps(1);
    assert_eq!(batch.state().app.active_ws().unwrap().history.undo.len(), 2);
    batch.event(egui::Event::Ime(egui::ImeEvent::Preedit {
        text: String::new(),
        active_range_chars: None,
    }));
    batch.run_steps(1);
    assert_eq!(canvas_bytes(&batch), canvas_bytes(&separate));
}

#[test]
fn text_canvas_native_scroll_survives_frames_and_ordinary_writes_resume() {
    use pdn_core::tool::ToolType;
    let mut h = app_harness();
    h.state_mut().app.set_active_tool_from_type(ToolType::Text);
    h.run();
    click(&mut h, 450.0, 320.0);
    h.event(egui::Event::Text("M".repeat(100)));
    h.run();
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .view
        .set_zoom_center(1.5);
    h.run();
    h.key_press(egui::Key::End);
    h.run_steps(1);
    let view = &h.state().app.active_ws().unwrap().view;
    assert!(view.has_native_text_scroll());
    let expected = (view.viewport_x, view.viewport_y);
    h.run_steps(3);
    let view = &h.state().app.active_ws().unwrap().view;
    assert_eq!((view.viewport_x, view.viewport_y), expected);
    assert!(view.has_native_text_scroll());
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .view
        .set_view_position(10.49, 20.49);
    h.run_steps(2);
    let view = &h.state().app.active_ws().unwrap().view;
    assert_eq!((view.viewport_x, view.viewport_y), (10.0, 20.0));
    assert!(!view.has_native_text_scroll());
}

#[test]
fn text_canvas_font_size_input_accepts_native_bounds_and_rejects_invalid_entries() {
    use pdn_core::tool::ToolType;
    let mut h = app_harness();
    h.state_mut().app.set_active_tool_from_type(ToolType::Text);
    h.run();
    click(&mut h, 300.0, 71.0);
    for (text, expected) in [
        ("1", 1.0),
        ("2000", 2000.0),
        ("0.99", 2000.0),
        ("2000.1", 2000.0),
        ("", 2000.0),
    ] {
        h.key_press_modifiers(
            egui::Modifiers::CTRL | egui::Modifiers::COMMAND,
            egui::Key::A,
        );
        h.key_press(egui::Key::Backspace);
        if !text.is_empty() {
            h.event(egui::Event::Text(text.into()));
        }
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        assert_eq!(
            h.state()
                .app
                .tool_settings
                .get(ToolType::Text, "fontSize")
                .as_f64(),
            Some(expected),
            "entry {text:?}"
        );
    }
    click(&mut h, 700.0, 40.0);
    assert_eq!(
        h.state().app.tool_settings.get(ToolType::Text, "fontSize"),
        2000
    );
}

/// Task36: native Touch metadata belongs to the immediately paired pointer, not the frame.
#[test]
fn pen_pressure_native_pairs_preserve_samples_and_keyboard_order() {
    use pdn_core::tool::{KeyPress, MouseButton, PointerInput, PointerType, Tool, ToolType};
    use std::{cell::RefCell, rc::Rc};
    #[derive(Clone, Debug, PartialEq)]
    enum Seen {
        Pointer(&'static str, PointerInput),
        Key(String),
    }
    struct Probe(Rc<RefCell<Vec<Seen>>>);
    impl Tool for Probe {
        fn tool_type(&self) -> ToolType {
            ToolType::PaintBrush
        }
        fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
            self
        }
        fn on_mouse_down(
            &mut self,
            _: &mut pdn_core::app::AppWorkspace,
            _: f64,
            _: f64,
            _: MouseButton,
            p: &PointerInput,
        ) -> bool {
            self.0.borrow_mut().push(Seen::Pointer("down", p.clone()));
            true
        }
        fn on_mouse_move(
            &mut self,
            _: &mut pdn_core::app::AppWorkspace,
            _: f64,
            _: f64,
            p: &PointerInput,
        ) -> bool {
            self.0.borrow_mut().push(Seen::Pointer("move", p.clone()));
            true
        }
        fn on_mouse_up(
            &mut self,
            _: &mut pdn_core::app::AppWorkspace,
            _: f64,
            _: f64,
            _: MouseButton,
            p: &PointerInput,
        ) -> bool {
            self.0.borrow_mut().push(Seen::Pointer("up", p.clone()));
            true
        }
        fn on_key_press(&mut self, _: &mut pdn_core::app::AppWorkspace, key: &KeyPress) -> bool {
            self.0.borrow_mut().push(Seen::Key(key.key.clone()));
            true
        }
    }
    let mut h = buttons_harness();
    let a = egui::pos2(400.0, 300.0);
    canvas_move(&mut h, a);
    let seen = Rc::new(RefCell::new(vec![]));
    h.state_mut().app.tool = Some(Box::new(Probe(seen.clone())));
    let touch = |id, phase, pos, force| egui::Event::Touch {
        device_id: egui::TouchDeviceId(7),
        id: egui::TouchId(id),
        phase,
        pos,
        force,
    };
    h.event(touch(1, egui::TouchPhase::Start, a, Some(0.0)));
    h.event(egui::Event::PointerMoved(a));
    h.event(button_event(a, egui::PointerButton::Primary, true));
    for (x, pressure) in [(430.0, 0.2), (460.0, 0.8)] {
        let p = egui::pos2(x, 300.0);
        h.event(touch(1, egui::TouchPhase::Move, p, Some(pressure)));
        h.event(egui::Event::PointerMoved(p));
    }
    h.event(touch(
        2,
        egui::TouchPhase::Start,
        egui::pos2(600.0, 300.0),
        Some(1.0),
    ));
    h.event(egui::Event::Key {
        key: egui::Key::ArrowRight,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    let b = egui::pos2(490.0, 300.0);
    h.event(touch(1, egui::TouchPhase::Move, b, Some(0.4)));
    h.event(egui::Event::PointerMoved(b));
    h.event(touch(1, egui::TouchPhase::End, b, Some(0.0)));
    h.event(button_event(b, egui::PointerButton::Primary, false));
    h.event(egui::Event::PointerGone);
    h.run();
    let events = seen.borrow();
    let pointers: Vec<_> = events
        .iter()
        .filter_map(|e| {
            if let Seen::Pointer(kind, p) = e {
                Some((*kind, p))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(pointers.iter().filter(|(k, _)| *k == "down").count(), 1);
    assert_eq!(pointers.iter().filter(|(k, _)| *k == "up").count(), 1);
    assert_eq!(
        pointers
            .iter()
            .find(|(k, _)| *k == "down")
            .unwrap()
            .1
            .samples[0]
            .pressure,
        Some(0.0)
    );
    let pressures: Vec<_> = pointers
        .iter()
        .filter(|(k, _)| *k == "move")
        .flat_map(|(_, p)| p.samples.iter().map(|s| s.pressure))
        .collect();
    assert_eq!(
        pressures,
        [
            Some(0.0),
            Some(0.2_f32 as f64),
            Some(0.8_f32 as f64),
            Some(0.4_f32 as f64)
        ]
    );
    assert!(
        pointers
            .iter()
            .all(|(_, p)| p.pointer_type == PointerType::Pen)
    );
    let key = events
        .iter()
        .position(|e| matches!(e, Seen::Key(_)))
        .unwrap();
    assert!(
        matches!(&events[key - 1], Seen::Pointer("move", p) if p.samples.last().unwrap().pressure == Some(0.8_f32 as f64))
    );
    assert!(
        matches!(&events[key + 1], Seen::Pointer("move", p) if p.samples[0].pressure == Some(0.4_f32 as f64))
    );
}

#[test]
fn pen_pressure_toolbar_requires_session_pen_and_enabled_setting() {
    use pdn_core::tool::types::{ToolType, option_definitions};
    let mut h = buttons_harness();
    let has = |h: &Harness<'_, paintrs::PaintApp>| {
        [
            ToolType::PaintBrush,
            ToolType::Eraser,
            ToolType::CloneStamp,
            ToolType::Recolor,
        ]
        .map(|t| {
            option_definitions(t, &h.state().app.tool_settings)
                .iter()
                .any(|d| d.key == "pressure")
        })
    };
    assert_eq!(has(&h), [false; 4]);
    h.state_mut().settings.set("pen.pointerInput", false.into());
    h.event(egui::Event::Touch {
        device_id: egui::TouchDeviceId(7),
        id: egui::TouchId(1),
        phase: egui::TouchPhase::Move,
        pos: egui::pos2(400.0, 300.0),
        force: Some(0.0),
    });
    h.run();
    assert_eq!(has(&h), [false; 4]);
    h.state_mut().settings.set("pen.pointerInput", true.into());
    h.run();
    assert_eq!(has(&h), [true; 4]);
}

#[test]
fn pen_pressure_changes_real_stroke_width_and_loss_commits_once() {
    use egui::{Event, TouchDeviceId, TouchId, TouchPhase};
    let mut counts = vec![];
    for (enabled, force) in [
        (true, Some(0.0)),
        (true, Some(0.25)),
        (true, None),
        (false, Some(0.25)),
    ] {
        let mut h = buttons_harness();
        h.state_mut()
            .settings
            .set("pen.pointerInput", enabled.into());
        h.state_mut().app.tool_settings.set(
            pdn_core::tool::ToolType::PaintBrush,
            "width",
            40.into(),
        );
        let a = egui::pos2(400.0, 300.0);
        let b = egui::pos2(600.0, 300.0);
        canvas_move(&mut h, a);
        let before = history_names(&h).len();
        h.event(Event::Touch {
            device_id: TouchDeviceId(7),
            id: TouchId(1),
            phase: TouchPhase::Start,
            pos: a,
            force,
        });
        h.event(Event::PointerMoved(a));
        h.event(button_event(a, egui::PointerButton::Primary, true));
        h.event(Event::Touch {
            device_id: TouchDeviceId(7),
            id: TouchId(1),
            phase: TouchPhase::Move,
            pos: b,
            force,
        });
        h.event(Event::PointerMoved(b));
        h.event(Event::Touch {
            device_id: TouchDeviceId(7),
            id: TouchId(1),
            phase: TouchPhase::Cancel,
            pos: b,
            force,
        });
        h.event(Event::PointerGone);
        h.run();
        counts.push(ink_count(&h));
        assert_eq!(&history_names(&h)[before..], ["Paintbrush"]);
        assert!(!h.state().app.canvas_captured());
    }
    assert!(
        counts[0] < counts[1],
        "zero stays smaller than quarter pressure: {counts:?}"
    );
    assert!(
        counts[1] < counts[2],
        "quarter pressure must shrink the stroke: {counts:?}"
    );
    assert_eq!(
        counts[2], counts[3],
        "absent pressure and disabled pen input are full size"
    );
}

/// The web-chrome shell's layout as `Shell::show` computes it.
fn shell_layout(h: &Harness<'_, paintrs::PaintApp>) -> paintrs::shell::ShellLayout {
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, h.ctx.content_rect().size());
    let painter = egui::Painter::new(
        h.ctx.clone(),
        egui::LayerId::background(),
        egui::Rect::EVERYTHING,
    );
    paintrs::shell::ShellLayout::compute(
        screen,
        false,
        paintrs::shell::main_menu::width(&painter),
        1,
    )
}

/// `key`'s control (points) in `tool`'s toolbar row at `ppp`, laid out as `ToolMenu::show` does.
fn tool_option_control(
    h: &Harness<'_, paintrs::PaintApp>,
    tool: pdn_core::tool::types::ToolType,
    key: &str,
    ppp: f32,
) -> egui::Rect {
    use paintrs::shell::tool_menu;
    use pdn_core::tool::types::option_definitions;
    let l = shell_layout(h);
    let settings = &h.state().app.tool_settings;
    let defs = option_definitions(tool, settings);
    let content = egui::Rect::from_min_max(
        egui::pos2(l.tool_menu.min.x, l.tool_menu.min.y + 1.0),
        l.tool_menu.max,
    );
    // Options start after the 68-DIP selector (1-DIP margins) and its 11-DIP separator.
    let x_options = l.tool_menu.min.x + 1.0 + 68.0 + 1.0 + 11.0;
    let row = tool_menu::layout_row(&defs, |k| settings.get(tool, k), content, x_options, ppp);
    row.options[defs.iter().position(|d| d.key == key).unwrap()].control
}

/// User (2026-10-24): the wheel over the brush size box steps it like its ± buttons: one per
/// wheel line, one per 100 px of a pixel delta (partial deltas add up), five with Ctrl.
#[test]
fn the_wheel_over_the_size_box_steps_it() {
    let mut h = harness_at(1.0);
    let width = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .tool_settings
            .get(pdn_core::tool::types::ToolType::PaintBrush, "width")
            .as_f64()
            .unwrap()
    };
    let start = width(&h);
    let control = tool_option_control(
        &h,
        pdn_core::tool::types::ToolType::PaintBrush,
        "width",
        1.0,
    );
    let at = control.center();
    h.event(egui::Event::PointerMoved(at));
    h.run();
    let wheel = |h: &mut Harness<'_, paintrs::PaintApp>, unit, dy: f32, modifiers| {
        h.event(egui::Event::MouseWheel {
            unit,
            source: egui::MouseWheelSource::Wheel,
            delta: egui::vec2(0.0, dy),
            modifiers,
            phase: egui::TouchPhase::Move,
        });
        h.run();
    };
    let none = egui::Modifiers::NONE;
    wheel(&mut h, egui::MouseWheelUnit::Line, 1.0, none);
    assert_eq!(width(&h), start + 1.0);
    wheel(&mut h, egui::MouseWheelUnit::Line, -2.0, none);
    assert_eq!(width(&h), start - 1.0);
    wheel(&mut h, egui::MouseWheelUnit::Point, 60.0, none);
    assert_eq!(width(&h), start - 1.0);
    wheel(&mut h, egui::MouseWheelUnit::Point, 60.0, none);
    assert_eq!(width(&h), start);
    wheel(
        &mut h,
        egui::MouseWheelUnit::Line,
        1.0,
        egui::Modifiers::CTRL,
    );
    assert_eq!(width(&h), start + 5.0);
}

/// Review M2: the wheel over a size box (a) is the open menu's when a menu lies over the box,
/// and (b) shows its value in the box's focused field, so Enter and ArrowUp go on from it.
#[test]
fn the_size_box_wheel_skips_menus_and_updates_its_field() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    let width = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .tool_settings
            .get(ToolType::PaintBrush, "width")
            .as_f64()
    };
    let wheel = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.event(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            source: egui::MouseWheelSource::Wheel,
            delta: egui::vec2(0.0, 1.0),
            modifiers: egui::Modifiers::NONE,
            phase: egui::TouchPhase::Move,
        });
        h.run();
    };
    assert_eq!(width(&harness), Some(2.0));
    // View's menu lies over the Brush size box: the wheel there leaves the size alone.
    let at = egui::pos2(185.0, 71.0);
    click(&mut harness, 100.0, 21.0);
    harness.event(egui::Event::PointerMoved(at));
    harness.run();
    wheel(&mut harness);
    assert!(harness.state().shell.menu_open());
    assert_eq!(width(&harness), Some(2.0));
    harness.key_press(egui::Key::Escape);
    harness.run();
    // The focused field ("2"): the wheel shows 3 in it, Enter keeps 3, ArrowUp steps to 4.
    click(&mut harness, at.x, at.y);
    wheel(&mut harness);
    assert_eq!(width(&harness), Some(3.0));
    harness.key_press(egui::Key::Enter);
    harness.run();
    assert_eq!(width(&harness), Some(3.0));
    harness.key_press(egui::Key::ArrowUp);
    harness.run();
    assert_eq!(width(&harness), Some(4.0));
}

/// A 1280×800 web-chrome app at `ppp` on the virtual clock.
fn harness_at(ppp: f32) -> Harness<'static, paintrs::PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(ppp)
        .build_eframe(new_web_chrome_app);
    h.run();
    h.state_mut().clock = paintrs::Clock::Virtual(0.0);
    h
}

/// One frame holding all of `events` (`Harness::event` runs a frame per event): what a loaded
/// machine delivers when frames are sparse.
fn batched(h: &mut Harness<'_, paintrs::PaintApp>, events: impl IntoIterator<Item = egui::Event>) {
    h.input_mut().events.extend(events);
    h.run();
}

/// User report (brush dots): the toolbar's Spacing meter sets the value where it is pressed and
/// dragged (v = 1 % + 499 %·f², ui-text-and-glyphs.md §7.3), at 100 % and 150 % UI scale. A
/// click reads where its button went up — the frame's last such release — not the pointer at the
/// frame's end: a loaded machine's frame bringing the release and later moves right of the meter
/// set 500 %, which spaces dabs five diameters apart.
#[test]
fn spacing_meter_reads_the_click_where_the_button_went_up() {
    use egui::PointerButton::Primary;
    use pdn_core::tool::types::ToolType;
    for ppp in [1.0, 1.5] {
        let mut h = harness_at(ppp);
        let spacing = |h: &Harness<'_, paintrs::PaintApp>| {
            h.state()
                .app
                .tool_settings
                .get(ToolType::PaintBrush, "spacing")
        };
        assert_eq!(spacing(&h), serde_json::json!(15), "ppp {ppp}");
        // The meter lies between the − / + buttons, inside its border.
        let control = tool_option_control(&h, ToolType::PaintBrush, "spacing", ppp);
        let b = paintrs::kit::tool_option::border(ppp);
        let inner = control.shrink2(egui::vec2(paintrs::theme::TOOL_BUTTON + b, b));
        let at = |f: f32| egui::pos2(inner.min.x + inner.width() * f, control.center().y);
        // f = 0.3 → 1 + 499·0.09 = 46 %; 0.35 → 62 %; + steps 1 %.
        click(&mut h, at(0.3).x, at(0.3).y);
        assert_eq!(spacing(&h), serde_json::json!(46), "ppp {ppp}");
        drag(&mut h, at(0.3), at(0.35), 3);
        assert_eq!(spacing(&h), serde_json::json!(62), "ppp {ppp}");
        click(&mut h, control.max.x - 11.0, control.center().y);
        assert_eq!(spacing(&h), serde_json::json!(63), "ppp {ppp}");
        // The release and the moves after it in one frame.
        h.event(egui::Event::PointerMoved(at(0.3)));
        h.run();
        h.event(button_event(at(0.3), Primary, true));
        h.run();
        batched(
            &mut h,
            [
                button_event(at(0.3), Primary, false),
                egui::Event::PointerMoved(egui::pos2(1000.0, 300.0)),
                egui::Event::PointerMoved(egui::pos2(1100.0, 400.0)),
            ],
        );
        assert_eq!(spacing(&h), serde_json::json!(46), "ppp {ppp}");
        // A drag's release and then a click in one frame: egui's click is the last release
        // (f = 0.2 → 1 + 499·0.04 = 21 %), not the drag's end (f = 0.6 → 181 %).
        h.event(button_event(at(0.3), Primary, true));
        h.run();
        h.event(egui::Event::PointerMoved(at(0.6)));
        h.run();
        assert_eq!(spacing(&h), serde_json::json!(181), "ppp {ppp}");
        batched(
            &mut h,
            [
                button_event(at(0.6), Primary, false),
                egui::Event::PointerMoved(at(0.2)),
                button_event(at(0.2), Primary, true),
                button_event(at(0.2), Primary, false),
            ],
        );
        assert_eq!(spacing(&h), serde_json::json!(21), "ppp {ppp}");
    }
}

/// Review fix: a split dropdown's click picks Main or Arrow where the button went up — a press
/// and release on Rectangle Select's mode arrow followed, in the same frame, by a move onto the
/// main part opens the list and does not cycle the mode.
#[test]
fn split_dropdown_reads_the_click_where_the_button_went_up() {
    use egui::PointerButton::Primary;
    use pdn_core::tool::types::ToolType;
    let mut h = harness_at(1.0);
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::RectangleSelect);
    h.run();
    let mode = |h: &Harness<'_, paintrs::PaintApp>| {
        h.state()
            .app
            .tool_settings
            .get(ToolType::RectangleSelect, "selectionMode")
    };
    assert_eq!(mode(&h), "normal");
    let control = tool_option_control(&h, ToolType::RectangleSelect, "selectionMode", 1.0);
    let y = control.center().y;
    // The split's 14-DIP drop part is at the right; the main part is the rest.
    let (arrow, main) = (
        egui::pos2(control.max.x - 5.0, y),
        egui::pos2(control.min.x + 10.0, y),
    );
    h.event(egui::Event::PointerMoved(arrow));
    h.run();
    batched(
        &mut h,
        [
            button_event(arrow, Primary, true),
            button_event(arrow, Primary, false),
            egui::Event::PointerMoved(main),
        ],
    );
    assert_eq!(mode(&h), "normal");
    assert!(h.state().shell.tool_menu.popup.is_some());
}

/// Known bugs (click positions): an open option list's document click listener reads where the
/// click went up — a click outside whose release frame also moves the pointer into the list
/// closes it — and the size menu's outside `pointerdown` reads where the press went down.
#[test]
fn option_popups_read_outside_clicks_where_they_happened() {
    use egui::PointerButton::Primary;
    use pdn_core::tool::types::ToolType;
    let mut h = harness_at(1.0);
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::RectangleSelect);
    h.run();
    let control = tool_option_control(&h, ToolType::RectangleSelect, "selectionMode", 1.0);
    click(&mut h, control.max.x - 5.0, control.center().y);
    h.run();
    assert!(h.state().shell.tool_menu.popup.is_some());
    // Outside: the tool bar's empty end; inside: the list's first row, under the face.
    let outside = egui::pos2(1200.0, control.center().y);
    let inside = egui::pos2(control.min.x + 10.0, control.max.y + 8.0);
    h.event(egui::Event::PointerMoved(outside));
    h.run();
    h.event(button_event(outside, Primary, true));
    h.run();
    batched(
        &mut h,
        [
            button_event(outside, Primary, false),
            egui::Event::PointerMoved(inside),
        ],
    );
    assert_eq!(h.state().shell.tool_menu.popup, None, "the list closes");
    h.state_mut()
        .app
        .set_active_tool_from_type(ToolType::PaintBrush);
    h.run();
    let control = tool_option_control(&h, ToolType::PaintBrush, "width", 1.0);
    let b = paintrs::kit::tool_option::border(1.0);
    let [_, combo, _, arrow, _] =
        paintrs::kit::tool_option::size_parts(control, paintrs::theme::TOOL_SIZE_COMBO.0, b);
    click(&mut h, arrow.center().x, arrow.center().y);
    h.run();
    assert!(h.state().shell.tool_menu.popup.is_some());
    let inside = egui::pos2(combo.min.x + 10.0, combo.max.y + 10.0);
    h.event(egui::Event::PointerMoved(outside));
    h.run();
    batched(
        &mut h,
        [
            button_event(outside, Primary, true),
            egui::Event::PointerMoved(inside),
        ],
    );
    assert_eq!(
        h.state().shell.tool_menu.popup,
        None,
        "the size menu closes"
    );
}

/// Known bugs (click positions): a right click on a document strip item opens its menu where the
/// button went up, though that frame also moves the pointer off the item; the open document
/// list's outside `pointerdown` reads where the press went down, not the list it moved onto.
#[test]
fn document_strip_reads_clicks_and_presses_where_they_happened() {
    use egui::PointerButton::{Primary, Secondary};
    let mut h = app_harness();
    // The item and its context menu's Close entry (document_strip_closes_clean_documents_…).
    let item = egui::pos2(424.0, 39.0);
    h.event(egui::Event::PointerMoved(item));
    h.run();
    h.event(button_event(item, Secondary, true));
    h.run();
    batched(
        &mut h,
        [
            button_event(item, Secondary, false),
            egui::Event::PointerMoved(egui::pos2(424.0, 300.0)),
        ],
    );
    click(&mut h, 520.0, 190.0);
    assert!(h.state().app.documents.is_empty(), "the menu opened there");
    let mut h = app_harness();
    click(&mut h, 452.0, 30.0);
    assert!(h.state().shell.document_strip.list_open);
    // The list hangs from the strip's bottom right corner.
    let strip = shell_layout(&h).document_strip;
    let (outside, inside) = (
        egui::pos2(700.0, 15.0),
        egui::pos2(strip.max.x - 10.0, strip.max.y + 10.0),
    );
    h.event(egui::Event::PointerMoved(outside));
    h.run();
    batched(
        &mut h,
        [
            button_event(outside, Primary, true),
            egui::Event::PointerMoved(inside),
        ],
    );
    assert!(!h.state().shell.document_strip.list_open);
}

/// User report (brush dots): a Paintbrush drag whose moves arrive 40–60 px apart (one per frame)
/// through the canvas input at the default Size 2 and Spacing 15 % paints one gap-free run.
#[test]
fn sparse_canvas_moves_paint_one_brush_run() {
    use pdn_core::tool::types::ToolType;
    let mut h = harness_at(1.0);
    let brush = ToolType::PaintBrush;
    assert_eq!(h.state().app.tool_type, Some(brush));
    let settings = &h.state().app.tool_settings;
    assert_eq!(
        (settings.get(brush, "width"), settings.get(brush, "spacing")),
        (serde_json::json!(2), serde_json::json!(15))
    );
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .view
        .set_zoom_center(1.0);
    h.run();
    let n = history_names(&h).len();
    let xs = [400.0, 440.0, 500.0, 545.0, 600.0, 640.0];
    let at = |x: f32| egui::pos2(x, 300.0);
    canvas_move(&mut h, at(xs[0]));
    canvas_button(&mut h, at(xs[0]), egui::PointerButton::Primary, true);
    for &x in &xs[1..] {
        canvas_move(&mut h, at(x));
    }
    canvas_button(&mut h, at(xs[5]), egui::PointerButton::Primary, false);
    assert_eq!(&history_names(&h)[n..], ["Paintbrush"]);
    // The row with the most ink holds one run as long as the drag.
    let doc_w = h.state().app.active_ws().unwrap().document.width as usize;
    let bytes = canvas_bytes(&h);
    let row = bytes
        .chunks(doc_w * 4)
        .max_by_key(|r| r.chunks(4).filter(|p| p[0] < 128).count())
        .unwrap();
    let ink: Vec<usize> = (0..doc_w).filter(|&x| row[x * 4] < 128).collect();
    let (first, last) = (ink[0], ink[ink.len() - 1]);
    assert_eq!(ink.len(), last - first + 1, "a gap in {ink:?}");
    assert!(ink.len() >= 240, "{} columns", ink.len());
}

/// Ruling R231: the Magnetic Lasso draws as Lasso Select does. The border shows from the first
/// move, tinted, with marching ants (white dashes over black, no solid black line); the release
/// selects it as one history item; and a tool switch mid-border leaves no stale pixels (the
/// canvas equals a forced full refresh). `ML_SCRATCH=<dir>` saves the renders there.
#[test]
fn magnetic_lasso_draws_like_lasso_select() {
    use pdn_core::tool::types::ToolType;
    let mut harness = app_harness();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    let image = harness.render().expect("render");
    // The document's top-left on screen: the white runs left and up from the window's centre.
    let white = |x: u32, y: u32| image.get_pixel(x, y).0 == [255; 4];
    let left = (0..640).rev().find(|&x| !white(x, 400)).unwrap() + 1;
    let top = (0..400).rev().find(|&y| !white(640, y)).unwrap() + 1;
    let at = |x: f32, y: f32| egui::pos2(left as f32 + x, top as f32 + y);
    // A dark 300-px square in the document, and a ring 6 px outside its corners.
    {
        let ws = harness.state_mut().app.active_ws_mut().unwrap();
        let w = ws.document.width as usize;
        let surface = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
        for (i, px) in surface
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            if (250..550).contains(&(i % w)) && (150..450).contains(&(i / w)) {
                *px = [40, 40, 40, 255];
            }
        }
        ws.invalidate(None);
    }
    harness
        .state_mut()
        .app
        .set_active_tool_from_type(ToolType::MagneticLasso);
    harness.run_steps(2);
    let ring = [at(244.0, 144.0), at(556.0, 144.0), at(556.0, 456.0)];
    let save = |image: &image::RgbaImage, name: &str| {
        if let Some(dir) = std::env::var_os("ML_SCRATCH") {
            image.save(std::path::Path::new(&dir).join(name)).unwrap();
        }
    };
    let stale = |h: &mut Harness<'_, paintrs::PaintApp>| {
        let shown = h.render().expect("render");
        h.state_mut().shell.canvas_view.refresh_all();
        h.run_steps(1);
        let full = h.render().expect("render");
        shown
            .pixels()
            .zip(full.pixels())
            .filter(|(a, b)| a != b)
            .count()
    };
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    // Along the ring's first two sides in 20-px moves, the button held.
    let press_and_trace = |h: &mut Harness<'_, paintrs::PaintApp>| {
        h.event(egui::Event::PointerMoved(ring[0]));
        h.run();
        h.event(button(ring[0], true));
        h.run();
        let before = h.render().expect("render");
        let first = ring[0] + egui::vec2(20.0, 0.0);
        h.event(egui::Event::PointerMoved(first));
        h.run_steps(1);
        // The first move shows at once.
        let shown = h.render().expect("render");
        let band = (ring[0].x as u32..first.x as u32).flat_map(|x| (140..150).map(move |y| (x, y)));
        let moved = band
            .map(|(x, y)| (x, y + top))
            .filter(|&(x, y)| before.get_pixel(x, y) != shown.get_pixel(x, y));
        assert!(moved.count() > 10, "nothing shows after the first move");
        for w in ring.windows(2) {
            let n = ((w[1] - w[0]).length() / 20.0).ceil() as u32;
            for k in 1..=n {
                h.event(egui::Event::PointerMoved(
                    w[0] + (w[1] - w[0]) * (k as f32 / n as f32),
                ));
                h.run();
            }
        }
    };
    press_and_trace(&mut harness);
    harness.run_steps(2);
    let mid = harness.render().expect("render");
    save(&mid, "magnetic-mid.png");
    // Inside the border (the triangle it closes): the dark square, tinted blue.
    let inside = mid.get_pixel(left + 500, top + 200).0;
    assert!(inside[2] > inside[0] + 20, "no tint: {inside:?}");
    // The outline's row on the square's top edge, where the border snapped: white dashes and
    // black gaps along it (a solid line would be one colour).
    let edge: Vec<[u8; 4]> = (left + 300..left + 500)
        .map(|x| mid.get_pixel(x, top + 150).0)
        .collect();
    assert!(
        edge.iter().any(|p| p[0] > 235 && p[1] > 235 && p[2] > 235),
        "no white dashes"
    );
    assert!(
        edge.iter().any(|p| p[0] < 20 && p[1] < 20 && p[2] < 20),
        "no black gaps"
    );
    assert_eq!(stale(&mut harness), 0, "stale pixels mid-border");
    // The release selects the border.
    harness.event(button(ring[2], false));
    harness.run_steps(3);
    let released = harness.render().expect("render");
    save(&released, "magnetic-released.png");
    {
        let ws = harness.state().app.active_ws().unwrap();
        assert_eq!(ws.history.undo.last().unwrap().name, "Magnetic Lasso");
        assert!(!ws.selection.is_empty());
    }
    let inside = released.get_pixel(left + 500, top + 200).0;
    assert!(inside[2] > inside[0] + 20, "no tint: {inside:?}");
    assert_eq!(stale(&mut harness), 0, "stale pixels after the release");
    // A new border, then a tool switch mid-drag: nothing of it stays on the canvas.
    press_and_trace(&mut harness);
    harness
        .state_mut()
        .app
        .set_active_tool_from_type(ToolType::RectangleSelect);
    harness.event(button(ring[2], false));
    harness.run_steps(3);
    let switched = harness.render().expect("render");
    save(&switched, "magnetic-switched.png");
    assert_eq!(stale(&mut harness), 0, "stale pixels after the switch");
    let ws = harness.state().app.active_ws().unwrap();
    assert_eq!(ws.history.undo.last().unwrap().name, "Magnetic Lasso");
}

/// Review M1: with animations off nothing else repaints, yet a Magnetic Lasso backlog (one long
/// move's steps, traced a few a frame) drains by itself: the frames chain until it is empty.
#[test]
fn a_magnetic_lasso_backlog_drains_without_input() {
    use pdn_core::tool::types::ToolType;
    let mut harness = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_pixels_per_point(1.0)
        .with_max_steps(64)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    harness.run();
    harness.state_mut().clock = paintrs::Clock::Virtual(0.0);
    harness
        .state_mut()
        .settings
        .set("ui.animations", false.into());
    harness
        .state_mut()
        .app
        .set_active_tool_from_type(ToolType::MagneticLasso);
    harness.run();
    // A click starts the border (click mode); one 300-px move queues 13 steps.
    click(&mut harness, 500.0, 400.0);
    harness.event(egui::Event::PointerMoved(egui::pos2(800.0, 400.0)));
    harness.run();
    assert!(
        !harness.state_mut().app.tool_ticking(),
        "the backlog waits for input"
    );
}
