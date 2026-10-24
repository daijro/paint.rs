//! Task55 uses one serial scenario because I18n is process-global.
use egui_kittest::{Harness, kittest::Queryable};
use paintrs::{
    PaintApp,
    platform::{
        PickedFile,
        asset_library::{LOCALES_KEY, TRANSLATIONS_KEY},
    },
    prefs::Prefs,
};
use pdn_core::{
    i18n::{self, I18n},
    settings::AppSettings,
};
use serde_json::json;

struct Restore(Option<I18n>);
impl Drop for Restore {
    fn drop(&mut self) {
        i18n::with_global(|current| *current = self.0.take().unwrap());
    }
}

#[test]
fn pack_translation_overrides_builtin_after_reload() {
    let _restore = Restore(Some(i18n::with_global(|current| {
        std::mem::replace(
            current,
            I18n::new(include_str!("../../../assets/lang/en.json")),
        )
    })));
    let ctx = egui::Context::default();
    let mut seed = serde_json::Map::new();
    seed.insert(
        TRANSLATIONS_KEY.into(),
        json!(r#"{"en":{"menu":{"file":{"saveAs":{"text":"Save Copy As..."}}}}}"#),
    );
    let app = PaintApp::new_for_test(&ctx, seed);
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Save Copy As...");
    drop(app);
    assert_regional_culture(&ctx);

    // Missing dictionaries fall back to English even when a locale is advertised.
    let mut prefs = Prefs::default();
    prefs.set(LOCALES_KEY, r#"["de","ar"]"#);
    let mut settings = AppSettings::load(None);
    settings.set("ui.language", json!("ar"));
    paintrs::apply_language(&ctx, &prefs, &settings);
    assert!(!paintrs::language::is_rtl(&ctx));
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Save As...");
    prefs.set(
        TRANSLATIONS_KEY,
        r#"{"ar":{"effects":{"name":{"format":{"configurable":"HOSTILE {0}"}}}}}"#,
    );
    paintrs::apply_language(&ctx, &prefs, &settings);
    assert_eq!(
        pdn_core::action::menu_label(&Default::default(), "adjustment.bbBlackAndWhitePlus"),
        "Black and White+..."
    );
    assert_rtl_box(&ctx);
    assert_rtl_buttons(&ctx);
    settings.set("ui.language", json!("en"));
    paintrs::apply_language(&ctx, &Prefs::default(), &settings);
    assert!(!paintrs::language::is_rtl(&ctx));
    assert_text_status("Anchor point: 0.20in, 0.09in");
    assert_selection_status(
        "Selection top left: 0.20in, 0.30in. Bounding rectangle size: 0.40in × 0.50in. Area: 0.20 inches square",
    );
    reload_scenario();
}

fn assert_regional_culture(ctx: &egui::Context) {
    let mut prefs = Prefs::default();
    prefs.set(TRANSLATIONS_KEY, r#"{"de":{"regionalWord":"Deutsch"}}"#);
    let mut settings = AppSettings::load(None);
    settings.set("ui.language", json!("auto"));
    paintrs::language::apply_with_locale(ctx, &prefs, &settings, "de_CH");
    assert_eq!(i18n::t("regionalWord"), "Deutsch");
    assert_eq!(pdn_core::app::format_fixed2(1.25), "1.25");
    assert_eq!(pdn_core::app::format_n(1234.5, 2), "1'234.50");
    assert_eq!(i18n::language_code(), "de");
    assert_eq!(i18n::culture_code(), "de-CH");
    settings.set("ui.language", json!("de"));
    prefs.remove(TRANSLATIONS_KEY);
    assert_eq!(
        i18n::t("regionalWord"),
        "Deutsch",
        "pending preferences do not change applied translations"
    );
    assert_eq!(
        pdn_core::app::format_fixed2(1.25),
        "1.25",
        "pending preferences do not change applied culture"
    );
    settings.set("ui.language", json!("auto"));
    paintrs::language::apply_with_locale(ctx, &prefs, &settings, "en-IN");
    assert_eq!(
        i18n::language_code(),
        "en",
        "regional numbers do not require a regional dictionary"
    );
    assert_eq!(i18n::culture_code(), "en-IN");
    assert_eq!(i18n::t("regionalWord"), "regionalWord");
    assert_eq!(pdn_core::app::format_n(1234567.5, 2), "12,34,567.50");
    settings.set("ui.language", json!("de"));
    assert_eq!(pdn_core::app::format_n(1234567.5, 2), "12,34,567.50");
}

fn assert_rtl_box(ctx: &egui::Context) {
    use paintrs::kit::text::{self, FontSpec};
    text::enable_text_recording(ctx);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        let painter = ui.painter().clone();
        text::draw_ellipsized(
            &painter,
            egui::Rect::from_min_size(egui::pos2(10., 20.), egui::vec2(200., 20.)),
            "abc",
            FontSpec::new(12.),
            egui::Color32::WHITE,
        );
        for (y, label, alignment) in [
            (50., "left", text::Alignment::Left),
            (80., "right", text::Alignment::Right),
            (110., "center", text::Alignment::Center),
        ] {
            text::draw_ellipsized_aligned(
                &painter,
                egui::Rect::from_min_size(egui::pos2(10., y), egui::vec2(200., 20.)),
                label,
                FontSpec::new(12.),
                egui::Color32::WHITE,
                alignment,
            );
        }
        let runs = [text::Run {
            text: "link",
            spec: FontSpec::new(12.),
            color: egui::Color32::WHITE,
            underline: true,
        }];
        let lines = text::wrap_runs(&painter, &runs, 200.);
        assert!(
            lines[0][0].x > 170.,
            "paragraph text and link hit boxes share their RTL placement"
        );
        let fixed = text::wrap_runs_aligned(&painter, &runs, 200., text::Alignment::Left);
        assert_eq!(fixed[0][0].x, 0., "CSS text-align:left stays left in RTL");
    });
    output.textures_delta.clear();
    let runs = text::take_text_runs(ctx);
    let (rect, _, _) = runs
        .iter()
        .find(|(_, _, s)| s.as_deref() == Some("abc"))
        .unwrap();
    assert!(
        rect.min.x > 180.,
        "RTL start must use the right edge of the text box: {rect:?}"
    );
    let bounds = |label: &str| {
        runs.iter()
            .find(|(_, _, s)| s.as_deref() == Some(label))
            .unwrap()
            .0
    };
    assert_eq!(bounds("left").min.x, 10.);
    assert!((bounds("right").max.x - 210.).abs() < 0.1);
    assert!((bounds("center").center().x - 110.).abs() < 0.1);
    let mut output = ctx.run_ui(Default::default(), |ui| {
        assert!(
            ui.ctx()
                .fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(12.), "سلام עברית")),
            "bundled UI fallbacks must cover Arabic and Hebrew"
        );
        let painter = ui.painter();
        let hebrew = painter.layout_no_wrap(
            "אבג".into(),
            egui::FontId::proportional(12.),
            egui::Color32::WHITE,
        );
        assert_eq!(
            hebrew.rows[0]
                .glyphs
                .iter()
                .filter(|g| g.advance_width > 0.)
                .map(|g| g.chr)
                .collect::<String>(),
            "גבא",
            "pure Hebrew run uses visual RTL glyph order"
        );
        assert!(
            text::measure(painter, "لا", FontSpec::new(20.))
                < text::measure(painter, "ل", FontSpec::new(20.))
                    + text::measure(painter, "ا", FontSpec::new(20.)),
            "Arabic joining uses shaping, not isolated glyphs"
        );
        for (y, label) in [(150., "سلام"), (180., "אבג")] {
            let runs = [text::Run {
                text: label,
                spec: FontSpec::new(12.),
                color: egui::Color32::WHITE,
                underline: true,
            }];
            let lines = text::wrap_runs(painter, &runs, 200.);
            let painted = text::draw(
                painter,
                egui::pos2(10. + lines[0][0].x, y),
                label,
                FontSpec::new(12.),
                egui::Color32::WHITE,
            );
            assert!(
                (painted.max.x - 210.).abs() < 0.1,
                "actual shaped RTL line box must end at its edge: {label} {painted:?}"
            );
            text::draw_lines(painter, egui::pos2(10., y), &runs, &lines, 20.);
        }
    });
    output.textures_delta.clear();
}

