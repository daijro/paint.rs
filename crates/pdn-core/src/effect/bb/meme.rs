//! BB spec: effects/photo-meme-maker.md; common.md §1–§3, §5.6, §5.9, §5.10
//!
//! Meme Maker: a top and a bottom caption, word-wrapped to the selection bounds in Impact (else
//! the bundled Anton, R70), stroked 4 px round-joined in the outline colour, filled in the text
//! colour, drop-shadowed once from the combined overlay and composited over the source (R157).
//! All caps uses `meme_casing.json`, the adopted R157 snapshot (candidate-casing.json, SHA-256
//! ffef11206e7fae6b52c1616d6c2cb029ebac0ba6cac5c8665ccf0a8adc39d3c7): public .NET 10.0.11
//! `Rune.ToUpper` over every scalar of 907 cultures on Linux ICU 78.3, deduplicated to an
//! invariant base and per-profile overrides.
use super::blocks::{colour_exact, drop_shadow_with, shadow_offset, source_over};
use super::conventions::*;
use super::manifest::manifest;
use super::text::{GlyphRun, coverage, glyph_run, pt_to_px};
use super::ui::{ControlUi, Help, IndirectUi, Tab, TabStrip};
use crate::effect::surface::{Alpha, EffectSurface, premultiply, unpremultiply};
use crate::effect::{
    ColorDefault, Control, EffectDef, EffectEnv, EffectKind, Values, color_value, flag, num, range,
};
use crate::fonts::FaceStyle;
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::LazyLock;

const COLOR_UI: ControlUi = ControlUi {
    reset: false,
    ..ControlUi::NO_HEADER
};
static UI: IndirectUi = IndirectUi {
    tabs: &[TabStrip {
        key: "tab",
        tabs: &[
            Tab {
                caption: "\u{a0}Meme Text\u{a0}",
                keys: &["top", "topSize", "bottom", "bottomSize"],
            },
            Tab {
                caption: "\u{a0}Options\u{a0}",
                keys: &[
                    "textColor",
                    "outlineColor",
                    "swap",
                    "caps",
                    "centered",
                    "shadow",
                ],
            },
        ],
    }],
    // The plugin has no tab property; the face is this session's resolved family (R157).
    transient_keys: &["tab", "face"],
    controls: &[
        ("top", ControlUi::NO_HEADER),
        ("topSize", ControlUi::NO_HEADER),
        ("bottom", ControlUi::NO_HEADER),
        ("bottomSize", ControlUi::NO_HEADER),
        ("textColor", COLOR_UI),
        ("outlineColor", COLOR_UI),
        ("swap", ControlUi::NO_HEADER),
        ("caps", ControlUi::NO_HEADER),
        ("centered", ControlUi::NO_HEADER),
        ("shadow", ControlUi::NO_HEADER),
    ],
    fonts: |v| {
        let face = family(v);
        // Impact Bold resolves its bold face, else regular Impact (R111 synthesis); Anton has
        // only its Regular asset, drawn bold.
        let style = if face == MEME_FONT {
            FaceStyle::Bold
        } else {
            FaceStyle::Regular
        };
        vec![(face.into(), style)]
    },
    font_fallback: |v| {
        let fall = family(v) == MEME_FONT;
        if fall {
            v.insert("face".into(), json!(IMPACT_FALLBACK));
        }
        fall
    },
    help: Help::Text(&["Meme Maker v6.0"]),
    ..IndirectUi::DEFAULT
};

pub fn effects() -> Vec<EffectDef> {
    let text = |key, default| Control::Text {
        key,
        label: "",
        default,
        // R118: Paint.NET's default text-box maximum, in UTF-16 code units.
        max_len: 32767,
        multiline: true,
    };
    let colour = |key, label, rgb| Control::Color {
        key,
        label,
        default: ColorDefault::Rgb(rgb),
    };
    let check = |key, label, default| Control::Checkbox {
        key,
        label,
        default,
    };
    let m = manifest("bbMemeMaker").unwrap();
    vec![EffectDef {
        id: "bbMemeMaker",
        icon: m.icon,
        category: m.category,
        controls: vec![
            Control::Value {
                key: "tab",
                default: || json!(0),
            },
            Control::Value {
                key: "face",
                default: || json!(MEME_FONT),
            },
            text("top", "Enter top text here"),
            range("topSize", "Top text font size", 10.0, 255.0, 36.0),
            text("bottom", "Enter bottom text here"),
            range("bottomSize", "Bottom text font size", 10.0, 255.0, 36.0),
            colour("textColor", "Text color", 0xFFFFFF),
            colour("outlineColor", "Outline color", 0),
            check("swap", "Swap colors", false),
            check("caps", "All caps", true),
            check("centered", "Centered", true),
            check("shadow", "Add drop shadow", true),
        ],
        kind: EffectKind::Gpu(GPU_CONTEXT, render),
        dialog: None,
        dialog_title: Some("Meme Maker"),
        indirect: Some(&UI),
    }]
}

/// The session's resolved face (R157): Impact unless the dialog fell back to Anton.
fn family(v: &Values) -> &'static str {
    if super::text(v, "face") == IMPACT_FALLBACK {
        IMPACT_FALLBACK
    } else {
        MEME_FONT
    }
}

struct Casing {
    base: Vec<(u32, u32)>,
    overrides: Vec<Vec<(u32, u32)>>,
    cultures: BTreeMap<String, usize>,
}
static CASING: LazyLock<Casing> = LazyLock::new(|| {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Raw {
        base_profile: Vec<(u32, u32)>,
        overrides: Vec<Vec<(u32, u32)>>,
        cultures: BTreeMap<String, usize>,
    }
    let raw: Raw = serde_json::from_str(include_str!("meme_casing.json"))
        .expect("adopted R157 casing snapshot");
    Casing {
        base: raw.base_profile,
        overrides: raw.overrides,
        cultures: raw
            .cultures
            .into_iter()
            .map(|(k, v)| (k.to_lowercase(), v))
            .collect(),
    }
});

/// MEME_CURRENT_CULTURE_SIMPLE_UPPER (R157): each scalar's simple uppercase in `culture`'s
/// profile (its override, then the invariant base, then itself). One scalar always maps to one
/// scalar of the same UTF-16 width; nothing is normalized, folded or truncated.
pub fn upper(text: &str, culture: &str) -> String {
    let c = &*CASING;
    let profile = &c.overrides[*crate::number_format::culture_entry(&c.cultures, culture)];
    let find = |map: &[(u32, u32)], s: u32| {
        map.binary_search_by_key(&s, |&(k, _)| k)
            .ok()
            .map(|i| map[i].1)
    };
    text.chars()
        .map(|ch| {
            let s = ch as u32;
            find(profile, s)
                .or_else(|| find(&c.base, s))
                .and_then(char::from_u32)
                .unwrap_or(ch)
        })
        .collect()
}

/// A caption's private rendering string: an empty box is one space; All caps cases it.
fn caption(v: &Values, key: &str, culture: &str) -> String {
    let raw = text_value(v, key);
    let raw = if raw.is_empty() { " " } else { raw };
    if flag(v, "caps") {
        MEME_CURRENT_CULTURE_SIMPLE_UPPER(raw, culture)
    } else {
        raw.to_string()
    }
}
fn text_value<'a>(v: &'a Values, key: &str) -> &'a str {
    super::text(v, key)
}

