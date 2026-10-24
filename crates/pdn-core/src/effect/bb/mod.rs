//! BB spec: README.md "Menu tree", "Flavor, DLL and version", "OS and Paint.NET service
//! dependencies"; common.md §2, §3, §5.11
//!
//! The BB plugin pack (47 menu items) as ordinary `EffectDef`s: the manifest, the IndirectUI
//! presentation model, the named conventions and the value accessors the effects share.

pub mod adjust;
pub mod bevel;
pub mod blocks;
pub mod blurs;
pub mod calendar;
pub mod channels;
pub mod chart;
pub mod classic;
pub mod classic_fx;
pub mod conventions;
pub mod dither;
pub mod feather;
pub mod fill;
pub mod generators;
pub mod labels;
pub mod manifest;
pub mod meme;
pub mod outlines;
pub mod painterly;
pub mod photo_fx;
pub mod pixel_moves;
pub mod raster;
pub mod render_fx;
pub mod seam;
pub mod shadows;
pub mod staircase;
#[cfg(test)]
pub mod test_util;
pub mod text;
pub mod text_fun;
pub mod textures;
pub mod ui;
pub mod vignettes;

use super::{EffectDef, EffectEnv, Values, choice, color_value, defaults, num};
use crate::pixel::Px;
use serde_json::Value;

/// The plugin effects in manifest order, including families that span several categories.
pub fn effects() -> Vec<EffectDef> {
    let mut effects = adjust::effects();
    effects.extend(channels::effects());
    effects.extend(pixel_moves::effects());
    effects.extend(generators::effects());
    effects.extend(blurs::effects());
    effects.extend(shadows::effects());
    effects.extend(outlines::effects());
    effects.extend(feather::effects());
    effects.extend(bevel::effects());
    effects.extend(vignettes::effects());
    effects.extend(photo_fx::effects());
    effects.extend(painterly::effects());
    effects.extend(classic_fx::effects());
    effects.extend(seam::effects());
    effects.extend(dither::effects());
    effects.extend(render_fx::effects());
    effects.extend(calendar::effects());
    effects.extend(fill::effects());
    effects.extend(labels::effects());
    effects.extend(meme::effects());
    effects.extend(text_fun::effects());
    effects.extend(chart::effects());
    effects.sort_by_key(|def| {
        manifest::MANIFEST
            .iter()
            .position(|m| m.id == def.id)
            .expect("registered effect has a manifest row")
    });
    effects
}

/// The static defaults, then the def's dynamic ones (`IndirectUi::defaults`) for `env`.
pub fn indirect_defaults(def: &EffectDef, env: &EffectEnv) -> Values {
    let mut v = defaults(def);
    if let Some(ui) = def.indirect {
        (ui.defaults)(&mut v, env);
    }
    v
}

/// common.md §1 "#RRGGBBAA": `"#rrggbbaa"` keeps its alpha, `"#rrggbb"` is opaque, and
/// `"primary"`/`"secondary"` keep the palette colour's alpha.
pub fn color_value_alpha(values: &Values, key: &str, env: &EffectEnv) -> Px {
    let mut c = color_value(values, key, env);
    if let Some(a) = choice(values, key)
        .get(7..9)
        .and_then(|s| u8::from_str_radix(s, 16).ok())
    {
        c[3] = a;
    }
    c
}

/// A pan control's `[x, y]` (0 for a missing coordinate).
pub fn pan(values: &Values, key: &str) -> [f64; 2] {
    let a = values.get(key).and_then(Value::as_array);
    std::array::from_fn(|i| a.and_then(|a| a.get(i)?.as_f64()).unwrap_or(0.0))
}

/// A text box's string.
pub fn text<'a>(values: &'a Values, key: &str) -> &'a str {
    choice(values, key)
}

/// A layer drop-down's index (bottom = 0), or any other non-negative integer value (`as`
/// saturates negatives to 0).
pub fn index(values: &Values, key: &str) -> usize {
    num(values, key) as usize
}

#[cfg(test)]
mod tests {
    use super::manifest::{MANIFEST, manifest};
    use super::ui::{IndirectUi, no_defaults};
    use super::*;
    use crate::effect::rng::CivilDate;
    use crate::effect::session::EffectSession;
    use crate::effect::surface::{Alpha, Space};
    use crate::effect::test_util::solid;
    use crate::effect::{
        Category, Control, EffectEnv, EffectKind, LabelText, PanThumb, RenderInfo, apply,
    };
    use serde_json::json;