fn assert_rtl_buttons(ctx: &egui::Context) {
    use paintrs::kit::{
        inputs,
        text::{self, FontSpec},
        window,
    };
    let mut problems = vec![];
    let mut expected_centers = vec![];
    let mut output=ctx.run_ui(Default::default(),|ui| {
        for (i,label) in ["سلام","עברית","إظهار >>"].into_iter().enumerate() {
            let rect=egui::Rect::from_min_size(egui::pos2(20.,240.+i as f32*30.),egui::vec2(180.,24.));
            inputs::push_button(ui,("rtl-push",i),rect,label,true,false);
            expected_centers.push((label,rect.center().x));
        }
        for (i,label) in ["تأكيد","אישור"].into_iter().enumerate() {
            let measured=text::measure(ui.painter(),label,FontSpec::new(12.));
            let width=window::dialog_button_width(ui.painter(),label,0.);
            if (width-measured-26.).abs()>0.1 {
                problems.push(format!("dialog width {label}: {width}, shaped label {measured} plus CSS padding/borders 26"));
            }
            let rect=egui::Rect::from_min_size(egui::pos2(20.,350.+i as f32*30.),egui::vec2(width,24.));
            window::dialog_button(ui,("rtl-dialog",i),rect,label,false);
            expected_centers.push((label,rect.center().x));
        }
    });
    for (label, want) in expected_centers {
        let shape = output
            .shapes
            .iter()
            .find_map(|s| match &s.shape {
                egui::epaint::Shape::Text(t) if t.galley.job.text == label => Some(t),
                _ => None,
            })
            .unwrap();
        let leading = shape.galley.job.sections[0].leading_space;
        let center = shape.pos.x + leading + (shape.galley.size().x - leading) / 2.;
        if (center - want).abs() > 0.1 {
            problems.push(format!(
                "real button label {label}: painted layout center {center}, button center {want}"
            ));
        }
    }
    output.textures_delta.clear();
    assert!(
        problems.is_empty(),
        "RTL shared-button metric failures: {problems:#?}"
    );
}

