//! BB spec: effects/stylize-floyd-steinberg-dithering.md; common.md §2–5.

use super::classic::{brightness_contrast, intensity};
use super::conventions::DITHER_EMPTY_CURRENT_PALETTE;
use super::manifest::manifest;
use super::ui::{ControlUi, IndirectUi};
use crate::canvas::ImageData;
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, choice, color_value, num,
    range,
};

static UI: IndirectUi = IndirectUi {
    controls: &[
        ("palette", ControlUi::NO_HEADER),
        ("method", ControlUi::NO_HEADER),
        (
            "brightness",
            ControlUi {
                decimals: Some(0),
                ..ControlUi::DEFAULT
            },
        ),
        (
            "contrast",
            ControlUi {
                decimals: Some(0),
                ..ControlUi::NO_HEADER
            },
        ),
        (
            "count",
            ControlUi {
                decimals: Some(0),
                ..ControlUi::DEFAULT
            },
        ),
    ],
    enabled: |v, key| match key {
        "customColor" => choice(v, "palette") == "bwPlusOne",
        "count" => choice(v, "palette") == "current",
        _ => true,
    },
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let m = manifest("bbFloydSteinbergDithering").unwrap();
    vec![EffectDef {
        id: m.id,
        icon: m.icon,
        category: m.category,
        controls: vec![
            Control::Select {
                key: "palette",
                label: "",
                default: "bw",
                choices: &[
                    ("bw", "Black & White Palette"),
                    ("gray4", "4 Shades of Gray Palette"),
                    ("gray8", "8 Shades of Gray Palette"),
                    ("cmyk", "4 Colors CMYK Palette"),
                    ("windows16", "16 Color Original Windows Palette"),
                    ("custom16", "16 Color Custom Palette"),
                    ("pebble64", "Pebble Time 64 Color Palette"),
                    ("bwPlusOne", "Black & White Plus One Color"),
                    ("current", "Use Current Palette"),
                ],
            },
            Control::Select {
                key: "method",
                label: "",
                default: "fs",
                choices: &[
                    ("fs", "Floyd-Steinberg Dithering Method (1/16)"),
                    ("custom", "Custom Dithering Method (1/32)"),
                ],
            },
            range("brightness", "Brightness/Contrast", -100.0, 100.0, 0.0),
            range("contrast", "", -100.0, 100.0, 0.0),
            Control::Color {
                key: "customColor",
                label: "Custom Color",
                default: ColorDefault::Rgb(0xFF0000),
            },
            range(
                "count",
                "Number of colors to use from current palette",
                2.0,
                96.0,
                4.0,
            ),
        ],
        kind: EffectKind::Cpu {
            render: dither,
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some("Floyd-Steinberg Dithering"),
        indirect: Some(&UI),
    }]
}

// quirk(bb): stylize-floyd-steinberg-dithering.md Palettes: repeats and off-by-one entries.
pub const PEBBLE_64: [u32; 64] = [
    0x000000, 0x000055, 0x0000AA, 0x0000FF, 0x005500, 0x005555, 0x0055A9, 0x0055FF, 0x00AA00,
    0x00AA54, 0x00AAAA, 0x00AAFF, 0x00FF00, 0x01FF55, 0x01FFAA, 0x01FFFF, 0x550000, 0x550055,
    0x5500AA, 0x5500FF, 0x555501, 0x555555, 0x5555AA, 0x5555FF, 0x55AA00, 0x55AA55, 0x5555AA,
    0x55AAFF, 0x55FF00, 0x55FF55, 0x55FFA9, 0x55FFFF, 0xAA0000, 0xAA0055, 0xAA00AA, 0xAA01FF,
    0xAA5500, 0xA95555, 0xAA55AA, 0xAA55FF, 0xAAAA01, 0xAAAA55, 0xAAAAAA, 0xAAAAFF, 0xAAFF00,
    0xAAFF55, 0xAAFFAA, 0xAAAAFF, 0xFF0000, 0xFF0055, 0xFF00AA, 0xFF00FF, 0xFF5500, 0xFF5554,
    0xFF55AA, 0xFF54FF, 0xFFAA00, 0xFFAA55, 0xFFAAAA, 0xFFAAFF, 0xFFFF01, 0xFFFF55, 0xFFFFAA,
    0xFFFFFF,
];

/// Exact search order. Current palette deduplication compares RGBA before dropping alpha.
pub fn palette_for(choice: &str, env: &EffectEnv, count: usize) -> Vec<[u8; 3]> {
    if choice == "current" {
        // spec-question: Q38 Dither direct-helper empty current palette
        let source = if env.palette.is_empty() {
            DITHER_EMPTY_CURRENT_PALETTE
        } else {
            env.palette
        };
        let mut unique = Vec::new();
        for &color in source {
            if !unique.contains(&color) {
                unique.push(color);
            }
        }
        return unique
            .into_iter()
            .take(count)
            .map(|[r, g, b, _]| [r, g, b])
            .collect();
    }
    let table: &[u32] = match choice {
        "gray4" => &[0xFFFFFF, 0xC0C0C0, 0x808080, 0x000000],
        "gray8" => &[
            0x000000, 0x242424, 0x484848, 0x6C6C6C, 0x909090, 0xB4B4B4, 0xD8D8D8, 0xFFFFFF,
        ],
        "cmyk" => &[0xFFFFFF, 0x00FFFF, 0xFFFF00, 0xFF00FF, 0x000000],
        "windows16" => &[
            0xFFFFFF, 0xC0C0C0, 0x808080, 0x000000, 0xFF0000, 0x800000, 0xFFFF00, 0x808000,
            0x00FF00, 0x008000, 0x00FFFF, 0x008080, 0x0000FF, 0x000080, 0xFF00FF, 0x800080,
        ],
        "custom16" => &[
            0xFFFFFF, 0xD3D3D3, 0x4D4D4D, 0x000000, 0xFF0000, 0x800000, 0xFFFF00, 0xD2691E,
            0xA52A2A, 0x32CD32, 0x006400, 0x87CEFA, 0x0000FF, 0x2020D8, 0xFFC0FF, 0x800080,
        ],
        "pebble64" => &PEBBLE_64,
        "bwPlusOne" => &[0x000000, 0x808080, 0xFFFFFF],
        _ => &[0x000000, 0xFFFFFF],
    };
    table
        .iter()
        .map(|c| [(c >> 16) as u8, (c >> 8) as u8, *c as u8])
        .collect()
}

fn dither(src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    let selected = choice(values, "palette");
    let palette = palette_for(
        selected,
        env,
        num(values, "count").clamp(2.0, 96.0) as usize,
    );
    let mut work = brightness_contrast(
        src,
        num(values, "brightness").clamp(-100.0, 100.0) as i32,
        num(values, "contrast").clamp(-100.0, 100.0) as i32,
    );
    let custom = choice(values, "method") == "custom";
    for y in 0..work.height {
        for x in 0..work.width {
            let i = (y as usize * work.width as usize + x as usize) * 4;
            let mut p: [u8; 4] = work.data[i..i + 4].try_into().unwrap();
            if matches!(selected, "bw" | "gray4" | "gray8") {
                // Incoming error saturates RGB before this pixel's desaturation.
                let grey = intensity(p);
                p[..3].fill(grey);
            }
            let nearest = palette
                .iter()
                .min_by_key(|color| {
                    (0..3)
                        .map(|k| (p[k] as i32 - color[k] as i32).pow(2))
                        .sum::<i32>()
                })
                .unwrap();
            let error = std::array::from_fn(|k| p[k] as i32 - nearest[k] as i32);
            work.data[i..i + 3].copy_from_slice(nearest);
            diffuse(&mut work, x, y, error, custom);
        }
    }
    if selected == "bwPlusOne" {
        let [r, g, b, _] = color_value(values, "customColor", env);
        for pixel in work.data.as_chunks_mut::<4>().0 {
            // quirk(bb): stylize-floyd-steinberg-dithering.md step5 recolours only opaque gray.
            if *pixel == [128, 128, 128, 255] {
                *pixel = [r, g, b, 255];
            }
        }
    }
    work
}

fn diffuse(work: &mut ImageData, x: u32, y: u32, error: [i32; 3], custom: bool) {
    let (matrix, denominator): (&[(i32, u32, i32)], i32) = if custom {
        (
            &[
                (1, 0, 8),
                (2, 0, 4),
                (-1, 1, 4),
                (0, 1, 8),
                (1, 1, 4),
                (2, 1, 1),
                (0, 2, 2),
                (1, 2, 1),
            ],
            32,
        )
    } else {
        (&[(1, 0, 7), (-1, 1, 3), (0, 1, 5), (1, 1, 1)], 16)
    };
    for &(dx, dy, weight) in matrix {
        // quirk(bb): stylize-floyd-steinberg-dithering.md step4 "down-left share" requires x>1.
        if dx == -1 && x <= 1 {
            continue;
        }
        let Some(nx) = x.checked_add_signed(dx).filter(|&nx| nx < work.width) else {
            continue;
        };
        let Some(ny) = y.checked_add(dy).filter(|&ny| ny < work.height) else {
            continue;
        };
        let i = (ny as usize * work.width as usize + nx as usize) * 4;
        for (k, e) in error.into_iter().enumerate() {
            work.data[i + k] =
                (work.data[i + k] as i32 + e * weight / denominator).clamp(0, 255) as u8;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::canvas::ImageData;
    use crate::effect::bb::test_util::*;
    use crate::effect::{EffectEnv, find};
    use serde_json::json;

    const ID: &str = "bbFloydSteinbergDithering";
    const W: [u8; 4] = [255; 4];
    const B: [u8; 4] = [0, 0, 0, 255];
    fn grey(v: u8) -> [u8; 4] {
        [v, v, v, 255]
    }
    fn pixels(img: &ImageData) -> &[[u8; 4]] {
        img.data.as_chunks::<4>().0
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 1: both exact checkerboards.
    #[test]
    fn checkerboards() {
        for method in ["fs", "custom"] {
            assert_eq!(
                pixels(&bb(ID, json!({"method": method}), &solid(4, 4, grey(128)))),
                [W, B, W, B, B, W, B, W, W, B, W, B, B, W, B, W],
                "{method}"
            );
        }
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 2: raster-order row gradient.
    #[test]
    fn row_gradient() {
        let src = from_fn(8, 1, |x, _| grey((32 * x) as u8));
        assert_eq!(pixels(&bb(ID, json!({}), &src)), [B, B, B, W, B, W, W, W]);
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 3: column-one down-left is lost.
    #[test]
    fn down_left_boundary() {
        let src = from_fn(2, 2, |x, y| {
            grey([[0, 128], [140, 0]][y as usize][x as usize])
        });
        assert_eq!(pixels(&bb(ID, json!({}), &src)), [B, W, W, B]);
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 4: exact opaque-only Plus One.
    #[test]
    fn plus_one() {
        for method in ["fs", "custom"] {
            for alpha in [0, 1, 128, 254, 255] {
                let src = solid(8, 8, [128, 128, 128, alpha]);
                let out = bb(
                    ID,
                    json!({"method":method,"palette":"bwPlusOne","customColor":"#00ff00"}),
                    &src,
                );
                let want = if alpha == 255 {
                    [0, 255, 0, 255]
                } else {
                    [128, 128, 128, alpha]
                };
                assert_eq!(pixels(&out), vec![want; 64]);
            }
        }
        // Yellow is nearest RGB gray, but its luma would choose white if Plus One desaturated.
        assert_eq!(
            pixels(&bb(
                ID,
                json!({"palette":"bwPlusOne","customColor":"#00ff00"}),
                &solid(1, 1, [255, 255, 0, 255])
            )),
            [[0, 255, 0, 255]]
        );
        let mut env = EffectEnv::whole(1, 1);
        env.primary = [12, 34, 56, 7];
        assert_eq!(
            pixels(&bb_env(
                ID,
                json!({"palette":"bwPlusOne","customColor":"primary"}),
                &solid(1, 1, grey(128)),
                &env
            )),
            [[12, 34, 56, 255]]
        );
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 5: every palette member and black.
    #[test]
    fn palette_members_and_transparent_black() {
        let env = EffectEnv::whole(1, 1);
        for palette in [
            "bw",
            "gray4",
            "gray8",
            "cmyk",
            "windows16",
            "custom16",
            "pebble64",
            "bwPlusOne",
            "current",
        ] {
            let colors = super::palette_for(palette, &env, 96);
            for color in colors {
                if palette == "bwPlusOne" && color == [128; 3] {
                    continue;
                }
                let src = solid(2, 3, [color[0], color[1], color[2], 255]);
                for method in ["fs", "custom"] {
                    assert_eq!(
                        bb(
                            ID,
                            json!({"palette":palette,"method":method,"count":96}),
                            &src
                        )
                        .data,
                        src.data,
                        "{palette} {color:?}"
                    );
                }
            }
            let src = solid(4, 4, [0; 4]);
            assert_eq!(
                bb(ID, json!({"palette":palette}), &src).data,
                src.data,
                "{palette}"
            );
        }
        let src = from_fn(4, 4, |x, _| {
            if x < 2 {
                [255, 0, 0, 255]
            } else {
                [0, 0, 128, 255]
            }
        });
        assert_eq!(bb(ID, json!({"palette":"windows16"}), &src).data, src.data);
    }

    /// stylize-floyd-steinberg-dithering.md Expected results 6: both brightness100 outputs.
    #[test]
    fn brightness() {
        for method in ["fs", "custom"] {
            assert_eq!(
                pixels(&bb(
                    ID,
                    json!({"method":method,"brightness":100}),
                    &solid(4, 4, grey(128))
                )),
                [W; 16]
            );
        }
        // Non-gray data, B/C threshold and signed pre-adjust occur before quantization.
        let src = from_fn(2, 1, |x, _| {
            if x == 0 {
                [200, 40, 10, 9]
            } else {
                [10, 210, 40, 177]
            }
        });
        assert_eq!(
            pixels(&bb(ID, json!({"contrast":100}), &src)),
            [[0, 0, 0, 9], [255, 255, 255, 177]]
        );
    }

    /// stylize-floyd-steinberg-dithering.md current palette: stable RGBA dedup before the first
    /// N are taken.
    #[test]
    fn current_palette() {
        let mut env = EffectEnv::whole(3, 1);
        let palette = [
            [0, 0, 0, 255],
            [0, 0, 0, 255],
            [0, 0, 0, 0],
            [255, 255, 255, 255],
            [255, 0, 0, 255],
        ];
        env.palette = &palette;
        assert_eq!(super::palette_for("current", &env, 2), [[0; 3], [0; 3]]);
        assert_eq!(
            super::palette_for("current", &env, 3),
            [[0; 3], [0; 3], [255; 3]]
        );
        assert_eq!(
            super::palette_for("current", &env, 96),
            [[0; 3], [0; 3], [255; 3], [255, 0, 0]]
        );
        let one = [[12, 34, 56, 7]];
        env.palette = &one;
        assert_eq!(super::palette_for("current", &env, 96), [[12, 34, 56]]);
        assert_eq!(
            pixels(&bb_env(
                ID,
                json!({"palette":"current","count":96}),
                &solid(3, 1, [90, 80, 70, 128]),
                &env
            )),
            [[12, 34, 56, 128]; 3]
        );
        env.palette = &[];
        assert_eq!(
            super::palette_for("current", &env, 96),
            super::palette_for("current", &EffectEnv::whole(3, 1), 96)
        );
        env.palette = &[[0, 0, 0, 255], [255; 4]];
        let src = from_fn(8, 1, |x, _| grey((x * 32) as u8));
        assert_eq!(
            pixels(&bb_env(
                ID,
                json!({"palette":"current","count":2}),
                &src,
                &env
            )),
            [B, B, B, W, B, W, W, W]
        );
    }

    /// stylize-floyd-steinberg-dithering.md palette tables: full search order including quirks.
    #[test]
    fn palette_tables() {
        let env = EffectEnv::whole(1, 1);
        for (name, hex) in [
            ("bw", vec![0x000000, 0xFFFFFF]),
            ("gray4", vec![0xFFFFFF, 0xC0C0C0, 0x808080, 0x000000]),
            (
                "gray8",
                vec![
                    0x000000, 0x242424, 0x484848, 0x6C6C6C, 0x909090, 0xB4B4B4, 0xD8D8D8, 0xFFFFFF,
                ],
            ),
            (
                "cmyk",
                vec![0xFFFFFF, 0x00FFFF, 0xFFFF00, 0xFF00FF, 0x000000],
            ),
            (
                "windows16",
                vec![
                    0xFFFFFF, 0xC0C0C0, 0x808080, 0x000000, 0xFF0000, 0x800000, 0xFFFF00, 0x808000,
                    0x00FF00, 0x008000, 0x00FFFF, 0x008080, 0x0000FF, 0x000080, 0xFF00FF, 0x800080,
                ],
            ),
            (
                "custom16",
                vec![
                    0xFFFFFF, 0xD3D3D3, 0x4D4D4D, 0x000000, 0xFF0000, 0x800000, 0xFFFF00, 0xD2691E,
                    0xA52A2A, 0x32CD32, 0x006400, 0x87CEFA, 0x0000FF, 0x2020D8, 0xFFC0FF, 0x800080,
                ],
            ),
            (
                "pebble64",
                vec![
                    0x000000, 0x000055, 0x0000AA, 0x0000FF, 0x005500, 0x005555, 0x0055A9, 0x0055FF,
                    0x00AA00, 0x00AA54, 0x00AAAA, 0x00AAFF, 0x00FF00, 0x01FF55, 0x01FFAA, 0x01FFFF,
                    0x550000, 0x550055, 0x5500AA, 0x5500FF, 0x555501, 0x555555, 0x5555AA, 0x5555FF,
                    0x55AA00, 0x55AA55, 0x5555AA, 0x55AAFF, 0x55FF00, 0x55FF55, 0x55FFA9, 0x55FFFF,
                    0xAA0000, 0xAA0055, 0xAA00AA, 0xAA01FF, 0xAA5500, 0xA95555, 0xAA55AA, 0xAA55FF,
                    0xAAAA01, 0xAAAA55, 0xAAAAAA, 0xAAAAFF, 0xAAFF00, 0xAAFF55, 0xAAFFAA, 0xAAAAFF,
                    0xFF0000, 0xFF0055, 0xFF00AA, 0xFF00FF, 0xFF5500, 0xFF5554, 0xFF55AA, 0xFF54FF,
                    0xFFAA00, 0xFFAA55, 0xFFAAAA, 0xFFAAFF, 0xFFFF01, 0xFFFF55, 0xFFFFAA, 0xFFFFFF,
                ],
            ),
            ("bwPlusOne", vec![0x000000, 0x808080, 0xFFFFFF]),
        ] {
            let got: Vec<_> = super::palette_for(name, &env, 4)
                .into_iter()
                .map(|[r, g, b]| (r as u32) << 16 | (g as u32) << 8 | b as u32)
                .collect();
            assert_eq!(got, hex, "{name}");
        }
        assert_eq!(super::PEBBLE_64.len(), 64);
    }

    /// stylize-floyd-steinberg-dithering.md step3: earliest ties, independent RGB, hidden color.
    #[test]
    fn nearest_ties_alpha_and_hidden_rgb() {
        let mut env = EffectEnv::whole(2, 1);
        let palette = [[0, 0, 0, 7], [2, 0, 0, 9]];
        env.palette = &palette;
        assert_eq!(
            pixels(&bb_env(
                ID,
                json!({"palette":"current","count":2}),
                &solid(1, 1, [1, 0, 0, 128]),
                &env
            )),
            [[0, 0, 0, 128]]
        );
        env.palette = &[[2, 0, 0, 9], [0, 0, 0, 7]];
        assert_eq!(
            pixels(&bb_env(
                ID,
                json!({"palette":"current","count":2}),
                &solid(1, 1, [1, 0, 0, 0]),
                &env
            )),
            [[2, 0, 0, 0]]
        );
        // Hidden 128 emits -55 to its neighbor, changing opaque128 from white to black.
        for method in ["fs", "custom"] {
            let src = from_fn(2, 1, |x, _| {
                if x == 0 {
                    [128, 128, 128, 0]
                } else {
                    grey(128)
                }
            });
            assert_eq!(
                pixels(&bb(ID, json!({"method":method}), &src)),
                [[255, 255, 255, 0], B]
            );
            for alpha in [0, 1, 128, 255] {
                let src = solid(4, 3, [91, 142, 201, alpha]);
                let out = bb(ID, json!({"method":method,"palette":"custom16"}), &src);
                let opaque = bb(
                    ID,
                    json!({"method":method,"palette":"custom16"}),
                    &solid(4, 3, [91, 142, 201, 255]),
                );
                for (a, b) in pixels(&out).iter().zip(pixels(&opaque)) {
                    assert_eq!(&a[..3], &b[..3]);
                    assert_eq!(a[3], alpha);
                }
            }
        }
    }

    /// stylize-floyd-steinberg-dithering.md step3: desaturate only after incoming saturated RGB.
    #[test]
    fn desaturation_timing() {
        for (method, second) in [("fs", 27), ("custom", 47)] {
            let src = from_fn(2, 1, |x, _| {
                if x == 0 {
                    [255, 0, 0, 255]
                } else {
                    [255, second, second, 255]
                }
            });
            assert_eq!(pixels(&bb(ID, json!({"method":method}), &src)), [B, B]);
        }
        for (palette, want) in [
            ("bw", B),
            ("gray4", grey(128)),
            ("gray8", grey(72)),
            ("windows16", [255, 0, 0, 255]),
        ] {
            assert_eq!(
                pixels(&bb(
                    ID,
                    json!({"palette":palette}),
                    &solid(1, 1, [255, 0, 0, 255])
                )),
                [want]
            );
        }
    }

    /// stylize-floyd-steinberg-dithering.md steps2–4: every matrix offset, signed truncation.
    #[test]
    fn diffusion_shares_and_saturation() {
        for (custom, expected) in [
            (
                false,
                vec![
                    (3, 1, [183, 73, 141, 19]),
                    (1, 2, [151, 105, 133, 19]),
                    (2, 2, [167, 89, 137, 19]),
                    (3, 2, [135, 121, 129, 19]),
                ],
            ),
            (
                true,
                vec![
                    (3, 1, [159, 97, 135, 19]),
                    (4, 1, [143, 113, 131, 19]),
                    (1, 2, [143, 113, 131, 19]),
                    (2, 2, [159, 97, 135, 19]),
                    (3, 2, [143, 113, 131, 19]),
                    (4, 2, [131, 125, 128, 19]),
                    (2, 3, [135, 121, 129, 19]),
                    (3, 3, [131, 125, 128, 19]),
                ],
            ),
        ] {
            let mut work = solid(5, 5, [128, 128, 128, 19]);
            super::diffuse(&mut work, 2, 1, [127, -127, 31], custom);
            for y in 0..5 {
                for x in 0..5 {
                    let want = expected
                        .iter()
                        .find(|(a, b, _)| *a == x && *b == y)
                        .map_or([128, 128, 128, 19], |(_, _, p)| *p);
                    assert_eq!(px(&work, x, y), want, "{custom} {x},{y}");
                }
            }
            // Column-one must not diffuse into column zero; other shares saturate immediately.
            let mut work = solid(3, 2, [250, 5, 128, 0]);
            super::diffuse(&mut work, 1, 0, [127, -127, -3], custom);
            assert_eq!(px(&work, 0, 1), [250, 5, 128, 0]);
            assert_eq!(px(&work, 2, 0), [255, 0, if custom { 128 } else { 127 }, 0]);
            // Off-canvas shares are discarded, never renormalized or wrapped.
            let mut work = solid(1, 1, [1, 2, 3, 4]);
            super::diffuse(&mut work, 0, 0, [255, -255, 127], custom);
            assert_eq!(pixels(&work), [[1, 2, 3, 4]]);
        }
    }

    /// stylize-floyd-steinberg-dithering.md selection: whole layer first, one fractional clip.
    #[test]
    fn selection_and_cpu_determinism() {
        use crate::effect::{
            RenderInfo,
            surface::{Alpha, Space},
        };
        use crate::pixel::ops::blend;
        let src = solid(4, 4, grey(128));
        let (mut mask, polys) = rect_selection(4, 4, 1, 1, 4, 4);
        mask.data[6] = 128;
        let env = with_selection(4, 4, &mask, &polys);
        for method in ["fs", "custom"] {
            let row = solid(2, 1, grey(128));
            let (row_mask, row_polys) = rect_selection(2, 1, 1, 0, 2, 1);
            let row_env = with_selection(2, 1, &row_mask, &row_polys);
            assert_eq!(
                pixels(&bb_env(ID, json!({"method":method}), &row, &row_env)),
                [grey(128), B]
            );
            let changed = from_fn(2, 1, |x, _| if x == 0 { B } else { grey(128) });
            assert_eq!(
                pixels(&bb_env(ID, json!({"method":method}), &changed, &row_env)),
                [B, W]
            );
            let whole = bb(ID, json!({"method":method}), &src);
            let selected = bb_env(ID, json!({"method":method}), &src, &env);
            for y in 0..4 {
                for x in 0..4 {
                    assert_eq!(
                        px(&selected, x, y),
                        blend(
                            px(&src, x, y),
                            px(&whole, x, y),
                            mask.get(x as i32, y as i32)
                        )
                    );
                }
            }
            assert_eq!(
                px(&selected, 1, 1),
                W,
                "unselected pixels emit error into selection"
            );
            for (w, h) in [(0, 0), (0, 3), (3, 0), (1, 1), (1, 7), (7, 1), (2, 2)] {
                let src = from_fn(w, h, |x, y| {
                    [(x * 71 + 51) as u8, (y * 37 + 102) as u8, 153, 128]
                });
                let a = bb(ID, json!({"method":method}), &src);
                for info in [
                    RenderInfo::new(Space::Srgb, Alpha::Straight),
                    RenderInfo::new(Space::Linear, Alpha::Premultiplied),
                ] {
                    let env = EffectEnv {
                        render_info: Some(info),
                        ..EffectEnv::whole(w, h)
                    };
                    assert_eq!(
                        bb_env(ID, json!({"method":method}), &src, &env).data,
                        a.data
                    );
                }
            }
        }
    }

    /// stylize-floyd-steinberg-dithering.md controls: exact metadata, enable rules, stored values.
    #[test]
    fn presentation() {
        use crate::effect::bb::ui::{Help, HelpWindow};
        use crate::effect::{Category, ColorDefault, Control, EffectKind};
        let def = find(ID).unwrap();
        assert_eq!(def.dialog_title, Some("Floyd-Steinberg Dithering"));
        assert_eq!(def.category, Category::Effect("stylize"));
        assert_eq!(def.icon, "floyd_steinberg_dithering.png");
        assert!(matches!(
            def.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        assert!(def.dialog.is_none());
        assert_eq!(
            def.controls.iter().map(Control::key).collect::<Vec<_>>(),
            [
                "palette",
                "method",
                "brightness",
                "contrast",
                "customColor",
                "count"
            ]
        );
        let ui = def.indirect.unwrap();
        assert_eq!(ui.width, 1.0);
        assert!(ui.tabs.is_empty() && ui.transient_keys.is_empty());
        assert!(matches!(ui.help, Help::Window(HelpWindow::Standard)));
        for (i, key, label, min, max, default) in [
            (2, "brightness", "Brightness/Contrast", -100., 100., 0.),
            (3, "contrast", "", -100., 100., 0.),
            (
                5,
                "count",
                "Number of colors to use from current palette",
                2.,
                96.,
                4.,
            ),
        ] {
            let Control::Range {
                key: k,
                label: l,
                min: a,
                max: b,
                default: d,
                step,
                keyboard,
                ..
            } = def.controls[i]
            else {
                panic!("range")
            };
            assert_eq!((k, l, a, b, d, step), (key, label, min, max, default, 1.));
            assert_eq!(
                (
                    keyboard.decimals,
                    keyboard.small,
                    keyboard.large,
                    keyboard.exponential
                ),
                (0, 1., 5., false)
            );
            assert_eq!(ui.control(key).decimals, Some(0));
        }
        assert!(matches!(
            def.controls[4],
            Control::Color {
                key: "customColor",
                label: "Custom Color",
                default: ColorDefault::Rgb(0xFF0000)
            }
        ));
        for key in ["palette", "method", "contrast"] {
            assert!(!ui.control(key).header_line);
        }
        for key in ["brightness", "customColor", "count"] {
            assert!(ui.control(key).header_line);
        }
        for c in &def.controls {
            let c = ui.control(c.key());
            assert!(!c.alpha && !c.wrap && c.reset);
        }
        for (i, default, want) in [
            (
                0,
                "bw",
                vec![
                    ("bw", "Black & White Palette"),
                    ("gray4", "4 Shades of Gray Palette"),
                    ("gray8", "8 Shades of Gray Palette"),
                    ("cmyk", "4 Colors CMYK Palette"),
                    ("windows16", "16 Color Original Windows Palette"),
                    ("custom16", "16 Color Custom Palette"),
                    ("pebble64", "Pebble Time 64 Color Palette"),
                    ("bwPlusOne", "Black & White Plus One Color"),
                    ("current", "Use Current Palette"),
                ],
            ),
            (
                1,
                "fs",
                vec![
                    ("fs", "Floyd-Steinberg Dithering Method (1/16)"),
                    ("custom", "Custom Dithering Method (1/32)"),
                ],
            ),
        ] {
            let Control::Select {
                label,
                choices,
                default: d,
                ..
            } = &def.controls[i]
            else {
                panic!("select")
            };
            assert_eq!((*label, *d), ("", default));
            assert_eq!(*choices, want);
        }
        let mut v = crate::effect::defaults(&def);
        v.insert("customColor".into(), json!("#123456"));
        v.insert("count".into(), json!(17));
        for palette in ["bw", "bwPlusOne", "current", "gray4"] {
            v.insert("palette".into(), json!(palette));
            assert_eq!((ui.enabled)(&v, "customColor"), palette == "bwPlusOne");
            assert_eq!((ui.enabled)(&v, "count"), palette == "current");
            assert_eq!(v["customColor"], json!("#123456"));
            assert_eq!(v["count"], json!(17));
            for key in ["palette", "method", "brightness", "contrast"] {
                assert!((ui.enabled)(&v, key));
            }
        }
    }
}
