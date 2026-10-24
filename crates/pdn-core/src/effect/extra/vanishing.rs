//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; research R§B (Distort)
//!
//! Vanishing Point (`extra::fx::vanishing`). Its own dialog (`paintrs::dialogs::vanishing_point`)
//! makes a plane from four clicks and places the clipboard image on it. The effect's value is
//! that plane and placement: `corners` (four document points, clockwise from the top-left), `at`
//! (the image's top-left in plane coordinates) and `width` (% of the plane's width). The image
//! is an external input like a clipboard plugin's (R155), so Repeat reads the clipboard again.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::bb::ui::{External, IndirectUi};
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values, num};
use crate::extra::fx::vanishing::{VpPaste, VpPlane, paste};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use serde_json::{Value, json};

/// The effect id; the shell opens `VanishingPointDialog` for it instead of the Effect Config one.
pub const ID: &str = "pcVanishingPoint";

/// Only the clipboard read: the dialog is its own, so no row is presented through this.
static UI: IndirectUi = IndirectUi {
    external_for: |_| External::Clipboard,
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let m = manifest(ID).expect("effect has a manifest row");
    vec![EffectDef {
        id: ID,
        icon: m.icon,
        category: m.category,
        // What the dialog sets; no row draws them.
        controls: vec![
            Control::Value {
                key: "corners",
                default: || json!([]),
            },
            Control::Value {
                key: "at",
                default: || json!([0.25, 0.25]),
            },
            Control::Value {
                key: "width",
                default: || json!(50),
            },
        ],
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: Some(&UI),
    }]
}

/// The stored plane and placement; `None` without four numeric corners. The placement is kept
/// within upstream's limits: `at` in −1..2 per axis, the width 1–1000 % of the plane's.
pub fn paste_of(v: &Values) -> Option<VpPaste> {
    let pt = |p: &Value| Some([p.get(0)?.as_f64()?, p.get(1)?.as_f64()?]);
    let c = v.get("corners")?.as_array()?;
    let corners = match c.as_slice() {
        [a, b, c, d] => [pt(a)?, pt(b)?, pt(c)?, pt(d)?],
        _ => return None,
    };
    let at = v
        .get("at")
        .and_then(pt)
        .unwrap_or([0.25, 0.25])
        .map(|x| x.clamp(-1.0, 2.0));
    Some(VpPaste {
        plane: VpPlane { corners },
        at,
        width: (num(v, "width") / 100.0).clamp(0.01, 10.0),
    })
}

