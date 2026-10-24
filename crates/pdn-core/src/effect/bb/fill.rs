//! BB spec: effects/fill-from-clipboard.md; effects/fill-from-file.md; effects/object-paste-alpha.md; common.md §1–4, §5.11

use super::ui::{ControlUi, External, IndirectUi};
use super::{
    classic::intensity,
    conventions::{GPU_CONTEXT, PASTE_ALPHA_BILINEAR_BYTE_POLICY},
    manifest::manifest,
    pan,
};
use crate::canvas::ImageData;
use crate::effect::distort::{catmull_rom, filter4};
use crate::effect::slider::SliderKeys;
use crate::effect::surface::{Alpha, Edge, EffectSurface};
use crate::effect::{
    Control, CpuFn, EffectDef, EffectEnv, EffectKind, PanThumb, RenderInfo, Values, choice, flag,
    num, range,
};
use crate::pixel::{IntRect, Px};

const PAN_UI: ControlUi = ControlUi {
    decimals: Some(3),
    pan_sliders: Some(SliderKeys {
        decimals: 3,
        small: 0.05,
        large: 0.25,
        exponential: false,
    }),
    ..ControlUi::DEFAULT
};
static FILL_UI: IndirectUi = IndirectUi {
    external_for: |_| External::Clipboard,
    controls: &[("placement", PAN_UI), ("zoom", ControlUi::NO_HEADER)],
    ..IndirectUi::DEFAULT
};
static FILE_UI: IndirectUi = IndirectUi {
    external_for: |_| External::File("file"),
    controls: &[
        ("file", ControlUi::NO_HEADER),
        ("placement", PAN_UI),
        ("zoom", ControlUi::NO_HEADER),
    ],
    ..IndirectUi::DEFAULT
};
static ALPHA_UI: IndirectUi = IndirectUi {
    external_for: |_| External::Clipboard,
    controls: &[("placement", PAN_UI)],
    ..IndirectUi::DEFAULT
};

