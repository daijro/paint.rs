//! Task42k: keyboard outcomes from adjustments-and-effect-framework §§7–8 and R94.
use super::*;
use egui::{Key, Modifiers, accesskit::Role};
use egui_kittest::kittest::{NodeT, Queryable};
use pdn_core::effect::{EffectDef, Values, range};

fn open_def(h: &mut Harness<'_, paintrs::PaintApp>, def: EffectDef, values: Values) {
    let s = pdn_core::effect::session::EffectSession::begin(&mut h.state_mut().app, def).unwrap();
    h.state_mut().shell.dialogs.push(Box::new(
        paintrs::dialogs::effect_config::EffectConfigDialog::new(s, values),
    ));
    h.run_steps(4);
}

type EffectResult = std::rc::Rc<std::cell::Cell<Option<paintrs::dialogs::DialogResult>>>;

// Observe the real parent result without changing DialogStack's production API.
struct RecordedEffect {
    dialog: paintrs::dialogs::effect_config::EffectConfigDialog,
    result: EffectResult,
}

impl paintrs::dialogs::Dialog for RecordedEffect {
    fn title(&self) -> String {
        self.dialog.title()
    }

    fn show(
        &mut self,
        ui: &mut egui::Ui,
        cx: &mut paintrs::dialogs::DialogCx,
    ) -> paintrs::dialogs::DialogState {
        let state = self.dialog.show(ui, cx);
        if let paintrs::dialogs::DialogState::Closed(result) = state {
            self.result.set(Some(result));
        }
        state
    }

    fn persistence_id(&self) -> Option<&'static str> {
        self.dialog.persistence_id()
    }
}

fn open_recorded_def(
    h: &mut Harness<'_, paintrs::PaintApp>,
    def: EffectDef,
    values: Values,
) -> EffectResult {
    let result = std::rc::Rc::new(std::cell::Cell::new(None));
    let session =
        pdn_core::effect::session::EffectSession::begin(&mut h.state_mut().app, def).unwrap();
    h.state_mut().shell.dialogs.push(Box::new(RecordedEffect {
        dialog: paintrs::dialogs::effect_config::EffectConfigDialog::new(session, values),
        result: result.clone(),
    }));
    h.run_steps(4);
    result
}

fn select_pixels(h: &Harness<'_, paintrs::PaintApp>) -> Vec<u8> {
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

fn finish(h: &mut Harness<'_, paintrs::PaintApp>, id: &str) -> Values {
    h.get_by_label("OK").click();
    h.run_steps(2);
    assert!(!h.state().shell.dialogs.is_open());
    h.state().app.effect_tokens[id].clone()
}

fn press(h: &mut Harness<'_, paintrs::PaintApp>, key: Key) {
    h.key_press(key);
    h.run_steps(3);
}

fn key_event(h: &mut Harness<'_, paintrs::PaintApp>, pressed: bool, repeat: bool) {
    h.event(egui::Event::Key {
        key: Key::Space,
        physical_key: None,
        pressed,
        repeat,
        modifiers: Modifiers::NONE,
    });
    h.run();
}

// Catches the old close-button initial focus and the duplicate checkbox-label Tab stop.
#[test]
fn native_initial_focus_and_checkbox_traversal() {
    let mut h = app_harness();
    open_effect(&mut h, "addNoiseEffect");
    assert_eq!(focused_node(&h), ("intensity".into(), "64".into()));
    press(&mut h, Key::Escape);
    open_probe(&mut h);
    assert_eq!(focused_node(&h), ("amount".into(), "360".into()));
    h.get_by_role_and_label(Role::CheckBox, "Flag").focus();
    h.run_steps(3);
    let flag = h
        .get_by_role_and_label(Role::CheckBox, "Flag")
        .accesskit_node()
        .id();
    press(&mut h, Key::Tab);
    assert_ne!(h.get_by(|n| n.is_focused()).accesskit_node().id(), flag);
    assert_ne!(
        focused_node(&h).0,
        "Gated",
        "read-only children are skipped"
    );
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(3);
    assert_eq!(h.get_by(|n| n.is_focused()).accesskit_node().id(), flag);
    // No hidden page editors or pan pad occurs anywhere in the dialog's Tab cycle.
    let mut seen = std::collections::HashSet::new();
    let mut help_stops = 0;
    for _ in 0..40 {
        let n = h.get_by(|n| n.is_focused());
        if !seen.insert(n.accesskit_node().id()) {
            break;
        }
        if n.accesskit_node().role() == Role::Button
            && n.accesskit_node().label().as_deref() == Some("Help")
        {
            help_stops += 1;
        }
        assert!(!matches!(
            n.accesskit_node().label().as_deref(),
            Some("Gated" | "Words" | "Spot")
        ));
        press(&mut h, Key::Tab);
    }
    assert_eq!(help_stops, 1, "one Help button in the complete Tab cycle");
    assert_eq!(
        seen.len(),
        13,
        "tab, Amount slider/input, Radius slider/input/reset, Flag, Tint, Preview, OK, Cancel, close, Help"
    );
}

// Catches key-down/repeat toggles and Enter's synthetic egui checkbox click.
#[test]
fn checkbox_space_release_and_enter_outcomes() {
    let mut h = app_harness();
    open_probe(&mut h);
    h.get_by_role_and_label(Role::CheckBox, "Flag").focus();
    h.run_steps(2);
    key_event(&mut h, true, false);
    assert_eq!(
        h.get_by_role_and_label(Role::CheckBox, "Flag")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    key_event(&mut h, true, true);
    assert_eq!(
        h.get_by_role_and_label(Role::CheckBox, "Flag")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::False)
    );
    key_event(&mut h, false, false);
    assert_eq!(
        h.get_by_role_and_label(Role::CheckBox, "Flag")
            .accesskit_node()
            .toggled(),
        Some(egui::accesskit::Toggled::True)
    );
    assert!(h.state().shell.dialogs.is_open());
    let before = undo_len(&h);
    press(&mut h, Key::Enter);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(h.state().app.effect_tokens["bbProbe"]["flag"], true);
    assert_eq!(undo_len(&h), before + 1, "one acceptance");
}

// Values are literal analyst vectors, not values derived from slider metadata.
#[test]
fn registered_slider_keyboard_values() {
    for (effect, key, initial, input, want) in [
        (
            "brightnessAndContrastAdjustment",
            "brightness",
            10.0,
            Key::ArrowRight,
            11.0,
        ),
        (
            "brightnessAndContrastAdjustment",
            "brightness",
            10.0,
            Key::ArrowDown,
            9.0,
        ),
        (
            "brightnessAndContrastAdjustment",
            "brightness",
            99.0,
            Key::PageUp,
            100.0,
        ),
        (
            "highlightsAndShadowsEffect",
            "radius",
            1.25,
            Key::ArrowRight,
            1.5,
        ),
        (
            "highlightsAndShadowsEffect",
            "radius",
            1.25,
            Key::PageUp,
            3.75,
        ),
        (
            "highlightsAndShadowsEffect",
            "radius",
            1.25,
            Key::PageDown,
            0.0,
        ),
        ("rotateZoomEffect", "panX", 0.0, Key::PageUp, 0.25),
        ("reduceNoiseEffect", "strength", 0.4, Key::PageUp, 0.5),
        ("cloudsEffect", "scale", 250.0, Key::PageUp, 255.0),
        ("addNoiseEffect", "coverage", 10.125, Key::ArrowRight, 11.13),
        (
            "bokehEffect",
            "radius",
            2.0,
            Key::ArrowRight,
            5.826920333333333,
        ),
        ("blurEffect", "gamma", 0.0, Key::PageUp, 0.199712),
        (
            "blurEffect",
            "gamma",
            -0.25,
            Key::ArrowRight,
            -0.03309191919191919,
        ),
        (
            "vignetteEffect",
            "radius",
            0.5,
            Key::ArrowRight,
            0.9206464102564103,
        ),
        (
            "bbCombinedAdjustments",
            "temperature",
            0.4,
            Key::ArrowLeft,
            -0.6,
        ),
    ] {
        let mut h = app_harness();
        let def = pdn_core::effect::find(effect).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert(key.into(), serde_json::json!(initial));
        open_def(&mut h, def, values);
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        press(&mut h, input);
        assert!(h.state().shell.dialogs.is_open(), "{effect} {key}");
        let values = finish(&mut h, effect);
        assert!(
            (values[key].as_f64().unwrap() - want).abs() < 1e-9,
            "{effect} {key}: {:?}, want {want}",
            values[key]
        );
    }
}

#[test]
fn slider_declines_modifiers_and_endpoints() {
    let mut h = app_harness();
    open_effect(&mut h, "addNoiseEffect");
    for (mods, key) in [
        (Modifiers::NONE, Key::Space),
        (Modifiers::NONE, Key::Home),
        (Modifiers::NONE, Key::End),
        (Modifiers::CTRL, Key::ArrowRight),
        (Modifiers::SHIFT, Key::ArrowRight),
        (Modifiers::ALT, Key::ArrowRight),
        (Modifiers::SHIFT, Key::PageUp),
    ] {
        h.get_by_role_and_label(Role::Slider, "intensity").focus();
        h.run_steps(2);
        h.key_press_modifiers(mods, key);
        h.run_steps(2);
    }
    assert_eq!(finish(&mut h, "addNoiseEffect")["intensity"], 64.0);
}

#[test]
fn hue_slider_clamps_while_builtin_numeric_wraps() {
    for numeric in [false, true] {
        let mut h = app_harness();
        let def = pdn_core::effect::find("hueAndSaturationAdjustment").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("hue".into(), serde_json::json!(180));
        open_def(&mut h, def, values);
        if !numeric {
            h.get_by_role_and_label(Role::Slider, "hue").focus();
            h.run_steps(2);
        }
        press(&mut h, Key::ArrowUp);
        assert_eq!(
            finish(&mut h, "hueAndSaturationAdjustment")["hue"],
            if numeric { -180.0 } else { 180.0 }
        );
    }
}

#[test]
fn pan_pad_is_not_a_tab_destination() {
    let mut h = app_harness();
    open_effect(&mut h, "rotateZoomEffect");
    h.get_by_role_and_label(Role::Slider, "panX").focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(3);
    let previous = h.get_by(|n| n.is_focused());
    assert_ne!(previous.accesskit_node().role(), Role::Image);
    // The preceding enabled control is the rollAmount Reset, not the graphical pan square.
    assert_eq!(previous.accesskit_node().role(), Role::Button);
    press(&mut h, Key::Escape);
    open_probe(&mut h);
    h.get_by_role_and_label(Role::Tab, "Second").click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::ComboBox, "Mode").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    assert_eq!(
        focused_node(&h),
        ("X".into(), "0".into()),
        "plugin pan pad is skipped too"
    );
    assert_eq!(
        finish(&mut h, "bbProbe")["spot"],
        serde_json::json!([0.0, 0.0])
    );
}

// Generic exponential arithmetic from §7.7, exercised through real slider keys and acceptance.
#[test]
fn exponential_keyboard_uses_transformed_absolute_steps() {
    for (key, want) in [
        (Key::ArrowRight, 9.0),
        (Key::ArrowLeft, 1.0),
        (Key::PageUp, 17.943696),
        (Key::PageDown, 0.0),
    ] {
        let mut h = app_harness();
        let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
        def.id = "keyboardProbe";
        def.controls = vec![
            range("value", "Value", 0.0, 100.0, 4.0)
                .exponential()
                .slider_keys(2, 1.0, 5.0, true),
        ];
        let values = pdn_core::effect::defaults(&def);
        open_def(&mut h, def, values);
        h.get_by_role_and_label(Role::Slider, "value").focus();
        h.run_steps(2);
        press(&mut h, key);
        let got = finish(&mut h, "keyboardProbe")["value"].as_f64().unwrap();
        assert!((got - want).abs() < 1e-9, "{key:?}: {got} != {want}");
    }
}

#[test]
fn focused_reset_cancel_and_slider_escape_have_single_actions() {
    let mut h = app_harness();
    let def = pdn_core::effect::find("addNoiseEffect").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("intensity".into(), serde_json::json!(70));
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Slider, "intensity").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Enter);
    assert!(h.state().shell.dialogs.is_open(), "Reset must not accept");
    assert_eq!(finish(&mut h, "addNoiseEffect")["intensity"], 64.0);
    let history = undo_len(&h);
    let remembered = h.state().app.effect_tokens["addNoiseEffect"].clone();
    open_effect(&mut h, "addNoiseEffect");
    h.get_by_label("Cancel").focus();
    h.run_steps(2);
    press(&mut h, Key::Enter);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), history);
    assert_eq!(h.state().app.effect_tokens["addNoiseEffect"], remembered);
    open_effect(&mut h, "addNoiseEffect");
    h.get_by_role_and_label(Role::Slider, "intensity").focus();
    h.run_steps(2);
    press(&mut h, Key::Escape);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), history);
}

#[test]
fn checkbox_is_the_first_editor_when_it_is_the_first_property() {
    let mut h = app_harness();
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "checkboxProbe";
    def.controls = vec![pdn_core::effect::Control::Checkbox {
        key: "flag",
        label: "First flag",
        default: false,
    }];
    let values = pdn_core::effect::defaults(&def);
    open_def(&mut h, def, values);
    assert!(
        h.get_by_role_and_label(Role::CheckBox, "First flag")
            .is_focused()
    );
    press(&mut h, Key::Enter);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(h.state().app.effect_tokens["checkboxProbe"]["flag"], false);
}

#[test]
fn subresolution_slider_step_does_not_edit_the_property() {
    let mut h = app_harness();
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "resolutionProbe";
    def.controls = vec![range("value", "Value", 0.0, 100.0, 1.0).slider_keys(2, 0.004, 5.0, false)];
    let values = pdn_core::effect::defaults(&def);
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Slider, "value").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    assert_eq!(finish(&mut h, "resolutionProbe")["value"], 1.0);
}

#[test]
fn first_selection_skips_disabled_properties() {
    use pdn_core::effect::bb::ui::IndirectUi;
    static UI: IndirectUi = IndirectUi {
        enabled: |_, key| key != "disabled",
        ..IndirectUi::DEFAULT
    };
    let mut h = app_harness();
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "disabledProbe";
    def.indirect = Some(&UI);
    def.controls = vec![
        range("disabled", "Disabled", 0.0, 10.0, 2.0),
        pdn_core::effect::Control::Checkbox {
            key: "flag",
            label: "First enabled",
            default: false,
        },
    ];
    let values = pdn_core::effect::defaults(&def);
    open_def(&mut h, def, values);
    assert!(
        h.get_by_role_and_label(Role::CheckBox, "First enabled")
            .is_focused()
    );
    press(&mut h, Key::Space);
    let values = finish(&mut h, "disabledProbe");
    assert_eq!(values["disabled"], 2.0);
    assert_eq!(values["flag"], true);
}

#[test]
fn checkbox_box_and_caption_clicks_share_focus_and_enter() {
    for offset in [8.0, 35.0] {
        let mut h = app_harness();
        let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
        def.id = "clickProbe";
        def.controls = vec![pdn_core::effect::Control::Checkbox {
            key: "flag",
            label: "Click this caption",
            default: false,
        }];
        let values = pdn_core::effect::defaults(&def);
        open_def(&mut h, def, values);
        let r = h
            .get_by_role_and_label(Role::CheckBox, "Click this caption")
            .rect();
        click(&mut h, r.min.x + offset, r.center().y);
        assert!(
            h.get_by_role_and_label(Role::CheckBox, "Click this caption")
                .is_focused()
        );
        press(&mut h, Key::Enter);
        assert_eq!(h.state().app.effect_tokens["clickProbe"]["flag"], true);
    }
}

#[test]
fn modified_enter_and_escape_do_not_invoke_dialog_defaults() {
    let mut h = app_harness();
    open_effect(&mut h, "addNoiseEffect");
    for label in ["OK", "Cancel"] {
        h.get_by_label(label).focus();
        h.run_steps(2);
        for modifiers in [Modifiers::CTRL, Modifiers::ALT] {
            for key in [Key::Enter, Key::Escape] {
                h.key_press_modifiers(modifiers, key);
                h.run_steps(2);
                assert!(
                    h.state().shell.dialogs.is_open(),
                    "{label} {modifiers:?} {key:?}"
                );
            }
        }
    }
    press(&mut h, Key::Escape);
    assert!(!h.state().shell.dialogs.is_open());
}

// §7.3: focused push buttons use the same release activation as property checkboxes.
#[test]
fn push_button_space_waits_for_release_and_clears_on_focus_loss() {
    for label in ["OK", "Cancel"] {
        let mut h = app_harness();
        let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
        def.id = "releaseProbe";
        def.controls = vec![range("value", "Value", 0.0, 10.0, 1.0)];
        let values = pdn_core::effect::defaults(&def);
        open_def(&mut h, def, values);
        let history = undo_len(&h);
        h.get_by_label(label).focus();
        h.run_steps(2);
        key_event(&mut h, true, false);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{label}: no action on Space down"
        );
        key_event(&mut h, true, true);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{label}: no action on repeat"
        );
        h.input_mut().focused = false;
        h.event(egui::Event::WindowFocused(false));
        h.run();
        h.input_mut().focused = true;
        h.event(egui::Event::WindowFocused(true));
        h.run();
        key_event(&mut h, false, false);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{label}: losing focus disarms Space"
        );
        h.get_by_label(label).focus();
        h.run_steps(2);
        key_event(&mut h, true, false);
        key_event(&mut h, true, true);
        assert!(h.state().shell.dialogs.is_open());
        key_event(&mut h, false, false);
        assert!(
            !h.state().shell.dialogs.is_open(),
            "{label}: one release action"
        );
        let accepted = label == "OK";
        assert_eq!(
            h.state().app.effect_tokens.contains_key("releaseProbe"),
            accepted
        );
        assert_eq!(undo_len(&h), history + usize::from(accepted));
        key_event(&mut h, false, false);
        assert_eq!(
            undo_len(&h),
            history + usize::from(accepted),
            "a second release does nothing"
        );
    }
}

