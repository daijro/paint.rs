//! Photocraft phase spec: S§4.2; research R§B (Artistic, Noise, Stylize: the Filter Gallery looks)
//!
//! The Filter Gallery looks as Extra effects (`extra::fx::artistic_fx`). One definition per
//! manifest row whose id is `pc` + a gallery filter key (`pcColoredPencil`); the controls come
//! from the filter's parameter table, so adding a look is only a manifest row.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::canvas::ImageData;
use crate::effect::{
    Category, ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, range,
};
use crate::extra::fx::artistic::{GalleryFilter, GalleryParamKind};
use crate::extra::fx::artistic_fx;
use crate::extra::geom::Rect;
use crate::extra::shim;

use super::manifest::MANIFEST;

/// The gallery filter behind an Extra effect id (`pcColoredPencil` -> `coloredPencil`).
fn filter_of(id: &str) -> Option<GalleryFilter> {
    let rest = id.strip_prefix("pc")?;
    let mut chars = rest.chars();
    let key = chars
        .next()?
        .to_lowercase()
        .chain(chars)
        .collect::<String>();
    GalleryFilter::from_key(&key)
}

/// Every manifest row that names a gallery filter.
pub fn effects() -> Vec<EffectDef> {
    MANIFEST
        .iter()
        .filter(|m| filter_of(m.id).is_some())
        .map(|m| gallery_def(m.id, m.name, m.category, m.icon))
        .collect()
}

/// The effect definition of gallery filter `id` (`pc<Key>`): controls from its parameter table,
/// foreground/background colours as Primary/Secondary colour controls. Panics for an id that is
/// not a gallery filter.
pub fn gallery_def(
    id: &'static str,
    name: &'static str,
    category: Category,
    icon: &'static str,
) -> EffectDef {
    let filter = filter_of(id).unwrap_or_else(|| panic!("{id} is not a gallery filter"));
    EffectDef {
        id,
        icon,
        category,
        controls: controls(filter),
        kind: EffectKind::Cpu {
            render: renderer(filter),
            clip_to_selection: true,
        },
        dialog: None,
        dialog_title: Some(name),
        indirect: None,
    }
}

/// `strokePressure` -> `Stroke Pressure`.
fn label(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            if c.is_uppercase() {
                out.push(' ');
            }
            out.push(c);
        }
    }
    out
}

