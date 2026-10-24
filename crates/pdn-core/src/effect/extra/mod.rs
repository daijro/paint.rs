//! Photocraft phase spec: S§4.1 (provenance, the plus indicator), S§4.2 (vendored algorithm code)
//!
//! The Extra effects: features paint.rs adds beyond Paint.NET, ported from PhotoCraft
//! (`crate::extra`). Each is an ordinary `EffectDef` with a manifest row; the menus mark it with
//! the plus.

pub mod adjust;
pub mod blur;
pub mod blur_gallery;
pub mod camera_raw;
pub mod displace_custom;
pub mod distort;
pub mod flame;
pub mod gallery;
pub mod halftone;
pub mod lens;
pub mod liquify;
pub mod manifest;
pub mod matting;
pub mod noise;
pub mod object;
pub mod photo;
pub mod relight_render;
pub mod render;
pub mod stylize;
pub mod tone;
pub mod vanishing;
pub mod warp;

use super::EffectDef;

/// The Extra effects in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let mut effects = stylize::effects();
    effects.extend(photo::effects());
    effects.extend(adjust::effects());
    effects.extend(gallery::effects());
    effects.extend(tone::effects());
    effects.extend(displace_custom::effects());
    effects.extend(relight_render::effects());
    effects.extend(distort::effects());
    effects.extend(camera_raw::effects());
    effects.extend(matting::effects());
    effects.extend(noise::effects());
    effects.extend(object::effects());
    effects.extend(blur::effects());
    effects.extend(blur_gallery::effects());
    effects.extend(halftone::effects());
    effects.extend(lens::effects());
    effects.extend(liquify::effects());
    effects.extend(render::effects());
    effects.extend(vanishing::effects());
    effects.extend(flame::effects());
    effects.extend(warp::effects());
    effects.sort_by_key(|def| {
        manifest::MANIFEST
            .iter()
            .position(|m| m.id == def.id)
            .expect("registered effect has a manifest row")
    });
    effects
}

#[cfg(test)]
mod tests {
    use super::manifest::{MANIFEST, manifest};
    use super::*;
    use crate::effect::bb::indirect_defaults;
    use crate::effect::bb::test_util::{ellipse_selection, rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, solid};
    use crate::effect::{
        EffectEnv, Provenance, apply, defaults, display_name_of, find, provenance_of,
    };