#[test]
fn push_button_reset_space_has_no_action_until_release() {
    for lose_focus in [false, true] {
        let mut h = app_harness();
        let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
        def.id = "resetReleaseProbe";
        def.controls = vec![range("value", "Value", 0.0, 10.0, 1.0)];
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("value".into(), serde_json::json!(5));
        open_def(&mut h, def, values);
        let r = h.get_by_role_and_label(Role::Slider, "value").rect();
        h.get_by_role_and_label(Role::Slider, "value").focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h).0, "Reset");
        let number_pixels = |h: &mut Harness<'_, paintrs::PaintApp>| {
            let image = h.render().unwrap();
            image::imageops::crop_imm(
                &image,
                (r.max.x + 8.0) as u32,
                (r.center().y - 8.0) as u32,
                55,
                16,
            )
            .to_image()
        };
        let before = number_pixels(&mut h);
        key_event(&mut h, true, false);
        assert!(
            number_pixels(&mut h) == before,
            "Reset must leave the visible numeric value unchanged on down"
        );
        key_event(&mut h, true, true);
        assert!(
            number_pixels(&mut h) == before,
            "Reset must ignore repeated downs"
        );
        if lose_focus {
            h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
            h.run_steps(2);
        }
        key_event(&mut h, false, false);
        assert!(h.state().shell.dialogs.is_open(), "Reset never accepts");
        assert_eq!(
            finish(&mut h, "resetReleaseProbe")["value"],
            if lose_focus { 5.0 } else { 1.0 }
        );
    }
}

#[test]
fn push_button_disabling_a_row_drops_its_held_space() {
    use pdn_core::effect::bb::ui::IndirectUi;
    static UI: IndirectUi = IndirectUi {
        enabled: |values, key| key != "value" || pdn_core::effect::flag(values, "enabled"),
        ..IndirectUi::DEFAULT
    };
    let mut h = app_harness();
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "disabledSpaceProbe";
    def.indirect = Some(&UI);
    def.controls = vec![
        pdn_core::effect::Control::Checkbox {
            key: "enabled",
            label: "Enable value",
            default: true,
        },
        range("value", "Value", 0.0, 10.0, 1.0),
    ];
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("value".into(), serde_json::json!(5));
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Slider, "value").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    key_event(&mut h, true, false);
    h.get_by_role_and_label(Role::CheckBox, "Enable value")
        .click();
    h.run_steps(2);
    assert!(
        h.get_by_role_and_label(Role::Slider, "value")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_role_and_label(Role::CheckBox, "Enable value")
        .click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Slider, "value").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    key_event(&mut h, false, false);
    assert!(h.state().shell.dialogs.is_open());
    assert_eq!(finish(&mut h, "disabledSpaceProbe")["value"], 5.0);
}

// §7.8 and the controller's scope decision preserve custom-editor initial focus/Enter.
#[test]
fn custom_dialog_initial_enter_keeps_existing_acceptance() {
    for effect in ["curvesEffect", "levelsEffect"] {
        let mut h = app_harness();
        open_effect(&mut h, effect);
        press(&mut h, Key::Enter);
        assert!(
            !h.state().shell.dialogs.is_open(),
            "{effect}: opening Enter still accepts"
        );
        assert!(h.state().app.effect_tokens.contains_key(effect));
    }
}

// A pending numeric edit must not overwrite the slider's primary press on blur.
#[test]
fn numeric_edit_then_slider_press_keeps_pointer_value() {
    let mut h = app_harness();
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "pointerProbe";
    def.controls = vec![range("value", "Value", 0.0, 100.0, 10.0)];
    let values = pdn_core::effect::defaults(&def);
    open_def(&mut h, def, values);
    h.event(egui::Event::Text("20".into()));
    h.run();
    assert_eq!(focused_node(&h), ("value".into(), "20".into()));
    let slider = h.get_by_role_and_label(Role::Slider, "value").rect();
    click(&mut h, slider.max.x - 1.0, slider.center().y);
    assert!(h.get_by_role_and_label(Role::Slider, "value").is_focused());
    assert_eq!(finish(&mut h, "pointerProbe")["value"], 100.0);
}

// BB12: arrow/Page metadata, displayed precision and numeric increments are independent.
#[test]
fn bb12_strength_and_zoom_keys() {
    for (effect, key, tab, initial, numeric, input, want, shown) in [
        (
            "bbInnerShadowObject",
            "strength",
            0,
            0.4,
            false,
            Key::ArrowRight,
            0.41,
            "0.41",
        ),
        (
            "bbInnerShadowSelection",
            "strength",
            0,
            0.4,
            false,
            Key::PageUp,
            0.5,
            "0.50",
        ),
        (
            "bbDropShadowPlus",
            "strength",
            0,
            0.4,
            false,
            Key::ArrowRight,
            0.45,
            "0.450",
        ),
        (
            "bbDropShadowPlus",
            "strength",
            0,
            0.4,
            false,
            Key::PageUp,
            0.65,
            "0.650",
        ),
        (
            "bbDropShadowPlus",
            "strength",
            0,
            0.405,
            true,
            Key::ArrowUp,
            0.415,
            "0.415",
        ),
        (
            "bbDropShadowPlus",
            "zoom",
            2,
            1.0,
            false,
            Key::PageDown,
            0.75,
            "0.750",
        ),
        (
            "bbDropShadowPlus",
            "zoom",
            2,
            1.0,
            true,
            Key::ArrowDown,
            0.99,
            "0.990",
        ),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find(effect).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert(key.into(), serde_json::json!(initial));
        values.insert("shadowType".into(), serde_json::json!(tab));
        open_def(&mut h, def, values);
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        if numeric {
            press(&mut h, Key::Tab);
        }
        press(&mut h, input);
        if !numeric {
            press(&mut h, Key::Tab);
        }
        assert_eq!(focused_node(&h), (key.into(), shown.into()));
        let got = finish(&mut h, effect)[key].as_f64().unwrap();
        assert!(
            (got - want).abs() < 1e-9,
            "{effect} {key}: {got}, want {want}"
        );
    }
}

// BB12/R107: strict endpoint crossing discards overshoot; direct typing clamps.
#[test]
fn bb12_angle_numeric_steps_and_pending_text() {
    for (initial, typed, input, want) in [
        (45.0, None, Key::ArrowUp, 46.0),
        (45.0, None, Key::ArrowDown, 44.0),
        (179.0, None, Key::ArrowUp, 180.0),
        (180.0, None, Key::ArrowUp, -180.0),
        (-180.0, None, Key::ArrowDown, 180.0),
        (179.75, None, Key::ArrowUp, -180.0),
        (-179.75, None, Key::ArrowDown, 180.0),
        (45.25, None, Key::ArrowUp, 46.25),
        (45.0, Some("181"), Key::ArrowDown, 179.0),
        (45.0, Some("-181"), Key::ArrowUp, -179.0),
        (45.0, Some("181"), Key::Tab, 180.0),
        (45.0, Some("-181"), Key::Tab, -180.0),
        (45.0, None, Key::PageUp, 45.0),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find("bbInnerShadowObject").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("angle".into(), serde_json::json!(initial));
        open_def(&mut h, def, values);
        assert_eq!(focused_node(&h), ("angle".into(), format!("{initial:.3}")));
        if let Some(text) = typed {
            h.event(egui::Event::Text(text.into()));
            h.run();
        }
        press(&mut h, input);
        assert_eq!(
            finish(&mut h, "bbInnerShadowObject")["angle"],
            serde_json::json!(want)
        );
    }
}

#[test]
fn bb12_location_companion_keys_and_tab_order() {
    for (axis, input, numeric, want) in [
        ("X", Key::ArrowRight, false, 0.05),
        ("Y", Key::PageUp, false, 0.25),
        ("X", Key::PageDown, false, -0.25),
        ("Y", Key::ArrowUp, true, 0.01),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find("bbDropShadowPlus").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("shadowType".into(), serde_json::json!(2));
        open_def(&mut h, def, values);
        let key = format!("location.{axis}");
        h.get_by_role_and_label(Role::Slider, &key).focus();
        h.run_steps(2);
        if numeric {
            press(&mut h, Key::Tab);
        }
        press(&mut h, input);
        if !numeric {
            press(&mut h, Key::Tab);
        }
        assert_eq!(focused_node(&h), (key.clone(), format!("{want:.3}")));
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h).0, "Reset");
        if axis == "X" {
            press(&mut h, Key::Tab);
            assert!(
                h.get_by_role_and_label(Role::Slider, "location.Y")
                    .is_focused()
            );
        }
        let token = finish(&mut h, "bbDropShadowPlus");
        let index = usize::from(axis == "Y");
        assert_eq!(token["location"][index], serde_json::json!(want));
        assert_eq!(token["location"][1 - index], serde_json::json!(0.0));
    }
}

fn bb12_harness() -> Harness<'static, paintrs::PaintApp> {
    let mut h = app_harness();
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(32, 32, 96.0, true);
    h.run_steps(2);
    h
}

// R120: omitted numeric increments inherit native1, independently of slider SmallChange=.01.
#[test]
fn bb12_inner_strength_native_numeric_default() {
    for (input, want) in [(Key::ArrowUp, 1.0), (Key::ArrowDown, 0.0)] {
        let mut h = bb12_harness();
        open_effect(&mut h, "bbInnerShadowObject");
        h.get_by_role_and_label(Role::Slider, "strength").focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        press(&mut h, input);
        assert_eq!(
            finish(&mut h, "bbInnerShadowObject")["strength"],
            serde_json::json!(want)
        );
    }
}

#[test]
fn bb12_native_numeric_preserves_unedited_precision_and_declines_modifiers() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbInnerShadowObject").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("angle".into(), serde_json::json!(45.2534));
    open_def(&mut h, def, values);
    for modifiers in [Modifiers::SHIFT, Modifiers::CTRL, Modifiers::ALT] {
        h.key_press_modifiers(modifiers, Key::ArrowUp);
        h.run_steps(2);
    }
    press(&mut h, Key::ArrowUp);
    press(&mut h, Key::ArrowUp);
    assert_eq!(focused_node(&h), ("angle".into(), "47.253".into()));
    let got = finish(&mut h, "bbInnerShadowObject")["angle"]
        .as_f64()
        .unwrap();
    assert!((got - 47.2534).abs() < 1e-10);
}

#[test]
fn bb12_numeric_spin_matches_keys_and_narrow_tilt_clamps() {
    for (key, initial, typed, up, want) in [
        ("angle", 179.75, None, true, -180.0),
        ("angle", 45.0, Some("181"), false, 179.0),
        ("tiltAmount", 90.0, None, true, 90.0),
    ] {
        let mut h = bb12_harness();
        let effect = if key == "angle" {
            "bbInnerShadowObject"
        } else {
            "bbDropShadowPlus"
        };
        let def = pdn_core::effect::find(effect).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert(key.into(), serde_json::json!(initial));
        values.insert("shadowType".into(), serde_json::json!(2));
        open_def(&mut h, def, values);
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        if let Some(text) = typed {
            h.key_press_modifiers(Modifiers::CTRL, Key::A);
            h.event(egui::Event::Text(text.into()));
            h.run();
        }
        let rect = h.get_by_role_and_label(Role::Slider, key).rect();
        let pos = egui::pos2(
            rect.max.x + 8.0 + 72.0 - 7.0,
            rect.center().y + if up { -4.0 } else { 4.0 },
        );
        h.event(egui::Event::PointerMoved(pos));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(3);
        let got = finish(&mut h, effect)[key].as_f64().unwrap();
        assert!((got - want).abs() < 1e-9, "{key}: {got} != {want}");
    }
}

#[test]
fn bb12_pan_initial_focus_clamp_reset_disabled_and_hidden() {
    use pdn_core::effect::bb::ui::{ControlUi, IndirectUi};
    static UI: IndirectUi = IndirectUi {
        controls: &[(
            "location",
            ControlUi {
                decimals: Some(3),
                pan_sliders: Some(pdn_core::effect::slider::SliderKeys {
                    decimals: 3,
                    small: 0.05,
                    large: 0.25,
                    exponential: false,
                }),
                ..ControlUi::NO_HEADER
            },
        )],
        enabled: |v, key| key != "location" || pdn_core::effect::flag(v, "allow"),
        ..IndirectUi::DEFAULT
    };
    let mut h = bb12_harness();
    let mut def = pdn_core::effect::find("bbDropShadowPlus").unwrap();
    def.id = "panCompanionProbe";
    def.controls.retain(|c| c.key() == "location");
    def.indirect = Some(&UI);
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("allow".into(), serde_json::json!(true));
    values.insert("location".into(), serde_json::json!([0.99, -0.99]));
    open_def(&mut h, def.clone(), values);
    assert_eq!(focused_node(&h), ("location.X".into(), "0.990".into()));
    h.get_by_role_and_label(Role::Slider, "location.X").focus();
    h.run_steps(2);
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(2);
    assert_ne!(
        h.get_by(|n| n.is_focused()).accesskit_node().role(),
        Role::Image
    );
    h.get_by_role_and_label(Role::Slider, "location.X").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h).1, "1.000");
    press(&mut h, Key::Tab);
    press(&mut h, Key::Enter);
    assert!(h.state().shell.dialogs.is_open());
    h.get_by_role_and_label(Role::Slider, "location.X").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h).1, "0.000");
    h.get_by_role_and_label(Role::Slider, "location.Y").focus();
    h.run_steps(2);
    press(&mut h, Key::PageDown);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h).1, "-1.000");
    let token = finish(&mut h, "panCompanionProbe");
    assert_eq!(token["location"], serde_json::json!([0.0, -1.0]));
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("allow".into(), serde_json::json!(false));
    open_def(&mut h, def, values);
    assert!(
        h.get_by_role_and_label(Role::Slider, "location.X")
            .accesskit_node()
            .is_disabled()
    );
    for _ in 0..8 {
        press(&mut h, Key::Tab);
        assert!(!focused_node(&h).0.starts_with("location."));
    }
    press(&mut h, Key::Escape);
    open_effect(&mut h, "bbDropShadowPlus");
    assert!(
        h.query_by_role_and_label(Role::Slider, "location.X")
            .is_none()
    );
    press(&mut h, Key::Escape);
}

#[test]
fn bb12_transient_inner_tab_and_persistent_drop_tab_reopen() {
    let mut h = bb12_harness();
    open_effect(&mut h, "bbInnerShadowObject");
    h.get_by_role_and_label(Role::Tab, " Options ").click();
    h.run_steps(2);
    let token = finish(&mut h, "bbInnerShadowObject");
    assert!(!token.contains_key("tab"));
    open_effect(&mut h, "bbInnerShadowObject");
    assert!(selected(&h, " Shadow "));
    press(&mut h, Key::Escape);
    open_effect(&mut h, "bbDropShadowPlus");
    h.get_by_role_and_label(Role::Tab, "Long Shadow").click();
    h.run_steps(2);
    assert_eq!(
        finish(&mut h, "bbDropShadowPlus")["shadowType"],
        serde_json::json!(1)
    );
    open_effect(&mut h, "bbDropShadowPlus");
    assert!(selected(&h, "Long Shadow"));
    press(&mut h, Key::Escape);
}

#[test]
fn bb12_wrapped_plugin_hue_discards_overshoot_and_parses_pending_text() {
    for (text, input, want) in [
        ("359.75", Key::ArrowUp, 0.0),
        ("361", Key::ArrowDown, 359.0),
        ("-1", Key::ArrowUp, 1.0),
    ] {
        let mut h = bb12_harness();
        open_probe(&mut h);
        h.event(egui::Event::Text(text.into()));
        h.run();
        press(&mut h, input);
        assert_eq!(finish(&mut h, "bbProbe")["amount"], serde_json::json!(want));
    }
}

fn select_probe() -> (Harness<'static, paintrs::PaintApp>, EffectResult) {
    let mut h = bb12_harness();
    let ws = h.state_mut().app.active_ws_mut().unwrap();
    for (i, px) in ws
        .document
        .layers
        .get_at_mut(0)
        .unwrap()
        .surface
        .data_mut()
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .enumerate()
    {
        px.copy_from_slice(&[(i % 16) as u8 * 10, (i / 32) as u8 * 7, 30, 255]);
    }
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "selectKeyboardProbe";
    def.controls = vec![pdn_core::effect::Control::Select {
        key: "choice",
        label: "Choice",
        default: "a",
        choices: &[("a", "Alpha"), ("b", "Beta"), ("c", "Gamma")],
    }];
    let values = pdn_core::effect::defaults(&def);
    let result = open_recorded_def(&mut h, def, values);
    h.get_by_role_and_label(Role::CheckBox, "Preview").click();
    h.run_steps(3);
    (h, result)
}

#[test]
fn focused_select_arrows_pick_without_closing_or_committing_parent() {
    use paintrs::dialogs::DialogResult;
    for (endpoint, arrow, action) in [
        (Key::End, Key::ArrowUp, "OK"),
        (Key::Home, Key::ArrowDown, "Cancel"),
    ] {
        let (mut h, result) = select_probe();
        let pixels = select_pixels(&h);
        let tokens = h.state().app.effect_tokens.clone();
        let history = undo_len(&h);
        h.get_by_label("Choice").focus();
        h.run_steps(2);
        let field = h.get_by_label("Choice").accesskit_node().id();
        press(&mut h, Key::Enter);
        press(&mut h, endpoint);
        press(&mut h, arrow);
        let arrow_focus = h.get_by_label("Choice").is_focused();
        assert!(h.query_by_label("Alpha").is_some());
        press(&mut h, Key::Enter);
        assert_eq!(result.get(), None);
        assert!(h.state().shell.dialogs.is_open());
        assert!(arrow_focus);
        assert_eq!(h.get_by(|n| n.is_focused()).accesskit_node().id(), field);
        assert_eq!(h.get_by_label("Choice").value().as_deref(), Some("Beta"));
        assert!(h.query_by_label("Alpha").is_none());
        assert_eq!(h.state().app.effect_tokens, tokens);
        assert_eq!(undo_len(&h), history);
        assert_eq!(select_pixels(&h), pixels);
        // Reopen, saturate the last option with consecutive arrows, then choose Beta again.
        for key in [
            Key::Enter,
            Key::End,
            Key::ArrowDown,
            Key::ArrowDown,
            Key::ArrowUp,
            Key::Enter,
        ] {
            press(&mut h, key);
            assert!(h.get_by_label("Choice").is_focused());
        }
        assert_eq!(h.get_by_label("Choice").value().as_deref(), Some("Beta"));
        assert_eq!(result.get(), None);
        assert_eq!(undo_len(&h), history);
        h.get_by_label(action).focus();
        h.run_steps(2);
        press(&mut h, Key::Enter);
        assert!(!h.state().shell.dialogs.is_open());
        if action == "OK" {
            assert_eq!(result.get(), Some(DialogResult::Ok));
            assert_eq!(
                h.state().app.effect_tokens["selectKeyboardProbe"]["choice"],
                "b"
            );
            assert_eq!(undo_len(&h), history + 1);
            let inverted: Vec<u8> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| [255 - p[0], 255 - p[1], 255 - p[2], p[3]])
                .collect();
            assert_eq!(select_pixels(&h), inverted);
        } else {
            assert_eq!(result.get(), Some(DialogResult::Cancel));
            assert_eq!(h.state().app.effect_tokens, tokens);
            assert_eq!(undo_len(&h), history);
            assert_eq!(select_pixels(&h), pixels);
        }
    }
}