fn leak(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// The controls of `filter`, built once (their strings are leaked, once per filter).
fn controls(filter: GalleryFilter) -> Vec<Control> {
    static CACHE: OnceLock<Mutex<HashMap<&'static str, Vec<Control>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().unwrap();
    cache
        .entry(filter.key())
        .or_insert_with(|| build_controls(filter))
        .clone()
}

fn build_controls(filter: GalleryFilter) -> Vec<Control> {
    let mut out = Vec::new();
    for p in filter.params() {
        let (key, lbl) = (p.key, leak(label(p.key)));
        out.push(match p.kind {
            GalleryParamKind::Range { min, max, default } => {
                range(key, lbl, f64::from(min), f64::from(max), f64::from(default))
            }
            GalleryParamKind::Choice(names) => Control::Select {
                key,
                label: lbl,
                default: names[0],
                choices: Box::leak(
                    names
                        .iter()
                        .map(|n| (*n, leak(label(n))))
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
            },
            GalleryParamKind::Bool(default) => Control::Checkbox {
                key,
                label: lbl,
                default,
            },
        });
    }
    if filter == GalleryFilter::NeonGlow {
        out.push(Control::Color {
            key: "glowColor",
            label: "Glow Color",
            default: ColorDefault::Rgb(0x009EFF),
        });
    }
    let (fg, bg) = colours_read(filter);
    for (used, key, label, default) in [
        (fg, "foreground", "Foreground", ColorDefault::Primary),
        (bg, "background", "Background", ColorDefault::Secondary),
    ] {
        if used {
            out.push(Control::Color {
                key,
                label,
                default,
            });
        }
    }
    out
}

/// Which of (foreground, background) the filter's kernel reads (`artistic_fx::apply`), not what
/// the parameter notation lists; `glowColor` is Neon Glow's own. A test pins it to the kernels.
fn colours_read(filter: GalleryFilter) -> (bool, bool) {
    use GalleryFilter::*;
    match filter {
        ColoredPencil | DiffuseGlow => (false, true),
        StainedGlass => (true, false),
        NeonGlow | BasRelief | ChalkCharcoal | Charcoal | ConteCrayon | GraphicPen
        | HalftonePattern | NotePaper | Photocopy | Plaster | Reticulation | Stamp | TornEdges
        | Grain => (true, true),
        DryBrush | Fresco | PaintDaubs | PaletteKnife | PlasticWrap | PosterEdges
        | RoughPastels | SmudgeStick | Sponge | Underpainting | Watercolor | FilmGrain | Cutout
        | AccentedEdges | AngledStrokes | Crosshatch | DarkStrokes | InkOutlines | Spatter
        | SprayedStrokes | SumiE | Glass | OceanRipple | Chrome | WaterPaper | GlowingEdges
        | Craquelure | MosaicTiles | Patchwork | Texturizer => (false, false),
    }
}

/// R92: the patterns anchor on the selection bounds, as their Distort siblings' do.
fn render(filter: GalleryFilter, src: &ImageData, values: &Values, env: &EffectEnv) -> ImageData {
    let rgba = |p: [u8; 4]| p.map(|v| f32::from(v) / 255.0);
    let b = env.bounds;
    let buf = artistic_fx::run(
        filter.key(),
        values,
        &shim::to_f32(src),
        src.width as usize,
        src.height as usize,
        Rect::new(b.x, b.y, b.x + b.w as i32, b.y + b.h as i32),
        rgba(env.primary),
        rgba(env.secondary),
    );
    shim::from_f32(&buf, src.width, src.height)
}

type RenderFn = fn(&ImageData, &Values, &EffectEnv) -> ImageData;

/// `EffectKind::Cpu` takes a plain `fn`, so each filter gets its own.
macro_rules! renderers {
    ($($v:ident),* $(,)?) => {
        fn renderer(filter: GalleryFilter) -> RenderFn {
            match filter {
                $(GalleryFilter::$v => |s, v, e| render(GalleryFilter::$v, s, v, e),)*
            }
        }
    };
}

renderers!(
    ColoredPencil,
    Cutout,
    DryBrush,
    FilmGrain,
    Fresco,
    NeonGlow,
    PaintDaubs,
    PaletteKnife,
    PlasticWrap,
    PosterEdges,
    RoughPastels,
    SmudgeStick,
    Sponge,
    Underpainting,
    Watercolor,
    AccentedEdges,
    AngledStrokes,
    Crosshatch,
    DarkStrokes,
    InkOutlines,
    Spatter,
    SprayedStrokes,
    SumiE,
    DiffuseGlow,
    Glass,
    OceanRipple,
    BasRelief,
    ChalkCharcoal,
    Charcoal,
    Chrome,
    ConteCrayon,
    GraphicPen,
    HalftonePattern,
    NotePaper,
    Photocopy,
    Plaster,
    Reticulation,
    Stamp,
    TornEdges,
    WaterPaper,
    GlowingEdges,
    Craquelure,
    Grain,
    MosaicTiles,
    Patchwork,
    StainedGlass,
    Texturizer,
);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::effect::test_util::{from_fn, px, run, run_env};
    use crate::effect::{apply, defaults, find};
    use serde_json::json;

    /// N4: with no selection the bounds are the layer, so every gallery effect gives the
    /// whole-layer render (the kernel anchored on the layer origin), and so does a selection of
    /// the whole layer.
    #[test]
    fn no_selection_is_the_whole_layer_render() {
        use crate::effect::bb::test_util::{rect_selection, with_selection};
        let (w, h) = (40u32, 32u32);
        let src = pattern(w, h);
        let (mask, polys) = rect_selection(w, h, 0, 0, w, h);
        let all = with_selection(w, h, &mask, &polys);
        let filters: Vec<_> = MANIFEST
            .iter()
            .filter_map(|m| filter_of(m.id).map(|f| (m.id, f)))
            .collect();
        assert_eq!(filters.len(), 47);
        for (id, f) in filters {
            let def = find(id).unwrap();
            let layer = artistic_fx::run(
                f.key(),
                &defaults(&def),
                &shim::to_f32(&src),
                w as usize,
                h as usize,
                Rect::new(0, 0, w as i32, h as i32),
                [0.0, 0.0, 0.0, 1.0],
                [1.0; 4],
            );
            let out = run(id, json!({}), &src);
            assert_eq!(out.data, shim::from_f32(&layer, w, h).data, "{id}");
            assert_eq!(run_env(id, json!({}), &src, &all).data, out.data, "{id}");
        }
    }

    /// R92: the pattern looks anchor on the selection bounds, as their Distort siblings do. On a
    /// layer that repeats every 16 px, a selection moved one period diagonally gives the same
    /// output moved by one period, in x and in y; a pattern anchored on the layer origin would
    /// not move with it.
    #[test]
    fn pattern_looks_anchor_on_the_selection_bounds() {
        use crate::effect::bb::test_util::{rect_selection, with_selection};
        let src = from_fn(96, 96, |x, y| {
            let (u, v) = (x % 16, y % 16);
            let r = (u * 16 + v * 3) as u8;
            let g = if (u / 4 + v / 4) % 2 == 0 { 0 } else { 255 };
            [r, g, 255 - r, 255]
        });
        for id in [
            "pcHalftonePattern",
            "pcStainedGlass",
            "pcTexturizer",
            "pcMosaicTiles",
            "pcPatchwork",
        ] {
            let at = |l: u32| {
                let (mask, polys) = rect_selection(96, 96, l, l, l + 32, l + 32);
                let env = with_selection(96, 96, &mask, &polys);
                run_env(id, json!({}), &src, &env)
            };
            let (a, b) = (at(16), at(32));
            for (x, y) in (16..48).flat_map(|y| (16..48).map(move |x| (x, y))) {
                let (p, q) = (px(&a, x, y), px(&b, x + 16, y + 16));
                assert!(
                    (0..4).all(|c| p[c].abs_diff(q[c]) <= 1),
                    "{id} {x},{y}: {p:?} {q:?}"
                );
            }
            assert_ne!(a.data, src.data, "{id} changed nothing");
        }
    }

    const FIFTEEN: [&str; 15] = [
        "pcColoredPencil",
        "pcCutout",
        "pcDryBrush",
        "pcFresco",
        "pcPaintDaubs",
        "pcPaletteKnife",
        "pcPosterEdges",
        "pcRoughPastels",
        "pcSmudgeStick",
        "pcSponge",
        "pcUnderpainting",
        "pcWatercolor",
        "pcFilmGrain",
        "pcNeonGlow",
        "pcPlasticWrap",
    ];

    fn pattern(w: u32, h: u32) -> ImageData {
        from_fn(w, h, |x, y| {
            let v = ((x * 7 + y * 3) % 23) as f32 / 22.0;
            let c = if (x / 9 + y / 7) % 2 == 0 {
                [v, 0.3, 1.0 - v]
            } else {
                [0.9, v * 0.5, 0.2]
            };
            let [r, g, b] = c.map(|v| (v * 255.0) as u8);
            [r, g, b, 255]
        })
    }

    /// Every gallery filter of the table (all 47, not only the registered 15) gives the same
    /// finite output twice, whatever the thread schedule.
    #[test]
    fn every_gallery_item_is_deterministic() {
        let src = shim::to_f32(&pattern(40, 32));
        for f in GalleryFilter::ALL {
            let a = artistic_fx::run(
                f.key(),
                &Values::new(),
                &src,
                40,
                32,
                Rect::new(0, 0, 40, 32),
                [0.; 4],
                [1.; 4],
            );
            let b = artistic_fx::run(
                f.key(),
                &Values::new(),
                &src,
                40,
                32,
                Rect::new(0, 0, 40, 32),
                [0.; 4],
                [1.; 4],
            );
            assert_eq!(a, b, "{}", f.key());
            assert!(a.iter().all(|v| v.is_finite()), "{}", f.key());
            assert_ne!(a, src, "{} changed nothing", f.key());
        }
    }

    #[test]
    fn gallery_items_keep_alpha() {
        let src = pattern(40, 32);
        for id in FIFTEEN {
            let def = find(id).unwrap_or_else(|| panic!("{id} not registered"));
            let out = apply(&src, &def, &defaults(&def), &EffectEnv::whole(40, 32));
            assert!(out.data.chunks(4).all(|p| p[3] == 255), "{id}");
        }
        let mut half = pattern(40, 32);
        half.data.chunks_mut(4).for_each(|p| p[3] = 100);
        let out = run("pcWatercolor", json!({}), &half);
        assert!(out.data.chunks(4).all(|p| p[3] == 100));
    }

    /// Cutout posterises each channel to `2 * numberOfLevels` steps.
    #[test]
    fn cutout_reduces_colour_count() {
        let src = from_fn(64, 8, |x, _| {
            let v = (x * 4) as u8;
            [v, v / 2, 255 - v, 255]
        });
        let levels = 4;
        let out = run("pcCutout", json!({ "numberOfLevels": levels }), &src);
        for c in 0..3 {
            let mut seen: Vec<u8> = out.data.chunks(4).map(|p| p[c]).collect();
            seen.sort_unstable();
            seen.dedup();
            assert!(seen.len() <= 2 * levels, "channel {c}: {}", seen.len());
        }
        let distinct = |i: &ImageData| {
            let mut v: Vec<_> = i.data.chunks(4).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(distinct(&out) < distinct(&src));
    }

    #[test]
    fn neon_glow_uses_its_colour() {
        let src = from_fn(32, 32, |x, _| {
            let v = if x < 16 { 255 } else { 0 };
            [v, v, v, 255]
        });
        let red = run("pcNeonGlow", json!({ "glowColor": "#ff0000" }), &src);
        let green = run("pcNeonGlow", json!({ "glowColor": "#00ff00" }), &src);
        assert_ne!(red, green);
        // On the dark side, next to the edge, the glow adds its own colour.
        let (r, g) = (px(&red, 17, 16), px(&green, 17, 16));
        assert!(r[0] > g[0] && g[1] > r[1], "{r:?} {g:?}");
    }

    /// Foreground/background are Primary/Secondary colour controls, so the palette drives them.
    #[test]
    fn gallery_colours_follow_the_palette() {
        let def = find("pcNeonGlow").unwrap();
        let fg = def
            .controls
            .iter()
            .find(|c| c.key() == "foreground")
            .unwrap();
        assert_eq!(fg.default_value(), json!("primary"));
        let src = pattern(24, 24);
        let mut env = EffectEnv::whole(24, 24);
        let black = run_env("pcNeonGlow", json!({}), &src, &env);
        env.primary = [255, 0, 0, 255];
        assert_ne!(black, run_env("pcNeonGlow", json!({}), &src, &env));
    }

    #[test]
    fn gallery_def_covers_the_whole_table() {
        for f in GalleryFilter::ALL {
            let id: &'static str = leak(format!("pc{}", label(f.key()).replace(' ', "")));
            let def = gallery_def(id, f.name(), Category::Effect("artistic"), "x.png");
            assert!(def.controls.len() >= f.params().len(), "{id}");
        }
    }

    /// A gallery def has a Color control for exactly the colours its kernel reads.
    #[test]
    fn colour_controls_match_what_the_kernel_reads() {
        // Every tone from black to white, in blocks, so no filter's threshold hides a colour.
        let src = shim::to_f32(&from_fn(48, 48, |x, y| {
            let v = ((x / 3 * 7 + y / 3 * 13) % 17 * 16) as u8;
            [v, v, v, 255]
        }));
        // A colour counts if it matters under the defaults or under any one choice (Grain's
        // types, for one, read the foreground only in some).
        let changes = |f: GalleryFilter, key: &str| {
            let mut variants = vec![Values::new()];
            for p in f.params() {
                if let GalleryParamKind::Choice(names) = p.kind {
                    for n in names {
                        variants.push([(p.key.to_string(), json!(n))].into_iter().collect());
                    }
                }
            }
            variants.into_iter().any(|base| {
                let run = |c: &str| {
                    let mut v = base.clone();
                    v.insert(key.to_string(), json!(c));
                    artistic_fx::run(
                        f.key(),
                        &v,
                        &src,
                        48,
                        48,
                        Rect::new(0, 0, 48, 48),
                        [0.; 4],
                        [1.; 4],
                    )
                };
                run("#ff0000") != run("#00ff00")
            })
        };
        for f in GalleryFilter::ALL {
            let id: &'static str = leak(format!("pc{}", label(f.key()).replace(' ', "")));
            let def = gallery_def(id, f.name(), Category::Effect("artistic"), "x.png");
            for key in ["foreground", "background", "glowColor"] {
                let has = def
                    .controls
                    .iter()
                    .any(|c| matches!(c, Control::Color { key: k, .. } if *k == key));
                assert_eq!(has, changes(*f, key), "{} {key}", f.key());
            }
        }
    }

    #[test]
    fn brush_strokes_submenu_lists_eight() {
        let n = super::super::effects()
            .iter()
            .filter(|d| d.category == Category::Effect("brushStrokes"))
            .count();
        assert_eq!(n, 8);
        let keys: Vec<_> = crate::effect::menu_categories(true, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(keys.contains(&"brushStrokes"));
    }

    /// Distortion 0 clamps every offset to 0, so each pixel is sampled at its own centre.
    #[test]
    fn glass_zero_distortion_is_identity() {
        let src = pattern(40, 32);
        for tex in ["frosted", "blocks", "canvas", "tinyLens"] {
            let out = run("pcGlass", json!({ "distortion": 0, "texture": tex }), &src);
            assert_eq!(out, src, "{tex}");
        }
    }

    #[test]
    fn glowing_edges_flat_is_black() {
        let src = from_fn(32, 32, |_, _| [90, 140, 200, 255]);
        let out = run("pcGlowingEdges", json!({}), &src);
        assert!(
            out.data
                .chunks(4)
                .all(|p| p[..3] == [0, 0, 0] && p[3] == 255)
        );
    }

    #[test]
    fn sketch_submenus_list_their_items() {
        let n = |k| {
            super::super::effects()
                .iter()
                .filter(|d| d.category == Category::Effect(k))
                .count()
        };
        assert_eq!((n("sketch"), n("halftone")), (13, 0));
        let keys: Vec<_> = crate::effect::menu_categories(true, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(keys.contains(&"sketch") && !keys.contains(&"halftone"));
    }

    /// Halftone Pattern sits in Distort with the Extra plus, there is no Halftone submenu, and
    /// with Extras hidden Distort is exactly Paint.NET's nine ids.
    #[test]
    fn halftone_pattern_is_in_distort_with_the_plus() {
        use crate::effect::{CATEGORIES, Provenance, menu_categories, provenance_of};
        let all = super::super::effects();
        let d = all.iter().find(|d| d.id == "pcHalftonePattern").unwrap();
        assert_eq!(d.category, Category::Effect("distort"));
        assert_eq!(provenance_of(d.id), Provenance::Extra);
        assert!(crate::action::extra_item("menu.effects.pcHalftonePattern").is_some());
        let shown = menu_categories(true, true);
        assert!(!shown.iter().any(|(k, _)| *k == "halftone"));
        fn distort(c: Vec<(&'static str, Vec<&'static str>)>) -> Vec<&'static str> {
            c.into_iter().find(|(k, _)| *k == "distort").unwrap().1
        }
        assert!(distort(shown.clone()).contains(&"pcHalftonePattern"));
        let pdn = CATEGORIES.iter().find(|(k, _)| *k == "distort").unwrap().1;
        assert_eq!(pdn.len(), 9);
        assert_eq!(distort(menu_categories(false, false)), pdn);
    }

    /// Stamp is ink on paper: with red and yellow as the palette, every pixel is a mix of them.
    #[test]
    fn sketch_items_use_primary_and_secondary() {
        let src = from_fn(48, 48, |x, y| {
            let v = ((x / 3 * 7 + y / 3 * 13) % 17 * 16) as u8;
            [v, v, v, 255]
        });
        let mut env = EffectEnv::whole(48, 48);
        env.primary = [255, 0, 0, 255];
        env.secondary = [255, 255, 0, 255];
        let out = run_env("pcStamp", json!({}), &src, &env);
        assert!(
            out.data
                .chunks(4)
                .all(|p| p[0] >= 253 && p[2] <= 2 && p[3] == 255),
            "off the red-yellow segment"
        );
        let greens: Vec<u8> = out.data.chunks(4).map(|p| p[1]).collect();
        assert!(greens.iter().min() < greens.iter().max(), "one colour only");
    }

    #[test]
    fn chrome_and_water_paper_ignore_colours() {
        let src = pattern(40, 32);
        for id in ["pcChrome", "pcWaterPaper"] {
            let def = find(id).unwrap();
            assert!(
                !def.controls
                    .iter()
                    .any(|c| matches!(c, Control::Color { .. })),
                "{id}"
            );
            let mut env = EffectEnv::whole(40, 32);
            let a = run_env(id, json!({}), &src, &env);
            env.primary = [255, 0, 0, 255];
            env.secondary = [0, 0, 255, 255];
            assert_eq!(a, run_env(id, json!({}), &src, &env), "{id}");
        }
    }

    #[test]
    fn texture_submenu_lists_six() {
        let n = super::super::effects()
            .iter()
            .filter(|d| d.category == Category::Effect("texture"))
            .count();
        assert_eq!(n, 6);
        let keys: Vec<_> = crate::effect::menu_categories(true, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert!(keys.contains(&"texture"));
    }

    /// Every gallery look is registered by exactly one manifest row, so none is left out.
    #[test]
    fn every_gallery_filter_is_registered_once() {
        for f in GalleryFilter::ALL {
            let id = format!("pc{}", label(f.key()).replace(' ', ""));
            let rows = super::super::manifest::MANIFEST
                .iter()
                .filter(|r| r.id == id)
                .count();
            assert_eq!(rows, 1, "{id}");
            assert!(find(&id).is_some(), "{id} not an effect");
        }
    }

    /// The four textures are deterministic and differ from one another. Upstream's textures are
    /// procedural with no period, so nothing here claims they tile.
    #[test]
    fn texturizer_textures_are_deterministic_and_distinct() {
        let src = from_fn(48, 48, |_, _| [128, 128, 128, 255]);
        let outs: Vec<_> = ["canvas", "brick", "burlap", "sandstone"]
            .iter()
            .map(|t| {
                let p = json!({ "texture": t, "relief": 30 });
                let a = run("pcTexturizer", p.clone(), &src);
                assert_eq!(a, run("pcTexturizer", p, &src), "{t}");
                a
            })
            .collect();
        for i in 0..4 {
            assert_ne!(outs[i], src, "texture {i} did nothing");
            for j in i + 1..4 {
                assert_ne!(outs[i], outs[j], "textures {i} and {j} match");
            }
        }
    }

    /// Stained glass is cells of one flat colour each: inside a cell, away from the borders,
    /// a smooth gradient image gives far fewer distinct colours than it has pixels.
    #[test]
    fn stained_glass_cells_are_flat() {
        let src = from_fn(64, 64, |x, y| [(x * 4) as u8, (y * 4) as u8, 128, 255]);
        let out = run(
            "pcStainedGlass",
            json!({ "cellSize": 16, "borderThickness": 1 }),
            &src,
        );
        let mut colours: Vec<[u8; 4]> = out
            .data
            .chunks(4)
            .map(|p| [p[0], p[1], p[2], p[3]])
            .collect();
        colours.sort();
        colours.dedup();
        assert!(colours.len() < 64 * 64 / 4, "{} colours", colours.len());
    }
}