    /// Solarize: a channel c > 127 becomes 255 - c; alpha untouched (PC14:crates/algo/src/
    /// stylize.rs `solarize`).
    #[test]
    fn solarize_inverts_only_above_half() {
        let def = find("pcSolarize").unwrap();
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [100, 200, 128, 77]
            } else {
                [255, 0, 127, 255]
            }
        });
        let out = apply(&src, &def, &defaults(&def), &EffectEnv::whole(2, 1));
        assert_eq!(px(&out, 0, 0), [100, 55, 127, 77]);
        assert_eq!(px(&out, 1, 0), [0, 0, 127, 255]);
    }

    /// Defringe: a half-transparent blue edge pixel next to a red core takes the core's red;
    /// its alpha stays.
    #[test]
    fn defringe_replaces_edge_colour() {
        let def = find("pcDefringe").unwrap();
        let src = from_fn(5, 1, |x, _| match x {
            0 => [0, 0, 0, 0],
            1 => [0, 0, 255, 128],
            _ => [255, 0, 0, 255],
        });
        let out = apply(&src, &def, &defaults(&def), &EffectEnv::whole(5, 1));
        assert_eq!(px(&out, 1, 0), [255, 0, 0, 128]);
        assert_eq!(px(&out, 0, 0), [0, 0, 0, 0]);
        assert_eq!(px(&out, 3, 0), [255, 0, 0, 255]);
    }

    /// Black matte: colour over black divides by alpha; white matte undoes (c - (1 - a)) / a;
    /// opaque and clear pixels are untouched.
    #[test]
    fn remove_black_matte_unmultiplies() {
        let src = from_fn(3, 1, |x, _| match x {
            0 => [64, 32, 0, 128],
            1 => [10, 20, 30, 255],
            _ => [9, 9, 9, 0],
        });
        let env = EffectEnv::whole(3, 1);
        let def = find("pcRemoveBlackMatte").unwrap();
        let out = apply(&src, &def, &defaults(&def), &env);
        assert_eq!(px(&out, 0, 0), [128, 64, 0, 128]);
        assert_eq!(px(&out, 1, 0), [10, 20, 30, 255]);
        assert_eq!(px(&out, 2, 0), [9, 9, 9, 0]);
        let white = find("pcRemoveWhiteMatte").unwrap();
        let out = apply(
            &solid(1, 1, [255, 127, 127, 128]),
            &white,
            &defaults(&white),
            &EffectEnv::whole(1, 1),
        );
        assert_eq!(px(&out, 0, 0), [255, 0, 0, 128]);
    }

    const Q: crate::pixel::mask::SelectionQuality =
        crate::pixel::mask::SelectionQuality::HighQuality;

    fn faded_app() -> (crate::app::AppWorkspace, Vec<u8>, Vec<u8>) {
        use crate::testing::{fresh_doc, layer_bytes};
        let mut app = fresh_doc(8, 6, Some(4));
        let before = layer_bytes(&app, None);
        assert!(!crate::action::is_executable(&app, "menu.edit.fade"));
        assert!(crate::effect::session::run_instant(
            &mut app,
            find("invertColorsEffect").unwrap()
        ));
        let after = layer_bytes(&app, None);
        assert_ne!(before, after);
        (app, before, after)
    }

    /// R90: Edit ▸ Fade opens a dialog, so its menu label ends in "...", as Paint.NET's dialog
    /// commands' do (its dialog title and history names keep the bare name).
    #[test]
    fn fade_label_has_the_dialog_ellipsis() {
        let app = crate::app::AppWorkspace::default();
        assert_eq!(crate::action::menu_label(&app, "menu.edit.fade"), "Fade...");
    }

    /// Fade at 100 % Normal keeps the effect; it is still one new history item named for it, and
    /// undoing it gives the effect's pixels back.
    #[test]
    fn fade_100_is_noop() {
        use crate::document::layer::BlendMode;
        use crate::testing::layer_bytes;
        let (mut app, _, after) = faded_app();
        assert!(crate::action::is_executable(&app, "menu.edit.fade"));
        let ws = app.active_ws_mut().unwrap();
        let len = ws.history.undo.len();
        assert!(ws.fade(100.0, BlendMode::Normal, Q));
        assert_eq!(ws.history.undo.len(), len + 1);
        assert_eq!(ws.history.undo.last().unwrap().name, "Fade Invert Colors");
        assert!(!ws.can_fade(), "a Fade cannot be faded");
        assert_eq!(layer_bytes(&app, None), after);
        app.active_ws_mut().unwrap().undo();
        assert_eq!(layer_bytes(&app, None), after);
    }

    /// Fade at 0 % restores the before-pixels; undo returns the effect, and a second undo the
    /// before-pixels.
    #[test]
    fn fade_0_restores_before() {
        use crate::document::layer::BlendMode;
        use crate::testing::layer_bytes;
        let (mut app, before, after) = faded_app();
        assert!(
            app.active_ws_mut()
                .unwrap()
                .fade(0.0, BlendMode::Multiply, Q)
        );
        assert_eq!(layer_bytes(&app, None), before);
        app.active_ws_mut().unwrap().undo();
        assert_eq!(layer_bytes(&app, None), after);
        // The effect is the newest item again, so it can be faded halfway: between the two.
        assert!(
            app.active_ws_mut()
                .unwrap()
                .fade(50.0, BlendMode::Normal, Q)
        );
        let mid = layer_bytes(&app, None);
        for i in 0..mid.len() {
            let (lo, hi) = (before[i].min(after[i]), before[i].max(after[i]));
            assert!((lo..=hi).contains(&mid[i]), "byte {i}");
        }
        app.active_ws_mut().unwrap().undo();
        app.active_ws_mut().unwrap().undo();
        assert_eq!(layer_bytes(&app, None), before);
    }

    /// Anything but an effect or adjustment as the newest item (a fill, here a Bitmap entry
    /// too) leaves Fade disabled.
    #[test]
    fn fade_disabled_after_a_tool_stroke() {
        use crate::action::{is_executable, perform};
        let (mut app, ..) = faded_app();
        perform(&mut app, "menu.edit.selectAll");
        perform(&mut app, "menu.edit.fillSelection");
        assert!(!is_executable(&app, "menu.edit.fade"));
        assert!(!app.active_ws_mut().unwrap().fade(
            100.0,
            crate::document::layer::BlendMode::Normal,
            Q
        ));
        // Undoing back to the effect re-enables it.
        app.active_ws_mut().unwrap().undo();
        app.active_ws_mut().unwrap().undo();
        assert!(is_executable(&app, "menu.edit.fade"));
    }

    /// Fade 50 % Difference after an effect inside a selection: nothing outside the selection
    /// changes (compose(Difference, x, x) is not x).
    #[test]
    fn fade_respects_selection() {
        use crate::document::layer::BlendMode;
        use crate::testing::{Ev, drive_tool, fresh_doc, layer_bytes};
        use crate::tool::types::ToolType;
        use crate::util::shortcut::MouseButton::Left;
        let mut app = fresh_doc(16, 12, Some(4));
        drive_tool(
            &mut app,
            ToolType::RectangleSelect,
            &[
                Ev::Down(4.0, 3.0, Left),
                Ev::Move(10.0, 8.0),
                Ev::Wait(60.0),
                Ev::Up(10.0, 8.0, Left),
            ],
        );
        let original = layer_bytes(&app, None);
        let sel = crate::testing::selection_mask(&app, 16, 12);
        assert!(sel.contains(&0) && sel.contains(&255));
        assert!(crate::effect::session::run_instant(
            &mut app,
            find("invertColorsEffect").unwrap()
        ));
        let after = layer_bytes(&app, None);
        assert_ne!(original, after);
        assert!(
            app.active_ws_mut()
                .unwrap()
                .fade(50.0, BlendMode::Difference, Q)
        );
        let faded = layer_bytes(&app, None);
        let inside = |x: usize, y: usize| sel[y * 16 + x] != 0;
        let mut changed_inside = false;
        for y in 0..12 {
            for x in 0..16 {
                let i = (y * 16 + x) * 4;
                if inside(x, y) {
                    changed_inside |= faded[i..i + 4] != after[i..i + 4];
                } else {
                    assert_eq!(faded[i..i + 4], after[i..i + 4], "({x}, {y})");
                }
            }
        }
        assert!(changed_inside);
    }

    /// Fade blends premultiplied: a 50 % fade of transparent -> opaque c is ~ [c, c, c, 128],
    /// whatever colour the transparent pixel hid.
    #[test]
    fn fade_blends_premultiplied() {
        use crate::document::layer::BlendMode;
        use crate::testing::layer_bytes;
        let mut app = crate::testing::fresh_doc(2, 1, None);
        {
            let ws = app.active_ws_mut().unwrap();
            let s = &mut ws.document.layers.get_at_mut(0).unwrap().surface;
            s.set_pixel(0, 0, [200, 0, 0, 0]);
            s.set_pixel(1, 0, [0, 0, 0, 0]);
        }
        let def = crate::effect::EffectDef {
            kind: crate::effect::EffectKind::Cpu {
                render: |src, _, _| crate::canvas::ImageData {
                    width: src.width,
                    height: src.height,
                    data: [90, 90, 90, 255].repeat(src.width as usize),
                },
                clip_to_selection: true,
            },
            ..find("pcSolarize").unwrap()
        };
        assert!(crate::effect::session::run_instant(&mut app, def));
        assert!(
            app.active_ws_mut()
                .unwrap()
                .fade(50.0, BlendMode::Normal, Q)
        );
        let out = layer_bytes(&app, None);
        for p in out.chunks(4) {
            assert!(p[3].abs_diff(128) <= 1, "{p:?}");
            assert!(p[..3].iter().all(|c| c.abs_diff(90) <= 1), "{p:?}");
        }
    }

    /// R14: registered ids are unique and cover every manifest row (names no module).
    #[test]
    fn every_manifest_row_is_registered_exactly_once() {
        let ids: Vec<_> = effects().iter().map(|d| d.id).collect();
        let mut seen = std::collections::HashSet::new();
        for id in &ids {
            assert!(seen.insert(*id), "duplicate registered id {id}");
        }
        for m in MANIFEST {
            assert!(seen.contains(m.id), "{} is not registered", m.id);
        }
        assert_eq!(ids.len(), MANIFEST.len());
    }

    /// The registry rule every later effect task obeys (Effect recipe steps 4–5).
    #[test]
    fn every_registered_extra_matches_its_manifest_row() {
        let definitions = effects();
        let ids: Vec<_> = definitions.iter().map(|def| def.id).collect();
        let ordered: Vec<_> = MANIFEST
            .iter()
            .filter(|m| ids.contains(&m.id))
            .map(|m| m.id)
            .collect();
        // This also rejects unknown or duplicate definitions rather than silently dropping them.
        assert_eq!(ids, ordered);
        let svgs = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/icons-svg/photocraft");
        for def in definitions {
            let m = manifest(def.id).unwrap_or_else(|| panic!("{} has no manifest row", def.id));
            assert!(def.id.starts_with("pc"), "{}", def.id);
            assert_eq!(def.category, m.category, "{}", def.id);
            assert_eq!(def.icon, m.icon, "{}", def.id);
            assert_eq!(def.is_configurable(), m.configurable, "{}", def.id);
            assert_eq!(def.dialog_title.unwrap_or(m.name), m.name, "{}", def.id);
            let stem = m.icon.strip_suffix(".png").unwrap();
            assert!(svgs.join(format!("{stem}.svg")).is_file(), "{}", m.icon);
            assert_eq!(provenance_of(def.id), Provenance::Extra);
            assert_eq!(display_name_of(def.id), m.name);
            assert_eq!(find(def.id).map(|d| d.id), Some(def.id));
        }
    }

    /// Review Focus 2: degenerate layers and 1-px selections never panic.
    #[test]
    fn every_extra_effect_survives_tiny_layers() {
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
            let (mask, polys) = rect_selection(5, 5, 2, 2, 3, 3);
            let env = with_selection(5, 5, &mask, &polys);
            let out = apply(
                &solid(5, 5, [90, 140, 200, 255]),
                &def,
                &indirect_defaults(&def, &env),
                &env,
            );
            assert_eq!(out.data.len(), 100, "{}", def.id);
        }
    }

    /// A selection wholly outside the layer leaves `env.bounds` empty: every Extra effect
    /// returns the layer instead of panicking.
    #[test]
    fn every_extra_effect_survives_empty_bounds() {
        let src = from_fn(9, 7, |x, y| [(x * 20) as u8, (y * 30) as u8, 90, 255]);
        for def in effects() {
            let mut env = EffectEnv::whole(9, 7);
            env.bounds = crate::pixel::IntRect::new(0, 0, 0, 0);
            let out = apply(&src, &def, &indirect_defaults(&def, &env), &env);
            assert_eq!((out.width, out.height), (9, 7), "{}", def.id);
        }
    }

    /// Fully transparent layers render deterministically.
    #[test]
    fn every_extra_effect_is_deterministic_on_transparent_layers() {
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

    /// Review Focus 2: a 21×21 ellipse selection on a noise image; every pixel outside the mask
    /// is unchanged.
    #[test]
    fn every_extra_effect_stays_inside_an_elliptical_selection() {
        let noise = from_fn(21, 21, |x, y| {
            let mut state = u64::from(y * 21 + x);
            let b = crate::extra::rng::splitmix64(&mut state).to_le_bytes();
            [b[0], b[1], b[2], b[3]]
        });
        let (mask, polys) = ellipse_selection(21, 21, [10.5, 10.5], [8.0, 6.0]);
        let env = with_selection(21, 21, &mask, &polys);
        for def in effects() {
            let out = apply(&noise, &def, &indirect_defaults(&def, &env), &env);
            for (x, y) in (0..21).flat_map(|y| (0..21).map(move |x| (x, y))) {
                if mask.get(x as i32, y as i32) == 0 {
                    assert_eq!(px(&out, x, y), px(&noise, x, y), "{} ({x}, {y})", def.id);
                }
            }
        }
    }

    /// S§4.1: every effect has exactly one of the three kinds.
    #[test]
    fn provenance_kinds() {
        assert_eq!(provenance_of("embossEffect"), Provenance::PaintNet);
        assert_eq!(provenance_of("bbDream"), Provenance::Plugin);
        assert_eq!(provenance_of("pcSolarize"), Provenance::Extra);
    }

    /// Extra items are actions with literal labels, like plugins; so are the four new submenus.
    #[test]
    fn extra_actions_resolve() {
        use crate::action::{extra_item, get, menu_label, plugin_item};
        let a = get("menu.effects.pcSolarize").unwrap();
        assert_eq!(
            (a.label, a.icon, a.category),
            (Some("Solarize"), Some("solarize_effect.png"), "Effects")
        );
        assert_eq!(
            extra_item("menu.effects.pcSolarize").unwrap().name,
            "Solarize"
        );
        assert!(plugin_item("menu.effects.pcSolarize").is_none());
        assert!(extra_item("menu.effects.bbDream").is_none());
        assert!(extra_item("menu.effects.embossEffect").is_none());
        let app = crate::app::AppWorkspace::default();
        assert_eq!(menu_label(&app, "menu.effects.pcSolarize"), "Solarize");
        for (id, label) in [
            ("effects.brushStrokes.submenu", "Brush Strokes"),
            ("effects.sketch.submenu", "Sketch"),
            ("effects.texture.submenu", "Texture"),
        ] {
            assert_eq!(get(id).unwrap().label, Some(label));
        }
        // Extra-only submenus appear only once a member is registered.
        let keys: Vec<_> = crate::effect::menu_categories(true, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        let used: Vec<_> = effects().iter().map(|d| d.category).collect();
        for key in ["brushStrokes", "sketch", "texture"] {
            let member = used.contains(&crate::effect::Category::Effect(key));
            assert_eq!(keys.contains(&key), member, "{key}");
        }
    }
}