#[test]
fn focused_select_first_arrow_after_new_keyboard_or_pointer_focus() {
    for (pointer, arrow, want) in [
        (false, Key::ArrowDown, "Beta"),
        (false, Key::ArrowUp, "Alpha"),
        (true, Key::ArrowDown, "Beta"),
        (true, Key::ArrowUp, "Alpha"),
        (false, Key::Escape, "Alpha"),
        (true, Key::Escape, "Alpha"),
    ] {
        let (mut h, result) = select_probe();
        h.get_by_role_and_label(Role::CheckBox, "Preview").focus();
        h.run_steps(2);
        assert!(
            h.get_by_role_and_label(Role::CheckBox, "Preview")
                .is_focused()
        );
        if pointer {
            h.get_by_label("Choice").click();
        } else {
            h.get_by_label("Choice").focus();
            h.key_press(Key::Enter);
        }
        h.step();
        assert!(h.get_by_label("Choice").is_focused());
        let list_visible_on_open = h.query_by_label("Gamma").is_some();
        assert_eq!(result.get(), None);
        // No settling frame between opening and the first arrow. Down/up are separate passes.
        h.event(egui::Event::Key {
            key: arrow,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        assert!(
            h.get_by_label("Choice").is_focused(),
            "pointer={pointer}, arrow={arrow:?}"
        );
        assert!(
            list_visible_on_open,
            "the opening frame already shows the list"
        );
        h.event(egui::Event::Key {
            key: arrow,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.step();
        assert!(h.get_by_label("Choice").is_focused());
        assert_eq!(result.get(), None);
        if arrow == Key::Escape {
            assert!(h.query_by_label("Gamma").is_none());
            assert_eq!(h.get_by_label("Choice").value().as_deref(), Some("Alpha"));
            press(&mut h, Key::Escape);
            assert_eq!(result.get(), Some(paintrs::dialogs::DialogResult::Cancel));
            continue;
        }
        assert!(h.query_by_label("Gamma").is_some());
        press(&mut h, Key::Enter);
        assert_eq!(result.get(), None);
        assert_eq!(h.get_by_label("Choice").value().as_deref(), Some(want));
        press(&mut h, Key::Escape);
        assert_eq!(result.get(), Some(paintrs::dialogs::DialogResult::Cancel));
    }
}

#[test]
fn focused_select_dismissal_clears_filter_and_preserves_tab_modifiers_and_pointer_pick() {
    let (mut h, result) = select_probe();
    h.get_by_label("Choice").focus();
    h.run_steps(2);
    for pick in [false, true] {
        press(&mut h, Key::Enter);
        press(&mut h, Key::End);
        press(&mut h, if pick { Key::Enter } else { Key::Escape });
        assert!(h.get_by_label("Choice").is_focused());
        assert_eq!(
            h.get_by_label("Choice").value().as_deref(),
            Some(if pick { "Gamma" } else { "Alpha" })
        );
        assert!(h.query_by_label("Alpha").is_none());
        // A closed field no longer owns the arrow, including after opening again.
        press(&mut h, Key::ArrowUp);
        assert!(!h.get_by_label("Choice").is_focused());
        h.get_by_label("Choice").focus();
        h.run_steps(2);
    }
    press(&mut h, Key::Enter);
    for modifiers in [Modifiers::CTRL, Modifiers::ALT, Modifiers::MAC_CMD] {
        h.key_press_modifiers(modifiers, Key::Escape);
        h.run_steps(2);
        assert!(h.query_by_label("Beta").is_some());
        assert_eq!(result.get(), None);
    }
    press(&mut h, Key::Escape);
    h.get_by_label("Choice").click();
    h.run_steps(2);
    h.get_by_label("Beta").click();
    h.run_steps(3);
    assert_eq!(h.get_by_label("Choice").value().as_deref(), Some("Beta"));
    assert_eq!(result.get(), None);
    assert!(h.query_by_label("Alpha").is_none());
    let pixels = select_pixels(&h);
    let tokens = h.state().app.effect_tokens.clone();
    let history = undo_len(&h);
    h.get_by_label("Choice").click();
    h.run();
    assert!(h.query_by_label("Alpha").is_some());
    // The outside press over OK dismisses only the popup, including its release frame.
    h.get_by_label("OK").click();
    h.run_steps(3);
    assert!(h.query_by_label("Alpha").is_none());
    assert_eq!(result.get(), None);
    assert_eq!(undo_len(&h), history);
    assert_eq!(h.state().app.effect_tokens, tokens);
    assert_eq!(select_pixels(&h), pixels);
    h.get_by_label("Cancel").click();
    h.run_steps(2);
    assert_eq!(result.get(), Some(paintrs::dialogs::DialogResult::Cancel));
}

#[test]
fn focused_select_tab_and_shift_tab_leave_the_open_list() {
    for modifiers in [Modifiers::NONE, Modifiers::SHIFT] {
        let (mut h, result) = select_probe();
        h.get_by_label("Choice").focus();
        h.run_steps(2);
        press(&mut h, Key::Enter);
        h.key_press_modifiers(modifiers, Key::Tab);
        h.run_steps(3);
        assert!(!h.get_by_label("Choice").is_focused(), "{modifiers:?}");
        assert!(h.query_by_label("Alpha").is_none());
        assert_eq!(result.get(), None);
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        assert_eq!(result.get(), Some(paintrs::dialogs::DialogResult::Cancel));
    }
}

// BB15: native numeric increments remain distinct from fractional replacement-slider metadata.
#[test]
fn bb15_strength_keys_and_numeric_default() {
    for effect in ["bbBevelObject", "bbBevelSelection"] {
        for (numeric, input, want, shown) in [
            (false, Key::ArrowRight, 0.41, "0.41"),
            (false, Key::PageUp, 0.5, "0.50"),
            (true, Key::ArrowUp, 1.4, "1.40"),
        ] {
            let mut h = bb12_harness();
            let def = pdn_core::effect::find(effect).unwrap();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert("strength".into(), serde_json::json!(0.4));
            open_def(&mut h, def, values);
            h.get_by_role_and_label(Role::Slider, "strength").focus();
            h.run_steps(2);
            if numeric {
                press(&mut h, Key::Tab);
            }
            press(&mut h, input);
            if !numeric {
                press(&mut h, Key::Tab);
            }
            assert_eq!(focused_node(&h), ("strength".into(), shown.into()));
            assert!((finish(&mut h, effect)["strength"].as_f64().unwrap() - want).abs() < 1e-9);
        }
    }
}
// R107: 0..360 wraps on strict numeric overflow, discards fractional overshoot and clamps typed text.
#[test]
fn bb15_angle_numeric_wrap_and_typed_clamp() {
    for effect in ["bbBevelObject", "bbBevelSelection"] {
        for (initial, typed, key, want) in [
            (359.0, None, Key::ArrowUp, 360.0),
            (360.0, None, Key::ArrowUp, 0.0),
            (359.75, None, Key::ArrowUp, 0.0),
            (0.0, None, Key::ArrowDown, 360.0),
            (45.0, Some("361"), Key::ArrowUp, 0.0),
            (45.0, Some("361"), Key::Tab, 360.0),
            (45.0, Some("-1"), Key::Tab, 0.0),
        ] {
            let mut h = bb12_harness();
            let def = pdn_core::effect::find(effect).unwrap();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert("angle".into(), serde_json::json!(initial));
            open_def(&mut h, def, values);
            assert_eq!(focused_node(&h), ("angle".into(), format!("{initial:.3}")));
            if let Some(text) = typed {
                h.event(egui::Event::Text(text.into()));
                h.run();
            }
            press(&mut h, key);
            assert_eq!(finish(&mut h, effect)["angle"], serde_json::json!(want));
        }
    }
}
#[test]
fn bb15_angle_spin_and_transient_tab() {
    for effect in ["bbBevelObject", "bbBevelSelection"] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find(effect).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("angle".into(), serde_json::json!(359.75));
        open_def(&mut h, def, values);
        let rect = h.get_by_role_and_label(Role::Slider, "angle").rect();
        let pos = egui::pos2(rect.max.x + 8.0 + 72.0 - 7.0, rect.center().y - 4.0);
        h.event(egui::Event::PointerMoved(pos));
        h.run_steps(1);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
        h.run_steps(3);
        h.get_by_role_and_label(Role::Tab, " Colors ").click();
        h.run_steps(2);
        let token = finish(&mut h, effect);
        assert_eq!(token["angle"], serde_json::json!(0.0));
        assert!(!token.contains_key("tab"));
        open_effect(&mut h, effect);
        assert!(selected(&h, " Shadow "));
        press(&mut h, Key::Escape);
    }
}

// BB16: decimal display does not turn native numeric/slider key changes into hundredths.
#[test]
fn bb16_decimal_keys_and_numeric_display() {
    for key in ["radius", "strength"] {
        for (numeric, input, want, shown) in [
            (false, Key::ArrowRight, 1.0, "1.00"),
            (false, Key::ArrowLeft, 0.0, "0.00"),
            (false, Key::PageUp, 1.0, "1.00"),
            (false, Key::PageDown, 0.0, "0.00"),
            (true, Key::ArrowUp, 1.0, "1.00"),
            (true, Key::ArrowDown, 0.0, "0.00"),
        ] {
            let mut h = bb12_harness();
            let def = pdn_core::effect::find("bbVignettePlus").unwrap();
            let values = pdn_core::effect::defaults(&def);
            open_def(&mut h, def, values);
            h.get_by_role_and_label(Role::Slider, key).focus();
            h.run_steps(2);
            press(&mut h, Key::Tab);
            assert_eq!(focused_node(&h), (key.into(), "0.50".into()));
            if !numeric {
                h.get_by_role_and_label(Role::Slider, key).focus();
                h.run_steps(2);
            }
            press(&mut h, input);
            if !numeric {
                press(&mut h, Key::Tab);
            }
            assert_eq!(focused_node(&h), (key.into(), shown.into()));
            assert_eq!(
                finish(&mut h, "bbVignettePlus")[key],
                serde_json::json!(want)
            );
        }
    }
}
// BB16: reset uses the document-sized creation default rather than the selection or static ten.
#[test]
fn bb16_dream_dynamic_reset_and_keys() {
    let mut h = bb12_harness();
    h.state_mut().app = pdn_core::testing::fresh_doc(1920, 4, Some(4));
    let def = pdn_core::effect::find("bbDream").unwrap();
    let session =
        pdn_core::effect::session::EffectSession::begin(&mut h.state_mut().app, def.clone())
            .unwrap();
    let mut values = session.defaults();
    assert_eq!(values["radius"], serde_json::json!(24));
    values.insert("radius".into(), serde_json::json!(40));
    values.insert("vignette".into(), serde_json::json!(false));
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Slider, "radius").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::PageUp);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("radius".into(), "46".into()));
    press(&mut h, Key::Tab);
    press(&mut h, Key::Enter);
    h.get_by_role_and_label(Role::Slider, "radius").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("radius".into(), "24".into()));
    assert_eq!(finish(&mut h, "bbDream")["radius"], serde_json::json!(24.0));
}

// BB17: actual integer slider/numeric key changes, reset defaults, and unlabelled rows remain editable.
#[test]
fn bb17_integer_slider_keys_and_reset() {
    for (key, default) in [
        ("shadowRecovery", 100),
        ("brightness", 0),
        ("contrast", 0),
        ("temperature", 0),
        ("tint", 0),
        ("saturation", 0),
        ("dynamicRange", 0),
    ] {
        let mut h = bb12_harness();
        open_effect(&mut h, "bbCombinedPhotoEffects");
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        for input in [Key::ArrowRight, Key::PageUp] {
            press(&mut h, input);
        }
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), (key.into(), (default + 6).to_string()));
        press(&mut h, Key::ArrowUp);
        assert_eq!(focused_node(&h), (key.into(), (default + 7).to_string()));
        press(&mut h, Key::ArrowDown);
        assert_eq!(focused_node(&h), (key.into(), (default + 6).to_string()));
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        press(&mut h, Key::ArrowLeft);
        press(&mut h, Key::PageDown);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), (key.into(), default.to_string()));
        press(&mut h, Key::ArrowUp);
        press(&mut h, Key::Tab);
        press(&mut h, Key::Enter);
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), (key.into(), default.to_string()));
        assert_eq!(
            finish(&mut h, "bbCombinedPhotoEffects")[key],
            serde_json::json!(default as f64)
        );
    }
}

// BB17: Preview disables all seven sliders while retaining values, and keyboard traversal skips them.
#[test]
fn bb17_preview_disables_and_retains_controls() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbCombinedPhotoEffects").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    let settings = [
        ("shadowRecovery", 150),
        ("brightness", 10),
        ("contrast", 20),
        ("temperature", 30),
        ("tint", 40),
        ("saturation", 50),
        ("dynamicRange", 60),
    ];
    for (key, value) in settings {
        values.insert(key.into(), serde_json::json!(value));
    }
    open_def(&mut h, def, values);
    let checkbox = h.get_by_role_and_label(Role::CheckBox, "Preview original image");
    checkbox.focus();
    h.run_steps(2);
    press(&mut h, Key::Space);
    for (key, _) in settings {
        assert!(
            h.get_by_role_and_label(Role::Slider, key)
                .accesskit_node()
                .is_disabled()
        );
    }
    press(&mut h, Key::Tab);
    assert_eq!(
        h.get_by(|n| n.is_focused())
            .accesskit_node()
            .label()
            .as_deref(),
        Some("Preview")
    );
    let token = finish(&mut h, "bbCombinedPhotoEffects");
    assert_eq!(token["previewOriginal"], true);
    for (key, value) in settings {
        assert_eq!(token[key], serde_json::json!(value));
    }
    open_effect(&mut h, "bbCombinedPhotoEffects");
    h.get_by_role_and_label(Role::CheckBox, "Preview original image")
        .focus();
    h.run_steps(2);
    press(&mut h, Key::Space);
    for (key, _) in settings {
        assert!(
            !h.get_by_role_and_label(Role::Slider, key)
                .accesskit_node()
                .is_disabled()
        );
    }
    assert_eq!(
        finish(&mut h, "bbCombinedPhotoEffects")["previewOriginal"],
        false
    );
}

// BB19/R107: registered angle numeric keys, pending text, endpoint replacement and R14 slider keys.
#[test]
fn bb19_angle_numeric_and_slider_keys() {
    for (initial, typed, input, numeric, want) in [
        (45.0, None, Key::ArrowUp, true, 46.0),
        (45.0, None, Key::ArrowDown, true, 44.0),
        (179.0, None, Key::ArrowUp, true, 180.0),
        (180.0, None, Key::ArrowUp, true, -180.0),
        (-180.0, None, Key::ArrowDown, true, 180.0),
        (179.75, None, Key::ArrowUp, true, -180.0),
        (-179.75, None, Key::ArrowDown, true, 180.0),
        (45.0, Some("181"), Key::ArrowUp, true, -180.0),
        (45.0, Some("-181"), Key::ArrowDown, true, 180.0),
        (45.0, Some("181"), Key::Tab, true, 180.0),
        (45.0, Some("-181"), Key::Tab, true, -180.0),
        (45.25, None, Key::ArrowRight, false, 46.25),
        (45.25, None, Key::PageDown, false, 40.25),
        (179.75, None, Key::ArrowRight, false, 180.0),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find("bbEdgeDetectClassic").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("angle".into(), serde_json::json!(initial));
        open_def(&mut h, def, values);
        assert_eq!(focused_node(&h).1, format!("{initial:.3}"));
        if !numeric {
            h.get_by_role_and_label(Role::Slider, "angle").focus();
            h.run_steps(2);
        }
        if let Some(text) = typed {
            h.key_press_modifiers(Modifiers::CTRL, Key::A);
            h.event(egui::Event::Text(text.into()));
            h.run();
        }
        press(&mut h, input);
        assert_eq!(
            finish(&mut h, "bbEdgeDetectClassic")["angle"],
            serde_json::json!(want)
        );
    }
}

// BB19: horizon companions preserve p3, small/large changes and independent numeric .01 steps.
#[test]
fn bb19_horizon_companions_and_reset() {
    for (axis, initial, key, numeric, want) in [
        ("X", 0.125, Key::ArrowRight, false, 0.175),
        ("Y", -0.25, Key::PageDown, false, -0.5),
        ("Y", -0.255, Key::ArrowUp, true, -0.245),
        ("X", 0.99, Key::PageUp, false, 1.0),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find("bbSharpenLandscape").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("horizon".into(), serde_json::json!([initial, initial]));
        open_def(&mut h, def, values);
        assert_eq!(focused_node(&h).0, "horizon.X");
        let label = format!("horizon.{axis}");
        h.get_by_role_and_label(Role::Slider, &label).focus();
        h.run_steps(2);
        if numeric {
            press(&mut h, Key::Tab);
        }
        press(&mut h, key);
        if !numeric {
            press(&mut h, Key::Tab);
        }
        assert_eq!(focused_node(&h).1, format!("{want:.3}"));
        let token = finish(&mut h, "bbSharpenLandscape");
        assert_eq!(
            token["horizon"][usize::from(axis == "Y")],
            serde_json::json!(want)
        );
    }
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbSharpenLandscape").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("horizon".into(), serde_json::json!([0.5, 0.5]));
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Slider, "horizon.Y").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Enter);
    assert_eq!(
        finish(&mut h, "bbSharpenLandscape")["horizon"],
        serde_json::json!([0.5, -0.25])
    );
}