/// Pastes the clipboard image onto the plane, clipped to the selection bounds; without an image
/// or a usable plane, or once cancelled (polled per row), the layer as it was.
fn render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (Some(image), Some(p)) = (env.image, paste_of(v)) else {
        return src.clone();
    };
    let b = env.bounds;
    let clip = Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32);
    let mut out = src.clone();
    let doc = [src.width, src.height];
    match paste(&mut out, doc, clip, &p, image, Interrupt(env.cancel)) {
        Ok(()) => out,
        Err(_) => src.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, solid};
    use crate::effect::{apply, defaults, find, with};

    fn noise() -> ImageData {
        from_fn(48, 40, |x, y| {
            let mut s = u64::from(y * 48 + x);
            let b = crate::extra::rng::splitmix64(&mut s).to_le_bytes();
            [b[0], b[1], b[2], 255]
        })
    }

    /// A trapezoid plane over most of the 48×40 layer with a red image covering it.
    fn covering() -> Values {
        with(
            &find(ID).unwrap(),
            &json!({"corners": [[8, 6], [40, 6], [46, 36], [2, 36]], "at": [-0.5, -0.5],
                "width": 300}),
        )
    }

    /// No plane (the defaults), no clipboard image or a red plane: the layer, byte for byte.
    #[test]
    fn vanishing_point_needs_a_plane_and_an_image() {
        let def = find(ID).unwrap();
        let (src, red) = (noise(), solid(8, 8, [255, 0, 0, 255]));
        let mut env = EffectEnv::whole(48, 40);
        assert_eq!(apply(&src, &def, &covering(), &env).data, src.data);
        env.image = Some(&red);
        assert_eq!(apply(&src, &def, &defaults(&def), &env).data, src.data);
        let bowtie = with(
            &def,
            &json!({"corners": [[8, 6], [46, 36], [40, 6], [2, 36]], "width": 300}),
        );
        assert_eq!(apply(&src, &def, &bowtie, &env).data, src.data);
        assert_ne!(apply(&src, &def, &covering(), &env).data, src.data);
    }

    /// With a rectangular selection the paste lands inside it only.
    #[test]
    fn vanishing_point_keeps_to_the_selection() {
        use crate::effect::bb::test_util::{rect_selection, with_selection};
        let def = find(ID).unwrap();
        let (src, red) = (noise(), solid(8, 8, [255, 0, 0, 255]));
        let (mask, polys) = rect_selection(48, 40, 0, 0, 24, 40);
        let mut env = with_selection(48, 40, &mask, &polys);
        env.image = Some(&red);
        let out = apply(&src, &def, &covering(), &env);
        for (x, y) in (0..40).flat_map(|y| (24..48).map(move |x| (x, y))) {
            assert_eq!(px(&out, x, y), px(&src, x, y), "({x}, {y})");
        }
        assert_eq!(px(&out, 20, 20), [255, 0, 0, 255]);
    }

    /// The stored placement is clamped to upstream's limits; malformed corners give no plane.
    #[test]
    fn stored_values_are_clamped() {
        let def = find(ID).unwrap();
        let p = paste_of(&with(
            &def,
            &json!({"corners": [[0, 0], [9, 0], [9, 9], [0, 9]], "at": [-7, 1e9], "width": 5e4}),
        ))
        .unwrap();
        assert_eq!((p.at, p.width), ([-1.0, 2.0], 10.0));
        for corners in [
            json!([]),
            json!([[0, 0], [9, 0], [9, 9]]),
            json!([[0, "a"], [9, 0], [9, 9], [0, 9]]),
        ] {
            assert!(paste_of(&with(&def, &json!({ "corners": corners }))).is_none());
        }
    }

    /// A cancelled render returns the layer unchanged.
    #[test]
    fn vanishing_point_polls_cancel() {
        let def = find(ID).unwrap();
        let (src, red) = (noise(), solid(8, 8, [255, 0, 0, 255]));
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut env = EffectEnv::whole(48, 40);
        env.image = Some(&red);
        env.cancel = Some(&flag);
        assert_eq!(apply(&src, &def, &covering(), &env).data, src.data);
    }

    /// Effects ▸ Repeat of Vanishing Point goes to the Effect Config dialog's cancellable final
    /// render with a fresh clipboard read (`RepeatExternal`), not a synchronous render here.
    #[test]
    fn repeat_vanishing_point_runs_on_the_worker() {
        use crate::action::{ActionOutcome, UiRequest, perform};
        use crate::effect::session::EffectSession;
        use crate::testing::{fresh_doc, layer_bytes};
        let mut app = fresh_doc(48, 40, Some(7));
        let mut s = EffectSession::begin(&mut app, find(ID).unwrap()).unwrap();
        s.set_image(Some(std::sync::Arc::new(solid(8, 8, [255, 0, 0, 255]))));
        let before = layer_bytes(&app, None);
        assert!(s.commit(&mut app, covering()));
        let (after, undo) = (
            layer_bytes(&app, None),
            app.active_ws().unwrap().history.undo.len(),
        );
        assert_ne!(after, before);
        assert_eq!(
            app.active_ws().unwrap().history.undo.last().unwrap().name,
            "Vanishing Point"
        );
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Ui(UiRequest::RepeatExternal)
        );
        assert_eq!(layer_bytes(&app, None), after, "nothing ran here");
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo);
    }
}