fn assert_text_status(expected: &str) {
    use pdn_core::{
        app::{AppWorkspace, MeasurementUnit, ToolType},
        tool::{MouseButton, PointerInput, PointerType, text::TextTool},
    };
    let mut app = AppWorkspace::default();
    app.create_blank_document_in_new_workspace(120, 80, 100., false);
    app.set_active_tool_from_type(ToolType::Text);
    app.measurement_unit = MeasurementUnit::Inch;
    // Tracking reports the first rendered line origin; its y differs from the click.
    let input = PointerInput {
        pointer_type: PointerType::Mouse,
        samples: vec![],
    };
    app.with_tool(|t, a| {
        t.on_mouse_down(a, 20., 20., MouseButton::Left, &input);
        t.as_any_mut()
            .downcast_mut::<TextTool>()
            .unwrap()
            .insert_text(a, "A");
        t.on_mouse_down(a, 80., 50., MouseButton::Right, &input);
    });
    assert_eq!(app.tool.as_ref().unwrap().status_text(), Some(expected));
    app.set_measurement_unit(MeasurementUnit::Centimeter);
    assert_eq!(
        app.tool.as_ref().unwrap().status_text(),
        Some(expected),
        "unit change alone preserves Text's event-triggered status"
    );
}

fn assert_selection_status(expected: &str) {
    use pdn_core::{
        app::{AppWorkspace, MeasurementUnit},
        selection::CombineMode,
        util::geom::Rectangle,
    };
    let mut app = AppWorkspace::default();
    app.create_blank_document_in_new_workspace(120, 80, 100., false);
    app.measurement_unit = MeasurementUnit::Inch;
    let selection = &mut app.active_ws_mut().unwrap().selection;
    selection.set_continuation(&Rectangle::new(20., 30., 40., 50.), CombineMode::Replace);
    selection.commit_continuation();
    assert_eq!(
        pdn_core::app::selection_info_text(&app).as_deref(),
        Some(expected)
    );
}