#[test]
fn bb19_angle_spin_matches_numeric_key() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbEdgeDetectClassic").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("angle".into(), serde_json::json!(179.75));
    open_def(&mut h, def, values);
    let rect = h.get_by_role_and_label(Role::Slider, "angle").rect();
    let pos = egui::pos2(rect.max.x + 8.0 + 72.0 - 7.0, rect.center().y - 4.0);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    assert_eq!(
        finish(&mut h, "bbEdgeDetectClassic")["angle"],
        serde_json::json!(-180.0)
    );
}

// BB20: scalar p0 keys, native numeric increment, default reset and final-only commit.
#[test]
fn bb20_integer_keys_reset_and_cancel_live_while_applying() {
    for id in ["bbSeamCarvingVertical", "bbSeamCarvingHorizontal"] {
        let mut h = bb12_harness();
        open_effect(&mut h, id);
        h.get_by_role_and_label(Role::CheckBox, "Preview").click();
        h.run_steps(3);
        h.get_by_role_and_label(Role::Slider, "amount").focus();
        h.run_steps(2);
        press(&mut h, Key::ArrowRight);
        press(&mut h, Key::PageUp);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), ("amount".into(), "7".into()));
        press(&mut h, Key::ArrowUp);
        assert_eq!(focused_node(&h), ("amount".into(), "8".into()));
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h).0, "Reset");
        press(&mut h, Key::Enter);
        h.get_by_role_and_label(Role::Slider, "amount").focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), ("amount".into(), "1".into()));
        let before = undo_len(&h);
        h.get_by_label("OK").click();
        for _ in 0..200 {
            h.run_steps(1);
            if !h.state().shell.dialogs.is_open() {
                break;
            }
            std::thread::yield_now();
        }
        assert!(!h.state().shell.dialogs.is_open());
        assert_eq!(undo_len(&h), before + 1);
        assert_eq!(h.state().app.effect_tokens[id]["amount"], 1.0);
    }
}

// BB20: final-only warnings happen after committing, with actual Warning/OK and Escape behavior.
#[test]
fn bb20_final_warning_and_repeat_cancel_boundary() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbSeamCarvingVertical").unwrap();
    let values = pdn_core::effect::with(&def, &serde_json::json!({"amount":40}));
    let session =
        pdn_core::effect::session::EffectSession::begin(&mut h.state_mut().app, def).unwrap();
    let before = undo_len(&h);
    h.state_mut().shell.dialogs.push(Box::new(
        paintrs::dialogs::effect_config::EffectConfigDialog::final_only(session, values),
    ));
    for _ in 0..300 {
        h.run_steps(1);
        if undo_len(&h) > before {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(undo_len(&h), before + 1);
    h.run_steps(2);
    assert!(h.get_by_label("OK").accesskit_node().label().is_some());
    press(&mut h, Key::Escape);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), before + 1);
    let repeated_before = undo_len(&h);
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    for _ in 0..200 {
        h.run_steps(1);
        if undo_len(&h) > repeated_before {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(undo_len(&h), repeated_before + 1);
    h.run_steps(2);
    assert!(h.get_by_label("OK").accesskit_node().label().is_some());
    assert!(h.query_by_role_and_label(Role::Slider, "amount").is_none());
    press(&mut h, Key::Escape);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), repeated_before + 1);
    // A long Repeat starts its final-only owner through the real keyboard action route.
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(512, 512, 96.0, true);
    h.run_steps(2);
    let before = undo_len(&h);
    let tokens = h.state().app.effect_tokens.clone();
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    h.run_steps(1);
    assert!(h.state().shell.dialogs.is_open());
    assert!(h.query_by_role_and_label(Role::Slider, "amount").is_none());
    h.key_press(Key::Escape);
    h.run_steps(2);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(undo_len(&h), before);
    assert_eq!(h.state().app.effect_tokens, tokens);
}

// BB21: real palette changes disable fields without losing values; standard p0 keys remain live.
#[test]
fn bb21_palette_enable_preservation_and_integer_keys() {
    let mut h = bb12_harness();
    let id = "bbFloydSteinbergDithering";
    let def = pdn_core::effect::find(id).unwrap();
    let mut v = pdn_core::effect::defaults(&def);
    v.insert("customColor".into(), serde_json::json!("#123456"));
    v.insert("count".into(), serde_json::json!(17));
    let result = open_recorded_def(&mut h, def, v);
    h.get_by_role_and_label(Role::CheckBox, "Preview").click();
    h.run_steps(2);
    assert!(
        h.get_by_label("Custom Color")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        h.get_by_role_and_label(Role::Slider, "count")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_label("palette").focus();
    h.run_steps(2);
    press(&mut h, Key::Enter);
    press(&mut h, Key::End);
    press(&mut h, Key::Enter);
    assert!(
        h.get_by_label("Custom Color")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        !h.get_by_role_and_label(Role::Slider, "count")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_role_and_label(Role::Slider, "count").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::PageUp);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("count".into(), "23".into()));
    press(&mut h, Key::ArrowUp);
    assert_eq!(focused_node(&h), ("count".into(), "24".into()));
    let pixels = select_pixels(&h);
    let tokens = h.state().app.effect_tokens.clone();
    let history = undo_len(&h);
    h.get_by_label("palette").focus();
    h.run_steps(2);
    press(&mut h, Key::Enter);
    press(&mut h, Key::End);
    press(&mut h, Key::ArrowUp);
    let arrow_focus = h.get_by_label("palette").is_focused();
    press(&mut h, Key::Enter);
    assert_eq!(result.get(), None, "list pick must not close the parent");
    assert!(arrow_focus, "palette retains focus after Up");
    assert_eq!(undo_len(&h), history);
    assert_eq!(h.state().app.effect_tokens, tokens);
    assert_eq!(select_pixels(&h), pixels);
    assert!(
        !h.get_by_label("Custom Color")
            .accesskit_node()
            .is_disabled()
    );
    assert!(
        h.get_by_role_and_label(Role::Slider, "count")
            .accesskit_node()
            .is_disabled()
    );
    h.get_by_role_and_label(Role::Slider, "brightness").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::PageUp);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("brightness".into(), "6".into()));
    press(&mut h, Key::ArrowDown);
    assert_eq!(focused_node(&h), ("brightness".into(), "5".into()));
    h.get_by_role_and_label(Role::Slider, "contrast").focus();
    h.run_steps(2);
    press(&mut h, Key::PageDown);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("contrast".into(), "-5".into()));
    let token = finish(&mut h, id);
    assert_eq!(result.get(), Some(paintrs::dialogs::DialogResult::Ok));
    assert_eq!(undo_len(&h), history + 1, "explicit OK commits once");
    assert_eq!(token["palette"], "bwPlusOne");
    assert_eq!(token["count"], 24.0);
    assert_eq!(token["customColor"], "#123456");
    assert_eq!(token["brightness"], 5.0);
    assert_eq!(token["contrast"], -5.0);
}

// BB22: real fractional/integer slider keys, independent numeric increments and display precision.
#[test]
fn bb22_slider_and_numeric_values() {
    for (id, key, start, numeric, input, want, shown) in [
        (
            "bbFlames",
            "roughness",
            0.5,
            false,
            Key::ArrowRight,
            0.55,
            "0.550",
        ),
        (
            "bbFlames",
            "roughness",
            0.5,
            false,
            Key::PageUp,
            0.75,
            "0.750",
        ),
        (
            "bbFlames",
            "roughness",
            0.5,
            true,
            Key::ArrowUp,
            0.51,
            "0.510",
        ),
        ("bbFlames", "scale", 250.0, false, Key::PageUp, 255.0, "255"),
        ("bbFlames", "seed", 0.0, true, Key::ArrowUp, 1.0, "1"),
        (
            "bbPolygonStars",
            "depth",
            0.4,
            false,
            Key::ArrowRight,
            0.45,
            "0.45",
        ),
        (
            "bbPolygonStars",
            "depth",
            0.4,
            false,
            Key::PageUp,
            0.65,
            "0.65",
        ),
        (
            "bbPolygonStars",
            "depth",
            0.4,
            true,
            Key::ArrowUp,
            0.41,
            "0.41",
        ),
        (
            "bbPolygonStars",
            "size",
            0.9,
            false,
            Key::PageDown,
            0.65,
            "0.65",
        ),
        (
            "bbPolygonStars",
            "size",
            0.9,
            true,
            Key::ArrowDown,
            0.89,
            "0.89",
        ),
        (
            "bbPolygonStars",
            "angle",
            90.0,
            false,
            Key::PageUp,
            95.0,
            "95.00",
        ),
        (
            "bbPolygonStars",
            "vertices",
            5.0,
            false,
            Key::ArrowRight,
            6.0,
            "6",
        ),
        (
            "bbPolygonStars",
            "lineWidth",
            2.0,
            true,
            Key::ArrowUp,
            3.0,
            "3",
        ),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find(id).unwrap();
        let mut v = pdn_core::effect::defaults(&def);
        v.insert(key.into(), serde_json::json!(start));
        open_def(&mut h, def, v);
        h.get_by_role_and_label(Role::Slider, key).focus();
        h.run_steps(2);
        if numeric {
            press(&mut h, Key::Tab);
        }
        press(&mut h, input);
        if !numeric {
            press(&mut h, Key::Tab);
        }
        assert_eq!(focused_node(&h), (key.into(), shown.into()));
        assert!((finish(&mut h, id)[key].as_f64().unwrap() - want).abs() < 1e-9);
    }
}

// BB22/R107: actual Polygon numeric entry clamps, strict crossings replace endpoints.
#[test]
fn bb22_angle_numeric_and_spin() {
    for (start, typed, key, want) in [
        (90.0, None, Key::ArrowUp, 91.0),
        (90.0, None, Key::ArrowDown, 89.0),
        (179.0, None, Key::ArrowUp, 180.0),
        (180.0, None, Key::ArrowUp, -180.0),
        (-180.0, None, Key::ArrowDown, 180.0),
        (179.75, None, Key::ArrowUp, -180.0),
        (-179.75, None, Key::ArrowDown, 180.0),
        (90.0, Some("181"), Key::ArrowUp, -180.0),
        (90.0, Some("-181"), Key::ArrowDown, 180.0),
        (90.0, Some("181"), Key::Tab, 180.0),
        (90.0, Some("-181"), Key::Tab, -180.0),
    ] {
        let mut h = bb12_harness();
        let def = pdn_core::effect::find("bbPolygonStars").unwrap();
        let mut v = pdn_core::effect::defaults(&def);
        v.insert("angle".into(), serde_json::json!(start));
        open_def(&mut h, def, v);
        h.get_by_role_and_label(Role::Slider, "angle").focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        assert_eq!(focused_node(&h), ("angle".into(), format!("{start:.2}")));
        if let Some(text) = typed {
            h.event(egui::Event::Text(text.into()));
            h.run();
        }
        press(&mut h, key);
        assert_eq!(
            finish(&mut h, "bbPolygonStars")["angle"],
            serde_json::json!(want)
        );
    }
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbPolygonStars").unwrap();
    let mut v = pdn_core::effect::defaults(&def);
    v.insert("angle".into(), serde_json::json!(179.75));
    open_def(&mut h, def, v);
    let rect = h.get_by_role_and_label(Role::Slider, "angle").rect();
    let pos = egui::pos2(rect.max.x + 8.0 + 72.0 - 7.0, rect.center().y - 4.0);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    assert_eq!(finish(&mut h, "bbPolygonStars")["angle"], -180.0);
}

// BB22: every style option and AA toggle persist through the actual registered dialog.
#[test]
fn bb22_styles_checkbox_and_reset() {
    for (label, value) in [
        ("Normal Flames", "normal"),
        ("Transparent Flames", "transparent"),
        ("Fog/Smoke", "fog"),
        ("Transparent Smoke/Steam", "smoke"),
    ] {
        let mut h = bb12_harness();
        open_effect(&mut h, "bbFlames");
        h.get_by_label("style").click();
        h.run_steps(2);
        h.get_by_label(label).click();
        h.run_steps(3);
        assert_eq!(finish(&mut h, "bbFlames")["style"], value);
    }
    let mut h = bb12_harness();
    open_effect(&mut h, "bbPolygonStars");
    h.get_by_role_and_label(Role::CheckBox, "Anti-Aliased")
        .click();
    h.run_steps(2);
    h.get_by_role_and_label(Role::Slider, "vertices").focus();
    h.run_steps(2);
    press(&mut h, Key::ArrowRight);
    press(&mut h, Key::Tab);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h).0, "Reset");
    press(&mut h, Key::Enter);
    let token = finish(&mut h, "bbPolygonStars");
    assert_eq!(token["antiAlias"], false);
    assert_eq!(token["vertices"], 5.0);
}

// BB22/BB25: the opaque, unresettable row opens from primary RGB and offers no alpha editor.
#[test]
fn bb22_color_row_opens_an_opaque_picker() {
    let mut h = bb12_harness();
    h.state_mut().app.primary_color = pdn_core::util::color::Color::new(0x12, 0x34, 0x56, 0x20);
    open_effect(&mut h, "bbPolygonStars");
    h.get_by_label("color").click();
    h.run_steps(3);
    assert_eq!(focused_node(&h), ("Hex".into(), "123456".into()));
    assert!(h.query_by_label("Alpha").is_none());
    bb25_type(&mut h, "abcdef");
    press(&mut h, Key::Enter);
    assert_eq!(finish(&mut h, "bbPolygonStars")["color"], "#abcdef");
}

// render-flames.md Expected results 6: actual preview/OK/Repeat keep the opening UTC snapshot.
#[test]
fn bb22_flames_preview_commit_and_repeat_dates() {
    use pdn_core::effect::rng::CivilDate;
    let mut h = bb12_harness();
    let src = pdn_core::canvas::ImageData {
        width: 32,
        height: 32,
        data: (0..32)
            .flat_map(|y| {
                (0..32).flat_map(move |_| {
                    let g = (y * 255 / 31) as u8;
                    [g, g, g, 255]
                })
            })
            .collect(),
    };
    h.state_mut()
        .app
        .active_ws_mut()
        .unwrap()
        .document
        .layers
        .get_at_mut(0)
        .unwrap()
        .surface
        .put_image_data(&src, 0, 0);
    h.state_mut().app.date_override = Some(CivilDate::new(2026, 1, 1));
    let def = pdn_core::effect::find("bbFlames").unwrap();
    let overrides = serde_json::json!({"style":"fog","scale":30,"seed":3});
    let values = pdn_core::effect::with(&def, &overrides);
    let mut env = pdn_core::effect::EffectEnv::whole(32, 32);
    env.today = CivilDate::new(2026, 1, 1);
    let want = pdn_core::effect::apply(&src, &def, &values, &env).data;
    open_def(&mut h, def, values);
    h.state_mut().app.date_override = Some(CivilDate::new(2026, 1, 2));
    for _ in 0..200 {
        h.run_steps(1);
        if pdn_core::testing::layer_bytes(&h.state().app, None) == want {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(pdn_core::testing::layer_bytes(&h.state().app, None), want);
    finish(&mut h, "bbFlames");
    assert_eq!(pdn_core::testing::layer_bytes(&h.state().app, None), want);
    for date in [
        CivilDate::new(2026, 1, 1),
        CivilDate::new(2026, 9, 14),
        CivilDate::new(2026, 1, 2),
    ] {
        h.state_mut().app.undo();
        h.run_steps(2);
        assert_eq!(
            pdn_core::testing::layer_bytes(&h.state().app, None),
            src.data
        );
        h.state_mut().app.date_override = Some(date);
        let before = undo_len(&h);
        h.key_press_modifiers(Modifiers::CTRL, Key::F);
        h.run_steps(4);
        assert!(!h.state().shell.dialogs.is_open());
        assert_eq!(undo_len(&h), before + 1);
        env.today = date;
        let def = pdn_core::effect::find("bbFlames").unwrap();
        let values = pdn_core::effect::with(&def, &overrides);
        let expected = pdn_core::effect::apply(&src, &def, &values, &env).data;
        let got = pdn_core::testing::layer_bytes(&h.state().app, None);
        assert_eq!(got, expected);
        if date.day_of_year == 1 || date.day_of_year == 257 {
            assert_eq!(got, want);
        } else {
            assert_ne!(got, want);
        }
    }
}

fn bb25_harness() -> Harness<'static, paintrs::PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .wgpu()
        .build_eframe(new_web_chrome_app);
    h.run_steps(2);
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(32, 32, 96.0, true);
    h.run_steps(2);
    h
}

fn bb25_double_click(h: &mut Harness<'_, paintrs::PaintApp>, at: egui::Pos2) {
    // Separate gestures: egui's triple-click counter includes a preceding different control.
    h.run_steps(20);
    h.event(egui::Event::PointerMoved(at));
    h.run_steps(1);
    for _ in 0..2 {
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
        }
    }
}

// R153: the first key after opening belongs to the picker, with no settling input.
#[test]
fn bb25_popup_first_escape_enter_tab_and_space_stay_local() {
    let mut h = bb25_harness();
    open_probe(&mut h);
    let before = undo_len(&h);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Hex");
    h.key_press(Key::Escape);
    h.run_steps(1);
    assert!(h.state().shell.dialogs.is_open());
    assert!(h.query_by_label("Hex").is_none());
    assert_eq!(focused_node(&h).0, "Tint");
    h.key_press(Key::Space);
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Hex");
    h.key_press(Key::Tab);
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Hue");
    h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Hex");
    h.key_press_modifiers(Modifiers::CTRL, Key::A);
    h.event(egui::Event::Text("12345678".into()));
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(1);
    assert!(h.state().shell.dialogs.is_open());
    assert!(h.query_by_label("Hex").is_none());
    assert_eq!(undo_len(&h), before);
    assert_eq!(finish(&mut h, "bbProbe")["tint"], "#12345678");
    assert_eq!(undo_len(&h), before + 1);
}

#[test]
fn bb25_popup_outside_press_consumes_parent_cancel_help_and_close() {
    for target in ["Cancel", "Help", "Close"] {
        let mut h = bb25_harness();
        open_probe(&mut h);
        let result_before = undo_len(&h);
        let at = if target == "Close" {
            h.get_by_label("Help").rect().center() + egui::vec2(paintrs::theme::DIALOG_CLOSE.0, 0.0)
        } else {
            h.get_by_label(target).rect().center()
        };
        h.get_by_label("Tint").click();
        h.run_steps(1);
        h.event(egui::Event::PointerMoved(at));
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.run_steps(1);
            assert!(h.state().shell.dialogs.is_open(), "{target}: {pressed}");
        }
        assert!(h.query_by_label("Hex").is_none());
        assert_eq!(undo_len(&h), result_before);
        h.key_press(Key::Escape);
        h.run_steps(1);
        assert!(
            !h.state().shell.dialogs.is_open(),
            "no Help child was opened"
        );
    }
}