/// Q67: CRLF, CR and LF each end a paragraph; empty paragraphs stay.
fn paragraphs(text: &str) -> Vec<&str> {
    let (mut out, mut start, b) = (vec![], 0, text.as_bytes());
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'\r' | b'\n' => {
                out.push(&text[start..i]);
                i += if b[i] == b'\r' && b.get(i + 1) == Some(&b'\n') {
                    2
                } else {
                    1
                };
                start = i;
            }
            _ => i += 1,
        }
    }
    out.push(&text[start..]);
    out
}

/// A variation selector stays with the scalar before it.
fn selector(c: char) -> bool {
    matches!(c, '\u{fe00}'..='\u{fe0f}' | '\u{e0100}'..='\u{e01ef}')
}

/// The byte ends of a paragraph's indivisible units: each word with its trailing separators.
fn word_ends(p: &str) -> Vec<usize> {
    let (mut ends, mut after_separator) = (vec![], false);
    for (i, c) in p.char_indices() {
        if selector(c) {
            continue;
        }
        if after_separator && !MEME_BREAK_AFTER(c) {
            ends.push(i);
        }
        after_separator = MEME_BREAK_AFTER(c);
    }
    ends.push(p.len());
    ends
}

/// `MEME_WORD_WRAP`: one paragraph's visual lines under `MEME_FIT_TRAILING_SEPARATORS`.
pub(super) fn wrap_paragraph<'a>(
    p: &'a str,
    width: f64,
    measure: &dyn Fn(&str) -> f64,
) -> Vec<&'a str> {
    wrap_paragraph_with(p, width, measure, MEME_FIT_TRAILING_SEPARATORS)
}

/// `line` without its trailing separators unless they count (`trailing`).
fn fit_part(line: &str, trailing: bool) -> &str {
    if trailing {
        line
    } else {
        line.trim_end_matches(|c| MEME_BREAK_AFTER(c) || selector(c))
    }
}

/// Each line takes the most whole units whose measured width is <= `width` (trailing
/// separators counted when `trailing`); a first unit that alone overflows stays whole.
fn wrap_paragraph_with<'a>(
    p: &'a str,
    width: f64,
    measure: &dyn Fn(&str) -> f64,
    trailing: bool,
) -> Vec<&'a str> {
    let ends = word_ends(p);
    let (mut lines, mut start, mut i) = (vec![], 0, 0);
    while i < ends.len() {
        let fits = |j: usize| measure(fit_part(&p[start..ends[j]], trailing)) <= width;
        // ponytail: galloping then binary search over the line's unit ends; equals the linear
        // greedy scan while a longer candidate never measures narrower (non-negative advances
        // from one line start; Text picks its fallback from that start's first unit).
        let (mut lo, mut step, mut hi) = (i, 1, ends.len());
        while lo + 1 < hi {
            let j = (lo + step).min(hi - 1);
            if fits(j) {
                lo = j;
                step *= 2;
            } else {
                hi = j;
                break;
            }
        }
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if fits(mid) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lines.push(&p[start..ends[lo]]);
        start = ends[lo];
        i = lo + 1;
    }
    lines
}

/// R157 Q67: the visual lines of (already cased) `text` in `family`, measured bold at `px` with
/// the outline projection the render draws. Hard breaks are kept, a word wider than `width`
/// overflows unbroken, separators stay at the end of their line.
pub fn wrap_lines(text: &str, family: &str, px: f32, width: f32) -> Vec<String> {
    let measure =
        |s: &str| glyph_run(&MEME_OUTLINE_PRESENTATION(s), family, px, true, [0.0; 2]).width;
    paragraphs(text)
        .into_iter()
        .flat_map(|p| MEME_WORD_WRAP(p, width as f64, &measure))
        .map(str::to_string)
        .collect()
}

/// Both captions' visual lines and their runs, placed: the top block's first line box at the
/// bounds top, the bottom block's last one ending at the bounds bottom; each line centred in
/// (or at the left of) the bounds width; everything shifted +0.5 px.
fn blocks(v: &Values, env: &EffectEnv) -> [(Vec<String>, GlyphRun); 2] {
    let (b, face) = (env.bounds, family(v));
    let block = |key: &str, size: &str, bottom: bool| {
        let px = pt_to_px(num(v, size).clamp(10.0, 255.0) as f32);
        let lines = wrap_lines(&caption(v, key, env.culture), face, px, b.w as f32);
        let projected: Vec<_> = lines.iter().map(|l| MEME_OUTLINE_PRESENTATION(l)).collect();
        let mut run = glyph_run(&projected.join("\n"), face, px, true, [0.0; 2]);
        let mut y = if bottom {
            b.bottom() as f64 - run.height
        } else {
            b.y as f64
        };
        replace_bitmaps(&mut run, face, px, true);
        for (l, raw) in run.layout.lines.iter_mut().zip(&lines) {
            let dx = if flag(v, "centered") {
                (b.w as f64 - align_width(raw, l.width, face, px, MEME_ALIGN_TRAILING_SEPARATORS))
                    / 2.0
            } else {
                0.0
            };
            l.origin = [b.x as f64 + dx + 0.5, y + 0.5];
            y += l.height;
        }
        (lines, run)
    };
    [
        block("top", "topSize", false),
        block("bottom", "bottomSize", true),
    ]
}

/// The width a centred line is centred by: its `measured` advance, or without its trailing
/// separators unless they count (`trailing`).
fn align_width(line: &str, measured: f64, family: &str, px: f32, trailing: bool) -> f64 {
    if trailing {
        measured
    } else {
        let trimmed = MEME_OUTLINE_PRESENTATION(fit_part(line, false));
        glyph_run(&trimmed, family, px, true, [0.0; 2]).width
    }
}

/// R157 Q66: the VS15 projection leaves no colour bitmap, but should one appear it is not
/// dropped: it draws the ordinary visible missing-glyph outline (Text's own outline for a scalar
/// no face covers) at its pen position on its baseline, its advance kept. Chart text shares it.
pub(super) fn replace_bitmaps(run: &mut GlyphRun, family: &str, px: f32, bold: bool) {
    use crate::tool::text::DrawItem;
    for l in &mut run.layout.lines {
        if !l.items.iter().any(|i| matches!(i, DrawItem::Bitmap(_))) {
            continue;
        }
        l.items.retain(|i| matches!(i, DrawItem::Outline(_)));
        for g in &l.glyphs {
            let bitmap =
                ttf_parser::Face::parse(&g.face, 0).is_ok_and(|f| f.tables().cbdt.is_some());
            if !bitmap {
                continue;
            }
            // U+FFFF is a noncharacter: no face maps it.
            let missing = glyph_run("\u{ffff}", family, px, bold, [0.0; 2]);
            let at = tiny_skia::Transform::from_translate(
                g.origin[0] as f32,
                (g.origin[1] - missing.ascent) as f32,
            );
            l.items.extend(
                missing.layout.lines[0]
                    .items
                    .iter()
                    .filter_map(|i| match i {
                        DrawItem::Outline(p) => p.clone().transform(at).map(DrawItem::Outline),
                        DrawItem::Bitmap(_) => None,
                    }),
            );
        }
    }
}