    /// README "Menu tree" and "Flavor, DLL and version": 47 items, 3 without a dialog.
    #[test]
    fn manifest_matches_the_readme() {
        assert_eq!(MANIFEST.len(), 47);
        let mut ids: Vec<_> = MANIFEST.iter().map(|m| m.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 47);
        let count = |c: Category| MANIFEST.iter().filter(|m| m.category == c).count();
        assert_eq!(count(Category::Adjustment), 5);
        for (key, n) in [
            ("artistic", 3),
            ("blurring", 1),
            ("color", 1),
            ("fill", 2),
            ("", 2),
            ("object", 9),
            ("photo", 9),
            ("render", 7),
            ("selection", 5),
            ("stylize", 2),
            ("textFormations", 1),
        ] {
            assert_eq!(count(Category::Effect(key)), n, "{key}");
        }
        let plain: Vec<_> = MANIFEST
            .iter()
            .filter(|m| !m.configurable)
            .map(|m| m.name)
            .collect();
        assert_eq!(
            plain,
            ["Flip Horizontal", "Flip Vertical", "Blur Selection Edge"]
        );
        let m = manifest("bbHueSaturationPlus").unwrap();
        assert_eq!(m.name, "Hue / Saturation+");
        assert_eq!(
            manifest("bbSeamCarvingHorizontal").unwrap().icon,
            "seam_carving__seam_carving_h.png"
        );
    }

    fn probe(
        id: &'static str,
        controls: Vec<Control>,
        indirect: Option<&'static IndirectUi>,
    ) -> crate::effect::EffectDef {
        crate::effect::EffectDef {
            id,
            icon: "",
            category: Category::Effect("render"),
            controls,
            kind: EffectKind::Gpu(RenderInfo::new(Space::Srgb, Alpha::Straight), |s, _, _| {
                s.clone()
            }),
            dialog: None,
            dialog_title: None,
            indirect,
        }
    }

    fn radius_from_size(v: &mut crate::effect::Values, env: &EffectEnv<'_>) {
        v.insert(
            "radius".into(),
            json!((env.width.max(env.height) / 80).clamp(10, 200)),
        );
    }
    static PROBE_UI: IndirectUi = IndirectUi {
        defaults: radius_from_size,
        ..IndirectUi::DEFAULT
    };

    /// common.md §3: text, label, layer list and pan values; dynamic defaults at dialog open.
    #[test]
    fn new_control_kinds_and_dynamic_defaults() {
        let d = probe(
            "bbDream",
            vec![
                Control::Text {
                    key: "label",
                    label: "Label",
                    default: "hi",
                    max_len: 255,
                    multiline: false,
                },
                Control::Label {
                    key: "info",
                    label: "",
                    text: LabelText::CurrentLayer("Current layer    "),
                },
                Control::LayerSelect {
                    key: "mask",
                    label: "",
                },
                Control::Pan {
                    key: "at",
                    label: "",
                    min: -1.0,
                    max: 1.0,
                    default: [-0.9, 0.0],
                    step: 0.01,
                    thumb: PanThumb::SelectionCrop,
                },
                crate::effect::range("radius", "Radius", 0.0, 200.0, 10.0),
            ],
            Some(&PROBE_UI),
        );
        let env = EffectEnv::whole(4000, 3000);
        let v = indirect_defaults(&d, &env);
        assert_eq!(
            (text(&v, "label"), index(&v, "mask"), pan(&v, "at")),
            ("hi", 0, [-0.9, 0.0])
        );
        assert_eq!(v["info"], serde_json::Value::Null);
        assert_eq!(v["radius"], json!(50));
        assert_eq!(d.display_name(), "Dream");
        let _ = no_defaults;
    }

    #[test]
    fn colour_values_keep_alpha_when_asked() {
        let mut env = EffectEnv::whole(1, 1);
        env.primary = [1, 2, 3, 128];
        let v = crate::effect::with(
            &probe("p", vec![], None),
            &json!({"a": "#11223344", "b": "#112233", "c": "primary"}),
        );
        assert_eq!(color_value_alpha(&v, "a", &env), [0x11, 0x22, 0x33, 0x44]);
        assert_eq!(color_value_alpha(&v, "b", &env), [0x11, 0x22, 0x33, 255]);
        assert_eq!(color_value_alpha(&v, "c", &env), [1, 2, 3, 128]);
    }

    #[test]
    fn civil_dates() {
        assert_eq!(
            CivilDate::from_unix_days(0),
            CivilDate {
                year: 1970,
                month: 1,
                day: 1,
                day_of_year: 1
            }
        );
        assert_eq!(
            CivilDate::from_unix_days(20485),
            CivilDate {
                year: 2026,
                month: 2,
                day: 1,
                day_of_year: 32
            }
        );
        assert_eq!(
            CivilDate::from_unix_days(20088),
            CivilDate {
                year: 2024,
                month: 12,
                day: 31,
                day_of_year: 366
            }
        );
    }