fn bb25_type(h: &mut Harness<'_, paintrs::PaintApp>, text: &str) {
    h.key_press_modifiers(Modifiers::CTRL, Key::A);
    h.event(egui::Event::Text(text.into()));
    h.run_steps(1);
}

#[test]
fn bb25_all_four_levels_endpoints_ignore_channel_mask_and_preserve_raw() {
    for key in ["inputLow", "inputHigh", "outputLow", "outputHigh"] {
        let mut h = bb25_harness();
        let def = pdn_core::effect::find("levelsEffect").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        let raw = serde_json::json!([0.5, 12.75, 253.125]);
        values.insert(key.into(), raw.clone());
        open_def(&mut h, def, values);
        for label in ["R", "G", "B"] {
            h.get_by_label(label).click();
            h.run_steps(1);
        }
        // Two actual pointer clicks on the swatch, without a programmatic open helper.
        let at = h.get_by_label(&format!("{key} color")).rect().center();
        bb25_double_click(&mut h, at);
        assert_eq!(focused_node(&h), ("Hex".into(), "010DFD".into()));
        h.key_press(Key::Enter);
        h.run_steps(1);
        // Unchanged keep retains the fractional raw triple.
        bb25_double_click(&mut h, at);
        bb25_type(&mut h, "112233");
        h.key_press(Key::Escape);
        h.run_steps(1);
        assert_eq!(finish(&mut h, "levelsEffect")[key], raw);
        open_effect(&mut h, "levelsEffect");
        for label in ["R", "G", "B"] {
            h.get_by_label(label).click();
            h.run_steps(1);
        }
        let at = h.get_by_label(&format!("{key} color")).rect().center();
        bb25_double_click(&mut h, at);
        bb25_type(&mut h, "112233");
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(
            finish(&mut h, "levelsEffect")[key],
            serde_json::json!([17, 34, 51])
        );
    }
}

#[test]
fn bb25_levels_middle_stays_passive() {
    let mut h = bb25_harness();
    open_effect(&mut h, "levelsEffect");
    let high = h.get_by_label("outputHigh color").rect();
    let low = h.get_by_label("outputLow color").rect();
    let at = egui::pos2(high.center().x, (high.center().y + low.center().y) / 2.0);
    bb25_double_click(&mut h, at);
    assert!(h.query_by_label("Hex").is_none());
    assert_eq!(
        finish(&mut h, "levelsEffect")["gamma"],
        serde_json::json!([1.0, 1.0, 1.0])
    );
}

#[test]
fn bb25_alpha_numeric_keeps_exact_rgb_and_unchanged_raw() {
    for edit in [false, true] {
        let mut h = bb25_harness();
        let def = bb5_probe::def();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("tint".into(), serde_json::json!("#fF800011"));
        open_def(&mut h, def, values);
        h.get_by_label("Tint").click();
        h.run_steps(1);
        assert_eq!(focused_node(&h), ("Hex".into(), "FF800011".into()));
        if edit {
            for _ in 0..4 {
                h.key_press(Key::Tab);
                h.run_steps(1);
            }
            assert_eq!(focused_node(&h).0, "Alpha");
            bb25_type(&mut h, "128");
        }
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(
            finish(&mut h, "bbProbe")["tint"],
            if edit { "#ff800080" } else { "#fF800011" }
        );
    }
}

#[test]
fn bb25_generic_row_and_secondary_shortcut() {
    let mut h = bb25_harness();
    h.state_mut().app.secondary_color = pdn_core::util::color::Color::new(18, 52, 86, 120);
    open_effect(&mut h, "cloudsEffect");
    h.get_by_label("Color 1").click();
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Hex");
    bb25_type(&mut h, "abcdef");
    h.key_press(Key::Enter);
    h.run_steps(1);
    let at = h.get_by_label("Color 2").rect().center();
    tap(&mut h, at.x, at.y, egui::PointerButton::Secondary);
    assert!(h.query_by_label("Hex").is_none());
    let token = finish(&mut h, "cloudsEffect");
    assert_eq!(token["color1"], "#abcdef");
    assert_eq!(token["color2"], "#123456");
}

#[test]
fn bb25_popup_modified_default_keys_and_full_tab_cycle() {
    let mut h = bb25_harness();
    open_probe(&mut h);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    for modifier in [
        Modifiers::CTRL,
        Modifiers::ALT,
        Modifiers::SHIFT,
        Modifiers::COMMAND,
    ] {
        for key in [Key::Enter, Key::Escape] {
            h.key_press_modifiers(modifier, key);
            h.run_steps(1);
            assert!(h.query_by_label("Hex").is_some());
        }
    }
    for label in ["Hue", "Saturation", "Value", "Alpha", "Hex", "Hue"] {
        h.key_press(Key::Tab);
        h.run_steps(1);
        assert_eq!(focused_node(&h).0, label);
    }
    h.check_accessibility();
    h.key_press(Key::Escape);
    h.run_steps(1);
    assert_eq!(focused_node(&h).0, "Tint");
    // Popup lock is gone; normal parent traversal can leave the opener immediately.
    h.key_press(Key::Tab);
    h.run_steps(1);
    assert_ne!(focused_node(&h).0, "Tint");
    assert!(h.query_by_label("Hex").is_none());
}

// common.md §3: popup edits preview, but only parent OK owns history, tokens and Repeat.
#[test]
fn bb25_preview_cancel_commit_repeat_and_preview_off() {
    for preview in [false, true] {
        for cancel in [false, true] {
            let mut h = bb25_harness();
            let before = select_pixels(&h);
            let history = undo_len(&h);
            let dirty = h.state().app.active_ws().unwrap().dirty;
            open_effect(&mut h, "bbGridCheckerboard");
            if !preview {
                h.get_by_label("Preview").click();
                h.run_steps(1);
            }
            h.get_by_label("color").click();
            h.run_steps(1);
            bb25_type(&mut h, "ff800080");
            for _ in 0..100 {
                h.run_steps(1);
                if !preview || select_pixels(&h) != before {
                    break;
                }
                std::thread::yield_now();
            }
            assert_eq!(undo_len(&h), history);
            assert!(
                !h.state()
                    .app
                    .effect_tokens
                    .contains_key("bbGridCheckerboard")
            );
            if preview {
                assert_ne!(select_pixels(&h), before);
            } else {
                assert_eq!(select_pixels(&h), before);
            }
            h.key_press(Key::Enter);
            h.run_steps(1);
            assert!(h.state().shell.dialogs.is_open());
            if cancel {
                h.get_by_label("Cancel").click();
                h.run_steps(2);
                assert_eq!(select_pixels(&h), before);
                assert_eq!(undo_len(&h), history);
                assert_eq!(h.state().app.active_ws().unwrap().dirty, dirty);
                assert!(
                    !h.state()
                        .app
                        .effect_tokens
                        .contains_key("bbGridCheckerboard")
                );
            } else {
                let token = finish(&mut h, "bbGridCheckerboard");
                assert_eq!(token["color"], "#ff800080");
                assert_eq!(undo_len(&h), history + 1);
                assert_eq!(
                    h.state().app.last_effect_values.as_ref().unwrap()["color"],
                    "#ff800080"
                );
                assert_eq!(&select_pixels(&h)[..4], &[255, 128, 0, 128]);
                h.key_press_modifiers(Modifiers::CTRL, Key::F);
                h.run_steps(5);
                assert_eq!(undo_len(&h), history + 2);
                assert_eq!(&select_pixels(&h)[..4], &[255, 128, 0, 128]);
            }
        }
    }
}

// Fix I1: black can retain saturation that its RGB bytes cannot represent.
#[test]
fn bb25_fix1_alpha_hex_retains_black_saturation() {
    for (text, want) in [("00000022", "#ff000022"), ("00000011", "#ff000011")] {
        let mut h = bb25_harness();
        let def = bb5_probe::def();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("tint".into(), serde_json::json!("#00000011"));
        open_def(&mut h, def, values);
        h.get_by_label("Tint").click();
        h.run_steps(1);
        for label in ["Hue", "Saturation"] {
            h.key_press(Key::Tab);
            h.run_steps(1);
            assert_eq!(focused_node(&h).0, label);
        }
        bb25_type(&mut h, "100");
        assert_eq!(focused_node(&h).1, "100");
        for label in ["Value", "Alpha", "Hex"] {
            h.key_press(Key::Tab);
            h.run_steps(1);
            assert_eq!(focused_node(&h).0, label);
        }
        bb25_type(&mut h, text);
        for label in ["Hue", "Saturation", "Value"] {
            h.key_press(Key::Tab);
            h.run_steps(1);
            assert_eq!(focused_node(&h).0, label);
            if label == "Saturation" {
                assert_eq!(focused_node(&h).1, "100", "{text}");
            }
        }
        bb25_type(&mut h, "100");
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(finish(&mut h, "bbProbe")["tint"], want, "{text}");
    }
}

// Fix I2/Q56: an explicit valid hex edit canonicalizes even if RGBA is unchanged.
#[test]
fn bb25_fix1_same_rgba_hex_canonicalizes_explicit_edits() {
    for (raw, text) in [
        ("#fF800011", "FF800011"),
        ("#fF800011", "FF8000"),
        ("primary", "FF800011"),
        ("primary", "FF8000"),
    ] {
        let mut h = bb25_harness();
        h.state_mut().app.primary_color = pdn_core::util::color::Color::new(255, 128, 0, 17);
        let def = bb5_probe::def();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("tint".into(), serde_json::json!(raw));
        open_def(&mut h, def, values);
        h.get_by_label("Tint").click();
        h.run_steps(1);
        bb25_type(&mut h, text);
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(
            finish(&mut h, "bbProbe")["tint"],
            "#ff800011",
            "{raw}: {text}"
        );
    }
}

#[test]
fn bb25_alpha_hex_leave_width_policy() {
    for (text, want) in [
        ("ABCDEF", "#abcdef11"),
        ("#ABCDEF12", "#abcdef12"),
        ("12345", "#fF800011"),
        ("1234567", "#fF800011"),
        ("##123456", "#fF800011"),
        ("000000000", "#ffffffff"),
    ] {
        let mut h = bb25_harness();
        let def = bb5_probe::def();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("tint".into(), serde_json::json!("#fF800011"));
        open_def(&mut h, def, values);
        h.get_by_label("Tint").click();
        h.run_steps(1);
        bb25_type(&mut h, text);
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(finish(&mut h, "bbProbe")["tint"], want, "{text}");
    }
}

// Q57 is the explicit port layout. These are observations of paint.rs, not Windows ColorDialog.
#[test]
fn bb25_capture_popup_examples() {
    for (effect, label, alpha, name) in [
        ("cloudsEffect", "Color 1", false, "opaque-clouds-color1"),
        ("bbOutlineObject", "Color", true, "alpha-outline-object"),
        (
            "levelsEffect",
            "inputHigh color",
            false,
            "levels-input-high",
        ),
    ] {
        let mut h = bb25_harness();
        open_effect(&mut h, effect);
        if effect == "levelsEffect" {
            let at = h.get_by_label(label).rect().center();
            bb25_double_click(&mut h, at);
        } else {
            h.get_by_label(label).click();
            h.run_steps(1);
        }
        assert!(h.query_by_label("Hex").is_some(), "{effect}");
        assert_eq!(h.query_by_label("Alpha").is_some(), alpha);
        if let Ok(dir) = std::env::var("BB25_CAPTURE_DIR") {
            let dir = std::path::Path::new(&dir);
            std::fs::create_dir_all(dir).unwrap();
            h.render()
                .unwrap()
                .save(dir.join(format!("{name}.png")))
                .unwrap();
            let metadata = serde_json::json!({"effect":effect,"opener":label,"viewport_dip":[1280,800],"pixels_per_point":1.0,"canvas":[32,32],"document_dpi":96,"settings":h.state().settings.values,"primary_color":h.state().app.primary_color.to_hex(),"secondary_color":h.state().app.secondary_color.to_hex(),"hex_display":h.get_by_label("Hex").accesskit_node().value(),"effect_settings":"all defaults at dialog open","layout":"R153/Q57 portable effect popup; not native ColorDialog"});
            std::fs::write(
                dir.join(format!("{name}.json")),
                serde_json::to_vec_pretty(&metadata).unwrap(),
            )
            .unwrap();
        }
    }
}

#[test]
fn bb25_numeric_first_arrows_and_popup_alpha_secondary() {
    let mut h = bb25_harness();
    let def = bb5_probe::def();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("tint".into(), serde_json::json!("#ff800011"));
    open_def(&mut h, def, values);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    for (label, key, want) in [
        ("Hue", Key::ArrowUp, "31"),
        ("Saturation", Key::ArrowDown, "99"),
        ("Value", Key::ArrowDown, "99"),
        ("Alpha", Key::ArrowUp, "18"),
    ] {
        h.key_press(Key::Tab);
        h.run_steps(1);
        assert_eq!(focused_node(&h).0, label);
        h.key_press(key);
        h.run_steps(1);
        assert_eq!(focused_node(&h).1, want);
    }
    h.key_press(Key::Enter);
    h.run_steps(1);
    let [r, g, b] = pdn_core::util::color::HsvColor::new(31, 99, 99).to_rgb();
    assert_eq!(
        finish(&mut h, "bbProbe")["tint"],
        format!("#{r:02x}{g:02x}{b:02x}12")
    );
    h.state_mut().app.secondary_color = pdn_core::util::color::Color::new(1, 2, 3, 4);
    open_probe(&mut h);
    let at = h.get_by_label("Tint").rect().center();
    tap(&mut h, at.x, at.y, egui::PointerButton::Secondary);
    assert_eq!(finish(&mut h, "bbProbe")["tint"], "#01020304");
}

#[test]
fn bb25_wheel_modifiers_dpi_same_frame_and_stale_hex_leave() {
    use pdn_core::render::color_ui::{PickMods, wheel_geometry, wheel_pick};
    use pdn_core::util::color::HsvColor;
    for ppp in [1.0, 1.25, 1.5] {
        for modifiers in [
            Modifiers::NONE,
            Modifiers::SHIFT,
            Modifiers::CTRL,
            Modifiers::ALT,
        ] {
            let mut h = Harness::builder()
                .with_max_steps(16)
                .with_size(egui::vec2(1280.0, 800.0))
                .with_pixels_per_point(ppp)
                .with_context_setup(move |ctx| paintrs::seed_native_scale(ctx, ppp))
                .with_step_dt(1.0 / 60.0)
                .with_max_steps(100)
                .build_eframe(new_web_chrome_app);
            h.run_steps(2);
            h.state_mut()
                .app
                .create_blank_document_in_new_workspace(32, 32, 96.0, true);
            h.run_steps(2);
            let def = bb5_probe::def();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert("tint".into(), serde_json::json!("#ff800011"));
            open_def(&mut h, def, values);
            h.get_by_label("Tint").click();
            h.run_steps(1);
            bb25_type(&mut h, "000000000"); // Pending overflow must not win on wheel blur.
            let wheel = h.get_by_label("Color wheel").rect();
            let d = egui::vec2(120.8, 145.2);
            let pos = wheel.min + d / ppp;
            let side = (wheel.width() * ppp).round() as u32;
            let picked = wheel_pick(
                &wheel_geometry(side),
                120,
                145,
                HsvColor::new(30, 100, 100),
                PickMods {
                    shift: modifiers.shift,
                    ctrl: modifiers.ctrl,
                    alt: modifiers.alt,
                },
            );
            h.input_mut()
                .events
                .push(egui::Event::ModifiersChanged(modifiers));
            h.input_mut().events.extend([
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers,
                },
            ]);
            h.run_steps(1);
            h.input_mut()
                .events
                .push(egui::Event::ModifiersChanged(Modifiers::NONE));
            h.key_press(Key::Enter);
            h.run_steps(1);
            let [r, g, b] = picked.to_rgb();
            assert_eq!(
                finish(&mut h, "bbProbe")["tint"],
                format!("#{r:02x}{g:02x}{b:02x}11"),
                "ppp={ppp} modifiers={modifiers:?}"
            );
        }
    }
}

#[test]
fn bb25_outside_same_frame_taps_never_activate_parent() {
    for button in [egui::PointerButton::Primary, egui::PointerButton::Secondary] {
        let mut h = bb25_harness();
        open_probe(&mut h);
        let pos = h.get_by_label("Cancel").rect().center();
        h.get_by_label("Tint").click();
        h.run_steps(1);
        h.input_mut().events.extend([
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                button,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
            egui::Event::PointerButton {
                pos,
                button,
                pressed: false,
                modifiers: Modifiers::NONE,
            },
        ]);
        h.run_steps(1);
        assert!(h.state().shell.dialogs.is_open());
        h.run_steps(1);
        assert!(h.state().shell.dialogs.is_open());
        assert!(h.query_by_label("Hex").is_none());
        h.key_press(Key::Escape);
        h.run_steps(1);
        assert!(!h.state().shell.dialogs.is_open());
    }
}

#[test]
fn bb25_color_disabled_hidden_and_dynamic_reset() {
    use pdn_core::effect::bb::ui::{ControlUi, IndirectUi};
    static UI: IndirectUi = IndirectUi {
        controls: &[(
            "tint",
            ControlUi {
                alpha: true,
                reset: true,
                ..ControlUi::DEFAULT
            },
        )],
        enabled: |v, key| key != "tint" || pdn_core::effect::flag(v, "flag"),
        defaults: |v, env| {
            v.insert(
                "tint".into(),
                serde_json::json!(format!("#{:02x}000080", env.width)),
            );
        },
        ..IndirectUi::DEFAULT
    };
    let mut h = bb25_harness();
    let mut def = bb5_probe::def();
    def.controls.retain(|c| matches!(c.key(), "flag" | "tint"));
    def.indirect = Some(&UI);
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("tint".into(), serde_json::json!("#12345678"));
    open_def(&mut h, def, values);
    assert!(h.get_by_label("Tint").accesskit_node().is_disabled());
    h.get_by_label("Tint").click();
    h.run_steps(1);
    assert!(h.query_by_label("Hex").is_none());
    h.get_by_label("Flag").click();
    h.run_steps(1);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    assert!(h.query_by_label("Hex").is_some());
    h.key_press(Key::Escape);
    h.run_steps(1);
    h.get_by_label("Reset").click();
    h.run_steps(1);
    assert_eq!(finish(&mut h, "bbProbe")["tint"], "#20000080");
    open_probe(&mut h);
    h.get_by_label("Second").click();
    h.run_steps(2);
    assert!(
        h.query_by_label("Tint").is_none(),
        "inactive tab's colour is hidden"
    );
    h.key_press(Key::Escape);
    h.run_steps(1);
}

