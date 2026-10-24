//! Photocraft phase spec: S§4.4 "Interactive and Tier 3 items"; research R§B (Distort)
//!
//! Liquify (`extra::fx::liquify`). Its own dialog (`paintrs::dialogs::liquify`) records brush
//! strokes; the effect's value is that stroke list, replayed onto a fresh displacement field over
//! the layer and resampled at full resolution, so OK and Repeat give the same result.

use super::manifest::manifest;
use crate::canvas::ImageData;
use crate::effect::{Control, EffectDef, EffectEnv, EffectKind, Values};
use crate::extra::fx::liquify::{LiquifyField, LiquifyStroke, apply_liquify, auto_cell};
use crate::extra::geom::Rect;
use crate::extra::interrupt::Interrupt;
use serde::Deserialize;

/// The effect id; the shell opens `LiquifyDialog` for it instead of the Effect Config dialog.
pub const ID: &str = "pcLiquify";

pub fn effects() -> Vec<EffectDef> {
    let m = manifest(ID).expect("effect has a manifest row");
    vec![EffectDef {
        id: ID,
        icon: m.icon,
        category: m.category,
        // The recorded strokes (`LiquifyStroke`s as JSON); no row draws it.
        controls: vec![Control::Value {
            key: "strokes",
            default: || serde_json::json!([]),
        }],
        kind: EffectKind::Cpu {
            render,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(m.name),
        indirect: None,
    }]
}

/// A fresh displacement field over a w×h layer: the one the dialog previews and the render
/// replays, so both see the same field.
pub fn field(w: u32, h: u32) -> LiquifyField {
    let bounds = Rect::new(0, 0, w as i32, h as i32);
    LiquifyField::new(bounds, auto_cell(bounds))
}

/// The recorded strokes; malformed or out-of-range ones are skipped.
fn strokes(v: &Values) -> Vec<LiquifyStroke> {
    v.get("strokes")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| LiquifyStroke::deserialize(s).ok())
        .filter(|s| s.validate().is_ok())
        .collect()
}

/// Replays the strokes and resamples the layer; a cancel (polled between strokes, before each
/// segment of a stroke and per tile) returns it unchanged.
fn render(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let stop = Interrupt(env.cancel);
    let mut f = field(src.width, src.height);
    for s in strokes(v) {
        if stop.cancelled() || f.apply_stroke_until(&s, stop).is_err() {
            return src.clone();
        }
    }
    match apply_liquify(&src.data, &f, stop) {
        Ok(data) => ImageData {
            width: src.width,
            height: src.height,
            data,
        },
        Err(_) => src.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px};
    use crate::effect::{apply, defaults, find, with};
    use serde_json::json;

    /// A 48×32 black layer with a white marker column at x 16–17.
    fn marker() -> ImageData {
        from_fn(48, 32, |x, _| {
            if (16..18).contains(&x) {
                [255, 255, 255, 255]
            } else {
                [0, 0, 0, 255]
            }
        })
    }

    fn warp(points: serde_json::Value) -> Values {
        let def = find(ID).unwrap();
        with(
            &def,
            &json!({"strokes": [{"tool": "forwardWarp", "size": 24, "points": points}]}),
        )
    }

    /// No strokes (the default) and a zero-pressure stroke leave every byte as it was.
    #[test]
    fn liquify_empty_field_is_identity() {
        let def = find(ID).unwrap();
        let src = from_fn(21, 13, |x, y| {
            [(x * 12) as u8, (y * 19) as u8, 77, (x * y) as u8]
        });
        let env = EffectEnv::whole(21, 13);
        assert_eq!(apply(&src, &def, &defaults(&def), &env).data, src.data);
        let still = with(
            &def,
            &json!({"strokes": [{"tool": "forwardWarp", "size": 9, "pressure": 0,
                "points": [[3, 3], [15, 9]]}]}),
        );
        assert_eq!(apply(&src, &def, &still, &env).data, src.data);
    }

    /// A Forward Warp drag from (16, 16) to (28, 16) carries the marker right along row 16;
    /// rows the brush never reaches stay put.
    #[test]
    fn forward_warp_moves_pixels_along_drag() {
        let def = find(ID).unwrap();
        let src = marker();
        let out = apply(
            &src,
            &def,
            &warp(json!([[16, 16], [28, 16]])),
            &EffectEnv::whole(48, 32),
        );
        let brightest = (0..48).max_by_key(|&x| px(&out, x, 16)[0]).unwrap();
        assert!(brightest >= 20, "the marker moved right: {brightest}");
        assert!(px(&out, 16, 16)[0] < 128, "it left its old place");
        for y in [0, 1, 31] {
            for x in 0..48 {
                assert_eq!(px(&out, x, y), px(&src, x, y), "({x}, {y})");
            }
        }
    }

    /// OK is one history item named "Liquify"; undo restores the layer bit for bit; Repeat
    /// replays the same strokes to the same pixels.
    #[test]
    fn repeat_replays_the_strokes() {
        use crate::effect::session::{EffectSession, repeat_last};
        use crate::testing::{fresh_doc, layer_bytes};
        let mut app = fresh_doc(48, 32, Some(7));
        let before = layer_bytes(&app, None);
        let undo = app.active_ws().unwrap().history.undo.len();
        let values = warp(json!([[16, 16], [28, 16], [30, 20]]));
        let s = EffectSession::begin(&mut app, find(ID).unwrap()).unwrap();
        assert!(s.commit(&mut app, values.clone()));
        let after = layer_bytes(&app, None);
        assert_ne!(after, before);
        let ws = app.active_ws().unwrap();
        assert_eq!(ws.history.undo.len(), undo + 1);
        assert_eq!(ws.history.undo.last().unwrap().name, "Liquify");
        assert_eq!(app.last_effect_values.as_ref(), Some(&values));
        app.undo();
        assert_eq!(layer_bytes(&app, None), before);
        assert!(repeat_last(&mut app));
        assert_eq!(layer_bytes(&app, None), after);
    }

    /// Effects ▸ Repeat of Liquify goes to the dialog's worker (`RepeatSeam`: Effect Config's
    /// cancellable final render, as OK), not a synchronous replay on the UI thread.
    #[test]
    fn repeat_liquify_runs_on_the_worker() {
        use crate::action::{ActionOutcome, UiRequest, perform};
        use crate::effect::session::EffectSession;
        use crate::testing::{fresh_doc, layer_bytes};
        let mut app = fresh_doc(48, 32, Some(7));
        let s = EffectSession::begin(&mut app, find(ID).unwrap()).unwrap();
        assert!(s.commit(&mut app, warp(json!([[16, 16], [28, 16]]))));
        let (before, undo) = (
            layer_bytes(&app, None),
            app.active_ws().unwrap().history.undo.len(),
        );
        assert_eq!(
            perform(&mut app, "menu.effects.repeat"),
            ActionOutcome::Ui(UiRequest::RepeatSeam)
        );
        assert_eq!(layer_bytes(&app, None), before, "nothing ran here");
        assert_eq!(app.active_ws().unwrap().history.undo.len(), undo);
    }

    /// A cancelled full-resolution apply returns the layer unchanged.
    #[test]
    fn liquify_polls_cancel() {
        let def = find(ID).unwrap();
        let src = marker();
        let flag = std::sync::atomic::AtomicBool::new(true);
        let mut env = EffectEnv::whole(48, 32);
        env.cancel = Some(&flag);
        let out = apply(&src, &def, &warp(json!([[16, 16], [28, 16]])), &env);
        assert_eq!(out.data, src.data);
    }
}