    /// common.md §5.11: the session hands the effect the palette, the selection outline, the layers
    /// (only for layer-reading effects) and the date.
    #[test]
    fn session_env_carries_palette_selection_layers_and_today() {
        use crate::selection::CombineMode;
        use crate::util::geom::Rectangle;
        let mut app = crate::testing::fresh_doc(8, 6, Some(4));
        app.date_override = Some(CivilDate::new(2026, 2, 1));
        let s = EffectSession::begin(&mut app, probe("p", vec![], Some(&PROBE_UI))).unwrap();
        let env = s.env();
        assert_eq!((env.palette.len(), env.palette[2]), (96, [255, 0, 0, 255]));
        assert!(env.selection.is_empty() && env.layers.is_empty());
        assert_eq!(env.today, CivilDate::new(2026, 2, 1));
        assert_eq!(s.defaults()["radius"], json!(10));
        assert_eq!(s.initial_values(&app)["radius"], json!(10));
        let ws = app.active_ws_mut().unwrap();
        ws.selection
            .set_continuation(&Rectangle::new(1.0, 1.0, 3.0, 2.0), CombineMode::Replace);
        ws.selection.commit_continuation();
        let s = EffectSession::begin(
            &mut app,
            probe(
                "p",
                vec![Control::LayerSelect {
                    key: "m",
                    label: "",
                }],
                None,
            ),
        )
        .unwrap();
        assert_eq!(s.env().selection.len(), 1);
        assert_eq!(
            s.env().layers.len(),
            app.active_ws().unwrap().document.layers.size()
        );
    }

    /// The accessors' fallbacks, the default env's palette and date, and a session over a
    /// two-layer document (common.md §5.11 "Layer list": names always, pixels for a layer
    /// drop-down only).
    #[test]
    fn accessor_fallbacks_and_session_layers() {
        let env = EffectEnv::whole(1, 1);
        assert_eq!(env.palette[95], [0x7F, 0x00, 0x37, 0x80]);
        assert_eq!(env.today, CivilDate::new(2026, 2, 1));
        let v: crate::effect::Values = [("n".to_string(), json!(-3.0))].into_iter().collect();
        assert_eq!(
            (pan(&v, "none"), text(&v, "none"), index(&v, "n")),
            ([0.0, 0.0], "", 0)
        );
        let mut app = crate::testing::fresh_doc(8, 6, Some(4));
        crate::action::perform(&mut app, "menu.layers.add.new.layer");
        let ws = app.active_ws().unwrap();
        let names: Vec<String> = ws
            .document
            .layers
            .iter()
            .map(|l| l.properties.name.clone())
            .collect();
        let bottom = ws
            .document
            .layers
            .get_at(0)
            .unwrap()
            .surface
            .data()
            .to_vec();
        assert_eq!((ws.active_layer, names.len()), (1, 2));
        app.current_palette = vec![[1, 2, 3, 4]; 96];
        let s = EffectSession::begin(&mut app, probe("p", vec![], None)).unwrap();
        assert_eq!((s.layer_names(), s.env().layers.len()), (names.clone(), 0));
        assert_eq!(s.env().palette[95], [1, 2, 3, 4]);
        let select = vec![Control::LayerSelect {
            key: "m",
            label: "",
        }];
        let s = EffectSession::begin(&mut app, probe("p", select, None)).unwrap();
        let env = s.env();
        assert_eq!(
            (env.layer_index, env.layers[0].pixels.data.clone()),
            (1, bottom)
        );
        let layer_names: Vec<_> = env.layers.iter().map(|l| l.name.clone()).collect();
        assert_eq!(layer_names, names);
    }

    /// The registry rule every later task obeys.
    #[test]
    fn every_registered_effect_matches_its_manifest_row() {
        let definitions = effects();
        let ids: Vec<_> = definitions.iter().map(|def| def.id).collect();
        let ordered: Vec<_> = MANIFEST
            .iter()
            .filter(|m| ids.contains(&m.id))
            .map(|m| m.id)
            .collect();
        // This also rejects unknown or duplicate definitions rather than silently dropping them.
        assert_eq!(ids, ordered);
        for def in definitions {
            let m = manifest(def.id).unwrap_or_else(|| panic!("{} has no manifest row", def.id));
            assert_eq!(def.category, m.category, "{}", def.id);
            assert_eq!(def.icon, m.icon, "{}", def.id);
            assert_eq!(def.is_configurable(), m.configurable, "{}", def.id);
            assert!(
                !m.configurable || def.indirect.is_some() || def.dialog.is_some(),
                "{} needs its IndirectUi or its own dialog",
                def.id
            );
            assert!(crate::effect::find(def.id).is_some());
            assert_eq!(indirect_key_error(&def), None);
        }
    }