#[test]
fn bb25_small_viewport_keeps_editors_reachable() {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(200.0, 260.0))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .build_eframe(new_web_chrome_app);
    h.run_steps(2);
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(32, 32, 96.0, true);
    h.run_steps(2);
    open_probe(&mut h);
    h.get_by_label("Tint").focus();
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(1);
    assert!(h.query_by_label("Hex").is_some());
    for label in ["Hue", "Saturation", "Value", "Alpha", "Hex"] {
        h.key_press(Key::Tab);
        h.run_steps(1);
        assert_eq!(focused_node(&h).0, label);
        assert!(
            h.get_by(|n| n.is_focused())
                .rect()
                .intersects(h.ctx.content_rect()),
            "{label} reaches viewport"
        );
    }
    h.key_press(Key::Escape);
    h.run_steps(1);
    assert!(h.state().shell.dialogs.is_open());
}

#[test]
fn bb25_numeric_hue_at_black_wins_over_pending_hex_leave() {
    let mut h = bb25_harness();
    let def = bb5_probe::def();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("tint".into(), serde_json::json!("#00000011"));
    open_def(&mut h, def, values);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    bb25_type(&mut h, "000000000");
    let field = h.get_by_label("Hue").rect();
    let pos = egui::pos2(field.max.x + 7.0, field.min.y + 4.0);
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(
        h.get_by_label("Hex").accesskit_node().value().as_deref(),
        Some("00000011")
    );
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    h.key_press(Key::Enter);
    h.run_steps(1);
    assert_eq!(finish(&mut h, "bbProbe")["tint"], "#00000011");
}

#[test]
fn bb25_value_and_alpha_slider_taps_win_hex_blur_and_keep_rgb() {
    let mut h = bb25_harness();
    let def = bb5_probe::def();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("tint".into(), serde_json::json!("#ff800011"));
    open_def(&mut h, def, values);
    h.get_by_label("Tint").click();
    h.run_steps(1);
    bb25_type(&mut h, "000000000");
    let wheel = h.get_by_label("Color wheel").rect();
    let alpha = wheel.min + egui::vec2(62.0 + 84.0, 180.0 + 8.0 + 3.0 * 29.0 + 12.0);
    tap(&mut h, alpha.x, alpha.y, egui::PointerButton::Primary);
    assert_eq!(
        h.get_by_label("Hex").accesskit_node().value().as_deref(),
        Some("FF8000FF")
    );
    let value = wheel.min + egui::vec2(62.0 + 0.25, 180.0 + 8.0 + 2.0 * 29.0 + 12.0);
    tap(&mut h, value.x, value.y, egui::PointerButton::Primary);
    assert_eq!(
        h.get_by_label("Hex").accesskit_node().value().as_deref(),
        Some("000000FF")
    );
    h.key_press(Key::Enter);
    h.run_steps(1);
    assert_eq!(finish(&mut h, "bbProbe")["tint"], "#000000ff");
}

#[test]
fn bb25_levels_keyboard_opens_exactly_four_endpoint_destinations() {
    let mut h = bb25_harness();
    open_effect(&mut h, "levelsEffect");
    for label in [
        "inputLow color",
        "inputHigh color",
        "outputLow color",
        "outputHigh color",
    ] {
        h.get_by_label(label).focus();
        h.run_steps(1);
        h.key_press(Key::Enter);
        h.run_steps(1);
        assert_eq!(focused_node(&h).0, "Hex");
        assert!(h.query_by_label("Alpha").is_none());
        h.key_press(Key::Escape);
        h.run_steps(1);
        assert_eq!(focused_node(&h).0, label);
    }
    assert!(h.query_by_label("gamma color").is_none());
    h.key_press(Key::Escape);
    h.run_steps(1);
    assert!(!h.state().shell.dialogs.is_open());
}

// R161: these literal values distinguish native coordinates from the old flag/log/step paths.
fn d59_harness(ppp: f32) -> Harness<'static, paintrs::PaintApp> {
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 1000.0))
        .with_pixels_per_point(ppp)
        .with_context_setup(move |ctx| paintrs::seed_native_scale(ctx, ppp))
        .with_step_dt(1.0 / 60.0)
        .with_max_steps(100)
        .build_eframe(new_web_chrome_app);
    h.run_steps(2);
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(16, 16, 96.0, true);
    h.run_steps(2);
    h
}

fn d59_pointer(h: &mut Harness<'_, paintrs::PaintApp>, key: &str, fraction: f32) {
    let rect = h.get_by_role_and_label(Role::Slider, key).rect();
    // Existing Effect thumb is 14 DIP, with the same centre/drag travel as range_line.
    let pos = egui::pos2(
        rect.min.x + 7.0 + fraction * (rect.width() - 14.0),
        rect.center().y,
    );
    h.input_mut().events.extend([
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
}

#[test]
fn d59_registered_pointer_coordinates_are_independent_of_numeric_increment() {
    for ppp in [1.0, 1.25] {
        for (id, key, fraction, want) in [
            ("blurEffect", "radius", 0.25, 18.75),
            ("blurEffect", "radius", 0.5, 75.0),
            ("blurEffect", "radius", 0.75, 168.75),
            ("rotateZoomEffect", "zoom", 0.5, 4.045),
            ("polarInversion", "amount", 0.25, -2.0),
            ("polarInversion", "amount", 0.5, 0.0),
            ("polarInversion", "amount", 0.75, 2.0),
            ("cloudsEffect", "scale", 0.5, 501.0),
            ("twistEffect", "amount", 0.25, -100.0),
            ("bbVignettePlus", "radius", 0.25, 0.25),
            ("bbCombinedAdjustments", "temperature", 0.7, 0.4),
            ("bbCombinedAdjustments", "tint", 0.7, 0.4),
        ] {
            let mut h = d59_harness(ppp);
            open_effect(&mut h, id);
            d59_pointer(&mut h, key, fraction);
            let got = finish(&mut h, id)[key].as_f64().unwrap();
            // At 1.25x AccessKit snaps the chosen centre to x=576.799987793. The unchanged
            // DIP track [433,721], 14-wide thumb and f32 fraction select q=8018, not8030.
            let want = if ppp == 1.25 && id == "rotateZoomEffect" {
                4.03300903387704
            } else {
                want
            };
            assert!(
                (got - want).abs() < 1e-9,
                "{id}/{key} ppp={ppp}: {got}, want {want}"
            );
        }
    }
}

const D59_ANGLES: [(&str, &str, f64, f64, f64); 11] = [
    ("dentsEffect", "angle", -180.0, 180.0, 0.0),
    ("dropShadowEffect", "angle", -180.0, 180.0, -45.0),
    ("embossEffect", "angle", 0.0, 360.0, 0.0),
    ("fragmentEffect", "rotation", 0.0, 360.0, 0.0),
    ("juliaFractalEffect", "angle", -180.0, 180.0, 0.0),
    ("mandelbrotFractalEffect", "angle", -180.0, 180.0, 0.0),
    ("motionBlurEffect", "angle", -180.0, 180.0, 25.0),
    ("radialBlurEffect", "angle", 0.0, 360.0, 4.0),
    ("reliefEffect", "angle", -180.0, 180.0, 45.0),
    ("straightenEffect", "angle", -45.0, 45.0, 0.0),
    ("tileEffect", "rotation", -180.0, 180.0, 30.0),
];

fn d59_number(h: &mut Harness<'_, paintrs::PaintApp>, key: &str) {
    h.get_by_role_and_label(Role::Slider, key).focus();
    h.run_steps(1);
    h.key_press(Key::Tab);
    h.run_steps(1);
    assert_eq!(focused_node(h).0, key);
}

#[test]
fn d59_all_registered_builtin_angles_format_without_quantizing_and_step_one() {
    for (id, key, min, max, default) in D59_ANGLES {
        let registered = pdn_core::effect::find(id).unwrap();
        let pdn_core::effect::Control::Range {
            min: lo,
            max: hi,
            default: initial,
            ..
        } = registered.controls.iter().find(|c| c.key() == key).unwrap()
        else {
            panic!("registered angle")
        };
        assert_eq!((*lo, *hi, *initial), (min, max, default), "{id}/{key}");
        let mut h = d59_harness(1.0);
        open_effect(&mut h, id);
        d59_number(&mut h, key);
        assert_eq!(focused_node(&h).1, format!("{default:.2}"), "{id}");
        h.get_by_label("Cancel").click();
        h.run_steps(2);
        let def = pdn_core::effect::find(id).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert(key.into(), serde_json::json!(0.2534));
        open_def(&mut h, def, values);
        d59_number(&mut h, key);
        assert_eq!(focused_node(&h).1, "0.25");
        for _ in 0..2 {
            d59_press(&mut h, Key::ArrowUp);
        }
        assert_eq!(focused_node(&h).1, "2.25");
        assert!(
            (finish(&mut h, id)[key].as_f64().unwrap() - 2.2534).abs() < 1e-10,
            "{id}"
        );
    }
}

#[test]
fn d59_combined_numeric_defaults_keep_fractional_pointer_and_raw_storage() {
    for key in ["temperature", "tint"] {
        for (arrow, want) in [(Key::ArrowUp, 1.0), (Key::ArrowDown, -0.6)] {
            let mut h = d59_harness(1.0);
            let def = pdn_core::effect::find("bbCombinedAdjustments").unwrap();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert(key.into(), serde_json::json!(0.4));
            open_def(&mut h, def, values);
            d59_number(&mut h, key);
            assert_eq!(focused_node(&h).1, "0.40");
            d59_press(&mut h, arrow);
            assert!(
                (finish(&mut h, "bbCombinedAdjustments")[key]
                    .as_f64()
                    .unwrap()
                    - want)
                    .abs()
                    < 1e-10
            );
        }
    }
}

fn d59_press(h: &mut Harness<'_, paintrs::PaintApp>, key: Key) {
    h.key_press(key);
    h.run_steps(1);
}

fn d59_type(h: &mut Harness<'_, paintrs::PaintApp>, text: &str) {
    h.key_press_modifiers(Modifiers::CTRL, Key::A);
    h.event(egui::Event::Text(text.into()));
    h.run_steps(1);
}

#[test]
fn d59_angle_strict_endpoints_pending_text_keys_and_spin() {
    for (id, initial, typed, up, want) in [
        ("motionBlurEffect", 179.0, None, true, 180.0),
        ("motionBlurEffect", 180.0, None, true, -180.0),
        ("motionBlurEffect", 179.75, None, true, -180.0),
        ("motionBlurEffect", -180.0, None, false, 180.0),
        ("motionBlurEffect", 25.0, Some("181"), true, -180.0),
        ("motionBlurEffect", 25.0, Some("181"), false, 179.0),
        ("embossEffect", 359.0, None, true, 360.0),
        ("embossEffect", 360.0, None, true, 0.0),
        ("embossEffect", 359.75, None, true, 0.0),
        ("embossEffect", 0.0, None, false, 360.0),
        ("embossEffect", 0.0, Some("361"), true, 0.0),
        ("straightenEffect", 45.0, None, true, 45.0),
        ("straightenEffect", -45.0, None, false, -45.0),
        ("straightenEffect", 44.25, None, true, 45.0),
        ("straightenEffect", 0.0, Some("46"), false, 44.0),
    ] {
        for spin in [false, true] {
            let mut h = d59_harness(1.0);
            let def = pdn_core::effect::find(id).unwrap();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert("angle".into(), serde_json::json!(initial));
            open_def(&mut h, def, values);
            d59_number(&mut h, "angle");
            if let Some(text) = typed {
                d59_type(&mut h, text);
            }
            if spin {
                let rect = h.get_by_role_and_label(Role::Slider, "angle").rect();
                let pos = egui::pos2(
                    rect.max.x + 8.0 + 72.0 - 7.0,
                    rect.center().y + if up { -4.0 } else { 4.0 },
                );
                h.input_mut().events.extend([
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: Modifiers::NONE,
                    },
                ]);
                h.run_steps(1);
                h.event(egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: Modifiers::NONE,
                });
                h.run_steps(1);
            } else {
                d59_press(&mut h, if up { Key::ArrowUp } else { Key::ArrowDown });
            }
            assert_eq!(
                finish(&mut h, id)["angle"],
                serde_json::json!(want),
                "{id}, initial={initial}, typed={typed:?}, spin={spin}"
            );
        }
    }
}

#[test]
fn d59_angle_typed_clamps_and_modified_or_nonstep_keys_preserve_raw() {
    for (id, text, want) in [
        ("motionBlurEffect", "181", 180.0),
        ("embossEffect", "361", 360.0),
        ("straightenEffect", "46", 45.0),
    ] {
        let mut h = d59_harness(1.0);
        open_effect(&mut h, id);
        d59_number(&mut h, "angle");
        d59_type(&mut h, text);
        d59_press(&mut h, Key::Tab);
        assert_eq!(finish(&mut h, id)["angle"], serde_json::json!(want));
    }
    let mut h = d59_harness(1.0);
    let def = pdn_core::effect::find("motionBlurEffect").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("angle".into(), serde_json::json!(45.2534));
    open_def(&mut h, def, values);
    d59_number(&mut h, "angle");
    for modifiers in [
        Modifiers::SHIFT,
        Modifiers::CTRL,
        Modifiers::ALT,
        Modifiers::MAC_CMD,
    ] {
        h.key_press_modifiers(modifiers, Key::ArrowUp);
        h.run_steps(1);
    }
    for key in [
        Key::PageUp,
        Key::PageDown,
        Key::ArrowLeft,
        Key::ArrowRight,
        Key::Home,
        Key::End,
    ] {
        d59_press(&mut h, key);
    }
    d59_press(&mut h, Key::Tab);
    assert_eq!(
        finish(&mut h, "motionBlurEffect")["angle"],
        serde_json::json!(45.2534)
    );
}

#[test]
fn d59_angle_replacement_grids_remain_separate_from_numeric_steps() {
    for (id, fraction, want) in [
        ("straightenEffect", 0.137, -32.7),
        ("motionBlurEffect", 0.137, -131.0),
        ("bbInnerShadowObject", 0.137, -131.0),
    ] {
        let mut h = d59_harness(1.0);
        open_effect(&mut h, id);
        d59_pointer(&mut h, "angle", fraction);
        assert!(
            (finish(&mut h, id)["angle"].as_f64().unwrap() - want).abs() < 1e-9,
            "{id}"
        );
    }
}

#[test]
fn d59_companion_pointer_grids_and_pending_nonlinear_press() {
    for ppp in [1.0, 1.25] {
        let mut h = d59_harness(ppp);
        open_effect(&mut h, "rotateZoomEffect");
        d59_pointer(&mut h, "panX", 0.7);
        // The unchanged Pan track is [547,721]. At 1.25x the snapped-node 70% click
        // is x=665.919982910, f32 fraction.699499905, native q399, hence3.99.
        let want = if ppp == 1.0 { 4.0 } else { 3.99 };
        assert_eq!(finish(&mut h, "rotateZoomEffect")["panX"], want);
        let def = pdn_core::effect::find("bbDropShadowPlus").unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert("shadowType".into(), serde_json::json!(2));
        open_def(&mut h, def, values);
        d59_pointer(&mut h, "location.X", 0.6875);
        // Existing raw track [568.5,727.5], thumb14. Snapped-node clicks x=675.6875
        // and675.075012207 give f32 fractions.690948248/.686724246, q382/373 at p3.
        let want = if ppp == 1.0 { 0.382 } else { 0.373 };
        assert_eq!(finish(&mut h, "bbDropShadowPlus")["location"][0], want);
        open_effect(&mut h, "blurEffect");
        d59_number(&mut h, "radius");
        d59_type(&mut h, "20");
        d59_pointer(&mut h, "radius", 0.5);
        assert_eq!(finish(&mut h, "blurEffect")["radius"], 75.0);
    }
}

#[test]
fn d59_painting_and_numeric_formatting_preserve_off_grid_storage() {
    for (id, key, raw, shown) in [
        ("rotateZoomEffect", "zoom", 1.0, "1.00"),
        ("motionBlurEffect", "angle", 45.2534, "45.25"),
        ("bbCombinedAdjustments", "temperature", 0.375, "0.38"),
        ("bbCombinedAdjustments", "tint", 0.375, "0.38"),
    ] {
        let mut h = d59_harness(1.0);
        let def = pdn_core::effect::find(id).unwrap();
        let mut values = pdn_core::effect::defaults(&def);
        values.insert(key.into(), serde_json::json!(raw));
        open_def(&mut h, def, values);
        d59_number(&mut h, key);
        assert_eq!(focused_node(&h).1, shown);
        if id == "bbCombinedAdjustments" {
            d59_type(&mut h, ".375");
        }
        d59_press(&mut h, Key::Tab);
        h.run_steps(5);
        d59_number(&mut h, key);
        assert_eq!(focused_node(&h).1, shown);
        assert_eq!(
            finish(&mut h, id)[key],
            serde_json::json!(raw),
            "{id}/{key}"
        );
    }
}

#[test]
fn d59_zero_minimum_old_exponential_flags_open_finitely_and_keep_values() {
    for (id, key, default, max) in [
        ("dropShadowEffect", "blur", 10.0, 100.0),
        ("edgeDetectEffect", "blurring", 0.0, 10.0),
        ("sharpenEffect", "threshold", 0.0, 1.0),
        ("vignetteEffect", "strength", 1.0, 1.0),
    ] {
        for initial in [0.0, default, max] {
            let mut h = d59_harness(1.0);
            let def = pdn_core::effect::find(id).unwrap();
            let mut values = pdn_core::effect::defaults(&def);
            values.insert(key.into(), serde_json::json!(initial));
            open_def(&mut h, def, values);
            let rect = h.get_by_role_and_label(Role::Slider, key).rect();
            assert!(rect.is_finite());
            h.run_steps(2);
            assert_eq!(
                finish(&mut h, id)[key],
                serde_json::json!(initial),
                "{id}/{key}"
            );
        }
    }
}

