//! Photocraft phase spec: S§4.2; research R§B (Blurs)
//!
//! The Extra Blurs effects: Average, Tilt-Shift and Iris Blur (`extra::fx::{average, gallery}`).
//! Centres, the sharp band and the ellipse anchor on the selection bounds, as upstream anchors
//! on `ctx.bounds`; the whole layer is read. The centre is a Pan row, which only the IndirectUi
//! path draws.

use crate::canvas::ImageData;
use crate::effect::bb::{pan, ui::IndirectUi};
use crate::effect::{
    Category, Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, num, range,
};
use crate::extra::fx::gallery::IrisPin;
use crate::extra::geom::Rect;
use crate::extra::{fx, interrupt::Interrupt, shim};

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

static GALLERY_UI: IndirectUi = IndirectUi::DEFAULT;

/// 0-1 fractions of a 0-1 range: two decimals.
pub(super) fn fraction(
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> Control {
    range(key, label, min, max, default)
        .step(0.01)
        .slider_keys(2, 0.01, 0.1, false)
}

fn centre() -> Control {
    pan_control("center", "Center", [0.0, 0.0])
}

/// A -1..1 Pan row over the selection crop.
pub(super) fn pan_control(key: &'static str, label: &'static str, default: [f64; 2]) -> Control {
    Control::Pan {
        key,
        label,
        min: -1.0,
        max: 1.0,
        default,
        step: 0.01,
        thumb: PanThumb::SelectionCrop,
    }
}

pub fn effects() -> Vec<EffectDef> {
    let blurs = |id: &'static str,
                 controls: Vec<Control>,
                 render: RenderFn,
                 indirect: Option<&'static IndirectUi>| {
        let m = super::manifest::manifest(id).expect("manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: Category::Effect("blurring"),
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(m.name),
            indirect,
        }
    };
    vec![
        blurs("pcAverage", vec![], average, None),
        blurs(
            "pcTiltShift",
            vec![
                range("blur", "Blur", 0.0, 500.0, 15.0),
                centre(),
                range("angle", "Angle", -90.0, 90.0, 0.0),
                fraction("focus", "Focus", 0.0, 1.0, 0.1),
                fraction("transition", "Transition", 0.01, 1.0, 0.15),
            ],
            tilt_shift,
            Some(&GALLERY_UI),
        ),
        blurs(
            "pcIrisBlur",
            vec![
                range("blur", "Blur", 0.0, 500.0, 15.0),
                centre(),
                fraction("radiusX", "Radius X", 0.01, 1.0, 0.35),
                fraction("radiusY", "Radius Y", 0.01, 1.0, 0.25),
                range("angle", "Angle", -180.0, 180.0, 0.0),
                range("roundness", "Roundness", 0.0, 100.0, 0.0),
                fraction("feather", "Feather", 0.0, 0.99, 0.5),
            ],
            iris,
            Some(&GALLERY_UI),
        ),
    ]
}

/// The selection bounds ∩ the layer, as upstream's `ctx.bounds`.
pub(super) fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// The Pan row's -1..1 as a 0..1 fraction of the bounds.
fn centre_of(v: &Values) -> (f32, f32) {
    pan_frac(v, "center")
}

/// A Pan row's -1..1 as a 0..1 fraction of the bounds.
pub(super) fn pan_frac(v: &Values, key: &str) -> (f32, f32) {
    let [x, y] = pan(v, key);
    (
        (0.5 + x.clamp(-1.0, 1.0) / 2.0) as f32,
        (0.5 + y.clamp(-1.0, 1.0) / 2.0) as f32,
    )
}

/// A kernel's result as a layer; a cancelled render leaves the source.
pub(super) fn finish(src: &ImageData, out: Option<Vec<f32>>) -> ImageData {
    out.map_or_else(
        || src.clone(),
        |buf| shim::from_f32(&buf, src.width, src.height),
    )
}

fn average(src: &ImageData, _: &Values, env: &EffectEnv) -> ImageData {
    let out = fx::average::average(
        &shim::to_f32(src),
        src.width as usize,
        bounds(env),
        |x, y| f32::from(env.selected(x as u32, y as u32)),
    );
    shim::from_f32(&out, src.width, src.height)
}