/// Clipboard and file image effects, in manifest order.
pub fn effects() -> Vec<EffectDef> {
    let def = |id, title, controls, render: CpuFn, indirect| {
        let m = manifest(id).expect("registered fill effect");
        EffectDef {
            id,
            category: m.category,
            icon: m.icon,
            controls,
            kind: EffectKind::Cpu {
                render,
                clip_to_selection: true,
            },
            dialog: None,
            dialog_title: Some(title),
            indirect: Some(indirect),
        }
    };
    let placement = |label, min, max, default, thumb| Control::Pan {
        key: "placement",
        label,
        min,
        max,
        default,
        step: 0.01,
        thumb,
    };
    let nearest = || Control::Checkbox {
        key: "nearest",
        label: "Nearest neighbor sampling",
        default: false,
    };
    let layout = || {
        vec![
            placement("", -1.0, 1.0, [0.0; 2], PanThumb::White),
            range("zoom", "Zoom", 0.06, 16.0, 1.0)
                .step(0.01)
                .slider_keys(3, 0.05, 0.25, true)
                .exponential(),
            nearest(),
            Control::Select {
                key: "tiling",
                label: "",
                default: "one",
                choices: &[
                    ("one", "No Tiling - One copy only"),
                    ("wrap", "Tiling - No Mirroring"),
                    ("mirrorX", "Tiling with Horizontal Mirror"),
                    ("mirrorY", "Tiling with Vertical Mirror"),
                    ("mirrorBoth", "Tiling and Mirror Both Directions"),
                ],
            },
        ]
    };
    let file = Control::File {
        key: "file",
        label: "File to load",
        filters: &["png", "jpg", "gif", "bmp"],
        all_files: true,
    };
    vec![
        def(
            "bbFillFromClipboard",
            "Fill From Clipboard",
            layout(),
            fill,
            &FILL_UI,
        ),
        def(
            "bbFillFromFile",
            "Fill From File",
            std::iter::once(file).chain(layout()).collect(),
            fill_file,
            &FILE_UI,
        ),
        def(
            "bbPasteAlpha",
            "Paste Alpha",
            vec![
                Control::Select {
                    key: "method",
                    label: "",
                    default: "replace",
                    choices: &[
                        ("replace", "Replace current alpha with"),
                        ("minimum", "Minimum of current alpha and"),
                        ("multiply", "Multiply current alpha with"),
                    ],
                },
                Control::Select {
                    key: "source",
                    label: "",
                    default: "gray",
                    choices: &[
                        ("gray", "Shades of gray from clipboard"),
                        ("alpha", "Alpha channel from clipboard"),
                    ],
                },
                placement("Placement", -2.0, 2.0, [-1.0; 2], PanThumb::SelectionCrop),
                range("zoom", "Zoom", 0.01, 10.0, 1.0)
                    .step(0.01)
                    .slider_keys(3, 0.05, 0.25, false),
                nearest(),
                Control::Checkbox {
                    key: "invert",
                    label: "Invert calculation",
                    default: false,
                },
            ],
            paste_alpha,
            &ALPHA_UI,
        ),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tiling {
    One,
    Wrap,
    MirrorX,
    MirrorY,
    MirrorBoth,
}

pub struct FillParams {
    pub pan: [f64; 2],
    pub zoom: f64,
    pub nearest: bool,
    pub tiling: Tiling,
    /// From File uses selection-sized placement units for tiles (BB27).
    pub tile_units_selection: bool,
}

fn fill(base: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    fill_from(base, v, env, false)
}

fn fill_file(base: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    // quirk(bb): fill-from-file.md Behaviour "Tiled options": W and H are the selection's.
    fill_from(base, v, env, true)
}

fn fill_from(
    base: &ImageData,
    v: &Values,
    env: &EffectEnv,
    tile_units_selection: bool,
) -> ImageData {
    let Some(image) = env.image else {
        return base.clone();
    };
    render_fill(
        base,
        image,
        env.bounds,
        &FillParams {
            pan: pan(v, "placement"),
            zoom: num(v, "zoom"),
            nearest: flag(v, "nearest"),
            tiling: match choice(v, "tiling") {
                "wrap" => Tiling::Wrap,
                "mirrorX" => Tiling::MirrorX,
                "mirrorY" => Tiling::MirrorY,
                "mirrorBoth" => Tiling::MirrorBoth,
                _ => Tiling::One,
            },
            tile_units_selection,
        },
    )
}

/// fill-from-file.md "Loading the file": an existing file loads only when a built-in type takes
/// its final name component's extension (aliases only, never the content); otherwise
/// "‹ext› is not supported.", the extension as written with its dot ("" when there is none).
pub fn admit_file(name: &str) -> Result<(), String> {
    let file = std::path::Path::new(name)
        .file_name()
        .map_or_else(Default::default, |f| f.to_string_lossy());
    // .NET's GetExtension: a trailing dot is no extension.
    let ext = file.rfind('.').map_or("", |i| &file[i..]);
    let ext = if ext == "." { "" } else { ext };
    match crate::io::codecs::detect_format(ext, None, &[]) {
        Some(_) => Ok(()),
        None => Err(format!("{ext} is not supported.")),
    }
}

/// Draws over `base` in the pack's GPU context; selection coverage is applied by the caller.
pub fn render_fill(
    base: &ImageData,
    image: &ImageData,
    bounds: IntRect,
    p: &FillParams,
) -> ImageData {
    render_fill_context(base, image, bounds, p, GPU_CONTEXT)
}

fn render_fill_context(
    base: &ImageData,
    image: &ImageData,
    bounds: IntRect,
    p: &FillParams,
    info: RenderInfo,
) -> ImageData {
    if image.width == 0 || image.height == 0 || p.zoom <= 0.0 || !p.zoom.is_finite() {
        return base.clone();
    }
    let src = EffectSurface::from_rgba8(image, info.space, Alpha::Premultiplied);
    let mut dst = EffectSurface::from_rgba8(base, info.space, Alpha::Premultiplied);
    let center = [
        bounds.x as f64 + (bounds.w / 2) as f64,
        bounds.y as f64 + (bounds.h / 2) as f64,
    ];
    let size = [image.width as f64, image.height as f64];
    let half = [(image.width / 2) as f64, (image.height / 2) as f64];
    let selection = [bounds.w as f64, bounds.h as f64];
    let one = p.tiling == Tiling::One;
    let origin: [f64; 2] = std::array::from_fn(|i| {
        if one {
            center[i] + p.zoom * (p.pan[i] * selection[i] - half[i])
        } else {
            center[i]
                + p.pan[i]
                    * p.zoom
                    * (if p.tile_units_selection {
                        selection[i]
                    } else {
                        size[i]
                    })
                - half[i]
        }
    });
    let axis = |i: i32, n: u32, mirror: bool| {
        // quirk(bb): fill-from-clipboard.md "Mirroring follows Direct2D's mirror extend mode".
        let edge = if one {
            Edge::Clamp
        } else if mirror {
            Edge::Mirror
        } else {
            Edge::Wrap
        };
        match edge {
            Edge::Clamp => i.clamp(0, n as i32 - 1),
            Edge::Wrap => i.rem_euclid(n as i32),
            Edge::Mirror => {
                let k = i.rem_euclid(2 * n as i32);
                if k < n as i32 {
                    k
                } else {
                    2 * n as i32 - 1 - k
                }
            }
            Edge::Transparent => unreachable!(),
        }
    };
    let fetch = |x, y| {
        src.at(
            axis(
                x,
                src.w,
                matches!(p.tiling, Tiling::MirrorX | Tiling::MirrorBoth),
            ) as u32,
            axis(
                y,
                src.h,
                matches!(p.tiling, Tiling::MirrorY | Tiling::MirrorBoth),
            ) as u32,
        )
    };
    let b = bounds.intersect(&IntRect::new(0, 0, base.width, base.height));
    let mut touched = Vec::new();
    for y in b.y..b.bottom() {
        for x in b.x..b.right() {
            let s = [
                ((x as f64 + 0.5 - origin[0]) / p.zoom) as f32,
                ((y as f64 + 0.5 - origin[1]) / p.zoom) as f32,
            ];
            if one && (s[0] < 0.0 || s[1] < 0.0 || s[0] >= size[0] as f32 || s[1] >= size[1] as f32)
            {
                continue;
            }
            let sample = if p.nearest {
                fetch(s[0].floor() as i32, s[1].floor() as i32)
            }
            // approximation: D2D high-quality cubic (fill-from-clipboard.md "Resampling")
            else {
                filter4(s, catmull_rom, fetch)
            };
            if sample[3] <= 0.0 {
                continue;
            }
            let i = y as usize * base.width as usize + x as usize;
            dst.px[i] = super::blocks::source_over(dst.px[i], sample);
            touched.push(i);
        }
    }
    let rendered = dst.to_rgba8(info.space, Alpha::Premultiplied);
    let mut out = base.clone();
    for i in touched {
        out.data[i * 4..i * 4 + 4].copy_from_slice(&rendered.data[i * 4..i * 4 + 4]);
    }
    out
}

/// Integer-centred, wrapped Paint.NET classic mask sampling.
pub fn paste_alpha_sample(img: &ImageData, x: f32, y: f32, nearest: bool) -> Px {
    if img.width == 0 || img.height == 0 {
        return [0; 4];
    }
    let at = |x: i32, y: i32| {
        let i = (y.rem_euclid(img.height as i32) as usize * img.width as usize
            + x.rem_euclid(img.width as i32) as usize)
            * 4;
        img.data[i..i + 4].try_into().unwrap()
    };
    if nearest {
        return at(x.round_ties_even() as i32, y.round_ties_even() as i32);
    }
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (((x - ix) * 256.0) as u64, ((y - iy) * 256.0) as u64);
    let taps = [
        (0, 0, (256 - fx) * (256 - fy)),
        (1, 0, fx * (256 - fy)),
        (0, 1, (256 - fx) * fy),
        (1, 1, fx * fy),
    ];
    let mut alpha = 0u64;
    let mut rgb = [0u64; 3];
    for (dx, dy, w) in taps {
        let p: Px = at(ix as i32 + dx, iy as i32 + dy);
        let aw = p[3] as u64 * w;
        alpha += aw;
        for c in 0..3 {
            rgb[c] += p[c] as u64 * aw;
        }
    }
    if alpha == 0 {
        return [0; 4];
    }
    // spec-question: Q75 Paste Alpha bilinear byte conversion
    PASTE_ALPHA_BILINEAR_BYTE_POLICY(rgb, alpha)
}

fn paste_alpha(base: &ImageData, v: &Values, env: &EffectEnv) -> ImageData {
    let Some(image) = env.image else {
        return base.clone();
    };
    if image.width == 0 || image.height == 0 {
        return base.clone();
    }
    let [px, py] = pan(v, "placement").map(|p| p as f32);
    let zoom = num(v, "zoom") as f32;
    // quirk(bb): object-paste-alpha.md "Width and height really are swapped".
    let shift = [
        (px + 1.0) / 2.0 * image.height as f32,
        (py + 1.0) / 2.0 * image.width as f32,
    ];
    let mut out = base.clone();
    for y in 0..base.height {
        for x in 0..base.width {
            let s = paste_alpha_sample(
                image,
                (x as f32 + shift[0]) / zoom,
                (y as f32 + shift[1]) / zoom,
                flag(v, "nearest"),
            );
            let mut m = if choice(v, "source") == "alpha" {
                s[3]
            } else {
                intensity(s)
            };
            if flag(v, "invert") {
                m = 255 - m;
            }
            let a = &mut out.data[(y as usize * base.width as usize + x as usize) * 4 + 3];
            *a = match choice(v, "method") {
                "minimum" => (*a).min(m),
                "multiply" => ((m as u32 * (*a) as u32 + 127) / 255) as u8,
                _ => m,
            };
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::EffectEnv;
    use crate::effect::bb::test_util::*;
    use serde_json::json;
    const R: [u8; 4] = [255, 0, 0, 255];
    const G: [u8; 4] = [0, 255, 0, 255];
    const B: [u8; 4] = [0, 0, 255, 255];
    const K: [u8; 4] = [0, 0, 0, 255];
    const W: [u8; 4] = [255; 4];
    fn clip() -> crate::canvas::ImageData {
        from_fn(2, 2, |x, y| [[R, G], [B, K]][y as usize][x as usize])
    }
    fn map(rows: &[&str]) -> Vec<u8> {
        rows.iter()
            .flat_map(|r| {
                r.chars().flat_map(|c| match c {
                    'R' => R,
                    'G' => G,
                    'B' => B,
                    'K' => K,
                    'W' => W,
                    _ => panic!("color"),
                })
            })
            .collect()
    }
    fn fill_with(id: &str, v: serde_json::Value) -> crate::canvas::ImageData {
        let c = clip();
        let mut env = EffectEnv::whole(8, 8);
        env.image = Some(&c);
        let mut v = v;
        if v.get("nearest").is_none() {
            v["nearest"] = json!(true);
        }
        bb_env(id, v, &solid(8, 8, W), &env)
    }
    fn fill(v: serde_json::Value) -> crate::canvas::ImageData {
        fill_with("bbFillFromClipboard", v)
    }
    fn file(v: serde_json::Value) -> crate::canvas::ImageData {
        fill_with("bbFillFromFile", v)
    }
    /// fill-from-file.md Expected results 1: no image (empty or missing path) changes no byte;
    /// the UI consumer tests resolve the empty, missing and directory paths themselves.
    #[test]
    fn file_1_no_image() {
        for path in ["", "/no/such/file.png"] {
            assert_eq!(
                bb_env(
                    "bbFillFromFile",
                    json!({ "file": path }),
                    &solid(8, 8, W),
                    &EffectEnv::whole(8, 8)
                )
                .data,
                map(&["WWWWWWWW"; 8])
            );
        }
        let src = from_fn(8, 8, |x, y| {
            [x as u8, y as u8, 91, if x % 2 == 0 { 0 } else { 200 }]
        });
        assert_eq!(
            bb_env(
                "bbFillFromFile",
                json!({"tiling":"mirrorBoth","nearest":true}),
                &src,
                &EffectEnv::whole(8, 8)
            ),
            src
        );
    }
    /// fill-from-file.md Expected results 2: all 64 pixels.
    #[test]
    fn file_2_one_copy() {
        assert_eq!(
            file(json!({})).data,
            map(&[
                "WWWWWWWW", "WWWWWWWW", "WWWWWWWW", "WWWRGWWW", "WWWBKWWW", "WWWWWWWW", "WWWWWWWW",
                "WWWWWWWW"
            ])
        );
    }
    /// fill-from-file.md Expected results 3: tiled placement moves by zoomed selection units; X = .5
    /// equals X = 0 on every pixel, where From Clipboard shifts one pixel.
    #[test]
    fn file_3_selection_units() {
        assert_eq!(
            file(json!({"tiling":"wrap","placement":[0.125,0]})).data,
            map(&[
                "BKBKBKBK", "RGRGRGRG", "BKBKBKBK", "RGRGRGRG", "BKBKBKBK", "RGRGRGRG", "BKBKBKBK",
                "RGRGRGRG"
            ])
        );
        let zero = file(json!({"tiling":"wrap"}));
        assert_eq!(zero.data, map(&["KBKBKBKB", "GRGRGRGR"].repeat(4)));
        assert_eq!(file(json!({"tiling":"wrap","placement":[0.5,0]})), zero);
        assert_ne!(fill(json!({"tiling":"wrap","placement":[0.5,0]})), zero);
    }
    /// fill-from-file.md Expected results 4: all 64 pixels, repeated mirror edges included.
    #[test]
    fn file_4_mirror_both() {
        assert_eq!(
            file(json!({"tiling":"mirrorBoth"})).data,
            map(&[
                "KKBBKKBB", "KKBBKKBB", "GGRRGGRR", "GGRRGGRR", "KKBBKKBB", "KKBBKKBB", "GGRRGGRR",
                "GGRRGGRR"
            ])
        );
    }
    /// fill-from-file.md Behaviour "Tiled options": X·Zoom·W with the selection's W, an odd
    /// nonzero-origin selection and Zoom 2 (origin 3 + .2·2·5 − 1 = 4; image units would give 3.2).
    #[test]
    fn file_zoomed_selection_units() {
        let img = from_fn(3, 1, |x, _| [R, G, B][x as usize]);
        let (m, p) = rect_selection(7, 5, 1, 1, 6, 4);
        let mut env = with_selection(7, 5, &m, &p);
        env.image = Some(&img);
        let v = json!({"tiling":"wrap","zoom":2,"placement":[0.2,0],"nearest":true});
        let rows = |inner: &str| {
            let middle = format!("W{inner}W");
            map(&["WWWWWWW", &middle, &middle, &middle, "WWWWWWW"])
        };
        assert_eq!(
            bb_env("bbFillFromFile", v.clone(), &solid(7, 5, W), &env).data,
            rows("GBBRR")
        );
        assert_eq!(
            bb_env("bbFillFromClipboard", v, &solid(7, 5, W), &env).data,
            rows("BBRRG")
        );
    }
    /// fill-from-file.md Behaviour "Loading the file": only built-in aliases of the final name
    /// component's extension, spelled as written in the message; content never admits a name.
    #[test]
    fn file_admission() {
        for (name, error) in [
            ("notes.txt", ".txt"),
            ("NOTES.TXT", ".TXT"),
            ("noext", ""),
            ("/tmp/dir.png/noext", ""),
            ("trailing.", ""),
            ("data.json", ".json"),
            ("C.ico", ".ico"),
        ] {
            assert_eq!(
                admit_file(name),
                Err(format!("{error} is not supported.")),
                "{name}"
            );
        }
        for name in [
            "a.png",
            "A.PNG",
            "a.jpg",
            "a.jpeg",
            "a.JPE",
            "a.gif",
            "a.bmp",
            "a.tif",
            "a.tiff",
            "a.tga",
            "a.dds",
            "a.webp",
            "a.jxl",
            "a.avif",
            "a.heic",
            "a.heif",
            "a.jxr",
            "a.wdp",
            "a.wmp",
            "a.pdn",
            "/d.txt/p.Png",
        ] {
            assert_eq!(admit_file(name), Ok(()), "{name}");
        }
    }
    /// fill-from-file.md Controls table; common.md §3 "File chooser": exact strings, order, default,
    /// no header line, the external file source and From Clipboard's four controls verbatim.
    #[test]
    fn file_control_metadata() {
        use super::super::ui::{External, Help, HelpWindow};
        use crate::effect::{Control, EffectKind};
        let d = crate::effect::find("bbFillFromFile").unwrap();
        assert_eq!(
            d.controls.iter().map(Control::key).collect::<Vec<_>>(),
            ["file", "placement", "zoom", "nearest", "tiling"]
        );
        assert_eq!(d.dialog_title, Some("Fill From File"));
        assert!(matches!(
            d.kind,
            EffectKind::Cpu {
                clip_to_selection: true,
                ..
            }
        ));
        let Control::File {
            key,
            label,
            filters,
            all_files,
        } = &d.controls[0]
        else {
            panic!()
        };
        assert_eq!(
            (*key, *label, *filters, *all_files),
            (
                "file",
                "File to load",
                &["png", "jpg", "gif", "bmp"][..],
                true
            )
        );
        let defaults = crate::effect::defaults(&d);
        assert_eq!(defaults["file"], json!(""));
        let u = d.indirect.unwrap();
        assert_eq!(
            (u.external_for)(&crate::effect::Values::new()),
            External::File("file")
        );
        assert_eq!(u.width, 1.0);
        assert!(!u.control("file").header_line);
        assert!(matches!(u.help, Help::Window(HelpWindow::Standard)));
        for c in &d.controls {
            assert!((u.enabled)(&defaults, c.key()));
        }
        let clip = crate::effect::find("bbFillFromClipboard").unwrap();
        let cu = clip.indirect.unwrap();
        assert_eq!(
            format!("{:?}", &d.controls[1..]),
            format!("{:?}", clip.controls)
        );
        for k in ["placement", "zoom", "nearest", "tiling"] {
            assert_eq!(
                format!("{:?}", u.control(k)),
                format!("{:?}", cu.control(k))
            );
        }
    }
    /// fill-from-clipboard.md Expected results 1: every pixel, both samplers.
    #[test]
    fn fill_1_defaults() {
        for nearest in [true, false] {
            assert_eq!(
                fill(json!({"nearest":nearest})).data,
                map(&[
                    "WWWWWWWW", "WWWWWWWW", "WWWWWWWW", "WWWRGWWW", "WWWBKWWW", "WWWWWWWW",
                    "WWWWWWWW", "WWWWWWWW"
                ])
            );
        }
    }
    /// fill-from-clipboard.md Expected results 2: full 2x2 blocks and untouched pixels.
    #[test]
    fn fill_2_zoom() {
        assert_eq!(
            fill(json!({"zoom":2})).data,
            map(&[
                "WWWWWWWW", "WWWWWWWW", "WWRRGGWW", "WWRRGGWW", "WWBBKKWW", "WWBBKKWW", "WWWWWWWW",
                "WWWWWWWW"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 3: the complete wrapped canvas.
    #[test]
    fn fill_3_wrap() {
        assert_eq!(
            fill(json!({"tiling":"wrap"})).data,
            map(&[
                "KBKBKBKB", "GRGRGRGR", "KBKBKBKB", "GRGRGRGR", "KBKBKBKB", "GRGRGRGR", "KBKBKBKB",
                "GRGRGRGR"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 4: duplicated mirror edges, every row.
    #[test]
    fn fill_4_mirror() {
        assert_eq!(
            fill(json!({"tiling":"mirrorX"})).data,
            map(&[
                "KKBBKKBB", "GGRRGGRR", "KKBBKKBB", "GGRRGGRR", "KKBBKKBB", "GGRRGGRR", "KKBBKKBB",
                "GGRRGGRR"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 5: image-relative tiled placement.
    #[test]
    fn fill_5_tiled_placement() {
        assert_eq!(
            fill(json!({"tiling":"wrap","placement":[0.5,0]})).data,
            map(&[
                "BKBKBKBK", "RGRGRGRG", "BKBKBKBK", "RGRGRGRG", "BKBKBKBK", "RGRGRGRG", "BKBKBKBK",
                "RGRGRGRG"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 6: clipped copy and all white remainder.
    #[test]
    fn fill_6_placement() {
        assert_eq!(
            fill(json!({"placement":[0.5,0]})).data,
            map(&[
                "WWWWWWWW", "WWWWWWWW", "WWWWWWWW", "WWWWWWWR", "WWWWWWWB", "WWWWWWWW", "WWWWWWWW",
                "WWWWWWWW"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 7: full inside/outside selection buffers.
    #[test]
    fn fill_7_selection() {
        let c = clip();
        let (m, p) = rect_selection(8, 8, 2, 2, 6, 6);
        let mut env = with_selection(8, 8, &m, &p);
        env.image = Some(&c);
        assert_eq!(
            bb_env(
                "bbFillFromClipboard",
                json!({"nearest":true,"tiling":"wrap"}),
                &solid(8, 8, W),
                &env
            )
            .data,
            map(&[
                "WWWWWWWW", "WWWWWWWW", "WWKBKBWW", "WWGRGRWW", "WWKBKBWW", "WWGRGRWW", "WWWWWWWW",
                "WWWWWWWW"
            ])
        );
    }
    /// fill-from-clipboard.md Expected results 8; object-paste-alpha.md Expected results 6: exact no-image bytes, including hidden RGB.
    #[test]
    fn empty_images_unchanged() {
        let src = from_fn(8, 8, |x, y| {
            [x as u8, y as u8, 91, if x % 2 == 0 { 0 } else { 200 }]
        });
        for id in ["bbFillFromClipboard", "bbFillFromFile", "bbPasteAlpha"] {
            assert_eq!(bb_env(id, json!({}), &src, &EffectEnv::whole(8, 8)), src);
        }
    }
    fn pa(
        v: serde_json::Value,
        src: &crate::canvas::ImageData,
        img: &crate::canvas::ImageData,
    ) -> Vec<u8> {
        let mut env = EffectEnv::whole(src.width, src.height);
        env.image = Some(img);
        bb_env("bbPasteAlpha", v, src, &env).data
    }
    /// object-paste-alpha.md Expected results 1: hidden red survives alpha zero.
    #[test]
    fn alpha_1_basic() {
        assert_eq!(
            pa(
                json!({}),
                &solid(2, 1, R),
                &from_fn(2, 1, |x, _| if x == 0 { W } else { K })
            ),
            [255, 0, 0, 255, 255, 0, 0, 0]
        );
    }
    /// object-paste-alpha.md Expected results 2: exact luma and tiled RGBA.
    #[test]
    fn alpha_2_luma() {
        for (c, a) in [([128, 128, 128, 255], 128), (R, 76), (G, 149), (B, 29)] {
            assert_eq!(
                pa(json!({}), &solid(3, 1, W), &solid(1, 1, c)),
                [255, 255, 255, a].repeat(3)
            );
        }
    }
    /// object-paste-alpha.md Expected results 3: all method/invert RGBA bytes.
    #[test]
    fn alpha_3_methods() {
        for (m, a, b) in [
            ("replace", 100, 155),
            ("minimum", 100, 155),
            ("multiply", 78, 122),
        ] {
            for (inv, alpha) in [(false, a), (true, b)] {
                assert_eq!(
                    pa(
                        json!({"method":m,"invert":inv}),
                        &solid(1, 1, [255, 0, 0, 200]),
                        &solid(1, 1, [100, 100, 100, 255])
                    ),
                    [255, 0, 0, alpha]
                );
            }
        }
    }
    /// object-paste-alpha.md Expected results 4: alpha source ignores grey, grey ignores alpha.
    #[test]
    fn alpha_4_source() {
        for (source, a) in [("alpha", 64), ("gray", 255)] {
            assert_eq!(
                pa(
                    json!({"source":source}),
                    &solid(1, 1, R),
                    &solid(1, 1, [255, 255, 255, 64])
                ),
                [255, 0, 0, a]
            );
        }
    }
    /// object-paste-alpha.md Expected results 5: complete non-square swapped-axis result.
    #[test]
    fn alpha_5_placement() {
        let c = from_fn(4, 2, |x, y| {
            let v = if y == 0 {
                [0, 85, 170, 255][x as usize]
            } else {
                9
            };
            [v, v, v, 255]
        });
        assert_eq!(
            pa(
                json!({"placement":[0,-1],"nearest":true}),
                &solid(4, 1, R),
                &c
            ),
            [255, 0, 0, 85, 255, 0, 0, 170, 255, 0, 0, 255, 255, 0, 0, 0]
        );
    }
    /// object-paste-alpha.md Behaviour: ties even, negative wrap and quantized alpha-weighted bilinear.
    #[test]
    fn alpha_sampler() {
        let strip = from_fn(4, 1, |x, _| {
            [
                [0, 0, 0, 255],
                [85, 85, 85, 255],
                [170, 170, 170, 255],
                [255; 4],
            ][x as usize]
        });
        assert_eq!(
            (0..6)
                .map(|x| paste_alpha_sample(&strip, x as f32 / 2.0, 0.0, true)[0])
                .collect::<Vec<_>>(),
            [0, 0, 85, 170, 170, 170]
        );
        assert_eq!(paste_alpha_sample(&strip, -0.5, 0.0, true), [0, 0, 0, 255]);
        assert_eq!(paste_alpha_sample(&strip, -1.0, 0.0, true), [255; 4]);
        let c = from_fn(2, 1, |x, _| {
            if x == 0 {
                [255, 0, 0, 0]
            } else {
                [0, 0, 255, 255]
            }
        });
        assert_eq!(paste_alpha_sample(&c, 0.5, 0.0, false), [0, 0, 255, 128]);
        assert_eq!(
            paste_alpha_sample(&strip, 1.0 / 512.0, 0.0, false),
            [0, 0, 0, 255]
        );
    }
    fn params() -> FillParams {
        FillParams {
            pan: [0.0; 2],
            zoom: 1.0,
            nearest: true,
            tiling: Tiling::One,
            tile_units_selection: false,
        }
    }
    /// fill-from-clipboard.md Behaviour: both explicit contexts include nearest translucent source-over.
    #[test]
    fn source_over_contexts() {
        use crate::effect::RenderInfo;
        use crate::effect::surface::{Alpha, Space};
        for (space, white, red) in [
            (Space::Srgb, 128, [128, 0, 127, 255]),
            (Space::Linear, 188, [188, 0, 187, 255]),
        ] {
            let info = RenderInfo::new(space, Alpha::Premultiplied);
            let out = render_fill_context(
                &solid(1, 1, K),
                &solid(1, 1, [255, 255, 255, 128]),
                crate::pixel::IntRect::new(0, 0, 1, 1),
                &params(),
                info,
            );
            assert_px(&out, 0, 0, [white, white, white, 255]);
            let out = render_fill_context(
                &solid(1, 1, B),
                &solid(1, 1, [255, 0, 0, 128]),
                crate::pixel::IntRect::new(0, 0, 1, 1),
                &params(),
                info,
            );
            assert_px(&out, 0, 0, red);
            let hidden = solid(1, 1, [31, 67, 149, 0]);
            assert_eq!(
                render_fill_context(
                    &hidden,
                    &solid(1, 1, [200, 50, 0, 0]),
                    crate::pixel::IntRect::new(0, 0, 1, 1),
                    &params(),
                    info
                ),
                hidden
            );
        }
    }
    /// fill-from-clipboard.md Behaviour: half-texel Catmull-Rom context fixture, independently derived weights 27/32 and 5/32.
    #[test]
    fn cubic_contexts() {
        use crate::effect::RenderInfo;
        use crate::effect::surface::{Alpha, Space};
        let c = from_fn(2, 1, |x, _| if x == 0 { K } else { W });
        let mut p = params();
        p.nearest = false;
        p.tiling = Tiling::Wrap;
        p.zoom = 2.0;
        for (space, levels) in [
            (Space::Srgb, [215, 40, 40, 215]),
            (Space::Linear, [237, 110, 110, 237]),
        ] {
            let out = render_fill_context(
                &solid(4, 1, K),
                &c,
                crate::pixel::IntRect::new(0, 0, 4, 1),
                &p,
                RenderInfo::new(space, Alpha::Premultiplied),
            );
            let expected: Vec<u8> = levels.into_iter().flat_map(|v| [v, v, v, 255]).collect();
            assert!(
                out.data
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(b) <= 2),
                "{:?}",
                out.data
            );
        }
    }
    /// fill-from-clipboard.md Behaviour: independent mirror axes and zoomed reference origin.
    #[test]
    fn mirrored_axes_and_zoomed_tiles() {
        for (mode, rows) in [
            (
                "mirrorY",
                [
                    "KBKBKBKB", "KBKBKBKB", "GRGRGRGR", "GRGRGRGR", "KBKBKBKB", "KBKBKBKB",
                    "GRGRGRGR", "GRGRGRGR",
                ],
            ),
            (
                "mirrorBoth",
                [
                    "KKBBKKBB", "KKBBKKBB", "GGRRGGRR", "GGRRGGRR", "KKBBKKBB", "KKBBKKBB",
                    "GGRRGGRR", "GGRRGGRR",
                ],
            ),
        ] {
            assert_eq!(fill(json!({"tiling":mode})).data, map(&rows));
        }
        assert_eq!(
            fill(json!({"tiling":"wrap","zoom":2,"placement":[0.5,0]})).data,
            map(&[
                "GRRGGRRG", "KBBKKBBK", "KBBKKBBK", "GRRGGRRG", "GRRGGRRG", "KBBKKBBK", "KBBKKBBK",
                "GRRGGRRG"
            ])
        );
    }
    /// fill-from-clipboard.md Behaviour: floor-half placement for odd non-square bounds, plus BB27 helper's selection tile units.
    #[test]
    fn odd_bounds_and_file_units() {
        let src = solid(7, 5, W);
        let img = from_fn(3, 1, |x, _| [R, G, B][x as usize]);
        let bounds = crate::pixel::IntRect::new(1, 1, 5, 3);
        assert_eq!(
            render_fill(&src, &img, bounds, &params()).data,
            map(&["WWWWWWW", "WWWWWWW", "WWRGBWW", "WWWWWWW", "WWWWWWW"])
        );
        let mut p = params();
        p.tiling = Tiling::Wrap;
        p.pan = [1.0, 0.0];
        p.tile_units_selection = true;
        assert_eq!(
            render_fill(&src, &img, bounds, &p).data,
            map(&["WWWWWWW", "WRGBRGW", "WRGBRGW", "WRGBRGW", "WWWWWWW"])
        );
    }
    /// object-paste-alpha.md Behaviour: vertical shift swaps width too; selection does not move the mask.
    #[test]
    fn alpha_vertical_and_selection() {
        let c = from_fn(2, 4, |_, y| {
            let v = [0, 85, 170, 255][y as usize];
            [v, v, v, 255]
        });
        assert_eq!(
            pa(
                json!({"placement":[-1,0],"nearest":true}),
                &solid(1, 4, R),
                &c
            ),
            [255, 0, 0, 85, 255, 0, 0, 170, 255, 0, 0, 255, 255, 0, 0, 0]
        );
        let c = from_fn(3, 1, |x, _| {
            let v = [0, 100, 255][x as usize];
            [v, v, v, 255]
        });
        let (m, p) = rect_selection(5, 1, 2, 0, 4, 1);
        let mut env = with_selection(5, 1, &m, &p);
        env.image = Some(&c);
        assert_eq!(
            bb_env(
                "bbPasteAlpha",
                json!({"nearest":true}),
                &solid(5, 1, R),
                &env
            )
            .data,
            [
                255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 0, 255, 0, 0, 255
            ]
        );
    }
    /// common.md §2: automatic antialiased selection clipping applies once, including hidden RGB.
    #[test]
    fn selection_coverage_once() {
        use crate::pixel::mask::Mask;
        let m = Mask {
            rect: crate::pixel::IntRect::new(0, 0, 1, 1),
            data: vec![128],
        };
        let c = solid(1, 1, K);
        let mut env = EffectEnv::whole(1, 1);
        env.mask = Some(&m);
        env.image = Some(&c);
        assert_eq!(
            bb_env("bbPasteAlpha", json!({}), &solid(1, 1, R), &env).data,
            [255, 0, 0, 127]
        );
        let c = solid(1, 1, W);
        env.image = Some(&c);
        for id in ["bbFillFromClipboard", "bbFillFromFile"] {
            assert_eq!(
                bb_env(id, json!({"nearest":true}), &solid(1, 1, K), &env).data,
                [128, 128, 128, 255]
            );
        }
    }
    /// common.md §3; both effect Controls tables: exact strings, values, ordering and keyboard metadata.
    #[test]
    fn control_metadata() {
        use super::super::ui::{External, Help};
        use crate::effect::{Control, EffectKind, PanThumb};
        for (id, keys, title, thumb, range, default, exp) in [
            (
                "bbFillFromClipboard",
                vec!["placement", "zoom", "nearest", "tiling"],
                "Fill From Clipboard",
                PanThumb::White,
                (-1.0, 1.0),
                [0.0; 2],
                true,
            ),
            (
                "bbPasteAlpha",
                vec!["method", "source", "placement", "zoom", "nearest", "invert"],
                "Paste Alpha",
                PanThumb::SelectionCrop,
                (-2.0, 2.0),
                [-1.0; 2],
                false,
            ),
        ] {
            let d = crate::effect::find(id).unwrap();
            assert_eq!(
                d.controls.iter().map(Control::key).collect::<Vec<_>>(),
                keys
            );
            assert_eq!(d.dialog_title, Some(title));
            assert!(matches!(
                d.kind,
                EffectKind::Cpu {
                    clip_to_selection: true,
                    ..
                }
            ));
            let u = d.indirect.unwrap();
            assert_eq!(u.width, 1.0);
            for c in &d.controls {
                if let Control::Checkbox {
                    key,
                    label,
                    default,
                } = c
                {
                    assert!(!default);
                    assert_eq!(
                        *label,
                        if *key == "nearest" {
                            "Nearest neighbor sampling"
                        } else {
                            "Invert calculation"
                        }
                    );
                }
            }
            assert_eq!(
                (u.external_for)(&crate::effect::Values::new()),
                External::Clipboard
            );
            assert!(matches!(u.help, Help::Window(_)));
            for c in &d.controls {
                assert!((u.enabled)(&crate::effect::defaults(&d), c.key()));
            }
            let Control::Pan {
                min,
                max,
                default: actual,
                step,
                thumb: t,
                label,
                ..
            } = d.controls.iter().find(|c| c.key() == "placement").unwrap()
            else {
                panic!()
            };
            assert_eq!(
                (*min, *max, *actual, *step, *t, *label),
                (
                    range.0,
                    range.1,
                    default,
                    0.01,
                    thumb,
                    if exp { "" } else { "Placement" }
                )
            );
            let cu = u.control("placement");
            assert_eq!(cu.decimals, Some(3));
            let k = cu.pan_sliders.unwrap();
            assert_eq!(
                (k.decimals, k.small, k.large, k.exponential),
                (3, 0.05, 0.25, false)
            );
            let Control::Range {
                min,
                max,
                default,
                step,
                keyboard,
                exponential,
                label,
                ..
            } = d.controls.iter().find(|c| c.key() == "zoom").unwrap()
            else {
                panic!()
            };
            assert_eq!(
                (*min, *max, *default, *step, *exponential, *label),
                (
                    if exp { 0.06 } else { 0.01 },
                    if exp { 16.0 } else { 10.0 },
                    1.0,
                    0.01,
                    exp,
                    "Zoom"
                )
            );
            assert_eq!(
                (
                    keyboard.decimals,
                    keyboard.small,
                    keyboard.large,
                    keyboard.exponential
                ),
                (3, 0.05, 0.25, exp)
            );
            assert_eq!(u.control("zoom").header_line, !exp);
        }
        let d = crate::effect::find("bbFillFromClipboard").unwrap();
        let Control::Select {
            choices,
            default,
            label,
            ..
        } = &d.controls[3]
        else {
            panic!()
        };
        assert_eq!(
            (*label, *default, *choices),
            (
                "",
                "one",
                &[
                    ("one", "No Tiling - One copy only"),
                    ("wrap", "Tiling - No Mirroring"),
                    ("mirrorX", "Tiling with Horizontal Mirror"),
                    ("mirrorY", "Tiling with Vertical Mirror"),
                    ("mirrorBoth", "Tiling and Mirror Both Directions")
                ][..]
            )
        );
        let d = crate::effect::find("bbPasteAlpha").unwrap();
        for (i, values, default) in [
            (
                0,
                &[
                    ("replace", "Replace current alpha with"),
                    ("minimum", "Minimum of current alpha and"),
                    ("multiply", "Multiply current alpha with"),
                ][..],
                "replace",
            ),
            (
                1,
                &[
                    ("gray", "Shades of gray from clipboard"),
                    ("alpha", "Alpha channel from clipboard"),
                ][..],
                "gray",
            ),
        ] {
            let Control::Select {
                choices,
                default: d,
                label,
                ..
            } = &d.controls[i]
            else {
                panic!()
            };
            assert_eq!((*label, *d, *choices), ("", default, values));
        }
    }
    /// object-paste-alpha.md Behaviour: classic wrapped bilinear truncates final RGB, while alpha rounds.
    /// Controller black-box fixture: opaque black/white above white alpha1/alpha2, Zoom2.
    #[test]
    fn alpha_bilinear_integer_divisions() {
        let strip = from_fn(2, 1, |x, _| if x == 0 { K } else { W });
        assert_eq!(
            paste_alpha_sample(&strip, 0.5, 0., false),
            [127, 127, 127, 255]
        );
        let image = from_fn(2, 2, |x, y| {
            [[K, W], [[255, 255, 255, 1], [255, 255, 255, 2]]][y as usize][x as usize]
        });
        let base = solid(6, 4, R);
        for (source, zoom, alphas) in [
            (
                "gray",
                2,
                vec![
                    0, 127, 255, 127, 0, 127, 0, 128, 255, 128, 0, 128, 255, 255, 255, 255, 255,
                    255, 0, 128, 255, 128, 0, 128,
                ],
            ),
            (
                "alpha",
                2,
                vec![
                    255, 255, 255, 255, 255, 255, 128, 128, 129, 128, 128, 128, 1, 2, 2, 2, 1, 2,
                    128, 128, 129, 128, 128, 128,
                ],
            ),
            (
                "gray",
                3,
                vec![
                    0, 84, 169, 255, 170, 85, 0, 85, 169, 255, 170, 86, 1, 86, 170, 255, 171, 87,
                    255, 255, 255, 255, 255, 255,
                ],
            ),
        ] {
            let expected: Vec<u8> = alphas.into_iter().flat_map(|a| [255, 0, 0, a]).collect();
            assert_eq!(
                pa(json!({"zoom":zoom,"source":source}), &base, &image),
                expected,
                "{source}"
            );
        }
    }
}