#[test]
fn d59_actual_angle_preview_ok_and_cancel_preserve_single_history_boundary() {
    for cancel in [false, true] {
        let mut h = d59_harness(1.0);
        let ws = h.state_mut().app.active_ws_mut().unwrap();
        for (i, px) in ws
            .document
            .layers
            .get_at_mut(0)
            .unwrap()
            .surface
            .data_mut()
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .enumerate()
        {
            px.copy_from_slice(&[(i % 16) as u8 * 10, (i / 16) as u8 * 7, 30, 255]);
        }
        let before = select_pixels(&h);
        let history = undo_len(&h);
        let dirty = h.state().app.active_ws().unwrap().dirty;
        let def = pdn_core::effect::find("embossEffect").unwrap();
        let values = pdn_core::effect::with(&def, &serde_json::json!({"angle":46.2534}));
        let expected = pdn_core::effect::apply(
            &pdn_core::canvas::ImageData {
                width: 16,
                height: 16,
                data: before.clone(),
            },
            &def,
            &values,
            &pdn_core::effect::EffectEnv::whole(16, 16),
        )
        .data;
        let prior = serde_json::json!({"angle":12.2534});
        h.state_mut().app.effect_tokens.insert(
            "embossEffect".into(),
            serde_json::from_value(prior.clone()).unwrap(),
        );
        open_effect(&mut h, "embossEffect");
        d59_number(&mut h, "angle");
        d59_type(&mut h, "45.2534");
        d59_press(&mut h, Key::ArrowUp);
        for _ in 0..100 {
            h.run_steps(1);
            if select_pixels(&h) == expected {
                break;
            }
            std::thread::yield_now();
        }
        assert_eq!(select_pixels(&h), expected);
        assert_ne!(expected, before);
        assert_eq!(undo_len(&h), history);
        assert_eq!(
            h.state().app.effect_tokens["embossEffect"]["angle"],
            prior["angle"]
        );
        if cancel {
            h.get_by_label("Cancel").click();
            h.run_steps(2);
            assert_eq!(select_pixels(&h), before);
            assert_eq!(undo_len(&h), history);
            assert_eq!(h.state().app.active_ws().unwrap().dirty, dirty);
            assert_eq!(
                h.state().app.effect_tokens["embossEffect"]["angle"],
                prior["angle"]
            );
        } else {
            let shown = select_pixels(&h);
            assert_eq!(
                finish(&mut h, "embossEffect")["angle"],
                serde_json::json!(46.2534)
            );
            assert_eq!(select_pixels(&h), shown);
            assert_eq!(undo_len(&h), history + 1);
            assert_eq!(
                h.state().app.last_effect_values.as_ref().unwrap()["angle"],
                serde_json::json!(46.2534)
            );
        }
    }
}

#[test]
fn d59_thumb_offset_drag_clamps_release_and_reset_keep_existing_geometry() {
    let mut h = d59_harness(1.0);
    let def = pdn_core::effect::find("blurEffect").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("radius".into(), serde_json::json!(75.0));
    open_def(&mut h, def, values);
    let rect = h.get_by_role_and_label(Role::Slider, "radius").rect();
    let mut pos = egui::pos2(rect.center().x + 3.0, rect.center().y);
    h.input_mut().events.extend([
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run_steps(1);
    pos.x = rect.min.x + 7.0 + (rect.width() - 14.0) * 0.75 + 3.0;
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(finish(&mut h, "blurEffect")["radius"], 168.75);
    open_effect(&mut h, "blurEffect");
    let rect = h.get_by_role_and_label(Role::Slider, "radius").rect();
    pos = egui::pos2(rect.center().x, rect.center().y);
    h.input_mut().events.extend([
        egui::Event::PointerMoved(pos),
        egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        },
    ]);
    h.run_steps(1);
    pos.x = rect.max.x + 50.0;
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    h.event(egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(1);
    assert_eq!(finish(&mut h, "blurEffect")["radius"], 300.0);
    open_effect(&mut h, "blurEffect");
    d59_number(&mut h, "radius");
    d59_press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h).0, "Reset");
    h.get_by(|n| n.is_focused()).click();
    h.run_steps(1);
    assert_eq!(finish(&mut h, "blurEffect")["radius"], 2.0);
}

/// BB26 Controls tables: actual slider coordinates and numeric metadata on both clipboard dialogs.
#[test]
fn bb26_clipboard_fractional_keyboard_controls() {
    for (id, zoom) in [
        ("bbFillFromClipboard", 2.417550316185697),
        ("bbPasteAlpha", 1.30),
    ] {
        let mut h = Harness::builder()
            .with_max_steps(16)
            .with_size(egui::vec2(1280.0, 800.0))
            .with_step_dt(1.0 / 60.0)
            .build_eframe(new_web_chrome_app);
        h.run_steps(2);
        h.state_mut()
            .app
            .create_blank_document_in_new_workspace(3, 2, 96.0, true);
        h.state_mut().shell.clipboard = paintrs::platform::Clipboard::in_memory();
        let def = pdn_core::effect::find(id).unwrap();
        let values = pdn_core::effect::defaults(&def);
        open_def(&mut h, def, values);
        h.get_by_role_and_label(Role::Slider, "zoom").focus();
        h.run_steps(1);
        press(&mut h, Key::ArrowRight);
        h.get_by_role_and_label(Role::Slider, "zoom").focus();
        h.run_steps(1);
        press(&mut h, Key::PageUp);
        h.get_by_role_and_label(Role::Slider, "placement.X").focus();
        h.run_steps(1);
        press(&mut h, Key::ArrowRight);
        h.get_by_role_and_label(Role::Slider, "placement.X").focus();
        h.run_steps(1);
        press(&mut h, Key::PageUp);
        h.get_by_label("Nearest neighbor sampling").click();
        h.run_steps(1);
        let token = finish(&mut h, id);
        // Fill uses independently calculated native coordinates 39308+4592+18002=61902,
        // inverted over [.06,16]. Paste Alpha uses linear 1+.05+.25.
        assert!(
            (token["zoom"].as_f64().unwrap() - zoom).abs() < 1e-12,
            "{id}: {token:?}"
        );
        let x = if id == "bbPasteAlpha" { -0.70 } else { 0.30 };
        assert!((token["placement"][0].as_f64().unwrap() - x).abs() < 1e-12);
        assert_eq!(token["nearest"], serde_json::json!(true));
    }
}

/// BB27 common.md §3 "File chooser": the editable path box is the first editor, Tab reaches the
/// accessible Browse button, Space picks, and OK commits the picked image's fill.
#[test]
fn bb27_file_chooser_row_keyboard() {
    let files = paintrs::platform::Files::in_memory();
    files.clone().install();
    let quad = pdn_core::canvas::ImageData {
        width: 2,
        height: 2,
        data: [
            [255, 0, 0, 255],
            [0, 255, 0, 255],
            [0, 0, 255, 255],
            [0, 0, 0, 255],
        ]
        .concat(),
    };
    let png = pdn_core::io::encode::encode(
        &quad,
        "png",
        &pdn_core::io::encode::SaveOptions::default_for("png"),
    )
    .unwrap();
    files.queue_pick(Ok(vec![paintrs::platform::PickedFile::snapshot(
        "pick.png".into(),
        png,
    )]));
    let mut h = Harness::builder()
        .with_max_steps(16)
        .with_size(egui::vec2(1280.0, 800.0))
        .with_step_dt(1.0 / 60.0)
        .build_eframe(new_web_chrome_app);
    h.run_steps(2);
    h.state_mut()
        .app
        .create_blank_document_in_new_workspace(8, 8, 96.0, true);
    let def = pdn_core::effect::find("bbFillFromFile").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("nearest".into(), serde_json::json!(true));
    open_def(&mut h, def, values);
    let focused = h.query_by(|n| n.is_focused()).unwrap();
    assert_eq!(
        focused.accesskit_node().label().as_deref(),
        Some("File to load")
    );
    h.get_by_role_and_label(Role::Button, "Browse");
    press(&mut h, Key::Tab);
    let focused = h.query_by(|n| n.is_focused()).unwrap();
    assert_eq!(
        (
            focused.accesskit_node().role(),
            focused.accesskit_node().label().as_deref()
        ),
        (Role::Button, Some("Browse"))
    );
    press(&mut h, Key::Space);
    let token = finish(&mut h, "bbFillFromFile");
    assert_eq!(token["file"], serde_json::json!("pick.png"));
    let layer = select_pixels(&h);
    let at = |x: usize, y: usize| layer[(y * 8 + x) * 4..][..4].to_vec();
    assert_eq!(
        [at(3, 3), at(4, 3), at(3, 4), at(4, 4), at(0, 0)],
        [
            quad.data[..4].to_vec(),
            quad.data[4..8].to_vec(),
            quad.data[8..12].to_vec(),
            quad.data[12..].to_vec(),
            vec![255; 4]
        ]
    );
}

/// BB28 (R152 Q54): Level Horizon over the 32 × 32 harness document from `start`/`end`; the
/// layer before the dialog.
fn bb28_horizon(h: &mut Harness<'_, paintrs::PaintApp>, start: [f64; 2], end: [f64; 2]) -> Vec<u8> {
    let before = select_pixels(h);
    let def = pdn_core::effect::find("bbLevelHorizon").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("start".into(), serde_json::json!(start));
    values.insert("end".into(), serde_json::json!(end));
    open_def(h, def, values);
    before
}

/// The committed layer is the token's render with the indicator `seen`, which the token itself
/// never stores.
fn bb28_assert_label(
    h: &Harness<'_, paintrs::PaintApp>,
    before: &[u8],
    token: &Values,
    seen: bool,
) {
    assert!(!token.contains_key("plumbSeen"), "transient");
    let def = pdn_core::effect::find("bbLevelHorizon").unwrap();
    let culture = pdn_core::i18n::culture_code();
    let env = pdn_core::effect::EffectEnv {
        culture: &culture,
        ..pdn_core::effect::EffectEnv::whole(32, 32)
    };
    let src = pdn_core::canvas::ImageData {
        width: 32,
        height: 32,
        data: before.to_vec(),
    };
    let mut v = token.clone();
    let render = |v: &Values| pdn_core::effect::apply(&src, &def, v, &env).data;
    v.insert("plumbSeen".into(), serde_json::json!(seen));
    let want = render(&v);
    v.insert("plumbSeen".into(), serde_json::json!(!seen));
    if token["step"] == serde_json::json!(0) {
        assert_ne!(want, render(&v), "the indicators differ in pixels");
    }
    assert_eq!(select_pixels(h), want, "indicator seen = {seen}");
}

fn bb28_press_at(h: &mut Harness<'_, paintrs::PaintApp>, pos: egui::Pos2) {
    h.event(egui::Event::PointerMoved(pos));
    h.run_steps(1);
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        });
        h.run_steps(1);
    }
    h.run_steps(2);
}

// Keys: a plumb visit and back to level, OK on step 1, then a fresh Repeat instance.
#[test]
fn bb28_horizon_key_visit_sticks_through_ok() {
    let mut h = bb12_harness();
    let before = bb28_horizon(&mut h, [0.0, -1.0], [0.1, -1.0]);
    h.get_by_role_and_label(Role::Slider, "end.Y").focus();
    h.run_steps(2);
    press(&mut h, Key::PageUp);
    press(&mut h, Key::PageDown);
    let token = finish(&mut h, "bbLevelHorizon");
    assert_eq!(
        (token["end"].clone(), token["step"].clone()),
        (serde_json::json!([0.1, -1.0]), serde_json::json!(0))
    );
    bb28_assert_label(&h, &before, &token, true);
    // Repeat: a new instance with the remembered step; the level line shows ↔.
    let committed = select_pixels(&h);
    assert_eq!(
        pdn_core::action::perform(&mut h.state_mut().app, "menu.effects.repeat"),
        pdn_core::action::ActionOutcome::Done
    );
    bb28_assert_label(&h, &committed, &token, false);
}

// Typed numeric edits, the last committed in the same frame as the OK click.
#[test]
fn bb28_horizon_typed_visit_and_same_frame_ok() {
    let mut h = bb12_harness();
    let before = bb28_horizon(&mut h, [0.0, -1.0], [0.1, -1.0]);
    h.get_by_role_and_label(Role::Slider, "end.Y").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    for (text, commit) in [("1", true), ("-1", false)] {
        h.key_press_modifiers(Modifiers::CTRL, Key::A);
        h.event(egui::Event::Text(text.into()));
        h.run_steps(2);
        if commit {
            press(&mut h, Key::Tab);
            h.key_press_modifiers(Modifiers::SHIFT, Key::Tab);
            h.run_steps(2);
        }
    }
    let token = finish(&mut h, "bbLevelHorizon");
    assert_eq!(
        token["end"],
        serde_json::json!([0.1, -1.0]),
        "the pending text committed with OK"
    );
    bb28_assert_label(&h, &before, &token, true);
}

// The numeric spin button, then the axis reset arrow back to the level default.
#[test]
fn bb28_horizon_spin_visit_and_reset() {
    let mut h = bb12_harness();
    let before = bb28_horizon(&mut h, [0.0, -0.33], [0.005, -0.33]);
    let rect = h.get_by_role_and_label(Role::Slider, "end.Y").rect();
    bb28_press_at(
        &mut h,
        egui::pos2(rect.max.x + 8.0 + 72.0 - 7.0, rect.center().y - 4.0),
    );
    assert_eq!(focused_node(&h).1, "-0.320");
    bb28_press_at(&mut h, egui::pos2(rect.max.x + 102.0, rect.center().y));
    let token = finish(&mut h, "bbLevelHorizon");
    assert_eq!(
        token["end"],
        serde_json::json!([0.005, -0.33]),
        "reset to the default"
    );
    bb28_assert_label(&h, &before, &token, true);
}

// A pointer press on the pad, then keys back to a non-plumb line; the tab switch keeps it.
#[test]
fn bb28_horizon_pointer_visit_and_tabs() {
    let mut h = bb12_harness();
    let before = bb28_horizon(&mut h, [0.0, -1.0], [0.1, -1.0]);
    let pad = h
        .get_by_role_and_label(Role::Image, "Ending Position")
        .rect();
    bb28_press_at(&mut h, egui::pos2(pad.center().x, pad.max.y - 3.0));
    h.get_by_role_and_label(Role::Slider, "end.Y").focus();
    h.run_steps(2);
    for _ in 0..8 {
        press(&mut h, Key::PageDown);
    }
    for _ in 0..2 {
        h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
        h.run_steps(3);
    }
    let token = finish(&mut h, "bbLevelHorizon");
    assert_eq!(token["end"][1], serde_json::json!(-1.0));
    assert_eq!(token["step"], serde_json::json!(0));
    bb28_assert_label(&h, &before, &token, true);
}

// The tab drives the output: OK on step 2 rotates and Repeat keeps the step.
#[test]
fn bb28_horizon_step_tab_drives_output_and_repeat() {
    let mut h = bb12_harness();
    let before = bb28_horizon(&mut h, [-0.9, -0.33], [0.9, -0.23]);
    h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    h.run_steps(3);
    assert!(
        h.query_by_role_and_label(Role::Slider, "end.Y").is_none(),
        "step 2 page"
    );
    let token = finish(&mut h, "bbLevelHorizon");
    assert_eq!(token["step"], serde_json::json!(1));
    bb28_assert_label(&h, &before, &token, false);
    let committed = select_pixels(&h);
    assert_eq!(
        pdn_core::action::perform(&mut h.state_mut().app, "menu.effects.repeat"),
        pdn_core::action::ActionOutcome::Done
    );
    bb28_assert_label(&h, &committed, &token, false);
}

/// Runs frames until no dialog is open (lazy faces load on the app's font manager).
fn bb29_until_closed(h: &mut Harness<'_, paintrs::PaintApp>) {
    for _ in 0..1000 {
        h.run_steps(1);
        if !h.state().shell.dialogs.is_open() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the dialog did not close");
}

/// R118 / R157 in the real app: Meme Maker's top caption is the first editor; typed text goes
/// in, Enter, Shift+Enter and Ctrl+Enter stay in the box (no submit); OK waits for the app's
/// font manager (no Impact here, so the bundled Anton) and commits one entry with the raw
/// CRLF kept and no face/tab in the token; Ctrl+F repeats it through the final-only owner.
#[test]
fn bb29_meme_text_keys_lazy_face_ok_and_repeat() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbMemeMaker").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    values.insert("top".into(), serde_json::json!("a\r\nb"));
    open_def(&mut h, def, values);
    let focused = h.query_by(|n| n.is_focused()).unwrap();
    assert_eq!(focused.accesskit_node().role(), Role::MultilineTextInput);
    let history = h.state().app.active_ws().unwrap().history.undo.len();
    h.event(egui::Event::Text("c".into()));
    h.run_steps(1);
    for modifiers in [Modifiers::NONE, Modifiers::SHIFT, Modifiers::CTRL] {
        h.key_press_modifiers(modifiers, Key::Enter);
        h.run_steps(2);
        assert!(
            h.state().shell.dialogs.is_open(),
            "{modifiers:?} Enter submitted"
        );
    }
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history
    );
    h.get_by_label("OK").click();
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 1
    );
    let token = h.state().app.effect_tokens["bbMemeMaker"].clone();
    // R178: each of the three Enters inserted exactly one LF; the existing CRLF is kept.
    assert_eq!(token["top"], serde_json::json!("a\r\nbc\n\n\n"));
    assert!(!token.contains_key("face") && !token.contains_key("tab"));
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 2
    );
    assert_eq!(h.state().app.effect_tokens["bbMemeMaker"], token);
}

