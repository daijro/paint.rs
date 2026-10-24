//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; plan Task PC39
//!
//! Layers ▸ Puppet Warp and Perspective Warp (Extras; `extra::puppet`, `extra::perspective`).
//! Puppet Warp's own dialog (`paintrs::dialogs::warp`) records the pins, Perspective Warp's canvas
//! mode (`tool::perspective_warp`) the moved corners; the effect's value is that record, replayed
//! at full resolution (Effect Config's worker for Puppet Warp), so OK gives what the preview
//! showed. Both read and anchor on `warp_source` (the layer, or with a
//! selection its selected pixels lifted; anchored on their content) and are clipped to the
//! selection. As Layers effects (like Rotate/Zoom) Effects ▸ Repeat does not run them.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use crate::extra::perspective::{Plane, perspective_warp};
use crate::extra::puppet::{PuppetPin, PuppetWarp, puppet_warp};
use crate::extra::warp::Interp;
use serde::Deserialize;
use serde_json::{Value, json};

pub const PUPPET: &str = "pcPuppetWarp";
pub const PERSPECTIVE: &str = "pcPerspectiveWarp";

pub fn effects() -> Vec<EffectDef> {
    let def = |id, controls, render| {
        let m = manifest(id).expect("effect has a manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: m.category,
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(m.name),
            indirect: None,
        }
    };
    // No row draws these values; the warp dialog edits them.
    let value = |key, default| Control::Value { key, default };
    vec![
        def(
            PUPPET,
            vec![
                value("pins", || json!([])),
                value("mode", || json!("normal")),
                value("density", || json!("normal")),
                value("expansion", || json!(2.0)),
            ],
            render_puppet,
        ),
        def(
            PERSPECTIVE,
            vec![value("planes", || json!([]))],
            render_perspective,
        ),
    ]
}

/// The selection bounds ∩ the layer: where the warps write.
pub fn clip(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// What both warps (and their dialog's mesh and preview) read and anchor on. Ruling R83: with a
/// selection, the layer with its unselected pixels cleared (hard coverage, so an anti-aliased
/// edge pixel keeps its alpha): the warp lifts the selection, what it vacates turns transparent
/// and nothing from outside comes in; `None` without a selection (the layer itself, R71). I1:
/// the anchor is the bounds of that source's pixels with any alpha inside `env.bounds`, as
/// upstream anchors on `content_bounds()`.
pub fn warp_source(src: &ImageData, env: &EffectEnv) -> (Option<ImageData>, Rect) {
    let w = src.width as usize;
    let lifted = env.mask.map(|_| {
        let mut out = src.clone();
        for (i, p) in out.data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            if !env.selected((i % w) as u32, (i / w) as u32) {
                *p = [0; 4];
            }
        }
        out
    });
    let s = lifted.as_ref().unwrap_or(src);
    let b = clip(env).intersect(&Rect::new(0, 0, src.width as i32, src.height as i32));
    let mut content = Rect::EMPTY;
    for y in b.y0..b.y1 {
        let row =
            &s.data[(y as usize * w + b.x0 as usize) * 4..(y as usize * w + b.x1 as usize) * 4];
        let opaque = |p: &[u8; 4]| p[3] > 0;
        let px = row.as_chunks::<4>().0;
        if let (Some(a), Some(z)) = (px.iter().position(opaque), px.iter().rposition(opaque)) {
            let r = Rect::new(b.x0 + a as i32, y, b.x0 + z as i32 + 1, y + 1);
            content = content.union(&r);
        }
    }
    (lifted, content)
}

fn finite(p: &[f64; 2]) -> bool {
    p.iter().all(|v| v.is_finite())
}

/// The Puppet Warp value: malformed or non-finite pins are skipped, an unknown mode or density
/// is the default, the expansion is upstream's −200…200 px (2 when missing).
pub fn puppet(v: &Values) -> PuppetWarp {
    let pick = |key| v.get(key).cloned().unwrap_or(Value::Null);
    PuppetWarp {
        pins: v
            .get("pins")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|p| PuppetPin::deserialize(p).ok())
            .filter(|p| finite(&p.src) && finite(&p.dst))
            .collect(),
        mode: serde_json::from_value(pick("mode")).unwrap_or_default(),
        density: serde_json::from_value(pick("density")).unwrap_or_default(),
        expansion: v
            .get("expansion")
            .and_then(Value::as_f64)
            .filter(|e| e.is_finite())
            .map_or(2.0, |e| e.clamp(-200.0, 200.0)),
    }
}

/// The Perspective Warp value: the planes, or none when any is malformed or non-finite.
pub fn planes(v: &Values) -> Vec<Plane> {
    let planes = v
        .get("planes")
        .and_then(|p| Vec::<Plane>::deserialize(p).ok())
        .unwrap_or_default();
    let ok = |p: &Plane| p.src.iter().chain(&p.dst).all(finite);
    if planes.iter().all(ok) {
        planes
    } else {
        Vec::new()
    }
}