fn pack() -> Vec<u8> {
    use std::io::{Cursor, Write};
    let mut zip = zip::ZipWriter::new(Cursor::new(vec![]));
    for (path, data) in [
        (
            "asset-pack.json",
            json!({"formatVersion":1,"name":"Language Pack"}),
        ),
        (
            "lang/en.json",
            json!({"packOnly":"pack English", "menu":{"file":{"saveAs":{"text":"Pack English Save"}}}}),
        ),
        (
            "lang/de.json",
            json!({"mainToolBarForm":{"text":"Werkzeuge"}, "menu":{"file":{"saveAs":{"text":"Kopie speichern"}}},"measurementUnit":{"inch":{"abbreviation":"Zoll"}}}),
        ),
        ("lang/ar.json", json!({"mainToolBarForm":{"text":"أدوات"}})),
        ("lang/he.json", json!({"mainToolBarForm":{"text":"כלים"}})),
    ] {
        zip.start_file(path, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data.to_string().as_bytes()).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn settle(h: &mut Harness<'_, PaintApp>) {
    for _ in 0..100 {
        h.run();
        if !h.state().asset_library.busy() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("asset import did not finish");
}
fn choose(h: &mut Harness<'_, PaintApp>, language: &str) {
    h.get_by_label("Language:").click();
    h.run();
    h.get_by_label(language).click();
    h.run();
}
fn reload_scenario() {
    let mut h = Harness::builder().with_max_steps(16).with_size(egui::vec2(1280.,800.)).build_eframe(|cc| PaintApp::new_for_test(&cc.egui_ctx, json!({"paintdotjs.welcome.dismissed":"true","paintdotjs.settings.v1":"{\"ui\":{\"language\":\"en\"}}"}).as_object().unwrap().clone()));
    h.run();
    h.state_mut().asset_library.import(
        "assets",
        vec![PickedFile::snapshot("languages.zip".into(), pack())],
    );
    settle(&mut h);
    h.state_mut()
        .app
        .ui_requests
        .push(pdn_core::action::UiRequest::OpenSettings(Some("ui".into())));
    h.run();
    let documents = h.state().app.documents.len();
    choose(&mut h, "de");
    let dictionaries: serde_json::Value =
        serde_json::from_str(h.state().prefs.get(TRANSLATIONS_KEY).unwrap()).unwrap();
    assert!(
        dictionaries.get("de").is_some(),
        "preference change must prepare its selected dictionary"
    );
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Save As...");
    assert_eq!(pdn_core::app::format_fixed2(1.25), "1.25");
    h.get_by_label("Reload now").click();
    h.run();
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Kopie speichern");
    assert_eq!(i18n::t("packOnly"), "pack English");
    assert_eq!(i18n::t("unknown.path"), "unknown.path");
    assert_eq!(h.state().shell.forms.windows[0].title, "Werkzeuge");
    assert_eq!(h.state().app.documents.len(), documents);
    assert!(h.state().shell.dialogs.top_is("settings"));
    assert_eq!(pdn_core::app::format_fixed2(1.25), "1,25");
    assert_eq!(pdn_core::app::format_n(1234.5, 2), "1.234,50");
    assert_text_status("Anchor point: 0,20Zoll, 0,09Zoll");
    assert_selection_status(
        "Selection top left: 0,20Zoll, 0,30Zoll. Bounding rectangle size: 0,40Zoll × 0,50Zoll. Area: 0,20 inches square",
    );
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .document
        .resolution = 100.;
    h.state_mut()
        .app
        .set_measurement_unit(pdn_core::app::MeasurementUnit::Inch);
    assert_eq!(
        h.state().app.to_unit(125.),
        pdn_core::app::UnitValue::Formatted("1,25".into())
    );
    let screen = h
        .state()
        .app
        .active_ws()
        .unwrap()
        .view
        .to_screen_position(pdn_core::util::geom::Point::new(125., 50.));
    h.state_mut().app.last_mouse = (screen.x, screen.y);
    h.state_mut().app.fire(pdn_core::app::AppEvent::MouseMove {
        x: screen.x,
        y: screen.y,
    });
    h.run();
    let runs = paintrs::kit::text::take_text_runs(&h.ctx);
    assert!(
        runs.iter()
            .any(|(_, _, s)| s.as_deref() == Some("1,25, 0,50")),
        "pointer status uses the applied culture"
    );
    assert!(
        runs.iter()
            .any(|(_, _, s)| s.as_deref() == Some("8,00 × 6,00")),
        "size status uses the applied culture"
    );
    choose(&mut h, "ar");
    assert!(!paintrs::language::is_rtl(&h.ctx));
    h.get_by_label("Reload now").click();
    h.run();
    assert!(paintrs::language::is_rtl(&h.ctx));
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Pack English Save");
    assert_eq!(h.state().shell.forms.windows[0].title, "أدوات");
    choose(&mut h, "he");
    assert_eq!(
        i18n::t("mainToolBarForm.text"),
        "أدوات",
        "requested Hebrew waits for reload"
    );
    h.get_by_label("Reload now").click();
    h.run();
    assert_eq!(h.state().shell.forms.windows[0].title, "כלים");
    assert!(paintrs::language::is_rtl(&h.ctx));
    let stored: serde_json::Value =
        serde_json::from_str(h.state().prefs.get(TRANSLATIONS_KEY).unwrap()).unwrap();
    assert!(
        stored.get("ar").is_none(),
        "reload preparation removes previous Arabic dictionary"
    );
    h.get_by_label("Language:").click();
    h.run();
    h.event(egui::Event::Key {
        key: egui::Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    });
    h.run();
    assert!(
        h.state().shell.dialogs.top_is("settings"),
        "an RTL language popup owns Escape before the modal"
    );
    assert_eq!(h.state().app.documents.len(), documents);
    // Removing the active pack changes persisted reload state, then fresh initialization clears it.
    let ctx = h.ctx.clone();
    let app = h.state_mut();
    app.asset_library
        .activate("default", &ctx, &mut app.prefs)
        .unwrap();
    paintrs::apply_language(&h.ctx, &h.state().prefs, &h.state().settings);
    assert_eq!(i18n::t("packOnly"), "packOnly");
    assert_eq!(i18n::t("menu.file.saveAs.text"), "Save As...");
    assert!(!paintrs::language::is_rtl(&h.ctx));
    assert!(
        h.ctx
            .fonts_mut(|f| f.has_glyphs(&egui::FontId::proportional(12.), "سلام עברית")),
        "pack removal preserves bundled UI glyph fallback"
    );
}