/// R154 in the real app (captured 2026-02-10, culture en): the month list shows the culture's
/// names, both font rows list Task 53's families with Selawik chosen, the year slider stops at
/// today ± 120 while a remembered out-of-range year clamps only in the opened draft (Cancel
/// keeps the token, Repeat renders the remembered year), and OK waits for the emoji face.
#[test]
fn bb30_calendar_dialog_rows_year_range_ok_and_repeat() {
    use pdn_core::effect::rng::CivilDate;
    let mut h = bb12_harness();
    // The app's glyph policy: the R47 interface faces have no emoji, so the holiday box shows
    // the default list's emoji as boxes (the document text draws them in colour).
    h.ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    h.state_mut().app.date_override = Some(CivilDate::new(2026, 2, 10));
    open_effect(&mut h, "bbCalendar");
    let month = h.get_by_role_and_label(Role::ComboBox, "month");
    assert_eq!(
        month.accesskit_node().value().as_deref(),
        Some("February (2)")
    );
    let font = h.get_by_role_and_label(Role::ComboBox, "font");
    assert_eq!(font.accesskit_node().value().as_deref(), Some("Selawik"));
    h.get_by_role_and_label(Role::Slider, "year").focus();
    h.run_steps(2);
    for _ in 0..30 {
        press(&mut h, Key::PageUp);
    }
    h.get_by_label("OK").click();
    bb29_until_closed(&mut h);
    let token = h.state().app.effect_tokens["bbCalendar"].clone();
    assert_eq!(
        token["year"].as_f64(),
        Some(2146.0),
        "the slider stops at 2026 + 120"
    );
    assert_eq!(token["month"], serde_json::json!("february"));
    assert!(
        token["holidays"]
            .as_str()
            .unwrap()
            .starts_with("2026-08-04, ")
    );
    // A remembered year outside this session's range: Cancel keeps it; OK clamps the draft.
    let mut old = token.clone();
    old.insert("year".into(), serde_json::json!(1800));
    h.state_mut()
        .app
        .effect_tokens
        .insert("bbCalendar".into(), old.clone());
    open_effect(&mut h, "bbCalendar");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(
        h.state().app.effect_tokens["bbCalendar"]["year"],
        serde_json::json!(1800)
    );
    open_effect(&mut h, "bbCalendar");
    h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    h.key_press_modifiers(Modifiers::CTRL, Key::Tab);
    h.run_steps(3);
    let holiday = h.get_by_role_and_label(Role::ComboBox, "Holiday font");
    assert_eq!(holiday.accesskit_node().value().as_deref(), Some("Selawik"));
    let history = h.state().app.active_ws().unwrap().history.undo.len();
    h.get_by_label("OK").click();
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.effect_tokens["bbCalendar"]["year"].as_f64(),
        Some(1906.0)
    );
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 1
    );
    // Repeat keeps a remembered valid year whatever today's range.
    h.state_mut().app.last_effect_values = Some(old);
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 2
    );
    assert_eq!(
        h.state().app.last_effect_values.as_ref().unwrap()["year"],
        serde_json::json!(1800)
    );
}

/// R154: a drop-down's option labels come from its presentation callback and the session env
/// (Calendar's culture month names); the stored value stays the stable key.
#[test]
fn bb30_select_labels_follow_the_session_callback() {
    use pdn_core::effect::bb::ui::{ControlUi, IndirectUi};
    static UI: IndirectUi = IndirectUi {
        controls: &[(
            "choice",
            ControlUi {
                choice_label: Some(|v, env| Some(format!("{v} {}", env.today.year))),
                ..ControlUi::DEFAULT
            },
        )],
        ..IndirectUi::DEFAULT
    };
    let mut h = bb12_harness();
    h.state_mut().app.date_override = Some(pdn_core::effect::rng::CivilDate::new(2031, 5, 6));
    let mut def = pdn_core::effect::find("invertColorsEffect").unwrap();
    def.id = "bb30LabelProbe";
    def.controls = vec![pdn_core::effect::Control::Select {
        key: "choice",
        label: "Choice",
        default: "b",
        choices: &[("a", "Alpha"), ("b", "Beta")],
    }];
    def.indirect = Some(&UI);
    let values = pdn_core::effect::defaults(&def);
    open_def(&mut h, def, values);
    let select = h.get_by_role_and_label(Role::ComboBox, "Choice");
    assert_eq!(select.accesskit_node().value().as_deref(), Some("b 2031"));
    assert_eq!(
        finish(&mut h, "bb30LabelProbe")["choice"],
        serde_json::json!("b")
    );
}

/// R154 (captured 2026-02-10): the dynamic year range through accessibility (value, min 1906,
/// max 2146) and a pointer drag far past the track's end (2146); a typed year committed by the OK
/// click in the same frame (in range as typed, outside clamped to the session's limits, never to
/// the static 1..9999); Undo and Redo restore and reapply the exact layer bytes.
#[test]
fn bb30_calendar_year_pointer_accessibility_same_frame_ok_and_history() {
    use pdn_core::effect::rng::CivilDate;
    let mut h = bb12_harness();
    h.ctx
        .set_missing_glyph_policy(egui::MissingGlyphPolicy::Tofu);
    h.state_mut().app.date_override = Some(CivilDate::new(2026, 2, 10));
    let source = select_pixels(&h);
    let history = h.state().app.active_ws().unwrap().history.undo.len();
    open_effect(&mut h, "bbCalendar");
    let year = h.get_by_role_and_label(Role::Slider, "year");
    let node = year.accesskit_node();
    assert_eq!(
        (
            node.numeric_value(),
            node.min_numeric_value(),
            node.max_numeric_value()
        ),
        (Some(2026.0), Some(1906.0), Some(2146.0))
    );
    let rect = year.rect();
    let start = rect.center();
    h.event(egui::Event::PointerMoved(start));
    h.run_steps(1);
    let button = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    };
    h.event(button(start, true));
    h.run_steps(1);
    let end = egui::pos2(rect.max.x + 400.0, start.y);
    for x in [rect.max.x + 40.0, end.x] {
        h.event(egui::Event::PointerMoved(egui::pos2(x, start.y)));
        h.run_steps(1);
    }
    h.event(button(end, false));
    h.run_steps(2);
    let node = h
        .get_by_role_and_label(Role::Slider, "year")
        .accesskit_node();
    assert_eq!(
        node.numeric_value(),
        Some(2146.0),
        "the drag stops at 2026 + 120"
    );
    h.get_by_label("OK").click();
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.effect_tokens["bbCalendar"]["year"].as_f64(),
        Some(2146.0)
    );
    let applied = select_pixels(&h);
    assert_ne!(applied, source);
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 1
    );
    for (action, want) in [("menu.edit.undo", &source), ("menu.edit.redo", &applied)] {
        assert_eq!(
            pdn_core::action::perform(&mut h.state_mut().app, action),
            pdn_core::action::ActionOutcome::Done
        );
        h.run_steps(2);
        assert!(select_pixels(&h) == *want, "{action} bytes");
    }
    // A typed year left pending in the number box commits with the OK click.
    for (text, want) in [("2030", 2030.0), ("1800", 1906.0), ("3000", 2146.0)] {
        open_effect(&mut h, "bbCalendar");
        h.get_by_role_and_label(Role::Slider, "year").focus();
        h.run_steps(2);
        press(&mut h, Key::Tab);
        h.key_press_modifiers(Modifiers::CTRL, Key::A);
        h.event(egui::Event::Text(text.into()));
        h.run_steps(2);
        h.get_by_label("OK").click();
        bb29_until_closed(&mut h);
        assert_eq!(
            h.state().app.effect_tokens["bbCalendar"]["year"].as_f64(),
            Some(want),
            "typed {text}"
        );
    }
}

/// BB31 in the real app: the Fill tab holds its sub-tabs and the selected sub-tab is the fill
/// type; both tab values (plugin settings here) survive OK and Repeat, one history entry each.
#[test]
fn bb31_text_fun_factory_fill_tabs_ok_and_repeat() {
    let mut h = bb12_harness();
    let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    for (k, v) in [
        ("text", serde_json::json!("Hi")),
        ("font", serde_json::json!("Noto Sans Mono")),
        ("size", serde_json::json!(20)),
    ] {
        values.insert(k.into(), v);
    }
    open_def(&mut h, def, values);
    let history = h.state().app.active_ws().unwrap().history.undo.len();
    h.get_by_role_and_label(Role::Tab, "Fill").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Tab, "Gradient").click();
    h.run_steps(3);
    // The Gradient page's captioned drop-down is the one shown.
    assert!(h.query_by_label("Style:").is_some());
    assert!(h.query_by_label("Color:").is_none());
    let token = finish(&mut h, "bbTextFunFactory");
    assert_eq!(
        (token["tab"].clone(), token["fillTab"].clone()),
        (serde_json::json!(1), serde_json::json!(1))
    );
    assert!(!token.contains_key("fallbackFont") && !token.contains_key("fallbackFamily"));
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 1
    );
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    bb29_until_closed(&mut h);
    assert_eq!(
        h.state().app.active_ws().unwrap().history.undo.len(),
        history + 2
    );
    assert_eq!(h.state().app.effect_tokens["bbTextFunFactory"], token);
}

/// Runs frames until the bottom layer's bytes differ from `from`, then returns them.
fn bb31_until_changed(h: &mut Harness<'_, paintrs::PaintApp>, from: &[u8]) -> Vec<u8> {
    for _ in 0..1000 {
        h.run_steps(1);
        let now = select_pixels(h);
        if now != from {
            return now;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    panic!("the layer did not change");
}

fn bb31_values(extra: serde_json::Value) -> (EffectDef, Values) {
    let def = pdn_core::effect::find("bbTextFunFactory").unwrap();
    let mut values = pdn_core::effect::defaults(&def);
    for (k, v) in [
        ("text", serde_json::json!("H")),
        ("font", serde_json::json!("Noto Sans Mono")),
        ("size", serde_json::json!(20)),
        ("offsetX", serde_json::json!(0)),
    ] {
        values.insert(k.into(), v);
    }
    values.extend(extra.as_object().unwrap().clone());
    (def, values)
}

/// R158 in the real app: the live preview follows the inner Fill tab (the outer tab alone changes
/// nothing), Preview off restores the layer and on renders again; Cancel leaves the layer, history,
/// dirty flag and tokens as they were; an accepted render undoes and redoes to exact bytes.
#[test]
fn bb31_preview_fill_tab_cancel_and_undo_redo() {
    let mut h = bb12_harness();
    let before = select_pixels(&h);
    let history = undo_len(&h);
    let dirty = h.state().app.active_ws().unwrap().dirty;
    let (def, values) = bb31_values(serde_json::json!({}));
    open_def(&mut h, def.clone(), values.clone());
    let solid = bb31_until_changed(&mut h, &before);
    h.get_by_role_and_label(Role::Tab, "Fill").click();
    h.run_steps(5);
    assert_eq!(
        select_pixels(&h),
        solid,
        "the outer tab alone renders nothing new"
    );
    h.get_by_role_and_label(Role::Tab, "Gradient").click();
    let gradient = bb31_until_changed(&mut h, &solid);
    h.get_by_role_and_label(Role::CheckBox, "Preview").click();
    assert_eq!(bb31_until_changed(&mut h, &gradient), before, "Preview off");
    h.get_by_role_and_label(Role::CheckBox, "Preview").click();
    assert_eq!(bb31_until_changed(&mut h, &before), gradient, "Preview on");
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(select_pixels(&h), before);
    assert_eq!(undo_len(&h), history);
    assert_eq!(h.state().app.active_ws().unwrap().dirty, dirty);
    assert!(!h.state().app.effect_tokens.contains_key("bbTextFunFactory"));

    // Accept once; Undo and Redo give the exact bytes.
    open_def(&mut h, def, values);
    let shown = bb31_until_changed(&mut h, &before);
    let token = finish(&mut h, "bbTextFunFactory");
    assert_eq!(select_pixels(&h), shown, "OK keeps the previewed render");
    assert_eq!(undo_len(&h), history + 1);
    pdn_core::action::perform(&mut h.state_mut().app, "menu.edit.undo");
    h.run_steps(2);
    assert_eq!(select_pixels(&h), before);
    pdn_core::action::perform(&mut h.state_mut().app, "menu.edit.redo");
    h.run_steps(2);
    assert_eq!(select_pixels(&h), shown);
    assert_eq!(h.state().app.effect_tokens["bbTextFunFactory"], token);
}

/// R158 / R157: a saved family that no longer loads falls back for the session; reopening from
/// the remembered token, changing tabs and Cancel leave the token (font name, tab, fillTab, no
/// transient fallbackFont), the layer and the history untouched.
#[test]
fn bb31_cancel_keeps_the_remembered_token() {
    let mut h = bb12_harness();
    let (def, values) = bb31_values(serde_json::json!({"font": "bb31-gone", "tab": 1,
        "fillTab": 1}));
    open_def(&mut h, def, values);
    h.get_by_label("OK").click();
    bb29_until_closed(&mut h);
    let token = h.state().app.effect_tokens["bbTextFunFactory"].clone();
    assert_eq!(token["font"], serde_json::json!("bb31-gone"));
    assert_eq!(
        (token["tab"].clone(), token["fillTab"].clone()),
        (serde_json::json!(1), serde_json::json!(1))
    );
    assert!(!token.contains_key("fallbackFont") && !token.contains_key("fallbackFamily"));
    let (before, history) = (select_pixels(&h), undo_len(&h));
    open_effect(&mut h, "bbTextFunFactory");
    h.get_by_role_and_label(Role::Tab, "Image").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Tab, "Outline").click();
    h.run_steps(3);
    let _ = bb31_until_changed(&mut h, &before);
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert!(!h.state().shell.dialogs.is_open());
    assert_eq!(select_pixels(&h), before);
    assert_eq!(undo_len(&h), history);
    assert_eq!(h.state().app.effect_tokens["bbTextFunFactory"], token);
}

/// BB33 in the real app: with a remembered "Use Image on Clipboard", the Fill ▸ Image tab previews
/// the clipboard image at its own size and Solid the red fill; a new clipboard image is read on
/// the next change; OK commits it, and Repeat (Ctrl+F) reads the clipboard afresh, one history
/// entry each.
#[test]
fn bb33_text_fun_factory_clipboard_image_ok_and_repeat() {
    use pdn_core::canvas::ImageData;
    let mut h = bb12_harness();
    let tile = |w: u32, ht: u32, seed: u8| ImageData {
        width: w,
        height: ht,
        data: (0..(w * ht) as u8)
            .flat_map(|i| [seed + 30 * i, 200 - 20 * i, 90 + seed / 2, 255])
            .collect(),
    };
    let (a, b) = (tile(3, 2, 30), tile(2, 3, 60));
    let (def, values) = bb31_values(serde_json::json!({"texture": "t49", "size": 30,
        "depth": 0, "outlineSize": 0, "shadowLength": 0}));
    // The layer each state renders, from the untouched source (the core session's own render).
    let expect = |h: &mut Harness<'_, paintrs::PaintApp>, image: Option<&ImageData>, fill: u32| {
        let mut s =
            pdn_core::effect::session::EffectSession::begin(&mut h.state_mut().app, def.clone())
                .unwrap();
        s.set_image(image.map(|i| std::sync::Arc::new(i.clone())));
        let mut v = values.clone();
        v.insert("fillTab".into(), serde_json::json!(fill));
        s.render(&v).data
    };
    let solid = expect(&mut h, None, 0);
    let (with_a, with_b) = (expect(&mut h, Some(&a), 3), expect(&mut h, Some(&b), 3));
    assert!(solid != with_a && with_a != with_b);
    let before = select_pixels(&h);
    let history = undo_len(&h);
    h.state_mut().shell.clipboard = paintrs::platform::Clipboard::in_memory();
    h.state_mut().shell.clipboard.write_image(&a);
    open_def(&mut h, def.clone(), values.clone());
    assert_eq!(bb31_until_changed(&mut h, &before), solid);
    h.get_by_role_and_label(Role::Tab, "Fill").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Tab, "Image").click();
    assert_eq!(bb31_until_changed(&mut h, &solid), with_a);
    h.get_by_role_and_label(Role::Tab, "Solid").click();
    assert_eq!(bb31_until_changed(&mut h, &with_a), solid);
    h.state_mut().shell.clipboard.write_image(&b);
    h.get_by_role_and_label(Role::Tab, "Image").click();
    assert_eq!(bb31_until_changed(&mut h, &solid), with_b);
    let token = finish(&mut h, "bbTextFunFactory");
    assert_eq!(
        (token["fillTab"].clone(), token["texture"].clone()),
        (serde_json::json!(3), serde_json::json!("t49"))
    );
    assert_eq!(select_pixels(&h), with_b);
    assert_eq!(undo_len(&h), history + 1);
    h.state_mut().shell.clipboard.write_image(&a);
    let again = expect(&mut h, Some(&a), 3);
    h.key_press_modifiers(Modifiers::CTRL, Key::F);
    bb29_until_closed(&mut h);
    assert_eq!(select_pixels(&h), again);
    assert_eq!(undo_len(&h), history + 2);
    assert_eq!(h.state().app.effect_tokens["bbTextFunFactory"], token);
}

/// "Two controls appear on two tabs each": 3D Depth edited on Fill > Metallic reads back on the
/// 3D tab, Light direction edited on Shadow reads back on Fill > Metallic, and OK stores the
/// one value of each.
#[test]
fn bb31_shared_depth_and_light_rows() {
    let mut h = bb12_harness();
    let (def, values) = bb31_values(serde_json::json!({}));
    open_def(&mut h, def, values);
    h.get_by_role_and_label(Role::Tab, "Fill").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Tab, "Metallic").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Slider, "depth").focus();
    h.run_steps(2);
    for _ in 0..5 {
        press(&mut h, Key::PageUp);
    }
    h.get_by_role_and_label(Role::Tab, "3D").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Slider, "depth").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    assert_eq!(focused_node(&h), ("depth".into(), "30".into()));
    h.get_by_role_and_label(Role::Tab, "Shadow").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Slider, "light").focus();
    h.run_steps(2);
    for _ in 0..3 {
        press(&mut h, Key::PageDown);
    }
    h.get_by_role_and_label(Role::Tab, "Fill").click();
    h.run_steps(3);
    h.get_by_role_and_label(Role::Slider, "light").focus();
    h.run_steps(2);
    press(&mut h, Key::Tab);
    let (key, shown) = focused_node(&h);
    assert_eq!(
        (key.as_str(), shown.parse::<f64>().unwrap()),
        ("light", -60.0)
    );
    let token = finish(&mut h, "bbTextFunFactory");
    assert_eq!(
        (token["depth"].clone(), token["light"].clone()),
        (serde_json::json!(30.0), serde_json::json!(-60.0))
    );
}
