//! Photocraft phase spec: S§4.2; research R§B (Render)
//!
//! The Extra Render effects Lens Flare, Lighting Effects and Fibers (`extra::fx::render`). The
//! centre and light position are Pan rows (only the IndirectUi path draws them). Everything
//! anchors on the selection bounds, as upstream anchors on `ctx.bounds`; the whole layer is read.
//! Upstream's colour-mode and 32-bit parameters, the per-light list and Lighting's gloss,
//! metallic, exposure, bump-direction and spot cone/radius/z rows are not offered: Paint.NET documents
//! are 8-bit RGB, and the rows left out keep upstream's defaults.

use crate::canvas::ImageData;
use crate::effect::bb::{pan, ui::IndirectUi};
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, PanThumb, Values, choice,
    color_value, num, range,
};
use crate::extra::fx::render::{self, LensType, Light, LightKind, LightingSpec, TextureChannel};
use crate::extra::geom::Rect;
use crate::extra::{interrupt::Interrupt, shim};

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

static RENDER_UI: IndirectUi = IndirectUi::DEFAULT;

fn centre(label: &'static str, default: [f64; 2]) -> Control {
    Control::Pan {
        key: "center",
        label,
        min: -1.0,
        max: 1.0,
        default,
        step: 0.01,
        thumb: PanThumb::SelectionCrop,
    }
}

pub fn effects() -> Vec<EffectDef> {
    let def = |id: &'static str,
               controls: Vec<Control>,
               render: RenderFn,
               indirect: Option<&'static IndirectUi>| {
        let m = super::manifest::manifest(id).expect("manifest row");
        EffectDef {
            id,
            icon: m.icon,
            category: Category::Effect("render"),
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
        def(
            "pcLensFlare",
            vec![
                range("brightness", "Brightness", 10.0, 300.0, 100.0),
                centre("Center", [0.0, 0.0]),
                Control::Select {
                    key: "lens",
                    label: "Lens Type",
                    default: "zoom",
                    choices: &[
                        ("zoom", "50-300mm Zoom"),
                        ("prime35", "35mm Prime"),
                        ("prime105", "105mm Prime"),
                        ("moviePrime", "Movie Prime"),
                    ],
                },
            ],
            lens_flare,
            Some(&RENDER_UI),
        ),
        def(
            "pcLightingEffects",
            vec![
                Control::Select {
                    key: "lightType",
                    label: "Light Type",
                    default: "spot",
                    choices: &[
                        ("spot", "Spot"),
                        ("point", "Point"),
                        ("infinite", "Infinite"),
                    ],
                },
                range("intensity", "Intensity", -100.0, 100.0, 75.0),
                range("focus", "Focus", 0.0, 100.0, 50.0),
                centre("Light Position", [-0.5, -0.6]),
                Control::Color {
                    key: "color",
                    label: "Light Color",
                    default: ColorDefault::Rgb(0xff_ffff),
                },
                range("ambience", "Ambience", -100.0, 100.0, 8.0),
                Control::Select {
                    key: "texture",
                    label: "Texture Channel",
                    default: "none",
                    choices: &[
                        ("none", "None"),
                        ("red", "Red"),
                        ("green", "Green"),
                        ("blue", "Blue"),
                        ("alpha", "Alpha"),
                        ("luminance", "Luminance"),
                    ],
                },
                range("height", "Texture Height", 0.0, 100.0, 50.0),
            ],
            lighting,
            Some(&RENDER_UI),
        ),
        def(
            "pcFibers",
            vec![
                range("variance", "Variance", 1.0, 64.0, 16.0),
                range("strength", "Strength", 1.0, 64.0, 4.0),
                // BB's reseed row: 0-255, folded with the instance seed.
                range("seed", "Randomize", 0.0, 255.0, 0.0),
                Control::Color {
                    key: "foreground",
                    label: "Foreground",
                    default: ColorDefault::Primary,
                },
                Control::Color {
                    key: "background",
                    label: "Background",
                    default: ColorDefault::Secondary,
                },
            ],
            fibers,
            None,
        ),
    ]
}

/// The selection bounds ∩ the layer, as upstream's `ctx.bounds`.
fn bounds(env: &EffectEnv) -> Rect {
    let b = env.bounds;
    Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32)
}