/// A warp's result as the layer; a cancelled one (or a puppet stretched too far) leaves it.
fn finish(
    src: &ImageData,
    out: Result<Option<ImageData>, crate::extra::interrupt::Cancelled>,
) -> ImageData {
    out.ok().flatten().unwrap_or_else(|| src.clone())
}

fn render_puppet(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (lifted, b) = warp_source(src, env);
    let s = lifted.as_ref().unwrap_or(src);
    let stop = Interrupt(env.cancel);
    finish(
        src,
        puppet_warp(s, b, &puppet(v), clip(env), Interp::Bicubic, stop),
    )
}

fn render_perspective(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let lifted = warp_source(src, env).0;
    let s = lifted.as_ref().unwrap_or(src);
    let stop = Interrupt(env.cancel);
    let out = perspective_warp(s, &planes(v), 1.0, clip(env), Interp::Bicubic, stop);
    finish(src, out.map(Some))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px};
    use crate::effect::{apply, defaults, find, with};

    /// The L-shaped puppet of upstream's tests in a 90×90 layer.
    fn blob() -> ImageData {
        from_fn(90, 90, |x, y| {
            if (20..36).contains(&x) && (10..70).contains(&y) {
                [((x + y) % 7 * 42) as u8, 128, (y * 3) as u8, 255]
            } else if (36..70).contains(&x) && (56..70).contains(&y) {
                [51, ((x * 3) % 5 * 63) as u8, 204, 255]
            } else {
                [0; 4]
            }
        })
    }

    /// A 40×30 layer: a 6×6 red block in the top-left corner, a gradient elsewhere.
    fn corners() -> ImageData {
        from_fn(40, 30, |x, y| {
            if x < 6 && y < 6 {
                [255, 0, 0, 255]
            } else {
                [(x * 6) as u8, (y * 8) as u8, 90, 255]
            }
        })
    }

    fn quad(r: [f64; 4]) -> [[f64; 2]; 4] {
        [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]]
    }

    /// No pins (the default) and pins that never moved give the layer back byte for byte.
    #[test]
    fn puppet_identity_without_moves() {
        let def = find(PUPPET).unwrap();
        let src = blob();
        let env = EffectEnv::whole(90, 90);
        assert_eq!(apply(&src, &def, &defaults(&def), &env).data, src.data);
        let still = with(
            &def,
            &json!({"pins": [{"src": [28, 20], "dst": [28, 20]}, {"src": [60, 62], "dst": [60, 62]}]}),
        );
        assert_eq!(apply(&src, &def, &still, &env).data, src.data);
    }

    /// One pin dragged by (10, −5) carries the whole puppet with it, exactly (bicubic at
    /// whole-pixel offsets); where it was is now transparent.
    #[test]
    fn puppet_single_pin_translates() {
        let def = find(PUPPET).unwrap();
        let src = blob();
        let v = with(
            &def,
            &json!({"pins": [{"src": [28, 30], "dst": [38, 25]}], "mode": "rigid"}),
        );
        let out = apply(&src, &def, &v, &EffectEnv::whole(90, 90));
        for y in 0..90 {
            for x in 0..90 {
                let want = if x >= 10 && y + 5 < 90 {
                    px(&src, x - 10, y + 5)
                } else {
                    [0; 4]
                };
                assert_eq!(px(&out, x, y), want, "({x}, {y})");
            }
        }
    }

    /// Corners that never moved give the layer back byte for byte, as does the default.
    #[test]
    fn perspective_identity_corners() {
        let def = find(PERSPECTIVE).unwrap();
        let src = corners();
        let env = EffectEnv::whole(40, 30);
        assert_eq!(apply(&src, &def, &defaults(&def), &env).data, src.data);
        let q = quad([0.0, 0.0, 40.0, 30.0]);
        let v = with(&def, &json!({"planes": [{"src": q, "dst": q}]}));
        assert_eq!(apply(&src, &def, &v, &env).data, src.data);
    }

    /// The layer's corners land exactly on the dragged ones: the map sends each to its target,
    /// the top-left block now starts at (8, 6), and what lies outside the new quad is
    /// transparent.
    #[test]
    fn perspective_maps_corners_exactly() {
        let def = find(PERSPECTIVE).unwrap();
        let src = corners();
        let s = quad([0.0, 0.0, 40.0, 30.0]);
        let d = [[8.0, 6.0], [40.0, 0.0], [40.0, 30.0], [0.0, 30.0]];
        let planes = [Plane { src: s, dst: d }];
        let m = crate::extra::perspective::PerspectiveMap::new(&planes).unwrap();
        for (a, b) in s.iter().zip(&d) {
            let (x, y) = m.map(a[0], a[1]);
            assert!(
                (x - b[0]).abs() < 1e-9 && (y - b[1]).abs() < 1e-9,
                "{a:?} → ({x}, {y})"
            );
        }
        let v = with(&def, &json!({"planes": planes}));
        let out = apply(&src, &def, &v, &EffectEnv::whole(40, 30));
        assert_eq!(
            px(&out, 9, 7),
            [255, 0, 0, 255],
            "the red block follows its corner"
        );
        assert_eq!(px(&out, 4, 3)[3], 0, "outside the new quad");
    }

    /// Ruling R83: with a selection the warps lift it. On an opaque layer under an ellipse,
    /// unselected pixels stay, nothing from outside the selection (red) shows inside it, and
    /// what each warp vacates inside it turns transparent.
    #[test]
    fn the_warps_lift_the_selection() {
        use crate::effect::bb::test_util::{ellipse_selection, with_selection};
        let (mask, polys) = ellipse_selection(40, 40, [20.0, 20.0], [14.0, 10.0]);
        let env = with_selection(40, 40, &mask, &polys);
        let inside = |x: u32, y: u32| mask.get(x as i32, y as i32) > 0;
        let src = from_fn(40, 40, |x, y| {
            if inside(x, y) {
                [0, 200, 0, 255]
            } else {
                [255, 0, 0, 255]
            }
        });
        // (8, 20) is well inside the ellipse; each warp below pulls it from outside.
        assert_eq!(mask.get(8, 20), 255);
        let (_, b) = warp_source(&src, &env);
        let [x0, y0, x1, y1] = [b.x0, b.y0, b.x1, b.y1].map(f64::from);
        let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
        let (hw, hh) = ((x1 - x0) / 4.0, (y1 - y0) / 4.0);
        let puppet = find(PUPPET).unwrap();
        let persp = find(PERSPECTIVE).unwrap();
        for (def, v) in [
            (
                &puppet,
                json!({"pins": [{"src": [20, 20], "dst": [26, 20]}], "mode": "rigid"}),
            ),
            (
                &persp,
                json!({"planes": [{"src": quad([x0, y0, x1, y1]),
                    "dst": quad([cx - hw, cy - hh, cx + hw, cy + hh])}]}),
            ),
        ] {
            let out = apply(&src, def, &with(def, &v), &env);
            for (x, y) in (0..40).flat_map(|y| (0..40).map(move |x| (x, y))) {
                if inside(x, y) {
                    assert_eq!(
                        px(&out, x, y)[0],
                        0,
                        "{} ({x}, {y}): outside came in",
                        def.id
                    );
                } else {
                    assert_eq!(px(&out, x, y), px(&src, x, y), "{} ({x}, {y})", def.id);
                }
            }
            assert_eq!(px(&out, 8, 20)[3], 0, "{}: vacated", def.id);
            assert_eq!(px(&out, 20, 20), [0, 200, 0, 255], "{}: the middle", def.id);
        }
    }

    /// I1: the warps anchor on the content inside `env.bounds`, as upstream anchors on
    /// `content_bounds()`: a 1000 px disc on a 3000×2000 layer gets the fine mesh its own size
    /// asks for, not one sized by the layer.
    #[test]
    fn the_mesh_anchors_on_the_content() {
        use crate::extra::puppet::{PuppetDensity, build_mesh};
        let src = from_fn(3000, 2000, |x, y| {
            let r = (f64::from(x) + 0.5 - 1500.0).hypot(f64::from(y) + 0.5 - 1000.0);
            if r <= 500.0 {
                [90, 140, 200, 255]
            } else {
                [0; 4]
            }
        });
        let (lifted, b) = warp_source(&src, &EffectEnv::whole(3000, 2000));
        assert!(lifted.is_none(), "no selection, nothing lifted");
        assert_eq!(b, Rect::new(1000, 500, 2000, 1500));
        let m = build_mesh(&src, b, PuppetDensity::Normal, 2.0);
        assert!(m.spacing < 51.0, "{}", m.spacing);
        assert!(m.verts.len() > 300, "{}", m.verts.len());
    }

    /// M7: every Layers Extra has its Layers action (else it would have no menu item).
    #[test]
    fn every_layers_extra_has_a_layers_action() {
        use super::super::manifest::{MANIFEST, layers_action};
        let layers = MANIFEST
            .iter()
            .filter(|m| m.category == crate::effect::Category::Layers);
        for m in layers {
            let action = layers_action(m.id).unwrap_or_else(|| panic!("{} has no action", m.id));
            assert_eq!(
                crate::action::get(action).map(|a| a.label),
                Some(Some(m.name))
            );
        }
    }

    /// A cancelled render leaves the layer as it was.
    #[test]
    fn warp_render_polls_cancel() {
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut env = EffectEnv::whole(90, 90);
        env.cancel = Some(&flag);
        let src = blob();
        let puppet = find(PUPPET).unwrap();
        let v = with(
            &puppet,
            &json!({"pins": [{"src": [28, 30], "dst": [38, 25]}]}),
        );
        assert_eq!(apply(&src, &puppet, &v, &env).data, src.data);
        let persp = find(PERSPECTIVE).unwrap();
        let q = quad([0.0, 0.0, 90.0, 90.0]);
        let mut d = q;
        d[0] = [20.0, 20.0];
        let v = with(&persp, &json!({"planes": [{"src": q, "dst": d}]}));
        assert_eq!(apply(&src, &persp, &v, &env).data, src.data);
    }

    /// Malformed values are refused piecewise: a bad pin is skipped, bad planes are dropped.
    #[test]
    fn malformed_values_are_skipped() {
        let v = with(
            &find(PUPPET).unwrap(),
            &json!({"pins": [{"src": [1, 2], "dst": [3, 4]}, {"src": "x"}, 7],
                    "mode": "sideways", "expansion": 1e9}),
        );
        let w = puppet(&v);
        assert_eq!(
            w.pins,
            vec![PuppetPin {
                src: [1.0, 2.0],
                dst: [3.0, 4.0]
            }]
        );
        assert_eq!((w.mode, w.expansion), (Default::default(), 200.0));
        let v = with(
            &find(PERSPECTIVE).unwrap(),
            &json!({"planes": [{"src": [[0, 0]]}]}),
        );
        assert!(planes(&v).is_empty());
    }

    /// OK through a session is one history item named after the command, undo restores the
    /// layer bit for bit, and as Layers effects they never become Effects ▸ Repeat.
    #[test]
    fn warp_commit_is_one_history_item_and_not_repeated() {
        use crate::effect::session::EffectSession;
        use crate::testing::{fresh_doc, layer_bytes};
        for (id, name, v) in [
            (
                PUPPET,
                "Puppet Warp",
                json!({"pins": [{"src": [20, 20], "dst": [26, 18]}]}),
            ),
            (
                PERSPECTIVE,
                "Perspective Warp",
                json!({"planes": [{"src": quad([0.0, 0.0, 48.0, 32.0]),
                    "dst": [[6, 4], [48, 0], [48, 32], [0, 32]]}]}),
            ),
        ] {
            let mut app = fresh_doc(48, 32, Some(7));
            let before = layer_bytes(&app, None);
            let undo = app.active_ws().unwrap().history.undo.len();
            let s = EffectSession::begin(&mut app, find(id).unwrap()).unwrap();
            let values = with(&s.def, &v);
            assert!(s.commit(&mut app, values));
            assert_ne!(layer_bytes(&app, None), before, "{id}");
            let ws = app.active_ws().unwrap();
            assert_eq!(ws.history.undo.len(), undo + 1, "{id}");
            assert_eq!(ws.history.undo.last().unwrap().name, name);
            assert!(app.last_effect.is_none(), "{id}: not Repeat's");
            app.undo();
            assert_eq!(layer_bytes(&app, None), before, "{id}");
        }
    }

    /// Layers ▸ Puppet Warp… and Perspective Warp… are Extra actions with their icons; Puppet
    /// Warp opens the effect's dialog, Perspective Warp its canvas mode (`tool::perspective_warp`).
    #[test]
    fn layers_actions_open_the_warps() {
        use crate::action::{ActionOutcome, UiRequest, get, is_extra, perform};
        use crate::effect::is_configurable_action;
        let mut app = crate::testing::fresh_doc(8, 8, None);
        for (action, id, name, icon) in [
            (
                "menu.layers.puppetWarp",
                PUPPET,
                "Puppet Warp",
                "menu_layers_puppet_warp_icon.png",
            ),
            (
                "menu.layers.perspectiveWarp",
                PERSPECTIVE,
                "Perspective Warp",
                "menu_layers_perspective_warp_icon.png",
            ),
        ] {
            let a = get(action).unwrap();
            assert_eq!(
                (a.label, a.icon, a.category),
                (Some(name), Some(icon), "Layers")
            );
            assert!(
                is_extra(action) && is_configurable_action(action),
                "{action}"
            );
            assert_eq!(find(id).unwrap().action_id(), action);
            let outcome = perform(&mut app, action);
            if id == PUPPET {
                assert_eq!(
                    outcome,
                    ActionOutcome::Ui(UiRequest::EffectDialog(id.into()))
                );
            } else {
                assert_eq!(outcome, ActionOutcome::Done);
                assert!(app.perspective.is_some());
            }
        }
        assert_eq!(
            crate::effect::rotate_zoom().action_id(),
            "menu.layers.rotateZoom"
        );
    }
}