fn tilt_shift(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = fx::gallery::tilt_shift(
        &shim::to_f32(src),
        (src.width as usize, src.height as usize),
        bounds(env),
        num(v, "blur") as f32,
        centre_of(v),
        num(v, "angle") as f32,
        num(v, "focus").clamp(0.0, 2.0) as f32,
        num(v, "transition").clamp(0.001, 2.0) as f32,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn iris(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (x, y) = centre_of(v);
    let pin = IrisPin {
        x,
        y,
        radius_x: num(v, "radiusX").max(0.001) as f32,
        radius_y: num(v, "radiusY").max(0.001) as f32,
        angle: num(v, "angle") as f32,
        roundness: num(v, "roundness").clamp(0.0, 100.0) as f32,
        feather: num(v, "feather").clamp(0.0, 0.99) as f32,
        blur: num(v, "blur") as f32,
    };
    let out = fx::gallery::iris(
        &shim::to_f32(src),
        (src.width as usize, src.height as usize),
        bounds(env),
        &[pin],
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use serde_json::json;

    fn checker(x: u32, y: u32) -> [u8; 4] {
        let v = if (x + y).is_multiple_of(2) { 0 } else { 255 };
        [v, v, v, 255]
    }

    /// A rectangular selection over a red | blue split fills with the purple mean; nothing
    /// outside it changes.
    #[test]
    fn average_fills_selection_with_mean() {
        let src = from_fn(16, 8, |x, _| {
            if x < 8 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        });
        let (mask, polys) = rect_selection(16, 8, 4, 2, 12, 6);
        let env = with_selection(16, 8, &mask, &polys);
        let out = run_env("pcAverage", json!({}), &src, &env);
        for (x, y) in (2..6).flat_map(|y| (4..12).map(move |x| (x, y))) {
            let p = px(&out, x, y);
            assert!(
                p[0].abs_diff(128) <= 1 && p[2].abs_diff(128) <= 1 && p[1] == 0,
                "{p:?}"
            );
            assert_eq!(p[3], 255);
        }
        assert_eq!(px(&out, 0, 0), px(&src, 0, 0));
        assert_eq!(px(&out, 15, 7), px(&src, 15, 7));
        // No selection: the whole layer.
        let whole = run("pcAverage", json!({}), &src);
        assert_eq!(px(&whole, 0, 0), px(&whole, 15, 7));
    }

    /// Tilt-Shift's band at angle 0 is horizontal through the centre rows, which stay sharp; the
    /// top rows blur.
    #[test]
    fn tilt_shift_band_is_sharp() {
        let src = from_fn(48, 48, checker);
        let out = run(
            "pcTiltShift",
            json!({"blur": 30, "angle": 0, "focus": 0.1, "transition": 0.2}),
            &src,
        );
        for x in 0..48 {
            assert_eq!(px(&out, x, 24), px(&src, x, 24), "x {x}");
        }
        let v = px(&out, 24, 1)[0];
        assert!(v.abs_diff(127) < 60, "{v}");
        // Blur 0 is the identity.
        assert_eq!(run("pcTiltShift", json!({"blur": 0}), &src).data, src.data);
    }

    #[test]
    fn iris_centre_is_sharp_edges_blurred() {
        let src = from_fn(64, 64, checker);
        let out = run("pcIrisBlur", json!({}), &src);
        assert_eq!(px(&out, 32, 32), px(&src, 32, 32));
        let v = px(&out, 1, 1)[0];
        assert!(v.abs_diff(127) < 60, "{v}");
    }

    /// The centre Pan moves the sharp core: panned to the left edge, the left-middle pixel is
    /// sharp and the layer's centre is not.
    #[test]
    fn iris_centre_follows_the_pan() {
        let src = from_fn(64, 64, checker);
        let out = run("pcIrisBlur", json!({"center": [-0.8, 0.0]}), &src);
        assert_eq!(px(&out, 6, 32), px(&src, 6, 32));
        assert_ne!(px(&out, 32, 32), px(&src, 32, 32));
    }
}