/// The Pan row's -1..1 as a 0..1 fraction of the bounds.
fn fraction_of(v: &Values) -> (f32, f32) {
    let [x, y] = pan(v, "center");
    (
        (0.5 + x.clamp(-1.0, 1.0) / 2.0) as f32,
        (0.5 + y.clamp(-1.0, 1.0) / 2.0) as f32,
    )
}

/// The seed row folded with the instance seed, as Clouds does.
fn seed(v: &Values, env: &EffectEnv) -> u32 {
    (num(v, "seed").clamp(0.0, 255.0) as u32 ^ env.instance_seed) & 255
}

/// A colour row as RGB 0-1.
fn rgb(v: &Values, key: &str, env: &EffectEnv) -> [f32; 3] {
    let c = color_value(v, key, env);
    [0, 1, 2].map(|i| f32::from(c[i]) / 255.0)
}

/// A kernel's result as a layer; a cancelled render leaves the source.
fn finish(src: &ImageData, out: Option<Vec<f32>>) -> ImageData {
    out.map_or_else(
        || src.clone(),
        |buf| shim::from_f32(&buf, src.width, src.height),
    )
}

fn lens_flare(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let lens = match choice(v, "lens") {
        "prime35" => LensType::Prime35,
        "prime105" => LensType::Prime105,
        "moviePrime" => LensType::MoviePrime,
        _ => LensType::Zoom,
    };
    let out = render::lens_flare(
        &shim::to_f32(src),
        (src.width as usize, src.height as usize),
        bounds(env),
        num(v, "brightness").clamp(10.0, 300.0) as f32,
        fraction_of(v),
        lens,
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn lighting(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let (x, y) = fraction_of(v);
    let kind = match choice(v, "lightType") {
        "point" => LightKind::Point,
        "infinite" => LightKind::Infinite,
        _ => LightKind::Spot,
    };
    let [px, py] = pan(v, "center");
    let light = Light {
        kind,
        x,
        y,
        // The spot's target moves with the light, keeping upstream's oblique default look.
        target_x: 0.5 + (x - 0.25),
        target_y: 0.55 + (y - 0.2),
        // An infinite light has no position: it shines from the side the Pan is moved to.
        angle: (-py).atan2(px).to_degrees() as f32,
        color: rgb(v, "color", env),
        intensity: num(v, "intensity").clamp(-100.0, 100.0) as f32,
        hotspot: num(v, "focus").clamp(0.0, 100.0) as f32,
        ..Light::default()
    };
    let texture = match choice(v, "texture") {
        "red" => TextureChannel::Red,
        "green" => TextureChannel::Green,
        "blue" => TextureChannel::Blue,
        "alpha" => TextureChannel::Alpha,
        "luminance" => TextureChannel::Luminance,
        _ => TextureChannel::None,
    };
    let out = render::lighting(
        &shim::to_f32(src),
        (src.width as usize, src.height as usize),
        bounds(env),
        &LightingSpec {
            lights: &[light],
            gloss: 0.0,
            metallic: 0.0,
            exposure: 0.0,
            ambience: num(v, "ambience").clamp(-100.0, 100.0) as f32,
            texture,
            height: num(v, "height").clamp(0.0, 100.0) as f32,
            white_is_high: true,
        },
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

fn fibers(src: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let out = render::fibers(
        &shim::to_f32(src),
        (src.width as usize, src.height as usize),
        bounds(env),
        num(v, "variance").clamp(1.0, 64.0) as f32,
        num(v, "strength").clamp(1.0, 64.0) as f32,
        seed(v, env),
        (rgb(v, "foreground", env), rgb(v, "background", env)),
        &Interrupt(env.cancel),
    );
    finish(src, out)
}

#[cfg(test)]
mod tests {
    use crate::effect::bb::test_util::{rect_selection, with_selection};
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use crate::effect::{EffectEnv, find};
    use serde_json::json;
    use std::sync::atomic::AtomicBool;

    fn noise(x: u32, y: u32) -> [u8; 4] {
        let mut s = u64::from(y * 97 + x);
        let b = crate::extra::rng::splitmix64(&mut s).to_le_bytes();
        [b[0], b[1], b[2], 255]
    }

    /// Upstream `lens_flare_is_brightest_at_its_centre`: the 105mm flare on a dark grey layer is
    /// near white at its centre (a quarter across, half down) and brighter there than in a far
    /// corner, which the screen blend never darkens.
    #[test]
    fn lens_flare_brightest_at_centre() {
        let src = from_fn(40, 30, |_, _| [51, 51, 51, 255]);
        let out = run(
            "pcLensFlare",
            json!({"center": [-0.5, 0.0], "lens": "prime105"}),
            &src,
        );
        assert!(px(&out, 10, 15)[0] > 230, "{:?}", px(&out, 10, 15));
        assert!(px(&out, 10, 15)[0] > px(&out, 38, 2)[0]);
        assert!(px(&out, 38, 2)[0] >= 51);
        assert_eq!(px(&out, 10, 15)[3], 255);
        // Brightness scales it; every lens type changes the layer.
        let dim = run("pcLensFlare", json!({"brightness": 10}), &src);
        let bright = run("pcLensFlare", json!({"brightness": 300}), &src);
        assert!(px(&bright, 25, 18)[0] > px(&dim, 25, 18)[0]);
        for lens in ["zoom", "prime35", "prime105", "moviePrime"] {
            assert_ne!(
                run("pcLensFlare", json!({"lens": lens}), &src).data,
                src.data
            );
        }
    }

    /// Upstream `lens_flare_8_bit_result_is_the_screen_blend`: the flare added over any colour
    /// is `1 - (1 - c)(1 - amount)`, the amount being what it adds over black.
    #[test]
    fn lens_flare_result_is_the_screen_blend() {
        let v = json!({"brightness": 150, "center": [-0.5, 0.0]});
        let black = run(
            "pcLensFlare",
            v.clone(),
            &from_fn(40, 30, |_, _| [0, 0, 0, 255]),
        );
        let src = from_fn(40, 30, noise);
        let out = run("pcLensFlare", v, &src);
        for (x, y) in (0..30).flat_map(|y| (0..40).map(move |x| (x, y))) {
            let (c, a, o) = (px(&src, x, y), px(&black, x, y), px(&out, x, y));
            for k in 0..3 {
                let want = 1.0 - (1.0 - f32::from(c[k]) / 255.0) * (1.0 - f32::from(a[k]) / 255.0);
                assert!((f32::from(o[k]) - want * 255.0).abs() <= 1.5, "{x},{y},{k}");
            }
        }
    }

    /// Upstream `lighting_point_light_falls_off`, and a spot light stays inside its cone.
    #[test]
    fn lighting_point_and_spot_fall_off() {
        let src = from_fn(40, 30, |_, _| [204, 204, 204, 255]);
        let point =
            json!({"lightType": "point", "center": [-0.8, 0.0], "intensity": 50, "ambience": 0});
        let out = run("pcLightingEffects", point, &src);
        assert!(px(&out, 4, 15)[0] > px(&out, 36, 15)[0] + 30);
        let spot = json!({"lightType": "spot", "center": [0.0, 0.0], "ambience": 0});
        let out = run("pcLightingEffects", spot, &src);
        assert!(
            px(&out, 20, 15)[0] > 80 && px(&out, 1, 1)[0] == 0,
            "{:?} {:?}",
            px(&out, 20, 15),
            px(&out, 1, 1)
        );
        // The lit pool follows the Pan: the brightest column moves right with it.
        let peak = |pan: f64| {
            let v = json!({"lightType": "spot", "center": [pan, 0.0], "ambience": 0});
            let out = run("pcLightingEffects", v, &src);
            (0..40 * 30)
                .max_by_key(|&i| (px(&out, i % 40, i / 40)[0], std::cmp::Reverse(i)))
                .map(|i| i % 40)
                .unwrap()
        };
        assert!(peak(0.5) > peak(-0.5) + 5, "{} {}", peak(0.5), peak(-0.5));
    }

    /// With no light intensity only the ambience shows: the colour times `ambience / 200`, so
    /// zero ambience is black. Alpha is untouched.
    #[test]
    fn lighting_zero_intensity_is_dark_ambient_only() {
        let src = from_fn(12, 12, |_, _| [200, 100, 40, 77]);
        for kind in ["spot", "point", "infinite"] {
            let off = json!({"lightType": kind, "intensity": 0, "ambience": 0});
            let out = run("pcLightingEffects", off, &src);
            assert!(out.data.chunks(4).all(|p| p == [0, 0, 0, 77]), "{kind}");
            let amb = json!({"lightType": kind, "intensity": 0, "ambience": 50});
            let out = run("pcLightingEffects", amb, &src);
            assert!(out.data.chunks(4).all(|p| p == [50, 25, 10, 77]), "{kind}");
        }
    }

    /// The colour row tints the light and the texture channel bumps the surface; Pan moves an
    /// infinite light's side.
    #[test]
    fn lighting_color_texture_and_infinite_direction() {
        let src = from_fn(24, 24, |x, y| {
            let v = ((x / 4 + y / 4) % 2 * 200) as u8;
            [v, v, v, 255]
        });
        let red = json!({"lightType": "infinite", "color": "#ff0000", "ambience": 0});
        let out = run("pcLightingEffects", red, &src);
        let white = run(
            "pcLightingEffects",
            json!({"lightType": "infinite", "ambience": 0}),
            &src,
        );
        // Red light darkens green and blue (only the metallic highlight keeps some), not red.
        for (r, w) in out.data.chunks(4).zip(white.data.chunks(4)) {
            assert!(r[0] == w[0] && r[1] <= w[1] && r[2] <= w[2], "{r:?} {w:?}");
        }
        assert!(
            out.data
                .chunks(4)
                .zip(white.data.chunks(4))
                .any(|(r, w)| r[1] < w[1])
        );
        let flat = json!({"texture": "none", "lightType": "point", "center": [0.0, 0.0]});
        let bump = json!({"texture": "luminance", "lightType": "point", "center": [0.0, 0.0]});
        assert_ne!(
            run("pcLightingEffects", flat, &src).data,
            run("pcLightingEffects", bump, &src).data
        );
        let left = run(
            "pcLightingEffects",
            json!({"lightType": "infinite", "texture": "luminance", "height": 100, "center": [-1.0, 0.0]}),
            &src,
        );
        let right = run(
            "pcLightingEffects",
            json!({"lightType": "infinite", "texture": "luminance", "height": 100, "center": [1.0, 0.0]}),
            &src,
        );
        assert_ne!(left.data, right.data);
    }

    /// Upstream `fibers_lie_between_colours_and_are_seeded`: with red foreground and blue
    /// background every pixel is a red/blue mix.
    #[test]
    fn fibers_use_only_primary_secondary_mixes() {
        let src = from_fn(40, 30, |_, _| [255, 255, 255, 10]);
        let mut env = EffectEnv::whole(40, 30);
        env.primary = [255, 0, 0, 255];
        env.secondary = [0, 0, 255, 255];
        // The defaults are the env's colours, not black and white.
        let a = run_env("pcFibers", json!({"seed": 1}), &src, &env);
        for p in a.data.chunks(4) {
            assert!(
                p[0].abs_diff(255 - p[2]) <= 1 && p[1] == 0 && p[3] == 255,
                "{p:?}"
            );
        }
        assert!(a.data.chunks(4).any(|p| p[0] > 200) && a.data.chunks(4).any(|p| p[2] > 200));
        // An explicit colour wins over the env's.
        let g = run_env(
            "pcFibers",
            json!({"foreground": "#00ff00", "background": "#00ff00"}),
            &src,
            &env,
        );
        assert!(g.data.chunks(4).all(|p| p == [0, 255, 0, 255]));
        // Swapping the env colours mirrors the picture.
        let (p, s) = (env.primary, env.secondary);
        env.primary = s;
        env.secondary = p;
        let b = run_env("pcFibers", json!({"seed": 1}), &src, &env);
        assert!(
            a.data
                .chunks(4)
                .zip(b.data.chunks(4))
                .all(|(x, y)| x[0].abs_diff(y[2]) <= 1)
        );
    }

    /// The Randomize row folds with the instance seed: equal seeds repeat, different ones
    /// differ, and row 0 under instance seed 7 is row 7 under instance seed 0.
    #[test]
    fn fibers_seed_row_is_folded_with_the_instance_seed() {
        let src = from_fn(40, 30, |_, _| [0, 0, 0, 255]);
        let a = run("pcFibers", json!({"seed": 1}), &src);
        assert_eq!(a.data, run("pcFibers", json!({"seed": 1}), &src).data);
        assert_ne!(a.data, run("pcFibers", json!({"seed": 2}), &src).data);
        let mut env = EffectEnv::whole(40, 30);
        env.instance_seed = 7;
        assert_eq!(
            run_env("pcFibers", json!({"seed": 0}), &src, &env).data,
            run("pcFibers", json!({"seed": 7}), &src).data
        );
    }

    /// Variance and strength change the streaks.
    #[test]
    fn fibers_parameters_matter() {
        let src = from_fn(40, 30, |_, _| [0, 0, 0, 255]);
        let base = run("pcFibers", json!({}), &src).data;
        assert_ne!(base, run("pcFibers", json!({"variance": 64}), &src).data);
        assert_ne!(base, run("pcFibers", json!({"strength": 40}), &src).data);
    }

    /// Centres, light positions and noise grids anchor on the selection bounds: inside a
    /// rectangular selection the result equals the effect run on the layer cropped to it, so an
    /// effect anchored on the layer's origin would fail.
    #[test]
    fn render_effects_anchor_on_selection_bounds() {
        let (w, h, l, t, r, b) = (48u32, 36u32, 8u32, 4u32, 32u32, 20u32);
        let full = from_fn(w, h, noise);
        let crop = from_fn(r - l, b - t, |x, y| noise(x + l, y + t));
        let (mask, polys) = rect_selection(w, h, l, t, r, b);
        let env = with_selection(w, h, &mask, &polys);
        let cases = [
            ("pcLensFlare", json!({"center": [-0.4, 0.3]})),
            ("pcLensFlare", json!({"lens": "moviePrime"})),
            (
                "pcLightingEffects",
                json!({"lightType": "point", "center": [0.2, 0.1]}),
            ),
            (
                "pcLightingEffects",
                json!({"lightType": "spot", "texture": "red"}),
            ),
            (
                "pcLightingEffects",
                json!({"lightType": "infinite", "texture": "luminance"}),
            ),
            ("pcFibers", json!({"seed": 9})),
        ];
        for (id, v) in cases {
            let a = run_env(id, v.clone(), &full, &env);
            let c = run(id, v.clone(), &crop);
            // Bump normals read neighbours across the selection edge, so textured Lighting
            // is compared one pixel in.
            let n = u32::from(v.get("texture").is_some());
            for (x, y) in (t + n..b - n).flat_map(|y| (l + n..r - n).map(move |x| (x, y))) {
                let (p, q) = (px(&a, x, y), px(&c, x - l, y - t));
                assert!(
                    (0..4).all(|i| p[i].abs_diff(q[i]) <= 1),
                    "{id} {v} at {x},{y}: {p:?} {q:?}"
                );
            }
            // And the render is not the layer-anchored one run over the whole layer.
            let whole = run(id, v.clone(), &full);
            let inside = |img: &crate::canvas::ImageData| -> Vec<[u8; 4]> {
                (t..b)
                    .flat_map(|y| (l..r).map(move |x| px(img, x, y)))
                    .collect()
            };
            assert_ne!(inside(&whole), inside(&a), "{id} {v}");
        }
    }

    /// A cancelled render leaves the layer untouched.
    #[test]
    fn render_effects_stop_when_cancelled() {
        let src = from_fn(32, 32, noise);
        let flag = AtomicBool::new(true);
        let mut env = EffectEnv::whole(32, 32);
        env.cancel = Some(&flag);
        for id in ["pcLensFlare", "pcLightingEffects", "pcFibers"] {
            assert_eq!(run_env(id, json!({}), &src, &env).data, src.data, "{id}");
        }
    }

    /// Lens Flare and Lighting Effects have a Pan row, so each carries an `IndirectUi`.
    #[test]
    fn pan_effects_have_indirect_ui() {
        for id in ["pcLensFlare", "pcLightingEffects"] {
            assert!(find(id).unwrap().indirect.is_some(), "{id}");
        }
        assert!(find("pcFibers").unwrap().indirect.is_none());
    }
}