    /// The first IndirectUI key that names none of `def`'s controls, or a tab strip whose key is
    /// not a `Control::Value`: a typo in a table would silently mis-tab or drop a `ControlUi`.
    fn indirect_key_error(def: &crate::effect::EffectDef) -> Option<String> {
        let ui = def.indirect?;
        let control = |k: &str| def.controls.iter().find(|c| c.key() == k);
        let tabbed = ui.tabs.iter().flat_map(|s| {
            std::iter::once(s.key).chain(s.tabs.iter().flat_map(|t| t.keys.iter().copied()))
        });
        let mut keys = ui
            .controls
            .iter()
            .map(|(k, _)| *k)
            .chain(tabbed)
            .chain(ui.transient_keys.iter().copied());
        if let Some(k) = keys.find(|k| control(k).is_none()) {
            return Some(format!(
                "{}: IndirectUi names unknown control {k:?}",
                def.id
            ));
        }
        ui.tabs
            .iter()
            .find(|s| !matches!(control(s.key), Some(Control::Value { .. })))
            .map(|s| format!("{}: tab strip {:?} is not a Control::Value", def.id, s.key))
    }

    /// The registry sweep's key check fires on each kind of bad table and passes a good one.
    #[test]
    fn indirect_key_check_fires_on_bad_tables() {
        use super::ui::{ControlUi, Tab, TabStrip};
        // A table listing `$c` in `controls`, one tab strip at `$strip` with one tab holding `$tab`.
        macro_rules! ui {
            ($c:literal, $strip:literal, $tab:literal) => {
                IndirectUi {
                    controls: &[($c, ControlUi::NO_HEADER)],
                    tabs: &[TabStrip {
                        key: $strip,
                        tabs: &[Tab {
                            caption: "Tab\u{a0}",
                            keys: &[$tab],
                        }],
                    }],
                    ..IndirectUi::DEFAULT
                }
            };
        }
        static GOOD: IndirectUi = ui!("radius", "tab", "radius");
        static TYPO: IndirectUi = ui!("raduis", "tab", "radius");
        static TAB_TYPO: IndirectUi = ui!("radius", "tab", "radus");
        static STRIP_TYPO: IndirectUi = ui!("radius", "tabs", "radius");
        static STRIP_NOT_VALUE: IndirectUi = ui!("radius", "radius", "radius");
        let def = |ui| {
            let tab = Control::Value {
                key: "tab",
                default: || json!(0),
            };
            let radius = crate::effect::range("radius", "Radius", 0.0, 9.0, 1.0);
            probe("p", vec![radius, tab], Some(ui))
        };
        assert_eq!(indirect_key_error(&def(&GOOD)), None);
        for (ui, want) in [
            (&TYPO, "p: IndirectUi names unknown control \"raduis\""),
            (&TAB_TYPO, "p: IndirectUi names unknown control \"radus\""),
            (&STRIP_TYPO, "p: IndirectUi names unknown control \"tabs\""),
            (
                &STRIP_NOT_VALUE,
                "p: tab strip \"radius\" is not a Control::Value",
            ),
        ] {
            assert_eq!(indirect_key_error(&def(ui)).as_deref(), Some(want));
        }
    }

    /// Review Focus 2: degenerate layers and 1-px selections never panic.
    #[test]
    fn every_plugin_effect_survives_tiny_layers() {
        for def in effects() {
            for (w, h) in [(1, 1), (1, 7), (7, 1)] {
                let env = EffectEnv::whole(w, h);
                let out = apply(
                    &solid(w, h, [90, 140, 200, 255]),
                    &def,
                    &indirect_defaults(&def, &env),
                    &env,
                );
                assert_eq!((out.width, out.height), (w, h), "{} {w}x{h}", def.id);
            }
            let (mask, polys) = test_util::rect_selection(5, 5, 2, 2, 3, 3);
            let env = test_util::with_selection(5, 5, &mask, &polys);
            let out = apply(
                &solid(5, 5, [90, 140, 200, 255]),
                &def,
                &indirect_defaults(&def, &env),
                &env,
            );
            assert_eq!(out.data.len(), 100, "{}", def.id);
        }
    }

    /// Review Focus 5: fully transparent layers render deterministically.
    #[test]
    fn every_plugin_effect_is_deterministic_on_transparent_layers() {
        for def in effects() {
            let img = solid(9, 9, [0, 0, 0, 0]);
            let env = EffectEnv::whole(9, 9);
            let v = indirect_defaults(&def, &env);
            assert_eq!(
                apply(&img, &def, &v, &env).data,
                apply(&img, &def, &v, &env).data,
                "{}",
                def.id
            );
        }
    }
}