fn render(src: &EffectSurface, v: &Values, env: &EffectEnv) -> EffectSurface {
    let info = env.render_info.unwrap_or(GPU_CONTEXT);
    let colour = |key| colour_exact(color_value(v, key, env), info);
    let (mut fill, mut rim) = (colour("textColor"), colour("outlineColor"));
    if flag(v, "swap") {
        std::mem::swap(&mut fill, &mut rim);
    }
    let (w, h) = (src.w, src.h);
    let mut overlay = EffectSurface::new(w, h);
    for (_, run) in blocks(v, env) {
        let paths = run.outlines();
        // Each block strokes every outline first, then fills them all over it.
        for (cov, c) in [
            (coverage(&paths, Some(4.0), w, h), rim),
            (coverage(&paths, None, w, h), fill),
        ] {
            for (p, k) in overlay.px.iter_mut().zip(cov) {
                *p = source_over(*p, c.map(|c| c * k));
            }
        }
    }
    let overlay = if flag(v, "shadow") {
        drop_shadow_with(
            &overlay,
            4.0 * RADIUS_TO_SIGMA,
            shadow_offset(4.0, 315.0 + SHADOW_ANGLE_OFFSET_DEG),
            [0.0, 0.0, 0.0, 191.0 / 255.0],
            false,
        )
    } else {
        overlay
    };
    // Source-over is premultiplied: a straight boundary converts around it.
    EffectSurface::from_fn(w, h, |x, y| {
        let s = src.at(x, y);
        let s = if info.input == Alpha::Straight {
            premultiply(s)
        } else {
            s
        };
        let out = source_over(s, overlay.at(x, y));
        if info.output == Alpha::Straight {
            unpremultiply(out)
        } else {
            out
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::ImageData;
    use crate::effect::bb::conventions::{
        ALT_GPU_CONTEXT, GPU_CONTEXT, IMPACT_FALLBACK, MEME_FIT_TRAILING_SEPARATORS, MEME_FONT,
        MEME_OUTLINE_PRESENTATION, MEME_WORD_WRAP,
    };
    use crate::effect::bb::test_util::*;
    use crate::effect::bb::text::{coverage, glyph_run, pt_to_px};
    use crate::effect::bb::ui::{Help, IndirectUi};
    use crate::effect::{Category, ColorDefault, Control, EffectKind, RenderInfo, find};
    use crate::fonts::{self, FaceStyle};
    use crate::pixel::gamma::SRGB8_TO_LINEAR;
    use crate::tool::text::DrawItem;
    use serde_json::{Value, json};

    const ANTON: &[u8] = include_bytes!("../../../../../assets/fonts/anton/Anton-Regular.ttf");
    const G: [u8; 4] = [128, 128, 128, 255];
    /// Anton's OS/2 sCapHeight in its 2048-unit em.
    const ANTON_CAP: f64 = 1760.0 / 2048.0;

    /// R70 Decision 2: the tests pin the bundled Anton, registered by its own name (never as
    /// Impact) and never unregistered, so parallel tests cannot race a removal.
    fn anton() {
        fonts::register_lazy_face(IMPACT_FALLBACK, FaceStyle::Regular, ANTON.to_vec()).unwrap();
    }
    fn def() -> EffectDef {
        find("bbMemeMaker").unwrap()
    }
    fn values(overrides: Value, env: &EffectEnv) -> Values {
        anton();
        let mut v = crate::effect::bb::indirect_defaults(&def(), env);
        v.insert("face".into(), json!(IMPACT_FALLBACK));
        v.extend(overrides.as_object().unwrap().clone());
        v
    }
    fn meme_env(overrides: Value, src: &ImageData, env: &EffectEnv) -> ImageData {
        let v = values(overrides, env);
        crate::effect::apply(src, &def(), &v, env)
    }
    fn meme(overrides: Value, src: &ImageData) -> ImageData {
        meme_env(overrides, src, &EffectEnv::whole(src.width, src.height))
    }
    fn near(a: [u8; 4], b: [u8; 4], tol: u8) -> bool {
        a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= tol)
    }
    /// The changed pixels' bounds (l, t, r, b) within `rows`.
    fn ink_in(
        img: &ImageData,
        base: [u8; 4],
        rows: std::ops::Range<u32>,
    ) -> Option<(u32, u32, u32, u32)> {
        let mut b: Option<(u32, u32, u32, u32)> = None;
        for y in rows {
            for x in 0..img.width {
                if !near(px(img, x, y), base, 8) {
                    b = Some(b.map_or((x, y, x, y), |(l, t, r, bt)| {
                        (l.min(x), t.min(y), r.max(x), bt.max(y))
                    }));
                }
            }
        }
        b
    }
    /// Both blocks' stroke and fill coverage, as the render paints them.
    fn masks(v: &Values, env: &EffectEnv) -> (Vec<f32>, Vec<f32>) {
        let paths: Vec<_> = blocks(v, env)
            .iter()
            .flat_map(|(_, r)| r.outlines())
            .collect();
        (
            coverage(&paths, Some(4.0), env.width, env.height),
            coverage(&paths, None, env.width, env.height),
        )
    }
    fn lin(c: u8) -> f32 {
        SRGB8_TO_LINEAR[c as usize]
    }

    /// photo-meme-maker.md Expected results 1
    #[test]
    fn defaults_on_mid_grey() {
        let src = solid(600, 400, G);
        let env = EffectEnv::whole(600, 400);
        let v = values(json!({}), &env);
        let [(top_lines, top), (bottom_lines, bottom)] = blocks(&v, &env);
        assert_eq!(top_lines, ["ENTER TOP TEXT HERE"]);
        assert_eq!(bottom_lines, ["ENTER BOTTOM TEXT HERE"]);
        let flat = meme(json!({"shadow": false}), &src);
        let (tl, tt, tr, _) = ink_in(&flat, G, 0..200).unwrap();
        let (bl, _, br, bb_) = ink_in(&flat, G, 200..400).unwrap();
        // Each block centred on x = 300.
        for (l, r) in [(tl, tr), (bl, br)] {
            assert!(((l + r) as f64 / 2.0 - 300.0).abs() <= 2.0, "{l}..{r}");
        }
        // Ink tops: cap top (one ascent below the line box at y = 0.5) less the synthetic bold
        // px/48 and the 2 px rim. Anton's ascent (2409/2048 em) puts it ~13 px down, Impact's
        // ~8 px: the spec's "a few pixels" assumes Impact (report).
        let ink_top = 0.5 + top.ascent - ANTON_CAP * 48.0 - 1.0 - 2.0;
        assert_eq!(top.layout.lines[0].origin[1], 0.5);
        assert!(
            (tt as f64 - ink_top.floor()).abs() <= 1.0,
            "top {tt} want {ink_top}"
        );
        assert!(tt <= 14, "top ink {tt}");
        // Bottom: the last line box ends at the selection bottom, ink a descender above it.
        let line = &bottom.layout.lines[0];
        assert!((line.origin[1] + line.height - 400.5).abs() < 1e-9);
        let ink_bottom = line.origin[1] + bottom.ascent + 1.0 + 2.0;
        assert!(
            (bb_ as f64 - ink_bottom.floor()).abs() <= 1.0,
            "bottom {bb_}"
        );
        assert!((380..=395).contains(&bb_), "descender space {bb_}");
        let out = meme(json!({}), &src);
        let (stroke, fill) = masks(&v, &env);
        let (mut white, mut black, mut shadow, mut above, mut below) = (0, 0, vec![], 0, 0);
        for y in 0..400 {
            for x in 0..600 {
                let (i, p) = ((y * 600 + x) as usize, px(&out, x, y));
                if fill[i] == 1.0 {
                    assert!(near(p, [255; 4], TOL), "interior ({x}, {y}) {p:?}");
                    white += 1;
                } else if stroke[i] == 1.0 && fill[i] == 0.0 {
                    assert!(near(p, [0, 0, 0, 255], TOL), "rim ({x}, {y}) {p:?}");
                    black += 1;
                } else if stroke[i] == 0.0 && fill[i] == 0.0 && p != G {
                    // Shadow only: dark, translucent (alpha 191 leaves a quarter of the grey).
                    assert!(p[..3].iter().all(|&c| (50..128).contains(&c)) && p[3] == 255);
                    shadow.push((x, y));
                    if y < 200 && y < tt {
                        above += 1;
                    }
                    if y < 200 && y > ink_top as u32 + 41 {
                        below += 1;
                    }
                }
            }
        }
        assert!(white > 1000 && black > 1000 && shadow.len() > 500);
        assert!(
            below > 4 * above.max(1),
            "down-right shadow {below} / {above}"
        );
        // The rim is about 2 px: from the first changed pixel to the first white on the E's
        // left stem's middle row.
        let mid = (0.5 + top.ascent - ANTON_CAP * 24.0) as u32;
        let x0 = (0..600).find(|&x| px(&flat, x, mid) != G).unwrap();
        let x1 = (x0..600)
            .find(|&x| near(px(&flat, x, mid), [255; 4], TOL))
            .unwrap();
        assert!((1..=3).contains(&(x1 - x0)), "rim {x0}..{x1}");
        // The middle rows are untouched.
        for y in 100..300 {
            assert!((0..600).all(|x| px(&out, x, y) == G), "row {y}");
        }
    }

    /// photo-meme-maker.md Expected results 2
    #[test]
    fn empty_texts_leave_the_input() {
        for base in [G, [0, 0, 0, 0], [100, 150, 200, 128]] {
            let src = solid(60, 40, base);
            for shadow in [true, false] {
                for (top, bottom) in [("", ""), (" ", ""), ("", "   "), ("\n", " \r\n ")] {
                    let out = meme(
                        json!({"top": top, "bottom": bottom, "shadow": shadow}),
                        &src,
                    );
                    for y in 0..40 {
                        for x in 0..60 {
                            assert_px(&out, x, y, base);
                        }
                    }
                }
            }
        }
        // An empty block adds nothing next to a non-empty one.
        let src = solid(200, 300, G);
        let out = meme(json!({"top": "", "bottom": "X"}), &src);
        assert!(ink_in(&out, G, 0..200).is_none());
        assert!(ink_in(&out, G, 200..300).is_some());
    }

    /// photo-meme-maker.md Expected results 3
    #[test]
    fn swapped_colours_without_shadow() {
        let src = solid(200, 120, G);
        let env = EffectEnv::whole(200, 120);
        let o = json!({"top": "A", "bottom": "", "shadow": false, "swap": true});
        let swapped = meme(o.clone(), &src);
        let plain = meme(json!({"top": "A", "bottom": "", "shadow": false}), &src);
        let (stroke, fill) = masks(&values(o, &env), &env);
        let covered: Vec<(f64, f64)> = (0..120 * 200)
            .filter(|&i| fill[i] > 0.0)
            .map(|i| ((i % 200) as f64, (i / 200) as f64))
            .collect();
        let (mut interior, mut rim) = (0, 0);
        for y in 0..120 {
            for x in 0..200 {
                let i = (y * 200 + x) as usize;
                let (s, p) = (px(&swapped, x, y), px(&plain, x, y));
                if fill[i] == 1.0 {
                    assert!(near(s, [0, 0, 0, 255], TOL) && near(p, [255; 4], TOL));
                    interior += 1;
                } else if stroke[i] == 1.0 && fill[i] == 0.0 {
                    assert!(near(s, [255; 4], TOL) && near(p, [0, 0, 0, 255], TOL));
                    rim += 1;
                }
                let d = covered
                    .iter()
                    .map(|&(cx, cy)| (cx - x as f64).hypot(cy - y as f64))
                    .fold(f64::INFINITY, f64::min);
                if d > 3.0 {
                    assert_eq!(s, G, "({x}, {y}) is {d:.2} px from the glyph");
                }
            }
        }
        assert!(interior > 200 && rim > 100);
    }

    /// photo-meme-maker.md Expected results 4
    #[test]
    fn left_aligned_wrapping() {
        anton();
        assert_eq!(pt_to_px(36.0), 48.0);
        let lines = wrap_lines("AAAA AAAA AAAA", IMPACT_FALLBACK, 48.0, 200.0);
        // The text wraps onto several lines. Anton's "AAAA" is 93.2 px at 48 px, not the
        // spec's ~110 px Impact estimate (report): "AAAA AAAA " (208.9 px, its separator
        // counted, Q67) overflows 200, but the last "AAAA AAAA" (197.6 px) fits.
        assert_eq!(lines, ["AAAA ", "AAAA AAAA"]);
        let width = |t| glyph_run(t, IMPACT_FALLBACK, 48.0, true, [0.0; 2]).width;
        assert!((width("AAAA") - 4.0 * 994.0 * 48.0 / 2048.0).abs() < 0.01);
        assert!(width("AAAA AAAA ") > 200.0 && width("AAAA AAAA") <= 200.0);
        let src = solid(200, 400, G);
        let env = EffectEnv::whole(200, 400);
        let o = json!({"top": "AAAA AAAA AAAA", "bottom": "", "centered": false});
        let out = meme(o.clone(), &src);
        let [(top_lines, top), _] = blocks(&values(o, &env), &env);
        assert_eq!(top_lines, lines);
        for l in &top.layout.lines {
            assert_eq!(l.origin[0], 0.5);
            let rows = l.origin[1] as u32 + 4..(l.origin[1] + l.height) as u32;
            let (left, ..) = ink_in(&out, G, rows).unwrap();
            assert!(left <= 3, "line at {} starts at x = {left}", l.origin[1]);
        }
    }

    /// photo-meme-maker.md Expected results 5
    #[test]
    fn selection_placement() {
        let src = solid(600, 400, G);
        let (m, p) = rect_selection(600, 400, 100, 100, 400, 300);
        let env = with_selection(600, 400, &m, &p);
        let v = values(json!({}), &env);
        let [(top_lines, top), (_, bottom)] = blocks(&v, &env);
        assert_eq!(top_lines.concat(), "ENTER TOP TEXT HERE");
        assert_eq!(top.layout.lines[0].origin[1], 100.5);
        let last = bottom.layout.lines.last().unwrap();
        assert!((last.origin[1] + last.height - 300.5).abs() < 1e-9);
        for (lines, run) in [&blocks(&v, &env)[0], &blocks(&v, &env)[1]] {
            assert!(lines.len() >= 2, "300 px wraps {lines:?}");
            for l in &run.layout.lines {
                assert!((l.origin[0] - 0.5 + l.width / 2.0 - 250.0).abs() < 1e-9);
            }
        }
        let out = meme_env(json!({}), &src, &env);
        for y in 0..400 {
            for x in 0..600 {
                if !(100..400).contains(&x) || !(100..300).contains(&y) {
                    assert_eq!(px(&out, x, y), G, "outside ({x}, {y})");
                }
            }
        }
        let flat = meme_env(json!({"shadow": false}), &src, &env);
        let (l, t, r, _) = ink_in(&flat, G, 100..200).unwrap();
        assert!((100..=116).contains(&t) && ((l + r) as f64 / 2.0 - 250.0).abs() <= 6.0);
        let (l, _, r, b) = ink_in(&flat, G, 200..300).unwrap();
        assert!((280..300).contains(&b) && ((l + r) as f64 / 2.0 - 250.0).abs() <= 6.0);
    }

    /// Expected results 5 extended: odd, offset bounds lay out from the bounds; a fractional
    /// non-rectangular mask clips the bounds layout once, after the render.
    #[test]
    fn selection_bounds_and_fractional_masks() {
        let src = solid(320, 260, G);
        let (m, p) = rect_selection(320, 260, 33, 17, 290, 251);
        let env = with_selection(320, 260, &m, &p);
        let [(_, top), (_, bottom)] = blocks(&values(json!({}), &env), &env);
        assert_eq!(top.layout.lines[0].origin[1], 17.5);
        let last = bottom.layout.lines.last().unwrap();
        assert!((last.origin[1] + last.height - 251.5).abs() < 1e-9);
        for l in top.layout.lines.iter().chain(&bottom.layout.lines) {
            assert!((l.origin[0] - 0.5 + l.width / 2.0 - (33.0 + 257.0 / 2.0)).abs() < 1e-9);
        }
        let (mask, polys) = ellipse_selection(320, 260, [160.0, 130.0], [140.0, 120.0]);
        let masked = with_selection(320, 260, &mask, &polys);
        let unmasked = EffectEnv {
            mask: None,
            ..masked
        };
        let full = meme_env(json!({}), &src, &unmasked);
        let mut want = full.clone();
        crate::effect::underwrite(&src, &mut want, &mask);
        assert_eq!(meme_env(json!({}), &src, &masked).data, want.data);
        assert_ne!(full.data, want.data, "the mask clipped some ink");
    }

    /// R157 Q14: All caps maps scalars through the captured culture's public .NET snapshot.
    #[test]
    fn caps_follow_the_captured_culture() {
        for (cultures, outputs) in PROBE_CLASSES {
            for culture in cultures {
                for (input, want) in PROBE_INPUTS.iter().zip(outputs) {
                    assert_eq!(upper(input, culture), want, "{culture:?} {input:?}");
                }
            }
        }
        // The native observation: straße iı with caps on equals STRAßE II with caps off.
        for c in ["en", "en-US", "fr", "fr-FR"] {
            assert_eq!(upper("straße iı", c), "STRAßE II");
        }
        // Exact name, then parents, then English; only the explicit invariant name is invariant.
        assert_eq!(upper("i", "tr-DE"), "İ");
        assert_eq!(upper("i", "az-Arab-IR"), "İ");
        assert_eq!(upper("i", "TR_tr"), "İ");
        for unknown in ["xx", "xx-YY", "-x", "zz-"] {
            assert_eq!(upper("ıi", unknown), "II", "{unknown:?}");
        }
        assert_eq!(upper("ıi", ""), "ıI");
        // Raw newlines and selectors pass; nothing is normalized or truncated.
        let long = "ß".repeat(32767);
        assert_eq!(upper(&long, "en"), long);
        assert_eq!(upper("a\r\nb\u{fe0f}", "en"), "A\r\nB\u{fe0f}");
    }

    /// R157: the embedded snapshot is the adopted candidate (27902 bytes, 907 cultures, three
    /// profiles with memberships 1 / 898 / 8).
    #[test]
    fn casing_snapshot_is_the_adopted_data() {
        let raw = include_str!("meme_casing.json");
        assert_eq!(raw.len(), 27902);
        let d: Value = serde_json::from_str(raw).unwrap();
        assert_eq!(d["baseProfile"].as_array().unwrap().len(), 1504);
        assert_eq!(
            d["overrides"],
            json!([[], [[305, 73]], [[105, 304], [305, 73]]])
        );
        let cultures = d["cultures"].as_object().unwrap();
        assert_eq!(cultures.len(), 907);
        let members = |p| cultures.values().filter(|v| **v == json!(p)).count();
        assert_eq!([members(0), members(1), members(2)], [1, 898, 8]);
        let mut turkic: Vec<_> = cultures
            .iter()
            .filter(|(_, v)| **v == json!(2))
            .map(|(k, _)| k.as_str())
            .collect();
        turkic.sort();
        assert_eq!(
            turkic,
            [
                "az",
                "az-Cyrl",
                "az-Cyrl-AZ",
                "az-Latn",
                "az-Latn-AZ",
                "tr",
                "tr-CY",
                "tr-TR"
            ]
        );
    }

    /// R157: caps off keeps the raw caption; caps on cases before wrapping; empty is a space.
    #[test]
    fn captions_are_cased_privately_before_wrapping() {
        let mut env = EffectEnv::whole(400, 300);
        let raw = "a i\r\nb";
        let v = values(json!({"top": raw, "caps": false}), &env);
        assert_eq!(caption(&v, "top", "tr"), raw);
        assert_eq!(super::text_value(&v, "top"), raw);
        let v = values(json!({"top": raw}), &env);
        assert_eq!(caption(&v, "top", "tr"), "A İ\r\nB");
        env.culture = "tr";
        assert_eq!(blocks(&v, &env)[0].0, ["A İ", "B"]);
        env.culture = "en";
        assert_eq!(blocks(&v, &env)[0].0, ["A I", "B"]);
        let v = values(json!({"top": "", "bottom": ""}), &env);
        assert_eq!(caption(&v, "top", "en"), " ");
        assert_eq!(blocks(&v, &env)[1].0, [" "]);
        assert_eq!(v["top"], json!(""), "the value is not rewritten");
    }

    /// R157 Q67: hard breaks, separators and the measured greedy fit.
    #[test]
    fn word_wrap_boundaries() {
        anton();
        let f = IMPACT_FALLBACK;
        let w = |t: &str, width: f32| wrap_lines(t, f, 48.0, width);
        assert_eq!(w("A\r\nB\rC\nD", 1e6), ["A", "B", "C", "D"]);
        assert_eq!(w("\n\nA\n", 1e6), ["", "", "A", ""]);
        assert_eq!(w("\r\n\r\n", 1e6), ["", "", ""]);
        assert_eq!(w("\r\r\n\n", 1e6), ["", "", "", ""]);
        assert_eq!(w("", 1e6), [""]);
        assert_eq!(w("A  B", 1e6), ["A  B"]);
        assert_eq!(w("A  B", 0.0), ["A  ", "B"]);
        assert_eq!(w("A\tB", 0.0), ["A\t", "B"]);
        assert_eq!(w("A\u{3000}B", 0.0), ["A\u{3000}", "B"]);
        assert_eq!(w("A\u{a0}B", 0.0), ["A\u{a0}B"]);
        assert_eq!(w("A\u{202f}B", 0.0), ["A\u{202f}B"]);
        assert_eq!(w("A\u{200b}B", 0.0), ["A\u{200b}", "B"]);
        assert_eq!(w("A B C", 0.0), ["A ", "B ", "C"]);
        assert_eq!(w("  A B", 0.0), ["  ", "A ", "B"]);
        assert_eq!(w("   ", 0.0), ["   "]);
        assert_eq!(w("ABCDEFGHIJ KL", 50.0), ["ABCDEFGHIJ ", "KL"]);
        assert_eq!(w("e\u{301}e\u{301} x", 0.0), ["e\u{301}e\u{301} ", "x"]);
        assert_eq!(w("𐐨𐐨 𐐨", 0.0), ["𐐨𐐨 ", "𐐨"]);
        assert_eq!(w("A\u{fe0f} B\u{fe0e}", 0.0), ["A\u{fe0f} ", "B\u{fe0e}"]);
        assert_eq!(w("A \u{fe0f}B", 0.0), ["A \u{fe0f}", "B"]);
        assert_eq!(w("😀 😀", 0.0), ["😀 ", "😀"]);
        // Exact fit is <=, trailing separators included.
        let two = glyph_run("AAAA AAAA ", f, 48.0, true, [0.0; 2]).width as f32;
        assert_eq!(w("AAAA AAAA AAAA", two), ["AAAA AAAA ", "AAAA"]);
        assert_eq!(w("AAAA AAAA AAAA", two.next_up()), ["AAAA AAAA ", "AAAA"]);
        assert_eq!(w("AAAA AAAA AAAA", two.next_down()), ["AAAA ", "AAAA AAAA"]);
        let one = glyph_run("AAAA ", f, 48.0, true, [0.0; 2]).width as f32;
        assert_eq!(w("AAAA AAAA AAAA", one), ["AAAA ", "AAAA ", "AAAA"]);
        let all = glyph_run("AAAA AAAA AAAA", f, 48.0, true, [0.0; 2]).width as f32;
        assert_eq!(w("AAAA AAAA AAAA", all), ["AAAA AAAA AAAA"]);
    }

    /// R157 Q67: the trailing-separator rule, both settings, for fit and for the centring
    /// width; the named convention is the paragraph wrapper itself.
    #[test]
    fn trailing_separator_rule_both_settings() {
        anton();
        let f = IMPACT_FALLBACK;
        let measure = |s: &str| glyph_run(s, f, 48.0, true, [0.0; 2]).width;
        let p = "AAAA AAAA AAAA";
        assert_eq!(
            wrap_paragraph_with(p, 200.0, &measure, true),
            ["AAAA ", "AAAA AAAA"]
        );
        // DirectWrite-style: the separator hangs, "AAAA AAAA" (197.6 px) fits first.
        assert_eq!(
            wrap_paragraph_with(p, 200.0, &measure, false),
            ["AAAA AAAA ", "AAAA"]
        );
        assert_eq!(
            MEME_WORD_WRAP(p, 200.0, &measure),
            wrap_paragraph_with(p, 200.0, &measure, MEME_FIT_TRAILING_SEPARATORS)
        );
        // Leading separators stay their own unit; a whitespace-only line fits at any width
        // when its separators hang.
        assert_eq!(
            wrap_paragraph_with("  A B", 0.0, &measure, true),
            ["  ", "A ", "B"]
        );
        assert_eq!(
            wrap_paragraph_with("  A B", 0.0, &measure, false),
            ["  ", "A ", "B"]
        );
        assert_eq!(
            wrap_paragraph_with("A  B", 60.0, &measure, false),
            ["A  ", "B"]
        );
        let line = "AB  \u{200b}";
        let with = glyph_run(&MEME_OUTLINE_PRESENTATION(line), f, 48.0, true, [0.0; 2]).width;
        let without = glyph_run("AB", f, 48.0, true, [0.0; 2]).width;
        assert!(with > without);
        assert_eq!(align_width(line, with, f, 48.0, true), with);
        assert_eq!(align_width(line, with, f, 48.0, false), without);
        assert_eq!(align_width("   ", 30.0, f, 48.0, false), 0.0);
    }

    /// Unregisters its test family when the test ends, passing or failing.
    struct Unregister(&'static str);
    impl Drop for Unregister {
        fn drop(&mut self) {
            fonts::unregister_family(self.0);
        }
    }

    /// R157 Q66, release-safe: a colour bitmap the projection should never leave is not
    /// dropped; it draws the ordinary visible missing-glyph outline at its pen position.
    #[test]
    fn unexpected_bitmaps_draw_the_missing_glyph_outline() {
        const NOTO: &[u8] =
            include_bytes!("../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        fonts::register_lazy_face("bb29-bitmap-run", FaceStyle::Regular, NOTO.to_vec()).unwrap();
        let _unregister = Unregister("bb29-bitmap-run");
        let mut run = glyph_run("A😀", "bb29-bitmap-run", 30.0, true, [10.0, 20.0]);
        assert!(
            run.layout.lines[0]
                .items
                .iter()
                .any(|i| matches!(i, DrawItem::Bitmap(_)))
        );
        let plain = run.outlines().len();
        replace_bitmaps(&mut run, "bb29-bitmap-run", 30.0, true);
        let line = &run.layout.lines[0];
        assert!(line.items.iter().all(|i| matches!(i, DrawItem::Outline(_))));
        let paths = run.outlines();
        assert!(paths.len() > plain, "the bitmap is replaced, not dropped");
        let emoji = &line.glyphs[1];
        let (pen, baseline) = (
            line.origin[0] + emoji.origin[0],
            line.origin[1] + emoji.origin[1],
        );
        for p in &paths[plain..] {
            let b = p.bounds();
            assert!(
                b.left() as f64 >= pen - 1.0 && (b.top() as f64) < baseline,
                "{b:?}"
            );
            assert!(
                (b.bottom() as f64) <= baseline + 1.0 && (b.right() as f64) < pen + emoji.advance
            );
        }
        // A projected run has nothing to replace.
        anton();
        let mut projected = glyph_run(
            &MEME_OUTLINE_PRESENTATION("A😀"),
            IMPACT_FALLBACK,
            30.0,
            true,
            [0.0; 2],
        );
        let before = projected.outlines().len();
        replace_bitmaps(&mut projected, IMPACT_FALLBACK, 30.0, true);
        assert_eq!(projected.outlines().len(), before);
    }

    /// R157: the wrap measures every candidate at the full 32767-unit capacity without a
    /// hidden smaller limit.
    #[test]
    fn long_captions_wrap_at_full_capacity() {
        anton();
        let word = "A".repeat(32767);
        assert_eq!(wrap_lines(&word, IMPACT_FALLBACK, 48.0, 600.0), [word]);
        let words = "a ".repeat(16383) + "a";
        let lines = wrap_lines(&words, IMPACT_FALLBACK, pt_to_px(10.0), 4000.0);
        assert_eq!(lines.concat(), words);
        assert!(lines.len() > 5 && lines.iter().all(|l| l.starts_with('a')));
    }

    /// R157 Q66: the private VS15 projection draws outlines (the visible missing-glyph outline
    /// for bitmap-only scalars); raw text and the generic colour route are unchanged.
    #[test]
    fn outline_presentation_never_paints_bitmaps() {
        const NOTO: &[u8] =
            include_bytes!("../../../../../assets/fonts/noto-color-emoji/NotoColorEmoji.ttf");
        fonts::register_lazy_face("bb29-colour-run", FaceStyle::Regular, NOTO.to_vec()).unwrap();
        let _unregister = Unregister("bb29-colour-run");
        let raw = "😀\u{fe0f} A";
        assert_eq!(
            MEME_OUTLINE_PRESENTATION(raw),
            "😀\u{fe0e} \u{fe0e}A\u{fe0e}"
        );
        let generic = glyph_run(raw, "bb29-colour-run", 20.0, true, [0.0; 2]);
        assert!(matches!(
            generic.layout.lines[0].items[0],
            DrawItem::Bitmap(_)
        ));
        let projected = glyph_run(
            &MEME_OUTLINE_PRESENTATION(raw),
            "bb29-colour-run",
            20.0,
            true,
            [0.0; 2],
        );
        let items = &projected.layout.lines[0].items;
        assert!(!items.is_empty() && items.iter().all(|i| matches!(i, DrawItem::Outline(_))));
        let src = solid(200, 120, G);
        let out = meme(
            json!({"top": "😀\u{fe0f}", "bottom": "", "shadow": false}),
            &src,
        );
        let all: Vec<_> = out.data.chunks(4).collect();
        assert!(
            all.iter()
                .any(|p| near((*p).try_into().unwrap(), [255; 4], TOL))
        );
        assert!(
            all.iter()
                .any(|p| near((*p).try_into().unwrap(), [0, 0, 0, 255], TOL))
        );
    }

    /// common.md §1, R88: colours in the effective working space, both contexts, opaque and
    /// translucent sources; the shadow scales each working-space channel alike.
    #[test]
    fn colours_and_shadow_follow_the_effective_context() {
        for info in [GPU_CONTEXT, ALT_GPU_CONTEXT] {
            for base in [[30, 90, 150, 255], [200, 120, 40, 160]] {
                let src = solid(160, 110, base);
                let env = EffectEnv::whole(160, 110);
                let o = json!({"top": "HO", "bottom": "", "textColor": "#3366cc",
                    "outlineColor": "#cc6633"});
                let mut d = def();
                let EffectKind::Gpu(_, render) = d.kind else {
                    panic!()
                };
                d.kind = EffectKind::Gpu(info, render);
                let v = values(o, &env);
                let out = crate::effect::apply(&src, &d, &v, &env);
                let (stroke, fill) = masks(&v, &env);
                let linear = info.space == crate::effect::surface::Space::Linear;
                let decode = |c: u8| if linear { lin(c) } else { c as f32 / 255.0 };
                let encode = |w: f32| {
                    if linear {
                        crate::pixel::gamma::linear_to_srgb8(w)
                    } else {
                        crate::pixel::gamma::quantize(w)
                    }
                };
                let mut shadowed = 0;
                for y in 0..110 {
                    for x in 0..160 {
                        let (i, p) = ((y * 160 + x) as usize, px(&out, x, y));
                        if fill[i] == 1.0 {
                            assert!(near(p, [0x33, 0x66, 0xcc, 255], TOL), "{p:?}");
                        } else if stroke[i] == 1.0 && fill[i] == 0.0 {
                            assert!(near(p, [0xcc, 0x66, 0x33, 255], TOL), "{p:?}");
                        } else if stroke[i] == 0.0 && !near(p, base, TOL) {
                            // Black shadow over the source: every premultiplied working
                            // channel × (1 − a), alpha a + αs(1 − a), a ≤ 191/255. Estimate a
                            // from alpha (or the brightest channel), then predict all bytes.
                            let (ab, ap) = (base[3] as f32 / 255.0, p[3] as f32 / 255.0);
                            let a = if base[3] < 255 {
                                (ap - ab) / (1.0 - ab)
                            } else {
                                let c = (0..3).max_by_key(|&c| base[c]).unwrap();
                                1.0 - decode(p[c]) / decode(base[c])
                            };
                            assert!((0.0..=191.0 / 255.0 + 0.02).contains(&a), "{a}");
                            let out_a = a + ab * (1.0 - a);
                            for c in 0..3 {
                                let w = decode(base[c]) * ab * (1.0 - a) / out_a;
                                let want = encode(w);
                                assert!(p[c].abs_diff(want) <= 3, "{info:?} {c} {p:?} {want} {a}");
                            }
                            assert!((ap - out_a).abs() <= 1.5 / 255.0, "{p:?}");
                            shadowed += 1;
                        } else if stroke[i] == 0.0 {
                            assert_px(&out, x, y, base);
                        }
                    }
                }
                assert!(shadowed > 50, "{info:?} {base:?}");
            }
        }
        // Straight boundaries convert around the premultiplied source-over.
        let src = solid(80, 60, [200, 120, 40, 160]);
        let straight = RenderInfo::new(
            crate::effect::surface::Space::Linear,
            crate::effect::surface::Alpha::Straight,
        );
        let a = bb_ctx(
            "bbMemeMaker",
            straight,
            json!({"face": IMPACT_FALLBACK, "top": "", "bottom": ""}),
            &src,
        );
        assert_eq!(a.data, src.data);
    }

    /// The shadow is taken once from the combined overlay: overlapping blocks never darken
    /// past alpha 191; bottom strokes over top fills; off means no shadow at all.
    #[test]
    fn one_shadow_from_the_combined_overlay() {
        let src = solid(120, 100, G);
        let env = EffectEnv::whole(120, 100);
        let o = json!({"top": "O", "bottom": "O"});
        let out = meme(o.clone(), &src);
        let flat = meme(json!({"top": "O", "bottom": "O", "shadow": false}), &src);
        let v = values(o, &env);
        let [(_, top), (_, bottom)] = blocks(&v, &env);
        assert!(bottom.layout.lines[0].origin[1] < top.layout.lines[0].origin[1] + top.height);
        let (stroke, fill) = masks(&v, &env);
        let (top_fill, bottom_stroke, bottom_fill) = (
            coverage(&top.outlines(), None, 120, 100),
            coverage(&bottom.outlines(), Some(4.0), 120, 100),
            coverage(&bottom.outlines(), None, 120, 100),
        );
        let mut over = 0;
        for i in 0..120 * 100 {
            let (x, y) = ((i % 120) as u32, (i / 120) as u32);
            let p = px(&out, x, y);
            if stroke[i] == 0.0 && fill[i] == 0.0 {
                assert_eq!(px(&flat, x, y), G);
                let k = lin(p[0]) / lin(128);
                assert!(
                    k >= 1.0 - 191.0 / 255.0 - 0.01,
                    "double shadow at ({x}, {y}): {k}"
                );
            }
            if top_fill[i] == 1.0 && bottom_stroke[i] == 1.0 && bottom_fill[i] == 0.0 {
                assert!(near(px(&flat, x, y), [0, 0, 0, 255], TOL));
                over += 1;
            }
        }
        assert!(over > 10, "the blocks overlap");
    }

    /// The 4 px round stroke is constant across sizes; counters stay open; synthetic bold keeps
    /// its overlapping pieces filled (Winding).
    #[test]
    fn constant_stroke_and_open_counters() {
        for size in [10.0, 120.0] {
            let src = solid(400, 300, G);
            let out = meme(
                json!({"top": "IO", "bottom": "", "shadow": false, "topSize": size}),
                &src,
            );
            let env = EffectEnv::whole(400, 300);
            let [(_, run), _] = blocks(&values(json!({"top": "IO", "topSize": size}), &env), &env);
            let px_size = pt_to_px(size as f32) as f64;
            let mid = (0.5 + run.ascent - ANTON_CAP * px_size / 2.0) as u32;
            let x0 = (0..400).find(|&x| px(&out, x, mid) != G).unwrap();
            let x1 = (x0..400)
                .find(|&x| near(px(&out, x, mid), [255; 4], TOL))
                .unwrap();
            assert!((1..=3).contains(&(x1 - x0)), "{size}: rim {x0}..{x1}");
            if size == 120.0 {
                // The O's counter centre is untouched; its walls are white.
                let o = &run.layout.lines[0].glyphs[1];
                let cx = (run.layout.lines[0].origin[0] + o.origin[0] + o.advance / 2.0) as u32;
                assert_eq!(px(&out, cx, mid), G, "counter");
                let wall = (x1..cx).find(|&x| near(px(&out, x, mid), [255; 4], TOL));
                assert!(wall.is_some());
            }
        }
    }

    /// Sizes are points at 96 DPI per block; the top block hangs from the bounds top, the
    /// bottom grows upward by the sum of its line heights; one +0.5 shift.
    #[test]
    fn block_sizes_and_placement() {
        let env = EffectEnv::whole(800, 2000);
        let v = values(
            json!({"top": "A", "topSize": 10, "bottom": "A\nB\n\nC", "bottomSize": 255}),
            &env,
        );
        let [(_, top), (lines, bottom)] = blocks(&v, &env);
        assert_eq!(lines, ["A", "B", "", "C"]);
        let h = bottom.layout.lines[0].height;
        assert!((top.layout.lines[0].height * 25.5 - h).abs() < 0.01 * h);
        assert!(
            (bottom.height - bottom.layout.lines.iter().map(|l| l.height).sum::<f64>()).abs()
                < 1e-9
        );
        assert!((bottom.layout.lines[0].origin[1] - (2000.0 - bottom.height + 0.5)).abs() < 1e-9);
        assert_eq!(top.layout.lines[0].origin[1], 0.5);
        for (i, l) in bottom.layout.lines.iter().enumerate().skip(1) {
            let prev = &bottom.layout.lines[i - 1];
            assert!((l.origin[1] - prev.origin[1] - prev.height).abs() < 1e-9);
        }
    }

    /// The dialog contract: metadata, controls, tabs, help and font requirements.
    #[test]
    fn metadata_and_controls() {
        let d = def();
        assert_eq!(
            (d.id, d.icon, d.dialog_title, d.category),
            (
                "bbMemeMaker",
                "meme_maker.png",
                Some("Meme Maker"),
                Category::Effect("photo")
            )
        );
        let ui: &IndirectUi = d.indirect.unwrap();
        assert!(matches!(ui.help, Help::Text(l) if l == ["Meme Maker v6.0"]));
        let tabs = ui.tabs[0];
        assert_eq!(tabs.key, "tab");
        assert_eq!(
            tabs.tabs
                .iter()
                .map(|t| (t.caption, t.keys))
                .collect::<Vec<_>>(),
            [
                (
                    "\u{a0}Meme Text\u{a0}",
                    &["top", "topSize", "bottom", "bottomSize"][..]
                ),
                (
                    "\u{a0}Options\u{a0}",
                    &[
                        "textColor",
                        "outlineColor",
                        "swap",
                        "caps",
                        "centered",
                        "shadow"
                    ][..]
                ),
            ]
        );
        assert_eq!(ui.transient_keys, ["tab", "face"]);
        let keys: Vec<_> = d.controls.iter().map(Control::key).collect();
        assert_eq!(
            keys,
            [
                "tab",
                "face",
                "top",
                "topSize",
                "bottom",
                "bottomSize",
                "textColor",
                "outlineColor",
                "swap",
                "caps",
                "centered",
                "shadow"
            ]
        );
        for (key, default) in [
            ("top", "Enter top text here"),
            ("bottom", "Enter bottom text here"),
        ] {
            let c = d.controls.iter().find(|c| c.key() == key).unwrap();
            assert!(
                matches!(c, Control::Text { label: "", default: d, max_len: 32767, multiline: true, .. } if *d == default)
            );
        }
        for (key, label) in [
            ("topSize", "Top text font size"),
            ("bottomSize", "Bottom text font size"),
        ] {
            let c = d.controls.iter().find(|c| c.key() == key).unwrap();
            let Control::Range {
                label: l,
                min,
                max,
                default,
                step,
                exponential,
                keyboard,
                ..
            } = c
            else {
                panic!("{key}")
            };
            assert_eq!(
                (*l, *min, *max, *default, *step, *exponential),
                (label, 10.0, 255.0, 36.0, 1.0, false)
            );
            assert_eq!(
                (
                    keyboard.decimals,
                    keyboard.small,
                    keyboard.large,
                    keyboard.exponential
                ),
                (0, 1.0, 5.0, false)
            );
        }
        for (key, label, rgb) in [
            ("textColor", "Text color", 0xFFFFFF),
            ("outlineColor", "Outline color", 0),
        ] {
            let c = d.controls.iter().find(|c| c.key() == key).unwrap();
            assert!(
                matches!(c, Control::Color { label: l, default: ColorDefault::Rgb(v), .. } if *l == label && *v == rgb)
            );
            let cu = ui.control(key);
            assert!(!cu.reset && !cu.alpha && !cu.header_line);
        }
        for (key, label, default) in [
            ("swap", "Swap colors", false),
            ("caps", "All caps", true),
            ("centered", "Centered", true),
            ("shadow", "Add drop shadow", true),
        ] {
            let c = d.controls.iter().find(|c| c.key() == key).unwrap();
            assert!(
                matches!(c, Control::Checkbox { label: l, default: v, .. } if *l == label && *v == default)
            );
        }
        for key in &keys[2..] {
            assert!(!ui.control(key).header_line, "{key}");
        }
        // R157: Impact Bold first; a failed Impact moves this session to Anton, once.
        let mut v = crate::effect::defaults(&d);
        assert_eq!((ui.fonts)(&v), [(MEME_FONT.to_string(), FaceStyle::Bold)]);
        assert!((ui.font_fallback)(&mut v));
        assert_eq!(
            (ui.fonts)(&v),
            [(IMPACT_FALLBACK.to_string(), FaceStyle::Regular)]
        );
        assert!(!(ui.font_fallback)(&mut v));
        assert_eq!(
            (ui.fonts)(&Values::new()),
            [(MEME_FONT.to_string(), FaceStyle::Bold)]
        );
    }

    /// The tab is presentation only: it never changes pixels and, like the chosen face, is
    /// omitted from the remembered token and Repeat values.
    #[test]
    fn tab_and_face_are_transient() {
        let src = solid(200, 160, G);
        assert_eq!(
            meme(json!({"tab": 0}), &src).data,
            meme(json!({"tab": 1}), &src).data
        );
        let mut app = crate::testing::fresh_doc(40, 30, Some(4));
        let s = crate::effect::session::EffectSession::begin(&mut app, def()).unwrap();
        let mut v = values(json!({"tab": 1, "top": "a\r\nb"}), &s.env());
        v.insert("face".into(), json!(IMPACT_FALLBACK));
        assert!(s.commit(&mut app, v));
        let token = &app.effect_tokens["bbMemeMaker"];
        assert!(!token.contains_key("tab") && !token.contains_key("face"));
        assert_eq!(token["top"], json!("a\r\nb"));
        assert_eq!(app.last_effect_values.as_ref(), Some(token));
        // Drop Shadow+'s tab picks its pipeline and stays persistent.
        assert!(
            find("bbDropShadowPlus")
                .unwrap()
                .indirect
                .unwrap()
                .transient_keys
                .is_empty()
        );
    }

    include!("meme_casing_fixtures.rs");
}
